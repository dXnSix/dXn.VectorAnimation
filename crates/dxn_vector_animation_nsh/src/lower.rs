//! A plan made into NSH elements: each canvas's operations recorded with a
//! `Painter` (graph nodes as animated values the canvas reads), each layer
//! a view faded or blended as one.

use std::{
    cell::RefCell,
    f64::consts::TAU,
    sync::{Arc, Weak},
};

use dxn_vector_animation::{BlendMode, FillRule, ImageData, LineCap, LineJoin, TextAnchor};
use nsh_core::{
    Color,
    animation::{Easing as NshEasing, Extrapolate, Output},
    canvas::{
        self, CanvasGradient, CanvasImage, CanvasOp, Composite, GradientShape,
        Paint as CanvasPaint, PathOp,
    },
    element::{AnimatedNode, AnimatedValue, Interpolation},
    geometry::{Percentage, point, px},
    prelude::*,
};

use crate::{
    graph::{Curve, Expr, Graph, MathOp, Num, Operand, Paint},
    plan::{Gradient, Op, Unit},
};

/// The animated values of a graph, made from the playback time.
pub(crate) struct Nodes {
    nodes: Vec<Option<AnimatedNode>>,
}

impl Nodes {
    /// Every node of `graph`, derived from `time` (inputs come before what
    /// reads them, so one pass makes them all).
    pub(crate) fn new(graph: &Graph, time: AnimatedValue) -> Self {
        let mut nodes: Vec<Option<AnimatedNode>> = Vec::with_capacity(graph.nodes().len());
        for expr in graph.nodes() {
            let made = match expr {
                Expr::Time => Some(*time.node()),
                Expr::Interp {
                    input,
                    inputs,
                    outputs,
                    curve,
                } => nodes[*input].map(|input| {
                    input.interpolate_with(
                        &f32s(inputs),
                        Output::Numbers(f32s(outputs)),
                        interpolation(*curve),
                    )
                }),
                Expr::InterpColor {
                    input,
                    inputs,
                    colors,
                    curve,
                } => nodes[*input].map(|input| {
                    input.interpolate_with(
                        &f32s(inputs),
                        Output::Colors(colors.clone()),
                        interpolation(*curve),
                    )
                }),
                Expr::Math(op, a, b) => nodes[*a].and_then(|a| {
                    let b: nsh_core::element::AnimationOperand = match b {
                        Operand::Const(c) => (*c as f32).into(),
                        Operand::Node(n) => (&nodes[*n]?).into(),
                    };
                    Some(match op {
                        MathOp::Add => a.add(b),
                        MathOp::Sub => a.sub(b),
                        MathOp::Mul => a.mul(b),
                        MathOp::Div => a.div(b),
                        MathOp::Mod => a.modulo(b),
                    })
                }),
            };
            nodes.push(made);
        }
        Nodes { nodes }
    }

    fn node(&self, id: usize) -> Option<&AnimatedNode> {
        self.nodes.get(id).and_then(Option::as_ref)
    }
}

fn f32s(v: &[f64]) -> Vec<f32> {
    v.iter().map(|x| *x as f32).collect()
}

fn interpolation(curve: Curve) -> Interpolation {
    Interpolation {
        extrapolate_left: Extrapolate::Clamp,
        extrapolate_right: Extrapolate::Clamp,
        easing: match curve {
            Curve::Linear => NshEasing::Linear,
            Curve::Bezier(a, b, c, d) => NshEasing::Bezier(a as f32, b as f32, c as f32, d as f32),
        },
    }
}

