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
    Animated, BlendMode, Clip, Dash, Direction, Fill, FillRule, Frame, FramePaint, Geometry, Group,
    ImageData, Item, Keyframe, LineCap, LineJoin, Mask, Matrix, MotionPath, MotionRotate, Node,
    Paint as ScenePaint, PaintOrder, Path, PathEl, Point, Rect, Scene, Shape, ShapePath, Stroke,
    TimeMap, TimeRange, Track, TransformOp, Trim, TrimMode,
    outline::{geometry_path, path_length, trim_paths},
    transform::{decompose, point_along, unwrap_angles},
};

use crate::graph::{Curve, Expr, Graph, MathOp, NodeId, Num, Paint, STEP, TIME, pack};
use crate::span;

mod mask;
mod paint;
mod shadow;

use paint::Painting;

/// How far out the rectangles that stand for everything reach (what an
/// inverted mask shows outside its shapes, what a paint covers), in the
/// view's points: past any screen, and within what cairo (the Linux
/// canvas) can place, about ±8 million device pixels.
const FAR: f64 = 5.0e4;

/// [`FAR`] in the scene's lengths, drawn through `base`.
/// The coordinates of `p`'s points, in order.
fn coords(p: &Path) -> Vec<f64> {
    let mut out = Vec::with_capacity(p.elements().len() * 6);
    for el in p.elements() {
        match *el {
            PathEl::MoveTo(a) | PathEl::LineTo(a) => out.extend([a.x, a.y]),
            PathEl::QuadTo(c, a) => out.extend([c.x, c.y, a.x, a.y]),
            PathEl::CubicTo(c1, c2, a) => out.extend([c1.x, c1.y, c2.x, c2.y, a.x, a.y]),
            PathEl::Close => {}
        }
    }
    out
}

/// The commands of `p` with the coordinates `nums`, in order.
fn path_ops(p: &Path, nums: Vec<Num>) -> Vec<Op> {
    let mut it = nums.into_iter();
    let mut next = || it.next().unwrap_or(Num::Const(0.0));
    p.elements()
        .iter()
        .map(|el| match el {
            PathEl::MoveTo(_) => Op::MoveTo(next(), next()),
            PathEl::LineTo(_) => Op::LineTo(next(), next()),
            PathEl::QuadTo(..) => Op::QuadTo([next(), next(), next(), next()]),
            PathEl::CubicTo(..) => Op::CubicTo([next(), next(), next(), next(), next(), next()]),
            PathEl::Close => Op::Close,
        })
        .collect()
}

/// Whether `node` holds a group with effects (in its masks too).
pub fn has_effects(node: &Node) -> bool {
    match node {
        Node::Group(g) => {
            !g.effects.is_empty()
                || g.mask
                    .as_ref()
                    .is_some_and(|m| has_effects(&Node::Group(m.content.clone())))
                || g.children.iter().any(has_effects)
        }
        _ => false,
    }
}

/// Whether `scene` draws the same at every time: sampled at its keyframes'
/// span, every frame alike.
pub fn still(scene: &Scene) -> bool {
    let d = scene.timeline.duration.max(0.0);
    let first = scene.frame_at(0.0);
    (1..=16).all(|k| scene.frame_at(d * f64::from(k) / 16.0) == first)
}

/// `scene` drawn through `base` into a `w` × `h` view, wrapped for
/// [`raster`].
pub fn in_view(scene: &Scene, base: Matrix, w: f64, h: f64) -> Scene {
    Scene {
        width: w,
        height: h,
        timeline: scene.timeline.clone(),
        root: Group {
            transform: vec![TransformOp::Matrix {
                matrix: Animated::Static(base),
            }],
            children: vec![Node::Group(scene.root.clone())],
            ..Group::new()
        },
    }
}

/// `view` (a scene [`in_view`]) at `time` as the reference renderer draws
/// it, `ratio` pixels a point: a PNG of where it draws (in whole pixels),
/// and where that is in the view.
pub fn raster(view: &Scene, time: f64, ratio: f64) -> Option<(Arc<ImageData>, Rect)> {
    let frame = view.frame_at(time);
    let area = frame
        .bounds()?
        .intersect(Rect::new(0.0, 0.0, view.width, view.height))?;
    // Out to whole pixels.
    let (x0, y0) = (
        (area.x * ratio).floor() / ratio,
        (area.y * ratio).floor() / ratio,
    );
    let x1 = ((area.x + area.width) * ratio).ceil() / ratio;
    let y1 = ((area.y + area.height) * ratio).ceil() / ratio;
    let region = Rect::new(x0, y0, x1 - x0, y1 - y0);
    let pixmap = dxn_vector_animation_render::render_region(&frame, region, ratio).ok()?;
    Some((
        Arc::new(ImageData {
            mime: "image/png".into(),
            bytes: pixmap.encode_png().ok()?,
        }),
        region,
    ))
}

/// The runs of `track`: its keyframes split where their commands change.
fn runs(track: &Track<Path>) -> Vec<&[Keyframe<Path>]> {
    track
        .keyframes
        .chunk_by(|a, b| a.value.same_structure(&b.value))
        .collect()
}

