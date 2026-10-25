//! The part of an outline between two lengths along it that move: how
//! trims and dash patterns whose lengths change are drawn (a canvas dash
//! pattern has lengths fixed when it is recorded).
//!
//! The part is a path whose points are graph values. Each segment's piece
//! runs from `clamp(from, a, b)` to `clamp(to, a, b)` along the outline:
//! the whole segment, a cut of it, or a point at `a` or `b` where the
//! segment lies outside the part, so the pieces always join. The point at
//! `a` or `b` is a lookup of the outline sampled by length. A cubic's piece
//! from `t0` to `t1` has the control points `C(t0) + (t1 − t0)·C′(t0)/3`
//! and `C(t1) − (t1 − t0)·C′(t1)/3`, with `t` and `C′/3` lookups of the
//! segment sampled by length (they clamp at its ends, so a piece outside
//! has `t1 = t0`).
//!
//! Pieces outside the part are points, and a stroker (Skia's among them)
//! turns the start cap after such points the wrong way. The path therefore
//! starts and ends with a line a hair long along the outline's direction
//! there, which gives each cap its direction.

use dxn_vector_animation::Path;
use kurbo::{CubicBez, ParamCurve, ParamCurveArclen, ParamCurveDeriv, PathEl, PathSeg, Point};

use crate::graph::{Graph, MathOp, Num};
use crate::plan::Op;

/// How closely lengths are measured.
const ACCURACY: f64 = 1e-3;

/// Segment ends this close join (as the reference renderer joins the
/// pieces of a trim).
const JOIN: f64 = 1e-9;

/// Cubics shorter than this are drawn as lines.
const SHORT: f64 = 0.05;

/// How far apart a cubic's samples are, at most, in lengths.
const SPACING: f64 = 4.0;

/// How far a cubic's tangent turns between samples, at most, in radians.
const TURN: f64 = 0.1;

/// How far apart the knots of a lookup are, at least.
const KNOT: f64 = 1e-3;

#[derive(Debug, Clone, Copy)]
enum Seg {
    Line(Point, Point),
    Cubic(CubicBez),
}

impl Seg {
    fn of(seg: PathSeg) -> Seg {
        match seg {
            PathSeg::Line(l) => Seg::Line(l.p0, l.p1),
            PathSeg::Quad(q) => Seg::Cubic(q.raise()),
            PathSeg::Cubic(c) => Seg::Cubic(c),
        }
    }

    fn start(self) -> Point {
        match self {
            Seg::Line(a, _) => a,
            Seg::Cubic(c) => c.p0,
        }
    }

    fn end(self) -> Point {
        match self {
            Seg::Line(_, b) => b,
            Seg::Cubic(c) => c.p3,
        }
    }

    fn length(self) -> f64 {
        match self {
            Seg::Line(a, b) => (b - a).hypot(),
            Seg::Cubic(c) => c.arclen(ACCURACY),
        }
    }
}

/// A segment at its place along a run.
#[derive(Debug, Clone)]
struct Piece {
    seg: Seg,
    from: f64,
    to: f64,
}

/// A segment sampled by length: where, at which `t`, the point and the
/// tangent `C′(t)/3`.
struct Sample {
    at: f64,
    t: f64,
    point: Point,
    tangent: (f64, f64),
}

impl Piece {
    /// Its samples, close enough that straight lines between them stay
    /// within `tolerance` of the curve.
    fn samples(&self, tolerance: f64) -> Vec<Sample> {
        let length = self.to - self.from;
        match self.seg {
            Seg::Line(a, b) => vec![
                Sample {
                    at: self.from,
                    t: 0.0,
                    point: a,
                    tangent: ((b.x - a.x) / 3.0, (b.y - a.y) / 3.0),
                },
                Sample {
                    at: self.to,
                    t: 1.0,
                    point: b,
                    tangent: ((b.x - a.x) / 3.0, (b.y - a.y) / 3.0),
                },
            ],
            Seg::Cubic(c) => {
                let d = c.deriv();
                // A chord of length s on a curve turning by θ along it
                // strays about s·θ/8 from it.
                let turn = turning(c);
                let n = (length * turn / (8.0 * tolerance.max(1e-6)))
                    .sqrt()
                    .max(turn / TURN)
                    .max(length / SPACING)
                    .ceil();
                let n = (n as usize).clamp(4, 256);
                (0..=n)
                    .map(|k| {
                        let s = length * k as f64 / n as f64;
                        let t = match k {
                            0 => 0.0,
                            k if k == n => 1.0,
                            _ => c.inv_arclen(s, ACCURACY),
                        };
                        let v = d.eval(t);
                        Sample {
                            at: if k == n { self.to } else { self.from + s },
                            t,
                            point: c.eval(t),
                            tangent: (v.x / 3.0, v.y / 3.0),
                        }
                    })
                    .collect()
            }
        }
    }
}

