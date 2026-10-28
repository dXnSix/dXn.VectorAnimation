//! Lottie's shape modifiers as geometry at one moment: merge paths
//! (boolean operations on flattened outlines, in the order skottie merges
//! them), round corners, pucker and bloat, zig zag and offset path (as
//! lottie-web modifies outlines), and twist.

use dxn_vector_animation::{Path, PathEl, Point};
use i_overlay::{
    core::{fill_rule::FillRule, overlay_rule::OverlayRule},
    float::single::SingleFloatOverlay,
};
use kurbo::{CubicBez, ParamCurve, ParamCurveDeriv};

/// How far along to its corner lottie-web pulls the handles of a rounded
/// corner.
const ROUND_CORNER: f64 = 0.5519;

/// The most points one outline holds after a modifier, at one moment.
pub(crate) const MAX_POINTS: usize = 200_000;

/// How far a flattened curve strays from the curve, as a share of the
/// diagonal of the outlines' bounds: a circle 2000 across by 0.15 at
/// most.
const FLATNESS: f64 = 5e-5;

/// Coordinates beyond this are left unmerged.
const FAR: f64 = 1e12;

/// The most points of the flattened outlines one merge takes.
const MAX_MERGE_POINTS: usize = 20_000;

/// The most pairs of segments whose boxes overlap (an upper bound of
/// their crossings) one merge takes.
const MAX_MERGE_PAIRS: usize = 1_000_000;

/// One point of an outline as Lottie writes shapes: the point and its in
/// and out handles (absolute; a handle on its point is none).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Vertex {
    pub(crate) at: Point,
    pub(crate) into: Point,
    pub(crate) out: Point,
}

impl Vertex {
    /// A point without handles.
    fn corner(at: Point) -> Vertex {
        Vertex {
            at,
            into: at,
            out: at,
        }
    }
}

/// One subpath as vertices.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Contour {
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) closed: bool,
}

/// The subpaths of `path` as vertices. A closed subpath whose last point
/// returns to its first (as Lottie's outlines do) has that point once.
pub(crate) fn contours(path: &Path) -> Vec<Contour> {
    let mut out = Vec::new();
    let mut current: Option<Contour> = None;
    // Where the pen is, and where its subpath started.
    let (mut pen, mut start) = (Point::ZERO, Point::ZERO);
    // The open subpath, started at the pen when there is none (drawing on
    // after a close, or before any move).
    fn open<'c>(
        current: &'c mut Option<Contour>,
        start: &mut Point,
        pen: Point,
    ) -> &'c mut Contour {
        current.get_or_insert_with(|| {
            *start = pen;
            Contour {
                vertices: vec![Vertex::corner(pen)],
                closed: false,
            }
        })
    }
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                out.extend(current.take());
                current = Some(Contour {
                    vertices: vec![Vertex::corner(p)],
                    closed: false,
                });
                (pen, start) = (p, p);
            }
            PathEl::LineTo(p) => {
                open(&mut current, &mut start, pen)
                    .vertices
                    .push(Vertex::corner(p));
                pen = p;
            }
            PathEl::QuadTo(q, p) => {
                let c = open(&mut current, &mut start, pen);
                if let Some(last) = c.vertices.last_mut() {
                    last.out = last.at + (q - last.at) * (2.0 / 3.0);
                }
                c.vertices.push(Vertex {
                    at: p,
                    into: p + (q - p) * (2.0 / 3.0),
                    out: p,
                });
                pen = p;
            }
            PathEl::CubicTo(c1, c2, p) => {
                let c = open(&mut current, &mut start, pen);
                if let Some(last) = c.vertices.last_mut() {
                    last.out = c1;
                }
                c.vertices.push(Vertex {
                    at: p,
                    into: c2,
                    out: p,
                });
                pen = p;
            }
            PathEl::Close => {
                if let Some(mut c) = current.take() {
                    c.closed = true;
                    let first = c.vertices[0].at;
                    if c.vertices.len() > 1
                        && let Some(last) = c.vertices.last().copied()
                        && same(last.at, first)
                    {
                        c.vertices.pop();
                        c.vertices[0].into = last.into;
                    }
                    out.push(c);
                }
                pen = start;
            }
        }
    }
    out.extend(current);
    out
}

