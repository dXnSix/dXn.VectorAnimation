//! A Lottie composition turned into a scene: layers (shape, null, nested
//! composition, solid, image), parenting, time offsets, stretches and
//! remapping, masks and track mattes, shape groups and their modifiers.

mod text;

use std::{
    cell::Cell,
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use dxn_vector_animation::{
    Animated, AnimatedPoint, BlendMode, Blur, Clip, ClipShape, Color, ColorEffect, ColorOp, Dash,
    Direction, DropShadow, Easing, Effect, EllipseStart, Error, Fill, FillRule, Geometry,
    GradientStop, Group, Image, ImageData, InnerShadow, Keyframe, Limits, LineCap, LineJoin,
    LinearGradient, LoopMode, Marker, Mask, MaskKind, Matrix, MotionPath, MotionRotate, Node,
    Paint, Path, Point, RadialGradient, Rect, RectStart, Scene, Shape, ShapePath, Star, StarKind,
    Stroke, TimeMap, TimeRange, Timeline, Track, Transfer, TransformOp, Trim, TrimMode,
    outline::{geometry_path, path_length, trim_paths},
    transform::transform_at,
};
use serde_json::Value;

use crate::{
    modifiers::{
        Join, MAX_POINTS, MergeMode, contours, merge, offset, path_of, pucker_bloat, round_corners,
        twist, zig_zag,
    },
    props::{
        self, Prop, color, gradient_stops, key_times, num, nums, prop, sampled, sampled_color,
        scalar, shape,
    },
};

/// A rectangle far larger than any composition: what an inverted mask
/// shows outside its shape.
const HUGE: f64 = 1.0e6;

/// The conversion of one file.
pub(crate) struct Converter<'a> {
    fr: f64,
    assets: HashMap<&'a str, &'a Value>,
    pub(crate) warnings: BTreeSet<String>,
    limits: Limits,
    depth: usize,
    nodes: usize,
    /// The file's fonts (`fonts.list`) and glyphs (`chars`).
    fonts: Vec<&'a Value>,
    chars: Vec<&'a Value>,
    /// Glyph outlines by their `chars` index, converted once.
    glyphs: HashMap<usize, Vec<ShapePath>>,
    /// Keyframes text animators were sampled into so far.
    text_keyframes: usize,
    /// Path points and keyframes left for the outlines shape modifiers
    /// make.
    bake_points: usize,
    bake_keys: usize,
}

impl<'a> Converter<'a> {
    fn warn(&mut self, message: impl Into<String>) {
        self.warnings.insert(message.into());
    }

