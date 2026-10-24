//! A scene compiled into what NSH draws: canvases of recorded Canvas 2D
//! operations whose numbers and colors are nodes of the animation graph
//! ([`Graph`]), in layers that fade groups as one. The UI thread then
//! plays the whole scene by moving the graph's time.
//!
//! Each canvas draws in scene coordinates from the view's origin: it starts
//! with the fit of the scene into the view and the setup (transforms,
//! clips) of the groups it draws inside. Group opacity over overlapping
//! content and blend modes make layers (views of their own); masks become
//! clips of their shapes' outlines.

use std::sync::Arc;

use dxn_vector_animation::{
    Animated, BlendMode, Clip, Direction, Fill, FillRule, Frame, FramePaint, Geometry,
    GradientUnits, Group, ImageData, Item, LineCap, LineJoin, Mask, Matrix, MotionPath,
    MotionRotate, Node, Paint as ScenePaint, PaintOrder, Path, PathEl, Point, Rect, Scene, Shape,
    ShapePath, Stroke, TimeMap, TimeRange, TransformOp, Trim, TrimMode,
    outline::{geometry_path, path_length, trim_paths},
    transform::{decompose, point_along, unwrap_angles},
};

use crate::graph::{Curve, Expr, Graph, MathOp, NodeId, Num, Paint, TIME, pack};

/// A rectangle far larger than any scene: what an inverted mask shows
/// outside its shapes.
const HUGE: f64 = 1.0e6;

/// One recorded drawing operation (HTML Canvas 2D semantics: a path is
/// built in device space as its operations come, so a transform set while
/// building it moves its points only).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `save()`.
    Save,
    /// `restore()`.
    Restore,
    /// `translate(x, y)`.
    Translate(Num, Num),
    /// `scale(x, y)`.
    Scale(Num, Num),
    /// `rotate(radians)`.
    Rotate(Num),
    /// `transform(a, b, c, d, e, f)`.
    Transform([Num; 6]),
    /// `beginPath()`.
    BeginPath,
    /// `moveTo(x, y)`.
    MoveTo(Num, Num),
    /// `lineTo(x, y)`.
    LineTo(Num, Num),
    /// `quadraticCurveTo(cpx, cpy, x, y)`.
    QuadTo([Num; 4]),
    /// `bezierCurveTo(cp1x, cp1y, cp2x, cp2y, x, y)`.
    CubicTo([Num; 6]),
    /// `roundRect(x, y, w, h, radius)`.
    RoundRect([Num; 4], Num),
    /// `ellipse(cx, cy, rx, ry, 0, 0, 2π, ccw)`.
    Ellipse([Num; 4], bool),
    /// `closePath()`.
    Close,
    /// `fill(rule)`.
    Fill(FillRule),
    /// `stroke()`.
    Stroke,
    /// `clip(rule)`.
    Clip(FillRule),
    /// `fillStyle = color`.
    FillColor(Paint),
    /// `strokeStyle = color`.
    StrokeColor(Paint),
    /// `fillStyle = gradient`.
    FillGradient(Arc<Gradient>),
    /// `strokeStyle = gradient`.
    StrokeGradient(Arc<Gradient>),
    /// `lineWidth`.
    LineWidth(Num),
    /// `lineCap`.
    LineCap(LineCap),
    /// `lineJoin`.
    LineJoin(LineJoin),
    /// `miterLimit`.
    MiterLimit(f64),
    /// `setLineDash()`.
    LineDash(Vec<f64>),
    /// `lineDashOffset`.
    LineDashOffset(Num),
    /// `globalAlpha`.
    GlobalAlpha(Num),
    /// `globalCompositeOperation` (a blend mode).
    Blend(BlendMode),
    /// `drawImage(image, x, y, w, h)`.
    Image(Arc<ImageData>, Rect),
}

/// A gradient in the coordinates current when it paints.
#[derive(Debug, Clone, PartialEq)]
pub enum Gradient {
    /// Along a line.
    Linear {
        /// Offset 0.
        start: Point,
        /// Offset 1.
        end: Point,
        /// Offsets and colors (`0xRRGGBBAA`).
        stops: Vec<(f64, u32)>,
    },
    /// From a focal point out to a circle.
    Radial {
        /// Offset 0.
        focal: Point,
        /// The circle's center.
        center: Point,
        /// Its radius.
        radius: f64,
        /// Offsets and colors.
        stops: Vec<(f64, u32)>,
    },
}

/// A canvas, or a layer of canvases faded or blended as one.
#[derive(Debug, Clone, PartialEq)]
pub enum Unit {
    /// A canvas, the size of the view.
    Canvas(Vec<Op>),
    /// A view the size of the view over the units below it.
    Layer {
        /// Its opacity.
        opacity: Num,
        /// How it composes with what is under it.
        blend: BlendMode,
        /// What it holds, bottom first.
        units: Vec<Unit>,
    },
}

/// A compiled scene.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The nodes the drawings read.
    pub graph: Graph,
    /// What is drawn, bottom first.
    pub units: Vec<Unit>,
}

/// Why a scene cannot be played on the native graph (it is then sampled
/// frame by frame).
#[derive(Debug, Clone, PartialEq)]
pub struct Unsupported(pub String);

