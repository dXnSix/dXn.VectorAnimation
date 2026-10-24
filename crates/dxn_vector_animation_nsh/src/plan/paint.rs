//! Gradients that move, on canvases whose gradients are still. A gradient
//! whose ends move is a still gradient placed by a transform the graph
//! moves; one whose colors change is its keyframes' gradients faded into
//! one another; a linear one whose stops move is a gradient of its own
//! between each two neighbouring stops, in bands the graph moves.
//!
//! Paints of several gradients are added up (`lighter`) in a canvas of
//! their own, within a clip of the shape (a stroke's outline, for a
//! stroke). The platforms' canvases agree on additions and clips; they do
//! not agree on composite operations that clear what lies outside the
//! drawing (`destination-in`).

use std::sync::Arc;

use dxn_vector_animation::{
    Animated, AnimatedPoint, BlendMode, FillRule, GradientStop, GradientUnits, Keyframe, LineCap,
    LineJoin, Matrix, Paint as ScenePaint, Path, Point, Shape, Stroke, Track, outline::trim_paths,
};

use super::{
    Compiler, Gradient, Op, Unsupported, consts, huge_rect, sample_times, static_path,
    static_paths, unsupported,
};
use crate::graph::{MathOp, NodeId, Num, Paint, pack};

/// How far the bands of [`Painting::Bands`] reach across the shape, in its
/// lengths.
const REACH: f64 = 1.0e5;

/// The narrowest band, in lengths of the gradient (narrower is a jump).
const NARROWEST: f64 = 1.0e-4;

/// How far a radial gradient's focal point may drift within its circle, in
/// radii, for the gradient to be placed still.
const FOCAL_DRIFT: f64 = 0.02;

/// A gradient paint as drawn.
pub(super) enum Painting {
    /// One gradient, in its space through `m`, then placed by `place`
    /// (where its ends move: the gradient is then the unit one).
    One {
        gradient: Arc<Gradient>,
        m: Matrix,
        place: Option<[Num; 6]>,
    },
    /// Gradients added up, each with its weight: the keyframes of a
    /// gradient whose colors change.
    Fades {
        gradients: Vec<(Arc<Gradient>, Num)>,
        m: Matrix,
        place: Option<[Num; 6]>,
    },
    /// A linear gradient whose stops move, along the unit gradient's x:
    /// before the first stop and past the last their colors, between each
    /// two stops a gradient of their two, out to `reach`.
    Bands {
        colors: Vec<u32>,
        edges: Vec<Num>,
        m: Matrix,
        place: [Num; 6],
        reach: f64,
    },
}

impl Painting {
    /// Whether it adds gradients up, in a canvas of its own.
    pub(super) fn alone(&self) -> bool {
        !matches!(self, Painting::One { .. })
    }
}

/// A gradient's stops as drawn: offsets and colors (`0xRRGGBBAA`).
type StopList = Vec<(f64, u32)>;

/// How a gradient's stops change.
enum Stops {
    Still(StopList),
    /// Each keyframe's stops, with the track of its weight.
    Fading(Vec<(StopList, Animated<f64>)>),
    /// The colors, with the track of each stop's place.
    Moving(Vec<u32>, Vec<Animated<f64>>),
}

fn stops_of(list: &[GradientStop]) -> StopList {
    list.iter().map(|s| (s.offset, pack(s.color))).collect()
}

