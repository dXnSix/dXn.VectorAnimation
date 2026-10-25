//! What reading an untrusted file may take, the errors importers report,
//! and the counts a scene's size is checked by.

use crate::{Animated, Geometry, Group, Node, Paint, Scene, TransformOp};

/// What reading one file may take. A file over a limit is refused with
/// [`Error::Limit`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// The file's size in bytes.
    pub max_bytes: usize,
    /// Groups, shapes and images.
    pub max_nodes: usize,
    /// Keyframes of every track together.
    pub max_keyframes: usize,
    /// Groups nested in groups (masks and nested compositions included).
    pub max_depth: usize,
    /// Points of every path and keyframed path together.
    pub max_path_points: usize,
    /// Seconds of timeline.
    pub max_duration: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_bytes: 32 * 1024 * 1024,
            max_nodes: 100_000,
            max_keyframes: 1_000_000,
            max_depth: 64,
            max_path_points: 4_000_000,
            max_duration: 3600.0,
        }
    }
}

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    /// The file is over one of the [`Limits`].
    #[error("the file exceeds the limit of {limit} {what}")]
    Limit {
        /// What was counted.
        what: &'static str,
        /// The limit.
        limit: f64,
    },
    /// The file is not a valid document of its format.
    #[error("invalid file: {0}")]
    Invalid(String),
    /// The file uses something this library cannot represent at all.
    #[error("unsupported: {0}")]
    Unsupported(String),
}

/// A scene read from a file, with what of the file it leaves out.
#[derive(Debug, Clone)]
pub struct Import {
    /// The scene.
    pub scene: Scene,
    /// What the file holds that the scene leaves out or approximates, one
    /// message each.
    pub warnings: Vec<String>,
}

/// Counts of a scene's parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Groups.
    pub groups: usize,
    /// Shapes.
    pub shapes: usize,
    /// Images.
    pub images: usize,
    /// Properties that change over time.
    pub animated_properties: usize,
    /// Keyframes of every track.
    pub keyframes: usize,
    /// The deepest nesting of groups.
    pub depth: usize,
    /// Points of every path and keyframed path.
    pub path_points: usize,
}

impl Scene {
    /// Counts the scene's parts.
    pub fn stats(&self) -> Stats {
        let mut stats = Stats::default();
        count_group(&self.root, 1, &mut stats);
        stats
    }

    /// Checks the scene against `limits`.
    pub fn check(&self, limits: &Limits) -> Result<Stats, Error> {
        if self.timeline.duration.is_nan() || self.timeline.duration > limits.max_duration {
            return Err(Error::Limit {
                what: "seconds of timeline",
                limit: limits.max_duration,
            });
        }
        let stats = self.stats();
        let over = |count: usize, limit: usize, what: &'static str| {
            (count > limit).then_some(Error::Limit {
                what,
                limit: limit as f64,
            })
        };
        let error = over(
            stats.groups + stats.shapes + stats.images,
            limits.max_nodes,
            "nodes",
        )
        .or_else(|| over(stats.keyframes, limits.max_keyframes, "keyframes"))
        .or_else(|| over(stats.depth, limits.max_depth, "levels of nesting"))
        .or_else(|| over(stats.path_points, limits.max_path_points, "path points"));
        match error {
            Some(e) => Err(e),
            None => Ok(stats),
        }
    }
}

fn count<T>(value: &Animated<T>, stats: &mut Stats) {
    if value.is_animated() {
        stats.animated_properties += 1;
        stats.keyframes += value.keyframes().len();
    }
}

fn count_ops(ops: &[TransformOp], stats: &mut Stats) {
    for op in ops {
        match op {
            TransformOp::Translate { x, y }
            | TransformOp::Scale { x, y }
            | TransformOp::Skew { x, y } => {
                count(x, stats);
                count(y, stats);
            }
            TransformOp::Rotate { degrees } => count(degrees, stats),
            TransformOp::Matrix { matrix } => count(matrix, stats),
            TransformOp::Motion(motion) => {
                count(&motion.progress, stats);
                stats.path_points += motion.path.point_count();
            }
        }
    }
}

fn count_geometry(geometry: &Geometry, stats: &mut Stats) {
    match geometry {
        Geometry::Path { path } => {
            count(path, stats);
            match path {
                Animated::Static(p) => stats.path_points += p.point_count(),
                Animated::Track(track) => {
                    stats.path_points += track
                        .keyframes
                        .iter()
                        .map(|k| k.value.point_count())
                        .sum::<usize>();
                }
            }
        }
        Geometry::Rect {
            center,
            size,
            radius,
            ..
        } => {
            for p in [center, size, radius] {
                count(&p.x, stats);
                count(&p.y, stats);
            }
        }
        Geometry::Ellipse { center, radius, .. } => {
            for p in [center, radius] {
                count(&p.x, stats);
                count(&p.y, stats);
            }
        }
        Geometry::Star(star) => {
            count(&star.center.x, stats);
            count(&star.center.y, stats);
            for v in [
                &star.points,
                &star.outer_radius,
                &star.inner_radius,
                &star.outer_roundness,
                &star.inner_roundness,
                &star.rotation,
            ] {
                count(v, stats);
            }
            if let Some(points) = star.points.as_static() {
                stats.path_points += points.clamp(0.0, 1e6) as usize * 6;
            }
        }
    }
}