fn unsupported<T>(what: impl Into<String>) -> Result<T, Unsupported> {
    Err(Unsupported(what.into()))
}

/// The layer being filled.
struct Open {
    opacity: Num,
    blend: BlendMode,
    units: Vec<Unit>,
    canvas: Vec<Op>,
    /// The opacity of what is drawn now, within the layer.
    alpha: Num,
}

struct Compiler {
    g: Graph,
    base: Matrix,
    layers: Vec<Open>,
    /// The setup of each group drawn inside, outermost first: what a canvas
    /// starting there replays.
    context: Vec<Vec<Op>>,
}

/// Compiles `scene` drawn through `base` (the scene's fit into the view).
pub fn compile(scene: &Scene, base: Matrix) -> Result<Plan, Unsupported> {
    let mut c = Compiler {
        g: Graph::default(),
        base,
        layers: vec![Open {
            opacity: Num::Const(1.0),
            blend: BlendMode::Normal,
            units: Vec::new(),
            canvas: Vec::new(),
            alpha: Num::Const(1.0),
        }],
        context: Vec::new(),
    };
    c.group(&scene.root, TIME, 0)?;
    c.flush();
    let root = c.layers.pop().map(|l| l.units).unwrap_or_default();
    Ok(Plan {
        graph: c.g,
        units: root,
    })
}

/// A frame (a sampled scene) as constant drawings: what the fallback draws
/// each frame. Layers' opacity multiplies into what they hold; their clips
/// and masks clip.
pub fn from_frame(frame: &Frame, base: Matrix) -> Plan {
    let mut ops = Vec::new();
    let mut alpha = vec![1.0];
    frame_items(&frame.items, base, &mut alpha, &mut ops);
    Plan {
        graph: Graph::default(),
        units: vec![Unit::Canvas(ops)],
    }
}

fn frame_items(items: &[Item], base: Matrix, alpha: &mut Vec<f64>, ops: &mut Vec<Op>) {
    let a = |alpha: &Vec<f64>| Num::Const(*alpha.last().unwrap_or(&1.0));
    for item in items {
        match item {
            Item::Fill {
                path,
                transform,
                paint,
                rule,
            } => {
                ops.push(Op::Save);
                ops.push(Op::Transform(consts(base * *transform)));
                ops.push(Op::BeginPath);
                ops.extend(static_path(path));
                ops.push(Op::GlobalAlpha(a(alpha)));
                match frame_paint(paint) {
                    Ok(p) => ops.push(Op::FillColor(Paint::Const(p))),
                    Err(g) => ops.push(Op::FillGradient(g)),
                }
                ops.push(Op::Fill(*rule));
                ops.push(Op::Restore);
            }
            Item::Stroke {
                path,
                transform,
                paint,
                style,
            } => {
                ops.push(Op::Save);
                ops.push(Op::Transform(consts(base * *transform)));
                ops.push(Op::BeginPath);
                ops.extend(static_path(path));
                ops.push(Op::GlobalAlpha(a(alpha)));
                match frame_paint(paint) {
                    Ok(p) => ops.push(Op::StrokeColor(Paint::Const(p))),
                    Err(g) => ops.push(Op::StrokeGradient(g)),
                }
                ops.push(Op::LineWidth(Num::Const(style.width)));
                ops.push(Op::LineCap(style.cap));
                ops.push(Op::LineJoin(style.join));
                ops.push(Op::MiterLimit(style.miter_limit));
                if let Some((array, offset)) = &style.dash {
                    ops.push(Op::LineDash(array.clone()));
                    ops.push(Op::LineDashOffset(Num::Const(*offset)));
                }
                ops.push(Op::Stroke);
                ops.push(Op::Restore);
            }
            Item::Image {
                data,
                rect,
                transform,
            } => {
                ops.push(Op::Save);
                ops.push(Op::Transform(consts(base * *transform)));
                ops.push(Op::GlobalAlpha(a(alpha)));
                ops.push(Op::Image(data.clone(), *rect));
                ops.push(Op::Restore);
            }
            Item::PushLayer(layer) => {
                ops.push(Op::Save);
                let current = *alpha.last().unwrap_or(&1.0);
                alpha.push(current * layer.opacity);
                if !layer.blend.is_normal() {
                    ops.push(Op::Blend(layer.blend));
                }
                if let Some(clip) = &layer.clip {
                    ops.push(Op::BeginPath);
                    for c in clip {
                        ops.push(Op::Save);
                        ops.push(Op::Transform(consts(base * c.transform)));
                        ops.extend(static_path(&c.path));
                        ops.push(Op::Restore);
                    }
                    ops.push(Op::Clip(clip.first().map_or(FillRule::NonZero, |c| c.rule)));
                }
                if let Some(mask) = &layer.mask {
                    // The mask's filled outlines, as a clip.
                    if mask.inverted {
                        ops.push(Op::BeginPath);
                        ops.push(Op::Save);
                        ops.push(Op::Transform(consts(base)));
                        ops.extend(huge_rect());
                        ops.push(Op::Restore);
                    } else {
                        ops.push(Op::BeginPath);
                    }
                    for i in &mask.items {
                        if let Item::Fill {
                            path, transform, ..
                        } = i
                        {
                            ops.push(Op::Save);
                            ops.push(Op::Transform(consts(base * *transform)));
                            ops.extend(static_path(path));
                            ops.push(Op::Restore);
                        }
                    }
                    ops.push(Op::Clip(if mask.inverted {
                        FillRule::EvenOdd
                    } else {
                        FillRule::NonZero
                    }));
                }
            }
            Item::PopLayer => {
                ops.push(Op::Restore);
                if alpha.len() > 1 {
                    alpha.pop();
                }
            }
        }
    }
}

