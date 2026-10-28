//! Outlines at a time: rectangles, ellipses and stars as paths, and the
//! trimmed part of a shape's outlines.

use kurbo::{ParamCurve, ParamCurveArclen};

use crate::{
    Direction, EllipseStart, Geometry, Path, PathEl, Point, RectStart, Star, StarKind, Trim,
    TrimMode, transform::segments,
};

/// The control point distance of a quarter circle of radius 1 drawn as a
/// cubic Bézier curve.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// How precisely trims measure lengths, in user units.
const ARCLEN_ACCURACY: f64 = 1e-3;

/// The outline of `geometry` at `time`.
pub fn geometry_path(geometry: &Geometry, time: f64) -> Path {
    match geometry {
        Geometry::Path { path } => path.sample(time),
        Geometry::Rect {
            center,
            size,
            radius,
            direction,
            start,
        } => rect_path(
            center.sample(time),
            size.sample(time),
            radius.sample(time),
            *direction,
            *start,
        ),
        Geometry::Ellipse {
            center,
            radius,
            direction,
            start,
        } => ellipse_path(center.sample(time), radius.sample(time), *direction, *start),
        Geometry::Star(star) => star_path(star, time),
    }
}

/// A rectangle's outline: `size` around `center`, corners rounded by
/// `radius`.
pub fn rect_path(
    center: Point,
    size: Point,
    radius: Point,
    direction: Direction,
    start: RectStart,
) -> Path {
    let (w, h) = (size.x.abs(), size.y.abs());
    let (x0, y0) = (center.x - w / 2.0, center.y - h / 2.0);
    let (x1, y1) = (x0 + w, y0 + h);
    let (rx, ry) = match start {
        // SVG clamps each radius to half its side.
        RectStart::TopLeft => (
            radius.x.max(0.0).min(w / 2.0),
            radius.y.max(0.0).min(h / 2.0),
        ),
        // Lottie's one roundness fits both sides.
        RectStart::TopRight => {
            let r = radius.x.max(0.0).min(w / 2.0).min(h / 2.0);
            (r, r)
        }
    };
    let (kx, ky) = (rx * (1.0 - KAPPA), ry * (1.0 - KAPPA));
    let p = Point::new;
    // The four corners clockwise from the top right: (edge end, arc).
    let corner_tr = (
        p(x1 - rx, y0),
        [p(x1 - kx, y0), p(x1, y0 + ky), p(x1, y0 + ry)],
    );
    let corner_br = (
        p(x1, y1 - ry),
        [p(x1, y1 - ky), p(x1 - kx, y1), p(x1 - rx, y1)],
    );
    let corner_bl = (
        p(x0 + rx, y1),
        [p(x0 + kx, y1), p(x0, y1 - ky), p(x0, y1 - ry)],
    );
    let corner_tl = (
        p(x0, y0 + ry),
        [p(x0, y0 + ky), p(x0 + kx, y0), p(x0 + rx, y0)],
    );
    let corners = match start {
        RectStart::TopLeft => [corner_tr, corner_br, corner_bl, corner_tl],
        RectStart::TopRight => [corner_br, corner_bl, corner_tl, corner_tr],
    };
    let begin = corners[3].1[2];
    let rounded = rx > 0.0 && ry > 0.0;
    // A move, four edges (and corners), a close.
    let mut path = Path(Vec::with_capacity(if rounded { 10 } else { 6 }));
    path.move_to(begin);
    for (edge_end, arc) in corners {
        path.line_to(edge_end);
        if rounded {
            path.cubic_to(arc[0], arc[1], arc[2]);
        }
    }
    path.close();
    match direction {
        Direction::Clockwise => path,
        Direction::CounterClockwise => reverse_closed(&path),
    }
}

