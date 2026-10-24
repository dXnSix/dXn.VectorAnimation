//! A Lottie composition turned into a scene: layers (shape, null, nested
//! composition, solid, image), parenting, time offsets, stretches and
//! remapping, masks and track mattes, and shape groups.

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use dxn_vector_animation::{
    Animated, AnimatedPoint, BlendMode, Clip, ClipShape, Color, Dash, Direction, EllipseStart,
    Error, Fill, FillRule, Geometry, Group, Image, ImageData, Keyframe, Limits, LineCap, LineJoin,
    LinearGradient, LoopMode, Marker, Mask, MaskKind, Matrix, MotionPath, MotionRotate, Node,
    Paint, Path, Point, RadialGradient, Rect, RectStart, Scene, Shape, ShapePath, Star, StarKind,
    Stroke, TimeMap, TimeRange, Timeline, Track, TransformOp, Trim, TrimMode, outline::path_length,
};
use serde_json::Value;

use crate::props::{
    self, Prop, color, gradient_stops, has_expression, key_times, num, nums, prop, sampled, scalar,
    shape,
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
        };
        if root.get("fonts").is_some() || root.get("chars").is_some() {
            converter.warn("fonts and glyphs are left out");
        }
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
            5 => {
                self.warn("text layers are left out: convert text to shapes before exporting");
                Vec::new()
            }
            _ => Vec::new(),
        };
        if content.is_empty() {
            return Ok(None);
        }
        if layer
            .get("ef")
            .and_then(Value::as_array)
            .is_some_and(|e| !e.is_empty())
        {
            self.warn("layer effects (shadows, blurs) are left out");
        }
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
                        });
                    }
                }
                "fl" | "st" | "gf" | "gs" => {
                    let style = self.style(ty, item);
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
                "rp" => self.warn("repeaters are left out"),
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
            return Paint::RadialGradient(RadialGradient {
                center: start.clone(),
                radius: sampled(&times, radius_at),
                focal: AnimatedPoint {
                    x: sampled(&times, |t| focal_at(t).x),
                    y: sampled(&times, |t| focal_at(t).y),
                },
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