/// A frame's paint: a color, or the gradient (with its transform folded
/// in only when it is the identity; the fallback draws others flat).
fn frame_paint(paint: &FramePaint) -> Result<u32, Arc<Gradient>> {
    match paint {
        FramePaint::Solid(c) => Ok(pack(*c)),
        FramePaint::Linear {
            start,
            end,
            stops,
            transform,
            ..
        } => Err(Arc::new(Gradient::Linear {
            start: transform.apply(*start),
            end: transform.apply(*end),
            stops: stops.iter().map(|s| (s.offset, pack(s.color))).collect(),
        })),
        FramePaint::Radial {
            center,
            radius,
            focal,
            stops,
            transform,
            ..
        } => {
            let scale = (transform.a.hypot(transform.b) + transform.c.hypot(transform.d)) / 2.0;
            Err(Arc::new(Gradient::Radial {
                focal: transform.apply(*focal),
                center: transform.apply(*center),
                radius: radius * scale,
                stops: stops.iter().map(|s| (s.offset, pack(s.color))).collect(),
            }))
        }
    }
}

fn consts(m: Matrix) -> [Num; 6] {
    [m.a, m.b, m.c, m.d, m.e, m.f].map(Num::Const)
}

fn static_path(path: &Path) -> Vec<Op> {
    let p = |p: Point| (Num::Const(p.x), Num::Const(p.y));
    path.elements()
        .iter()
        .map(|el| match *el {
            PathEl::MoveTo(a) => {
                let (x, y) = p(a);
                Op::MoveTo(x, y)
            }
            PathEl::LineTo(a) => {
                let (x, y) = p(a);
                Op::LineTo(x, y)
            }
            PathEl::QuadTo(c, a) => Op::QuadTo([c.x, c.y, a.x, a.y].map(Num::Const)),
            PathEl::CubicTo(c1, c2, a) => {
                Op::CubicTo([c1.x, c1.y, c2.x, c2.y, a.x, a.y].map(Num::Const))
            }
            PathEl::Close => Op::Close,
        })
        .collect()
}

fn huge_rect() -> Vec<Op> {
    let c = Num::Const;
    vec![
        Op::MoveTo(c(-HUGE), c(-HUGE)),
        Op::LineTo(c(HUGE), c(-HUGE)),
        Op::LineTo(c(HUGE), c(HUGE)),
        Op::LineTo(c(-HUGE), c(HUGE)),
        Op::Close,
    ]
}

impl Compiler {
    fn open(&mut self) -> &mut Open {
        // The root layer is never popped while compiling.
        let last = self.layers.len() - 1;
        &mut self.layers[last]
    }

    /// Adds drawing `ops` to the current canvas, starting it with the fit
    /// and the groups' setup when it is new.
    fn draw(&mut self, ops: Vec<Op>) {
        if self.open().canvas.is_empty() {
            let mut prologue = vec![Op::Transform(consts(self.base))];
            for frame in &self.context {
                prologue.extend(frame.iter().cloned());
            }
            self.open().canvas = prologue;
        }
        self.open().canvas.extend(ops);
    }

    /// Ends the current canvas.
    fn flush(&mut self) {
        let open = self.open();
        if !open.canvas.is_empty() {
            let canvas = std::mem::take(&mut open.canvas);
            open.units.push(Unit::Canvas(canvas));
        }
    }

    fn mul(&mut self, a: Num, b: Num) -> Num {
        self.g.math(MathOp::Mul, a, b)
    }

    fn node(&mut self, n: Num) -> NodeId {
        match n {
            Num::Node(id) => id,
            Num::Const(c) => self.g.add(Expr::Interp {
                input: TIME,
                inputs: vec![0.0],
                outputs: vec![c],
                curve: Curve::Linear,
            }),
        }
    }