    pub(crate) fn run(root: &'a Value, limits: Limits) -> Result<(Scene, BTreeSet<String>), Error> {
        let field = |k: &str| root.get(k).and_then(num);
        let fr = field("fr")
            .filter(|f| *f > 0.0)
            .ok_or_else(|| Error::Invalid("no frame rate (`fr`)".into()))?;
        let ip = field("ip").unwrap_or(0.0);
        let op = field("op").ok_or_else(|| Error::Invalid("no out point (`op`)".into()))?;
        let width = field("w").unwrap_or(0.0);
        let height = field("h").unwrap_or(0.0);
        let layers = root
            .get("layers")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Invalid("no layers".into()))?;
        let duration = ((op - ip) / fr).max(0.0);
        if duration > limits.max_duration {
            return Err(Error::Limit {
                what: "seconds of timeline",
                limit: limits.max_duration,
            });
        }
        let mut assets = HashMap::new();
        for asset in root
            .get("assets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = asset.get("id").and_then(Value::as_str) {
                assets.insert(id, asset);
            }
        }
        let mut converter = Converter {
            fr,
            assets,
            warnings: BTreeSet::new(),
            limits,
            depth: 0,
            nodes: 0,
            fonts: root
                .get("fonts")
                .and_then(|f| f.get("list"))
                .and_then(Value::as_array)
                .map(|l| l.iter().collect())
                .unwrap_or_default(),
            chars: root
                .get("chars")
                .and_then(Value::as_array)
                .map(|l| l.iter().collect())
                .unwrap_or_default(),
            glyphs: HashMap::new(),
            text_keyframes: 0,
            bake_points: limits.max_path_points / 2,
            bake_keys: limits.max_keyframes / 2,
        };
        let children = converter.composition(layers)?;
        let markers = root
            .get("markers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| {
                Some(Marker::new(
                    m.get("cm").and_then(Value::as_str).unwrap_or(""),
                    (m.get("tm").and_then(num)? - ip) / fr,
                    m.get("dr").and_then(num).unwrap_or(0.0) / fr,
                ))
            })
            .collect();
        let scene = Scene {
            width,
            height,
            timeline: Timeline {
                duration,
                frame_rate: Some(fr),
                loop_start: 0.0,
                loop_mode: LoopMode::Loop,
                markers,
            },
            root: Group {
                // Scene time 0 is the composition's in point.
                time: (ip != 0.0).then(|| TimeMap {
                    offset: -ip / fr,
                    scale: 1.0,
                    remap: None,
                }),
                children,
                ..Group::new()
            },
        };
        Ok((scene, converter.warnings))
    }

    /// The layers of a composition, bottom first.
    fn composition(&mut self, layers: &'a [Value]) -> Result<Vec<Node>, Error> {
        if self.depth >= self.limits.max_depth {
            return Err(Error::Limit {
                what: "levels of nesting",
                limit: self.limits.max_depth as f64,
            });
        }
        self.depth += 1;
        let by_index: HashMap<i64, &Value> = layers
            .iter()
            .filter_map(|l| Some((l.get("ind").and_then(num)? as i64, l)))
            .collect();
        let mut nodes = Vec::new();
        // The first layer is on top: draw from the last.
        for (position, layer) in layers.iter().enumerate().rev() {
            if layer.get("td").and_then(num).is_some_and(|t| t != 0.0) {
                continue; // A matte, drawn only through the layer it mattes.
            }
            let Some(mut node) = self.layer_with_parents(layer, &by_index)? else {
                continue;
            };
            let matte_type = layer.get("tt").and_then(num).unwrap_or(0.0) as i64;
            if matte_type != 0 {
                let source = match layer.get("tp").and_then(num) {
                    Some(tp) => by_index.get(&(tp as i64)).copied(),
                    None => position.checked_sub(1).and_then(|p| layers.get(p)),
                };
                match source
                    .map(|s| self.layer_with_parents(s, &by_index))
                    .transpose()?
                {
                    Some(Some(matte)) => {
                        let (kind, inverted) = match matte_type {
                            1 => (MaskKind::Alpha, false),
                            2 => (MaskKind::Alpha, true),
                            3 => (MaskKind::Luminance, false),
                            _ => (MaskKind::Luminance, true),
                        };
                        node = Node::Group(Group {
                            mask: Some(Box::new(Mask {
                                kind,
                                inverted,
                                content: Group {
                                    children: vec![matte],
                                    ..Group::new()
                                },
                            })),
                            children: vec![node],
                            ..Group::new()
                        });
                    }
                    _ => self.warn("a track matte without its matte layer is left out"),
                }
            }
            nodes.push(node);
        }
        self.depth -= 1;
        Ok(nodes)
    }

    /// A layer inside the transforms of its parents, each at its own time.
    fn layer_with_parents(
        &mut self,
        layer: &'a Value,
        by_index: &HashMap<i64, &'a Value>,
    ) -> Result<Option<Node>, Error> {
        let Some(mut group) = self.layer(layer)? else {
            return Ok(None);
        };
        // The parents, nearest first.
        let mut parents: Vec<&'a Value> = Vec::new();
        let mut current = layer;
        while let Some(p) = current.get("parent").and_then(num) {
            let Some(parent) = by_index.get(&(p as i64)).copied() else {
                break;
            };
            if parents.len() > 64
                || std::ptr::eq(parent, layer)
                || parents.iter().any(|q| std::ptr::eq(*q, parent))
            {
                self.warn("a parenting cycle is left out");
                break;
            }
            parents.push(parent);
            current = parent;
        }
        let active = group.active.take();
        let mut timing = layer_timing(layer, self.fr);
        group.time = Some(timing.clone());
        for parent in parents {
            let parent_timing = layer_timing(parent, self.fr);
            // The child's time relative to its parent's.
            group.time = Some(TimeMap {
                offset: (timing.offset - parent_timing.offset) / parent_timing.scale,
                scale: timing.scale / parent_timing.scale,
                remap: None,
            });
            let auto_orient = parent.get("ao").and_then(num).is_some_and(|a| a != 0.0);
            let (transform, _) = self.transform(parent.get("ks"), auto_orient);
            group = Group {
                transform,
                children: vec![Node::Group(group)],
                time: Some(parent_timing.clone()),
                ..Group::new()
            };
            timing = parent_timing;
        }
        group.active = active;
        Ok(Some(Node::Group(group)))
    }

    /// One layer's group: its transform, opacity, blend mode, masks and
    /// content; `active` holds its in and out points.
    fn layer(&mut self, layer: &'a Value) -> Result<Option<Group>, Error> {
        if layer.get("hd").and_then(Value::as_bool) == Some(true) {
            return Ok(None);
        }
        self.nodes += 1;
        if self.nodes > self.limits.max_nodes {
            return Err(Error::Limit {
                what: "nodes",
                limit: self.limits.max_nodes as f64,
            });
        }
        let kind = layer.get("ty").and_then(num).unwrap_or(-1.0) as i64;
        let content: Vec<Node> = match kind {
            0 => self.precomp(layer)?,
            1 => self.solid(layer),
            2 => self.image(layer),
            3 => Vec::new(),
            4 => {
                let items = layer
                    .get("shapes")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                self.shapes(items)?.0
            }
            5 => self.text(layer)?,
            _ => Vec::new(),
        };
        if content.is_empty() {
            return Ok(None);
        }
        let mut effects = self.effects(layer);
        effects.extend(self.layer_styles(layer));
        if layer.get("ddd").and_then(num).is_some_and(|d| d != 0.0) {
            self.warn("3D layers are drawn flat");
        }
        let auto_orient = layer.get("ao").and_then(num).is_some_and(|a| a != 0.0);
        let (transform, opacity) = self.transform(layer.get("ks"), auto_orient);
        let ip = layer.get("ip").and_then(num).unwrap_or(f64::NEG_INFINITY);
        let op = layer.get("op").and_then(num).unwrap_or(f64::INFINITY);
        let mask = self.layer_masks(layer);
        Ok(Some(Group {
            id: layer
                .get("ind")
                .and_then(num)
                .map(|i| (i as i64).to_string()),
            name: layer.get("nm").and_then(Value::as_str).map(str::to_owned),
            transform,
            opacity,
            blend: blend_mode(layer.get("bm").and_then(num).unwrap_or(0.0) as i64, self),
            mask: mask.map(Box::new),
            effects,
            // A layer playing backwards has its in point after its out point.
            active: Some(TimeRange {
                start: ip.min(op) / self.fr,
                end: ip.max(op) / self.fr,
            }),
            children: content,
            ..Group::new()
        }))
    }

    /// A nested composition, clipped to its size, at its remapped time.
    fn precomp(&mut self, layer: &'a Value) -> Result<Vec<Node>, Error> {
        let Some(asset) = layer
            .get("refId")
            .and_then(Value::as_str)
            .and_then(|id| self.assets.get(id).copied())
        else {
            return Ok(Vec::new());
        };
        let Some(layers) = asset.get("layers").and_then(Value::as_array) else {
            return Ok(Vec::new());
        };
        let children = self.composition(layers)?;
        // A remapped time on the layer's out point shows the frame before
        // it, as lottie-web does (the out point itself shows nothing).
        let op = layer.get("op").and_then(num);
        let fr = self.fr;
        let before_end = |v: f64| match op {
            Some(op) if (v * fr - op).abs() < 1e-6 => (op - 1.0) / fr,
            _ => v,
        };
        let remap = layer
            .get("tm")
            .map(|tm| match scalar(Some(tm), 0, 1.0, 0.0, fr) {
                Animated::Static(v) => Animated::Static(before_end(v)),
                Animated::Track(t) => Animated::Track(Track::new(
                    t.keyframes
                        .into_iter()
                        .map(|k| Keyframe::new(k.time, before_end(k.value), k.easing))
                        .collect(),
                )),
            });
        let w = layer.get("w").and_then(num);
        let h = layer.get("h").and_then(num);
        let clip = match (w, h) {
            (Some(w), Some(h)) => Some(Clip {
                shapes: vec![ClipShape {
                    geometry: rect(0.0, 0.0, w, h),
                    transform: Vec::new(),
                    rule: FillRule::NonZero,
                }],
                one_path: false,
            }),
            _ => None,
        };
        Ok(vec![Node::Group(Group {
            clip,
            time: remap.map(|remap| TimeMap {
                offset: 0.0,
                scale: 1.0,
                remap: Some(remap),
            }),
            children,
            ..Group::new()
        })])
    }

    fn solid(&mut self, layer: &Value) -> Vec<Node> {
        let w = layer.get("sw").and_then(num).unwrap_or(0.0);
        let h = layer.get("sh").and_then(num).unwrap_or(0.0);
        let c = layer
            .get("sc")
            .and_then(Value::as_str)
            .and_then(hex_color)
            .unwrap_or(Color::BLACK);
        vec![Node::shape(Shape {
            paths: vec![ShapePath::new(rect(0.0, 0.0, w, h))],
            fill: Some(Fill {
                paint: Paint::solid(c),
                opacity: Animated::Static(1.0),
                rule: FillRule::NonZero,
            }),
            ..Shape::default()
        })]
    }

    fn image(&mut self, layer: &Value) -> Vec<Node> {
        let Some(asset) = layer
            .get("refId")
            .and_then(Value::as_str)
            .and_then(|id| self.assets.get(id).copied())
        else {
            return Vec::new();
        };
        let w = asset.get("w").and_then(num).unwrap_or(0.0);
        let h = asset.get("h").and_then(num).unwrap_or(0.0);
        let Some(data) = asset.get("p").and_then(Value::as_str).and_then(data_uri) else {
            self.warn("images that are not embedded (data:) are left out");
            return Vec::new();
        };
        vec![Node::Image(Image {
            id: asset.get("id").and_then(Value::as_str).map(str::to_owned),
            name: None,
            data: Arc::new(data),
            rect: Rect::new(0.0, 0.0, w, h),
        })]
    }

    /// Layer masks (`masksProperties`) as one alpha mask: added,
    /// subtracted and intersected shapes in order.
    /// A layer's effects (`ef`): drop shadows (25) and Gaussian blurs (29),
    /// converted as lottie-web converts them (a shadow's softness is four
    /// deviations, its direction clockwise from up; a blur's blurriness
    /// 0.3 of one); others are left out with a warning.
    fn effects(&mut self, layer: &Value) -> Vec<Effect> {
        let Some(list) = layer.get("ef").and_then(Value::as_array) else {
            return Vec::new();
        };
        let fr = self.fr;
        let mut out = Vec::new();
        for effect in list {
            if effect.get("en").and_then(num) == Some(0.0) {
                continue;
            }
            let params = effect
                .get("ef")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let param = |i: usize| params.get(i).and_then(|p| p.get("v"));
            let name = effect
                .get("nm")
                .and_then(Value::as_str)
                .unwrap_or("an effect");
            match effect.get("ty").and_then(num).map(|t| t as i64) {
                Some(25) => {
                    if param(5).and_then(prop).is_some_and(|p| match p {
                        Prop::Static(v) => num(v).is_some_and(|v| v != 0.0),
                        Prop::Keys(_) => true,
                    }) {
                        self.warn("a drop shadow showing the shadow only is left out");
                        continue;
                    }
                    let color = color(param(0), fr);
                    let opacity = scalar(param(1), 0, 1.0 / 255.0, 1.0, fr);
                    let direction = scalar(param(2), 0, 1.0, 135.0, fr);
                    let distance = scalar(param(3), 0, 1.0, 5.0, fr);
                    let softness = scalar(param(4), 0, 0.25, 0.0, fr);
                    let mut times = key_times(param(0), fr);
                    times.extend(key_times(param(1), fr));
                    let color = if times.is_empty() {
                        let c = color.sample(0.0);
                        Animated::Static(c.with_opacity(opacity.sample(0.0).clamp(0.0, 1.0)))
                    } else {
                        sampled_color(&times, |t| {
                            color
                                .sample(t)
                                .with_opacity(opacity.sample(t).clamp(0.0, 1.0))
                        })
                    };
                    let mut times = key_times(param(2), fr);
                    times.extend(key_times(param(3), fr));
                    let along = |t: f64, f: fn(f64) -> f64| {
                        let angle = (direction.sample(t) - 90.0).to_radians();
                        distance.sample(t) * f(angle)
                    };
                    out.push(Effect::DropShadow(DropShadow {
                        color,
                        offset: AnimatedPoint {
                            x: sampled(&times, |t| along(t, f64::cos)),
                            y: sampled(&times, |t| along(t, f64::sin)),
                        },
                        blur: softness,
                        knockout: false,
                        hard: false,
                    }));
                }
                Some(29) => {
                    let dimensions = param(1).and_then(prop).and_then(|p| match p {
                        Prop::Static(v) => num(v),
                        Prop::Keys(_) => None,
                    });
                    if dimensions.is_some_and(|d| d != 1.0) {
                        self.warn("a Gaussian blur along one axis is blurred along both");
                    }
                    out.push(Effect::Blur(Blur {
                        radius: scalar(param(0), 0, 0.3, 0.0, fr),
                    }));
                }
                // Tint: the luminance mapped from black to white.
                Some(20) => push_color(
                    &mut out,
                    ColorOp::Tint {
                        black: color(param(0), fr),
                        white: color(param(1), fr),
                        amount: scalar(param(2), 0, 0.01, 1.0, fr),
                    },
                ),
                // Fill: one color, the alpha times the opacity.
                Some(21) => push_color(
                    &mut out,
                    ColorOp::Fill {
                        color: color(param(2), fr),
                        opacity: scalar(param(6), 0, 1.0, 1.0, fr),
                    },
                ),
                // Tritone: shadows, midtones and highlights by luminance,
                // blended with the original.
                Some(23) => {
                    let (highlights, midtones, shadows) = (
                        color(param(0), fr),
                        color(param(1), fr),
                        color(param(2), fr),
                    );
                    let mut times = Vec::new();
                    for i in 0..3 {
                        times.extend(key_times(param(i), fr));
                    }
                    let stops = |t: f64| {
                        vec![
                            GradientStop {
                                offset: 0.0,
                                color: shadows.sample(t),
                            },
                            GradientStop {
                                offset: 0.5,
                                color: midtones.sample(t),
                            },
                            GradientStop {
                                offset: 1.0,
                                color: highlights.sample(t),
                            },
                        ]
                    };
                    times.sort_by(f64::total_cmp);
                    times.dedup();
                    let stops = if times.is_empty() {
                        Animated::Static(stops(0.0))
                    } else {
                        Animated::from_keyframes(
                            times
                                .iter()
                                .map(|&t| Keyframe::new(t, stops(t), Easing::Linear))
                                .collect(),
                            stops(0.0),
                        )
                    };
                    let blend = scalar(param(3), 0, 0.01, 0.0, fr);
                    push_color(
                        &mut out,
                        ColorOp::GradientMap {
                            stops,
                            amount: map(&blend, |b| 1.0 - b),
                        },
                    );
                }
                // Levels: each channel through its curve (the composite one
                // first for the colors), as tables.
                Some(24) => {
                    let value = |i: usize, default: f64| -> f64 {
                        match param(i).and_then(prop) {
                            Some(Prop::Static(v)) => num(v).unwrap_or(default),
                            Some(Prop::Keys(keys)) => {
                                keys.first().and_then(|k| num(k.value)).unwrap_or(default)
                            }
                            None => default,
                        }
                    };
                    if (3..36).any(|i| {
                        param(i)
                            .and_then(prop)
                            .is_some_and(|p| matches!(p, Prop::Keys(_)))
                    }) {
                        self.warn("animated levels keep their first values");
                    }
                    let curve = |at: usize| {
                        let (in_black, in_white, gamma, out_black, out_white) = (
                            value(at, 0.0),
                            value(at + 1, 1.0),
                            value(at + 2, 1.0),
                            value(at + 3, 0.0),
                            value(at + 4, 1.0),
                        );
                        move |c: f64| {
                            if c <= in_black {
                                out_black
                            } else if c >= in_white {
                                out_white
                            } else {
                                let x = (c - in_black) / (in_white - in_black);
                                out_black + (out_white - out_black) * x.powf(1.0 / gamma.max(1e-3))
                            }
                        }
                    };
                    let composite = curve(3);
                    let table = |f: &dyn Fn(f64) -> f64| Transfer::Table {
                        values: (0..=255).map(|i| f(f64::from(i) / 255.0)).collect(),
                    };
                    let (r, g, b, a) = (curve(10), curve(17), curve(24), curve(31));
                    push_color(
                        &mut out,
                        ColorOp::Transfer {
                            red: table(&|c| r(composite(c))),
                            green: table(&|c| g(composite(c))),
                            blue: table(&|c| b(composite(c))),
                            alpha: table(&a),
                        },
                    );
                }
                // Expression controls (sliders, angles, colors): nothing to
                // draw.
                Some(5) => {}
                _ => self.warn(format!("the layer effect `{name}` is left out")),
            }
        }
        out
    }

    /// The layer's styles (`sy`): drop and inner shadows and color
    /// overlays; others are left out with a warning.
    fn layer_styles(&mut self, layer: &Value) -> Vec<Effect> {
        let Some(list) = layer.get("sy").and_then(Value::as_array) else {
            return Vec::new();
        };
        let fr = self.fr;
        let mut out = Vec::new();
        for style in list {
            let get = |key: &str| style.get(key);
            let ty = style.get("ty").and_then(num).map(|t| t as i64);
            match ty {
                Some(1 | 2) => {
                    let base = color(get("c"), fr);
                    let opacity = scalar(get("o"), 0, 0.01, 1.0, fr);
                    let angle = scalar(get("a"), 0, 1.0, 120.0, fr);
                    let distance = scalar(get("d"), 0, 1.0, 5.0, fr);
                    let mut times = key_times(get("c"), fr);
                    times.extend(key_times(get("o"), fr));
                    let shade = if times.is_empty() {
                        Animated::Static(
                            base.sample(0.0)
                                .with_opacity(opacity.sample(0.0).clamp(0.0, 1.0)),
                        )
                    } else {
                        sampled_color(&times, |t| {
                            base.sample(t)
                                .with_opacity(opacity.sample(t).clamp(0.0, 1.0))
                        })
                    };
                    let mut times = key_times(get("a"), fr);
                    times.extend(key_times(get("d"), fr));
                    // The light comes from the angle (counterclockwise
                    // from the right): the shadow falls away from it.
                    let offset = AnimatedPoint {
                        x: sampled(&times, |t| {
                            -distance.sample(t) * angle.sample(t).to_radians().cos()
                        }),
                        y: sampled(&times, |t| {
                            distance.sample(t) * angle.sample(t).to_radians().sin()
                        }),
                    };
                    let blur = scalar(get("s"), 0, 0.5, 0.0, fr);
                    if get("ch").and_then(prop).is_some_and(|p| match p {
                        Prop::Static(v) => num(v).is_some_and(|v| v != 0.0),
                        Prop::Keys(_) => true,
                    }) {
                        self.warn("the spread (choke) of layer style shadows is left out");
                    }
                    out.push(if ty == Some(1) {
                        Effect::DropShadow(DropShadow {
                            color: shade,
                            offset,
                            blur,
                            knockout: false,
                            hard: false,
                        })
                    } else {
                        Effect::InnerShadow(InnerShadow {
                            color: shade,
                            offset,
                            blur,
                            hard: false,
                        })
                    });
                }
                Some(7) => push_color(
                    &mut out,
                    ColorOp::Overlay {
                        color: color(get("c"), fr),
                        amount: scalar(get("so").or(get("o")), 0, 0.01, 1.0, fr),
                    },
                ),
                _ => {
                    let name = match ty {
                        Some(0) => "stroke",
                        Some(3) => "outer glow",
                        Some(4) => "inner glow",
                        Some(5) => "bevel and emboss",
                        Some(6) => "satin",
                        Some(8) => "gradient overlay",
                        _ => "an unknown",
                    };
                    self.warn(format!("the {name} layer style is left out"));
                }
            }
        }
        out
    }

    fn layer_masks(&mut self, layer: &Value) -> Option<Mask> {
        let masks = layer.get("masksProperties")?.as_array()?;
        let mut content = Group::new();
        let mut first = true;
        for m in masks {
            let mode = m.get("mode").and_then(Value::as_str).unwrap_or("a");
            if mode == "n" {
                continue;
            }
            let Some(path) = shape(m.get("pt"), self.fr) else {
                continue;
            };
            if m.get("x")
                .and_then(prop)
                .is_some_and(|p| !matches!(p, Prop::Static(v) if num(v) == Some(0.0)))
            {
                self.warn("mask expansion is left out");
            }
            let inverted = m.get("inv").and_then(Value::as_bool).unwrap_or(false);
            let opacity = scalar(m.get("o"), 0, 0.01, 1.0, self.fr);
            if first && mode == "s" {
                // Subtracting from nothing subtracts from everything.
                content.children.push(Node::shape(white(
                    vec![ShapePath::new(rect(-HUGE, -HUGE, 2.0 * HUGE, 2.0 * HUGE))],
                    Animated::Static(1.0),
                    FillRule::NonZero,
                )));
            }
            let blend = match mode {
                "s" => BlendMode::DestinationOut,
                "i" => BlendMode::DestinationIn,
                "a" => BlendMode::Normal,
                _ => {
                    self.warn("lighten, darken and difference masks are drawn as added");
                    BlendMode::Normal
                }
            };
            let mut paths = vec![ShapePath::new(Geometry::Path { path })];
            let mut rule = FillRule::NonZero;
            if inverted {
                paths.insert(
                    0,
                    ShapePath::new(rect(-HUGE, -HUGE, 2.0 * HUGE, 2.0 * HUGE)),
                );
                rule = FillRule::EvenOdd;
            }
            content.children.push(Node::Group(Group {
                blend,
                children: vec![Node::shape(white(paths, opacity, rule))],
                ..Group::new()
            }));
            first = false;
        }
        (!content.children.is_empty()).then_some(Mask {
            kind: MaskKind::Alpha,
            inverted: false,
            content,
        })
    }

    /// A Lottie transform (`ks`, `tr`): position (or a motion path), rotation,
    /// skew, scale and anchor, and the opacity.
    fn transform(
        &mut self,
        ks: Option<&'a Value>,
        auto_orient: bool,
    ) -> (Vec<TransformOp>, Animated<f64>) {
        let Some(ks) = ks else {
            return (Vec::new(), Animated::Static(1.0));
        };
        if ks.get("rx").is_some() || ks.get("ry").is_some() || ks.get("or").is_some() {
            self.warn("3D rotations are left out");
        }
        let fr = self.fr;
        let mut ops = Vec::new();
        ops.extend(self.position(ks.get("p"), auto_orient));
        let rotation = ks.get("r").or_else(|| ks.get("rz"));
        let r = scalar(rotation, 0, 1.0, 0.0, fr);
        ops.push(TransformOp::Rotate { degrees: r });
        let sk = scalar(ks.get("sk"), 0, 1.0, 0.0, fr);
        if sk != Animated::Static(0.0) {
            let sa = scalar(ks.get("sa"), 0, 1.0, 0.0, fr);
            let negate = |a: &Animated<f64>| negated(a);
            ops.push(TransformOp::Rotate {
                degrees: negate(&sa),
            });
            ops.push(TransformOp::Skew {
                x: negate(&sk),
                y: Animated::Static(0.0),
            });
            ops.push(TransformOp::Rotate { degrees: sa });
        }
        ops.push(TransformOp::Scale {
            x: scalar(ks.get("s"), 0, 0.01, 1.0, fr),
            y: scalar(ks.get("s"), 1, 0.01, 1.0, fr),
        });
        ops.push(TransformOp::Translate {
            x: negated(&scalar(ks.get("a"), 0, 1.0, 0.0, fr)),
            y: negated(&scalar(ks.get("a"), 1, 1.0, 0.0, fr)),
        });
        let opacity = scalar(ks.get("o"), 0, 0.01, 1.0, fr);
        (simplify(ops), opacity)
    }

    /// The position: split x and y, plain keyframes, or keyframes with
    /// spatial tangents as a motion path (also when the layer turns along
    /// its path).
    fn position(&mut self, p: Option<&'a Value>, auto_orient: bool) -> Vec<TransformOp> {
        let fr = self.fr;
        let Some(p) = p else { return Vec::new() };
        if p.get("s").and_then(Value::as_bool) == Some(true) {
            let x = scalar(p.get("x"), 0, 1.0, 0.0, fr);
            let y = scalar(p.get("y"), 0, 1.0, 0.0, fr);
            if auto_orient {
                self.warn("auto-orient on a split position is left out");
            }
            return vec![TransformOp::Translate { x, y }];
        }
        if let Some(Prop::Keys(keys)) = prop(p) {
            let spatial = keys.iter().any(|k| {
                let nonzero = |v: &Option<Vec<f64>>| {
                    v.as_ref()
                        .is_some_and(|v| v.iter().take(2).any(|c| *c != 0.0))
                };
                nonzero(&k.to) || nonzero(&k.ti)
            });
            if (spatial || auto_orient) && keys.len() > 1 {
                return vec![self.motion(&keys, auto_orient)];
            }
        }
        vec![TransformOp::Translate {
            x: scalar(Some(p), 0, 1.0, 0.0, fr),
            y: scalar(Some(p), 1, 1.0, 0.0, fr),
        }]
    }

    /// Position keyframes with spatial tangents: the path through them, and
    /// how far along it the layer is (each keyframe's share of the length,
    /// eased per segment).
    fn motion(&mut self, keys: &[props::RawKey<'_>], auto_orient: bool) -> TransformOp {
        let point = |v: &Value| -> Point {
            let n = nums(v).unwrap_or_default();
            Point::new(
                n.first().copied().unwrap_or(0.0),
                n.get(1).copied().unwrap_or(0.0),
            )
        };
        let mut path = Path::new();
        let mut lengths = vec![0.0];
        path.move_to(point(keys[0].value));
        for w in keys.windows(2) {
            let (a, b) = (point(w[0].value), point(w[1].value));
            let to = w[0].to.as_ref().map_or(Point::ZERO, |v| {
                Point::new(v[0], v.get(1).copied().unwrap_or(0.0))
            });
            let ti = w[0].ti.as_ref().map_or(Point::ZERO, |v| {
                Point::new(v[0], v.get(1).copied().unwrap_or(0.0))
            });
            let mut seg = Path::new();
            seg.move_to(a);
            if to == Point::ZERO && ti == Point::ZERO {
                seg.line_to(b);
                path.line_to(b);
            } else {
                seg.cubic_to(a + to, b + ti, b);
                path.cubic_to(a + to, b + ti, b);
            }
            let total = lengths.last().copied().unwrap_or(0.0) + path_length(&seg);
            lengths.push(total);
        }
        let total = lengths.last().copied().unwrap_or(0.0);
        let progress = Animated::from_keyframes(
            keys.iter()
                .zip(&lengths)
                .map(|(k, l)| {
                    Keyframe::new(
                        k.time / self.fr,
                        if total > 0.0 { l / total } else { 0.0 },
                        k.easing(0),
                    )
                })
                .collect(),
            0.0,
        );
        TransformOp::Motion(MotionPath {
            path,
            progress,
            rotate: if auto_orient {
                MotionRotate::Auto { offset: 0.0 }
            } else {
                MotionRotate::Fixed { degrees: 0.0 }
            },
        })
    }

    /// Shape items of a group: the nodes they draw (bottom first) and every
    /// outline in them, for the styles of enclosing groups.
    ///
    /// A style draws the outlines listed before it (above it, in After
    /// Effects), nested groups' included; the first style listed is on top.
    /// A trim cuts the outlines listed before it, and the other modifiers
    /// change them: a merge makes them one outline (the styles before it
    /// drawing nothing, as in After Effects); round corners, pucker and
    /// bloat, twist, zig zag and offset path change each one.
    fn shapes(&mut self, items: &'a [Value]) -> Result<(Vec<Node>, Vec<Outline>), Error> {
        if self.depth >= self.limits.max_depth {
            return Err(Error::Limit {
                what: "levels of nesting",
                limit: self.limits.max_depth as f64,
            });
        }
        self.depth += 1;
        let fr = self.fr;
        let mut outlines: Vec<Outline> = Vec::new();
        // Nodes in the order their items come: top first.
        let mut drawn: Vec<Node> = Vec::new();
        for item in items {
            if item.get("hd").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            self.nodes += 1;
            if self.nodes > self.limits.max_nodes {
                return Err(Error::Limit {
                    what: "nodes",
                    limit: self.limits.max_nodes as f64,
                });
            }
            let ty = item.get("ty").and_then(Value::as_str).unwrap_or("");
            match ty {
                "rc" | "el" | "sr" | "sh" => {
                    if let Some(geometry) = self.geometry(ty, item) {
                        outlines.push(Outline {
                            path: ShapePath::new(geometry),
                            trim: None,
                            faded: false,
                        });
                    }
                }
                "fl" | "st" | "gf" | "gs" => {
                    let style = self.style(ty, item);
                    if outlines.iter().any(|o| o.faded) {
                        self.warn("a repeater's fade of outlines styled after it is left out");
                    }
                    // Outlines cut by different trims draw as shapes of their own.
                    let mut runs: Vec<(Option<usize>, Vec<ShapePath>, Option<Trim>)> = Vec::new();
                    for outline in &outlines {
                        let key = outline.trim.as_ref().map(|t| t.0);
                        match runs.iter_mut().find(|r| r.0 == key) {
                            Some(run) => run.1.push(outline.path.clone()),
                            None => runs.push((
                                key,
                                vec![outline.path.clone()],
                                outline.trim.as_ref().map(|t| t.1.clone()),
                            )),
                        }
                    }
                    for (_, paths, trim) in runs {
                        let (fill, stroke) = style.clone();
                        drawn.push(Node::shape(Shape {
                            name: item.get("nm").and_then(Value::as_str).map(str::to_owned),
                            paths,
                            fill,
                            stroke,
                            trim,
                            ..Shape::default()
                        }));
                    }
                }
                "gr" => {
                    let sub = item
                        .get("it")
                        .and_then(Value::as_array)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    let (nodes, inner) = self.shapes(sub)?;
                    let tr = sub
                        .iter()
                        .find(|i| i.get("ty").and_then(Value::as_str) == Some("tr"));
                    let (transform, opacity) = self.transform(tr, false);
                    for outline in inner {
                        let mut t = transform.clone();
                        t.extend(outline.path.transform);
                        outlines.push(Outline {
                            path: ShapePath {
                                geometry: outline.path.geometry,
                                transform: t,
                            },
                            trim: outline.trim,
                            faded: outline.faded,
                        });
                    }
                    if !nodes.is_empty() {
                        drawn.push(Node::Group(Group {
                            name: item.get("nm").and_then(Value::as_str).map(str::to_owned),
                            transform,
                            opacity,
                            children: nodes,
                            ..Group::new()
                        }));
                    }
                }
                "tm" => {
                    let trim = Trim {
                        start: scalar(item.get("s"), 0, 0.01, 0.0, fr),
                        end: scalar(item.get("e"), 0, 0.01, 1.0, fr),
                        offset: scalar(item.get("o"), 0, 1.0 / 360.0, 0.0, fr),
                        mode: if item.get("m").and_then(num) == Some(2.0) {
                            TrimMode::Combined
                        } else {
                            TrimMode::EachPath
                        },
                    };
                    let id = self.nodes;
                    for outline in &mut outlines {
                        if outline.trim.is_some() {
                            self.warnings
                                .insert("trims of trimmed outlines keep the inner trim".into());
                            continue;
                        }
                        outline.trim = Some((id, trim.clone()));
                    }
                }
                "tr" => {}
                "rp" => self.repeat(item, &mut drawn, &mut outlines)?,
                "mm" => {
                    // The styles before a merge draw nothing (skottie and
                    // lottie-android hide them too).
                    drawn.clear();
                    self.merge_paths(item, &mut outlines);
                }
                "rd" | "pb" | "tw" | "zz" | "op" => self.modify(ty, item, &mut outlines),
                _ => {}
            }
        }
        self.depth -= 1;
        drawn.reverse();
        Ok((drawn, outlines))
    }

    /// A repeater (`rp`): what comes before it in its group (`drawn`, top
    /// first, and the `outlines` a style after it paints) in copies, as
    /// lottie-web repeats: copy `k` through the repeater's transform
    /// `k + offset` times (about its anchor), faded from the start opacity
    /// to the end one, later copies above (or below) earlier ones.
    fn repeat(
        &mut self,
        item: &'a Value,
        drawn: &mut Vec<Node>,
        outlines: &mut Vec<Outline>,
    ) -> Result<(), Error> {
        let fr = self.fr;
        let copies_track = scalar(item.get("c"), 0, 1.0, 1.0, fr);
        let copies_times = key_times(item.get("c"), fr);
        let hi = match &copies_track {
            Animated::Static(c) => *c,
            Animated::Track(t) => t
                .keyframes
                .iter()
                .fold(f64::NEG_INFINITY, |a, k| a.max(k.value)),
        };
        let count = hi.ceil().clamp(0.0, MAX_COPIES as f64) as usize;
        if hi > MAX_COPIES as f64 {
            self.warn(format!(
                "repeaters of more than {MAX_COPIES} copies keep {MAX_COPIES}"
            ));
        }
        let offsets = scalar(item.get("o"), 0, 1.0, 0.0, fr);
        let offset_times = key_times(item.get("o"), fr);
        let tr = item.get("tr");
        let get = |key: &str, dim: usize, scale: f64, default: f64| {
            scalar(tr.and_then(|t| t.get(key)), dim, scale, default, fr)
        };
        let (ax, ay) = (get("a", 0, 1.0, 0.0), get("a", 1, 1.0, 0.0));
        let (px, py) = (get("p", 0, 1.0, 0.0), get("p", 1, 1.0, 0.0));
        let (sx, sy) = (get("s", 0, 0.01, 1.0), get("s", 1, 0.01, 1.0));
        let r = get("r", 0, 1.0, 0.0);
        let start = get("so", 0, 0.01, 1.0);
        let end = get("eo", 0, 0.01, 1.0);
        // The repeater's transform `f` times (a share of it, or backwards).
        let step = |f: f64| -> Vec<TransformOp> {
            let times = |a: &Animated<f64>| map(a, |v| v * f);
            let power = |a: &Animated<f64>| map(a, |v| v.abs().max(1e-9).powf(f) * v.signum());
            vec![
                TransformOp::Translate {
                    x: times(&px),
                    y: times(&py),
                },
                TransformOp::Translate {
                    x: ax.clone(),
                    y: ay.clone(),
                },
                TransformOp::Rotate { degrees: times(&r) },
                TransformOp::Scale {
                    x: power(&sx),
                    y: power(&sy),
                },
                TransformOp::Translate {
                    x: negated(&ax),
                    y: negated(&ay),
                },
            ]
        };
        // Turning and scaling about the anchor alone (evenly, or without
        // turning), `k + offset` steps are one turn and one scale: the
        // offset may move.
        let still_position = px == Animated::Static(0.0) && py == Animated::Static(0.0);
        let even = sx == sy || r == Animated::Static(0.0);
        let closed = still_position && even;
        let offset = match &offsets {
            Animated::Static(o) => *o,
            Animated::Track(t) => t.keyframes.first().map_or(0.0, |k| k.value),
        };
        // Otherwise a moving offset changes how many steps a copy takes:
        // each copy's matrix, sampled frame by frame while anything moves.
        let sampled_steps = !closed && matches!(offsets, Animated::Track(_));
        // Copies further than `MAX_STEPS` steps away stop there.
        let capped = std::cell::Cell::new(false);
        let steps = |whole: f64| {
            let n = whole.abs();
            capped.set(capped.get() || n > MAX_STEPS as f64);
            (n as usize).min(MAX_STEPS)
        };
        let step_times = if sampled_steps {
            let mut keys = offset_times.clone();
            for key in ["p", "a", "s", "r"] {
                keys.extend(key_times(tr.and_then(|t| t.get(key)), fr));
            }
            frame_times(&keys, fr, MAX_STEP_SAMPLES / count.max(1))
        } else {
            Vec::new()
        };
        // Each copy's matrix at each of those times: powers of a whole step
        // (forwards or backwards), then the share of one.
        let stepped: Vec<Vec<Matrix>> = step_times
            .iter()
            .map(|&t| {
                let o = offsets.sample(t);
                let units = [transform_at(&step(1.0), t), transform_at(&step(-1.0), t)];
                let mut powers = [vec![Matrix::IDENTITY], vec![Matrix::IDENTITY]];
                (0..count)
                    .map(|k| {
                        let j = k as f64 + o;
                        let whole = j.trunc();
                        let back = usize::from(whole < 0.0);
                        let n = steps(whole);
                        while powers[back].len() <= n {
                            let last = powers[back][powers[back].len() - 1];
                            powers[back].push(last * units[back]);
                        }
                        let mut m = powers[back][n];
                        if j - whole != 0.0 {
                            m = m * transform_at(&step(j - whole), t);
                        }
                        m
                    })
                    .collect()
            })
            .collect();
        let closed_form = |k: usize| -> Vec<TransformOp> {
            // Samples between the offset's keyframes: scales are powers.
            let mut times = Vec::new();
            for w in offset_times.windows(2) {
                times.extend((0..16).map(|i| w[0] + (w[1] - w[0]) * f64::from(i) / 16.0));
            }
            times.extend(offset_times.last());
            let j = |t: f64| k as f64 + offsets.sample(t);
            let along = |a: &Animated<f64>, f: &dyn Fn(f64, f64) -> f64| match (a, &offsets) {
                (Animated::Static(v), Animated::Static(_)) => Animated::Static(f(*v, j(0.0))),
                (Animated::Static(v), Animated::Track(_)) => sampled(&times, |t| f(*v, j(t))),
                (moving, _) => {
                    let mut all = times.clone();
                    all.extend(moving.keyframes().iter().map(|k| k.time));
                    sampled(&all, |t| f(moving.sample(t), j(t)))
                }
            };
            simplify(vec![
                TransformOp::Translate {
                    x: ax.clone(),
                    y: ay.clone(),
                },
                TransformOp::Rotate {
                    degrees: along(&r, &|v, j| v * j),
                },
                TransformOp::Scale {
                    x: along(&sx, &|v, j| v.abs().max(1e-9).powf(j) * v.signum()),
                    y: along(&sy, &|v, j| v.abs().max(1e-9).powf(j) * v.signum()),
                },
                TransformOp::Translate {
                    x: negated(&ax),
                    y: negated(&ay),
                },
            ])
        };
        let transform = |k: usize| -> Vec<TransformOp> {
            if closed {
                return closed_form(k);
            }
            if sampled_steps {
                let keys = step_times
                    .iter()
                    .zip(&stepped)
                    .map(|(&t, all)| Keyframe::new(t, all[k], Easing::Linear))
                    .collect();
                return vec![TransformOp::Matrix {
                    matrix: Animated::Track(Track::new(keys)),
                }];
            }
            let j = k as f64 + offset;
            let whole = j.trunc();
            let mut ops = Vec::new();
            let unit = if whole < 0.0 { -1.0 } else { 1.0 };
            for _ in 0..steps(whole) {
                ops.extend(step(unit));
            }
            if j - whole != 0.0 {
                ops.extend(step(j - whole));
            }
            simplify(ops)
        };
        let copy_opacity = |k: usize| -> Animated<f64> {
            let share = if count > 1 {
                k as f64 / (count - 1) as f64
            } else {
                0.0
            };
            let mut times = key_times(tr.and_then(|t| t.get("so")), fr);
            times.extend(key_times(tr.and_then(|t| t.get("eo")), fr));
            times.extend(copies_times.iter().copied());
            let visible = |t: f64| (copies_track.sample(t) - k as f64).clamp(0.0, 1.0);
            let faded = |t: f64| {
                let (s, e) = (start.sample(t), end.sample(t));
                (s + (e - s) * share).clamp(0.0, 1.0) * visible(t)
            };
            sampled(&times, faded)
        };
        let below = item.get("m").and_then(num) == Some(2.0);
        // Top first: with copies above, the last copy first.
        let order: Vec<usize> = if below {
            (0..count).collect()
        } else {
            (0..count).rev().collect()
        };
        // Every copy counts against the limit.
        let each = drawn.iter().map(count_nodes).sum::<usize>() + outlines.len();
        self.nodes += each * count.saturating_sub(1);
        if self.nodes > self.limits.max_nodes {
            return Err(Error::Limit {
                what: "nodes",
                limit: self.limits.max_nodes as f64,
            });
        }
        if !drawn.is_empty() {
            let content = std::mem::take(drawn);
            let mut copies = Vec::with_capacity(count);
            for &k in &order {
                copies.push(Node::Group(Group {
                    transform: transform(k),
                    opacity: copy_opacity(k),
                    // `drawn` is top first; a group's children bottom first.
                    children: content.iter().rev().cloned().collect(),
                    ..Group::new()
                }));
            }
            // Bottom first inside: the group goes in the top-first list.
            copies.reverse();
            drawn.push(Node::Group(Group {
                children: copies,
                ..Group::new()
            }));
        }
        if !outlines.is_empty() {
            let faded = start != Animated::Static(1.0) || end != Animated::Static(1.0);
            let base = std::mem::take(outlines);
            for &k in order.iter().rev() {
                for outline in &base {
                    let mut t = transform(k);
                    t.extend(outline.path.transform.iter().cloned());
                    outlines.push(Outline {
                        path: ShapePath {
                            geometry: outline.path.geometry.clone(),
                            transform: t,
                        },
                        trim: outline.trim.clone(),
                        faded: outline.faded || faded,
                    });
                }
            }
        }
        if capped.get() {
            self.warn(format!(
                "repeater copies more than {MAX_STEPS} steps of the transform away stop at \
                 {MAX_STEPS}"
            ));
        }
        Ok(())
    }

    /// Merge paths (`mm`): the outlines before it as one, merged in its
    /// group's space (or the space they share, keeping its transform).
    fn merge_paths(&mut self, item: &Value, outlines: &mut Vec<Outline>) {
        let mode = match item.get("mm").and_then(num).map(|m| m.round() as i64) {
            None | Some(1) => MergeMode::Merge,
            Some(2) => MergeMode::Add,
            Some(3) => MergeMode::Subtract,
            Some(4) => MergeMode::Intersect,
            Some(5) => MergeMode::Exclude,
            Some(other) => {
                self.warn(format!(
                    "merge mode {other} is not a Lottie merge mode and merges"
                ));
                MergeMode::Merge
            }
        };
        if outlines.is_empty() {
            return;
        }
        self.untrim(outlines);
        let over = Cell::new(false);
        let faded = outlines.iter().any(|o| o.faded);
        let inputs: Vec<ShapePath> = std::mem::take(outlines)
            .into_iter()
            .map(|o| o.path)
            .collect();
        let shared = inputs.iter().all(|p| p.transform == inputs[0].transform);
        let mut times = Vec::new();
        let mut animated = false;
        for p in &inputs {
            times.extend(geometry_times(&p.geometry));
            animated |= p.geometry.is_animated();
            if !shared {
                times.extend(ops_times(&p.transform));
                animated |= p.transform.iter().any(TransformOp::is_animated);
            }
        }
        let at = |t: f64| -> Path {
            let paths: Vec<Path> = inputs
                .iter()
                .map(|p| {
                    if shared {
                        geometry_path(&p.geometry, t)
                    } else {
                        shape_path_at(p, t)
                    }
                })
                .collect();
            merge(&paths, mode, &over)
        };
        let baked = self.bake(animated, &times, at);
        if over.get() {
            self.warn(
                "merge paths of outlines too detailed to merge within the limits keep them as they are",
            );
        }
        match baked {
            Some(path) => outlines.push(Outline {
                path: ShapePath {
                    geometry: Geometry::Path { path },
                    transform: if shared {
                        inputs[0].transform.clone()
                    } else {
                        Vec::new()
                    },
                },
                trim: None,
                faded,
            }),
            None => {
                self.warn(OVER_LIMIT);
                outlines.extend(inputs.into_iter().map(|path| Outline {
                    path,
                    trim: None,
                    faded,
                }));
            }
        }
    }

    /// A modifier that changes each outline before it on its own, in the
    /// outline's space (as lottie-web modifies them).
    fn modify(&mut self, ty: &str, item: &'a Value, outlines: &mut Vec<Outline>) {
        let modifier = self.modifier(ty, item);
        if outlines.is_empty() || modifier.is_noop() {
            return;
        }
        self.untrim(outlines);
        let capped = Cell::new(false);
        // Outlines alike (a repeater's copies) are modified once.
        let mut done: Vec<(Geometry, Animated<Path>)> = Vec::new();
        for outline in outlines.iter_mut() {
            let geometry = &outline.path.geometry;
            let path = match done.iter().find(|(g, _)| g == geometry) {
                Some((_, path)) => {
                    let path = path.clone();
                    self.charge(&path).then_some(path)
                }
                None => {
                    let mut times = geometry_times(geometry);
                    times.extend(modifier.times());
                    let animated = geometry.is_animated() || modifier.is_animated();
                    let baked = self.bake(animated, &times, |t| {
                        modifier.apply(&geometry_path(geometry, t), t, &capped)
                    });
                    if let Some(path) = &baked {
                        done.push((geometry.clone(), path.clone()));
                    }
                    baked
                }
            };
            match path {
                Some(path) => outline.path.geometry = Geometry::Path { path },
                None => self.warn(OVER_LIMIT),
            }
        }
        if capped.get() {
            self.warn(format!(
                "zig zags of more than {MAX_POINTS} points an outline have fewer ridges"
            ));
        }
    }

    /// A modifier item's parameters.
    fn modifier(&mut self, ty: &str, item: &'a Value) -> Modifier {
        let fr = self.fr;
        // A property, or (as some exporters write it) a plain number.
        let value = |key: &str, default: f64| match item.get(key) {
            Some(Value::Number(n)) => Animated::Static(n.as_f64().unwrap_or(default)),
            p => scalar(p, 0, 1.0, default, fr),
        };
        match ty {
            "rd" => Modifier::Round {
                radius: value("r", 0.0),
            },
            "pb" => Modifier::Pucker {
                amount: value("a", 0.0),
            },
            "tw" => Modifier::Twist {
                angle: value("a", 0.0),
                center: AnimatedPoint {
                    x: scalar(item.get("c"), 0, 1.0, 0.0, fr),
                    y: scalar(item.get("c"), 1, 1.0, 0.0, fr),
                },
            },
            "zz" => Modifier::ZigZag {
                size: value("s", 0.0),
                ridges: value("r", 0.0),
                points: value("pt", 1.0),
            },
            _ => Modifier::Offset {
                amount: value("a", 0.0),
                join: match item.get("lj").and_then(num).map(|j| j as i64) {
                    Some(2) => Join::Round,
                    Some(3) => Join::Bevel,
                    _ => Join::Miter,
                },
                miter_limit: value("ml", 4.0),
            },
        }
    }

    /// Outlines cut by a trim, cut now in their group's space, so that a
    /// modifier after the trim changes what the trim leaves.
    fn untrim(&mut self, outlines: &mut Vec<Outline>) {
        if outlines.iter().all(|o| o.trim.is_none()) {
            return;
        }
        let mut out: Vec<Option<Outline>> = Vec::with_capacity(outlines.len());
        // Outlines trimmed as one line: (trim id, place, outlines, trim).
        let mut combined: Vec<(usize, usize, Vec<Outline>, Trim)> = Vec::new();
        for outline in std::mem::take(outlines) {
            match outline.trim.clone() {
                None => out.push(Some(outline)),
                Some((id, trim)) if trim.mode == TrimMode::Combined => {
                    match combined.iter_mut().find(|c| c.0 == id) {
                        Some(c) => c.2.push(outline),
                        None => {
                            combined.push((id, out.len(), vec![outline], trim));
                            out.push(None);
                        }
                    }
                }
                Some((_, trim)) => {
                    out.extend(self.cut(vec![outline], &trim).into_iter().map(Some));
                }
            }
        }
        for (_, place, list, trim) in combined {
            let mut cut = self.cut(list, &trim).into_iter();
            out[place] = cut.next();
            out.extend(cut.map(Some));
        }
        *outlines = out.into_iter().flatten().collect();
    }

    /// `list` cut by `trim` as one outline; the outlines as they were
    /// when that is over the limits.
    fn cut(&mut self, list: Vec<Outline>, trim: &Trim) -> Vec<Outline> {
        let mut times = trim_times(trim);
        let mut animated = [&trim.start, &trim.end, &trim.offset]
            .iter()
            .any(|v| v.is_animated());
        for o in &list {
            times.extend(geometry_times(&o.path.geometry));
            times.extend(ops_times(&o.path.transform));
            animated |= o.path.geometry.is_animated()
                || o.path.transform.iter().any(TransformOp::is_animated);
        }
        let at = |t: f64| -> Path {
            let paths: Vec<Path> = list.iter().map(|o| shape_path_at(&o.path, t)).collect();
            trim_paths(&paths, trim, t)
        };
        match self.bake(animated, &times, at) {
            Some(path) => vec![Outline {
                path: ShapePath::new(Geometry::Path { path }),
                trim: None,
                faded: list.iter().any(|o| o.faded),
            }],
            None => {
                self.warn(OVER_LIMIT);
                list
            }
        }
    }

    /// The path `at` gives: a constant when nothing it depends on moves;
    /// else keyframes at each frame from the first of the `keys` to the
    /// last (straight between frames of the same commands, held between
    /// others), fewer when they would take more than the limits leave.
    /// `None` when not even one path fits.
    fn bake(
        &mut self,
        animated: bool,
        keys: &[f64],
        at: impl Fn(f64) -> Path,
    ) -> Option<Animated<Path>> {
        let mut keys: Vec<f64> = keys.iter().copied().filter(|t| t.is_finite()).collect();
        keys.sort_by(f64::total_cmp);
        keys.dedup();
        let (first, last) = match (keys.first(), keys.last()) {
            (Some(&first), Some(&last)) if animated && last > first => (first, last),
            _ => {
                let path = Animated::Static(at(keys.first().copied().unwrap_or(0.0)));
                return self.charge(&path).then_some(path);
            }
        };
        let fr = self.fr;
        let frames = ((last - first) * fr).ceil().clamp(1.0, 1e9) as usize;
        // What one moment takes, by a few of them.
        let probe = [first, (first + last) / 2.0, last]
            .iter()
            .map(|&t| at(t).point_count())
            .max()
            .unwrap_or(0)
            .max(1);
        let fit = (self.bake_points / probe)
            .min(self.bake_keys)
            .min(MAX_BAKED_FRAMES);
        if fit < 2 {
            self.warn(FEWER_FRAMES);
            let path = Animated::Static(at(first));
            return self.charge(&path).then_some(path);
        }
        let mut times: Vec<f64> = if frames + keys.len() < fit {
            let mut times: Vec<f64> = (0..=frames)
                .map(|k| (first + k as f64 / fr).min(last))
                .collect();
            times.extend(&keys);
            times
        } else {
            self.warn(FEWER_FRAMES);
            (0..fit)
                .map(|k| first + (last - first) * k as f64 / (fit - 1) as f64)
                .collect()
        };
        times.sort_by(f64::total_cmp);
        times.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        let mut paths: Vec<(f64, Path)> = times.iter().map(|&t| (t, at(t))).collect();
        // Moments that took more than the probe said: every so many.
        let total: usize = paths.iter().map(|(_, p)| p.point_count()).sum();
        if total > self.bake_points {
            self.warn(FEWER_FRAMES);
            let stride = total.div_ceil(self.bake_points.max(1)) + 1;
            let end = paths.pop();
            paths = paths.into_iter().step_by(stride).collect();
            paths.extend(end);
        }
        let holds: Vec<bool> = paths
            .windows(2)
            .map(|w| !w[0].1.same_structure(&w[1].1))
            .collect();
        let frames: Vec<Keyframe<Path>> = paths
            .into_iter()
            .enumerate()
            .map(|(k, (t, path))| {
                let easing = if holds.get(k).copied().unwrap_or(false) {
                    Easing::Hold
                } else {
                    Easing::Linear
                };
                Keyframe::new(t, path, easing)
            })
            .collect();
        let path = Animated::from_keyframes(frames, Path::new());
        if self.charge(&path) {
            return Some(path);
        }
        // Still over: the first moment alone.
        self.warn(FEWER_FRAMES);
        let path = Animated::Static(at(first));
        self.charge(&path).then_some(path)
    }

    /// Takes what a baked path holds from what is left for them; false
    /// when it does not fit.
    fn charge(&mut self, path: &Animated<Path>) -> bool {
        let (points, keys) = match path {
            Animated::Static(p) => (p.point_count(), 0),
            Animated::Track(t) => (
                t.keyframes.iter().map(|k| k.value.point_count()).sum(),
                t.keyframes.len(),
            ),
        };
        if points > self.bake_points || keys > self.bake_keys {
            return false;
        }
        self.bake_points -= points;
        self.bake_keys -= keys;
        true
    }

    /// The outline of a geometry item.
    fn geometry(&mut self, ty: &str, item: &'a Value) -> Option<Geometry> {
        let fr = self.fr;
        let direction = if item.get("d").and_then(num) == Some(3.0) {
            Direction::CounterClockwise
        } else {
            Direction::Clockwise
        };
        let point = |key: &str, scale: f64| AnimatedPoint {
            x: scalar(item.get(key), 0, scale, 0.0, fr),
            y: scalar(item.get(key), 1, scale, 0.0, fr),
        };
        Some(match ty {
            "rc" => {
                let r = scalar(item.get("r"), 0, 1.0, 0.0, fr);
                Geometry::Rect {
                    center: point("p", 1.0),
                    size: point("s", 1.0),
                    radius: AnimatedPoint { x: r.clone(), y: r },
                    direction,
                    start: RectStart::TopRight,
                }
            }
            "el" => Geometry::Ellipse {
                center: point("p", 1.0),
                radius: point("s", 0.5),
                direction,
                start: EllipseStart::Top,
            },
            "sr" => Geometry::Star(Star {
                kind: if item.get("sy").and_then(num) == Some(2.0) {
                    StarKind::Polygon
                } else {
                    StarKind::Star
                },
                center: point("p", 1.0),
                points: scalar(item.get("pt"), 0, 1.0, 5.0, fr),
                outer_radius: scalar(item.get("or"), 0, 1.0, 0.0, fr),
                inner_radius: scalar(item.get("ir"), 0, 1.0, 0.0, fr),
                outer_roundness: scalar(item.get("os"), 0, 0.01, 0.0, fr),
                inner_roundness: scalar(item.get("is"), 0, 0.01, 0.0, fr),
                rotation: scalar(item.get("r"), 0, 1.0, 0.0, fr),
                direction,
            }),
            "sh" => Geometry::Path {
                path: shape(item.get("ks"), fr)?,
            },
            _ => return None,
        })
    }

    /// A style item: its fill or its stroke.
    fn style(&mut self, ty: &str, item: &'a Value) -> (Option<Fill>, Option<Stroke>) {
        let fr = self.fr;
        let opacity = scalar(item.get("o"), 0, 0.01, 1.0, fr);
        let paint = match ty {
            "fl" | "st" => Paint::Solid {
                color: color(item.get("c"), fr),
            },
            _ => self.gradient(item),
        };
        let rule = if item.get("r").and_then(num) == Some(2.0) {
            FillRule::EvenOdd
        } else {
            FillRule::NonZero
        };
        if ty == "fl" || ty == "gf" {
            return (
                Some(Fill {
                    paint,
                    opacity,
                    rule,
                }),
                None,
            );
        }
        let dash = item.get("d").and_then(Value::as_array).map(|list| {
            let mut array = Vec::new();
            let mut offset = Animated::Static(0.0);
            for d in list {
                match d.get("n").and_then(Value::as_str) {
                    Some("o") => offset = scalar(d.get("v"), 0, 1.0, 0.0, fr),
                    _ => array.push(scalar(d.get("v"), 0, 1.0, 0.0, fr)),
                }
            }
            if array.len() % 2 == 1 {
                array.extend(array.clone());
            }
            Dash { array, offset }
        });
        let stroke = Stroke {
            paint,
            opacity,
            width: scalar(item.get("w"), 0, 1.0, 1.0, fr),
            cap: match item.get("lc").and_then(num).unwrap_or(2.0) as i64 {
                1 => LineCap::Butt,
                3 => LineCap::Square,
                _ => LineCap::Round,
            },
            join: match item.get("lj").and_then(num).unwrap_or(2.0) as i64 {
                1 => LineJoin::Miter,
                3 => LineJoin::Bevel,
                _ => LineJoin::Round,
            },
            miter_limit: item.get("ml").and_then(num).unwrap_or(4.0).max(1.0),
            dash: dash.filter(|d| !d.array.is_empty()),
        };
        (None, Some(stroke))
    }

    /// A gradient fill's or stroke's paint.
    fn gradient(&mut self, item: &'a Value) -> Paint {
        let fr = self.fr;
        let stops = gradient_stops(item.get("g"), fr);
        let point = |key: &str| AnimatedPoint {
            x: scalar(item.get(key), 0, 1.0, 0.0, fr),
            y: scalar(item.get(key), 1, 1.0, 0.0, fr),
        };
        let (start, end) = (point("s"), point("e"));
        if item.get("t").and_then(num) == Some(2.0) {
            // Radial: centered at `s`, out to `e`; the highlight moves the
            // focal point towards the edge at an angle.
            let mut times = key_times(item.get("s"), fr);
            times.extend(key_times(item.get("e"), fr));
            times.extend(key_times(item.get("h"), fr));
            times.extend(key_times(item.get("a"), fr));
            let h = scalar(item.get("h"), 0, 0.01, 0.0, fr);
            let a = scalar(item.get("a"), 0, 1.0, 0.0, fr);
            let radius_at = |t: f64| start.sample(t).distance(end.sample(t));
            let focal_at = |t: f64| -> Point {
                let (s, e) = (start.sample(t), end.sample(t));
                let r = s.distance(e);
                let base = (e.y - s.y).atan2(e.x - s.x);
                let angle = base + a.sample(t).to_radians();
                let d = h.sample(t).clamp(-0.99, 0.99) * r;
                Point::new(s.x + d * angle.cos(), s.y + d * angle.sin())
            };
            // With no highlight the focal point is the center, moving as it
            // moves.
            let focal = if h.as_static() == Some(&0.0) {
                start.clone()
            } else {
                AnimatedPoint {
                    x: sampled(&times, |t| focal_at(t).x),
                    y: sampled(&times, |t| focal_at(t).y),
                }
            };
            return Paint::RadialGradient(RadialGradient {
                center: start.clone(),
                radius: sampled(&times, radius_at),
                focal,
                stops,
                units: Default::default(),
                transform: Matrix::IDENTITY,
                spread: Default::default(),
            });
        }
        Paint::LinearGradient(LinearGradient {
            start,
            end,
            stops,
            units: Default::default(),
            transform: Matrix::IDENTITY,
            spread: Default::default(),
        })
    }
}

