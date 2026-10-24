//! A scene at one moment: a flat display list any renderer draws.

use std::sync::Arc;

use crate::{
    BlendMode, Color, FillRule, GradientStop, GradientUnits, Group, ImageData, LineCap, LineJoin,
    MaskKind, Matrix, Node, Paint, PaintOrder, Path, Point, Rect, Scene, Shape, Spread,
    outline::{geometry_path, trim_paths},
    transform::transform_at,
};

/// How deep the sampler follows nested groups; deeper ones are left out
/// (importers refuse them long before, through their limits).
const MAX_DEPTH: usize = 256;

/// The scene at one moment, as a display list.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Width in user units.
    pub width: f64,
    /// Height in user units.
    pub height: f64,
    /// Drawn first to last.
    pub items: Vec<Item>,
}

/// One drawing operation.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// Fills `path`, transformed by `transform`.
    Fill {
        /// In its own coordinates.
        path: Path,
        /// From the path's coordinates to the scene's.
        transform: Matrix,
        /// The color or gradient, opacity included.
        paint: FramePaint,
        /// Which areas are inside.
        rule: FillRule,
    },
    /// Strokes `path`, transformed by `transform` (the stroke too).
    Stroke {
        /// In its own coordinates.
        path: Path,
        /// From the path's coordinates to the scene's.
        transform: Matrix,
        /// The color or gradient, opacity included.
        paint: FramePaint,
        /// Width, ends, corners and dashes.
        style: StrokeStyle,
    },
    /// Draws an image stretched into `rect`.
    Image {
        /// The encoded image.
        data: Arc<ImageData>,
        /// Where, in the image's coordinates.
        rect: Rect,
        /// From the image's coordinates to the scene's.
        transform: Matrix,
    },
    /// Starts drawing into a layer, composed when its [`Item::PopLayer`]
    /// comes.
    PushLayer(Layer),
    /// Composes the last layer pushed onto what is under it.
    PopLayer,
}

/// How a layer composes onto what is under it.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// 0 to 1.
    pub opacity: f64,
    /// How its colors mix with what is under it.
    pub blend: BlendMode,
    /// Outside these shapes nothing of the layer shows.
    pub clip: Option<Vec<ClipPath>>,
    /// How much of the layer shows, pixel by pixel.
    pub mask: Option<FrameMask>,
}

/// One shape of a layer's clip.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipPath {
    /// In its own coordinates.
    pub path: Path,
    /// From the path's coordinates to the scene's.
    pub transform: Matrix,
    /// Which areas are inside.
    pub rule: FillRule,
}

/// What masks a layer.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameMask {
    /// Which channel of the drawn items masks.
    pub kind: MaskKind,
    /// Shows the layer where the items do not.
    pub inverted: bool,
    /// The mask's content, in scene coordinates.
    pub items: Vec<Item>,
}

/// A color or gradient at one moment.
#[derive(Debug, Clone, PartialEq)]
pub enum FramePaint {
    /// One color.
    Solid(Color),
    /// Colors along the line from `start` to `end`.
    Linear {
        /// Where offset 0 is.
        start: Point,
        /// Where offset 1 is.
        end: Point,
        /// The colors, opacity included.
        stops: Vec<GradientStop>,
        /// Past the ends.
        spread: Spread,
        /// From the gradient's coordinates to the item's.
        transform: Matrix,
    },
    /// Colors from `focal` out to the circle around `center`.
    Radial {
        /// The circle's center.
        center: Point,
        /// The circle's radius.
        radius: f64,
        /// Where offset 0 is.
        focal: Point,
        /// The colors, opacity included.
        stops: Vec<GradientStop>,
        /// Past the circle.
        spread: Spread,
        /// From the gradient's coordinates to the item's.
        transform: Matrix,
    },
}

/// A stroke's geometry at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeStyle {
    /// Line width.
    pub width: f64,
    /// Line ends.
    pub cap: LineCap,
    /// Corners.
    pub join: LineJoin,
    /// Where miters turn into bevels.
    pub miter_limit: f64,
    /// Dash and gap lengths in turn, and where the pattern starts.
    pub dash: Option<(Vec<f64>, f64)>,
}

impl Scene {
    /// The scene at `time` (seconds of its timeline), as a display list.
    /// The time is not wrapped: looping and ping-pong are the player's.
    pub fn frame_at(&self, time: f64) -> Frame {
        let mut items = Vec::new();
        sample_group(&self.root, time, Matrix::IDENTITY, &mut items, 0);
        Frame {
            width: self.width,
            height: self.height,
            items,
        }
    }
}

