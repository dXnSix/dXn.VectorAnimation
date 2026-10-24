//! Drop shadows on the canvas. Its shadow (`shadowColor`, `shadowBlur`,
//! `shadowOffset`) does not move, falls in the view's points whatever the
//! transform, and is clipped with what casts it: so a group's shadows are
//! cast by its casters (its content compiled again as a silhouette, without
//! clips: a clip a shape of the group fills whole is that shape) in layers
//! of their own under it, each shadow's opacity its layer's (a blur that
//! moves fades between still ones).
//!
//! Where the shadow is still and hidden under the content (knocked out, or
//! the content opaque), the casters are drawn in place with it and taken
//! away again. Otherwise (an offset that moves, a shadow seen through the
//! content) they are drawn past the view, on a canvas reaching past it and
//! shown within it: only their shadows fall into the view, moved there by an
//! animated translation, then clipped by the groups' clips and knocked out
//! as the content asks. A hue that moves colors the shadow over
//! (`source-atop`).

use dxn_vector_animation::{
    Animated, BlendMode, Clip, ClipShape, Color, Effect, Fill, Group, Item, Keyframe, Matrix,
    MotionRotate, Node, Paint as ScenePaint, Path, Point, Rect, Scene, Shape, ShapePath, Track,
    TransformOp,
    outline::geometry_path,
    transform::{decompose, transform_at, unwrap_angles},
};

use super::{
    Compiler, FAR, Gradient, LayerTransform, Op, Open, Unit, Unsupported, closed, huge_rect,
    min_max, sample_times, shell, simplified, unsupported,
};
use crate::graph::{Curve, Expr, MathOp, NodeId, Num, Paint, TIME, pack};

/// How far apart, at most, the blurs a moving shadow blur is drawn at are:
/// a share of the smaller (or a length of the group, for small ones).
const BLUR_STEP: f64 = 1.0;
const BLUR_RATIO: f64 = 0.3;

/// The most blurs a moving shadow blur is drawn at.
const BLUR_LEVELS: usize = 10;

/// How far past the view the casters drawn past it stay from it, in points.
const PAST: f64 = 4.0;

/// How many times a second what moves is sampled for where it goes.
const PROBE_RATE: f64 = 60.0;

/// The most times it is sampled.
const PROBES: usize = 600;

/// A drop shadow as the canvas draws it.
pub(super) struct CanvasShadow {
    /// Its color, opaque: still, or a node.
    color: Paint,
    /// Its opacity.
    pub(super) alpha: Num,
    /// Twice the deviation of its blur, in the view's points.
    blur: f64,
    /// How far it falls, in the view's points.
    offset: [Num; 2],
    /// Hidden under the content, even where it is see-through.
    knockout: bool,
    /// Cast by the content's silhouette made all or nothing.
    hard: bool,
}

/// A group's shadows as the canvas draws them.
#[derive(Default)]
pub(super) struct Shadows {
    list: Vec<CanvasShadow>,
    /// How the group's lengths move in the view, where it turns or scales
    /// (the shadows are then in its lengths).
    turning: Option<LayerTransform>,
}

