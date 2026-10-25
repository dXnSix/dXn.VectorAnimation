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

use dxn_vector_animation::{Group, Matrix, Rect, Scene, TransformOp};

use super::{Compiler, NodeId, Num, Op, Open, Unit, Unsupported, closed, shell};

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
fn still_content(g: &Group, stage: &Scene) -> bool {
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

impl Compiler {
    /// Whether `g` may be drawn as a moving layer: its transform moves,
    /// its content holds still, and nothing about it needs the view's
    /// coordinates.
    pub(super) fn can_bind(&self, g: &Group) -> bool {
        self.casting.is_none()
            && self.bound.is_none()
            && g.transform.iter().any(TransformOp::is_animated)
            && g.effects.is_empty()
            && g.mask.is_none()
            && !self
                .ancestors
                .iter()
                .any(|(a, _)| a.clip.is_some() || a.mask.is_some() || !a.effects.is_empty())
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
        self.open().units.push(Unit::Layer {
            opacity: layer.opacity,
            blend: layer.blend,
            units,
            transform: Some(transform),
        });
        Ok(true)
    }
}