    fn group(&mut self, g: &Group, time: NodeId, depth: usize) -> Result<(), Unsupported> {
        if depth > 128 {
            return unsupported("groups nested too deep");
        }
        let visible = match g.active {
            Some(range) => self.visibility(range, time),
            None => Num::Const(1.0),
        };
        if visible.is(0.0) {
            return Ok(());
        }
        let local = match &g.time {
            Some(map) => self.local_time(map, time),
            None => time,
        };
        let mut frame = vec![Op::Save];
        frame.extend(self.transform(&g.transform, local)?);
        if let Some(clip) = &g.clip {
            frame.extend(self.clip(clip, local)?);
        }
        if let Some(mask) = &g.mask {
            frame.extend(self.mask(mask, local, depth)?);
        }
        let opacity = self.g.track(&g.opacity, local, |o| o.clamp(0.0, 1.0));
        let opacity = self.mul(opacity, visible);
        if opacity.is(0.0) {
            return Ok(());
        }
        let isolate = !g.blend.is_normal() || (!opacity.is(1.0) && drawables(g, 2) > 1);
        let outer_alpha = self.open().alpha;
        if isolate {
            self.flush();
            let opacity = self.mul(outer_alpha, opacity);
            self.layers.push(Open {
                opacity,
                blend: g.blend,
                units: Vec::new(),
                canvas: Vec::new(),
                alpha: Num::Const(1.0),
            });
        } else {
            let alpha = self.mul(outer_alpha, opacity);
            self.open().alpha = alpha;
        }
        // The setup goes in the current canvas, or in the next one's prologue.
        if !self.open().canvas.is_empty() {
            self.open().canvas.extend(frame.iter().cloned());
        }
        self.context.push(frame);
        for child in &g.children {
            match child {
                Node::Group(child) => self.group(child, local, depth + 1)?,
                Node::Shape(shape) => self.shape(shape, local)?,
                Node::Image(image) => {
                    let alpha = self.open().alpha;
                    self.draw(vec![
                        Op::Save,
                        Op::GlobalAlpha(alpha),
                        Op::Image(image.data.clone(), image.rect),
                        Op::Restore,
                    ]);
                }
            }
        }
        self.context.pop();
        if !self.open().canvas.is_empty() {
            self.open().canvas.push(Op::Restore);
        }
        if isolate {
            self.flush();
            if let Some(layer) = self.layers.pop() {
                let unit = Unit::Layer {
                    opacity: layer.opacity,
                    blend: layer.blend,
                    units: layer.units,
                };
                self.open().units.push(unit);
            }
        } else {
            self.open().alpha = outer_alpha;
        }
        Ok(())
    }

    /// 1 while `range` holds `time`, else 0.
    fn visibility(&mut self, range: TimeRange, time: NodeId) -> Num {
        const EDGE: f64 = 1e-6;
        let (start, end) = (range.start, range.end);
        let (inputs, outputs) = match (start.is_finite(), end.is_finite()) {
            (true, true) if end > start => (
                vec![start - EDGE, start, end - EDGE, end],
                vec![0.0, 1.0, 1.0, 0.0],
            ),
            (true, true) => return Num::Const(0.0),
            (true, false) => (vec![start - EDGE, start], vec![0.0, 1.0]),
            (false, true) => (vec![end - EDGE, end], vec![1.0, 0.0]),
            (false, false) => return Num::Const(1.0),
        };
        Num::Node(self.g.add(Expr::Interp {
            input: time,
            inputs,
            outputs,
            curve: Curve::Linear,
        }))
    }

    fn local_time(&mut self, map: &TimeMap, time: NodeId) -> NodeId {
        let scale = if map.scale == 0.0 { 1.0 } else { map.scale };
        let shifted = self
            .g
            .math(MathOp::Sub, Num::Node(time), Num::Const(map.offset));
        let local = self.g.math(MathOp::Div, shifted, Num::Const(scale));
        let local = self.node(local);
        match &map.remap {
            Some(remap) => {
                let n = self.g.track(remap, local, |v| v);
                self.node(n)
            }
            None => local,
        }
    }

    /// The canvas operations of a transform list.
    fn transform(&mut self, ops: &[TransformOp], time: NodeId) -> Result<Vec<Op>, Unsupported> {
        let mut out = Vec::new();
        for op in ops {
            match op {
                TransformOp::Translate { x, y } => {
                    let (x, y) = (self.g.track(x, time, |v| v), self.g.track(y, time, |v| v));
                    if !(x.is(0.0) && y.is(0.0)) {
                        out.push(Op::Translate(x, y));
                    }
                }
                TransformOp::Scale { x, y } => {
                    let (x, y) = (self.g.track(x, time, |v| v), self.g.track(y, time, |v| v));
                    if !(x.is(1.0) && y.is(1.0)) {
                        out.push(Op::Scale(x, y));
                    }
                }
                TransformOp::Rotate { degrees } => {
                    let r = self.g.track(degrees, time, f64::to_radians);
                    if !r.is(0.0) {
                        out.push(Op::Rotate(r));
                    }
                }
                TransformOp::Skew { x, y } => {
                    let tan = |a: &Animated<f64>, g: &mut Graph| match a {
                        Animated::Static(v) => Num::Const(v.to_radians().tan()),
                        Animated::Track(t) => {
                            let times: Vec<f64> = t.keyframes.iter().map(|k| k.time).collect();
                            g.sampled(&times, time, |s| a.sample(s).to_radians().tan())
                        }
                    };
                    let (tx, ty) = (tan(x, &mut self.g), tan(y, &mut self.g));
                    out.push(Op::Transform([
                        Num::Const(1.0),
                        ty,
                        tx,
                        Num::Const(1.0),
                        Num::Const(0.0),
                        Num::Const(0.0),
                    ]));
                }
                TransformOp::Matrix { matrix } => match matrix {
                    Animated::Static(m) => out.push(Op::Transform(consts(*m))),
                    Animated::Track(track) => {
                        out.extend(self.animated_matrix(&track.keyframes, time))
                    }
                },
                TransformOp::Motion(motion) => out.extend(self.motion(motion, time)),
            }
        }
        Ok(out)
    }