impl Shadows {
    pub(super) fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

/// The deviation of a shadow's blur in the view's points.
enum ViewBlur {
    Still(f64),
    /// Moving between `lo` and `hi` as `node` does.
    Moving {
        lo: f64,
        hi: f64,
        node: Num,
    },
}

/// Times from 0 to `duration`, [`PROBE_RATE`] a second, at most
/// [`PROBES`] of them.
fn probe_times(duration: f64) -> Vec<f64> {
    let d = duration.max(0.0);
    let n = ((d * PROBE_RATE).ceil() as usize).clamp(1, PROBES);
    (0..=n).map(|k| d * k as f64 / n as f64).collect()
}

/// Whether `op` turns, scales or skews over time (moves the shadow's
/// direction and length, not only where it falls).
fn turns(op: &TransformOp) -> bool {
    match op {
        TransformOp::Translate { .. } => false,
        TransformOp::Scale { x, y } | TransformOp::Skew { x, y } => {
            x.is_animated() || y.is_animated()
        }
        TransformOp::Rotate { degrees } => degrees.is_animated(),
        TransformOp::Matrix { matrix } => matrix.is_animated(),
        TransformOp::Motion(m) => matches!(m.rotate, MotionRotate::Auto { .. }),
    }
}

/// `a` with each value mapped by `f`.
fn mapped<T: Clone>(a: &Animated<T>, f: impl Fn(&T) -> T) -> Animated<T> {
    match a {
        Animated::Static(v) => Animated::Static(f(v)),
        Animated::Track(t) => Animated::Track(Track::new(
            t.keyframes
                .iter()
                .map(|k| Keyframe::new(k.time, f(&k.value), k.easing.clone()))
                .collect(),
        )),
    }
}

/// Every value `a` takes at its keyframes.
fn values<T: Clone>(a: &Animated<T>) -> Vec<T> {
    match a {
        Animated::Static(v) => vec![v.clone()],
        Animated::Track(t) => t.keyframes.iter().map(|k| k.value.clone()).collect(),
    }
}

/// An alpha as the silhouette casts it: all or nothing but at its faintest
/// where `hard` (times 127, as Figma's `hardAlpha`).
fn harden(a: f64, hard: bool) -> f64 {
    if hard { (a * 127.0).min(1.0) } else { a }
}

/// `paint` as a silhouette casts it: black, as see-through as it (or
/// hardened); one opaque color where it is opaque throughout.
fn silhouette_paint(paint: &ScenePaint, hard: bool) -> ScenePaint {
    let black = |a: f64| Color::rgba(0.0, 0.0, 0.0, harden(a, hard));
    let opaque = ScenePaint::solid(Color::rgba(0.0, 0.0, 0.0, 1.0));
    // Hardened, a fade between stops turns all or nothing where it reaches
    // a 127th: a stop there.
    let stops = |stops: &Animated<Vec<dxn_vector_animation::GradientStop>>| {
        mapped(stops, |list| {
            let mut out = Vec::with_capacity(list.len());
            for (i, s) in list.iter().enumerate() {
                if hard && let Some(prev) = i.checked_sub(1).map(|p| &list[p]) {
                    let (a0, a1) = (prev.color.a, s.color.a);
                    let edge = 1.0 / 127.0;
                    if (a0 - edge) * (a1 - edge) < 0.0 {
                        let k = (edge - a0) / (a1 - a0);
                        out.push(dxn_vector_animation::GradientStop {
                            offset: prev.offset + (s.offset - prev.offset) * k,
                            color: black(edge),
                        });
                    }
                }
                out.push(dxn_vector_animation::GradientStop {
                    offset: s.offset,
                    color: black(s.color.a),
                });
            }
            out
        })
    };
    let all_opaque = |s: &Animated<Vec<dxn_vector_animation::GradientStop>>| {
        values(s)
            .iter()
            .flatten()
            .all(|s| harden(s.color.a, hard) >= 1.0)
    };
    match paint {
        ScenePaint::Solid { color } => ScenePaint::Solid {
            color: mapped(color, |c| black(c.a)),
        },
        ScenePaint::LinearGradient(g) if all_opaque(&g.stops) => opaque,
        ScenePaint::RadialGradient(g) if all_opaque(&g.stops) => opaque,
        ScenePaint::LinearGradient(g) => {
            let mut g = g.clone();
            g.stops = stops(&g.stops);
            ScenePaint::LinearGradient(g)
        }
        ScenePaint::RadialGradient(g) => {
            let mut g = g.clone();
            g.stops = stops(&g.stops);
            ScenePaint::RadialGradient(g)
        }
    }
}

/// `s` as its silhouette casts it.
pub(super) fn silhouette(s: &Shape, hard: bool) -> Shape {
    let mut out = s.clone();
    if let Some(fill) = &mut out.fill {
        fill.paint = silhouette_paint(&fill.paint, hard);
    }
    if let Some(stroke) = &mut out.stroke {
        stroke.paint = silhouette_paint(&stroke.paint, hard);
    }
    out
}

/// Whether `p` and `c` are the same outline: alike, or drawing the same
/// path where both are still.
fn same_outline(p: &ShapePath, c: &ClipShape) -> bool {
    if p.geometry == c.geometry && p.transform == c.transform {
        return true;
    }
    let animated = |g: &dxn_vector_animation::Geometry, t: &[TransformOp]| {
        g.is_animated() || t.iter().any(TransformOp::is_animated)
    };
    if animated(&p.geometry, &p.transform) || animated(&c.geometry, &c.transform) {
        return false;
    }
    let path = |g, t: &[TransformOp]| geometry_path(g, 0.0).transformed(transform_at(t, 0.0));
    let (a, b): (Path, Path) = (
        path(&p.geometry, &p.transform),
        path(&c.geometry, &c.transform),
    );
    let (a, b) = (a.elements(), b.elements());
    let near = |p: Point, q: Point| (p.x - q.x).abs() < 1e-6 && (p.y - q.y).abs() < 1e-6;
    use dxn_vector_animation::PathEl::*;
    a.len() == b.len()
        && a.iter().zip(b).all(|pair| match pair {
            (MoveTo(p), MoveTo(q)) | (LineTo(p), LineTo(q)) => near(*p, *q),
            (QuadTo(c, p), QuadTo(d, q)) => near(*c, *d) && near(*p, *q),
            (CubicTo(c1, c2, p), CubicTo(d1, d2, q)) => {
                near(*c1, *d1) && near(*c2, *d2) && near(*p, *q)
            }
            (Close, Close) => true,
            _ => false,
        })
}

/// Whether `fill` covers its outline whole (as a silhouette sees it).
fn fills_whole(fill: &Fill, hard: bool) -> bool {
    let full = |a: f64| harden(a, hard) >= 1.0;
    let opacity = fill.opacity.as_static().is_some_and(|o| full(*o));
    opacity
        && match &fill.paint {
            ScenePaint::Solid { color } => values(color).iter().all(|c| full(c.a)),
            ScenePaint::LinearGradient(g) => {
                values(&g.stops).iter().flatten().all(|s| full(s.color.a))
            }
            ScenePaint::RadialGradient(g) => {
                values(&g.stops).iter().flatten().all(|s| full(s.color.a))
            }
        }
}

/// The shape a silhouette of `g` clipped by `clip` is, where a shape of `g`
/// fills the clip's one outline whole: that outline, filled.
fn covering(g: &Group, clip: &Clip, hard: bool) -> Option<Shape> {
    let [c] = clip.shapes.as_slice() else {
        return None;
    };
    let fills = |s: &Shape| match (s.paths.as_slice(), &s.fill) {
        ([p], Some(fill)) => s.trim.is_none() && same_outline(p, c) && fills_whole(fill, hard),
        _ => false,
    };
    g.children
        .iter()
        .any(|n| matches!(n, Node::Shape(s) if fills(s)))
        .then(|| Shape {
            paths: vec![ShapePath {
                geometry: c.geometry.clone(),
                transform: c.transform.clone(),
            }],
            fill: Some(Fill {
                paint: ScenePaint::solid(Color::rgba(0.0, 0.0, 0.0, 1.0)),
                opacity: Animated::Static(1.0),
                rule: c.rule,
            }),
            ..Default::default()
        })
}

/// Where `ops` part the setup of the groups (transforms) from their clips:
/// each run from a `beginPath` to its `clip`.
fn clip_runs(ops: &[Op]) -> Vec<bool> {
    let mut inside = false;
    ops.iter()
        .map(|op| match op {
            Op::BeginPath => {
                inside = true;
                true
            }
            Op::Clip(_) => {
                inside = false;
                true
            }
            _ => inside,
        })
        .collect()
}

/// The setup `ops` of groups without their clips.
fn transforms_only(ops: &[Op]) -> Vec<Op> {
    ops.iter()
        .zip(clip_runs(ops))
        .filter(|(_, clip)| !clip)
        .map(|(op, _)| op.clone())
        .collect()
}

/// The setup `ops` of groups with their clips taking away what is drawn
/// outside them (`destination-in`).
fn clips_after(ops: &[Op]) -> Vec<Op> {
    let mut out = Vec::new();
    for op in ops {
        match op {
            Op::Clip(rule) => out.extend([
                Op::GlobalAlpha(Num::Const(1.0)),
                Op::FillColor(Paint::Const(0x0000_00ff)),
                Op::Blend(BlendMode::DestinationIn),
                Op::Fill(*rule),
                Op::Blend(BlendMode::Normal),
            ]),
            other => out.push(other.clone()),
        }
    }
    out
}

/// The shadow's operation.
fn shadow_op(color: Paint, blur: f64, offset: (f64, f64)) -> Op {
    Op::Shadow {
        color: match color {
            Paint::Const(c) => c | 0xff,
            // Colored over afterwards.
            Paint::Node(_) => 0xffff_ffff,
        },
        blur,
        offset,
    }
}

/// Coloring over what the canvas holds with a hue that moves.
fn recolor(color: Paint) -> Vec<Op> {
    let Paint::Node(_) = color else {
        return Vec::new();
    };
    let mut ops = vec![Op::Save, Op::Atop, Op::FillColor(color), Op::BeginPath];
    ops.extend(huge_rect(FAR));
    ops.extend([Op::Fill(Default::default()), Op::Restore]);
    ops
}

impl Compiler {
    /// The drop shadows of `g` as the canvas draws them (in the view's
    /// points, or in the group's lengths where it turns or scales: see
    /// [`Self::shadow_units`]); an error for blurs, which are rasterized.
    pub(super) fn canvas_shadows(
        &mut self,
        g: &Group,
        local: NodeId,
    ) -> Result<Shadows, Unsupported> {
        if g.effects.is_empty() {
            return Ok(Shadows::default());
        }
        if self.casting.is_some() {
            return unsupported("a shadow of a shadow");
        }
        if g.effects.iter().any(|e| matches!(e, Effect::Blur(_))) {
            return unsupported("a blur");
        }
        let linear = self.still_linear(g);
        let m = linear.unwrap_or(Matrix::IDENTITY);
        let mut out = Vec::new();
        for effect in &g.effects {
            let Effect::DropShadow(shadow) = effect else {
                continue;
            };
            let (offset, blur, unit) = {
                {
                    let scale = (m.a * m.d - m.b * m.c).abs().sqrt();
                    let mut along = |kx: f64, ky: f64| {
                        let x = self.g.track(&shadow.offset.x, local, |v| kx * v);
                        let y = self.g.track(&shadow.offset.y, local, |v| ky * v);
                        self.g.math(MathOp::Add, x, y)
                    };
                    let offset = [along(m.a, m.c), along(m.b, m.d)];
                    let blur = match &shadow.blur {
                        Animated::Static(b) => ViewBlur::Still(b.max(0.0) * scale),
                        moving => {
                            let times = sample_times(&[moving]);
                            let (lo, hi) =
                                min_max(times.iter().map(|&t| moving.sample(t).max(0.0) * scale));
                            let node = self.g.track(moving, local, |v| v.max(0.0) * scale);
                            ViewBlur::Moving { lo, hi, node }
                        }
                    };
                    (offset, blur, scale)
                }
            };
            let colors = values(&shadow.color);
            let Some(first) = colors.first().copied() else {
                continue;
            };
            let color = if colors
                .iter()
                .all(|c| (c.r, c.g, c.b) == (first.r, first.g, first.b))
            {
                Paint::Const(pack(first) | 0xff)
            } else {
                let opaque = mapped(&shadow.color, |c| Color { a: 1.0, ..*c });
                self.g.color_track(&opaque, local, 1.0)
            };
            let alpha = match &shadow.color {
                Animated::Static(c) => Num::Const(c.a.clamp(0.0, 1.0)),
                Animated::Track(t) => {
                    let a = Animated::Track(Track::new(
                        t.keyframes
                            .iter()
                            .map(|k| Keyframe::new(k.time, k.value.a, k.easing.clone()))
                            .collect(),
                    ));
                    self.g.track(&a, local, |v| v.clamp(0.0, 1.0))
                }
            };
            if alpha.is(0.0) {
                continue;
            }
            let levels = match blur {
                ViewBlur::Still(sigma) => vec![(sigma, Num::Const(1.0))],
                ViewBlur::Moving { lo, hi, node } => self.blur_levels(lo, hi, node, unit)?,
            };
            for (sigma, weight) in levels {
                let alpha = self.mul(alpha, weight);
                out.push(CanvasShadow {
                    color,
                    alpha,
                    blur: 2.0 * sigma,
                    offset,
                    knockout: shadow.knockout,
                    hard: shadow.hard,
                });
            }
        }
        let turning = match (linear, out.is_empty()) {
            (None, false) => Some(self.chain_transform(g)),
            _ => None,
        };
        Ok(Shadows { list: out, turning })
    }

