//! What plays when an event happens (SVG's SMIL begins on `click`): a
//! segment of the timeline past its end, under a marker named `on:<event>`,
//! which the timeline never reaches. [`Scene::with_events`] moves each
//! segment's keyframes to when its event happened.

use crate::{
    Animated, AnimatedPoint, ColorOp, Easing, Effect, Geometry, Group, Interpolate, Keyframe, Node,
    Paint, Scene, TransformOp,
};

/// The prefix of a marker naming an event's segment.
pub const EVENT_PREFIX: &str = "on:";

/// Visits every animated property of a scene.
pub(crate) trait Visit {
    fn visit<T: Interpolate>(&mut self, a: &mut Animated<T>);
}

fn point<V: Visit>(p: &mut AnimatedPoint, v: &mut V) {
    v.visit(&mut p.x);
    v.visit(&mut p.y);
}

fn ops<V: Visit>(list: &mut [TransformOp], v: &mut V) {
    for op in list {
        match op {
            TransformOp::Translate { x, y }
            | TransformOp::Scale { x, y }
            | TransformOp::Skew { x, y } => {
                v.visit(x);
                v.visit(y);
            }
            TransformOp::Rotate { degrees } => v.visit(degrees),
            TransformOp::Matrix { matrix } => v.visit(matrix),
            TransformOp::Motion(m) => v.visit(&mut m.progress),
        }
    }
}

fn geometry<V: Visit>(g: &mut Geometry, v: &mut V) {
    match g {
        Geometry::Path { path } => v.visit(path),
        Geometry::Rect {
            center,
            size,
            radius,
            ..
        } => {
            point(center, v);
            point(size, v);
            point(radius, v);
        }
        Geometry::Ellipse { center, radius, .. } => {
            point(center, v);
            point(radius, v);
        }
        Geometry::Star(s) => {
            point(&mut s.center, v);
            for a in [
                &mut s.points,
                &mut s.outer_radius,
                &mut s.inner_radius,
                &mut s.outer_roundness,
                &mut s.inner_roundness,
                &mut s.rotation,
            ] {
                v.visit(a);
            }
        }
    }
}

fn paint<V: Visit>(p: &mut Paint, v: &mut V) {
    match p {
        Paint::Solid { color } => v.visit(color),
        Paint::LinearGradient(g) => {
            point(&mut g.start, v);
            point(&mut g.end, v);
            v.visit(&mut g.stops);
        }
        Paint::RadialGradient(g) => {
            point(&mut g.center, v);
            point(&mut g.focal, v);
            v.visit(&mut g.radius);
            v.visit(&mut g.stops);
        }
        Paint::Pattern(p) => group(&mut p.content, v),
    }
}

fn color_op<V: Visit>(op: &mut ColorOp, v: &mut V) {
    match op {
        ColorOp::Matrix { matrix } => v.visit(matrix),
        ColorOp::Saturate { amount }
        | ColorOp::Grayscale { amount }
        | ColorOp::Sepia { amount }
        | ColorOp::Invert { amount }
        | ColorOp::Opacity { amount }
        | ColorOp::Brightness { amount }
        | ColorOp::Contrast { amount } => v.visit(amount),
        ColorOp::HueRotate { degrees } => v.visit(degrees),
        ColorOp::LuminanceToAlpha | ColorOp::Transfer { .. } => {}
        ColorOp::Tint {
            black,
            white,
            amount,
        } => {
            v.visit(black);
            v.visit(white);
            v.visit(amount);
        }
        ColorOp::Fill { color, amount } => {
            v.visit(color);
            v.visit(amount);
        }
        ColorOp::GradientMap { stops, amount } => {
            v.visit(stops);
            v.visit(amount);
        }
    }
}

