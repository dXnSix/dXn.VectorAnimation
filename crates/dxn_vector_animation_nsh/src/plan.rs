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
    Animated, BlendMode, Clip, Dash, Direction, EllipseStart, Fill, FillRule, Frame, FramePaint,
    FrameTextSpan, Geometry, Group, ImageData, Item, Keyframe, LineCap, LineJoin, Mask, Matrix,
    MotionPath, MotionRotate, Node, Paint as ScenePaint, PaintOrder, Path, PathEl, Point, Rect,
    Scene, Shape, ShapePath, Stroke, Text, TextAnchor, TextSpan, TimeMap, TimeRange, Track,
    TransformOp, Trim, TrimMode,
    outline::{geometry_path, path_length, trim_paths},
    transform::{decompose, point_along, transform_at, unwrap_angles},
};

use crate::graph::{Curve, Expr, Graph, MathOp, NodeId, Num, Paint, STEP, TIME, pack};
use crate::span;

mod bound;
mod mask;
mod paint;
mod pattern;
mod recolor;
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

/// The numbers of a path operation, where it has only numbers.
fn op_values(op: &Op) -> Vec<f64> {
    let v = |n: &Num| match n {
        Num::Const(c) => *c,
        Num::Node(_) => 0.0,
    };
    match op {
        Op::MoveTo(x, y) | Op::LineTo(x, y) => vec![v(x), v(y)],
        Op::QuadTo(a) => a.iter().map(v).collect(),
        Op::CubicTo(a) => a.iter().map(v).collect(),
        _ => Vec::new(),
    }
}