    /// Blurs from `lo` to `hi` (deviations) a moving one (`node`) is drawn
    /// at, each with its share as the blur moves: each a step past the one
    /// before (a share of it, or `unit` at least).
    fn blur_levels(
        &mut self,
        lo: f64,
        hi: f64,
        node: Num,
        unit: f64,
    ) -> Result<Vec<(f64, Num)>, Unsupported> {
        let step = BLUR_STEP * unit.max(1e-9);
        let mut at = vec![lo];
        while let Some(&last) = at.last()
            && last < hi
        {
            if at.len() >= BLUR_LEVELS {
                return unsupported("a drop shadow whose blur moves far");
            }
            at.push((last + step.max(last * BLUR_RATIO)).min(hi));
        }
        if at.len() < 2 {
            at.push(hi + step);
        }
        let n = at.len() - 1;
        Ok((0..=n)
            .map(|k| {
                let (inputs, outputs): (Vec<f64>, Vec<f64>) = match k {
                    0 => (vec![at[0], at[1]], vec![1.0, 0.0]),
                    k if k == n => (vec![at[n - 1], at[n]], vec![0.0, 1.0]),
                    k => (vec![at[k - 1], at[k], at[k + 1]], vec![0.0, 1.0, 0.0]),
                };
                (at[k], self.g.lookup(node, &inputs, &outputs))
            })
            .collect())
    }