/// Visits `g`'s properties and everything in it.
pub(crate) fn group<V: Visit>(g: &mut Group, v: &mut V) {
    v.visit(&mut g.opacity);
    ops(&mut g.transform, v);
    if let Some(remap) = g.time.as_mut().and_then(|t| t.remap.as_mut()) {
        v.visit(remap);
    }
    if let Some(clip) = &mut g.clip {
        for shape in &mut clip.shapes {
            geometry(&mut shape.geometry, v);
            ops(&mut shape.transform, v);
        }
    }
    if let Some(mask) = &mut g.mask {
        group(&mut mask.content, v);
    }
    for effect in &mut g.effects {
        match effect {
            Effect::DropShadow(s) => {
                v.visit(&mut s.color);
                point(&mut s.offset, v);
                v.visit(&mut s.blur);
            }
            Effect::InnerShadow(s) => {
                v.visit(&mut s.color);
                point(&mut s.offset, v);
                v.visit(&mut s.blur);
            }
            Effect::Blur(b) => v.visit(&mut b.radius),
            Effect::Color(c) => c.ops.iter_mut().for_each(|op| color_op(op, v)),
        }
    }
    for child in &mut g.children {
        match child {
            Node::Group(g) => group(g, v),
            Node::Image(_) => {}
            Node::Shape(s) => {
                for p in &mut s.paths {
                    geometry(&mut p.geometry, v);
                    ops(&mut p.transform, v);
                }
                if let Some(fill) = &mut s.fill {
                    v.visit(&mut fill.opacity);
                    paint(&mut fill.paint, v);
                }
                if let Some(stroke) = &mut s.stroke {
                    v.visit(&mut stroke.opacity);
                    v.visit(&mut stroke.width);
                    paint(&mut stroke.paint, v);
                    if let Some(dash) = &mut stroke.dash {
                        for d in &mut dash.array {
                            v.visit(d);
                        }
                        v.visit(&mut dash.offset);
                    }
                }
                if let Some(trim) = &mut s.trim {
                    v.visit(&mut trim.start);
                    v.visit(&mut trim.end);
                    v.visit(&mut trim.offset);
                }
            }
            Node::Text(t) => {
                for span in &mut t.spans {
                    if let Some(p) = &mut span.position {
                        point(p, v);
                    }
                    v.visit(&mut span.font.size);
                    v.visit(&mut span.letter_spacing);
                    if let Some(fill) = &mut span.fill {
                        v.visit(&mut fill.opacity);
                        paint(&mut fill.paint, v);
                    }
                    if let Some(stroke) = &mut span.stroke {
                        v.visit(&mut stroke.opacity);
                        v.visit(&mut stroke.width);
                        paint(&mut stroke.paint, v);
                    }
                }
            }
        }
    }
}

/// Moves the keyframes of event segments to when their events happened.
struct Retime {
    /// Each happened event's segment (from, to) and the time it moves to.
    moves: Vec<(f64, f64, f64)>,
}

impl Visit for Retime {
    fn visit<T: Interpolate>(&mut self, a: &mut Animated<T>) {
        let Animated::Track(track) = a else {
            return;
        };
        for &(from, to, at) in &self.moves {
            let inside = |t: f64| t >= from && t < to;
            if !track.keyframes.iter().any(|k| inside(k.time)) {
                continue;
            }
            // Up to the event, what played; from it on, what plays on it
            // overrides what would play then.
            let before = track.sample(at);
            track
                .keyframes
                .retain(|k| inside(k.time) || k.time < at || k.time >= to);
            for k in &mut track.keyframes {
                if inside(k.time) {
                    k.time += at - from;
                }
            }
            track.keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
            let i = track.keyframes.partition_point(|k| k.time < at);
            track
                .keyframes
                .insert(i, Keyframe::new(at, before, Easing::Hold));
        }
    }
}

impl Scene {
    /// The events the scene has something to play on (its markers named
    /// `on:<event>`), by name.
    pub fn events(&self) -> Vec<&str> {
        self.timeline
            .markers
            .iter()
            .filter_map(|m| m.name.strip_prefix(EVENT_PREFIX))
            .collect()
    }

    /// The scene with what plays on each of `happened`'s events (a name and
    /// the time it happened, in the timeline's seconds) moved to that
    /// time, overriding what the property would do then. Events it has
    /// nothing for change nothing; what has not happened stays out of the
    /// timeline's reach.
    pub fn with_events(&self, happened: &[(&str, f64)]) -> Scene {
        let mut moves = Vec::new();
        for (name, at) in happened {
            if let Some(m) = self.timeline.markers.iter().find(|m| {
                m.name
                    .strip_prefix(EVENT_PREFIX)
                    .is_some_and(|n| n == *name)
            }) && at.is_finite()
            {
                moves.push((m.start, m.start + m.duration.max(0.0) + 1e-6, *at));
            }
        }
        let mut scene = self.clone();
        if !moves.is_empty() {
            group(&mut scene.root, &mut Retime { moves });
        }
        scene
    }
}