/// The elements of `units`, bottom first: canvases and layers the size of
/// the view, over one another; `part` makes the canvas of a part sampled
/// or rasterized.
pub(crate) fn elements(
    units: &[Unit],
    nodes: &Nodes,
    part: &dyn Fn(&Unit) -> AnyElement,
) -> Vec<AnyElement> {
    units
        .iter()
        .map(|unit| match unit {
            Unit::Canvas(ops) => {
                let painter = painter(ops, nodes);
                canvas(painter).absolute().size_full().into_element()
            }
            Unit::Wide {
                margin: [left, top, right, bottom],
                ops,
            } => {
                let painter = painter(ops, nodes);
                let wide = canvas(painter)
                    .absolute()
                    .left(px(-*left as f32))
                    .top(px(-*top as f32))
                    .right(px(-*right as f32))
                    .bottom(px(-*bottom as f32));
                view()
                    .absolute()
                    .size_full()
                    .overflow_hidden()
                    .child(wide)
                    .into_element()
            }
            Unit::Sampled(_) | Unit::Raster(_) => part(unit),
            Unit::Area { area, shown, ops } => {
                let painter = painter(ops, nodes);
                let (x, y) = match shown {
                    Some(shown) => (area.x - shown.x, area.y - shown.y),
                    None => (area.x, area.y),
                };
                let placed = canvas(painter)
                    .absolute()
                    .left(px(x as f32))
                    .top(px(y as f32))
                    .w(px(area.width as f32))
                    .h(px(area.height as f32));
                match shown {
                    None => placed.into_element(),
                    Some(shown) => view()
                        .absolute()
                        .left(px(shown.x as f32))
                        .top(px(shown.y as f32))
                        .w(px(shown.width as f32))
                        .h(px(shown.height as f32))
                        .overflow_hidden()
                        .child(placed)
                        .into_element(),
                }
            }
            Unit::Layer {
                opacity,
                blend,
                units,
                transform,
            } => {
                let mut layer = view().absolute().size_full();
                if let Some(t) = transform {
                    use nsh_core::TransformKind as K;
                    layer = layer.origin_top_left();
                    for (kind, n) in [
                        (K::TranslateX, t.translate.0),
                        (K::TranslateY, t.translate.1),
                        (K::Rotate, t.rotate),
                        (K::SkewX, t.skew),
                        (K::ScaleX, t.scale.0),
                        (K::ScaleY, t.scale.1),
                    ] {
                        if let Num::Node(id) = n
                            && let Some(node) = nodes.node(id)
                        {
                            layer = layer.bind_transform(kind, node);
                        }
                    }
                }
                match opacity {
                    Num::Const(c) => {
                        if *c < 1.0 {
                            layer = layer.opacity(*c as f32);
                        }
                    }
                    Num::Node(id) => {
                        if let Some(n) = nodes.node(*id) {
                            layer = layer.bind_opacity(n);
                        }
                    }
                }
                if let Some(mode) = blend_mode(*blend) {
                    layer = layer.mix_blend_mode(mode);
                }
                layer.children(elements(units, nodes, part)).into_element()
            }
        })
        .collect()
}

fn blend_mode(b: BlendMode) -> Option<nsh_core::element::BlendMode> {
    use nsh_core::element::BlendMode as M;
    Some(match b {
        BlendMode::Normal | BlendMode::DestinationIn | BlendMode::DestinationOut => return None,
        BlendMode::Multiply => M::Multiply,
        BlendMode::Screen => M::Screen,
        BlendMode::Overlay => M::Overlay,
        BlendMode::Darken => M::Darken,
        BlendMode::Lighten => M::Lighten,
        BlendMode::ColorDodge => M::ColorDodge,
        BlendMode::ColorBurn => M::ColorBurn,
        BlendMode::HardLight => M::HardLight,
        BlendMode::SoftLight => M::SoftLight,
        BlendMode::Difference => M::Difference,
        BlendMode::Exclusion => M::Exclusion,
        BlendMode::Hue => M::Hue,
        BlendMode::Saturation => M::Saturation,
        BlendMode::Color => M::Color,
        BlendMode::Luminosity => M::Luminosity,
        BlendMode::Add => M::PlusLighter,
    })
}