    /// The linear part of the transforms from `g`'s lengths to the view's,
    /// where none of them turns or scales over time.
    fn still_linear(&self, g: &Group) -> Option<Matrix> {
        let mut m = self.base;
        let groups = self.ancestors.iter().map(|(a, _)| a).chain([g]);
        for group in groups {
            if group.transform.iter().any(turns) {
                return None;
            }
            m = m * transform_at(&group.transform, 0.0);
        }
        Some(Matrix {
            e: 0.0,
            f: 0.0,
            ..m
        })
    }

    /// How `g`'s lengths move in the view, where it or a group it is in
    /// turns or scales: its matrix to the view, sampled from the scene's
    /// frames, as a layer's transform (`g` not yet among the groups drawn
    /// inside).
    fn chain_transform(&mut self, g: &Group) -> LayerTransform {
        let mut probe = shell(g);
        probe.effects = Vec::new();
        probe.clip = None;
        probe.mask = None;
        probe.opacity = Animated::Static(1.0);
        probe.active = None;
        probe.children = vec![Node::shape(Shape {
            paths: vec![ShapePath::new(dxn_vector_animation::Geometry::Rect {
                center: dxn_vector_animation::AnimatedPoint::fixed(0.5, 0.5),
                size: dxn_vector_animation::AnimatedPoint::fixed(1.0, 1.0),
                radius: dxn_vector_animation::AnimatedPoint::fixed(0.0, 0.0),
                direction: dxn_vector_animation::Direction::Clockwise,
                start: dxn_vector_animation::RectStart::TopLeft,
            })],
            fill: Some(Fill {
                paint: ScenePaint::solid(Color::rgba(0.0, 0.0, 0.0, 1.0)),
                opacity: Animated::Static(1.0),
                rule: Default::default(),
            }),
            ..Default::default()
        })];
        let mut part = Node::Group(probe);
        for (ancestor, _) in self.ancestors.iter().rev() {
            let mut a = ancestor.clone();
            a.opacity = Animated::Static(1.0);
            a.active = None;
            a.clip = None;
            a.mask = None;
            a.effects = Vec::new();
            a.children = vec![part];
            part = Node::Group(a);
        }
        let scene = Scene {
            root: Group {
                children: vec![part],
                ..Group::new()
            },
            ..self.stage.clone()
        };
        let times = probe_times(self.stage.timeline.duration);
        let mut parts: [Vec<f64>; 6] = Default::default();
        let mut last = Matrix::IDENTITY;
        for &t in &times {
            if let Some(m) = scene.frame_at(t).items.iter().find_map(|item| match item {
                Item::Fill { transform, .. } => Some(*transform),
                _ => None,
            }) {
                last = m;
            }
            let d = decompose(self.base * last);
            let values = [
                d.translate.0,
                d.translate.1,
                d.angle,
                d.skew.atan().to_degrees(),
                d.scale.0,
                d.scale.1,
            ];
            for (list, v) in parts.iter_mut().zip(values) {
                list.push(v);
            }
        }
        unwrap_angles(&mut parts[2]);
        // Within a twentieth of a point, a hundredth of a degree, a
        // thousandth of a scale.
        let tolerances = [0.05, 0.05, 0.01, 0.01, 1e-3, 1e-3];
        let [tx, ty, rotate, skew, sx, sy] = [0, 1, 2, 3, 4, 5].map(|i| {
            let n = self.sampled_num(&times, &parts[i], tolerances[i]);
            Num::Node(self.node(n))
        });
        LayerTransform {
            translate: (tx, ty),
            rotate,
            skew,
            scale: (sx, sy),
        }
    }