fn stop_motion(stops: &Animated<Vec<GradientStop>>) -> Result<Stops, Unsupported> {
    let keys = match stops {
        Animated::Static(list) => return Ok(Stops::Still(stops_of(list))),
        Animated::Track(t) => &t.keyframes,
    };
    let Some(first) = keys.first() else {
        return Ok(Stops::Still(Vec::new()));
    };
    if keys.len() == 1 {
        return Ok(Stops::Still(stops_of(&first.value)));
    }
    if keys.iter().any(|k| k.value.len() != first.value.len()) {
        return unsupported("gradient stops that change in number");
    }
    let differ = |f: &dyn Fn(&GradientStop, &GradientStop) -> bool| {
        keys.iter()
            .any(|k| k.value.iter().zip(&first.value).any(|(a, b)| f(a, b)))
    };
    let moving = differ(&|a, b| (a.offset - b.offset).abs() > 1e-6);
    let fading = differ(&|a, b| pack(a.color) != pack(b.color));
    let track = |f: &dyn Fn(usize, &[GradientStop]) -> f64| {
        Animated::Track(Track::new(
            keys.iter()
                .enumerate()
                .map(|(i, k)| Keyframe::new(k.time, f(i, &k.value), k.easing.clone()))
                .collect(),
        ))
    };
    match (moving, fading) {
        (false, false) => Ok(Stops::Still(stops_of(&first.value))),
        (false, true) => Ok(Stops::Fading(
            (0..keys.len())
                .map(|j| {
                    let weight = track(&|i, _| if i == j { 1.0 } else { 0.0 });
                    (stops_of(&keys[j].value), weight)
                })
                .collect(),
        )),
        (true, false) => Ok(Stops::Moving(
            first.value.iter().map(|s| pack(s.color)).collect(),
            (0..first.value.len())
                .map(|n| track(&|_, stops| stops[n].offset))
                .collect(),
        )),
        (true, true) => unsupported("gradient stops whose colors and places both change"),
    }
}

/// How much `m` scales, at least about.
fn scale_of(m: Matrix) -> f64 {
    (m.a * m.d - m.b * m.c).abs().sqrt()
}