/// When `track` switches from the commands of one run to the next: where
/// its eased progress between them passes halfway, as the reference
/// switches (found by bisection; at the later keyframe for a hold).
fn switches(track: &Track<Path>) -> Vec<f64> {
    runs(track)
        .windows(2)
        .filter_map(|w| {
            let (a, b) = (w[0].last()?, w[1].first()?);
            let (mut lo, mut hi) = (a.time, b.time);
            for _ in 0..60 {
                let mid = (lo + hi) / 2.0;
                if track.sample(mid).same_structure(&b.value) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            Some(hi)
        })
        .collect()
}

/// The times the paths of `s` switch commands, ascending, those closer
/// than a millisecond one.
fn morph_switches(s: &Shape) -> Vec<f64> {
    let mut all: Vec<f64> = s
        .paths
        .iter()
        .flat_map(|p| match &p.geometry {
            Geometry::Path {
                path: Animated::Track(track),
            } => switches(track),
            _ => Vec::new(),
        })
        .collect();
    all.sort_by(f64::total_cmp);
    all.dedup_by(|b, a| *b - *a < 1e-3);
    all
}

/// The keyframes of `track` with the commands it has from `from` on (from
/// the start when there is no `from`).
fn run_at(track: &Track<Path>, from: Option<f64>) -> Track<Path> {
    let at = from.map_or(0, |t| {
        switches(track).iter().filter(|s| **s <= t + 1e-3).count()
    });
    let runs = runs(track);
    let run = runs.get(at).or(runs.last()).copied().unwrap_or_default();
    Track {
        keyframes: run.to_vec(),
    }
}

/// The keyframe times of everything `geometry`'s outline depends on.
fn key_times(geometry: &Geometry) -> Vec<f64> {
    let mut times = Vec::new();
    let mut add = |a: &Animated<f64>| times.extend(a.keyframes().iter().map(|k| k.time));
    match geometry {
        Geometry::Path { path } => {
            times.extend(path.keyframes().iter().map(|k| k.time));
        }
        Geometry::Rect {
            center,
            size,
            radius,
            ..
        } => {
            for a in [&center.x, &center.y, &size.x, &size.y, &radius.x, &radius.y] {
                add(a);
            }
        }
        Geometry::Ellipse { center, radius, .. } => {
            for a in [&center.x, &center.y, &radius.x, &radius.y] {
                add(a);
            }
        }
        Geometry::Star(star) => {
            for a in [
                &star.center.x,
                &star.center.y,
                &star.points,
                &star.outer_radius,
                &star.inner_radius,
                &star.outer_roundness,
                &star.inner_roundness,
                &star.rotation,
            ] {
                add(a);
            }
        }
    }
    times.sort_by(f64::total_cmp);
    times.dedup();
    times
}

/// How many dashes of a pattern whose lengths move may reach the outline:
/// each is a part the graph moves.
const MOVING_DASHES: usize = 64;

/// A run of a sampled outline's samples of the same commands: from when
/// (none for the first), and the samples.
type Window = (Option<f64>, Vec<(f64, Path)>);

/// How many runs of different commands a sampled outline may have: each
/// is a contour of its own.
const OUTLINE_WINDOWS: usize = 32;

/// How many times a second, at least, a sampled outline is sampled.
const OUTLINE_RATE: f64 = 120.0;

/// The times an outline moving between keyframes at `keys` is sampled at:
/// each segment evenly (at least 24 times, [`OUTLINE_RATE`] a second),
/// and just before its end, where a hold jumps.
fn outline_times(keys: &[f64]) -> Vec<f64> {
    let mut times = Vec::new();
    for w in keys.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = ((b - a) * OUTLINE_RATE).ceil().max(24.0) as usize;
        times.extend((0..n).map(|k| a + (b - a) * k as f64 / n as f64));
        times.push(b - (b - a) * 1e-4);
    }
    times.extend(keys.last());
    times
}

/// The samples (`times`, `values`) without those a straight line between
/// their neighbors kept stands for within `tolerance`.
/// The samples (`times`, `values`) without those a straight line between
/// their neighbors kept stands for within `tolerance`: in one pass, the
/// slopes from the last sample kept that pass every sample since within
/// the tolerance narrowing as it goes.
fn simplified(times: &[f64], values: &[f64], tolerance: f64) -> (Vec<f64>, Vec<f64>) {
    let n = times.len();
    if values.iter().all(|v| (v - values[0]).abs() <= tolerance) {
        return (vec![times[0]], vec![values[0]]);
    }
    let mut keep = vec![0];
    let mut from = 0;
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for i in 1..n.saturating_sub(1) {
        // `i` passed within the tolerance by a line from `from`...
        let dt = times[i] - times[from];
        if dt > 0.0 {
            lo = lo.max((values[i] - tolerance - values[from]) / dt);
            hi = hi.min((values[i] + tolerance - values[from]) / dt);
        }
        // ...on to the sample after it.
        let to = i + 1;
        let span = times[to] - times[from];
        let slope = if span > 0.0 {
            (values[to] - values[from]) / span
        } else {
            f64::NAN
        };
        if !(lo <= slope && slope <= hi) {
            keep.push(i);
            from = i;
            (lo, hi) = (f64::NEG_INFINITY, f64::INFINITY);
        }
    }
    if n > 1 {
        keep.push(n - 1);
    }
    (
        keep.iter().map(|&k| times[k]).collect(),
        keep.iter().map(|&k| values[k]).collect(),
    )
}