/// The path of `contours`: lines where a segment has no handles, cubic
/// curves elsewhere.
pub(crate) fn path_of(contours: &[Contour]) -> Path {
    let mut path = Path::new();
    for c in contours {
        let Some(first) = c.vertices.first().copied() else {
            continue;
        };
        path.move_to(first.at);
        for w in c.vertices.windows(2) {
            segment_to(&mut path, &w[0], &w[1]);
        }
        if c.closed {
            if let Some(last) = c.vertices.last().copied()
                && c.vertices.len() > 1
                && !(last.at == first.at && last.out == last.at && first.into == first.at)
            {
                segment_to(&mut path, &last, &first);
            }
            path.close();
        }
    }
    path
}

fn segment_to(path: &mut Path, a: &Vertex, b: &Vertex) {
    if a.out == a.at && b.into == b.at {
        path.line_to(b.at);
    } else {
        path.cubic_to(a.out, b.into, b.at);
    }
}

/// Whether two points are the same, but for rounding.
fn same(a: Point, b: Point) -> bool {
    let scale = 1.0 + a.x.abs().max(a.y.abs());
    (a.x - b.x).abs() <= 1e-9 * scale && (a.y - b.y).abs() <= 1e-9 * scale
}

fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

fn cross(a: Point, b: Point) -> f64 {
    a.x * b.y - a.y * b.x
}

fn length(a: Point) -> f64 {
    a.x.hypot(a.y)
}

fn kp(p: Point) -> kurbo::Point {
    kurbo::Point::new(p.x, p.y)
}

fn dp(p: kurbo::Point) -> Point {
    Point::new(p.x, p.y)
}

/// Round corners (`rd`), as lottie-web rounds them: each point without
/// handles (but an open outline's ends) becomes two, `radius` towards its
/// neighbours (at most halfway), joined by a curve about the corner.
pub(crate) fn round_corners(c: &Contour, radius: f64) -> Contour {
    let n = c.vertices.len();
    let mut out = Vec::with_capacity(n * 2);
    for (k, v) in c.vertices.iter().enumerate() {
        let sharp = v.into == v.at && v.out == v.at;
        if !sharp || (!c.closed && (k == 0 || k == n - 1)) {
            out.push(*v);
            continue;
        }
        let toward = |other: Point| -> Point {
            let d = v.at.distance(other);
            let share = if d > 0.0 {
                (d / 2.0).min(radius) / d
            } else {
                0.0
            };
            v.at + (other - v.at) * share
        };
        let a = toward(c.vertices[(k + n - 1) % n].at);
        let b = toward(c.vertices[(k + 1) % n].at);
        out.push(Vertex {
            at: a,
            into: a,
            out: a + (v.at - a) * ROUND_CORNER,
        });
        out.push(Vertex {
            at: b,
            into: b + (v.at - b) * ROUND_CORNER,
            out: b,
        });
    }
    Contour {
        vertices: out,
        closed: c.closed,
    }
}

/// Pucker and bloat (`pb`), as lottie-web does it: points move `percent`
/// of the way to the outline's center (the mean of its points), handles
/// as far away from it.
pub(crate) fn pucker_bloat(c: &Contour, percent: f64) -> Contour {
    let n = c.vertices.len();
    if n == 0 {
        return c.clone();
    }
    let sum = c.vertices.iter().fold(Point::ZERO, |s, v| s + v.at);
    let center = sum * (1.0 / n as f64);
    let p = percent / 100.0;
    Contour {
        vertices: c
            .vertices
            .iter()
            .map(|v| Vertex {
                at: v.at + (center - v.at) * p,
                into: v.into + (center - v.into) * -p,
                out: v.out + (center - v.out) * -p,
            })
            .collect(),
        closed: c.closed,
    }
}

/// The segment from vertex `i` to the next as a cubic, handles on their
/// points moved to the thirds (lottie-web's linearized segment).
fn shape_segment(c: &Contour, i: usize) -> CubicBez {
    let n = c.vertices.len();
    let (a, b) = (c.vertices[i], c.vertices[(i + 1) % n]);
    linearized(a.at, a.out, b.into, b.at)
}

/// The same segment run backwards.
fn shape_segment_inverted(c: &Contour, i: usize) -> CubicBez {
    let n = c.vertices.len();
    let (a, b) = (c.vertices[i], c.vertices[(i + 1) % n]);
    linearized(b.at, b.into, a.out, a.at)
}

fn linearized(p0: Point, p1: Point, p2: Point, p3: Point) -> CubicBez {
    let p1 = if p1 == p0 { p0.lerp(p3, 1.0 / 3.0) } else { p1 };
    let p2 = if p2 == p3 { p0.lerp(p3, 2.0 / 3.0) } else { p2 };
    CubicBez::new(kp(p0), kp(p1), kp(p2), kp(p3))
}