/// An outline gathered for the styles of a group, with the trim that cuts
/// it (an id telling trims apart, and the trim).
#[derive(Clone)]
struct Outline {
    path: ShapePath,
    trim: Option<(usize, Trim)>,
    /// Repeated by a repeater fading its copies, which a style after it
    /// does not show.
    faded: bool,
}

/// A shape modifier that changes each outline on its own, and its
/// parameters.
enum Modifier {
    /// Round corners (`rd`).
    Round { radius: Animated<f64> },
    /// Pucker and bloat (`pb`), percent.
    Pucker { amount: Animated<f64> },
    /// Twist (`tw`), degrees about a point.
    Twist {
        angle: Animated<f64>,
        center: AnimatedPoint,
    },
    /// Zig zag (`zz`): how far, how many ridges a segment, corner (1) or
    /// smooth (2) points.
    ZigZag {
        size: Animated<f64>,
        ridges: Animated<f64>,
        points: Animated<f64>,
    },
    /// Offset path (`op`).
    Offset {
        amount: Animated<f64>,
        join: Join,
        miter_limit: Animated<f64>,
    },
}

impl Modifier {
    fn values(&self) -> Vec<&Animated<f64>> {
        match self {
            Modifier::Round { radius } => vec![radius],
            Modifier::Pucker { amount } => vec![amount],
            Modifier::Twist { angle, center } => vec![angle, &center.x, &center.y],
            Modifier::ZigZag {
                size,
                ridges,
                points,
            } => vec![size, ridges, points],
            Modifier::Offset {
                amount,
                miter_limit,
                ..
            } => vec![amount, miter_limit],
        }
    }