fn far(base: Matrix) -> f64 {
    let scale = (base.a * base.d - base.b * base.c).abs().sqrt();
    if scale > 0.0 { FAR / scale } else { FAR }
}

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
    /// `globalCompositeOperation = "source-atop"`: what follows colors
    /// what the canvas holds, keeping its coverage.
    Atop,
    /// `shadowColor`, `shadowBlur` and `shadowOffsetX` / `Y` (in the view's
    /// points, whatever the transform), still.
    Shadow {
        /// Its color (`0xRRGGBBAA`); transparent for none.
        color: u32,
        /// Twice the standard deviation of its blur.
        blur: f64,
        /// How far it falls.
        offset: (f64, f64),
    },
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
    /// A canvas drawing a part of the scene the graph cannot express,
    /// sampled frame by frame: the part within copies of the groups it is
    /// in (their opacity and blending left to the layers it is drawn in).
    Sampled(Arc<Scene>),
    /// Such a part with effects the canvas does not draw (blurs, shadows
    /// that move), drawn as an image the reference renderer makes ([`raster`]):
    /// once where it does not move, frame by frame where it does.
    Raster(Arc<Scene>),
    /// A canvas reaching past the view by `margin` points (left, top,
    /// right, bottom), shown within the view only (what it draws past the
    /// view casts shadows into it); its operations in its own coordinates,
    /// the view's origin at (left, top).
    Wide {
        /// How far past the view it reaches on each side.
        margin: [f64; 4],
        /// Its operations.
        ops: Vec<Op>,
    },
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
    /// Why the parts drawn by [`Unit::Sampled`] are sampled, each reason
    /// once.
    pub sampled: Vec<String>,
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
    /// The groups drawn inside, outermost first, without their children,
    /// and whether each is a layer: what a sampled part is drawn within.
    ancestors: Vec<(Group, bool)>,
    /// The scene's size and timeline, for sampled parts.
    stage: Scene,
    /// Why parts are sampled.
    sampled: Vec<String>,
    /// Above zero while groups are compiled again with their soft masks
    /// clipping hard (their content drew on more than one canvas).
    hard_masks: usize,
    /// While shadows' casters are compiled: whether their silhouette is
    /// all or nothing.
    casting: Option<bool>,
}

/// Where the compiler was before a node: what drawing it is undone to.
struct Mark {
    graph: usize,
    layers: usize,
    units: usize,
    canvas: usize,
    alpha: Num,
    context: usize,
    ancestors: usize,
}

/// `g` without its children.
fn shell(g: &Group) -> Group {
    Group {
        id: g.id.clone(),
        name: g.name.clone(),
        transform: g.transform.clone(),
        opacity: g.opacity.clone(),
        blend: g.blend,
        clip: g.clip.clone(),
        mask: g.mask.clone(),
        effects: g.effects.clone(),
        active: g.active,
        time: g.time.clone(),
        children: Vec::new(),
    }
}

/// `node` added to the group `depth` groups down the last children of `g`:
/// the one a sampled part is in.
fn add_within(g: &mut Group, depth: usize, node: Node) {
    if depth > 0
        && let Some(Node::Group(inner)) = g.children.last_mut()
    {
        add_within(inner, depth - 1, node);
    } else {
        g.children.push(node);
    }
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
        ancestors: Vec::new(),
        stage: Scene {
            width: scene.width,
            height: scene.height,
            timeline: scene.timeline.clone(),
            root: Group::new(),
        },
        sampled: Vec::new(),
        hard_masks: 0,
        casting: None,
    };
    c.group(&scene.root, TIME, 0)?;
    c.flush();
    let root = c.layers.pop().map(|l| l.units).unwrap_or_default();
    Ok(Plan {
        graph: c.g,
        units: root,
        sampled: c.sampled,
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
        units: vec![Unit::Canvas(closed(ops))],
        sampled: Vec::new(),
    }
}

/// `ops` with every `save()` they leave open restored at the end. A canvas
/// starting inside groups replays their setup and ends inside them; the
/// platforms keep a context's state from one drawing to the next (the web
/// one does), so what a drawing leaves saved would clip the next one.
fn closed(mut ops: Vec<Op>) -> Vec<Op> {
    let open = ops.iter().fold(0usize, |depth, op| match op {
        Op::Save => depth + 1,
        Op::Restore => depth.saturating_sub(1),
        _ => depth,
    });
    ops.extend(std::iter::repeat_n(Op::Restore, open));
    ops
}