/// The number of segments of `c`.
fn segment_count(c: &Contour) -> usize {
    let n = c.vertices.len();
    if c.closed { n } else { n.saturating_sub(1) }
}

/// Zig zag (`zz`), as lottie-web draws it: `ridges` points along each
/// segment and one at each point, `size` off the outline to either side
/// in turn (outwards first on a clockwise outline's segments); smooth
/// points have handles along the outline.
pub(crate) fn zig_zag(c: &Contour, size: f64, ridges: usize, smooth: bool) -> Contour {
    let n = c.vertices.len();
    let count = segment_count(c);
    let mut out = Vec::new();
    if count == 0 {
        return Contour {
            vertices: out,
            closed: c.closed,
        };
    }
    let share = (ridges + 1) as f64 * 2.0;
    // A point `size` along the normal at `angle` (lottie-web's angles:
    // `(cos, -sin)` is the offset), with handles across it.
    let mut set = |at: Point, angle: f64, direction: f64, out_len: f64, in_len: f64| {
        let p = Point::new(
            at.x + angle.cos() * direction * size,
            at.y - angle.sin() * direction * size,
        );
        let (ao, ai) = (
            angle - std::f64::consts::FRAC_PI_2,
            angle + std::f64::consts::FRAC_PI_2,
        );
        out.push(Vertex {
            at: p,
            out: Point::new(p.x + ao.cos() * out_len, p.y - ao.sin() * out_len),
            into: Point::new(p.x + ai.cos() * in_len, p.y - ai.sin() * in_len),
        });
    };
    let corner = |set: &mut dyn FnMut(Point, f64, f64, f64, f64), cur: usize, direction: f64| {
        let point = c.vertices[cur % n].at;
        // An open outline's ends go across their own segment (lottie-web
        // wraps round to the other end).
        let prev = match (cur, c.closed) {
            (0, false) => point,
            (0, true) => c.vertices[n - 1].at,
            _ => c.vertices[cur - 1].at,
        };
        let next = if !c.closed && cur + 1 >= n {
            point
        } else {
            c.vertices[(cur + 1) % n].at
        };
        let v = next - prev;
        // lottie-web's projecting angle: the normal of the line from the
        // point before to the one after.
        let angle = -(-v.x).atan2(v.y);
        let (to_prev, to_next) = if smooth {
            (point.distance(prev), point.distance(next))
        } else {
            (0.0, 0.0)
        };
        set(point, angle, direction, to_next / share, to_prev / share);
    };
    let mut direction = -1.0;
    corner(&mut set, 0, direction);
    for i in 0..count {
        let segment = shape_segment(c, i);
        let length = if smooth {
            segment.p0.distance(segment.p3)
        } else {
            0.0
        };
        direction = -direction;
        for k in 0..ridges {
            let t = (k + 1) as f64 / (ridges + 1) as f64;
            let d = segment.deriv().eval(t);
            let angle = d.x.atan2(d.y);
            set(
                dp(segment.eval(t)),
                angle,
                direction,
                length / share,
                length / share,
            );
            direction = -direction;
        }
        corner(&mut set, i + 1, direction);
    }
    Contour {
        vertices: out,
        closed: c.closed,
    }
}

/// How a modifier joins offset segments at corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Join {
    Miter,
    Round,
    Bevel,
}