impl Compiler {
    /// A scene gradient on `s` as it is drawn.
    pub(super) fn painting(
        &mut self,
        paint: &ScenePaint,
        s: &Shape,
        time: NodeId,
    ) -> Result<Painting, Unsupported> {
        let space = |units: GradientUnits, transform: Matrix| -> Result<Matrix, Unsupported> {
            match units {
                GradientUnits::UserSpace => Ok(transform),
                GradientUnits::BoundingBox => {
                    let mut all = Path::new();
                    for p in static_paths(s)? {
                        all.extend(&p);
                    }
                    let b = all
                        .bounds()
                        .ok_or_else(|| Unsupported("an empty gradient box".into()))?;
                    Ok(Matrix::new(b.width, 0.0, 0.0, b.height, b.x, b.y) * transform)
                }
            }
        };
        match paint {
            ScenePaint::LinearGradient(g) => {
                let m = space(g.units, g.transform)?;
                let moving = g.start.is_animated() || g.end.is_animated();
                let times = sample_times(&[&g.start.x, &g.start.y, &g.end.x, &g.end.y]);
                let shortest = times
                    .iter()
                    .map(|&t| {
                        let (a, b) = (g.start.sample(t), g.end.sample(t));
                        (b.x - a.x).hypot(b.y - a.y)
                    })
                    .fold(f64::INFINITY, f64::min);
                let stops = stop_motion(&g.stops)?;
                if (moving || matches!(stops, Stops::Moving(..)))
                    && (shortest.is_nan() || shortest <= 1e-9)
                {
                    return unsupported("a moving gradient whose ends meet");
                }
                let own = |stops| {
                    Arc::new(Gradient::Linear {
                        start: g.start.sample(0.0),
                        end: g.end.sample(0.0),
                        stops,
                    })
                };
                let unit = |stops| {
                    Arc::new(Gradient::Linear {
                        start: Point::new(0.0, 0.0),
                        end: Point::new(1.0, 0.0),
                        stops,
                    })
                };
                let place = self.linear_place(&g.start, &g.end, time);
                Ok(match stops {
                    Stops::Still(stops) if moving => Painting::One {
                        gradient: unit(stops),
                        m,
                        place: Some(place),
                    },
                    Stops::Still(stops) => Painting::One {
                        gradient: own(stops),
                        m,
                        place: None,
                    },
                    Stops::Fading(keys) => Painting::Fades {
                        gradients: keys
                            .into_iter()
                            .map(|(stops, w)| {
                                let w = self.g.track(&w, time, |v| v);
                                (if moving { unit(stops) } else { own(stops) }, w)
                            })
                            .collect(),
                        m,
                        place: moving.then_some(place),
                    },
                    Stops::Moving(colors, places) => Painting::Bands {
                        colors,
                        edges: places
                            .iter()
                            .map(|p| self.g.track(p, time, |v| v))
                            .collect(),
                        m,
                        place,
                        reach: (REACH / (shortest * scale_of(m)).max(1e-9)).max(2.0),
                    },
                })
            }
            ScenePaint::RadialGradient(g) => {
                let m = space(g.units, g.transform)?;
                let moving =
                    g.center.is_animated() || g.focal.is_animated() || g.radius.is_animated();
                let own = |stops| {
                    Arc::new(Gradient::Radial {
                        focal: g.focal.sample(0.0),
                        center: g.center.sample(0.0),
                        radius: g.radius.sample(0.0),
                        stops,
                    })
                };
                // Where the focal point lies within the circle, in radii:
                // it must stay there (to a fiftieth, as sampled tracks
                // drift) for one gradient to be placed.
                let within = |t: f64| {
                    let (f, c) = (g.focal.sample(t), g.center.sample(t));
                    (f.x - c.x).hypot(f.y - c.y) / g.radius.sample(t)
                };
                let mut k = 0.0;
                if moving {
                    let times = sample_times(&[
                        &g.center.x,
                        &g.center.y,
                        &g.focal.x,
                        &g.focal.y,
                        &g.radius,
                    ]);
                    k = within(times[0]);
                    let still = times.iter().all(|&t| {
                        let v = within(t);
                        v.is_finite() && (v - k).abs() <= FOCAL_DRIFT
                    });
                    if !still {
                        return unsupported("a radial gradient whose focal point moves within it");
                    }
                }
                let unit = |stops| {
                    Arc::new(Gradient::Radial {
                        focal: Point::new(k, 0.0),
                        center: Point::new(0.0, 0.0),
                        radius: 1.0,
                        stops,
                    })
                };
                let place =
                    moving.then(|| self.radial_place(&g.center, &g.focal, &g.radius, k, time));
                match stop_motion(&g.stops)? {
                    Stops::Still(stops) => Ok(Painting::One {
                        gradient: if moving { unit(stops) } else { own(stops) },
                        m,
                        place,
                    }),
                    Stops::Fading(keys) => Ok(Painting::Fades {
                        gradients: keys
                            .into_iter()
                            .map(|(stops, w)| {
                                let w = self.g.track(&w, time, |v| v);
                                (if moving { unit(stops) } else { own(stops) }, w)
                            })
                            .collect(),
                        m,
                        place,
                    }),
                    Stops::Moving(..) => unsupported("a radial gradient whose stops move"),
                }
            }
            ScenePaint::Solid { .. } => unsupported("a color as a gradient"),
        }
    }

    /// The transform taking the unit linear gradient, from (0, 0) to
    /// (1, 0), to the one from `start` to `end`: a rotation, a scale and a
    /// move, its isolines staying square to it.
    fn linear_place(
        &mut self,
        start: &AnimatedPoint,
        end: &AnimatedPoint,
        time: NodeId,
    ) -> [Num; 6] {
        let sx = self.g.track(&start.x, time, |v| v);
        let sy = self.g.track(&start.y, time, |v| v);
        let ex = self.g.track(&end.x, time, |v| v);
        let ey = self.g.track(&end.y, time, |v| v);
        let dx = self.g.math(MathOp::Sub, ex, sx);
        let dy = self.g.math(MathOp::Sub, ey, sy);
        let across = self.g.math(MathOp::Mul, dy, Num::Const(-1.0));
        [dx, dy, across, dx, sx, sy]
    }