    /// A matrix track as its decomposition's parts, each a track.
    fn animated_matrix(
        &mut self,
        keys: &[dxn_vector_animation::Keyframe<Matrix>],
        time: NodeId,
    ) -> Vec<Op> {
        let parts: Vec<_> = keys.iter().map(|k| decompose(k.value)).collect();
        let mut angles: Vec<f64> = parts.iter().map(|d| d.angle).collect();
        unwrap_angles(&mut angles);
        let track = |values: Vec<f64>| -> Animated<f64> {
            Animated::Track(dxn_vector_animation::Track::new(
                keys.iter()
                    .zip(values)
                    .map(|(k, v)| dxn_vector_animation::Keyframe::new(k.time, v, k.easing.clone()))
                    .collect(),
            ))
        };
        let tx = self.g.track(
            &track(parts.iter().map(|d| d.translate.0).collect()),
            time,
            |v| v,
        );
        let ty = self.g.track(
            &track(parts.iter().map(|d| d.translate.1).collect()),
            time,
            |v| v,
        );
        let angle = self.g.track(&track(angles), time, f64::to_radians);
        let skew = self
            .g
            .track(&track(parts.iter().map(|d| d.skew).collect()), time, |v| v);
        let sx = self.g.track(
            &track(parts.iter().map(|d| d.scale.0).collect()),
            time,
            |v| v,
        );
        let sy = self.g.track(
            &track(parts.iter().map(|d| d.scale.1).collect()),
            time,
            |v| v,
        );
        let (zero, one) = (Num::Const(0.0), Num::Const(1.0));
        vec![
            Op::Translate(tx, ty),
            Op::Rotate(angle),
            Op::Transform([one, zero, skew, one, zero, zero]),
            Op::Scale(sx, sy),
        ]
    }

    /// A position along a path: the path sampled by length, its points and
    /// directions looked up by the progress.
    fn motion(&mut self, motion: &MotionPath, time: NodeId) -> Vec<Op> {
        let length = path_length(&motion.path);
        let count = ((length / 2.0) as usize).clamp(16, 512);
        let closed = motion.path.elements().last() == Some(&PathEl::Close);
        let mut progress = self.g.track(&motion.progress, time, |v| v);
        if closed {
            progress = self.g.math(MathOp::Mod, progress, Num::Const(1.0));
        }
        let mut inputs = Vec::with_capacity(count + 1);
        let (mut xs, mut ys, mut angles) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..=count {
            let s = i as f64 / count as f64;
            // An open path clamps; sample the closing point of a closed one
            // just before it wraps.
            let at = if closed && i == count { 1.0 - 1e-9 } else { s };
            let Some((p, angle)) = point_along(&motion.path, at) else {
                return Vec::new();
            };
            inputs.push(s);
            xs.push(p.x);
            ys.push(p.y);
            angles.push(angle);
        }
        unwrap_angles(&mut angles);
        let input = self.node(progress);
        let lookup = |g: &mut Graph, outputs: Vec<f64>| {
            Num::Node(g.add(Expr::Interp {
                input,
                inputs: inputs.clone(),
                outputs,
                curve: Curve::Linear,
            }))
        };
        let x = lookup(&mut self.g, xs);
        let y = lookup(&mut self.g, ys);
        let mut out = vec![Op::Translate(x, y)];
        match motion.rotate {
            MotionRotate::Fixed { degrees } if degrees != 0.0 => {
                out.push(Op::Rotate(Num::Const(degrees.to_radians())));
            }
            MotionRotate::Fixed { .. } => {}
            MotionRotate::Auto { offset } => {
                let radians = angles.iter().map(|a| (a + offset).to_radians()).collect();
                let r = lookup(&mut self.g, radians);
                out.push(Op::Rotate(r));
            }
        }
        out
    }