/// An ellipse's outline: four cubic arcs.
pub fn ellipse_path(
    center: Point,
    radius: Point,
    direction: Direction,
    start: EllipseStart,
) -> Path {
    let (rx, ry) = (radius.x.abs(), radius.y.abs());
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let (cx, cy) = (center.x, center.y);
    let p = Point::new;
    let right = p(cx + rx, cy);
    let bottom = p(cx, cy + ry);
    let left = p(cx - rx, cy);
    let top = p(cx, cy - ry);
    // Clockwise quarter arcs: (end, control 1, control 2) from each start.
    let from_right = (bottom, p(cx + rx, cy + ky), p(cx + kx, cy + ry));
    let from_bottom = (left, p(cx - kx, cy + ry), p(cx - rx, cy + ky));
    let from_left = (top, p(cx - rx, cy - ky), p(cx - kx, cy - ry));
    let from_top = (right, p(cx + kx, cy - ry), p(cx + rx, cy - ky));
    let (begin, arcs) = match start {
        EllipseStart::Right => (right, [from_right, from_bottom, from_left, from_top]),
        EllipseStart::Top => (top, [from_top, from_right, from_bottom, from_left]),
    };
    let mut path = Path(Vec::with_capacity(6));
    path.move_to(begin);
    for (end, c1, c2) in arcs {
        path.cubic_to(c1, c2, end);
    }
    path.close();
    match direction {
        Direction::Clockwise => path,
        Direction::CounterClockwise => reverse_closed(&path),
    }
}

/// A star's or polygon's outline at `time`, as Lottie draws its polystar:
/// the first point at the top (before rotation), rounded points pulled
/// along the outline's tangent.
pub fn star_path(star: &Star, time: f64) -> Path {
    let points = star.points.sample(time).floor().max(0.0);
    if points < 1.0 {
        return Path::new();
    }
    let center = star.center.sample(time);
    let outer = star.outer_radius.sample(time);
    let inner = star.inner_radius.sample(time);
    let outer_round = star.outer_roundness.sample(time);
    let inner_round = star.inner_roundness.sample(time);
    let dir = match star.direction {
        Direction::Clockwise => 1.0,
        Direction::CounterClockwise => -1.0,
    };
    let (count, step) = match star.kind {
        StarKind::Star => (points as usize * 2, std::f64::consts::PI / points),
        StarKind::Polygon => (points as usize, 2.0 * std::f64::consts::PI / points),
    };
    let mut angle = star.rotation.sample(time).to_radians() - std::f64::consts::FRAC_PI_2;
    // (vertex, in tangent, out tangent) of each point.
    let mut vertices = Vec::with_capacity(count);
    let mut long = true;
    for _ in 0..count {
        let (radius, roundness) = match (star.kind, long) {
            (StarKind::Polygon, _) | (StarKind::Star, true) => (outer, outer_round),
            (StarKind::Star, false) => (inner, inner_round),
        };
        let perimeter = 2.0 * std::f64::consts::PI * radius / (count as f64 * 4.0);
        let (x, y) = (radius * angle.cos(), radius * angle.sin());
        let len = x.hypot(y);
        let (ox, oy) = if len == 0.0 {
            (0.0, 0.0)
        } else {
            (y / len, -x / len)
        };
        let pull = perimeter * roundness * dir;
        let v = Point::new(center.x + x, center.y + y);
        vertices.push((
            v,
            Point::new(v.x + ox * pull, v.y + oy * pull),
            Point::new(v.x - ox * pull, v.y - oy * pull),
        ));
        long = !long;
        angle += step * dir;
    }
    let Some(&(first, _, _)) = vertices.first() else {
        return Path::new();
    };
    let mut path = Path(Vec::with_capacity(vertices.len() + 2));
    path.move_to(first);
    for i in 0..vertices.len() {
        let (_, _, out) = vertices[i];
        let (next, into, _) = vertices[(i + 1) % vertices.len()];
        path.cubic_to(out, into, next);
    }
    path.close();
    path
}