/// `op` with its numbers taken from `values`, in order.
fn with_values(op: &Op, values: &mut impl Iterator<Item = Num>) -> Op {
    let mut next = || values.next().unwrap_or(Num::Const(0.0));
    match op {
        Op::MoveTo(..) => Op::MoveTo(next(), next()),
        Op::LineTo(..) => Op::LineTo(next(), next()),
        Op::QuadTo(_) => Op::QuadTo([next(), next(), next(), next()]),
        Op::CubicTo(_) => Op::CubicTo([next(), next(), next(), next(), next(), next()]),
        other => other.clone(),
    }
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

/// Whether `node` holds a group with a soft mask (partly transparent, or by
/// luminance), which a frame's canvas (clipping) does not draw.
pub fn has_soft_masks(node: &Node) -> bool {
    fn partial(a: &Animated<f64>) -> bool {
        match a {
            Animated::Static(v) => *v < 1.0,
            Animated::Track(t) => t.keyframes.iter().any(|k| k.value < 1.0),
        }
    }
    fn faint(paint: &ScenePaint) -> bool {
        match paint {
            ScenePaint::Solid { color } => match color {
                Animated::Static(c) => c.a < 1.0,
                Animated::Track(t) => t.keyframes.iter().any(|k| k.value.a < 1.0),
            },
            _ => true,
        }
    }
    fn soft(g: &Group) -> bool {
        partial(&g.opacity)
            || g.children.iter().any(|n| match n {
                Node::Group(g) => soft(g),
                Node::Shape(s) => {
                    s.fill
                        .as_ref()
                        .is_some_and(|f| partial(&f.opacity) || faint(&f.paint))
                        || s.stroke
                            .as_ref()
                            .is_some_and(|f| partial(&f.opacity) || faint(&f.paint))
                }
                // Images, and text (its outline is the font's): drawn as
                // they are.
                Node::Image(_) | Node::Text(_) => true,
            })
    }
    match node {
        Node::Group(g) => {
            g.mask.as_ref().is_some_and(|m| {
                m.kind == dxn_vector_animation::MaskKind::Luminance || soft(&m.content)
            }) || g.children.iter().any(has_soft_masks)
        }
        _ => false,
    }
}

/// Whether `node`, sampled, is drawn as an image the reference renderer
/// makes: it holds effects or soft masks.
pub fn needs_raster(node: &Node) -> bool {
    has_effects(node) || has_soft_masks(node)
}

/// Whether `scene` draws the same at every time: sampled at its keyframes'
/// span, every frame alike.
pub fn still(scene: &Scene) -> bool {
    let d = scene.timeline.duration.max(0.0);
    let first = scene.frame_at(0.0);
    (1..=16).all(|k| scene.frame_at(d * f64::from(k) / 16.0) == first)
}

/// How far `ops` scale lengths, at most over a playback of `duration`.
fn probe_scale(ops: &[TransformOp], duration: f64) -> f64 {
    let scale = |t: f64| {
        let m = transform_at(ops, t);
        (m.a * m.d - m.b * m.c).abs().sqrt()
    };
    if !ops.iter().any(TransformOp::is_animated) {
        return scale(0.0);
    }
    let n = ((duration.max(0.0) * 60.0).ceil() as usize).clamp(1, 600);
    (0..=n)
        .map(|k| scale(duration * k as f64 / n as f64))
        .fold(0.0, f64::max)
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
    // What is blurred throughout loses nothing drawn at fewer pixels: as
    // few as keep its blur `BLUR_PIXELS` of them, scaled up where it shows.
    let blurred = blur_floor(&frame.items);
    let ratio = if blurred > 0.0 {
        ratio.min(BLUR_PIXELS / blurred)
    } else {
        ratio
    };
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

/// How many pixels the deviation of a blur takes, at least, in a raster of
/// what is blurred throughout.
const BLUR_PIXELS: f64 = 3.0;

/// How blurred everything `items` draw is, at least (the deviation, in
/// their lengths): each drawing by the blurs of the layers it is in, as
/// far out as a clip or mask, whose edges come after its layer's blur.
fn blur_floor(items: &[Item]) -> f64 {
    // Each open layer: its blur, and whether it clips or masks.
    let mut layers: Vec<(f64, bool)> = Vec::new();
    let mut floor = f64::INFINITY;
    for item in items {
        match item {
            Item::PushLayer(layer) => {
                let sigma = layer
                    .effects
                    .iter()
                    .map(|e| match e {
                        dxn_vector_animation::FrameEffect::Blur { sigma } => sigma * sigma,
                        _ => 0.0,
                    })
                    .sum::<f64>();
                layers.push((sigma, layer.clip.is_some() || layer.mask.is_some()));
            }
            Item::PopLayer => {
                layers.pop();
            }
            _ => {
                let mut squared = 0.0;
                for &(sigma, clips) in &layers {
                    if clips {
                        break;
                    }
                    squared += sigma;
                }
                floor = floor.min(squared.sqrt());
            }
        }
    }
    if floor.is_finite() { floor } else { 0.0 }
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

/// How many dashes of a pattern whose lengths move are drawn, at most:
/// each is a part the graph moves round the outline.
const MOVING_DASHES: usize = 256;

/// A run of a sampled outline's samples of the same commands: from when
/// (none for the first), and the samples.
type Window = (Option<f64>, Vec<(f64, Path)>);

/// How many different commands a sampled outline may have: each is a
/// contour of its own.
const OUTLINE_WINDOWS: usize = 32;

/// How many times a sampled outline's commands may change.
const OUTLINE_SWITCHES: usize = 1024;

/// How many copies of a shape whose paths morph between different commands
/// are drawn, at most: one for each time between switches (a merge baked
/// at every frame switches at every frame).
const MORPH_WINDOWS: usize = 32;

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
    /// `ellipse(cx, cy, rx, ry, 0, start, end, ccw)`: an arc of it.
    EllipseArc([Num; 6], bool),
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
    /// `font`, `textAlign` and `fillText(text, 0, 0)` (`strokeText` when
    /// `stroke`): one line from the origin on its baseline.
    Text {
        /// The characters.
        text: Arc<str>,
        /// The font.
        font: Arc<TextFont>,
        /// Which point of the line the origin is.
        anchor: TextAnchor,
        /// Stroked, not filled.
        stroke: bool,
    },
}

/// A font text is set in.
#[derive(Debug, Clone, PartialEq)]
pub struct TextFont {
    /// Family names, the preferred first (CSS's generic ones too).
    pub families: Vec<String>,
    /// The size (the em), in the coordinates current when it draws.
    pub size: f64,
    /// The weight, 100 to 900.
    pub weight: u16,
    /// Italic.
    pub italic: bool,
}

impl TextFont {
    /// As CSS's `font` shorthand writes it (`italic 700 16px "A B", serif`).
    pub fn css(&self) -> String {
        let families: Vec<String> = self
            .families
            .iter()
            .map(|f| {
                let generic = matches!(
                    f.as_str(),
                    "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui"
                );
                if generic || !f.contains(' ') {
                    f.clone()
                } else {
                    format!("\"{f}\"")
                }
            })
            .collect();
        format!(
            "{}{} {}px {}",
            if self.italic { "italic " } else { "" },
            self.weight,
            self.size,
            if families.is_empty() {
                "sans-serif".to_owned()
            } else {
                families.join(", ")
            }
        )
    }
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
    /// A canvas over `area` (points in the coordinates of what holds it),
    /// shown within `shown` only where there is one (what it draws past
    /// that casts shadows into it); its operations in its own coordinates,
    /// `area`'s corner at their origin.
    Area {
        /// Where it is.
        area: Rect,
        /// What of it shows.
        shown: Option<Rect>,
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
        /// How it moves (about its top left corner), where it does: what it
        /// holds is then in the coordinates it moves.
        transform: Option<LayerTransform>,
    },
}

/// An animated transform: moved, turned, skewed along x and scaled, in that
/// order (what each part reads).
#[derive(Debug, Clone, PartialEq)]
pub struct LayerTransform {
    /// How far it moves, in points.
    pub translate: (Num, Num),
    /// How far it turns, in degrees clockwise.
    pub rotate: Num,
    /// How far it skews along x, in degrees.
    pub skew: Num,
    /// How it scales.
    pub scale: (Num, Num),
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
    /// While shadows' casters are compiled: whether their silhouette is
    /// all or nothing.
    casting: Option<bool>,
    /// The display's pixels a point, for what the plan holds as images.
    pixels: f64,
    /// The color effects of the groups drawn inside, outermost first: what
    /// the colors drawn now go through.
    recolor: Vec<Vec<dxn_vector_animation::ColorEffect>>,
    /// While a group is drawn as a moving layer: where its canvases' setup
    /// starts.
    bound: Option<bound::Bound>,
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
    recolor: usize,
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

/// Compiles `scene` drawn through `base` (the scene's fit into the view),
/// for a display of a pixel a point.
pub fn compile(scene: &Scene, base: Matrix) -> Result<Plan, Unsupported> {
    compile_for(scene, base, 1.0)
}

/// Compiles `scene` drawn through `base` for a display of `pixels` pixels
/// a point (what the plan holds as images, masks' images, has as many).
pub fn compile_for(scene: &Scene, base: Matrix, pixels: f64) -> Result<Plan, Unsupported> {
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
        casting: None,
        pixels: if pixels > 0.0 { pixels } else { 1.0 },
        recolor: Vec::new(),
        bound: None,
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

/// `units` each taking away what `mask` (operations from the view's
/// origin) takes away; false where one cannot (a canvas reaching past the
/// view or over an area, a layer that moves).
fn mask_units(units: &mut [Unit], mask: &[Op]) -> bool {
    for unit in units {
        match unit {
            Unit::Canvas(ops) => ops.extend(mask.iter().cloned()),
            Unit::Layer {
                units,
                transform: None,
                ..
            } => {
                if !mask_units(units, mask) {
                    return false;
                }
            }
            Unit::Sampled(_) | Unit::Raster(_) => {}
            Unit::Wide { .. } | Unit::Area { .. } | Unit::Layer { .. } => return false,
        }
    }
    true
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
            Item::Text { spans, transform } => {
                ops.push(Op::Save);
                ops.push(Op::Transform(consts(base * *transform)));
                ops.push(Op::GlobalAlpha(a(alpha)));
                for line in frame_lines(spans) {
                    ops.extend(line);
                }
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

/// A frame's text: each line (from a placed span to the next) drawn from
/// where it is placed, its spans one after another as one text in the first
/// one's style (the fallback has no measures to place them apart).
fn frame_lines(spans: &[FrameTextSpan]) -> Vec<Vec<Op>> {
    let mut out = Vec::new();
    let mut pen = Point::new(0.0, 0.0);
    let mut i = 0;
    while i < spans.len() {
        let first = &spans[i];
        if let Some(p) = first.position {
            pen = p;
        }
        let mut text = first.text.clone();
        let mut j = i + 1;
        while j < spans.len() && spans[j].position.is_none() {
            text.push_str(&spans[j].text);
            j += 1;
        }
        let at = Point::new(pen.x + first.shift.x, pen.y + first.shift.y);
        let font = Arc::new(TextFont {
            families: first.families.clone(),
            size: first.size,
            weight: first.weight,
            italic: first.italic,
        });
        let c = Num::Const;
        let mut ops = vec![Op::Save, Op::Translate(c(at.x), c(at.y))];
        let fill = first.fill.as_ref().map(|p| (p, None));
        let stroke = first.stroke.as_ref().map(|(p, st)| (p, Some(st)));
        let order = match first.paint_order {
            PaintOrder::FillFirst => [fill, stroke],
            PaintOrder::StrokeFirst => [stroke, fill],
        };
        for (paint, style) in order.into_iter().flatten() {
            let color = match frame_paint(paint) {
                Ok(c) => c,
                // Text takes its gradient's first color here.
                Err(g) => match g.as_ref() {
                    Gradient::Linear { stops, .. } | Gradient::Radial { stops, .. } => {
                        stops.first().map_or(0, |s| s.1)
                    }
                },
            };
            match style {
                None => ops.push(Op::FillColor(Paint::Const(color))),
                Some(st) => {
                    ops.push(Op::StrokeColor(Paint::Const(color)));
                    ops.push(Op::LineWidth(c(st.width)));
                    ops.push(Op::LineJoin(st.join));
                }
            }
            ops.push(Op::Text {
                text: text.as_str().into(),
                font: font.clone(),
                anchor: first.anchor,
                stroke: style.is_some(),
            });
        }
        ops.push(Op::Restore);
        out.push(ops);
        i = j;
    }
    out
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
        // The fallback draws a pattern in a middle gray (its parts drawn
        // as the reference draws them are rasters).
        FramePaint::Pattern { .. } => Ok(0x8080_80ff),
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
            // From the fit and every group's setup; in a moving layer, from
            // its group's lengths, scaled up.
            let (mut prologue, from) = match self.bound {
                Some(b) => (
                    vec![Op::Scale(Num::Const(b.scale), Num::Const(b.scale))],
                    b.origin,
                ),
                None => (vec![Op::Transform(consts(self.base))], 0),
            };
            for frame in &self.context[from.min(self.context.len())..] {
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
        let recolor = self.recolor.len();
        let open = self.open();
        Mark {
            graph,
            layers,
            units: open.units.len(),
            canvas: open.canvas.len(),
            alpha: open.alpha,
            context,
            ancestors,
            recolor,
        }
    }

    /// Undoes what was drawn since `mark`.
    fn undo(&mut self, mark: Mark) {
        self.g.truncate(mark.graph);
        self.layers.truncate(mark.layers);
        self.context.truncate(mark.context);
        self.ancestors.truncate(mark.ancestors);
        self.recolor.truncate(mark.recolor);
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
        // Within groups whose colors change (or that cast inner shadows),
        // as the reference draws it: the fallback draws neither.
        let raster = needs_raster(node)
            || self.ancestors.iter().any(|(g, _)| {
                g.effects.iter().any(|e| {
                    matches!(
                        e,
                        dxn_vector_animation::Effect::Color(_)
                            | dxn_vector_animation::Effect::InnerShadow(_)
                    )
                })
            });
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
        // A group that only moves: a layer the platform moves.
        if self.can_bind(g) {
            let mark = self.mark();
            match self.bound_group(g, local, depth, visible) {
                Ok(true) => return Ok(()),
                _ => {
                    self.bound = None;
                    self.undo(mark);
                }
            }
        }
        let shadows = self.canvas_shadows(g, local)?;
        let mut frame = vec![Op::Save];
        frame.extend(self.transform(&g.transform, local)?);
        if let Some(clip) = &g.clip {
            frame.extend(self.clip(clip, local)?);
        }
        // A mask's colors are its own: the color effects drawn inside do
        // not change them.
        let recolor = std::mem::take(&mut self.recolor);
        let soft = match &g.mask {
            Some(mask) => self.soft_mask(g, mask, local, depth),
            None => Ok(None),
        };
        self.recolor = recolor;
        let soft = soft?;
        // A mask that shows nothing anywhere: nothing of the group to draw
        // (an empty clip is not honoured everywhere).
        if soft.as_ref().is_some_and(mask::SoftMask::hides_all) {
            return Ok(());
        }
        if let Some(soft) = &soft {
            frame.extend(soft.clips.iter().cloned());
        }
        if let Some(mask) = &g.mask
            && soft.is_none()
        {
            let recolor = std::mem::take(&mut self.recolor);
            let ops = self.mask(mask, local, depth);
            self.recolor = recolor;
            let ops = ops?;
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
        // Color effects: the paints inside recolored; what they do to
        // alpha fades the group.
        let colors = self.begin_colors(g, local)?;
        let opacity = match colors {
            Some(k) => self.mul(opacity, k),
            None => opacity,
        };
        // A layer fades as one; one that only fades (its content still) is
        // a layer too, so its canvas never draws again as it fades.
        let isolate = !g.blend.is_normal()
            || (!opacity.is(1.0) && drawables(g, 2) > 1)
            || !shadows.is_empty()
            || (matches!(opacity, Num::Node(_))
                && self.casting.is_none()
                && !self
                    .ancestors
                    .iter()
                    .any(|(a, _)| a.mask.is_some() || !a.effects.is_empty())
                && bound::still_content(g, &self.stage));
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
            let ops = self.soft_mask_ops(soft);
            if self.open().units.len() != units {
                // The content drew on more than one canvas: each takes away
                // what the mask hides, in the setup of the groups again (a
                // canvas keeps the fit it starts with; parts the reference
                // draws are masked in their scenes).
                let mut setup = vec![Op::Save];
                setup.extend(self.context.iter().flatten().cloned());
                setup.extend(ops.iter().cloned());
                let setup = closed(setup);
                if !mask_units(&mut self.open().units[units..], &setup) {
                    // A canvas past the view, or moving.
                    self.undo(start);
                    return unsupported("a soft mask of shadows");
                }
            }
            if !self.open().canvas.is_empty() {
                self.open().canvas.extend(ops);
            }
        }
        if !shadows.is_empty() {
            // The content: the setup of the groups it is in (the fit, then
            // theirs, this one's last), then what its canvases draw.
            let mut content = vec![Op::Transform(consts(self.base))];
            content.extend(self.context.iter().flatten().cloned());
            let outer = content.len();
            let mut several = false;
            for unit in &self.open().units[units..] {
                several = true;
                if let Unit::Canvas(ops) = unit {
                    content.extend(ops.iter().cloned());
                }
            }
            let open = &self.open().canvas;
            content.extend(
                open.get(outer.min(open.len())..)
                    .unwrap_or_default()
                    .iter()
                    .cloned(),
            );
            // Composited content casts as an image of it.
            let several = several
                || content
                    .iter()
                    .any(|op| matches!(op, Op::Blend(_) | Op::Atop | Op::Shadow { .. }));
            if content.len() > outer || several {
                let content = shadow::Content {
                    ops: &content,
                    outer,
                    several,
                };
                match self.shadow_units(g, local, depth, content, shadows) {
                    Ok(list) => {
                        let (over, under): (Vec<_>, Vec<_>) =
                            list.into_iter().partition(|(_, inner)| *inner);
                        for (i, (unit, _)) in under.into_iter().enumerate() {
                            self.open().units.insert(units + i, unit);
                        }
                        if !over.is_empty() {
                            // Inner shadows over the content: the canvas
                            // ends with it.
                            self.flush();
                            for (unit, _) in over {
                                self.open().units.push(unit);
                            }
                        }
                    }
                    Err(why) => {
                        self.undo(start);
                        return Err(why);
                    }
                }
            }
        }
        if colors.is_some() {
            self.end_colors();
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
                    transform: None,
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
                let data = match self.casting {
                    Some(_) => image.data.clone(),
                    None => self.recolored_image(&image.data)?,
                };
                self.draw(vec![
                    Op::Save,
                    Op::GlobalAlpha(alpha),
                    Op::Image(data, image.rect),
                    Op::Restore,
                ]);
                Ok(())
            }
            Node::Text(text) => self.text(text, time),
        }
    }

    /// Text set by the platform: each line (from a placed span to the next)
    /// one `fillText` (and `strokeText`) placed and sized by the graph.
    /// Lines of several styles, spaced letters and paints other than colors
    /// are sampled.
    fn text(&mut self, t: &Text, time: NodeId) -> Result<(), Unsupported> {
        let mut i = 0;
        while i < t.spans.len() {
            let first = &t.spans[i];
            let Some(position) = &first.position else {
                return unsupported("text placed after text of another style");
            };
            let mut text = first.text.clone();
            let mut j = i + 1;
            while j < t.spans.len() && t.spans[j].position.is_none() {
                let next = &t.spans[j];
                if !same_style(first, next) || next.shift != Point::new(0.0, 0.0) {
                    return unsupported("a line of text in several styles");
                }
                text.push_str(&next.text);
                j += 1;
            }
            i = j;
            if text.trim().is_empty() {
                continue;
            }
            if first.letter_spacing.is_animated()
                || first
                    .letter_spacing
                    .as_static()
                    .is_some_and(|s| s.abs() > 1e-9)
            {
                return unsupported("spaced text");
            }
            let size = match &first.font.size {
                Animated::Static(s) => *s,
                Animated::Track(track) => {
                    track.keyframes.iter().map(|k| k.value).fold(0.0, f64::max)
                }
            };
            if size.is_nan() || size <= 0.0 {
                continue;
            }
            let font = Arc::new(TextFont {
                families: first.font.families.clone(),
                size,
                weight: first.font.weight,
                italic: first.font.italic,
            });
            let x = self.g.track(&position.x, time, |v| v);
            let y = self.g.track(&position.y, time, |v| v);
            let c = Num::Const;
            let mut ops = vec![Op::Save, Op::Translate(x, y)];
            if first.shift != Point::new(0.0, 0.0) {
                ops.push(Op::Translate(c(first.shift.x), c(first.shift.y)));
            }
            // An animated size scales the text set at its largest.
            let k = self.g.track(&first.font.size, time, |s| s.max(0.0) / size);
            if !k.is(1.0) {
                ops.push(Op::Scale(k, k));
            }
            let alpha = self.open().alpha;
            let mut paints = Vec::new();
            if let Some(fill) = &first.fill {
                paints.push((false, &fill.paint, &fill.opacity, None));
            }
            if let Some(stroke) = &first.stroke {
                paints.push((true, &stroke.paint, &stroke.opacity, Some(stroke)));
            }
            if first.paint_order == PaintOrder::StrokeFirst {
                paints.reverse();
            }
            for (stroked, paint, opacity, stroke) in paints {
                let ScenePaint::Solid { color } = paint else {
                    return unsupported("text painted with a gradient or a pattern");
                };
                let opacity = self.g.track(opacity, time, |o| o.clamp(0.0, 1.0));
                let mut a = self.mul(alpha, opacity);
                let color = match self.casting {
                    Some(hard) => {
                        a = self.hardened(a, hard);
                        let shade = shadow::silhouette_color(color, hard);
                        self.g.color_track(&shade, time, 1.0)
                    }
                    None => {
                        let color = self.recolored(color);
                        self.g.color_track(&color, time, 1.0)
                    }
                };
                ops.push(Op::GlobalAlpha(a));
                if let Some(stroke) = stroke {
                    let width = self.g.track(&stroke.width, time, |w| w.max(0.0));
                    // In the text's lengths, as the stroke is not scaled.
                    let width = if k.is(1.0) {
                        width
                    } else {
                        self.g.math(MathOp::Div, width, k)
                    };
                    ops.push(Op::LineWidth(width));
                    ops.push(Op::LineJoin(stroke.join));
                    ops.push(Op::StrokeColor(color));
                } else {
                    ops.push(Op::FillColor(color));
                }
                ops.push(Op::Text {
                    text: text.as_str().into(),
                    font: font.clone(),
                    anchor: first.anchor,
                    stroke: stroked,
                });
            }
            ops.push(Op::Restore);
            self.draw(ops);
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
        // Windows of the same commands share a contour: its points those of
        // each window in turn, shown during any.
        let mut contours: Vec<Vec<usize>> = Vec::new();
        for w in 0..windows.len() {
            let first = &windows[w].1[0].1;
            match contours
                .iter_mut()
                .find(|c| windows[c[0]].1[0].1.same_structure(first))
            {
                Some(c) => c.push(w),
                None => contours.push(vec![w]),
            }
        }
        if contours.len() > OUTLINE_WINDOWS || windows.len() > OUTLINE_SWITCHES {
            return unsupported("an outline whose commands change many times");
        }
        let tolerance = self.tolerance();
        let far = self.far();
        let mut ops = Vec::new();
        for list in &contours {
            let run: Vec<(f64, Path)> = list
                .iter()
                .flat_map(|&w| windows[w].1.iter().cloned())
                .collect();
            let spans: Vec<(Option<f64>, Option<f64>)> = list
                .iter()
                .map(|&w| (windows[w].0, windows.get(w + 1).and_then(|next| next.0)))
                .collect();
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
            if contours.len() == 1 {
                return Ok(contour);
            }
            // The path takes its points where the transform puts them as
            // they come.
            let shown = self.g.windows(Num::Node(time), &spans);
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
        if switches.len() >= MORPH_WINDOWS {
            return unsupported("a shape whose commands change many times");
        }
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
                let Ok(paths) = static_paths(s) else {
                    ops.extend(self.sampled_trim(s, trim, time)?);
                    return Ok(ops);
                };
                if trim.start.is_animated() || trim.end.is_animated() || trim.offset.is_animated() {
                    // The trimmed outline as the graph moves it, filled.
                    for (runs, ends, ab) in self.moving_trim_runs(&paths, trim, time) {
                        let path = self.part_path(&runs, ab, &ends, false);
                        ops.extend(path.into_iter().skip(1));
                    }
                } else {
                    ops.extend(static_path(&trim_paths(&paths, trim, 0.0)));
                }
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
        let paint = self.recolored_paint(&fill.paint)?;
        match paint.as_ref() {
            ScenePaint::Pattern(p) => {
                // The outline clips the tiles.
                let tiles = self.pattern_fill(s, p, a)?;
                if !tiles.is_empty() {
                    ops.push(Op::Clip(fill.rule));
                    ops.extend(tiles);
                }
            }
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
        let paint = self.recolored_paint(&stroke.paint)?;
        match paint.as_ref() {
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
            // Along an outline that moves, the outline's sampled part.
            if static_paths(s).is_ok() {
                return Ok((self.moving_trim(s, trim, time, a, &style)?, false));
            }
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
        let mut ops = Vec::new();
        for (runs, ends, ab) in self.moving_trim_runs(&paths, trim, time) {
            ops.extend(self.parts(&runs, ab, &ends, false, alpha, style));
        }
        Ok(ops)
    }

    /// A trim (moving or not) of outlines that move, as path operations
    /// (after a `beginPath`): sampled as the outlines are, each run of each
    /// trimmed outline with every segment kept (outside the part a point
    /// where it ends, the run moved away while none of it shows).
    fn sampled_trim(
        &mut self,
        s: &Shape,
        trim: &Trim,
        time: NodeId,
    ) -> Result<Vec<Op>, Unsupported> {
        if !morph_switches(s).is_empty() {
            return unsupported("a trim of an outline whose commands change");
        }
        let mut keys: Vec<f64> = s
            .paths
            .iter()
            .flat_map(|p| key_times(&p.geometry))
            .collect();
        for p in &s.paths {
            for op in &p.transform {
                if let TransformOp::Translate { x, y }
                | TransformOp::Scale { x, y }
                | TransformOp::Skew { x, y } = op
                {
                    keys.extend(x.keyframes().iter().map(|k| k.time));
                    keys.extend(y.keyframes().iter().map(|k| k.time));
                }
                if let TransformOp::Rotate { degrees } = op {
                    keys.extend(degrees.keyframes().iter().map(|k| k.time));
                }
            }
        }
        for a in [&trim.start, &trim.end, &trim.offset] {
            keys.extend(a.keyframes().iter().map(|k| k.time));
        }
        keys.sort_by(f64::total_cmp);
        keys.dedup();
        if keys.len() < 2 {
            keys = vec![0.0, self.stage.timeline.duration.max(1e-3)];
        }
        let times = outline_times(&keys);
        let paths_at = |t: f64| -> Vec<Path> {
            s.paths
                .iter()
                .map(|p| {
                    let m = dxn_vector_animation::transform::transform_at(&p.transform, t);
                    geometry_path(&p.geometry, t).transformed(m)
                })
                .collect()
        };
        // The part, as fractions of the outline, at each time; past the
        // end it goes on from the start, the outline twice.
        let mut parts: Vec<(f64, f64)> = times
            .iter()
            .map(|&t| {
                let (a, b) = (trim.start.sample(t), trim.end.sample(t));
                let (lo, hi) = (a.min(b).clamp(0.0, 1.0), a.max(b).clamp(0.0, 1.0));
                let o = trim.offset.sample(t);
                (lo + o, hi + o)
            })
            .collect();
        let inside = parts.iter().all(|&(f, _)| (0.0..=1.0).contains(&f));
        let wraps = !inside || parts.iter().any(|&(_, t)| t > 1.0);
        if !inside {
            for (f, t) in &mut parts {
                let wrapped = f.rem_euclid(1.0);
                (*f, *t) = (wrapped, wrapped + (*t - *f));
            }
        }
        let count = match trim.mode {
            TrimMode::EachPath => s.paths.len(),
            TrimMode::Combined => 1,
        };
        let tolerance = self.tolerance();
        let far = self.far();
        let mut scratch = Graph::default();
        let mut ops = Vec::new();
        for group in 0..count {
            // Each sample's runs, ends and parts of runs.
            let mut samples: Vec<Vec<(Vec<Op>, bool)>> = Vec::with_capacity(times.len());
            for (&t, &(f, to)) in times.iter().zip(&parts) {
                let all = paths_at(t);
                let chosen: Vec<&Path> = match trim.mode {
                    TrimMode::EachPath => vec![&all[group]],
                    TrimMode::Combined => all.iter().collect(),
                };
                let mut runs = span::runs(&chosen);
                let total = runs.last().map_or(0.0, |r| r.end);
                if wraps && total > 0.0 {
                    runs = span::doubled(&runs, total);
                }
                let (a, b) = (f * total, to * total);
                samples.push(
                    runs.iter()
                        .map(|run| {
                            let shown = total > 0.0 && b.min(run.end) - a.max(run.start) > STEP;
                            let part =
                                run.span(&mut scratch, Num::Const(a), Num::Const(b), tolerance);
                            (part, shown)
                        })
                        .collect(),
                );
            }
            let Some(first) = samples.first() else {
                continue;
            };
            let runs = first.len();
            if samples.iter().any(|runs_t| {
                runs_t.len() != runs
                    || runs_t.iter().zip(first).any(|((a, _), (b, _))| {
                        a.len() != b.len()
                            || a.iter().zip(b).any(|(x, y)| {
                                std::mem::discriminant(x) != std::mem::discriminant(y)
                            })
                    })
            }) {
                return unsupported("a trim of an outline whose runs change");
            }
            for r in 0..runs {
                let shown: Vec<bool> = samples.iter().map(|runs_t| runs_t[r].1).collect();
                if !shown.iter().any(|&v| v) {
                    continue;
                }
                // Each number of the run's part, over the samples.
                let columns: Vec<Vec<f64>> = samples
                    .iter()
                    .map(|runs_t| runs_t[r].0.iter().flat_map(op_values).collect())
                    .collect();
                let mut nums = Vec::with_capacity(columns[0].len());
                for j in 0..columns[0].len() {
                    let values: Vec<f64> = columns.iter().map(|c| c[j]).collect();
                    let (inputs, outputs) = simplified(&times, &values, tolerance);
                    nums.push(match outputs.as_slice() {
                        [one] => Num::Const(*one),
                        _ => self.g.lookup(Num::Node(time), &inputs, &outputs),
                    });
                }
                let mut it = nums.into_iter();
                let part: Vec<Op> = first[r]
                    .0
                    .iter()
                    .map(|op| with_values(op, &mut it))
                    .collect();
                if shown.iter().all(|&v| v) {
                    ops.extend(part);
                    continue;
                }
                // Away while none of it shows: switching halfway between
                // samples.
                let (mut inputs, mut outputs) =
                    (vec![times[0]], vec![f64::from(u8::from(!shown[0]))]);
                for i in 1..times.len() {
                    if shown[i] != shown[i - 1] {
                        let at = (times[i - 1] + times[i]) / 2.0;
                        inputs.extend([at, at + 1e-6]);
                        outputs.extend([
                            f64::from(u8::from(!shown[i - 1])),
                            f64::from(u8::from(!shown[i])),
                        ]);
                    }
                }
                let hidden = self.g.lookup(Num::Node(time), &inputs, &outputs);
                let away = self.g.math(MathOp::Mul, hidden, Num::Const(far));
                ops.push(Op::Save);
                ops.push(Op::Translate(away, Num::Const(0.0)));
                ops.extend(part);
                ops.push(Op::Restore);
            }
        }
        Ok(ops)
    }

    /// The runs of each outline (each path's, or all paths' one after
    /// another) a moving trim of `paths` draws a part of, each with the
    /// part's ends over time (lengths along them) and as the graph moves
    /// them.
    #[allow(clippy::type_complexity)]
    fn moving_trim_runs(
        &mut self,
        paths: &[Path],
        trim: &Trim,
        time: NodeId,
    ) -> Vec<(Vec<span::Run>, Vec<(f64, f64)>, (Num, Num))> {
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
        let mut out = Vec::new();
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
            out.push((runs, ends, (a, b)));
        }
        out
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
        let periods: Vec<f64> = times
            .iter()
            .map(|&t| pattern.iter().map(|a| a.sample(t).max(0.0)).sum())
            .collect();
        let (shortest, _) = min_max(periods.iter().copied());
        if shortest <= 0.0 {
            return unsupported("an animated dash pattern of no length");
        }
        // Each repetition a slot going round as the offset moves: from a
        // period before the outline to past its end, where it wraps.
        let slots = (longest / shortest).ceil() as usize + 2;
        if slots * pattern.len() / 2 > MOVING_DASHES {
            return unsupported("an animated dash pattern of many dashes along the outline");
        }
        // Where dash `k` of slot `c` starts and ends at `t`.
        let place = |t: f64, c: usize, k: usize| {
            let lengths: Vec<f64> = pattern.iter().map(|a| a.sample(t).max(0.0)).collect();
            let period: f64 = lengths.iter().sum();
            let at = c as f64 * period + lengths[..k].iter().sum::<f64>() - dash.offset.sample(t);
            let a = (at + period).rem_euclid(slots as f64 * period) - period;
            (a, a + lengths[k])
        };
        let lengths: Vec<Num> = pattern
            .iter()
            .map(|a| self.g.track(a, time, |v| v.max(0.0)))
            .collect();
        let period = self.g.sum(&lengths);
        let round = self.g.math(MathOp::Mul, period, Num::Const(slots as f64));
        let offset = self.g.track(&dash.offset, time, |o| -o);
        // Round an ellipse each dash is an arc between two angles the graph
        // moves, a few nodes a dash, where a part of an outline moves each
        // of its points.
        let ellipse = ellipse_outline(s, longest);
        let mut ops = Vec::new();
        for c in 0..slots {
            for k in (0..pattern.len()).step_by(2) {
                let ends: Vec<(f64, f64)> = times.iter().map(|&t| place(t, c, k)).collect();
                if !ends.iter().any(|&(a, b)| b >= 0.0 && a <= longest) {
                    continue;
                }
                let before = self.g.sum(&lengths[..k]);
                let repeats = self.g.math(MathOp::Mul, period, Num::Const(c as f64 + 1.0));
                let start = self.g.math(MathOp::Add, repeats, before);
                let at = self.g.math(MathOp::Add, start, offset);
                let wrapped = self.g.math(MathOp::Mod, at, round);
                let a = self.g.math(MathOp::Sub, wrapped, period);
                let b = self.g.math(MathOp::Add, a, lengths[k]);
                match &ellipse {
                    Some(e) => ops.extend(self.dash_arc(e, (a, b), longest, alpha, style)),
                    None => ops.extend(self.parts(&contours, (a, b), &ends, true, alpha, style)),
                }
            }
        }
        Ok(ops)
    }

    /// A dash from `a` to `b` along ellipse `e` (an outline `total` long),
    /// stroked with `style`: an arc between the angles where they are, the
    /// part off the outline cut off; one of no length still draws its caps,
    /// as a dash pattern does, and one off the outline moves far out of
    /// sight (as [`Self::part_path`]'s dots).
    fn dash_arc(
        &mut self,
        e: &EllipseOutline,
        (a, b): (Num, Num),
        total: f64,
        alpha: Num,
        style: &[Op],
    ) -> Vec<Op> {
        let from = self.g.lookup(a, &e.lengths, &e.angles);
        let to = self.g.lookup(b, &e.lengths, &e.angles);
        let to = self
            .g
            .math(MathOp::Add, to, Num::Const(if e.ccw { -DOT } else { DOT }));
        let touched = self.g.step(b, -STEP, true);
        let reached = self.g.step(a, total + STEP, false);
        let shown = self.g.math(MathOp::Mul, touched, reached);
        let far = self.far();
        let away = self.g.math(MathOp::Mul, shown, Num::Const(-far));
        let away = self.g.math(MathOp::Add, away, Num::Const(far));
        let [cx, cy, rx, ry] = e.geometry.map(Num::Const);
        let mut ops = vec![
            Op::Save,
            Op::BeginPath,
            Op::Save,
            Op::Translate(away, Num::Const(0.0)),
            Op::EllipseArc([cx, cy, rx, ry, from, to], e.ccw),
            Op::Restore,
            Op::GlobalAlpha(alpha),
        ];
        ops.extend(style.iter().cloned());
        ops.push(Op::Stroke);
        ops.push(Op::Restore);
        ops
    }

    /// The part of `runs` from `a` to `b` (as at `ends`, over time), as a
    /// path (a `beginPath` first): stroked with `style`, see
    /// [`Self::parts`].
    fn part_path(
        &mut self,
        runs: &[span::Run],
        (a, b): (Num, Num),
        ends: &[(f64, f64)],
        dots: bool,
    ) -> Vec<Op> {
        let on = |run: &span::Run, a: f64, b: f64| {
            if dots {
                b >= run.start && a <= run.end
            } else {
                b.min(run.end) - a.max(run.start) > STEP
            }
        };
        let (a_max, b_min) = (
            min_max(ends.iter().map(|e| e.0)).1,
            min_max(ends.iter().map(|e| e.1)).0,
        );
        let mut path = vec![Op::BeginPath];
        for run in runs
            .iter()
            .filter(|r| ends.iter().any(|&(a, b)| on(r, a, b)))
        {
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
        path
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
            path = self.part_path(runs, (a, b), ends, dots);
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

/// The arc a dash of no length still draws, so that it has caps (radians).
const DOT: f64 = 1e-4;

/// An outline that is one constant ellipse, what [`Compiler::dash_arc`]
/// draws a dash of: the angle where each length along it is.
struct EllipseOutline {
    /// `cx, cy, rx, ry`.
    geometry: [f64; 4],
    /// Lengths along the outline, rising from 0 to its length.
    lengths: Vec<f64>,
    /// The angles at `lengths` (`ellipse()`'s, from its start the way it
    /// runs).
    angles: Vec<f64>,
    ccw: bool,
}

/// `s`'s outline as an ellipse, when it is one ellipse that stays (its
/// lengths scaled to `total`, the outline's length as dashes measure it).
fn ellipse_outline(s: &Shape, total: f64) -> Option<EllipseOutline> {
    /// Steps of a turn the lengths are summed over, and how many go
    /// between two of its points.
    const STEPS: usize = 512;
    const EVERY: usize = 4;
    let [path] = s.paths.as_slice() else {
        return None;
    };
    let Geometry::Ellipse {
        center,
        radius,
        direction,
        start,
    } = &path.geometry
    else {
        return None;
    };
    if !path.transform.is_empty() || path.geometry.is_animated() || total <= 0.0 {
        return None;
    }
    let (cx, cy) = (center.x.sample(0.0), center.y.sample(0.0));
    let (rx, ry) = (radius.x.sample(0.0).abs(), radius.y.sample(0.0).abs());
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let first = match start {
        EllipseStart::Right => 0.0,
        EllipseStart::Top => -std::f64::consts::FRAC_PI_2,
    };
    let ccw = *direction == Direction::CounterClockwise;
    let sign = if ccw { -1.0 } else { 1.0 };
    let turn = std::f64::consts::TAU;
    let (mut lengths, mut angles) = (vec![0.0], vec![first]);
    if (rx - ry).abs() <= 1e-9 * rx {
        lengths.push(total);
        angles.push(first + sign * turn);
    } else {
        let at = |t: f64| (rx * t.cos(), ry * t.sin());
        let (mut length, mut last) = (0.0, at(first));
        for i in 1..=STEPS {
            let angle = first + sign * turn * i as f64 / STEPS as f64;
            let p = at(angle);
            length += (p.0 - last.0).hypot(p.1 - last.1);
            last = p;
            if i % EVERY == 0 {
                lengths.push(length);
                angles.push(angle);
            }
        }
        let scale = total / length;
        lengths.iter_mut().for_each(|l| *l *= scale);
    }
    Some(EllipseOutline {
        geometry: [cx, cy, rx, ry],
        lengths,
        angles,
        ccw,
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

/// Whether `a` and `b` set text alike (font, spacing and paints).
fn same_style(a: &TextSpan, b: &TextSpan) -> bool {
    a.font == b.font
        && a.letter_spacing == b.letter_spacing
        && a.fill == b.fill
        && a.stroke == b.stroke
        && a.paint_order == b.paint_order
}

/// How many of `g`'s descendants draw, counting up to `limit`.
fn drawables(g: &Group, limit: usize) -> usize {
    let mut n = 0;
    for child in &g.children {
        n += match child {
            Node::Group(c) => drawables(c, limit - n.min(limit)),
            Node::Shape(_) | Node::Image(_) | Node::Text(_) => 1,
        };
        if n >= limit {
            return n;
        }
    }
    n
}