    /// The path operations of an outline (after a `BeginPath`); `None` for
    /// an outline that moves.
    fn geometry(&mut self, geometry: &Geometry, time: NodeId) -> Result<Vec<Op>, Unsupported> {
        if !geometry.is_animated() {
            return Ok(static_path(&geometry_path(geometry, 0.0)));
        }
        match geometry {
            Geometry::Path { path } => {
                let Animated::Track(track) = path else {
                    return Ok(static_path(&geometry_path(geometry, 0.0)));
                };
                let keys = &track.keyframes;
                let Some(first) = keys.first() else {
                    return Ok(Vec::new());
                };
                if keys.iter().any(|k| !k.value.same_structure(&first.value)) {
                    return unsupported("a path morphing between different commands");
                }
                // Each coordinate a track of its own.
                let coords = |p: &Path| -> Vec<f64> {
                    p.elements()
                        .iter()
                        .flat_map(|el| match *el {
                            PathEl::MoveTo(a) | PathEl::LineTo(a) => vec![a.x, a.y],
                            PathEl::QuadTo(c, a) => vec![c.x, c.y, a.x, a.y],
                            PathEl::CubicTo(c1, c2, a) => vec![c1.x, c1.y, c2.x, c2.y, a.x, a.y],
                            PathEl::Close => vec![],
                        })
                        .collect()
                };
                let per_key: Vec<Vec<f64>> = keys.iter().map(|k| coords(&k.value)).collect();
                let mut nums = Vec::with_capacity(per_key[0].len());
                for j in 0..per_key[0].len() {
                    let a = Animated::Track(dxn_vector_animation::Track::new(
                        keys.iter()
                            .zip(&per_key)
                            .map(|(k, c)| {
                                dxn_vector_animation::Keyframe::new(k.time, c[j], k.easing.clone())
                            })
                            .collect(),
                    ));
                    nums.push(self.g.track(&a, time, |v| v));
                }
                let mut it = nums.into_iter();
                let mut next = || it.next().unwrap_or(Num::Const(0.0));
                Ok(first
                    .value
                    .elements()
                    .iter()
                    .map(|el| match el {
                        PathEl::MoveTo(_) => Op::MoveTo(next(), next()),
                        PathEl::LineTo(_) => Op::LineTo(next(), next()),
                        PathEl::QuadTo(..) => Op::QuadTo([next(), next(), next(), next()]),
                        PathEl::CubicTo(..) => {
                            Op::CubicTo([next(), next(), next(), next(), next(), next()])
                        }
                        PathEl::Close => Op::Close,
                    })
                    .collect())
            }
            Geometry::Rect {
                center,
                size,
                radius,
                ..
            } => {
                if radius.x != radius.y {
                    return unsupported("an animated rectangle with elliptical corners");
                }
                let (cx, cy) = (
                    self.g.track(&center.x, time, |v| v),
                    self.g.track(&center.y, time, |v| v),
                );
                let (w, h) = (
                    self.g.track(&size.x, time, |v| v),
                    self.g.track(&size.y, time, |v| v),
                );
                let half_w = self.g.math(MathOp::Mul, w, Num::Const(0.5));
                let half_h = self.g.math(MathOp::Mul, h, Num::Const(0.5));
                let x = self.g.math(MathOp::Sub, cx, half_w);
                let y = self.g.math(MathOp::Sub, cy, half_h);
                let r = self.g.track(&radius.x, time, |v| v.max(0.0));
                Ok(vec![Op::RoundRect([x, y, w, h], r)])
            }
            Geometry::Ellipse {
                center,
                radius,
                direction,
                ..
            } => {
                let c = [
                    self.g.track(&center.x, time, |v| v),
                    self.g.track(&center.y, time, |v| v),
                    self.g.track(&radius.x, time, |v| v.abs()),
                    self.g.track(&radius.y, time, |v| v.abs()),
                ];
                Ok(vec![Op::Ellipse(
                    c,
                    *direction == Direction::CounterClockwise,
                )])
            }
            Geometry::Star(_) => unsupported("an animated star"),
        }
    }

    /// The path operations of a shape's outline, its transform moving its
    /// points only.
    fn shape_path(&mut self, p: &ShapePath, time: NodeId) -> Result<Vec<Op>, Unsupported> {
        let animated = p.transform.iter().any(TransformOp::is_animated);
        if !animated && !p.geometry.is_animated() {
            let m = dxn_vector_animation::transform::transform_at(&p.transform, 0.0);
            return Ok(static_path(&geometry_path(&p.geometry, 0.0).transformed(m)));
        }
        let mut ops = self.geometry(&p.geometry, time)?;
        if !p.transform.is_empty() {
            let mut wrapped = vec![Op::Save];
            wrapped.extend(self.transform(&p.transform, time)?);
            wrapped.append(&mut ops);
            wrapped.push(Op::Restore);
            ops = wrapped;
        }
        Ok(ops)
    }

    fn clip(&mut self, clip: &Clip, time: NodeId) -> Result<Vec<Op>, Unsupported> {
        let mut ops = vec![Op::BeginPath];
        for shape in &clip.shapes {
            ops.push(Op::Save);
            ops.extend(self.transform(&shape.transform, time)?);
            ops.extend(self.geometry(&shape.geometry, time)?);
            ops.push(Op::Restore);
        }
        ops.push(Op::Clip(
            clip.shapes.first().map_or(FillRule::NonZero, |s| s.rule),
        ));
        Ok(ops)
    }

    /// A mask as clips: the outlines its content fills (added, then each
    /// intersected or subtracted group).
    fn mask(&mut self, mask: &Mask, time: NodeId, depth: usize) -> Result<Vec<Op>, Unsupported> {
        let mut added = Vec::new();
        let mut later: Vec<(BlendMode, Vec<Op>)> = Vec::new();
        self.mask_outlines(&mask.content, time, &mut added, &mut later, depth)?;
        let mut ops = vec![Op::BeginPath];
        if mask.inverted {
            ops.extend(huge_rect());
        }
        ops.extend(added);
        ops.push(Op::Clip(if mask.inverted {
            FillRule::EvenOdd
        } else {
            FillRule::NonZero
        }));
        for (blend, outline) in later {
            ops.push(Op::BeginPath);
            if blend == BlendMode::DestinationOut {
                ops.extend(huge_rect());
            }
            ops.extend(outline);
            ops.push(Op::Clip(if blend == BlendMode::DestinationOut {
                FillRule::EvenOdd
            } else {
                FillRule::NonZero
            }));
        }
        Ok(ops)
    }