fn frame_items(items: &[Item], base: Matrix, alpha: &mut Vec<f64>, ops: &mut Vec<Op>) {
    let a = |alpha: &Vec<f64>| Num::Const(*alpha.last().unwrap_or(&1.0));
    // Inside a layer whose mask shows nothing: how deep, while skipping.
    let mut hidden = 0usize;
    for item in items {
        if hidden > 0 {
            match item {
                Item::PushLayer(_) => hidden += 1,
                Item::PopLayer => hidden -= 1,
                _ => {}
            }
            continue;
        }
        if let Item::PushLayer(layer) = item
            && layer.mask.as_ref().is_some_and(|m| {
                !m.inverted && !m.items.iter().any(|i| matches!(i, Item::Fill { .. }))
            })
        {
            // Its clip would be empty, which not every platform honours.
            hidden = 1;
            continue;
        }
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
                        ops.extend(huge_rect(far(base)));
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

/// A square reaching `h` every way: everything.
fn huge_rect(h: f64) -> Vec<Op> {
    let c = Num::Const;
    vec![
        Op::MoveTo(c(-h), c(-h)),
        Op::LineTo(c(h), c(-h)),
        Op::LineTo(c(h), c(h)),
        Op::LineTo(c(-h), c(h)),
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
            open.units.push(Unit::Canvas(closed(canvas)));
        }
    }

    /// Where it is now.
    fn mark(&mut self) -> Mark {
        let (layers, graph, context, ancestors) = (
            self.layers.len(),
            self.g.len(),
            self.context.len(),
            self.ancestors.len(),
        );
        let open = self.open();
        Mark {
            graph,
            layers,
            units: open.units.len(),
            canvas: open.canvas.len(),
            alpha: open.alpha,
            context,
            ancestors,
        }
    }

    /// Undoes what was drawn since `mark`.
    fn undo(&mut self, mark: Mark) {
        self.g.truncate(mark.graph);
        self.layers.truncate(mark.layers);
        self.context.truncate(mark.context);
        self.ancestors.truncate(mark.ancestors);
        let open = self.open();
        if open.units.len() > mark.units {
            // The canvas then open was flushed: it is the first unit since.
            let first = open.units.drain(mark.units..).next();
            open.canvas = match first {
                Some(Unit::Canvas(mut ops)) if mark.canvas > 0 => {
                    ops.truncate(mark.canvas);
                    ops
                }
                _ => Vec::new(),
            };
        } else {
            open.canvas.truncate(mark.canvas);
        }
        open.alpha = mark.alpha;
    }

    /// `node` sampled frame by frame, within the groups it is in: a canvas
    /// of its own, or the one of `joins` (the index of the sampled part of a
    /// sibling just before it, nothing drawn between). Returns the index of
    /// the part.
    fn sample(&mut self, node: &Node, joins: Option<usize>) -> usize {
        let depth = self.ancestors.len();
        let raster = has_effects(node);
        let open = self.open();
        if let Some(i) = joins
            && i + 1 == open.units.len()
            && open.canvas.is_empty()
            && let Some(Unit::Sampled(scene) | Unit::Raster(scene)) = open.units.get(i)
            && matches!(open.units[i], Unit::Raster(_)) == raster
        {
            let mut scene = scene.clone();
            add_within(&mut Arc::make_mut(&mut scene).root, depth - 1, node.clone());
            open.units[i] = if raster {
                Unit::Raster(scene)
            } else {
                Unit::Sampled(scene)
            };
            return i;
        }
        // The layers it is drawn in fade and blend it already.
        let layered = self.ancestors.iter().rposition(|(_, layer)| *layer);
        let mut part = node.clone();
        for (i, (group, _)) in self.ancestors.iter().enumerate().rev() {
            let mut group = group.clone();
            if layered.is_some_and(|l| i <= l) {
                group.opacity = Animated::Static(1.0);
                group.blend = BlendMode::Normal;
            }
            group.children = vec![part];
            part = Node::Group(group);
        }
        let root = match part {
            Node::Group(g) => g,
            other => Group {
                children: vec![other],
                ..Group::new()
            },
        };
        self.flush();
        let scene = Scene {
            root,
            ..self.stage.clone()
        };
        let open = self.open();
        open.units.push(if raster {
            Unit::Raster(Arc::new(scene))
        } else {
            Unit::Sampled(Arc::new(scene))
        });
        open.units.len() - 1
    }

    /// [`FAR`] in the scene's lengths.
    fn far(&self) -> f64 {
        far(self.base)
    }

    /// How far drawn geometry may stray, in scene lengths: a twentieth of
    /// a point in the view.
    fn tolerance(&self) -> f64 {
        let scale = (self.base.a * self.base.d - self.base.b * self.base.c)
            .abs()
            .sqrt();
        if scale > 0.0 { 0.05 / scale } else { 0.05 }
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
        if let Some(hard) = self.casting {
            if !g.effects.is_empty() {
                return unsupported("a shadow of a shadow");
            }
            return self.cast_group(g, time, depth, hard);
        }
        let start = self.mark();
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
        let shadows = self.canvas_shadows(g, local)?;
        let mut frame = vec![Op::Save];
        frame.extend(self.transform(&g.transform, local)?);
        if let Some(clip) = &g.clip {
            frame.extend(self.clip(clip, local)?);
        }
        let soft = match &g.mask {
            Some(mask) => self.soft_mask(mask, local, depth)?,
            None => None,
        };
        // A mask that shows nothing anywhere: nothing of the group to draw
        // (an empty clip is not honoured everywhere).
        if soft.as_ref().is_some_and(mask::SoftMask::hides_all) {
            return Ok(());
        }
        if let Some(mask) = &g.mask
            && soft.is_none()
        {
            let ops = self.mask(mask, local, depth)?;
            if matches!(ops.as_slice(), [Op::BeginPath, Op::Clip(_), ..]) {
                return Ok(());
            }
            frame.extend(ops);
        }
        let opacity = self.g.track(&g.opacity, local, |o| o.clamp(0.0, 1.0));
        let opacity = self.mul(opacity, visible);
        if opacity.is(0.0) {
            return Ok(());
        }
        let isolate = !g.blend.is_normal()
            || (!opacity.is(1.0) && drawables(g, 2) > 1)
            || !shadows.is_empty();
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
            // A soft mask takes away from the canvas: the content's own.
            if soft.is_some() {
                self.flush();
            }
        }
        let units = self.open().units.len();
        // The setup goes in the current canvas, or in the next one's prologue.
        if !self.open().canvas.is_empty() {
            self.open().canvas.extend(frame.iter().cloned());
        }
        self.context.push(frame);
        self.ancestors.push((shell(g), isolate));
        // The part sampled last, while nothing is drawn after it: where a
        // sibling sampled next joins it.
        let mut last: Option<usize> = None;
        for child in &g.children {
            let mark = self.mark();
            let done = self.child(child, local, depth);
            // What the graph cannot express is sampled, that part alone.
            if let Err(Unsupported(why)) = done {
                self.undo(mark);
                if !self.sampled.contains(&why) {
                    self.sampled.push(why);
                }
                last = Some(self.sample(child, last));
            } else if self.open().units.len() > mark.units || self.open().canvas.len() > mark.canvas
            {
                last = None;
            }
        }
        if let Some(soft) = &soft {
            if self.open().units.len() != units {
                // The content drew on more than one canvas: the mask clips
                // hard instead.
                self.undo(start);
                self.hard_masks += 1;
                let done = self.group(g, time, depth);
                self.hard_masks -= 1;
                return done;
            }
            if !self.open().canvas.is_empty() {
                let ops = self.soft_mask_ops(soft);
                self.open().canvas.extend(ops);
            }
        }
        if !shadows.is_empty() {
            // Cast by one canvas of the content, drawn as it is.
            let content = closed(self.open().canvas.clone());
            let composited = content
                .iter()
                .any(|op| matches!(op, Op::Blend(_) | Op::Atop | Op::Shadow { .. }));
            if self.open().units.len() != units || composited {
                self.undo(start);
                return unsupported("a drop shadow of more than one canvas");
            }
            if !content.is_empty() {
                // The canvas starts with the fit and the setup of the groups
                // it is in, this one's last.
                let outer = 1 + self.context.iter().map(Vec::len).sum::<usize>();
                match self.shadow_units(g, local, depth, &content, outer, shadows) {
                    Ok(list) => {
                        for (i, unit) in list.into_iter().enumerate() {
                            self.open().units.insert(units + i, unit);
                        }
                    }
                    Err(why) => {
                        self.undo(start);
                        return Err(why);
                    }
                }
            }
        }
        self.ancestors.pop();
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

    /// A child of a group at `depth` drawn at `time` (what is unsupported
    /// is left to the group).
    fn child(&mut self, child: &Node, time: NodeId, depth: usize) -> Result<(), Unsupported> {
        match child {
            Node::Group(child) => self.group(child, time, depth + 1),
            Node::Shape(shape) => self.shape(shape, time),
            Node::Image(image) => {
                let alpha = self.open().alpha;
                let alpha = match self.casting {
                    Some(hard) => self.hardened(alpha, hard),
                    None => alpha,
                };
                self.draw(vec![
                    Op::Save,
                    Op::GlobalAlpha(alpha),
                    Op::Image(image.data.clone(), image.rect),
                    Op::Restore,
                ]);
                Ok(())
            }
        }
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
                Ok(path_ops(&first.value, nums))
            }
            Geometry::Rect {
                center,
                size,
                radius,
                ..
            } => {
                if radius.x != radius.y {
                    return self.sampled_outline(geometry, time);
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
            Geometry::Star(_) => self.sampled_outline(geometry, time),
        }
    }

    /// The outline of `geometry`, its points sampled over its keyframes'
    /// times, each coordinate a lookup of the time: for outlines whose
    /// points move as the graph does not compute (stars, elliptical
    /// corners). Where its commands change (a star gaining points), each
    /// run of the same commands is a contour of its own, moved far out of
    /// sight outside its time.
    fn sampled_outline(
        &mut self,
        geometry: &Geometry,
        time: NodeId,
    ) -> Result<Vec<Op>, Unsupported> {
        let times = outline_times(&key_times(geometry));
        let paths: Vec<Path> = times.iter().map(|&t| geometry_path(geometry, t)).collect();
        if paths.is_empty() {
            return Ok(static_path(&geometry_path(geometry, 0.0)));
        }
        // Runs of samples of the same commands: from when, and the samples
        // (the switch's ends found by bisection, the run's first and last).
        let mut windows: Vec<Window> = Vec::new();
        for (i, (&t, path)) in times.iter().zip(&paths).enumerate() {
            match windows.last_mut() {
                Some((_, run)) if run.last().is_some_and(|(_, p)| p.same_structure(path)) => {
                    run.push((t, path.clone()));
                }
                _ if i == 0 => windows.push((None, vec![(t, path.clone())])),
                _ => {
                    let before = &paths[i - 1];
                    let (mut lo, mut hi) = (times[i - 1], t);
                    for _ in 0..60 {
                        let mid = (lo + hi) / 2.0;
                        if geometry_path(geometry, mid).same_structure(before) {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    if let Some((_, run)) = windows.last_mut() {
                        run.push((lo, geometry_path(geometry, lo)));
                    }
                    let first = geometry_path(geometry, hi);
                    let mut run = vec![(hi, first)];
                    if t > hi {
                        run.push((t, path.clone()));
                    }
                    windows.push((Some(hi), run));
                }
            }
        }
        if windows.len() > OUTLINE_WINDOWS {
            return unsupported("an outline whose commands change many times");
        }
        let tolerance = self.tolerance();
        let far = self.far();
        let mut ops = Vec::new();
        for w in 0..windows.len() {
            let (from, run) = &windows[w];
            let to = windows.get(w + 1).and_then(|next| next.0);
            let ts: Vec<f64> = run.iter().map(|(t, _)| *t).collect();
            let columns: Vec<Vec<f64>> = run.iter().map(|(_, p)| coords(p)).collect();
            let mut nums = Vec::with_capacity(columns[0].len());
            for j in 0..columns[0].len() {
                let values: Vec<f64> = columns.iter().map(|c| c[j]).collect();
                let (inputs, outputs) = simplified(&ts, &values, tolerance);
                nums.push(match outputs.as_slice() {
                    [one] => Num::Const(*one),
                    _ => self.g.lookup(Num::Node(time), &inputs, &outputs),
                });
            }
            let contour = path_ops(&run[0].1, nums);
            if windows.len() == 1 {
                return Ok(contour);
            }
            // The path takes its points where the transform puts them as
            // they come.
            let shown = self.g.window(Num::Node(time), *from, to);
            let away = self.g.math(MathOp::Mul, shown, Num::Const(-far));
            let away = self.g.math(MathOp::Add, away, Num::Const(far));
            ops.push(Op::Save);
            ops.push(Op::Translate(away, Num::Const(0.0)));
            ops.extend(contour);
            ops.push(Op::Restore);
        }
        Ok(ops)
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
            ops.extend(huge_rect(self.far()));
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
                ops.extend(huge_rect(self.far()));
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
        let switches = morph_switches(s);
        if !switches.is_empty() {
            return self.shape_windows(s, &switches, time);
        }
        // A shadow's caster: its silhouette.
        let owned;
        let s = match self.casting {
            Some(hard) => {
                owned = shadow::silhouette(s, hard);
                &owned
            }
            None => s,
        };
        let alpha = self.open().alpha;
        let mut fill = match &s.fill {
            Some(fill) => self.fill(s, fill, time, alpha)?,
            None => (Vec::new(), false),
        };
        let mut stroke = match &s.stroke {
            Some(stroke) => self.stroke(s, stroke, time, alpha)?,
            None => (Vec::new(), false),
        };
        if let Some(hard) = self.casting {
            for (ops, alone) in [&mut fill, &mut stroke] {
                if *alone {
                    return unsupported("a shadow of a paint of its own canvas");
                }
                for op in ops.iter_mut() {
                    if let Op::GlobalAlpha(a) = op {
                        *a = self.hardened(*a, hard);
                    }
                }
            }
        }
        let (first, second) = match s.paint_order {
            PaintOrder::FillFirst => (fill, stroke),
            PaintOrder::StrokeFirst => (stroke, fill),
        };
        for (ops, alone) in [first, second] {
            if ops.is_empty() {
                continue;
            }
            // A paint that adds gradients up draws on a canvas of its own.
            if alone {
                self.flush();
                self.draw(ops);
                self.flush();
            } else {
                self.draw(ops);
            }
        }
        Ok(())
    }

    /// A shape whose paths morph between different commands, which switch
    /// halfway (as the reference morphs them) at `switches`: a copy for
    /// each time between switches, shown only then, each path's keyframes
    /// there those of the commands it has then.
    fn shape_windows(
        &mut self,
        s: &Shape,
        switches: &[f64],
        time: NodeId,
    ) -> Result<(), Unsupported> {
        let outer = self.open().alpha;
        for w in 0..=switches.len() {
            let from = w.checked_sub(1).map(|i| switches[i]);
            let to = switches.get(w).copied();
            let shown = self.g.window(Num::Node(time), from, to);
            let mut copy = s.clone();
            for p in &mut copy.paths {
                if let Geometry::Path {
                    path: Animated::Track(track),
                } = &mut p.geometry
                {
                    *track = run_at(track, from);
                }
            }
            let alpha = self.mul(outer, shown);
            self.open().alpha = alpha;
            let done = self.shape(&copy, time);
            self.open().alpha = outer;
            done?;
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

    /// A shape's fill, and whether it needs a canvas of its own.
    fn fill(
        &mut self,
        s: &Shape,
        fill: &Fill,
        time: NodeId,
        alpha: Num,
    ) -> Result<(Vec<Op>, bool), Unsupported> {
        let mut ops = vec![Op::Save];
        ops.extend(self.outline(s, time)?);
        let opacity = self.g.track(&fill.opacity, time, |o| o.clamp(0.0, 1.0));
        let a = self.mul(alpha, opacity);
        let mut alone = false;
        match &fill.paint {
            ScenePaint::Solid { color } => {
                ops.push(Op::GlobalAlpha(a));
                ops.push(Op::FillColor(self.g.color_track(color, time, 1.0)));
                ops.push(Op::Fill(fill.rule));
            }
            gradient => match self.painting(gradient, s, time)? {
                Painting::One { gradient, m, place } => {
                    ops.push(Op::GlobalAlpha(a));
                    // The path is built already: a transform now moves the
                    // gradient only.
                    if m != Matrix::IDENTITY {
                        ops.push(Op::Transform(consts(m)));
                    }
                    if let Some(place) = place {
                        ops.push(Op::Transform(place));
                    }
                    ops.push(Op::FillGradient(gradient));
                    ops.push(Op::Fill(fill.rule));
                }
                painting => {
                    if painting.masked() {
                        // The shape once, colored over by the paint.
                        ops.push(Op::GlobalAlpha(a));
                        ops.push(Op::FillColor(Paint::Const(0x0000_00ff)));
                        ops.push(Op::Fill(fill.rule));
                    } else {
                        ops.push(Op::Clip(fill.rule));
                    }
                    ops.extend(self.plane(&painting, a));
                    alone = painting.alone();
                }
            },
        }
        ops.push(Op::Restore);
        Ok((ops, alone))
    }

    /// A shape's stroke, and whether it needs a canvas of its own.
    fn stroke(
        &mut self,
        s: &Shape,
        stroke: &Stroke,
        time: NodeId,
        alpha: Num,
    ) -> Result<(Vec<Op>, bool), Unsupported> {
        let opacity = self.g.track(&stroke.opacity, time, |o| o.clamp(0.0, 1.0));
        let a = self.mul(alpha, opacity);
        let width = self.g.track(&stroke.width, time, |w| w.max(0.0));
        if width.is(0.0) {
            return Ok((Vec::new(), false));
        }
        let mut style = vec![
            Op::LineWidth(width),
            Op::LineCap(stroke.cap),
            Op::LineJoin(stroke.join),
            Op::MiterLimit(stroke.miter_limit),
        ];
        match &stroke.paint {
            ScenePaint::Solid { color } => {
                style.push(Op::StrokeColor(self.g.color_track(color, time, 1.0)))
            }
            gradient => match self.painting(gradient, s, time)? {
                Painting::One {
                    gradient,
                    m,
                    place: None,
                } if m == Matrix::IDENTITY => style.push(Op::StrokeGradient(gradient)),
                // A transform set for the gradient would scale the stroke.
                painting => return self.painted_stroke(s, stroke, &painting, a),
            },
        }
        if let Some(trim) = &s.trim
            && (trim.start.is_animated() || trim.end.is_animated() || trim.offset.is_animated())
        {
            if stroke.dash.is_some() {
                return unsupported("a moving trim of a dashed stroke");
            }
            return Ok((self.moving_trim(s, trim, time, a, &style)?, false));
        }
        if let Some(dash) = &stroke.dash
            && dash.array.iter().any(Animated::is_animated)
        {
            if s.trim.is_some() {
                return unsupported("an animated dash pattern of a trimmed stroke");
            }
            return Ok((self.moving_dash(s, dash, time, a, &style)?, false));
        }
        let mut ops = vec![Op::Save];
        ops.extend(self.outline(s, time)?);
        ops.push(Op::GlobalAlpha(a));
        ops.extend(style);
        if let Some(dash) = &stroke.dash {
            let array: Vec<f64> = dash
                .array
                .iter()
                .map(|d| d.as_static().copied().unwrap_or(0.0).max(0.0))
                .collect();
            if array.iter().sum::<f64>() > 0.0 {
                ops.push(Op::LineDash(array));
                let offset = self.g.track(&dash.offset, time, |v| v);
                ops.push(Op::LineDashOffset(offset));
            }
        }
        ops.push(Op::Stroke);
        ops.push(Op::Restore);
        Ok((ops, false))
    }

    /// A trim that moves, as the reference trims: from the smaller of start
    /// and end to the larger (each within 0..1), both moved by the offset
    /// and wrapping around, in lengths along each outline (each path's, or
    /// all paths' one after another).
    fn moving_trim(
        &mut self,
        s: &Shape,
        trim: &Trim,
        time: NodeId,
        alpha: Num,
        style: &[Op],
    ) -> Result<Vec<Op>, Unsupported> {
        let paths = static_paths(s)?;
        let groups: Vec<Vec<&Path>> = match trim.mode {
            TrimMode::EachPath => paths.iter().map(|p| vec![p]).collect(),
            TrimMode::Combined => vec![paths.iter().collect()],
        };
        let times = sample_times(&[&trim.start, &trim.end, &trim.offset]);
        let part = |t: f64| {
            let (a, b) = (trim.start.sample(t), trim.end.sample(t));
            let (lo, hi) = (a.min(b).clamp(0.0, 1.0), a.max(b).clamp(0.0, 1.0));
            let o = trim.offset.sample(t);
            (lo + o, hi + o)
        };
        let mut parts: Vec<(f64, f64)> = times.iter().map(|&t| part(t)).collect();
        let start = self.g.track(&trim.start, time, |v| v);
        let end = self.g.track(&trim.end, time, |v| v);
        let ordered = times.iter().all(|&t| {
            let (a, b) = (trim.start.sample(t), trim.end.sample(t));
            a <= b && (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b)
        });
        let (lo, hi) = if ordered {
            (start, end)
        } else {
            // min and max, each within 0..1.
            let d = self.g.math(MathOp::Sub, start, end);
            let over = self.g.above(d, 0.0);
            let lo = self.g.math(MathOp::Sub, start, over);
            let hi = self.g.math(MathOp::Add, end, over);
            let unit = |g: &mut Graph, v: Num| g.lookup(v, &[0.0, 1.0], &[0.0, 1.0]);
            (unit(&mut self.g, lo), unit(&mut self.g, hi))
        };
        let offset = self.g.track(&trim.offset, time, |v| v);
        let mut from = self.g.math(MathOp::Add, lo, offset);
        let length = self.g.math(MathOp::Sub, hi, lo);
        // Past the end the part goes on from the start: the outline twice.
        let inside = parts.iter().all(|&(f, _)| (0.0..=1.0).contains(&f));
        let wraps = !inside || parts.iter().any(|&(_, t)| t > 1.0);
        if !inside {
            from = self.g.math(MathOp::Mod, from, Num::Const(1.0));
            for (f, t) in &mut parts {
                let wrapped = f.rem_euclid(1.0);
                (*f, *t) = (wrapped, wrapped + (*t - *f));
            }
        }
        let to = self.g.math(MathOp::Add, from, length);
        let mut ops = Vec::new();
        for group in groups {
            let mut runs = span::runs(&group);
            let total = runs.last().map_or(0.0, |r| r.end);
            if total <= 0.0 {
                continue;
            }
            if wraps {
                runs = span::doubled(&runs, total);
            }
            let a = self.g.math(MathOp::Mul, from, Num::Const(total));
            let b = self.g.math(MathOp::Mul, to, Num::Const(total));
            let ends: Vec<(f64, f64)> =
                parts.iter().map(|&(f, t)| (f * total, t * total)).collect();
            ops.extend(self.parts(&runs, (a, b), &ends, false, alpha, style));
        }
        Ok(ops)
    }

    /// A dash pattern whose lengths move: on each contour, each dash of
    /// each repetition of the pattern that reaches it, a part from where
    /// the dash starts (`-offset` and the lengths before it) to where it
    /// ends, both moved by the graph.
    fn moving_dash(
        &mut self,
        s: &Shape,
        dash: &Dash,
        time: NodeId,
        alpha: Num,
        style: &[Op],
    ) -> Result<Vec<Op>, Unsupported> {
        // An odd list repeats to an even one.
        let pattern: Vec<&Animated<f64>> = if dash.array.len() % 2 == 1 {
            dash.array.iter().chain(&dash.array).collect()
        } else {
            dash.array.iter().collect()
        };
        let paths = static_paths(s)?;
        let contours: Vec<span::Run> = paths.iter().flat_map(span::contours).collect();
        let longest = contours.iter().map(|r| r.end - r.start).fold(0.0, f64::max);
        let mut tracks = pattern.clone();
        tracks.push(&dash.offset);
        let times = sample_times(&tracks);
        // Where dash `k` of repetition `c` starts and ends at `t`.
        let place = |t: f64, c: i64, k: usize| {
            let lengths: Vec<f64> = pattern.iter().map(|a| a.sample(t).max(0.0)).collect();
            let period: f64 = lengths.iter().sum();
            let a = c as f64 * period + lengths[..k].iter().sum::<f64>() - dash.offset.sample(t);
            (a, a + lengths[k])
        };
        // The dashes that reach an outline at some time.
        let mut dashes = std::collections::BTreeSet::new();
        for &t in &times {
            let period: f64 = pattern.iter().map(|a| a.sample(t).max(0.0)).sum();
            if period <= 0.0 {
                return unsupported("an animated dash pattern of no length");
            }
            let o = dash.offset.sample(t);
            let first = ((o - period) / period).floor() as i64;
            let last = ((longest + o) / period).ceil() as i64;
            for c in first..=last {
                for k in (0..pattern.len()).step_by(2) {
                    let (a, b) = place(t, c, k);
                    if b >= 0.0 && a <= longest {
                        dashes.insert((c, k));
                    }
                }
            }
            if dashes.len() > MOVING_DASHES {
                return unsupported("an animated dash pattern of many dashes along the outline");
            }
        }
        let lengths: Vec<Num> = pattern
            .iter()
            .map(|a| self.g.track(a, time, |v| v.max(0.0)))
            .collect();
        let period = self.g.sum(&lengths);
        let offset = self.g.track(&dash.offset, time, |o| -o);
        let mut ops = Vec::new();
        for (c, k) in dashes {
            let before = self.g.sum(&lengths[..k]);
            let repeats = self.g.math(MathOp::Mul, period, Num::Const(c as f64));
            let start = self.g.math(MathOp::Add, repeats, before);
            let a = self.g.math(MathOp::Add, start, offset);
            let b = self.g.math(MathOp::Add, a, lengths[k]);
            let ends: Vec<(f64, f64)> = times.iter().map(|&t| place(t, c, k)).collect();
            ops.extend(self.parts(&contours, (a, b), &ends, true, alpha, style));
        }
        Ok(ops)
    }

    /// The stroke of the parts of `runs` from `a` to `b` (`ends`: their
    /// values at sampled times), one path of them all. A run the part
    /// always covers is still; a lone run where only one end of the part
    /// moves is a dash pattern moved along it; else each part's points
    /// move, and a part off its run moves far out of sight (`dots`: a part
    /// of no length that touches its run stays, as a dash pattern draws
    /// its caps).
    fn parts(
        &mut self,
        runs: &[span::Run],
        (a, b): (Num, Num),
        ends: &[(f64, f64)],
        dots: bool,
        alpha: Num,
        style: &[Op],
    ) -> Vec<Op> {
        let (a_min, a_max) = min_max(ends.iter().map(|e| e.0));
        let (b_min, b_max) = min_max(ends.iter().map(|e| e.1));
        let on = |run: &span::Run, a: f64, b: f64| {
            if dots {
                b >= run.start && a <= run.end
            } else {
                b.min(run.end) - a.max(run.start) > STEP
            }
        };
        let live: Vec<&span::Run> = runs
            .iter()
            .filter(|r| ends.iter().any(|&(a, b)| on(r, a, b)))
            .collect();
        if live.is_empty() {
            return Vec::new();
        }
        let mut path = vec![Op::BeginPath];
        if let [run] = live.as_slice()
            && (a_max <= run.start) != (b_min >= run.end)
        {
            // A dash as long as needed, ending at b or starting at a.
            let (start, end) = (run.start, run.end);
            let margin = 0.1 * (end - start) + 1.0;
            let (dash, offset) = if a_max <= start {
                let m = (b_max - start).max(end - b_min).max(end - start) + margin;
                (m, self.g.math(MathOp::Sub, Num::Const(m + start), b))
            } else {
                let m = (end - a_min).max(a_max - start).max(end - start) + margin;
                (m, self.g.math(MathOp::Sub, Num::Const(start), a))
            };
            path.extend(run.ops());
            path.push(Op::LineDash(vec![dash, dash]));
            path.push(Op::LineDashOffset(offset));
        } else {
            for run in live {
                if a_max <= run.start && b_min >= run.end {
                    path.extend(run.ops());
                    continue;
                }
                let tolerance = self.tolerance();
                let part = run.span(&mut self.g, a, b, tolerance);
                if ends.iter().all(|&(a, b)| on(run, a, b)) {
                    path.extend(part);
                    continue;
                }
                let shown = if dots {
                    let touched = self.g.step(b, run.start - STEP, true);
                    let reached = self.g.step(a, run.end + STEP, false);
                    self.g.math(MathOp::Mul, touched, reached)
                } else {
                    // The part's length on the run.
                    let past = self.g.above(b, run.end);
                    let short = self.g.below(a, run.start);
                    let to = self.g.math(MathOp::Sub, b, past);
                    let from = self.g.math(MathOp::Add, a, short);
                    let length = self.g.math(MathOp::Sub, to, from);
                    self.g.step(length, 0.0, true)
                };
                // The path takes its points where the transform puts them
                // as they come.
                let far = self.far();
                let away = self.g.math(MathOp::Mul, shown, Num::Const(-far));
                let away = self.g.math(MathOp::Add, away, Num::Const(far));
                path.push(Op::Save);
                path.push(Op::Translate(away, Num::Const(0.0)));
                path.extend(part);
                path.push(Op::Restore);
            }
        }
        let mut ops = vec![Op::Save];
        ops.extend(path);
        ops.push(Op::GlobalAlpha(alpha));
        ops.extend(style.iter().cloned());
        ops.push(Op::Stroke);
        ops.push(Op::Restore);
        ops
    }
}

/// Times to check tracks at: their keyframes and points between them.
fn sample_times(tracks: &[&Animated<f64>]) -> Vec<f64> {
    const BETWEEN: usize = 32;
    let mut keys: Vec<f64> = tracks
        .iter()
        .filter_map(|a| match a {
            Animated::Track(t) => Some(t.keyframes.iter().map(|k| k.time)),
            Animated::Static(_) => None,
        })
        .flatten()
        .filter(|t| t.is_finite())
        .collect();
    keys.sort_by(f64::total_cmp);
    keys.dedup();
    let mut out = Vec::with_capacity(keys.len() * BETWEEN + 1);
    for w in keys.windows(2) {
        for k in 0..BETWEEN {
            out.push(w[0] + (w[1] - w[0]) * k as f64 / BETWEEN as f64);
        }
    }
    out.push(keys.last().copied().unwrap_or(0.0));
    out
}

fn min_max(values: impl Iterator<Item = f64>) -> (f64, f64) {
    values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    })
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