/// A canvas's operations recorded with a painter.
pub(crate) fn painter(ops: &[Op], nodes: &Nodes) -> Painter {
    let mut p = Painter::new();
    for op in ops {
        let n = |p: &mut Painter, v: Num| match v {
            Num::Const(c) => p.num(c as f32),
            Num::Node(id) => match nodes.node(id) {
                Some(node) => p.num(node),
                None => p.num(0.0f32),
            },
        };
        let recorded = match op {
            Op::Save => CanvasOp::Save,
            Op::Restore => CanvasOp::Restore,
            Op::Translate(x, y) => {
                let (x, y) = (n(&mut p, *x), n(&mut p, *y));
                CanvasOp::Translate(x, y)
            }
            Op::Scale(x, y) => {
                let (x, y) = (n(&mut p, *x), n(&mut p, *y));
                CanvasOp::Scale(x, y)
            }
            Op::Rotate(a) => CanvasOp::Rotate(n(&mut p, *a)),
            Op::Transform(m) => CanvasOp::Transform(m.map(|v| n(&mut p, v))),
            Op::BeginPath => CanvasOp::BeginPath,
            Op::MoveTo(x, y) => {
                let (x, y) = (n(&mut p, *x), n(&mut p, *y));
                CanvasOp::Path(PathOp::MoveTo(x, y))
            }
            Op::LineTo(x, y) => {
                let (x, y) = (n(&mut p, *x), n(&mut p, *y));
                CanvasOp::Path(PathOp::LineTo(x, y))
            }
            Op::QuadTo(v) => CanvasOp::Path(PathOp::QuadTo(v.map(|v| n(&mut p, v)))),
            Op::CubicTo(v) => CanvasOp::Path(PathOp::CubicTo(v.map(|v| n(&mut p, v)))),
            Op::RoundRect(rect, r) => {
                let rect = rect.map(|v| n(&mut p, v));
                let r = n(&mut p, *r);
                CanvasOp::Path(PathOp::RoundRect(rect, [r; 4]))
            }
            Op::Ellipse([cx, cy, rx, ry], ccw) => {
                let geometry = [
                    n(&mut p, *cx),
                    n(&mut p, *cy),
                    n(&mut p, *rx),
                    n(&mut p, *ry),
                    canvas::Num::Const(0.0),
                ];
                CanvasOp::Path(PathOp::Ellipse {
                    geometry,
                    start: canvas::Num::Const(0.0),
                    end: canvas::Num::Const(TAU as f32),
                    ccw: *ccw,
                })
            }
            Op::EllipseArc([cx, cy, rx, ry, start, end], ccw) => CanvasOp::Path(PathOp::Ellipse {
                geometry: [
                    n(&mut p, *cx),
                    n(&mut p, *cy),
                    n(&mut p, *rx),
                    n(&mut p, *ry),
                    canvas::Num::Const(0.0),
                ],
                start: n(&mut p, *start),
                end: n(&mut p, *end),
                ccw: *ccw,
            }),
            Op::Close => CanvasOp::Path(PathOp::Close),
            Op::Fill(rule) => CanvasOp::Fill(None, fill_rule(*rule)),
            Op::Stroke => CanvasOp::Stroke(None),
            Op::Clip(rule) => CanvasOp::Clip(None, fill_rule(*rule)),
            Op::FillColor(c) => CanvasOp::FillStyle(paint(&mut p, *c, nodes)),
            Op::StrokeColor(c) => CanvasOp::StrokeStyle(paint(&mut p, *c, nodes)),
            Op::FillGradient(g) => CanvasOp::FillStyle(CanvasPaint::Gradient(gradient(g))),
            Op::StrokeGradient(g) => CanvasOp::StrokeStyle(CanvasPaint::Gradient(gradient(g))),
            Op::LineWidth(w) => CanvasOp::LineWidth(n(&mut p, *w)),
            Op::LineCap(c) => CanvasOp::LineCap(match c {
                LineCap::Butt => canvas::LineCap::Butt,
                LineCap::Round => canvas::LineCap::Round,
                LineCap::Square => canvas::LineCap::Square,
            }),
            Op::LineJoin(j) => CanvasOp::LineJoin(match j {
                LineJoin::Miter => canvas::LineJoin::Miter,
                LineJoin::Round => canvas::LineJoin::Round,
                LineJoin::Bevel => canvas::LineJoin::Bevel,
            }),
            Op::MiterLimit(m) => CanvasOp::MiterLimit((*m as f32).max(1.0)),
            Op::LineDash(array) => {
                CanvasOp::LineDash(array.iter().map(|d| px(*d as f32)).collect())
            }
            Op::LineDashOffset(o) => CanvasOp::LineDashOffset(n(&mut p, *o)),
            Op::GlobalAlpha(a) => CanvasOp::GlobalAlpha(n(&mut p, *a)),
            Op::Blend(b) => CanvasOp::Composite(composite(*b)),
            Op::Atop => CanvasOp::Composite(Composite::SourceAtop),
            Op::Copy => CanvasOp::Composite(Composite::Copy),
            Op::Shadow {
                color,
                blur,
                offset,
            } => {
                let k = shadow_scale();
                p = p
                    .op(CanvasOp::ShadowColor(Color(*color)))
                    .op(CanvasOp::ShadowBlur(px(*blur as f32 * k)));
                CanvasOp::ShadowOffset(Point::new(px(offset.0 as f32 * k), px(offset.1 as f32 * k)))
            }
            Op::Image(data, rect) => CanvasOp::DrawImage {
                image: CanvasImage { uri: uri(data) },
                source: None,
                dest: [rect.x, rect.y, rect.width, rect.height]
                    .map(|v| canvas::Num::Const(v as f32)),
            },
            Op::Text {
                text,
                font,
                anchor,
                stroke,
            } => {
                let font = canvas::CanvasFont::parse(&font.css()).unwrap_or_default();
                p = p
                    .op(CanvasOp::Font(font))
                    .op(CanvasOp::TextAlign(match anchor {
                        TextAnchor::Start => canvas::CanvasTextAlign::Start,
                        TextAnchor::Middle => canvas::CanvasTextAlign::Center,
                        TextAnchor::End => canvas::CanvasTextAlign::End,
                    }));
                let (text, x, y) = (
                    text.clone(),
                    canvas::Num::Const(0.0),
                    canvas::Num::Const(0.0),
                );
                if *stroke {
                    CanvasOp::StrokeText {
                        text,
                        x,
                        y,
                        max_width: None,
                    }
                } else {
                    CanvasOp::FillText {
                        text,
                        x,
                        y,
                        max_width: None,
                    }
                }
            }
        };
        p = p.op(recorded);
    }
    p
}