/// How far `c`'s tangent turns along it, in radians.
fn turning(c: CubicBez) -> f64 {
    const STEPS: usize = 32;
    let d = c.deriv();
    let mut total = 0.0;
    let mut last: Option<kurbo::Vec2> = None;
    for k in 0..=STEPS {
        let v = d.eval(k as f64 / STEPS as f64).to_vec2();
        if v.hypot() < 1e-12 {
            continue;
        }
        if let Some(l) = last {
            total += l.cross(v).atan2(l.dot(v)).abs();
        }
        last = Some(v);
    }
    total
}

/// Segments joined end to end, at their places along an outline (or along
/// several outlines one after another).
#[derive(Debug, Clone)]
pub struct Run {
    /// Where it starts.
    pub start: f64,
    /// Where it ends.
    pub end: f64,
    pieces: Vec<Piece>,
}

/// Adds `seg` at `at` (moving it on), to the last run when it joins it
/// (and `join`) or as a run of its own.
fn push(runs: &mut Vec<Run>, seg: PathSeg, at: &mut f64, join: bool) {
    let mut seg = Seg::of(seg);
    let length = seg.length();
    if length <= 0.0 || !length.is_finite() {
        return;
    }
    if let Seg::Cubic(c) = seg
        && length < SHORT
    {
        seg = Seg::Line(c.p0, c.p3);
    }
    let piece = Piece {
        seg,
        from: *at,
        to: *at + length,
    };
    *at = piece.to;
    match runs.last_mut() {
        Some(run)
            if join
                && run
                    .pieces
                    .last()
                    .is_some_and(|p| (p.seg.end() - seg.start()).hypot() < JOIN) =>
        {
            run.end = piece.to;
            run.pieces.push(piece);
        }
        _ => runs.push(Run {
            start: piece.from,
            end: piece.to,
            pieces: vec![piece],
        }),
    }
}

/// `paths` one after another, places counted along all of them (a
/// trim's), in runs of joined segments.
pub fn runs(paths: &[&Path]) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut at = 0.0;
    for path in paths {
        for seg in path.to_kurbo().segments() {
            push(&mut runs, seg, &mut at, true);
        }
    }
    runs
}

/// Each contour of `path` as a run from 0: where a dash pattern starts
/// again.
pub fn contours(path: &Path) -> Vec<Run> {
    let mut out = Vec::new();
    let mut contour = kurbo::BezPath::new();
    let mut flush = |contour: &kurbo::BezPath| {
        let mut runs = Vec::new();
        let mut at = 0.0;
        for seg in contour.segments() {
            // A contour is one run, whatever its segments' ends.
            push(&mut runs, seg, &mut at, false);
        }
        let mut pieces = runs.into_iter().flat_map(|r| r.pieces);
        if let Some(first) = pieces.next() {
            let mut run = Run {
                start: first.from,
                end: first.to,
                pieces: vec![first],
            };
            for p in pieces {
                run.end = p.to;
                run.pieces.push(p);
            }
            out.push(run);
        }
    };
    for el in path.to_kurbo().elements() {
        if matches!(el, PathEl::MoveTo(_)) && !contour.elements().is_empty() {
            flush(&contour);
            contour = kurbo::BezPath::new();
        }
        contour.push(*el);
    }
    flush(&contour);
    out
}

/// `runs` (along `length`) and the same again, `length` further on: what a
/// trim wrapping around draws on (its two pieces apart, as the reference
/// draws them).
pub fn doubled(runs: &[Run], length: f64) -> Vec<Run> {
    let shifted = runs.iter().map(|r| Run {
        start: r.start + length,
        end: r.end + length,
        pieces: r
            .pieces
            .iter()
            .map(|p| Piece {
                seg: p.seg,
                from: p.from + length,
                to: p.to + length,
            })
            .collect(),
    });
    runs.iter().cloned().chain(shifted).collect()
}

fn pair(p: Point) -> (Num, Num) {
    (Num::Const(p.x), Num::Const(p.y))
}

impl Run {
    /// Its path, whole and still (after a `BeginPath`).
    pub fn ops(&self) -> Vec<Op> {
        let mut ops = Vec::with_capacity(self.pieces.len() + 1);
        if let Some(first) = self.pieces.first() {
            let (x, y) = pair(first.seg.start());
            ops.push(Op::MoveTo(x, y));
        }
        for p in &self.pieces {
            ops.push(match p.seg {
                Seg::Line(_, b) => {
                    let (x, y) = pair(b);
                    Op::LineTo(x, y)
                }
                Seg::Cubic(c) => {
                    Op::CubicTo([c.p1.x, c.p1.y, c.p2.x, c.p2.y, c.p3.x, c.p3.y].map(Num::Const))
                }
            });
        }
        ops
    }

