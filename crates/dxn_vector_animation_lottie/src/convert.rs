//! A Lottie composition turned into a scene: layers (shape, null, nested
//! composition, solid, image), parenting, time offsets, stretches and
//! remapping, masks and track mattes, and shape groups.

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use dxn_vector_animation::{
    Animated, AnimatedPoint, BlendMode, Blur, Clip, ClipShape, Color, Dash, Direction, DropShadow,
    Easing, Effect, EllipseStart, Error, Fill, FillRule, Geometry, Group, Image, ImageData,
    Keyframe, Limits, LineCap, LineJoin, LinearGradient, LoopMode, Marker, Mask, MaskKind, Matrix,
    MotionPath, MotionRotate, Node, Paint, PaintOrder, Path, Point, RadialGradient, Rect,
    RectStart, Scene, Shape, ShapePath, Star, StarKind, Stroke, TimeMap, TimeRange, Timeline,
    Track, TransformOp, Trim, TrimMode, outline::path_length, transform::transform_at,
};
use serde_json::Value;

use crate::props::{
    self, Prop, color, gradient_stops, has_expression, key_times, num, nums, prop, sampled,
    sampled_color, scalar, shape,
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
        };
        let children = converter.composition(layers)?;
        let markers = root
            .get("markers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| {
                Some(Marker {
                    name: m.get("cm").and_then(Value::as_str).unwrap_or("").to_owned(),
                    start: (m.get("tm").and_then(num)? - ip) / fr,
                    duration: m.get("dr").and_then(num).unwrap_or(0.0) / fr,
                })
            })
            .collect();
        let scene = Scene {
            width,
            height,
            timeline: Timeline {
                duration,
                frame_rate: Some(fr),
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
        let effects = self.effects(layer);
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
                _ => self.warn(format!("the layer effect `{name}` is left out")),
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
        for key in ["a", "p", "s", "r", "o", "sk", "sa", "rz"] {
            if ks.get(key).is_some_and(has_expression) {
                self.warn("expressions are left out: their properties keep their keyframes");
            }
        }
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
    /// A trim cuts the outlines listed before it.
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
                "mm" => self.warn("merge paths are left out"),
                "rd" => self.warn("rounded corners are left out"),
                "pb" | "tw" | "zz" | "op" => {
                    self.warn("shape modifiers (pucker, twist, zig-zag, offset) are left out")
                }
                _ => {}
            }
        }
        self.depth -= 1;
        drawn.reverse();
        Ok((drawn, outlines))
    }

    /// A text layer drawn with the file's glyphs (`chars`), as lottie-web
    /// lays text out: each glyph at a hundredth of the size, advanced by
    /// its width and the tracking, lines `lh` apart and justified, box
    /// text wrapped at spaces; each keyframed document shown until the
    /// next. Text animators and text on a path are left out.
    fn text(&mut self, layer: &'a Value) -> Result<Vec<Node>, Error> {
        let fr = self.fr;
        let t = layer.get("t");
        if t.and_then(|t| t.get("a"))
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
        {
            self.warn("text animators are left out");
        }
        if t.and_then(|t| t.get("p"))
            .and_then(|p| p.get("m"))
            .is_some()
        {
            self.warn("text on a path is laid out on a line");
        }
        if self.chars.is_empty() {
            self.warn("text layers without glyphs are left out: export them with glyphs (chars)");
            return Ok(Vec::new());
        }
        let docs: Vec<(f64, &'a Value)> = t
            .and_then(|t| t.get("d"))
            .and_then(|d| d.get("k"))
            .and_then(Value::as_array)
            .map(|k| {
                k.iter()
                    .filter_map(|k| {
                        Some((k.get("t").and_then(num).unwrap_or(0.0) / fr, k.get("s")?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut out = Vec::new();
        for (i, (start, doc)) in docs.iter().enumerate() {
            let Some(shapes) = self.document(doc)? else {
                continue;
            };
            let active = (docs.len() > 1).then(|| TimeRange {
                start: if i == 0 { f64::NEG_INFINITY } else { *start },
                end: docs.get(i + 1).map_or(f64::INFINITY, |d| d.0),
            });
            out.push(Node::Group(Group {
                active,
                children: shapes,
                ..Group::new()
            }));
        }
        Ok(out)
    }

    /// One text document (`s`) as shapes: its glyphs filled and stroked.
    fn document(&mut self, doc: &'a Value) -> Result<Option<Vec<Node>>, Error> {
        let text = doc.get("t").and_then(Value::as_str).unwrap_or("");
        let size = doc.get("s").and_then(num).unwrap_or(10.0);
        let font = doc.get("f").and_then(Value::as_str).unwrap_or("");
        let justify = doc.get("j").and_then(num).unwrap_or(0.0) as i64;
        let tracking = doc.get("tr").and_then(num).unwrap_or(0.0) / 1000.0 * size;
        let line_height = doc.get("lh").and_then(num).unwrap_or(size * 1.2);
        let k = size / 100.0;
        let entry = self
            .fonts
            .iter()
            .find(|f| f.get("fName").and_then(Value::as_str) == Some(font))
            .copied();
        let family = entry
            .and_then(|f| f.get("fFamily").and_then(Value::as_str))
            .unwrap_or(font);
        let style = entry
            .and_then(|f| f.get("fStyle").and_then(Value::as_str))
            .unwrap_or("");
        let ascent = entry
            .and_then(|f| f.get("ascent").and_then(num))
            .unwrap_or(75.0);
        let chars = self.chars.clone();
        // The glyph of `ch`: its index among the chars and its advance.
        let find = |ch: char| -> Option<(usize, f64)> {
            let ch = ch.to_string();
            let fits = |c: &Value, strict: bool| {
                c.get("ch").and_then(Value::as_str) == Some(ch.as_str())
                    && (!strict
                        || (c.get("fFamily").and_then(Value::as_str) == Some(family)
                            && c.get("style").and_then(Value::as_str) == Some(style)))
            };
            let i = chars
                .iter()
                .position(|c| fits(c, true))
                .or_else(|| chars.iter().position(|c| fits(c, false)))?;
            Some((i, chars[i].get("w").and_then(num).unwrap_or(0.0)))
        };
        let advance = |c: char| find(c).map_or(0.0, |(_, w)| w * k) + tracking;
        let measure = |s: &str| s.chars().map(advance).sum::<f64>();
        // Lines: breaks, and in a box, wraps at spaces.
        let normalized = text.replace("\r\n", "\r").replace(['\n', '\u{3}'], "\r");
        let box_size = doc
            .get("sz")
            .and_then(nums)
            .filter(|s| s.len() >= 2 && s[0] > 0.0);
        let box_at = doc.get("ps").and_then(nums).filter(|p| p.len() >= 2);
        let mut lines: Vec<String> = Vec::new();
        for paragraph in normalized.split('\r') {
            let Some(sz) = &box_size else {
                lines.push(paragraph.to_owned());
                continue;
            };
            let mut line = String::new();
            for word in paragraph.split_inclusive(' ') {
                if !line.is_empty() && measure(&line) + measure(word.trim_end()) > sz[0] {
                    lines.push(std::mem::take(&mut line).trim_end().to_owned());
                }
                line.push_str(word);
            }
            lines.push(line.trim_end().to_owned());
        }
        let (x0, y0, box_width) = match (&box_at, &box_size) {
            (Some(ps), Some(sz)) => (ps[0], ps[1] + ascent * k, sz[0]),
            _ => (0.0, 0.0, 0.0),
        };
        let mut paths = Vec::new();
        for (row, line) in lines.iter().enumerate() {
            let width = measure(line) - if line.is_empty() { 0.0 } else { tracking };
            // Left, right or centered: on the anchor, or within the box.
            let mut x = if box_width > 0.0 {
                x0 + match justify {
                    1 => box_width - width,
                    2 => (box_width - width) / 2.0,
                    _ => 0.0,
                }
            } else {
                match justify {
                    1 => -width,
                    2 => -width / 2.0,
                    _ => 0.0,
                }
            };
            let y = y0 + row as f64 * line_height;
            for c in line.chars() {
                let Some((index, w)) = find(c) else {
                    if !c.is_whitespace() {
                        self.warn(format!("text has characters with no glyph (`{c}`)"));
                    }
                    x += tracking;
                    continue;
                };
                for glyph in self.glyph(index)? {
                    let mut transform = vec![
                        TransformOp::Translate {
                            x: Animated::Static(x),
                            y: Animated::Static(y),
                        },
                        TransformOp::Scale {
                            x: Animated::Static(k),
                            y: Animated::Static(k),
                        },
                    ];
                    transform.extend(glyph.transform);
                    paths.push(ShapePath {
                        geometry: glyph.geometry,
                        transform,
                    });
                }
                x += w * k + tracking;
            }
        }
        if paths.is_empty() {
            return Ok(None);
        }
        self.nodes += paths.len();
        if self.nodes > self.limits.max_nodes {
            return Err(Error::Limit {
                what: "nodes",
                limit: self.limits.max_nodes as f64,
            });
        }
        let color = |key: &str| -> Option<Color> {
            let c = doc.get(key).and_then(nums)?;
            let s = if c.iter().take(3).any(|v| *v > 1.0) {
                255.0
            } else {
                1.0
            };
            Some(Color::rgba(
                *c.first()? / s,
                *c.get(1)? / s,
                *c.get(2)? / s,
                1.0,
            ))
        };
        let fill = color("fc").map(|c| Fill {
            paint: Paint::solid(c),
            opacity: Animated::Static(1.0),
            rule: FillRule::NonZero,
        });
        let width = doc.get("sw").and_then(num).unwrap_or(0.0);
        let stroke = color("sc").filter(|_| width > 0.0).map(|c| Stroke {
            paint: Paint::solid(c),
            opacity: Animated::Static(1.0),
            width: Animated::Static(width),
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
            dash: None,
        });
        // `of`: the stroke over the fill.
        let over = doc.get("of").and_then(Value::as_bool).unwrap_or(true);
        Ok(Some(vec![Node::shape(Shape {
            paths,
            fill,
            stroke,
            paint_order: if over {
                PaintOrder::FillFirst
            } else {
                PaintOrder::StrokeFirst
            },
            ..Shape::default()
        })]))
    }

    /// The outlines of glyph `index` of `chars`, at size 100.
    fn glyph(&mut self, index: usize) -> Result<Vec<ShapePath>, Error> {
        if let Some(paths) = self.glyphs.get(&index) {
            return Ok(paths.clone());
        }
        let Some(char) = self.chars.get(index).copied() else {
            return Ok(Vec::new());
        };
        let items = char
            .get("data")
            .and_then(|d| d.get("shapes"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let (_, outlines) = self.shapes(items)?;
        let paths: Vec<ShapePath> = outlines.into_iter().map(|o| o.path).collect();
        self.glyphs.insert(index, paths.clone());
        Ok(paths)
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
                        let n = (whole.abs() as usize).min(MAX_STEPS);
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
            for _ in 0..(whole.abs() as usize).min(MAX_STEPS) {
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
        Ok(())
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
            "sh" => {
                if item.get("ks").is_some_and(has_expression) {
                    self.warn("expressions are left out: their properties keep their keyframes");
                }
                Geometry::Path {
                    path: shape(item.get("ks"), fr)?,
                }
            }
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
struct Outline {
    path: ShapePath,
    trim: Option<(usize, Trim)>,
    /// Repeated by a repeater fading its copies, which a style after it
    /// does not show.
    faded: bool,
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