    fn is_animated(&self) -> bool {
        self.values().iter().any(|v| v.is_animated())
    }

    fn times(&self) -> Vec<f64> {
        self.values()
            .iter()
            .flat_map(|v| v.keyframes().iter().map(|k| k.time))
            .collect()
    }

    /// Whether it changes nothing at any time.
    fn is_noop(&self) -> bool {
        let zero = |v: &Animated<f64>| v.as_static() == Some(&0.0);
        match self {
            Modifier::Round { radius } => radius.as_static().is_some_and(|r| *r <= 0.0),
            Modifier::Pucker { amount } | Modifier::Offset { amount, .. } => zero(amount),
            Modifier::Twist { angle, .. } => zero(angle),
            Modifier::ZigZag { size, .. } => zero(size),
        }
    }

    /// `path` changed at `time`; `capped` set when a zig zag keeps fewer
    /// ridges than it has.
    fn apply(&self, path: &Path, time: f64, capped: &Cell<bool>) -> Path {
        let finite = |v: f64| v.is_finite() && v != 0.0;
        match self {
            Modifier::Round { radius } => {
                let r = radius.sample(time);
                if r.is_nan() || r <= 0.0 {
                    return path.clone();
                }
                let list: Vec<_> = contours(path).iter().map(|c| round_corners(c, r)).collect();
                path_of(&list)
            }
            Modifier::Pucker { amount } => {
                let a = amount.sample(time);
                if !finite(a) {
                    return path.clone();
                }
                let list: Vec<_> = contours(path).iter().map(|c| pucker_bloat(c, a)).collect();
                path_of(&list)
            }
            Modifier::Twist { angle, center } => {
                twist(path, angle.sample(time), center.sample(time))
            }
            Modifier::ZigZag {
                size,
                ridges,
                points,
            } => {
                let s = size.sample(time);
                if !finite(s) {
                    return path.clone();
                }
                let list = contours(path);
                let vertices: usize = list.iter().map(|c| c.vertices.len()).sum();
                let most = (MAX_POINTS / vertices.max(1)).saturating_sub(1);
                let wanted = ridges.sample(time).round().max(0.0);
                let ridges = if wanted > most as f64 {
                    capped.set(true);
                    most
                } else {
                    wanted as usize
                };
                let smooth = points.sample(time).round() == 2.0;
                let list: Vec<_> = list.iter().map(|c| zig_zag(c, s, ridges, smooth)).collect();
                path_of(&list)
            }
            Modifier::Offset {
                amount,
                join,
                miter_limit,
            } => {
                let a = amount.sample(time);
                if !finite(a) {
                    return path.clone();
                }
                let limit = miter_limit.sample(time).max(1.0);
                let list: Vec<_> = contours(path)
                    .iter()
                    .map(|c| offset(c, a, *join, limit))
                    .collect();
                path_of(&list)
            }
        }
    }
}