fn sample_group(group: &Group, time: f64, parent: Matrix, out: &mut Vec<Item>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    if group.active.is_some_and(|range| !range.contains(time)) {
        return;
    }
    let time = group.local_time(time);
    let m = parent * transform_at(&group.transform, time);
    if !m.is_finite() {
        return;
    }
    let opacity = group.opacity.sample(time).clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    let clip = group.clip.as_ref().map(|clip| {
        clip.shapes
            .iter()
            .map(|shape| ClipPath {
                path: geometry_path(&shape.geometry, time),
                transform: m * transform_at(&shape.transform, time),
                rule: shape.rule,
            })
            .collect::<Vec<_>>()
    });
    let mask = group.mask.as_ref().map(|mask| {
        let mut items = Vec::new();
        sample_group(&mask.content, time, m, &mut items, depth + 1);
        FrameMask {
            kind: mask.kind,
            inverted: mask.inverted,
            items,
        }
    });
    let layer = opacity < 1.0 || !group.blend.is_normal() || clip.is_some() || mask.is_some();
    if layer {
        out.push(Item::PushLayer(Layer {
            opacity,
            blend: group.blend,
            clip,
            mask,
        }));
    }
    for child in &group.children {
        match child {
            Node::Group(g) => sample_group(g, time, m, out, depth + 1),
            Node::Shape(s) => sample_shape(s, time, m, out),
            Node::Image(image) => out.push(Item::Image {
                data: image.data.clone(),
                rect: image.rect,
                transform: m,
            }),
        }
    }
    if layer {
        out.push(Item::PopLayer);
    }
}

fn sample_shape(shape: &Shape, time: f64, m: Matrix, out: &mut Vec<Item>) {
    let paths: Vec<Path> = shape
        .paths
        .iter()
        .map(|p| {
            let path = geometry_path(&p.geometry, time);
            if p.transform.is_empty() {
                path
            } else {
                path.transformed(transform_at(&p.transform, time))
            }
        })
        .collect();
    let path = match &shape.trim {
        Some(trim) => trim_paths(&paths, trim, time),
        None => {
            let mut all = Path::new();
            for p in &paths {
                all.extend(p);
            }
            all
        }
    };
    if path.is_empty() {
        return;
    }
    let fill = shape.fill.as_ref().and_then(|fill| {
        let paint = paint_at(&fill.paint, time, fill.opacity.sample(time), &path)?;
        Some(Item::Fill {
            path: path.clone(),
            transform: m,
            paint,
            rule: fill.rule,
        })
    });
    let stroke = shape.stroke.as_ref().and_then(|stroke| {
        let width = stroke.width.sample(time);
        if width <= 0.0 || !width.is_finite() {
            return None;
        }
        let paint = paint_at(&stroke.paint, time, stroke.opacity.sample(time), &path)?;
        let dash = stroke.dash.as_ref().and_then(|dash| {
            let array: Vec<f64> = dash.array.iter().map(|d| d.sample(time).max(0.0)).collect();
            (!array.is_empty() && array.iter().sum::<f64>() > 0.0)
                .then(|| (array, dash.offset.sample(time)))
        });
        Some(Item::Stroke {
            path: path.clone(),
            transform: m,
            paint,
            style: StrokeStyle {
                width,
                cap: stroke.cap,
                join: stroke.join,
                miter_limit: stroke.miter_limit,
                dash,
            },
        })
    });
    let (first, second) = match shape.paint_order {
        PaintOrder::FillFirst => (fill, stroke),
        PaintOrder::StrokeFirst => (stroke, fill),
    };
    out.extend(first);
    out.extend(second);
}

/// `paint` at `time` with `opacity` folded in, or `None` when nothing would
/// show. Bounding-box gradients map onto `path`'s box.
fn paint_at(paint: &Paint, time: f64, opacity: f64, path: &Path) -> Option<FramePaint> {
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return None;
    }
    let stops = |stops: &crate::Animated<Vec<GradientStop>>| -> Vec<GradientStop> {
        stops
            .sample(time)
            .into_iter()
            .map(|s| GradientStop {
                offset: s.offset,
                color: s.color.with_opacity(opacity),
            })
            .collect()
    };
    let space = |units: GradientUnits, transform: Matrix| -> Option<Matrix> {
        match units {
            GradientUnits::UserSpace => Some(transform),
            GradientUnits::BoundingBox => {
                let b = path.bounds()?;
                if b.width <= 0.0 || b.height <= 0.0 {
                    return None;
                }
                Some(Matrix::new(b.width, 0.0, 0.0, b.height, b.x, b.y) * transform)
            }
        }
    };
    Some(match paint {
        Paint::Solid { color } => {
            let color = color.sample(time).with_opacity(opacity);
            if color.a <= 0.0 {
                return None;
            }
            FramePaint::Solid(color)
        }
        Paint::LinearGradient(g) => FramePaint::Linear {
            start: g.start.sample(time),
            end: g.end.sample(time),
            stops: stops(&g.stops),
            spread: g.spread,
            transform: space(g.units, g.transform)?,
        },
        Paint::RadialGradient(g) => FramePaint::Radial {
            center: g.center.sample(time),
            radius: g.radius.sample(time),
            focal: g.focal.sample(time),
            stops: stops(&g.stops),
            spread: g.spread,
            transform: space(g.units, g.transform)?,
        },
    })
}