    /// The transform taking the unit radial gradient (radius 1 around
    /// (0, 0), its focal point at (`k`, 0)) to the one around `center` of
    /// `radius`, its focal point at `focal`.
    fn radial_place(
        &mut self,
        center: &AnimatedPoint,
        focal: &AnimatedPoint,
        radius: &Animated<f64>,
        k: f64,
        time: NodeId,
    ) -> [Num; 6] {
        let cx = self.g.track(&center.x, time, |v| v);
        let cy = self.g.track(&center.y, time, |v| v);
        if k <= 1e-9 {
            let r = self.g.track(radius, time, |v| v);
            return [r, Num::Const(0.0), Num::Const(0.0), r, cx, cy];
        }
        // Toward the focal point, the radius long.
        let fx = self.g.track(&focal.x, time, |v| v);
        let fy = self.g.track(&focal.y, time, |v| v);
        let ax = self.g.math(MathOp::Sub, fx, cx);
        let ax = self.g.math(MathOp::Mul, ax, Num::Const(1.0 / k));
        let ay = self.g.math(MathOp::Sub, fy, cy);
        let ay = self.g.math(MathOp::Mul, ay, Num::Const(1.0 / k));
        let across = self.g.math(MathOp::Mul, ay, Num::Const(-1.0));
        [ax, ay, across, ax, cx, cy]
    }

    /// Operations painting `painting` over everything (within the clip) at
    /// `alpha`, in the current space.
    pub(super) fn plane(&mut self, painting: &Painting, alpha: Num) -> Vec<Op> {
        let mut ops = vec![Op::Save, Op::BeginPath];
        let place = |ops: &mut Vec<Op>, m: Matrix, place: Option<&[Num; 6]>| {
            if m != Matrix::IDENTITY {
                ops.push(Op::Transform(consts(m)));
            }
            if let Some(p) = place {
                ops.push(Op::Transform(*p));
            }
        };
        match painting {
            Painting::One {
                gradient,
                m,
                place: p,
            } => {
                // The path is built already: the transforms move the
                // gradient only.
                ops.extend(huge_rect());
                ops.push(Op::GlobalAlpha(alpha));
                place(&mut ops, *m, p.as_ref());
                ops.push(Op::FillGradient(gradient.clone()));
                ops.push(Op::Fill(FillRule::NonZero));
            }
            Painting::Fades {
                gradients,
                m,
                place: p,
            } => {
                ops.extend(huge_rect());
                place(&mut ops, *m, p.as_ref());
                ops.push(Op::Blend(BlendMode::Add));
                for (gradient, weight) in gradients {
                    let a = self.g.math(MathOp::Mul, alpha, *weight);
                    ops.push(Op::GlobalAlpha(a));
                    ops.push(Op::FillGradient(gradient.clone()));
                    ops.push(Op::Fill(FillRule::NonZero));
                }
            }
            Painting::Bands {
                colors,
                edges,
                m,
                place: p,
                reach,
            } => {
                place(&mut ops, *m, Some(p));
                ops.push(Op::Blend(BlendMode::Add));
                ops.push(Op::GlobalAlpha(alpha));
                let r = *reach;
                let band = |ops: &mut Vec<Op>, from: Num, to: Num| {
                    let c = Num::Const;
                    ops.extend([
                        Op::BeginPath,
                        Op::MoveTo(from, c(-r)),
                        Op::LineTo(to, c(-r)),
                        Op::LineTo(to, c(r)),
                        Op::LineTo(from, c(r)),
                        Op::Close,
                    ]);
                };
                let (Some(&first), Some(&last)) = (edges.first(), edges.last()) else {
                    ops.push(Op::Restore);
                    return ops;
                };
                band(&mut ops, Num::Const(-r), first);
                ops.push(Op::FillColor(Paint::Const(colors[0])));
                ops.push(Op::Fill(FillRule::NonZero));
                for (i, pair) in edges.windows(2).enumerate() {
                    let (from, to) = (pair[0], pair[1]);
                    band(&mut ops, from, to);
                    let width = self.g.math(MathOp::Sub, to, from);
                    let width = self.g.above(width, NARROWEST);
                    let width = self.g.math(MathOp::Add, width, Num::Const(NARROWEST));
                    let zero = Num::Const(0.0);
                    ops.push(Op::Save);
                    ops.push(Op::Transform([
                        width,
                        zero,
                        zero,
                        Num::Const(1.0),
                        from,
                        zero,
                    ]));
                    ops.push(Op::FillGradient(Arc::new(Gradient::Linear {
                        start: Point::new(0.0, 0.0),
                        end: Point::new(1.0, 0.0),
                        stops: vec![(0.0, colors[i]), (1.0, colors[i + 1])],
                    })));
                    ops.push(Op::Fill(FillRule::NonZero));
                    ops.push(Op::Restore);
                }
                band(&mut ops, last, Num::Const(r));
                ops.push(Op::FillColor(Paint::Const(colors[colors.len() - 1])));
                ops.push(Op::Fill(FillRule::NonZero));
            }
        }
        ops.push(Op::Restore);
        ops
    }