/// Points to the units a canvas shadow is measured in. The web canvas sets
/// `shadowBlur` and `shadowOffset` as given, in the canvas' pixels (the
/// transform does not move shadows), where the other platforms take points.
fn shadow_scale() -> f32 {
    #[cfg(target_arch = "wasm32")]
    {
        nsh_core::prelude::pixel_ratio().max(1.0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        1.0
    }
}

fn paint(p: &mut Painter, c: Paint, nodes: &Nodes) -> CanvasPaint {
    match c {
        Paint::Const(rgba) => CanvasPaint::Color(Color(rgba)),
        Paint::Node(id) => match nodes.node(id) {
            Some(node) => p.paint(node),
            None => CanvasPaint::Color(Color(0)),
        },
    }
}

fn gradient(g: &Gradient) -> Arc<CanvasGradient> {
    let pt = |p: dxn_vector_animation::Point| point(px(p.x as f32), px(p.y as f32));
    let (shape, stops) = match g {
        Gradient::Linear { start, end, stops } => (
            GradientShape::Linear {
                start: pt(*start),
                end: pt(*end),
            },
            stops,
        ),
        Gradient::Radial {
            focal,
            center,
            radius,
            stops,
        } => (
            GradientShape::Radial {
                start: pt(*focal),
                start_radius: px(0.0),
                end: pt(*center),
                end_radius: px(*radius as f32),
            },
            stops,
        ),
    };
    Arc::new(CanvasGradient {
        shape,
        stops: stops
            .iter()
            .map(|(offset, rgba)| (Percentage(offset.clamp(0.0, 1.0) as f32), Color(*rgba)))
            .collect(),
    })
}

fn fill_rule(rule: FillRule) -> canvas::FillRule {
    match rule {
        FillRule::NonZero => canvas::FillRule::NonZero,
        FillRule::EvenOdd => canvas::FillRule::EvenOdd,
    }
}

fn composite(b: BlendMode) -> Composite {
    match b {
        BlendMode::Normal => Composite::SourceOver,
        BlendMode::Multiply => Composite::Multiply,
        BlendMode::Screen => Composite::Screen,
        BlendMode::Overlay => Composite::Overlay,
        BlendMode::Darken => Composite::Darken,
        BlendMode::Lighten => Composite::Lighten,
        BlendMode::ColorDodge => Composite::ColorDodge,
        BlendMode::ColorBurn => Composite::ColorBurn,
        BlendMode::HardLight => Composite::HardLight,
        BlendMode::SoftLight => Composite::SoftLight,
        BlendMode::Difference => Composite::Difference,
        BlendMode::Exclusion => Composite::Exclusion,
        BlendMode::Hue => Composite::Hue,
        BlendMode::Saturation => Composite::Saturation,
        BlendMode::Color => Composite::Color,
        BlendMode::Luminosity => Composite::Luminosity,
        BlendMode::Add => Composite::Lighter,
        BlendMode::DestinationIn => Composite::DestinationIn,
        BlendMode::DestinationOut => Composite::DestinationOut,
    }
}

/// How many images' data URIs are kept.
const URIS: usize = 256;

thread_local! {
    /// The data URIs of the images drawn lately: a canvas drawn again (a
    /// raster held, a scene compiled again after a change) draws the same
    /// images, which are encoded once.
    static KEPT: RefCell<Vec<(Weak<ImageData>, Arc<str>)>> = const { RefCell::new(Vec::new()) };
}

/// `data` as a data URI.
fn uri(data: &Arc<ImageData>) -> Arc<str> {
    KEPT.with(|kept| {
        let mut kept = kept.borrow_mut();
        if let Some((_, uri)) = kept
            .iter()
            .find(|(w, _)| std::ptr::eq(w.as_ptr(), Arc::as_ptr(data)))
        {
            return uri.clone();
        }
        use base64::Engine;
        let uri: Arc<str> = format!(
            "data:{};base64,{}",
            data.mime,
            base64::engine::general_purpose::STANDARD.encode(&data.bytes)
        )
        .into();
        kept.retain(|(w, _)| w.strong_count() > 0);
        if kept.len() >= URIS {
            kept.remove(0);
        }
        kept.push((Arc::downgrade(data), uri.clone()));
        uri
    })
}