/// What a modifier says when its outlines would take more than the
/// limits leave.
const OVER_LIMIT: &str = "shape modifiers whose outlines would exceed the limit of path points or keyframes are left out";

/// What a modifier says when it samples fewer frames than the file has.
const FEWER_FRAMES: &str = "animated shape modifiers are sampled at fewer frames than the file's, within the limits of path points and keyframes";

/// The most moments one modified outline is sampled at.
const MAX_BAKED_FRAMES: usize = 20_000;

/// The keyframe times of a geometry's values.
fn geometry_times(g: &Geometry) -> Vec<f64> {
    let mut values: Vec<&Animated<f64>> = Vec::new();
    match g {
        Geometry::Path { path } => return path.keyframes().iter().map(|k| k.time).collect(),
        Geometry::Rect {
            center,
            size,
            radius,
            ..
        } => {
            for p in [center, size, radius] {
                values.extend([&p.x, &p.y]);
            }
        }
        Geometry::Ellipse { center, radius, .. } => {
            for p in [center, radius] {
                values.extend([&p.x, &p.y]);
            }
        }
        Geometry::Star(s) => {
            values.extend([
                &s.center.x,
                &s.center.y,
                &s.points,
                &s.outer_radius,
                &s.inner_radius,
                &s.outer_roundness,
                &s.inner_roundness,
                &s.rotation,
            ]);
        }
    }
    values
        .iter()
        .flat_map(|v| v.keyframes().iter().map(|k| k.time))
        .collect()
}