/// Offset path (`op`), as lottie-web offsets: each segment's control
/// polygon moved `amount` to its left (outwards on a clockwise outline),
/// split where the curve turns the other way, overlaps of neighbouring
/// segments cut off, and corners joined. An open outline goes round both
/// sides. Round joins are circular arcs about the corner, and a miter is
/// kept while it reaches at most `miter_limit` times the amount from its
/// corner (as strokes keep theirs).
pub(crate) fn offset(c: &Contour, amount: f64, join: Join, miter_limit: f64) -> Contour {
    let count = segment_count(c);
    // Each segment's offset pieces, and the outline's point it starts at.
    let mut multi: Vec<(Vec<CubicBez>, Point)> = Vec::new();
    for i in 0..count {
        let segment = shape_segment(c, i);
        if !degenerate(segment) {
            multi.push((offset_split(segment, amount), c.vertices[i].at));
        }
    }
    if !c.closed {
        for i in (0..count).rev() {
            let segment = shape_segment_inverted(c, i);
            if !degenerate(segment) {
                multi.push((offset_split(segment, amount), c.vertices[i + 1].at));
            }
        }
    }
    prune_intersections(&mut multi);
    let mut out: Vec<Vertex> = Vec::new();
    let mut last_point: Option<Point> = None;
    let mut last_segment: Option<CubicBez> = None;
    for (pieces, corner) in &multi {
        let Some(first) = pieces.first() else {
            continue;
        };
        if let Some(previous) = last_segment {
            last_point = Some(join_lines(
                &mut out,
                previous,
                *first,
                *corner,
                join,
                miter_limit * amount.abs(),
            ));
        }
        last_segment = pieces.last().copied();
        for piece in pieces {
            let start = dp(piece.p0);
            match out.last_mut() {
                Some(last) if last_point.is_some_and(|p| same(p, start)) => last.out = dp(piece.p1),
                _ => out.push(Vertex {
                    at: start,
                    into: start,
                    out: dp(piece.p1),
                }),
            }
            let end = dp(piece.p3);
            out.push(Vertex {
                at: end,
                into: dp(piece.p2),
                out: end,
            });
            last_point = Some(end);
        }
    }
    if let (Some(previous), Some((pieces, corner))) = (last_segment, multi.first())
        && let Some(first) = pieces.first()
    {
        join_lines(
            &mut out,
            previous,
            *first,
            *corner,
            join,
            miter_limit * amount.abs(),
        );
    }
    Contour {
        vertices: out,
        closed: c.closed,
    }
}

fn degenerate(s: CubicBez) -> bool {
    s.p0 == s.p1 && s.p1 == s.p2 && s.p2 == s.p3
}

/// Where the lines through `a0`, `a1` and through `b0`, `b1` cross, if
/// they are not parallel.
fn line_intersection(a0: Point, a1: Point, b0: Point, b1: Point) -> Option<Point> {
    let (da, db) = (a1 - a0, b1 - b0);
    let denom = cross(da, db);
    if denom.abs() <= 1e-9 * length(da) * length(db) || denom == 0.0 {
        return None;
    }
    let t = cross(b0 - a0, db) / denom;
    let p = a0 + da * t;
    (p.x.is_finite() && p.y.is_finite()).then_some(p)
}

/// One cubic offset through its control polygon: each leg moved
/// `amount` to its left (lottie-web's `linearOffset`: outwards on a
/// clockwise outline), the new control points where the moved legs
/// cross. A leg of no length borrows its neighbour's direction.
fn offset_segment(s: CubicBez, amount: f64) -> CubicBez {
    let (p0, p1, p2, p3) = (dp(s.p0), dp(s.p1), dp(s.p2), dp(s.p3));
    let normal = |a: Point, b: Point| -> Option<Point> {
        let d = b - a;
        let len = length(d);
        (len > 1e-12 * (1.0 + length(a).max(length(b))))
            .then(|| Point::new(d.y / len, -d.x / len) * amount)
    };
    let (n0, n1, n2) = (normal(p0, p1), normal(p1, p2), normal(p2, p3));
    let Some(any) = n0.or(n1).or(n2) else {
        return s;
    };
    let n0 = n0.or(n1).unwrap_or(any);
    let n2 = n2.or(n1).unwrap_or(any);
    // Legs nearly in line cross far away: keep the moved point then.
    let reach = 4.0 * (p0.distance(p1) + p1.distance(p2) + p2.distance(p3) + amount.abs());
    let near = |x: Option<Point>, fallback: Point| match x {
        Some(x) if x.distance(fallback) <= reach => x,
        _ => fallback,
    };
    let (q1, q2) = match n1 {
        Some(n1) => (
            near(
                line_intersection(p0 + n0, p1 + n0, p1 + n1, p2 + n1),
                p1 + n0,
            ),
            near(
                line_intersection(p2 + n2, p3 + n2, p1 + n1, p2 + n1),
                p2 + n2,
            ),
        ),
        None => (p1 + n0, p2 + n2),
    };
    let (start, end) = (p0 + n0, p3 + n2);
    let (q1, q2) = fit_middle(s, amount, start, q1, q2, end);
    CubicBez::new(kp(start), kp(q1), kp(q2), kp(end))
}

