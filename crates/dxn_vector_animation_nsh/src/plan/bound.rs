//! Groups that only move (or fade) as layers the platform moves: a group
//! whose transform moves while what it holds stays still is a layer with a
//! view transform (its matrix to the view, sampled over the playback),
//! its content drawn once in the group's own lengths (scaled up as far as
//! the group ever grows, so it stays sharp) on canvases over where it is.
//! The canvases then never draw again as it moves.
//!
//! What needs the view's own coordinates (a part sampled or rasterized, a
//! shadow, a clip or mask of a group around it) keeps the group drawn as
//! before.

use dxn_vector_animation::{
    Animated, Clip, Geometry, Group, Matrix, PathEl, Point, Rect, Scene, TransformOp,
    transform::transform_at,
};

use super::{
    Compiler, NodeId, Num, Op, Open, Unit, Unsupported, closed,
    shadow::{PROBE_RATE, PROBES},
    shell,
};

/// Where a group drawn as a moving layer starts: what its canvases' setup
/// is taken from, and how much its lengths are scaled up on them.
#[derive(Debug, Clone, Copy)]
pub(super) struct Bound {
    /// The index of its frame among the groups drawn inside.
    pub(super) origin: usize,
    /// Its canvases' points a length of its.
    pub(super) scale: f64,
}

/// Whether `g`'s content draws the same at every time of `stage`'s
/// timeline.
pub(super) fn still_content(g: &Group, stage: &Scene) -> bool {
    let scene = Scene {
        root: Group {
            children: g.children.clone(),
            ..Group::new()
        },
        ..stage.clone()
    };
    super::still(&scene)
}

/// Where `g`'s content draws over the playback, in its lengths.
fn content_bounds(g: &Group, stage: &Scene) -> Option<Rect> {
    let scene = Scene {
        root: Group {
            children: g.children.clone(),
            ..Group::new()
        },
        ..stage.clone()
    };
    scene.frame_at(0.0).bounds()
}

/// `units` (in a moving layer's coordinates) each over `area`: canvases
/// placed there, layers of them alike; `None` for anything else.
fn placed(units: Vec<Unit>, area: Rect) -> Option<Vec<Unit>> {
    units
        .into_iter()
        .map(|u| match u {
            Unit::Canvas(ops) => {
                let mut moved = vec![Op::Translate(Num::Const(-area.x), Num::Const(-area.y))];
                moved.extend(ops);
                Some(Unit::Area {
                    area,
                    shown: None,
                    ops: closed(moved),
                })
            }
            Unit::Layer {
                opacity,
                blend,
                units,
                transform: None,
            } => Some(Unit::Layer {
                opacity,
                blend,
                units: placed(units, area)?,
                transform: None,
            }),
            _ => None,
        })
        .collect()
}