/// The keyframe times of a transform list's values.
fn ops_times(ops: &[TransformOp]) -> Vec<f64> {
    let mut out = Vec::new();
    for op in ops {
        let times = |v: &Animated<f64>| v.keyframes().iter().map(|k| k.time).collect::<Vec<_>>();
        match op {
            TransformOp::Translate { x, y }
            | TransformOp::Scale { x, y }
            | TransformOp::Skew { x, y } => {
                out.extend(times(x));
                out.extend(times(y));
            }
            TransformOp::Rotate { degrees } => out.extend(times(degrees)),
            TransformOp::Matrix { matrix } => {
                out.extend(matrix.keyframes().iter().map(|k| k.time));
            }
            TransformOp::Motion(motion) => out.extend(times(&motion.progress)),
        }
    }
    out
}

/// The keyframe times of a trim's values.
fn trim_times(trim: &Trim) -> Vec<f64> {
    [&trim.start, &trim.end, &trim.offset]
        .iter()
        .flat_map(|v| v.keyframes().iter().map(|k| k.time))
        .collect()
}

/// An outline at `time`, its transform applied.
fn shape_path_at(p: &ShapePath, time: f64) -> Path {
    let path = geometry_path(&p.geometry, time);
    if p.transform.is_empty() {
        path
    } else {
        path.transformed(transform_at(&p.transform, time))
    }
}