    /// A stroke painted with what a stroke style cannot hold: its outline
    /// (still) a clip, the paint over everything within it. Whether it
    /// needs a canvas of its own comes along.
    pub(super) fn painted_stroke(
        &mut self,
        s: &Shape,
        stroke: &Stroke,
        painting: &Painting,
        alpha: Num,
    ) -> Result<(Vec<Op>, bool), Unsupported> {
        let Some(&width) = stroke.width.as_static() else {
            return unsupported("a stroke of a changing width painted with a moving gradient");
        };
        let mut paths = static_paths(s)?;
        if let Some(trim) = &s.trim {
            if trim.start.is_animated() || trim.end.is_animated() || trim.offset.is_animated() {
                return unsupported("a moving trim of a stroke painted with a moving gradient");
            }
            paths = vec![trim_paths(&paths, trim, 0.0)];
        }
        let mut style = kurbo::Stroke::new(width.max(0.0))
            .with_caps(match stroke.cap {
                LineCap::Butt => kurbo::Cap::Butt,
                LineCap::Round => kurbo::Cap::Round,
                LineCap::Square => kurbo::Cap::Square,
            })
            .with_join(match stroke.join {
                LineJoin::Miter => kurbo::Join::Miter,
                LineJoin::Round => kurbo::Join::Round,
                LineJoin::Bevel => kurbo::Join::Bevel,
            })
            .with_miter_limit(stroke.miter_limit);
        if let Some(dash) = &stroke.dash {
            let mut array = Vec::with_capacity(dash.array.len());
            for d in &dash.array {
                match d.as_static() {
                    Some(v) => array.push(v.max(0.0)),
                    None => {
                        return unsupported(
                            "an animated dash pattern painted with a moving gradient",
                        );
                    }
                }
            }
            let Some(&offset) = dash.offset.as_static() else {
                return unsupported("a moving dash pattern painted with a moving gradient");
            };
            if array.iter().sum::<f64>() > 0.0 {
                style = style.with_dashes(offset, array);
            }
        }
        let tolerance = self.tolerance();
        let mut outline = Path::new();
        for p in &paths {
            let stroked = kurbo::stroke(
                p.to_kurbo().iter(),
                &style,
                &kurbo::StrokeOpts::default(),
                tolerance,
            );
            outline.extend(&Path::from_kurbo(&stroked));
        }
        let mut ops = vec![Op::Save, Op::BeginPath];
        ops.extend(static_path(&outline));
        ops.push(Op::Clip(FillRule::NonZero));
        ops.extend(self.plane(painting, alpha));
        ops.push(Op::Restore);
        Ok((ops, painting.alone()))
    }
}