/// Where `clip` (through `m`) leaves what it clips shown: a rectangle
/// along the view's axes, where it is a still one.
fn clip_rect(clip: &Clip, m: Matrix) -> Option<Rect> {
    let [shape] = clip.shapes.as_slice() else {
        return None;
    };
    if shape.transform.iter().any(TransformOp::is_animated) {
        return None;
    }
    let m = m * transform_at(&shape.transform, 0.0);
    let corners = match &shape.geometry {
        Geometry::Rect {
            center,
            size,
            radius,
            ..
        } => {
            let still = |a: &Animated<f64>| match a {
                Animated::Static(v) => Some(*v),
                Animated::Track(_) => None,
            };
            let (cx, cy, w, h) = (
                still(&center.x)?,
                still(&center.y)?,
                still(&size.x)?,
                still(&size.y)?,
            );
            if still(&radius.x)? != 0.0 || still(&radius.y)? != 0.0 {
                return None;
            }
            let (x0, y0, x1, y1) = (cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0);
            [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
        }
        Geometry::Path {
            path: Animated::Static(path),
        } => rectangle(path.elements())?,
        _ => return None,
    };
    // Along the view's axes.
    let tiny = 1e-9 * (m.a.abs() + m.b.abs() + m.c.abs() + m.d.abs());
    if m.b.abs() > tiny || m.c.abs() > tiny {
        return None;
    }
    let points = corners.map(|(x, y)| m.apply(Point::new(x, y)));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in points {
        (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
    }
    let rect = Rect::new(x0, y0, x1 - x0, y1 - y0);
    (rect.width.is_finite() && rect.height.is_finite()).then_some(rect)
}

/// The corners of the rectangle `path` outlines, where it is one along
/// its axes: a move, then lines along x or y round its four corners.
fn rectangle(path: &[PathEl]) -> Option<[(f64, f64); 4]> {
    let mut points = Vec::with_capacity(5);
    for (i, el) in path.iter().enumerate() {
        match (i, el) {
            (0, PathEl::MoveTo(p)) => points.push(*p),
            (i, PathEl::LineTo(p)) if i > 0 => points.push(*p),
            (i, PathEl::Close) if i == path.len() - 1 => {}
            _ => return None,
        }
    }
    if points.len() == 5 && points[4] == points[0] {
        points.pop();
    }
    let [a, b, c, d] = points.as_slice() else {
        return None;
    };
    let along = |p: &Point, q: &Point| (p.x == q.x) != (p.y == q.y);
    let corners = [*a, *b, *c, *d];
    let closes = corners.iter().zip(corners.iter().cycle().skip(1));
    let mut turns = 0;
    for (p, q) in closes {
        if !along(p, q) {
            return None;
        }
        turns += 1;
    }
    // Opposite corners apart along both.
    (turns == 4 && a.x != c.x && a.y != c.y).then(|| corners.map(|p| (p.x, p.y)))
}

impl Compiler {
    /// Where the clips of the groups being drawn leave what is inside them
    /// shown, in the view's points: `Some(None)` without any, `None` where
    /// one is not a still rectangle along the view's axes (or a group's
    /// transform moves outside it).
    fn ancestors_clip(&self) -> Option<Option<Rect>> {
        let mut m = self.base;
        let mut still = true;
        let mut shown: Option<Rect> = None;
        for (a, _) in &self.ancestors {
            still &= !a.transform.iter().any(TransformOp::is_animated);
            if still {
                m = m * transform_at(&a.transform, 0.0);
            }
            if let Some(clip) = &a.clip {
                if !still {
                    return None;
                }
                let rect = clip_rect(clip, m)?;
                shown = Some(match shown {
                    None => rect,
                    Some(r) => r
                        .intersect(rect)
                        .unwrap_or(Rect::new(rect.x, rect.y, 0.0, 0.0)),
                });
            }
        }
        Some(shown)
    }

    /// Whether `g` may be drawn as a moving layer: its transform moves (or
    /// one of the groups it is in does, where the playback is short enough
    /// for its matrix to be sampled at every frame), its content holds
    /// still, and nothing about it needs the view's coordinates.
    pub(super) fn can_bind(&self, g: &Group) -> bool {
        let moves = |g: &Group| g.transform.iter().any(TransformOp::is_animated);
        let sampled_whole = self.stage.timeline.duration * PROBE_RATE <= PROBES as f64;
        self.casting.is_none()
            && self.bound.is_none()
            && (moves(g) || sampled_whole && self.ancestors.iter().any(|(a, _)| moves(a)))
            && g.effects.is_empty()
            && g.mask.is_none()
            && !self
                .ancestors
                .iter()
                .any(|(a, _)| a.mask.is_some() || !a.effects.is_empty())
            && self.ancestors_clip().is_some()
            && still_content(g, &self.stage)
    }

    /// `g` drawn as a moving layer (see the module's docs); false where
    /// its content needs more than canvases (the caller undoes it then).
    pub(super) fn bound_group(
        &mut self,
        g: &Group,
        local: NodeId,
        depth: usize,
        visible: Num,
    ) -> Result<bool, Unsupported> {
        // Within the clips around it, still rectangles in the view.
        let Some(shown) = self.ancestors_clip() else {
            return Ok(false);
        };
        let (times, matrices) = self.chain_samples(g);
        let det = |m: &Matrix| (m.a * m.d - m.b * m.c).abs().sqrt();
        let scale = matrices.iter().map(det).fold(0.0, f64::max);
        if scale.is_nan() || scale <= 1e-6 {
            return Ok(false);
        }
        let Some(bounds) = content_bounds(g, &self.stage) else {
            return Ok(true);
        };
        // Its lengths scaled up on the canvases: the layer scales back.
        let down = Matrix::scale(1.0 / scale, 1.0 / scale);
        let matrices: Vec<Matrix> = matrices.iter().map(|m| *m * down).collect();
        let transform = self.layer_transform(&times, &matrices);
        let opacity = self.g.track(&g.opacity, local, |o| o.clamp(0.0, 1.0));
        let opacity = self.mul(opacity, visible);
        if opacity.is(0.0) {
            return Ok(true);
        }
        let mut frame = vec![Op::Save];
        if let Some(clip) = &g.clip {
            frame.extend(self.clip(clip, local)?);
        }
        self.flush();
        let outer = self.open().alpha;
        let opacity = self.mul(outer, opacity);
        self.layers.push(Open {
            opacity,
            blend: g.blend,
            units: Vec::new(),
            canvas: Vec::new(),
            alpha: Num::Const(1.0),
        });
        self.bound = Some(Bound {
            origin: self.context.len(),
            scale,
        });
        self.context.push(frame);
        self.ancestors.push((shell(g), true));
        let mut done = Ok(());
        for child in &g.children {
            done = self.child(child, local, depth);
            if done.is_err() {
                break;
            }
        }
        self.ancestors.pop();
        self.context.pop();
        self.flush();
        self.bound = None;
        let layer = self.layers.pop();
        if done.is_err() {
            return Ok(false);
        }
        let Some(layer) = layer else {
            return Ok(false);
        };
        // Over where the content is, in the canvases' points, a pixel
        // around.
        let area = Rect::new(
            (bounds.x * scale).floor() - 2.0,
            (bounds.y * scale).floor() - 2.0,
            (bounds.width * scale).ceil() + 4.0,
            (bounds.height * scale).ceil() + 4.0,
        );
        let Some(units) = placed(layer.units, area) else {
            return Ok(false);
        };
        let layer = Unit::Layer {
            opacity: layer.opacity,
            blend: layer.blend,
            units,
            transform: Some(transform),
        };
        self.open().units.push(match shown {
            Some(rect) => Unit::Clip {
                rect,
                units: vec![layer],
            },
            None => layer,
        });
        Ok(true)
    }
}