    /// The path of its part from `a` to `b` (`a <= b`; places along the
    /// run, clamped to it), after a `BeginPath`, within `tolerance` of the
    /// outline.
    pub fn span(&self, g: &mut Graph, a: Num, b: Num, tolerance: f64) -> Vec<Op> {
        let samples: Vec<Vec<Sample>> = self.pieces.iter().map(|p| p.samples(tolerance)).collect();
        // The run sampled by length, for the points at `a` and `b`.
        let (mut at, mut xs, mut ys) = (Vec::new(), Vec::new(), Vec::new());
        for s in samples.iter().flatten() {
            // Knots a single-precision graph tells apart.
            if at.last().is_some_and(|&last| s.at <= last + KNOT) {
                continue;
            }
            at.push(s.at);
            xs.push(s.point.x);
            ys.push(s.point.y);
        }
        let pa = (g.lookup(a, &at, &xs), g.lookup(a, &at, &ys));
        let pb = (g.lookup(b, &at, &xs), g.lookup(b, &at, &ys));
        // The direction along the run, each piece's own just past where it
        // starts.
        let (mut along, mut ux, mut uy) = (Vec::new(), Vec::new(), Vec::new());
        for (i, list) in samples.iter().enumerate() {
            for (k, s) in list.iter().enumerate() {
                let place = if i > 0 && k == 0 {
                    s.at + 2.0 * KNOT
                } else {
                    s.at
                };
                let length = s.tangent.0.hypot(s.tangent.1);
                if length <= 0.0 || along.last().is_some_and(|&last| place <= last + KNOT) {
                    continue;
                }
                along.push(place);
                ux.push(s.tangent.0 / length);
                uy.push(s.tangent.1 / length);
            }
        }
        let hair = tolerance / 4.0;
        let beyond = |g: &mut Graph, from: (Num, Num), u: Num, sign: f64| {
            let dx = g.lookup(u, &along, &ux);
            let dy = g.lookup(u, &along, &uy);
            let dx = g.math(MathOp::Mul, dx, Num::Const(sign * hair));
            let dy = g.math(MathOp::Mul, dy, Num::Const(sign * hair));
            (
                g.math(MathOp::Add, from.0, dx),
                g.math(MathOp::Add, from.1, dy),
            )
        };
        // The point at `clamp(place, a, b)`: the run's own at `place`, or
        // the one at `a` or `b` where `place` lies outside.
        let point = |g: &mut Graph, place: f64, p: Point| {
            let before = g.step(a, place, true);
            let after = g.step(b, place, false);
            let mut coord = |c: f64, at_a: Num, at_b: Num| {
                let da = g.math(MathOp::Sub, at_a, Num::Const(c));
                let da = g.math(MathOp::Mul, da, before);
                let db = g.math(MathOp::Sub, at_b, Num::Const(c));
                let db = g.math(MathOp::Mul, db, after);
                let d = g.math(MathOp::Add, da, db);
                g.math(MathOp::Add, d, Num::Const(c))
            };
            (coord(p.x, pa.0, pb.0), coord(p.y, pa.1, pb.1))
        };
        let mut ops = Vec::with_capacity(self.pieces.len() + 1);
        let Some(first) = self.pieces.first() else {
            return ops;
        };
        let mut start = point(g, first.from, first.seg.start());
        let lead = beyond(g, start, a, -1.0);
        ops.push(Op::MoveTo(lead.0, lead.1));
        ops.push(Op::LineTo(start.0, start.1));
        for (piece, samples) in self.pieces.iter().zip(&samples) {
            let end = point(g, piece.to, piece.seg.end());
            match piece.seg {
                Seg::Line(..) => ops.push(Op::LineTo(end.0, end.1)),
                Seg::Cubic(_) => {
                    let at: Vec<f64> = samples.iter().map(|s| s.at).collect();
                    let table = |f: fn(&Sample) -> f64| samples.iter().map(f).collect::<Vec<_>>();
                    let (ts, dx, dy) = (
                        table(|s| s.t),
                        table(|s| s.tangent.0),
                        table(|s| s.tangent.1),
                    );
                    let t0 = g.lookup(a, &at, &ts);
                    let t1 = g.lookup(b, &at, &ts);
                    let dt = g.math(MathOp::Sub, t1, t0);
                    let mut control = |u: Num, base: (Num, Num), op: MathOp| {
                        let tx = g.lookup(u, &at, &dx);
                        let ty = g.lookup(u, &at, &dy);
                        let tx = g.math(MathOp::Mul, tx, dt);
                        let ty = g.math(MathOp::Mul, ty, dt);
                        (g.math(op, base.0, tx), g.math(op, base.1, ty))
                    };
                    let c1 = control(a, start, MathOp::Add);
                    let c2 = control(b, end, MathOp::Sub);
                    ops.push(Op::CubicTo([c1.0, c1.1, c2.0, c2.1, end.0, end.1]));
                }
            }
            start = end;
        }
        let tail = beyond(g, start, b, 1.0);
        ops.push(Op::LineTo(tail.0, tail.1));
        ops
    }
}