    /// `values` at `times` (of the playback) as a node: the samples a line
    /// does not stand for within `tolerance`.
    fn sampled_num(&mut self, times: &[f64], values: &[f64], tolerance: f64) -> Num {
        let (inputs, outputs) = simplified(times, values, tolerance);
        if inputs.len() < 2 {
            return Num::Const(outputs.first().copied().unwrap_or(0.0));
        }
        Num::Node(self.g.add(Expr::Interp {
            input: TIME,
            inputs,
            outputs,
            curve: Curve::Linear,
        }))
    }

    /// `a` hardened where `hard` (see [`harden`]).
    pub(super) fn hardened(&mut self, a: Num, hard: bool) -> Num {
        match a {
            _ if !hard => a,
            Num::Const(c) => Num::Const(harden(c, true)),
            node => self.g.lookup(node, &[0.0, 1.0 / 127.0], &[0.0, 1.0]),
        }
    }

    /// The silhouette `g`'s children cast, in the setup of the groups it is
    /// in (the operations after it): each shape in black as see-through as
    /// it (all or nothing where `hard`), a group clipped to an outline one
    /// of its shapes fills whole as that outline; an error for what the
    /// canvas casts no silhouette of (other clips, masks, effects and
    /// paints of a canvas of their own inside).
    pub(super) fn casters(
        &mut self,
        g: &Group,
        local: NodeId,
        depth: usize,
        hard: bool,
    ) -> Result<Vec<Op>, Unsupported> {
        let prologue = 1 + self.context.iter().map(Vec::len).sum::<usize>();
        self.layers.push(Open {
            opacity: Num::Const(1.0),
            blend: BlendMode::Normal,
            units: Vec::new(),
            canvas: Vec::new(),
            alpha: Num::Const(1.0),
        });
        let outer = self.casting.replace(hard);
        let mut done = Ok(());
        for child in &g.children {
            done = self.child(child, local, depth);
            if done.is_err() {
                break;
            }
        }
        self.casting = outer;
        let open = self.layers.pop();
        done?;
        let Some(open) = open else {
            return Ok(Vec::new());
        };
        if !open.units.is_empty() {
            return unsupported("a shadow of more than one canvas");
        }
        let ops = open
            .canvas
            .get(prologue..)
            .map(<[Op]>::to_vec)
            .unwrap_or_default();
        if ops.iter().any(|op| {
            matches!(
                op,
                Op::Clip(_) | Op::Atop | Op::Blend(_) | Op::Shadow { .. }
            )
        }) {
            return unsupported("a shadow of clipped content");
        }
        Ok(ops)
    }

    /// `g` (clipped by `clip`, where it is) as its silhouette casts it.
    pub(super) fn cast_group(
        &mut self,
        g: &Group,
        time: NodeId,
        depth: usize,
        hard: bool,
    ) -> Result<(), Unsupported> {
        if g.mask.is_some() {
            return unsupported("a shadow of masked content");
        }
        let proxy;
        let g = match &g.clip {
            None => g,
            Some(clip) => match covering(g, clip, hard) {
                Some(shape) => {
                    proxy = Group {
                        clip: None,
                        children: vec![Node::shape(shape)],
                        ..shell(g)
                    };
                    &proxy
                }
                None => return unsupported("a shadow of clipped content"),
            },
        };
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
        let opacity = self.g.track(&g.opacity, local, |o| o.clamp(0.0, 1.0));
        let opacity = self.mul(opacity, visible);
        if opacity.is(0.0) {
            return Ok(());
        }
        let mut frame = vec![Op::Save];
        frame.extend(self.transform(&g.transform, local)?);
        let outer_alpha = self.open().alpha;
        let alpha = self.mul(outer_alpha, opacity);
        self.open().alpha = alpha;
        if !self.open().canvas.is_empty() {
            self.open().canvas.extend(frame.iter().cloned());
        }
        self.context.push(frame);
        self.ancestors.push((shell(g), false));
        let mut done = Ok(());
        for child in &g.children {
            done = self.child(child, local, depth);
            if done.is_err() {
                break;
            }
        }
        self.ancestors.pop();
        self.context.pop();
        if !self.open().canvas.is_empty() {
            self.open().canvas.push(Op::Restore);
        }
        self.open().alpha = outer_alpha;
        done
    }

    /// Whether drawing `ops` leaves anything see-through where it draws
    /// (its edges aside): a fainter alpha, color or stop, or an image.
    fn see_through(&self, ops: &[Op]) -> bool {
        let times = probe_times(self.stage.timeline.duration);
        let faint = |a: f64| a > 0.001 && a < 0.999;
        let low = |n: Num| match n {
            Num::Const(c) => faint(c),
            Num::Node(id) => times.iter().any(|&t| faint(self.g.eval(id, t))),
        };
        let clear = |p: Paint| match p {
            Paint::Const(c) => c & 0xff < 0xff,
            Paint::Node(id) => times
                .iter()
                .any(|&t| self.g.eval_color(id, t) & 0xff < 0xff),
        };
        let stops = |g: &Gradient| match g {
            Gradient::Linear { stops, .. } | Gradient::Radial { stops, .. } => {
                stops.iter().any(|(_, c)| c & 0xff < 0xff)
            }
        };
        ops.iter().any(|op| match op {
            Op::GlobalAlpha(a) => low(*a),
            Op::FillColor(p) | Op::StrokeColor(p) => clear(*p),
            Op::FillGradient(g) | Op::StrokeGradient(g) => stops(g),
            Op::Image(..) => true,
            _ => false,
        })
    }