fn count_paint(paint: &Paint, depth: usize, stats: &mut Stats) {
    match paint {
        Paint::Solid { color } => count(color, stats),
        Paint::LinearGradient(g) => {
            for p in [&g.start, &g.end] {
                count(&p.x, stats);
                count(&p.y, stats);
            }
            count(&g.stops, stats);
        }
        Paint::RadialGradient(g) => {
            for p in [&g.center, &g.focal] {
                count(&p.x, stats);
                count(&p.y, stats);
            }
            count(&g.radius, stats);
            count(&g.stops, stats);
        }
        Paint::Pattern(p) => count_group(&p.content, depth + 1, stats),
    }
}

fn count_color_op(op: &crate::ColorOp, stats: &mut Stats) {
    use crate::ColorOp::*;
    match op {
        Matrix { matrix } => count(matrix, stats),
        Saturate { amount }
        | Grayscale { amount }
        | Sepia { amount }
        | Invert { amount }
        | Opacity { amount }
        | Brightness { amount }
        | Contrast { amount } => count(amount, stats),
        HueRotate { degrees } => count(degrees, stats),
        LuminanceToAlpha | Transfer { .. } => {}
        Tint {
            black,
            white,
            amount,
        } => {
            count(black, stats);
            count(white, stats);
            count(amount, stats);
        }
        Fill { color, amount } => {
            count(color, stats);
            count(amount, stats);
        }
        GradientMap { stops, amount } => {
            count(stops, stats);
            count(amount, stats);
        }
    }
}

fn count_group(group: &Group, depth: usize, stats: &mut Stats) {
    stats.groups += 1;
    stats.depth = stats.depth.max(depth);
    count(&group.opacity, stats);
    count_ops(&group.transform, stats);
    if let Some(remap) = group.time.as_ref().and_then(|t| t.remap.as_ref()) {
        count(remap, stats);
    }
    if let Some(clip) = &group.clip {
        for shape in &clip.shapes {
            count_geometry(&shape.geometry, stats);
            count_ops(&shape.transform, stats);
        }
    }
    if let Some(mask) = &group.mask {
        count_group(&mask.content, depth + 1, stats);
    }
    for effect in &group.effects {
        match effect {
            crate::Effect::DropShadow(shadow) => {
                count(&shadow.color, stats);
                count(&shadow.offset.x, stats);
                count(&shadow.offset.y, stats);
                count(&shadow.blur, stats);
            }
            crate::Effect::Blur(blur) => count(&blur.radius, stats),
            crate::Effect::InnerShadow(shadow) => {
                count(&shadow.color, stats);
                count(&shadow.offset.x, stats);
                count(&shadow.offset.y, stats);
                count(&shadow.blur, stats);
            }
            crate::Effect::Color(effect) => {
                for op in &effect.ops {
                    count_color_op(op, stats);
                }
            }
        }
    }
    for child in &group.children {
        match child {
            Node::Group(g) => count_group(g, depth + 1, stats),
            Node::Image(_) => stats.images += 1,
            Node::Text(text) => {
                stats.shapes += 1;
                for span in &text.spans {
                    if let Some(p) = &span.position {
                        count(&p.x, stats);
                        count(&p.y, stats);
                    }
                    count(&span.font.size, stats);
                    count(&span.letter_spacing, stats);
                    if let Some(fill) = &span.fill {
                        count(&fill.opacity, stats);
                        count_paint(&fill.paint, depth, stats);
                    }
                    if let Some(stroke) = &span.stroke {
                        count(&stroke.opacity, stats);
                        count(&stroke.width, stats);
                        count_paint(&stroke.paint, depth, stats);
                    }
                }
            }
            Node::Shape(shape) => {
                stats.shapes += 1;
                for p in &shape.paths {
                    count_geometry(&p.geometry, stats);
                    count_ops(&p.transform, stats);
                }
                if let Some(fill) = &shape.fill {
                    count(&fill.opacity, stats);
                    count_paint(&fill.paint, depth, stats);
                }
                if let Some(stroke) = &shape.stroke {
                    count(&stroke.opacity, stats);
                    count(&stroke.width, stats);
                    count_paint(&stroke.paint, depth, stats);
                    if let Some(dash) = &stroke.dash {
                        for d in &dash.array {
                            count(d, stats);
                        }
                        count(&dash.offset, stats);
                    }
                }
                if let Some(trim) = &shape.trim {
                    count(&trim.start, stats);
                    count(&trim.end, stats);
                    count(&trim.offset, stats);
                }
            }
        }
    }
}