/// The closed path `path` drawn the other way round from the same start.
fn reverse_closed(path: &Path) -> Path {
    let els = path.elements();
    let Some(PathEl::MoveTo(start)) = els.first().copied() else {
        return path.clone();
    };
    // Each drawing element with the point it starts from.
    let mut drawn = Vec::new();
    let mut current = start;
    for el in &els[1..] {
        match *el {
            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CubicTo(_, _, p) => {
                drawn.push((current, *el));
                current = p;
            }
            PathEl::MoveTo(_) | PathEl::Close => {}
        }
    }
    let mut out = Path::new();
    out.move_to(start);
    for (from, el) in drawn.into_iter().rev() {
        match el {
            PathEl::LineTo(_) => out.line_to(from),
            PathEl::QuadTo(c, _) => out.quad_to(c, from),
            PathEl::CubicTo(c1, c2, _) => out.cubic_to(c2, c1, from),
            PathEl::MoveTo(_) | PathEl::Close => {}
        }
    }
    out.close();
    out
}

/// The part of `paths` that `trim` draws at `time`, as one path.
pub fn trim_paths(paths: &[Path], trim: &Trim, time: f64) -> Path {
    let (start, end) = (trim.start.sample(time), trim.end.sample(time));
    let offset = trim.offset.sample(time);
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let (start, end) = (start.clamp(0.0, 1.0) + offset, end.clamp(0.0, 1.0) + offset);
    let mut out = Path::new();
    if end - start >= 1.0 {
        for p in paths {
            out.extend(p);
        }
        return out;
    }
    if end <= start {
        return out;
    }
    let shift = start.floor();
    let (start, end) = (start - shift, end - shift);
    let ranges: &[(f64, f64)] = if end <= 1.0 {
        &[(start, end)]
    } else {
        &[(start, 1.0), (0.0, end - 1.0)]
    };
    match trim.mode {
        TrimMode::EachPath => {
            for p in paths {
                let segs = segments(p);
                for &(a, b) in ranges {
                    cut(&segs, a, b, &mut out);
                }
            }
        }
        TrimMode::Combined => {
            let segs: Vec<kurbo::PathSeg> = paths.iter().flat_map(segments).collect();
            for &(a, b) in ranges {
                cut(&segs, a, b, &mut out);
            }
        }
    }
    out
}

/// Appends the part of `segs` from fraction `from` to `to` of their total
/// length to `out`, starting a subpath wherever the pieces do not join.
fn cut(segs: &[kurbo::PathSeg], from: f64, to: f64, out: &mut Path) {
    let lengths: Vec<f64> = segs.iter().map(|s| s.arclen(ARCLEN_ACCURACY)).collect();
    let total: f64 = lengths.iter().sum();
    if total <= 0.0 {
        return;
    }
    let (a, b) = (from * total, to * total);
    let mut at = 0.0;
    let mut last: Option<kurbo::Point> = None;
    for (seg, len) in segs.iter().zip(&lengths) {
        let (s0, s1) = (at, at + len);
        at = s1;
        if s1 <= a || s0 >= b || *len <= 0.0 {
            continue;
        }
        let t0 = if a > s0 {
            seg.inv_arclen(a - s0, ARCLEN_ACCURACY)
        } else {
            0.0
        };
        let t1 = if b < s1 {
            seg.inv_arclen(b - s0, ARCLEN_ACCURACY)
        } else {
            1.0
        };
        let piece = seg.subsegment(t0..t1);
        let begin = piece.start();
        let joined = last.is_some_and(|p| (p - begin).hypot() < 1e-9);
        if !joined {
            out.move_to(Point::new(begin.x, begin.y));
        }
        let pt = |p: kurbo::Point| Point::new(p.x, p.y);
        match piece {
            kurbo::PathSeg::Line(l) => out.line_to(pt(l.p1)),
            kurbo::PathSeg::Quad(q) => out.quad_to(pt(q.p1), pt(q.p2)),
            kurbo::PathSeg::Cubic(c) => out.cubic_to(pt(c.p1), pt(c.p2), pt(c.p3)),
        }
        last = Some(piece.end());
    }
}

/// The total length of `path`.
pub fn path_length(path: &Path) -> f64 {
    segments(path)
        .iter()
        .map(|s| s.arclen(ARCLEN_ACCURACY))
        .sum()
}