/// A layer's own time: `(t − st) / sr`.
fn layer_timing(layer: &Value, fr: f64) -> TimeMap {
    let st = layer.get("st").and_then(num).unwrap_or(0.0);
    let sr = layer
        .get("sr")
        .and_then(num)
        .filter(|s| *s != 0.0)
        .unwrap_or(1.0);
    TimeMap {
        offset: st / fr,
        scale: sr,
        remap: None,
    }
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Geometry {
    Geometry::Rect {
        center: AnimatedPoint::fixed(x + w / 2.0, y + h / 2.0),
        size: AnimatedPoint::fixed(w, h),
        radius: AnimatedPoint::fixed(0.0, 0.0),
        direction: Direction::Clockwise,
        start: RectStart::TopLeft,
    }
}

/// A white shape (a mask's content).
fn white(paths: Vec<ShapePath>, opacity: Animated<f64>, rule: FillRule) -> Shape {
    Shape {
        paths,
        fill: Some(Fill {
            paint: Paint::solid(Color::WHITE),
            opacity,
            rule,
        }),
        ..Shape::default()
    }
}

/// `-a`, keyframe by keyframe.
fn negated(a: &Animated<f64>) -> Animated<f64> {
    match a {
        Animated::Static(v) => Animated::Static(-v),
        Animated::Track(t) => Animated::Track(Track::new(
            t.keyframes
                .iter()
                .map(|k| Keyframe::new(k.time, -k.value, k.easing.clone()))
                .collect(),
        )),
    }
}

/// The nodes `node` holds, itself included.
fn count_nodes(node: &Node) -> usize {
    match node {
        Node::Group(g) => 1 + g.children.iter().map(count_nodes).sum::<usize>(),
        _ => 1,
    }
}

/// The most copies a repeater makes.
const MAX_COPIES: usize = 1000;

/// The most steps of its transform a repeater's copy takes (an offset
/// past them is as far as they go).
const MAX_STEPS: usize = 10_000;

/// The most matrices a repeater whose offset moves samples, over all its
/// copies.
const MAX_STEP_SAMPLES: usize = 200_000;

/// `keys` and the frames (`fr` a second) between the first and the last,
/// at most `most` times in all (evenly apart beyond that).
fn frame_times(keys: &[f64], fr: f64, most: usize) -> Vec<f64> {
    let mut keys: Vec<f64> = keys.iter().copied().filter(|t| t.is_finite()).collect();
    keys.sort_by(f64::total_cmp);
    keys.dedup();
    let (Some(&first), Some(&last)) = (keys.first(), keys.last()) else {
        return vec![0.0];
    };
    let most = most.max(2);
    let step = if fr > 0.0 { 1.0 / fr } else { 1.0 / 30.0 };
    let frames = ((last - first) / step).ceil().max(1.0);
    let n = (frames as usize).min(most.saturating_sub(keys.len()).max(1));
    let mut times = keys;
    times.extend((0..=n).map(|i| first + (last - first) * i as f64 / n as f64));
    times.sort_by(f64::total_cmp);
    times.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    times
}

/// `a` with each value through `f`.
fn map(a: &Animated<f64>, f: impl Fn(f64) -> f64) -> Animated<f64> {
    match a {
        Animated::Static(v) => Animated::Static(f(*v)),
        Animated::Track(t) => Animated::Track(Track::new(
            t.keyframes
                .iter()
                .map(|k| Keyframe::new(k.time, f(k.value), k.easing.clone()))
                .collect(),
        )),
    }
}

/// `ops` without operations that change nothing.
fn simplify(ops: Vec<TransformOp>) -> Vec<TransformOp> {
    ops.into_iter()
        .filter(|op| match op {
            TransformOp::Translate { x, y } => {
                !(x.as_static() == Some(&0.0) && y.as_static() == Some(&0.0))
            }
            TransformOp::Scale { x, y } => {
                !(x.as_static() == Some(&1.0) && y.as_static() == Some(&1.0))
            }
            TransformOp::Rotate { degrees } => degrees.as_static() != Some(&0.0),
            TransformOp::Skew { x, y } => {
                !(x.as_static() == Some(&0.0) && y.as_static() == Some(&0.0))
            }
            _ => true,
        })
        .collect()
}

/// Appends a color `op` (in sRGB) to `effects`, to the color effect they
/// end with if any.
fn push_color(effects: &mut Vec<Effect>, op: ColorOp) {
    match effects.last_mut() {
        Some(Effect::Color(last)) if !last.linear => last.ops.push(op),
        _ => effects.push(Effect::Color(ColorEffect {
            ops: vec![op],
            linear: false,
        })),
    }
}

fn blend_mode(bm: i64, converter: &mut Converter<'_>) -> BlendMode {
    match bm {
        0 => BlendMode::Normal,
        1 => BlendMode::Multiply,
        2 => BlendMode::Screen,
        3 => BlendMode::Overlay,
        4 => BlendMode::Darken,
        5 => BlendMode::Lighten,
        6 => BlendMode::ColorDodge,
        7 => BlendMode::ColorBurn,
        8 => BlendMode::HardLight,
        9 => BlendMode::SoftLight,
        10 => BlendMode::Difference,
        11 => BlendMode::Exclusion,
        12 => BlendMode::Hue,
        13 => BlendMode::Saturation,
        14 => BlendMode::Color,
        15 => BlendMode::Luminosity,
        16 => BlendMode::Add,
        17 => {
            converter.warn("the hard mix blend mode is drawn as normal");
            BlendMode::Normal
        }
        other => {
            converter.warn(format!(
                "blend mode {other} is not a Lottie blend mode and is drawn as normal"
            ));
            BlendMode::Normal
        }
    }
}

fn hex_color(s: &str) -> Option<Color> {
    let h = s.trim().strip_prefix('#')?;
    let v = u32::from_str_radix(h, 16).ok()?;
    match h.len() {
        6 => Some(Color::rgba8((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)),
        8 => Some(Color::rgba8(
            (v >> 24) as u8,
            (v >> 16) as u8,
            (v >> 8) as u8,
            v as u8,
        )),
        _ => None,
    }
}

fn data_uri(uri: &str) -> Option<ImageData> {
    use base64::Engine;
    let rest = uri.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if !meta.contains(";base64") {
        return None;
    }
    let mime = meta.split(';').next()?.trim().to_ascii_lowercase();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .ok()?;
    Some(ImageData {
        mime: if mime == "image/jpg" {
            "image/jpeg".into()
        } else {
            mime
        },
        bytes,
    })
}