/// The handles of the offset curve from `start` to `end`, lengths changed
/// (directions kept) so that it passes through the offset of `s`'s
/// middle, as a circle's offset is a circle.
fn fit_middle(
    s: CubicBez,
    amount: f64,
    start: Point,
    q1: Point,
    q2: Point,
    end: Point,
) -> (Point, Point) {
    let d = s.deriv().eval(0.5).to_vec2();
    let len = d.hypot();
    if len <= 1e-12 {
        return (q1, q2);
    }
    let target = dp(s.eval(0.5)) + Point::new(d.y / len, -d.x / len) * amount;
    let (u, v) = (q1 - start, q2 - end);
    let det = cross(u, v);
    if det.abs() <= 1e-9 * length(u) * length(v) || det == 0.0 {
        return (q1, q2);
    }
    // (start + end) / 2 + 3/8 (α u + β v) = target.
    let rhs = (target - (start + end) * 0.5) * (8.0 / 3.0);
    let alpha = cross(rhs, v) / det;
    let beta = cross(u, rhs) / det;
    if alpha > 0.0 && beta > 0.0 && alpha < 4.0 && beta < 4.0 {
        (start + u * alpha, end + v * beta)
    } else {
        (q1, q2)
    }
}

/// Where a cubic turns the other way (0 to 2 parameters, ascending).
fn inflections(s: CubicBez) -> Vec<f64> {
    let (p0, p1, p2, p3) = (dp(s.p0), dp(s.p1), dp(s.p2), dp(s.p3));
    let a = p3 - p0 + (p1 - p2) * 3.0;
    let b = (p0 - p1 * 2.0 + p2) * 3.0;
    let c = (p1 - p0) * 3.0;
    // B' × B'' = −6 (a×b) t² + 6 (c×a) t + 2 (c×b).
    let (qa, qb, qc) = (-6.0 * cross(a, b), 6.0 * cross(c, a), 2.0 * cross(c, b));
    let scale = length(a).max(length(b)).max(length(c)).powi(2);
    let mut roots = Vec::new();
    if qa.abs() <= 1e-12 * scale {
        if qb.abs() > 1e-12 * scale {
            roots.push(-qc / qb);
        }
    } else {
        let disc = qb * qb - 4.0 * qa * qc;
        if disc >= 0.0 {
            let r = disc.sqrt();
            roots.push((-qb - r) / (2.0 * qa));
            roots.push((-qb + r) / (2.0 * qa));
        }
    }
    roots.retain(|t| *t > 1e-6 && *t < 1.0 - 1e-6);
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
    roots
}

/// A cubic offset in pieces that each turn one way.
fn offset_split(s: CubicBez, amount: f64) -> Vec<CubicBez> {
    let mut cuts = inflections(s);
    cuts.push(1.0);
    let mut out = Vec::with_capacity(cuts.len());
    let mut from = 0.0;
    for to in cuts {
        out.push(offset_segment(s.subsegment(from..to), amount));
        from = to;
    }
    out
}

/// Cuts off where each offset segment's end overlaps the next one's start
/// (and the last's the first's).
fn prune_intersections(multi: &mut [(Vec<CubicBez>, Point)]) {
    let n = multi.len();
    let prune = |multi: &mut [(Vec<CubicBez>, Point)], a: usize, b: usize| {
        let (Some(&sa), Some(&sb)) = (multi[a].0.last(), multi[b].0.first()) else {
            return;
        };
        if let Some((ta, tb)) = intersection(sa, sb) {
            if let Some(last) = multi[a].0.last_mut() {
                *last = sa.subsegment(0.0..ta);
            }
            if let Some(first) = multi[b].0.first_mut() {
                *first = sb.subsegment(tb..1.0);
            }
        }
    };
    for i in 1..n {
        prune(multi, i - 1, i);
    }
    if n > 1 {
        prune(multi, n - 1, 0);
    }
}