    fn mask_outlines(
        &mut self,
        g: &Group,
        time: NodeId,
        added: &mut Vec<Op>,
        later: &mut Vec<(BlendMode, Vec<Op>)>,
        depth: usize,
    ) -> Result<(), Unsupported> {
        if depth > 128 {
            return unsupported("masks nested too deep");
        }
        let local = match &g.time {
            Some(map) => self.local_time(map, time),
            None => time,
        };
        let mut own = vec![Op::Save];
        own.extend(self.transform(&g.transform, local)?);
        let target_is_later = matches!(
            g.blend,
            BlendMode::DestinationIn | BlendMode::DestinationOut
        );
        let mut outlines = Vec::new();
        for child in &g.children {
            match child {
                Node::Shape(s) if s.fill.is_some() => {
                    for p in &s.paths {
                        outlines.extend(self.shape_path(p, local)?);
                    }
                }
                Node::Group(child) => {
                    let mut inner = Vec::new();
                    self.mask_outlines(child, local, &mut inner, later, depth + 1)?;
                    outlines.extend(inner);
                }
                _ => {}
            }
        }
        if outlines.is_empty() {
            return Ok(());
        }
        own.extend(outlines);
        own.push(Op::Restore);
        if target_is_later {
            later.push((g.blend, own));
        } else {
            added.extend(own);
        }
        Ok(())
    }

    fn shape(&mut self, s: &Shape, time: NodeId) -> Result<(), Unsupported> {
        let alpha = self.open().alpha;
        let fill = match &s.fill {
            Some(fill) => self.fill(s, fill, time, alpha)?,
            None => Vec::new(),
        };
        let stroke = match &s.stroke {
            Some(stroke) => self.stroke(s, stroke, time, alpha)?,
            None => Vec::new(),
        };
        let (first, second) = match s.paint_order {
            PaintOrder::FillFirst => (fill, stroke),
            PaintOrder::StrokeFirst => (stroke, fill),
        };
        if !first.is_empty() {
            self.draw(first);
        }
        if !second.is_empty() {
            self.draw(second);
        }
        Ok(())
    }

    /// The outline of a shape for its fill: all its paths, trimmed when the
    /// trim does not move.
    fn outline(&mut self, s: &Shape, time: NodeId) -> Result<Vec<Op>, Unsupported> {
        let mut ops = vec![Op::BeginPath];
        match &s.trim {
            None => {
                for p in &s.paths {
                    ops.extend(self.shape_path(p, time)?);
                }
            }
            Some(trim) => {
                let paths = static_paths(s)?;
                if trim.start.is_animated() || trim.end.is_animated() || trim.offset.is_animated() {
                    return unsupported("a moving trim of a fill");
                }
                ops.extend(static_path(&trim_paths(&paths, trim, 0.0)));
            }
        }
        Ok(ops)
    }

    fn fill(
        &mut self,
        s: &Shape,
        fill: &Fill,
        time: NodeId,
        alpha: Num,
    ) -> Result<Vec<Op>, Unsupported> {
        let mut ops = vec![Op::Save];
        ops.extend(self.outline(s, time)?);
        let opacity = self.g.track(&fill.opacity, time, |o| o.clamp(0.0, 1.0));
        let a = self.mul(alpha, opacity);
        ops.push(Op::GlobalAlpha(a));
        match &fill.paint {
            ScenePaint::Solid { color } => {
                ops.push(Op::FillColor(self.g.color_track(color, time, 1.0)));
                ops.push(Op::Fill(fill.rule));
            }
            gradient => {
                let (g, m) = self.gradient(gradient, s)?;
                // The path is built already: a transform now moves the
                // gradient only.
                if m != Matrix::IDENTITY {
                    ops.push(Op::Transform(consts(m)));
                }
                ops.push(Op::FillGradient(g));
                ops.push(Op::Fill(fill.rule));
            }
        }
        ops.push(Op::Restore);
        Ok(ops)
    }

