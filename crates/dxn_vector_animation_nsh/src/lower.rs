//! A plan made into NSH elements: each canvas's operations recorded with a
//! `Painter` (graph nodes as animated values the canvas reads), each layer
//! a view faded or blended as one.

use std::{f64::consts::TAU, sync::Arc};

use dxn_vector_animation::{BlendMode, FillRule, LineCap, LineJoin};
use nsh_core::{
    Color,
    animation::{Easing as NshEasing, Extrapolate, Output},
    canvas::{
        self, CanvasGradient, CanvasImage, CanvasOp, Composite, GradientShape,
        Paint as CanvasPaint, PathOp,
    },
    geometry::{Percentage, point, px},
    ui::{AnimatedNode, AnimatedValue, Interpolation, prelude::*},
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
                    let b: nsh_core::ui::AnimationOperand = match b {
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
/// the view, over one another; `sampled` makes the canvas of a part drawn
/// frame by frame.
pub(crate) fn elements(
    units: &[Unit],
    nodes: &Nodes,
    sampled: &dyn Fn(&Arc<dxn_vector_animation::Scene>) -> AnyElement,
) -> Vec<AnyElement> {
    units
        .iter()
        .map(|unit| match unit {
            Unit::Canvas(ops) => {
                let painter = painter(ops, nodes);
                canvas(painter).absolute().size_full().into_element()
            }
            Unit::Sampled(part) => sampled(part),
            Unit::Layer {
                opacity,
                blend,
                units,
            } => {
                let mut layer = view().absolute().size_full();
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
                layer
                    .children(elements(units, nodes, sampled))
                    .into_element()
            }
        })
        .collect()
}

fn blend_mode(b: BlendMode) -> Option<nsh_core::ui::BlendMode> {
    use nsh_core::ui::BlendMode as M;
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
            Op::Image(data, rect) => {
                use base64::Engine;
                let uri = format!(
                    "data:{};base64,{}",
                    data.mime,
                    base64::engine::general_purpose::STANDARD.encode(&data.bytes)
                );
                CanvasOp::DrawImage {
                    image: CanvasImage { uri: uri.into() },
                    source: None,
                    dest: [rect.x, rect.y, rect.width, rect.height]
                        .map(|v| canvas::Num::Const(v as f32)),
                }
            }
        };
        p = p.op(recorded);
    }
    p
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