/// Where `a` crosses `b` (parameters on each), found by subdividing both;
/// `a`'s end meeting `b`'s start does not count. The crossing nearest
/// `a`'s start, as lottie-web takes the first it finds.
fn intersection(a: CubicBez, b: CubicBez) -> Option<(f64, f64)> {
    use kurbo::ParamCurveExtrema;
    let size = a
        .bounding_box()
        .union(b.bounding_box())
        .size()
        .to_vec2()
        .hypot();
    let tolerance = (size * 1e-4).max(1e-9);
    let joint = (a.p3 - b.p0).hypot() <= tolerance;
    let hull = |c: &CubicBez| {
        let xs = [c.p0.x, c.p1.x, c.p2.x, c.p3.x];
        let ys = [c.p0.y, c.p1.y, c.p2.y, c.p3.y];
        let min = |v: [f64; 4]| v.iter().copied().fold(f64::INFINITY, f64::min);
        let max = |v: [f64; 4]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (min(xs), min(ys), max(xs), max(ys))
    };
    let mut found: Option<(f64, f64)> = None;
    let mut budget = 8192_usize;
    let mut stack = vec![(0.0, 1.0, 0.0, 1.0, 0_u32)];
    while let Some((a0, a1, b0, b1, depth)) = stack.pop() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let (pa, pb) = (a.subsegment(a0..a1), b.subsegment(b0..b1));
        let (ha, hb) = (hull(&pa), hull(&pb));
        if ha.2 < hb.0 || hb.2 < ha.0 || ha.3 < hb.1 || hb.3 < ha.1 {
            continue;
        }
        let small = |h: (f64, f64, f64, f64)| h.2 - h.0 <= tolerance && h.3 - h.1 <= tolerance;
        if (small(ha) && small(hb)) || depth >= 30 {
            let (ta, tb) = refine(a, b, (a0 + a1) / 2.0, (b0 + b1) / 2.0, tolerance);
            let at = a.eval(ta);
            if joint && (at - a.p3).hypot() <= 8.0 * tolerance {
                continue;
            }
            if found.is_none_or(|f| ta < f.0) {
                found = Some((ta, tb));
            }
            continue;
        }
        if found.is_some_and(|f| a0 > f.0) {
            continue;
        }
        let (am, bm) = ((a0 + a1) / 2.0, (b0 + b1) / 2.0);
        // Popped last first: `a`'s first half first.
        stack.push((am, a1, bm, b1, depth + 1));
        stack.push((am, a1, b0, bm, depth + 1));
        stack.push((a0, am, bm, b1, depth + 1));
        stack.push((a0, am, b0, bm, depth + 1));
    }
    found
}

/// The crossing of `a` and `b` near parameters `ta`, `tb`, made exact by
/// Newton's steps (kept as found when they stray).
fn refine(a: CubicBez, b: CubicBez, ta: f64, tb: f64, tolerance: f64) -> (f64, f64) {
    let (da, db) = (a.deriv(), b.deriv());
    let (mut x, mut y) = (ta, tb);
    for _ in 0..16 {
        let f = a.eval(x) - b.eval(y);
        if f.hypot() <= 1e-13 * (1.0 + tolerance * 1e4) {
            break;
        }
        let (u, v) = (da.eval(x).to_vec2(), db.eval(y).to_vec2());
        // f + u·δx − v·δy = 0.
        let det = v.x * u.y - u.x * v.y;
        if det.abs() <= 1e-18 {
            return (ta, tb);
        }
        let dx = (f.x * v.y - v.x * f.y) / det;
        let dy = (u.y * f.x - u.x * f.y) / det;
        x = (x + dx).clamp(0.0, 1.0);
        y = (y + dy).clamp(0.0, 1.0);
    }
    if (a.eval(x) - b.eval(y)).hypot() <= tolerance {
        (x, y)
    } else {
        (ta, tb)
    }
}

/// Joins offset segment `first` to `second` at the outline's point
/// `corner`, adding the join's points to `out`; returns where the join
/// ends.
fn join_lines(
    out: &mut Vec<Vertex>,
    first: CubicBez,
    second: CubicBez,
    corner: Point,
    join: Join,
    miter_reach: f64,
) -> Point {
    let (p0, p1) = (dp(first.p3), dp(second.p0));
    if join == Join::Bevel || same(p0, p1) {
        return p0;
    }
    match join {
        Join::Round => {
            arc(out, first, corner, p0, p1);
            p1
        }
        _ => {
            let t0 = if same(p0, dp(first.p2)) {
                dp(first.p0)
            } else {
                dp(first.p2)
            };
            let t1 = if same(p1, dp(second.p1)) {
                dp(second.p3)
            } else {
                dp(second.p1)
            };
            match line_intersection(t0, p0, p1, t1) {
                Some(x) if x.distance(corner) <= miter_reach => {
                    out.push(Vertex::corner(x));
                    x
                }
                _ => p0,
            }
        }
    }
}