    fn stroke(
        &mut self,
        s: &Shape,
        stroke: &Stroke,
        time: NodeId,
        alpha: Num,
    ) -> Result<Vec<Op>, Unsupported> {
        let opacity = self.g.track(&stroke.opacity, time, |o| o.clamp(0.0, 1.0));
        let a = self.mul(alpha, opacity);
        let width = self.g.track(&stroke.width, time, |w| w.max(0.0));
        if width.is(0.0) {
            return Ok(Vec::new());
        }
        let mut style = vec![
            Op::GlobalAlpha(a),
            Op::LineWidth(width),
            Op::LineCap(stroke.cap),
            Op::LineJoin(stroke.join),
            Op::MiterLimit(stroke.miter_limit),
        ];
        match &stroke.paint {
            ScenePaint::Solid { color } => {
                style.push(Op::StrokeColor(self.g.color_track(color, time, 1.0)))
            }
            gradient => {
                let (g, m) = self.gradient(gradient, s)?;
                if m != Matrix::IDENTITY {
                    return unsupported("a transformed gradient on a stroke");
                }
                style.push(Op::StrokeGradient(g));
            }
        }
        let moving_trim = s.trim.as_ref().is_some_and(|t| {
            t.start.is_animated() || t.end.is_animated() || t.offset.is_animated()
        });
        let mut ops = Vec::new();
        if moving_trim {
            // Each outline as a dash of its own length, moved along it.
            let Some(trim) = s.trim.clone() else {
                return Ok(Vec::new());
            };
            if stroke.dash.is_some() {
                return unsupported("a moving trim of a dashed stroke");
            }
            let paths = static_paths(s)?;
            if paths.len() > 1 && trim.mode == TrimMode::Combined {
                return unsupported("a moving trim across several outlines");
            }
            let offset = self.trim_offset(&trim, time)?;
            for p in paths {
                let length = path_length(&p);
                if length <= 0.0 {
                    continue;
                }
                let dash_offset = self.g.math(MathOp::Mul, offset, Num::Const(length));
                ops.push(Op::Save);
                ops.push(Op::BeginPath);
                ops.extend(static_path(&p));
                ops.extend(style.iter().cloned());
                ops.push(Op::LineDash(vec![length, length]));
                ops.push(Op::LineDashOffset(dash_offset));
                ops.push(Op::Stroke);
                ops.push(Op::Restore);
            }
            return Ok(ops);
        }
        ops.push(Op::Save);
        ops.extend(self.outline(s, time)?);
        ops.extend(style);
        if let Some(dash) = &stroke.dash {
            let mut array = Vec::with_capacity(dash.array.len());
            for d in &dash.array {
                match d {
                    Animated::Static(v) => array.push(v.max(0.0)),
                    Animated::Track(_) => return unsupported("an animated dash pattern"),
                }
            }
            if array.iter().sum::<f64>() > 0.0 {
                ops.push(Op::LineDash(array));
                let offset = self.g.track(&dash.offset, time, |v| v);
                ops.push(Op::LineDashOffset(offset));
            }
        }
        ops.push(Op::Stroke);
        ops.push(Op::Restore);
        Ok(ops)
    }

    /// A moving trim as the offset of a dash pattern `[L, L]`, in lengths
    /// `L`: drawing on (only the end moves) or off (only the start).
    fn trim_offset(&mut self, trim: &Trim, time: NodeId) -> Result<Num, Unsupported> {
        let still = |a: &Animated<f64>, v: f64| a.as_static() == Some(&v);
        match (trim.start.is_animated(), trim.end.is_animated()) {
            (false, true) if still(&trim.start, 0.0) && !trim.offset.is_animated() => {
                let o = trim.offset.as_static().copied().unwrap_or(0.0);
                // Visible from 0 to e: the dash ends at e.
                Ok(self
                    .g
                    .track(&trim.end, time, |e| 1.0 - e.clamp(0.0, 1.0) - o))
            }
            (true, false) if still(&trim.end, 1.0) && !trim.offset.is_animated() => {
                let o = trim.offset.as_static().copied().unwrap_or(0.0);
                Ok(self
                    .g
                    .track(&trim.start, time, |s| 2.0 - s.clamp(0.0, 1.0) - o))
            }
            _ => unsupported("a trim moving at both ends"),
        }
    }

    /// A scene gradient as a canvas gradient and the transform to set for
    /// it after the path is built.
    fn gradient(
        &mut self,
        paint: &ScenePaint,
        s: &Shape,
    ) -> Result<(Arc<Gradient>, Matrix), Unsupported> {
        let stops_of = |stops: &Animated<Vec<dxn_vector_animation::GradientStop>>| match stops {
            Animated::Static(list) => Ok(list.iter().map(|s| (s.offset, pack(s.color))).collect()),
            Animated::Track(_) => unsupported("animated gradient colors"),
        };
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
                if g.start.is_animated() || g.end.is_animated() {
                    return unsupported("a moving gradient");
                }
                let m = space(g.units, g.transform)?;
                Ok((
                    Arc::new(Gradient::Linear {
                        start: g.start.sample(0.0),
                        end: g.end.sample(0.0),
                        stops: stops_of(&g.stops)?,
                    }),
                    m,
                ))
            }
            ScenePaint::RadialGradient(g) => {
                if g.center.is_animated() || g.focal.is_animated() || g.radius.is_animated() {
                    return unsupported("a moving gradient");
                }
                let m = space(g.units, g.transform)?;
                Ok((
                    Arc::new(Gradient::Radial {
                        focal: g.focal.sample(0.0),
                        center: g.center.sample(0.0),
                        radius: g.radius.sample(0.0),
                        stops: stops_of(&g.stops)?,
                    }),
                    m,
                ))
            }
            ScenePaint::Solid { .. } => unsupported("a color as a gradient"),
        }
    }
}

/// A shape's outlines as constant paths (their transforms applied).
fn static_paths(s: &Shape) -> Result<Vec<Path>, Unsupported> {
    s.paths
        .iter()
        .map(|p| {
            if p.geometry.is_animated() || p.transform.iter().any(TransformOp::is_animated) {
                return unsupported("a trim or box of a moving outline");
            }
            let m = dxn_vector_animation::transform::transform_at(&p.transform, 0.0);
            Ok(geometry_path(&p.geometry, 0.0).transformed(m))
        })
        .collect()
}

/// How many of `g`'s descendants draw, counting up to `limit`.
fn drawables(g: &Group, limit: usize) -> usize {
    let mut n = 0;
    for child in &g.children {
        n += match child {
            Node::Group(c) => drawables(c, limit - n.min(limit)),
            Node::Shape(_) | Node::Image(_) => 1,
        };
        if n >= limit {
            return n;
        }
    }
    n
}