    /// The layers of `shadows` of `g`, bottom first, cast by its content
    /// `content` (a canvas's operations: the setup of the groups it is in,
    /// `outer` of them, then what it draws).
    pub(super) fn shadow_units(
        &mut self,
        g: &Group,
        local: NodeId,
        depth: usize,
        content: &[Op],
        outer: usize,
        shadows: Shadows,
    ) -> Result<Vec<Unit>, Unsupported> {
        let (setup, drawn) = content.split_at(outer.min(content.len()));
        let see_through = self.see_through(drawn);
        // A group that turns or scales: its shadows cast in its lengths,
        // in layers moving as it does (the groups' clips would not).
        let turning = shadows.turning;
        // The groups' clips in the turning group's lengths.
        let clips = match &turning {
            Some(_) if setup.iter().any(|op| matches!(op, Op::Clip(_))) => {
                let frames = self.context.clone();
                self.clips_within(&frames)?
            }
            _ => Vec::new(),
        };
        // Soft and hard silhouettes, as the shadows need them.
        let mut casters: [Option<Vec<Op>>; 2] = [None, None];
        let mut units = Vec::new();
        for shadow in shadows.list {
            let k = usize::from(shadow.hard);
            if casters[k].is_none() {
                casters[k] = Some(self.casters(g, local, depth, shadow.hard)?);
            }
            let cast = casters[k].clone().unwrap_or_default();
            if cast.is_empty() {
                continue;
            }
            let moving = shadow.offset.iter().any(|n| matches!(n, Num::Node(_)));
            let past = moving || (!shadow.knockout && see_through);
            let unit = match (&turning, past) {
                (None, true) => self.cast_past(setup, &cast, &shadow),
                (None, false) => Unit::Canvas(self.cast_in_place(setup, &cast, &shadow)),
                (Some(_), past) => self.cast_in_area(&cast, &shadow, past, &clips),
            };
            units.push(Unit::Layer {
                opacity: shadow.alpha,
                blend: BlendMode::Normal,
                units: vec![unit],
                transform: turning.clone(),
            });
        }
        Ok(units)
    }

    /// `cast` as what takes the shadow away again: opaque wherever it draws.
    fn cut(&mut self, cast: &[Op]) -> Vec<Op> {
        cast.iter()
            .map(|op| match op {
                Op::FillColor(_) | Op::FillGradient(_) => Op::FillColor(Paint::Const(0x0000_00ff)),
                Op::StrokeColor(_) | Op::StrokeGradient(_) => {
                    Op::StrokeColor(Paint::Const(0x0000_00ff))
                }
                Op::GlobalAlpha(a) => Op::GlobalAlpha(self.g.step(*a, 0.0, true)),
                other => other.clone(),
            })
            .collect()
    }

    /// The shadow cast by `cast` drawn in place in the setup `setup`, the
    /// casters then taken away (and the shadow under them).
    fn cast_in_place(&mut self, setup: &[Op], cast: &[Op], s: &CanvasShadow) -> Vec<Op> {
        let still = |n: Num| match n {
            Num::Const(c) => c,
            Num::Node(_) => 0.0,
        };
        let mut ops = vec![Op::Save];
        ops.extend(setup.iter().cloned());
        ops.push(shadow_op(
            s.color,
            s.blur,
            (still(s.offset[0]), still(s.offset[1])),
        ));
        ops.extend(cast.iter().cloned());
        ops.extend([
            shadow_op(Paint::Const(0), 0.0, (0.0, 0.0)),
            Op::Blend(BlendMode::DestinationOut),
        ]);
        ops.extend(self.cut(cast));
        let mut ops = closed(ops);
        ops.extend(recolor(s.color));
        ops
    }