/// A circular arc about `center` from `from` (where `before` ends, the
/// arc going on its way) to `to`, in quarter turns at most.
fn arc(out: &mut Vec<Vertex>, before: CubicBez, center: Point, from: Point, to: Point) {
    let (r0, r1) = (from - center, to - center);
    let radius = (length(r0) + length(r1)) / 2.0;
    if radius <= 0.0 {
        return;
    }
    let a0 = r0.y.atan2(r0.x);
    let mut sweep = r1.y.atan2(r1.x) - a0;
    let tau = std::f64::consts::TAU;
    sweep = sweep.rem_euclid(tau);
    if sweep > std::f64::consts::PI {
        sweep -= tau;
    }
    // Go on the way `before` goes.
    let d = before.deriv().eval(1.0).to_vec2();
    let heading = if d.hypot() > 1e-12 {
        Point::new(d.x, d.y)
    } else {
        dp(before.p3) - dp(before.p0)
    };
    let tangent = Point::new(-a0.sin(), a0.cos()) * sweep.signum();
    if dot(tangent, heading) < 0.0 {
        sweep -= tau * sweep.signum();
    }
    let pieces = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep / pieces as f64;
    let handle = 4.0 / 3.0 * (step / 4.0).tan() * radius;
    let at = |angle: f64| center + Point::new(angle.cos(), angle.sin()) * radius;
    let dir = |angle: f64| Point::new(-angle.sin(), angle.cos());
    for k in 0..pieces {
        let (s, e) = (a0 + step * k as f64, a0 + step * (k + 1) as f64);
        if let Some(last) = out.last_mut() {
            last.out = at(s) + dir(s) * handle;
        }
        let end = if k + 1 == pieces { to } else { at(e) };
        out.push(Vertex {
            at: end,
            into: end - dir(e) * handle,
            out: end,
        });
    }
}

/// Twist (`tw`): points turned about `center`, by `degrees` there and
/// less the farther they are, not at all at the outline's farthest point
/// (After Effects turns the middle more sharply than the edges; neither
/// lottie-web nor skottie twists). Segments are cut into pieces first so
/// straight lines bend.
pub(crate) fn twist(path: &Path, degrees: f64, center: Point) -> Path {
    if degrees == 0.0 || !degrees.is_finite() {
        return path.clone();
    }
    let contours = contours(path);
    let segments: Vec<Vec<CubicBez>> = contours
        .iter()
        .map(|c| (0..segment_count(c)).map(|i| shape_segment(c, i)).collect())
        .collect();
    let mut radius: f64 = 0.0;
    for c in &contours {
        for v in &c.vertices {
            radius = radius.max(v.at.distance(center));
        }
    }
    for s in segments.iter().flatten() {
        for k in 1..8 {
            radius = radius.max(dp(s.eval(f64::from(k) / 8.0)).distance(center));
        }
    }
    if radius <= 0.0 || !radius.is_finite() {
        return path.clone();
    }
    let turn_by = degrees.to_radians();
    let turn = |p: Point| -> Point {
        let d = p - center;
        let angle = turn_by * (1.0 - length(d) / radius).max(0.0);
        let (sin, cos) = angle.sin_cos();
        center + Point::new(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
    };
    let total: usize = segments.iter().map(Vec::len).sum();
    let most = (MAX_POINTS / (3 * total.max(1))).clamp(1, 64);
    let mut out = Vec::with_capacity(contours.len());
    for (c, segs) in contours.iter().zip(&segments) {
        let Some(first) = c.vertices.first() else {
            continue;
        };
        let mut vertices = vec![Vertex::corner(turn(first.at))];
        for (i, s) in segs.iter().enumerate() {
            let reach = s.p0.distance(s.p1) + s.p1.distance(s.p2) + s.p2.distance(s.p3);
            let pieces = ((8.0 * reach * turn_by.abs() / radius).ceil() as usize).clamp(1, most);
            for k in 0..pieces {
                let piece = s.subsegment(k as f64 / pieces as f64..(k + 1) as f64 / pieces as f64);
                if let Some(last) = vertices.last_mut() {
                    last.out = turn(dp(piece.p1));
                }
                let into = turn(dp(piece.p2));
                if c.closed && i + 1 == segs.len() && k + 1 == pieces {
                    vertices[0].into = into;
                } else {
                    let at = turn(dp(piece.p3));
                    vertices.push(Vertex { at, into, out: at });
                }
            }
        }
        out.push(Contour {
            vertices,
            closed: c.closed,
        });
    }
    path_of(&out)
}

/// How merge paths combines outlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MergeMode {
    /// All outlines as one path (the fill's rule decides).
    Merge,
    /// The union.
    Add,
    /// The first outline less the others.
    Subtract,
    /// Where they all are.
    Intersect,
    /// Where an odd number of them are.
    Exclude,
}

/// Merge paths (`mm`), as skottie merges: the first outline, then each
/// next one added, subtracted, intersected or excluded in turn, each
/// outline's inside by the non-zero rule. Curves are flattened for the
/// boolean modes; merge keeps them. Outlines that would take more than
/// the merge limits flattened even coarsely are merged as they are, and
/// `over` is set.
pub(crate) fn merge(paths: &[Path], mode: MergeMode, over: &std::cell::Cell<bool>) -> Path {
    let mut all = Path::new();
    for p in paths {
        all.extend(p);
    }
    let rule = match mode {
        MergeMode::Merge => return all,
        MergeMode::Add => OverlayRule::Union,
        MergeMode::Subtract => OverlayRule::Difference,
        MergeMode::Intersect => OverlayRule::Intersect,
        MergeMode::Exclude => OverlayRule::Xor,
    };
    if paths.len() < 2 {
        return all;
    }
    let Some(bounds) = all.bounds() else {
        return Path::new();
    };
    let reach = [
        bounds.x,
        bounds.y,
        bounds.x + bounds.width,
        bounds.y + bounds.height,
    ];
    if reach.iter().any(|v| !v.is_finite() || v.abs() > FAR) {
        return all;
    }
    let mut tolerance = (bounds.width.hypot(bounds.height) * FLATNESS).max(1e-9);
    let mut tries = 0;
    let polygons = loop {
        let polygons: Vec<Vec<Vec<[f64; 2]>>> =
            paths.iter().map(|p| flatten(p, tolerance)).collect();
        let count: usize = polygons.iter().flatten().map(Vec::len).sum();
        if count <= MAX_MERGE_POINTS && few_crossings(&polygons, MAX_MERGE_PAIRS) {
            break polygons;
        }
        tries += 1;
        if tries >= 6 {
            over.set(true);
            return all;
        }
        tolerance *= 4.0;
    };
    let mut result: Vec<Vec<[f64; 2]>> = polygons[0].clone();
    for clip in &polygons[1..] {
        if result.is_empty() && clip.is_empty() {
            continue;
        }
        result = result
            .overlay(clip, rule, FillRule::NonZero)
            .into_iter()
            .flatten()
            .collect();
    }
    let mut out = Path::new();
    for contour in result {
        let Some((first, rest)) = contour.split_first() else {
            continue;
        };
        out.move_to(Point::new(first[0], first[1]));
        for p in rest {
            out.line_to(Point::new(p[0], p[1]));
        }
        out.close();
    }
    out
}

/// Whether the edges of `polygons` have at most `most` pairs whose boxes
/// overlap (looking at no more than a hundred times as many pairs).
fn few_crossings(polygons: &[Vec<Vec<[f64; 2]>>], most: usize) -> bool {
    let mut boxes: Vec<[f64; 4]> =
        Vec::with_capacity(polygons.iter().flatten().map(Vec::len).sum());
    for contour in polygons.iter().flatten() {
        for (k, a) in contour.iter().enumerate() {
            let b = contour[(k + 1) % contour.len()];
            boxes.push([
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[0].max(b[0]),
                a[1].max(b[1]),
            ]);
        }
    }
    boxes.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let (mut pairs, mut looked) = (0_usize, 0_usize);
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            if b[0] > a[2] {
                break;
            }
            looked += 1;
            if b[1] <= a[3] && a[1] <= b[3] {
                pairs += 1;
            }
            if pairs > most || looked > most.saturating_mul(100) {
                return false;
            }
        }
    }
    true
}

/// `path` as polygons within `tolerance` of its curves (open subpaths
/// closed, as fills close them).
fn flatten(path: &Path, tolerance: f64) -> Vec<Vec<[f64; 2]>> {
    fn finish(current: &mut Vec<[f64; 2]>, out: &mut Vec<Vec<[f64; 2]>>) {
        if current.len() >= 3 {
            out.push(std::mem::take(current));
        } else {
            current.clear();
        }
    }
    let mut out = Vec::new();
    // At least a point an element.
    let mut current: Vec<[f64; 2]> = Vec::with_capacity(path.elements().len());
    let mut start = [0.0, 0.0];
    kurbo::flatten(path.to_kurbo(), tolerance, |el| match el {
        kurbo::PathEl::MoveTo(p) => {
            finish(&mut current, &mut out);
            start = [p.x, p.y];
            current.push(start);
        }
        kurbo::PathEl::LineTo(p) => {
            if current.is_empty() {
                current.push(start);
            }
            current.push([p.x, p.y]);
        }
        kurbo::PathEl::ClosePath => finish(&mut current, &mut out),
        _ => {}
    });
    finish(&mut current, &mut out);
    out
}