    /// The shadow cast by `cast` drawn past the view (on a canvas reaching
    /// left of it over the casters), falling into it, then clipped and
    /// knocked out there.
    fn cast_past(&mut self, setup: &[Op], cast: &[Op], s: &CanvasShadow) -> Unit {
        let transforms = transforms_only(setup);
        let mut drawing = transforms.clone();
        drawing.extend(cast.iter().cloned());
        // Where the casters are, moved by the offset, over time.
        let Some(b) = self.drawn_bounds(&drawing, s.offset) else {
            return Unit::Canvas(Vec::new());
        };
        // The view's bottom, about: the scene's in it.
        let (w, h) = (self.stage.width, self.stage.height);
        let bottom = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)]
            .iter()
            .map(|&(x, y)| self.base.apply(Point::new(x, y)).y)
            .fold(0.0, f64::max);
        // Moved left past the view, the canvas reaching over them.
        let dx = -(b.x + b.width) - PAST;
        let margin = [
            b.width + PAST,
            (-b.y).max(0.0),
            0.0,
            (b.y + b.height - bottom).max(0.0),
        ]
        .map(f64::ceil);
        let at = Op::Translate(Num::Const(margin[0]), Num::Const(margin[1]));
        let moved = self.g.math(MathOp::Add, s.offset[0], Num::Const(dx));
        let mut ops = vec![Op::Save, at.clone(), Op::Translate(moved, s.offset[1])];
        ops.extend(transforms.iter().cloned());
        ops.push(shadow_op(s.color, s.blur, (-dx, 0.0)));
        ops.extend(cast.iter().cloned());
        let mut ops = closed(ops);
        // The groups' clips, then the content's knock-out.
        if transforms.len() < setup.len() {
            let mut clip = vec![Op::Save, at.clone()];
            clip.extend(clips_after(setup));
            ops.extend(closed(clip));
        }
        if s.knockout {
            let mut knock = vec![Op::Save, at];
            knock.extend(transforms);
            knock.push(Op::Blend(BlendMode::DestinationOut));
            knock.extend(self.cut(cast));
            ops.extend(closed(knock));
        }
        ops.extend(recolor(s.color));
        Unit::Wide { margin, ops }
    }

    /// The shadow cast by `cast` in the group's own lengths, on a canvas
    /// over where it falls: drawn in place and taken away again, or, `past`
    /// that, falling into it from casters past it.
    fn cast_in_area(&mut self, cast: &[Op], s: &CanvasShadow, past: bool, clips: &[Op]) -> Unit {
        let Some(b) = self.drawn_bounds(cast, s.offset) else {
            return Unit::Canvas(Vec::new());
        };
        let still = self.drawn_bounds(cast, [Num::Const(0.0); 2]).unwrap_or(b);
        // Where the shadow falls, and the casters left of it.
        let shown = b.union(still).grown(1.5 * s.blur + 2.0);
        let dx = shown.x - (b.x + b.width) - PAST;
        let area = if past {
            shown.union(Rect::new(b.x + dx, b.y, b.width, b.height))
        } else {
            shown
        };
        let area = Rect::new(
            area.x.floor(),
            area.y.floor(),
            (area.x + area.width).ceil() - area.x.floor(),
            (area.y + area.height).ceil() - area.y.floor(),
        );
        let at = Op::Translate(Num::Const(-area.x), Num::Const(-area.y));
        let mut ops = vec![Op::Save, at.clone()];
        if past {
            let moved = self.g.math(MathOp::Add, s.offset[0], Num::Const(dx));
            ops.push(Op::Translate(moved, s.offset[1]));
            ops.push(shadow_op(s.color, s.blur, (-dx, 0.0)));
            ops.extend(cast.iter().cloned());
            ops.push(Op::Restore);
            if s.knockout {
                ops.extend([Op::Save, at, Op::Blend(BlendMode::DestinationOut)]);
                ops.extend(self.cut(cast));
                ops.push(Op::Restore);
            }
        } else {
            let still = |n: Num| match n {
                Num::Const(c) => c,
                Num::Node(_) => 0.0,
            };
            ops.push(shadow_op(
                s.color,
                s.blur,
                (still(s.offset[0]), still(s.offset[1])),
            ));
            ops.extend(cast.iter().cloned());
            ops.extend([
                shadow_op(Paint::Const(0), 0.0, (0.0, 0.0)),
                Op::Blend(BlendMode::DestinationOut),
            ]);
            ops.extend(self.cut(cast));
            ops.push(Op::Restore);
        }
        if !clips.is_empty() {
            let mut clip = vec![
                Op::Save,
                Op::Translate(Num::Const(-area.x), Num::Const(-area.y)),
            ];
            clip.extend(clips.iter().cloned());
            ops.extend(closed(clip));
        }
        ops.extend(recolor(s.color));
        let shown = Rect::new(
            shown.x.floor(),
            shown.y.floor(),
            (shown.x + shown.width).ceil() - shown.x.floor(),
            (shown.y + shown.height).ceil() - shown.y.floor(),
        );
        Unit::Area {
            area,
            shown: past.then_some(shown),
            ops,
        }
    }

    /// The clips of `frames` (the setup of the groups drawn inside, the
    /// last a turning group's) taking away what is drawn outside them, in
    /// the last group's lengths: each built after undoing the transforms of
    /// the groups between it and the last; an error where one of those
    /// cannot be undone on the graph (a matrix that moves otherwise than
    /// skewing along x).
    fn clips_within(&mut self, frames: &[Vec<Op>]) -> Result<Vec<Op>, Unsupported> {
        let mut out = Vec::new();
        for (k, frame) in frames.iter().enumerate() {
            let runs = clip_runs(frame);
            if !runs.iter().any(|&clip| clip) {
                continue;
            }
            out.push(Op::Save);
            for later in frames[k + 1..].iter().rev() {
                let transforms: Vec<Op> = later
                    .iter()
                    .zip(clip_runs(later))
                    .filter(|(op, clip)| !clip && !matches!(op, Op::Save | Op::Restore))
                    .map(|(op, _)| op.clone())
                    .collect();
                for op in transforms.iter().rev() {
                    out.push(self.inverse(op)?);
                }
            }
            let clip: Vec<Op> = frame
                .iter()
                .zip(&runs)
                .filter(|(_, clip)| **clip)
                .map(|(op, _)| op.clone())
                .collect();
            out.extend(clips_after(&clip));
            out.push(Op::Restore);
        }
        Ok(out)
    }

    /// The transform undoing `op`.
    fn inverse(&mut self, op: &Op) -> Result<Op, Unsupported> {
        let neg = |c: &mut Self, n: Num| c.g.math(MathOp::Mul, n, Num::Const(-1.0));
        let inv = |c: &mut Self, n: Num| c.g.math(MathOp::Div, Num::Const(1.0), n);
        Ok(match *op {
            Op::Translate(x, y) => Op::Translate(neg(self, x), neg(self, y)),
            Op::Scale(x, y) => Op::Scale(inv(self, x), inv(self, y)),
            Op::Rotate(a) => Op::Rotate(neg(self, a)),
            Op::Transform([a, b, c, d, e, f]) => {
                let one = |n: Num| n.is(1.0);
                let zero = |n: Num| n.is(0.0);
                if let (Num::Const(a), Num::Const(b), Num::Const(c), Num::Const(d)) = (a, b, c, d)
                    && let (Num::Const(e), Num::Const(f)) = (e, f)
                {
                    let m = Matrix::new(a, b, c, d, e, f);
                    let Some(i) = m.invert() else {
                        return unsupported("a shadow of a group flattened to a line");
                    };
                    Op::Transform([i.a, i.b, i.c, i.d, i.e, i.f].map(Num::Const))
                } else if one(a) && zero(b) && one(d) && zero(e) && zero(f) {
                    // A skew along x.
                    Op::Transform([a, b, neg(self, c), d, e, f])
                } else {
                    return unsupported("a shadow of a group whose matrix moves");
                }
            }
            _ => return unsupported("a shadow of a group whose setup draws"),
        })
    }

    /// Where `ops` (from the view's origin) draw over the playback, moved by
    /// `offset`, about: their path points (curves within their control
    /// points) through their transforms, grown by the widest line.
    fn drawn_bounds(&self, ops: &[Op], offset: [Num; 2]) -> Option<Rect> {
        let moves = offset
            .iter()
            .copied()
            .chain(ops.iter().flat_map(op_nums))
            .any(|n| matches!(n, Num::Node(_)));
        let times = if moves {
            probe_times(self.stage.timeline.duration)
        } else {
            vec![0.0]
        };
        let mut out: Option<Rect> = None;
        for t in times {
            let Some(r) = self.bounds_at(ops, t) else {
                continue;
            };
            let (dx, dy) = (self.g.num(offset[0], t), self.g.num(offset[1], t));
            let r = Rect::new(r.x + dx, r.y + dy, r.width, r.height);
            out = Some(out.map_or(r, |o| o.union(r)));
        }
        out
    }

    /// Where `ops` draw at `time` (see [`Self::drawn_bounds`]).
    fn bounds_at(&self, ops: &[Op], time: f64) -> Option<Rect> {
        let n = |v: Num| self.g.num(v, time);
        let mut m = Matrix::IDENTITY;
        let mut stack = Vec::new();
        let (mut reach, mut miter) = (0.0f64, 1.0f64);
        let mut points: Vec<Point> = Vec::new();
        for op in ops {
            match op {
                Op::Save => stack.push(m),
                Op::Restore => m = stack.pop().unwrap_or(Matrix::IDENTITY),
                Op::Translate(x, y) => m = m * Matrix::translate(n(*x), n(*y)),
                Op::Scale(x, y) => m = m * Matrix::scale(n(*x), n(*y)),
                Op::Rotate(a) => m = m * Matrix::rotate(n(*a).to_degrees()),
                Op::Transform(v) => {
                    let v = v.map(n);
                    m = m * Matrix::new(v[0], v[1], v[2], v[3], v[4], v[5]);
                }
                Op::MoveTo(x, y) | Op::LineTo(x, y) => {
                    points.push(m.apply(Point::new(n(*x), n(*y))))
                }
                Op::QuadTo(v) => {
                    for p in v.chunks(2) {
                        points.push(m.apply(Point::new(n(p[0]), n(p[1]))));
                    }
                }
                Op::CubicTo(v) => {
                    for p in v.chunks(2) {
                        points.push(m.apply(Point::new(n(p[0]), n(p[1]))));
                    }
                }
                Op::RoundRect([x, y, w, h], _) => {
                    let (x, y, w, h) = (n(*x), n(*y), n(*w), n(*h));
                    for (px, py) in [(x, y), (x + w, y), (x, y + h), (x + w, y + h)] {
                        points.push(m.apply(Point::new(px, py)));
                    }
                }
                Op::Ellipse([cx, cy, rx, ry], _) => {
                    let (cx, cy, rx, ry) = (n(*cx), n(*cy), n(*rx).abs(), n(*ry).abs());
                    for (px, py) in [
                        (cx - rx, cy - ry),
                        (cx + rx, cy - ry),
                        (cx - rx, cy + ry),
                        (cx + rx, cy + ry),
                    ] {
                        points.push(m.apply(Point::new(px, py)));
                    }
                }
                Op::LineWidth(w) => {
                    let scale = (m.a * m.d - m.b * m.c).abs().sqrt();
                    reach = reach.max(n(*w).abs() * scale);
                }
                Op::MiterLimit(l) => miter = miter.max(*l),
                Op::Image(_, r) => {
                    for (px, py) in [
                        (r.x, r.y),
                        (r.x + r.width, r.y),
                        (r.x, r.y + r.height),
                        (r.x + r.width, r.y + r.height),
                    ] {
                        points.push(m.apply(Point::new(px, py)));
                    }
                }
                _ => {}
            }
        }
        let (x0, y0, x1, y1) = points.iter().fold(
            (
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ),
            |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
        );
        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            return None;
        }
        Some(Rect::new(x0, y0, x1 - x0, y1 - y0).grown(reach * miter.max(1.5) / 2.0 + 1.0))
    }
}

/// The numbers `op` reads.
fn op_nums(op: &Op) -> Vec<Num> {
    match op {
        Op::Translate(a, b) | Op::Scale(a, b) | Op::MoveTo(a, b) | Op::LineTo(a, b) => vec![*a, *b],
        Op::Rotate(a) | Op::LineWidth(a) | Op::GlobalAlpha(a) | Op::LineDashOffset(a) => vec![*a],
        Op::Transform(v) | Op::CubicTo(v) => v.to_vec(),
        Op::QuadTo(v) => v.to_vec(),
        Op::RoundRect(v, r) => {
            let mut out = v.to_vec();
            out.push(*r);
            out
        }
        Op::Ellipse(v, _) => v.to_vec(),
        _ => Vec::new(),
    }
}
