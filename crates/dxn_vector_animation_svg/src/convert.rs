//! The SVG document turned into a scene: the cascade, shapes, groups,
//! paints, clips, masks, `use`, images, and their CSS animations.

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use dxn_vector_animation::{
    Animated, AnimatedPoint, BlendMode, Clip, ClipShape, Color, Dash, Direction, EllipseStart,
    Error, Fill, FillRule, Geometry, GradientStop, GradientUnits, Group, Image, ImageData,
    Interpolate, Item, Keyframe, Limits, LineCap, LineJoin, LinearGradient, Mask, MaskKind, Matrix,
    MotionPath, MotionRotate, Node, Paint, PaintOrder, Path, Point, RadialGradient, Rect,
    RectStart, Scene, Shape, ShapePath, Spread, Stroke, Timeline, Track, TransformOp,
    outline::path_length,
};
use roxmltree::Node as XmlNode;

use crate::{
    animate::{TimelineSpan, property_keyframes},
    css::{Stylesheet, parse_declarations},
    values::{
        self, AnimationSpec, TransformFn, angle, color, fraction, length, list_matrix, number,
        transform_list,
    },
};

const XLINK: &str = "http://www.w3.org/1999/xlink";

/// Properties SVG elements may set as attributes.
const PRESENTATION: &[&str] = &[
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-width",
    "stroke-opacity",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "opacity",
    "transform",
    "transform-origin",
    "display",
    "visibility",
    "clip-path",
    "clip-rule",
    "mask",
    "filter",
    "color",
    "stop-color",
    "stop-opacity",
    "paint-order",
    "mask-type",
    "mix-blend-mode",
];

/// Properties whose animation the scene plays.
const ANIMATABLE: &[&str] = &[
    "opacity",
    "transform",
    "translate",
    "rotate",
    "scale",
    "offset-distance",
    "fill",
    "stroke",
    "fill-opacity",
    "stroke-opacity",
    "stroke-width",
    "stroke-dashoffset",
    "stroke-dasharray",
    "d",
];

/// The winning declarations of one element: property → value.
type Declared = HashMap<String, String>;

/// A paint as the `fill` and `stroke` properties give it.
#[derive(Debug, Clone, PartialEq)]
enum PaintValue {
    None,
    Color(Color),
    CurrentColor,
    Url(String, Option<Box<PaintValue>>),
}

/// The inherited properties, computed.
#[derive(Debug, Clone)]
struct Style {
    fill: PaintValue,
    fill_opacity: f64,
    fill_rule: FillRule,
    stroke: PaintValue,
    stroke_opacity: f64,
    stroke_width: f64,
    cap: LineCap,
    join: LineJoin,
    miter_limit: f64,
    dasharray: Option<Vec<f64>>,
    dashoffset: f64,
    clip_rule: FillRule,
    visible: bool,
    color: Color,
    paint_order: PaintOrder,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            fill: PaintValue::Color(Color::BLACK),
            fill_opacity: 1.0,
            fill_rule: FillRule::NonZero,
            stroke: PaintValue::None,
            stroke_opacity: 1.0,
            stroke_width: 1.0,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
            dasharray: None,
            dashoffset: 0.0,
            clip_rule: FillRule::NonZero,
            visible: true,
            color: Color::BLACK,
            paint_order: PaintOrder::FillFirst,
        }
    }
}

/// The conversion of one document.
pub(crate) struct Converter<'a, 'input> {
    ids: HashMap<&'a str, XmlNode<'a, 'input>>,
    sheet: Stylesheet,
    /// The root's view box: what percentages and `view-box` refer to.
    view: Rect,
    pub(crate) warnings: BTreeSet<String>,
    span: TimelineSpan,
    limits: Limits,
    /// Elements being converted through `use`, against cycles.
    use_stack: Vec<roxmltree::NodeId>,
    depth: usize,
    nodes: usize,
    declared: HashMap<roxmltree::NodeId, Declared>,
}

impl<'a, 'input> Converter<'a, 'input> {
    fn warn(&mut self, message: impl Into<String>) {
        self.warnings.insert(message.into());
    }

    /// Reads `doc` into a scene.
    pub(crate) fn run(
        doc: &'a roxmltree::Document<'input>,
        limits: Limits,
    ) -> Result<(Scene, BTreeSet<String>), Error> {
        let root = doc.root_element();
        if root.tag_name().name() != "svg" {
            return Err(Error::Invalid("the root element is not <svg>".into()));
        }
        let mut ids = HashMap::new();
        let mut sheet = Stylesheet::default();
        let mut warnings = BTreeSet::new();
        for node in doc.descendants().filter(|n| n.is_element()) {
            if let Some(id) = node.attribute("id") {
                ids.entry(id).or_insert(node);
            }
            if node.tag_name().name() == "style" {
                let text: String = node.children().filter_map(|c| c.text()).collect();
                sheet.add(&text, &mut |w| {
                    warnings.insert(w);
                });
            }
        }
        let view_box: Option<svgtypes::ViewBox> =
            root.attribute("viewBox").and_then(|v| v.parse().ok());
        let (vb_w, vb_h) = view_box.map_or((300.0, 150.0), |v| (v.w, v.h));
        let width = root
            .attribute("width")
            .and_then(|w| length(w, vb_w))
            .filter(|w| *w > 0.0)
            .unwrap_or(vb_w);
        let height = root
            .attribute("height")
            .and_then(|h| length(h, vb_h))
            .filter(|h| *h > 0.0)
            .unwrap_or(vb_h);
        let view = view_box.map_or(Rect::new(0.0, 0.0, width, height), |v| {
            Rect::new(v.x, v.y, v.w, v.h)
        });
        let mut converter = Converter {
            ids,
            sheet,
            view,
            warnings,
            span: TimelineSpan::default(),
            limits,
            use_stack: Vec::new(),
            depth: 0,
            nodes: 0,
            declared: HashMap::new(),
        };
        // The timeline every animation fits in.
        for node in doc.descendants().filter(|n| n.is_element()) {
            for spec in converter.animation_specs(node) {
                converter.span.add(&spec);
            }
        }
        if converter.span.duration > limits.max_duration {
            return Err(Error::Limit {
                what: "seconds of timeline",
                limit: limits.max_duration,
            });
        }
        let aspect: svgtypes::AspectRatio = root
            .attribute("preserveAspectRatio")
            .and_then(|a| a.parse().ok())
            .unwrap_or_default();
        let fit = match view_box {
            Some(vb) => view_box_matrix(Rect::new(vb.x, vb.y, vb.w, vb.h), aspect, width, height),
            None => Matrix::IDENTITY,
        };
        let style = Style::default();
        let mut node = converter
            .element(root, &style)?
            .unwrap_or_else(|| Node::Group(Group::new()));
        let mut root_group = match node {
            Node::Group(ref mut g) => std::mem::take(g),
            other => Group {
                children: vec![other],
                ..Group::new()
            },
        };
        if fit != Matrix::IDENTITY {
            root_group.transform.insert(0, TransformOp::matrix(fit));
        }
        let scene = Scene {
            width,
            height,
            timeline: Timeline {
                duration: converter.span.duration,
                frame_rate: None,
                loop_mode: if converter.span.endless {
                    dxn_vector_animation::LoopMode::Loop
                } else {
                    dxn_vector_animation::LoopMode::Once
                },
                markers: Vec::new(),
            },
            root: root_group,
        };
        Ok((scene, converter.warnings))
    }

    /// The winning declarations of `node`: presentation attributes, then
    /// the sheets' rules by specificity and order, then its `style`.
    fn cascade(&mut self, node: XmlNode<'a, 'input>) -> Declared {
        if let Some(d) = self.declared.get(&node.id()) {
            return d.clone();
        }
        type Priority = (u8, (u32, u32, u32), usize);
        let mut best: HashMap<String, (Priority, String)> = HashMap::new();
        let mut put = |name: &str, value: &str, priority: Priority| {
            let entry = best.entry(name.to_owned());
            match entry {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    if priority >= e.get().0 {
                        e.insert((priority, value.to_owned()));
                    }
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert((priority, value.to_owned()));
                }
            }
        };
        for attr in node.attributes() {
            if attr.namespace().is_none() && PRESENTATION.contains(&attr.name()) {
                put(attr.name(), attr.value(), (0, (0, 0, 0), 0));
            }
        }
        // `d` is a property in SVG 2; the attribute is its value.
        if let Some(d) = node.attribute("d") {
            put("d", d, (0, (0, 0, 0), 0));
        }
        for rule in &self.sheet.rules {
            for sel in rule.selectors.iter().filter(|s| s.matches(node)) {
                for decl in &rule.declarations {
                    let level = if decl.important { 3 } else { 1 };
                    put(
                        &decl.name,
                        &decl.value,
                        (level, sel.specificity(), rule.order),
                    );
                }
            }
        }
        if let Some(style) = node.attribute("style") {
            for decl in parse_declarations(style) {
                let level = if decl.important { 4 } else { 2 };
                put(&decl.name, &decl.value, (level, (0, 0, 0), 0));
            }
        }
        let declared: Declared = best.into_iter().map(|(k, (_, v))| (k, v)).collect();
        self.declared.insert(node.id(), declared.clone());
        declared
    }

    /// `node`'s CSS animations whose `@keyframes` exist.
    fn animation_specs(&mut self, node: XmlNode<'a, 'input>) -> Vec<AnimationSpec> {
        let declared = self.cascade(node);
        let mut specs = declared
            .get("animation")
            .map(|v| values::animation_shorthand(v))
            .unwrap_or_default();
        for name in [
            "animation-name",
            "animation-duration",
            "animation-timing-function",
            "animation-delay",
            "animation-iteration-count",
            "animation-direction",
            "animation-fill-mode",
        ] {
            if let Some(v) = declared.get(name) {
                values::animation_longhand(&mut specs, name, v);
            }
        }
        let missing: Vec<String> = specs
            .iter()
            .filter(|s| !self.sheet.keyframes.contains_key(&s.name))
            .map(|s| s.name.clone())
            .collect();
        for name in missing {
            self.warn(format!(
                "animation `{name}` has no @keyframes and is left out"
            ));
        }
        specs.retain(|s| self.sheet.keyframes.contains_key(&s.name));
        specs
    }

    /// `parent` with `declared`'s inherited properties applied.
    fn inherit(&mut self, parent: &Style, declared: &Declared) -> Style {
        let mut s = parent.clone();
        let get = |name: &str| {
            declared
                .get(name)
                .map(|v| v.trim())
                .filter(|v| *v != "inherit")
        };
        let diagonal = (self.view.width.powi(2) + self.view.height.powi(2)).sqrt() / 2f64.sqrt();
        if let Some(c) = get("color").and_then(color) {
            s.color = c;
        }
        if let Some(v) = get("fill").and_then(paint_value) {
            s.fill = v;
        }
        if let Some(v) = get("stroke").and_then(paint_value) {
            s.stroke = v;
        }
        if let Some(v) = get("fill-opacity").and_then(fraction) {
            s.fill_opacity = v.clamp(0.0, 1.0);
        }
        if let Some(v) = get("stroke-opacity").and_then(fraction) {
            s.stroke_opacity = v.clamp(0.0, 1.0);
        }
        if let Some(v) = get("stroke-width").and_then(|v| length(v, diagonal)) {
            s.stroke_width = v.max(0.0);
        }
        if let Some(v) = get("fill-rule") {
            s.fill_rule = rule(v);
        }
        if let Some(v) = get("clip-rule") {
            s.clip_rule = rule(v);
        }
        if let Some(v) = get("stroke-linecap") {
            s.cap = match v {
                "round" => LineCap::Round,
                "square" => LineCap::Square,
                _ => LineCap::Butt,
            };
        }
        if let Some(v) = get("stroke-linejoin") {
            s.join = match v {
                "round" => LineJoin::Round,
                "bevel" => LineJoin::Bevel,
                _ => LineJoin::Miter,
            };
        }
        if let Some(v) = get("stroke-miterlimit").and_then(number) {
            s.miter_limit = v.max(1.0);
        }
        if let Some(v) = get("stroke-dasharray") {
            s.dasharray = dash_array(v, diagonal);
        }
        if let Some(v) = get("stroke-dashoffset").and_then(|v| length(v, diagonal)) {
            s.dashoffset = v;
        }
        if let Some(v) = get("visibility") {
            s.visible = v == "visible";
        }
        if let Some(v) = get("paint-order") {
            s.paint_order = match v.split_whitespace().next() {
                Some("stroke") => PaintOrder::StrokeFirst,
                Some("markers") if v.contains("stroke") && !v.contains("fill") => {
                    PaintOrder::StrokeFirst
                }
                _ => PaintOrder::FillFirst,
            };
            if v.find("stroke")
                .zip(v.find("fill"))
                .is_some_and(|(s, f)| s < f)
            {
                s.paint_order = PaintOrder::StrokeFirst;
            }
        }
        s
    }

    /// The scene node of element `node`, or `None` when it draws nothing.
    fn element(
        &mut self,
        node: XmlNode<'a, 'input>,
        parent: &Style,
    ) -> Result<Option<Node>, Error> {
        if !node.is_element() {
            return Ok(None);
        }
        self.nodes += 1;
        if self.nodes > self.limits.max_nodes {
            return Err(Error::Limit {
                what: "nodes",
                limit: self.limits.max_nodes as f64,
            });
        }
        if self.depth >= self.limits.max_depth {
            return Err(Error::Limit {
                what: "levels of nesting",
                limit: self.limits.max_depth as f64,
            });
        }
        let declared = self.cascade(node);
        if declared.get("display").is_some_and(|d| d.trim() == "none") {
            return Ok(None);
        }
        let style = self.inherit(parent, &declared);
        let tag = node.tag_name().name();
        let animations = self.animations(node, &declared, &style);
        self.depth += 1;
        let content = match tag {
            "svg" | "g" | "a" | "switch" | "symbol" => self.container(node, &style, &animations)?,
            "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "path" => {
                self.shape(node, &declared, &style, &animations)
            }
            "use" => self.use_element(node, &style)?,
            "image" => self.image(node),
            "text" => {
                self.warn("<text> is left out: export text as outlines");
                None
            }
            "foreignObject" => {
                self.warn("<foreignObject> is left out");
                None
            }
            _ => None,
        };
        self.depth -= 1;
        let Some(content) = content else {
            return Ok(None);
        };
        self.with_group_props(node, &declared, &animations, content)
            .map(Some)
    }

    /// The children of a container as a group.
    fn container(
        &mut self,
        node: XmlNode<'a, 'input>,
        style: &Style,
        _animations: &HashMap<String, Vec<Keyframe<String>>>,
    ) -> Result<Option<Node>, Error> {
        let mut group = Group::new();
        let tag = node.tag_name().name();
        // A nested `svg` (not the root) places its view box in its viewport.
        if tag == "svg" && node.parent_element().is_some() {
            let x = node
                .attribute("x")
                .and_then(|v| length(v, self.view.width))
                .unwrap_or(0.0);
            let y = node
                .attribute("y")
                .and_then(|v| length(v, self.view.height))
                .unwrap_or(0.0);
            if x != 0.0 || y != 0.0 {
                group.transform.push(TransformOp::translate(x, y));
            }
            if let Some(vb) = node
                .attribute("viewBox")
                .and_then(|v| v.parse::<svgtypes::ViewBox>().ok())
            {
                let w = node
                    .attribute("width")
                    .and_then(|v| length(v, self.view.width))
                    .unwrap_or(vb.w);
                let h = node
                    .attribute("height")
                    .and_then(|v| length(v, self.view.height))
                    .unwrap_or(vb.h);
                let aspect = node
                    .attribute("preserveAspectRatio")
                    .and_then(|a| a.parse().ok())
                    .unwrap_or_default();
                group.transform.push(TransformOp::matrix(view_box_matrix(
                    Rect::new(vb.x, vb.y, vb.w, vb.h),
                    aspect,
                    w,
                    h,
                )));
            }
        }
        for child in node.children() {
            if is_reference_only(child) {
                continue;
            }
            if let Some(n) = self.element(child, style)? {
                group.children.push(n);
            }
        }
        if let Some(id) = node.attribute("id") {
            group.id = Some(id.to_owned());
        }
        if group.children.is_empty() {
            return Ok(None);
        }
        Ok(Some(Node::Group(group)))
    }

    /// A `use`: the referenced element moved by `x` and `y`, inheriting the
    /// `use`'s styles.
    fn use_element(
        &mut self,
        node: XmlNode<'a, 'input>,
        style: &Style,
    ) -> Result<Option<Node>, Error> {
        let Some(target) = href(node).and_then(|id| self.ids.get(id).copied()) else {
            self.warn("a <use> refers to an element the document does not have");
            return Ok(None);
        };
        if self.use_stack.contains(&target.id()) || self.use_stack.len() > 32 {
            self.warn("a <use> refers to itself and is left out");
            return Ok(None);
        }
        self.use_stack.push(target.id());
        let content = if target.tag_name().name() == "symbol" {
            let mut group = Group::new();
            if let Some(vb) = target
                .attribute("viewBox")
                .and_then(|v| v.parse::<svgtypes::ViewBox>().ok())
            {
                let w = node
                    .attribute("width")
                    .and_then(|v| length(v, self.view.width))
                    .unwrap_or(vb.w);
                let h = node
                    .attribute("height")
                    .and_then(|v| length(v, self.view.height))
                    .unwrap_or(vb.h);
                let aspect = target
                    .attribute("preserveAspectRatio")
                    .and_then(|a| a.parse().ok())
                    .unwrap_or_default();
                group.transform.push(TransformOp::matrix(view_box_matrix(
                    Rect::new(vb.x, vb.y, vb.w, vb.h),
                    aspect,
                    w,
                    h,
                )));
            }
            for child in target.children() {
                if let Some(n) = self.element(child, style)? {
                    group.children.push(n);
                }
            }
            (!group.children.is_empty()).then_some(Node::Group(group))
        } else {
            self.element(target, style)?
        };
        self.use_stack.pop();
        let Some(content) = content else {
            return Ok(None);
        };
        let x = node
            .attribute("x")
            .and_then(|v| length(v, self.view.width))
            .unwrap_or(0.0);
        let y = node
            .attribute("y")
            .and_then(|v| length(v, self.view.height))
            .unwrap_or(0.0);
        let mut group = Group {
            children: vec![content],
            ..Group::new()
        };
        if x != 0.0 || y != 0.0 {
            group.transform.push(TransformOp::translate(x, y));
        }
        Ok(Some(Node::Group(group)))
    }

    /// An `<image>` holding a `data:` URI.
    fn image(&mut self, node: XmlNode<'a, 'input>) -> Option<Node> {
        let uri = href(node)?;
        let Some((mime, bytes)) = data_uri(uri) else {
            self.warn("an <image> that is not an embedded (data:) PNG or JPEG is left out");
            return None;
        };
        if !(mime == "image/png" || mime == "image/jpeg" || mime == "image/jpg") {
            self.warn(format!("an <image> of type {mime} is left out"));
            return None;
        }
        let natural = image_size(&bytes);
        let x = node
            .attribute("x")
            .and_then(|v| length(v, self.view.width))
            .unwrap_or(0.0);
        let y = node
            .attribute("y")
            .and_then(|v| length(v, self.view.height))
            .unwrap_or(0.0);
        let w = node
            .attribute("width")
            .and_then(|v| length(v, self.view.width))
            .or(natural.map(|n| n.0))?;
        let h = node
            .attribute("height")
            .and_then(|v| length(v, self.view.height))
            .or(natural.map(|n| n.1))?;
        let aspect: svgtypes::AspectRatio = node
            .attribute("preserveAspectRatio")
            .and_then(|a| a.parse().ok())
            .unwrap_or_default();
        let rect = match natural {
            Some((nw, nh)) if aspect.align != svgtypes::Align::None && nw > 0.0 && nh > 0.0 => {
                if aspect.slice {
                    self.warn("an <image> with `slice` is drawn as `meet`");
                }
                let m = view_box_matrix(
                    Rect::new(0.0, 0.0, nw, nh),
                    svgtypes::AspectRatio {
                        slice: false,
                        ..aspect
                    },
                    w,
                    h,
                );
                Rect::new(x + m.e, y + m.f, nw * m.a, nh * m.d)
            }
            _ => Rect::new(x, y, w, h),
        };
        Some(Node::Image(Image {
            id: node.attribute("id").map(str::to_owned),
            name: None,
            data: Arc::new(ImageData {
                mime: if mime == "image/jpg" {
                    "image/jpeg".into()
                } else {
                    mime
                },
                bytes,
            }),
            rect,
        }))
    }

    /// The keyframes of `node`'s animated properties, as text.
    fn animations(
        &mut self,
        node: XmlNode<'a, 'input>,
        declared: &Declared,
        style: &Style,
    ) -> HashMap<String, Vec<Keyframe<String>>> {
        let specs = self.animation_specs(node);
        if specs.is_empty() {
            return HashMap::new();
        }
        let base = |name: &str| -> String {
            if let Some(v) = declared.get(name) {
                return v.clone();
            }
            match name {
                "opacity" | "fill-opacity" | "stroke-opacity" => "1".into(),
                "transform" | "translate" | "rotate" | "scale" => "none".into(),
                "offset-distance" | "stroke-dashoffset" => "0".into(),
                "fill" => paint_css(&style.fill),
                "stroke" => paint_css(&style.stroke),
                "stroke-width" => style.stroke_width.to_string(),
                _ => String::new(),
            }
        };
        let tracks = property_keyframes(&specs, &self.sheet, &base, self.span.duration);
        for name in tracks.keys() {
            if !ANIMATABLE.contains(&name.as_str()) {
                self.warn(format!("animation of `{name}` is left out"));
            }
        }
        tracks
    }

    /// Wraps `content` in a group with the element's transform, opacity,
    /// blend mode, clip and mask (or sets them on `content` when it is a
    /// group of its own).
    fn with_group_props(
        &mut self,
        node: XmlNode<'a, 'input>,
        declared: &Declared,
        animations: &HashMap<String, Vec<Keyframe<String>>>,
        content: Node,
    ) -> Result<Node, Error> {
        let mut bounds_cache: Option<Option<Rect>> = None;
        let mut bounds = |content: &Node| *bounds_cache.get_or_insert_with(|| node_bounds(content));
        let transform = self.transform_ops(declared, animations, &mut || bounds(&content));
        let opacity = match animations.get("opacity") {
            Some(keys) => self.track(
                keys,
                "opacity",
                |v| fraction(v).map(|o| o.clamp(0.0, 1.0)),
                1.0,
            ),
            None => Animated::Static(
                declared
                    .get("opacity")
                    .and_then(|v| fraction(v))
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0),
            ),
        };
        let blend = declared
            .get("mix-blend-mode")
            .map(|v| blend_mode(v.trim()))
            .unwrap_or_default();
        if declared.contains_key("filter")
            && declared.get("filter").is_some_and(|f| f.trim() != "none")
        {
            self.warn("filters (shadows, blurs) are left out");
        }
        let clip = match declared.get("clip-path").map(|v| v.trim()) {
            Some(v) if v != "none" => self.clip(v, bounds(&content)),
            _ => None,
        };
        let mask = match declared.get("mask").map(|v| v.trim()) {
            Some(v) if v != "none" => self.mask(v, bounds(&content))?,
            _ => None,
        };
        let trivial = transform.is_empty()
            && opacity == Animated::Static(1.0)
            && blend == BlendMode::Normal
            && clip.is_none()
            && mask.is_none();
        let id = node.attribute("id").map(str::to_owned);
        let mut group = match content {
            Node::Group(g) => g,
            other if trivial => return Ok(other),
            other => Group {
                children: vec![other],
                ..Group::new()
            },
        };
        group.id = group.id.or(id);
        group.transform.extend(transform);
        group.opacity = opacity;
        group.blend = blend;
        group.clip = clip;
        group.mask = mask.map(Box::new);
        Ok(Node::Group(group))
    }

    /// A typed track from text keyframes; a value that does not parse drops
    /// the animation (with a warning) for the constant `fallback`.
    fn track<T: Interpolate + PartialEq>(
        &mut self,
        keys: &[Keyframe<String>],
        name: &str,
        parse: impl Fn(&str) -> Option<T>,
        fallback: T,
    ) -> Animated<T> {
        let mut out = Vec::with_capacity(keys.len());
        for k in keys {
            match parse(&k.value) {
                Some(v) => out.push(Keyframe::new(k.time, v, k.easing.clone())),
                None => {
                    self.warn(format!(
                        "animation of `{name}` has a value it cannot read (`{}`) and is left out",
                        k.value
                    ));
                    return Animated::Static(fallback);
                }
            }
        }
        Animated::from_keyframes(out, fallback)
    }

    /// The element's transform operations: CSS's `transform-origin`,
    /// `translate`, `rotate`, `scale`, motion path and `transform`, in the
    /// order CSS composes them.
    fn transform_ops(
        &mut self,
        declared: &Declared,
        animations: &HashMap<String, Vec<Keyframe<String>>>,
        bounds: &mut dyn FnMut() -> Option<Rect>,
    ) -> Vec<TransformOp> {
        let fill_box = declared.get("transform-box").is_some_and(|b| {
            matches!(
                b.trim(),
                "fill-box" | "stroke-box" | "content-box" | "border-box"
            )
        });
        let reference = if fill_box {
            bounds().unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
        } else {
            Rect::new(0.0, 0.0, self.view.width, self.view.height)
        };
        let origin = declared
            .get("transform-origin")
            .and_then(|v| position(v, reference))
            .unwrap_or(Point::new(reference.x, reference.y));
        let mut ops = Vec::new();
        // translate
        let box_size = (reference.width, reference.height);
        let translate = |v: &str| -> Option<(f64, f64)> {
            if v.trim() == "none" {
                return Some((0.0, 0.0));
            }
            let parts: Vec<&str> = v.split_whitespace().collect();
            let x = length(parts.first()?, box_size.0)?;
            let y = parts.get(1).map_or(Some(0.0), |p| length(p, box_size.1))?;
            Some((x, y))
        };
        match animations.get("translate") {
            Some(keys) => {
                let x = self.track(keys, "translate", |v| translate(v).map(|t| t.0), 0.0);
                let y = self.track(keys, "translate", |v| translate(v).map(|t| t.1), 0.0);
                ops.push(TransformOp::Translate { x, y });
            }
            None => {
                if let Some((x, y)) = declared.get("translate").and_then(|v| translate(v)) {
                    ops.push(TransformOp::translate(x, y));
                }
            }
        }
        let rotate = |v: &str| {
            if v.trim() == "none" {
                Some(0.0)
            } else {
                angle(v)
            }
        };
        match animations.get("rotate") {
            Some(keys) => ops.push(TransformOp::Rotate {
                degrees: self.track(keys, "rotate", rotate, 0.0),
            }),
            None => {
                if let Some(a) = declared.get("rotate").and_then(|v| rotate(v)) {
                    ops.push(TransformOp::rotate(a));
                }
            }
        }
        let scale = |v: &str| -> Option<(f64, f64)> {
            if v.trim() == "none" {
                return Some((1.0, 1.0));
            }
            let parts: Vec<&str> = v.split_whitespace().collect();
            let x = fraction(parts.first()?)?;
            let y = parts.get(1).map_or(Some(x), |p| fraction(p))?;
            Some((x, y))
        };
        match animations.get("scale") {
            Some(keys) => {
                let x = self.track(keys, "scale", |v| scale(v).map(|s| s.0), 1.0);
                let y = self.track(keys, "scale", |v| scale(v).map(|s| s.1), 1.0);
                ops.push(TransformOp::Scale { x, y });
            }
            None => {
                if let Some((x, y)) = declared.get("scale").and_then(|v| scale(v)) {
                    ops.push(TransformOp::scale(x, y));
                }
            }
        }
        ops.extend(self.motion_ops(declared, animations, origin, reference));
        // transform
        let base_list = declared
            .get("transform")
            .and_then(|v| transform_list(v, box_size))
            .unwrap_or_default();
        match animations.get("transform") {
            Some(keys) => {
                let mut lists = Vec::with_capacity(keys.len());
                for k in keys {
                    match transform_list(&k.value, box_size) {
                        Some(list) => lists.push(Keyframe::new(k.time, list, k.easing.clone())),
                        None => {
                            self.warn(format!("animation of `transform` has a value it cannot read (`{}`) and is left out", k.value));
                            lists.clear();
                            break;
                        }
                    }
                }
                if lists.is_empty() {
                    ops.extend(static_ops(&base_list));
                } else {
                    ops.extend(animated_list_ops(&lists));
                }
            }
            None => {
                if declared
                    .get("transform")
                    .is_some_and(|v| transform_list(v, box_size).is_none())
                {
                    self.warn("a transform it cannot read (3D functions?) is left out");
                }
                ops.extend(static_ops(&base_list));
            }
        }
        let ops = simplify(ops);
        if ops.is_empty() || (origin.x == 0.0 && origin.y == 0.0) {
            return ops;
        }
        let mut out = vec![TransformOp::translate(origin.x, origin.y)];
        out.extend(ops);
        out.push(TransformOp::translate(-origin.x, -origin.y));
        simplify(out)
    }

    /// CSS motion path: `offset-path`, `offset-distance`, `offset-rotate`,
    /// `offset-anchor`.
    fn motion_ops(
        &mut self,
        declared: &Declared,
        animations: &HashMap<String, Vec<Keyframe<String>>>,
        origin: Point,
        reference: Rect,
    ) -> Vec<TransformOp> {
        let Some(path_value) = declared.get("offset-path").map(|v| v.trim()) else {
            return Vec::new();
        };
        if path_value == "none" {
            return Vec::new();
        }
        let Some(path) = css_path_function(path_value) else {
            self.warn(format!(
                "offset-path `{path_value}` is left out (only path() is read)"
            ));
            return Vec::new();
        };
        let total = path_length(&path);
        let distance = |v: &str| -> Option<f64> {
            let v = v.trim();
            if let Some(p) = v.strip_suffix('%') {
                return number(p).map(|n| n / 100.0);
            }
            let l = length(v, total)?;
            Some(if total > 0.0 { l / total } else { 0.0 })
        };
        let progress = match animations.get("offset-distance") {
            Some(keys) => self.track(keys, "offset-distance", distance, 0.0),
            None => Animated::Static(
                declared
                    .get("offset-distance")
                    .and_then(|v| distance(v))
                    .unwrap_or(0.0),
            ),
        };
        let rotate = match declared
            .get("offset-rotate")
            .map(|v| v.trim().to_ascii_lowercase())
        {
            None => MotionRotate::Auto { offset: 0.0 },
            Some(v) => {
                let parts: Vec<&str> = v.split_whitespace().collect();
                let auto = parts.iter().any(|p| *p == "auto" || *p == "reverse");
                let reverse = parts.contains(&"reverse");
                let extra = parts.iter().filter_map(|p| angle(p)).sum::<f64>();
                if auto {
                    MotionRotate::Auto {
                        offset: extra + if reverse { 180.0 } else { 0.0 },
                    }
                } else {
                    MotionRotate::Fixed { degrees: extra }
                }
            }
        };
        let anchor = declared
            .get("offset-anchor")
            .filter(|v| v.trim() != "auto")
            .and_then(|v| position(v, reference))
            .unwrap_or(origin);
        let mut ops = Vec::new();
        if origin != Point::ZERO {
            ops.push(TransformOp::translate(-origin.x, -origin.y));
        }
        ops.push(TransformOp::Motion(MotionPath {
            path,
            progress,
            rotate,
        }));
        let back = origin - anchor;
        if back != Point::ZERO {
            ops.push(TransformOp::translate(back.x, back.y));
        }
        ops
    }

    /// A shape element.
    fn shape(
        &mut self,
        node: XmlNode<'a, 'input>,
        declared: &Declared,
        style: &Style,
        animations: &HashMap<String, Vec<Keyframe<String>>>,
    ) -> Option<Node> {
        if !style.visible {
            return None;
        }
        let geometry = self.geometry(node, declared, animations)?;
        let static_path = dxn_vector_animation::outline::geometry_path(&geometry, 0.0);
        let mut shape = Shape {
            id: node.attribute("id").map(str::to_owned),
            name: None,
            paths: vec![ShapePath::new(geometry)],
            paint_order: style.paint_order,
            ..Shape::default()
        };
        let fill_opacity = match animations.get("fill-opacity") {
            Some(keys) => self.track(
                keys,
                "fill-opacity",
                |v| fraction(v).map(|o| o.clamp(0.0, 1.0)),
                1.0,
            ),
            None => Animated::Static(style.fill_opacity),
        };
        if let Some(paint) = self.paint(&style.fill, style, animations.get("fill"), "fill") {
            shape.fill = Some(Fill {
                paint,
                opacity: fill_opacity,
                rule: style.fill_rule,
            });
        }
        let stroke_opacity = match animations.get("stroke-opacity") {
            Some(keys) => self.track(
                keys,
                "stroke-opacity",
                |v| fraction(v).map(|o| o.clamp(0.0, 1.0)),
                1.0,
            ),
            None => Animated::Static(style.stroke_opacity),
        };
        if let Some(paint) = self.paint(&style.stroke, style, animations.get("stroke"), "stroke") {
            let diagonal =
                (self.view.width.powi(2) + self.view.height.powi(2)).sqrt() / 2f64.sqrt();
            let width = match animations.get("stroke-width") {
                Some(keys) => self.track(
                    keys,
                    "stroke-width",
                    |v| length(v, diagonal).map(|w| w.max(0.0)),
                    style.stroke_width,
                ),
                None => Animated::Static(style.stroke_width),
            };
            // `pathLength` rescales dash lengths to the path's own.
            let scale = node
                .attribute("pathLength")
                .and_then(number)
                .filter(|l| *l > 0.0)
                .map_or(1.0, |l| path_length(&static_path) / l);
            let dash = self.dash(style, animations, diagonal, scale);
            shape.stroke = Some(Stroke {
                paint,
                opacity: stroke_opacity,
                width,
                cap: style.cap,
                join: style.join,
                miter_limit: style.miter_limit,
                dash,
            });
        }
        if shape.fill.is_none() && shape.stroke.is_none() {
            return None;
        }
        Some(Node::shape(shape))
    }

    fn dash(
        &mut self,
        style: &Style,
        animations: &HashMap<String, Vec<Keyframe<String>>>,
        diagonal: f64,
        scale: f64,
    ) -> Option<Dash> {
        let offset = match animations.get("stroke-dashoffset") {
            Some(keys) => self.track(
                keys,
                "stroke-dashoffset",
                |v| length(v, diagonal).map(|o| o * scale),
                style.dashoffset * scale,
            ),
            None => Animated::Static(style.dashoffset * scale),
        };
        let array = match animations.get("stroke-dasharray") {
            Some(keys) => {
                let lists: Option<Vec<Vec<f64>>> = keys
                    .iter()
                    .map(|k| dash_array(&k.value, diagonal))
                    .collect();
                match lists {
                    Some(lists)
                        if lists.iter().all(|l| l.len() == lists[0].len())
                            && !lists[0].is_empty() =>
                    {
                        (0..lists[0].len())
                            .map(|i| {
                                Animated::from_keyframes(
                                    keys.iter()
                                        .zip(&lists)
                                        .map(|(k, l)| {
                                            Keyframe::new(k.time, l[i] * scale, k.easing.clone())
                                        })
                                        .collect(),
                                    0.0,
                                )
                            })
                            .collect()
                    }
                    _ => {
                        self.warn("animation of `stroke-dasharray` between lists of different lengths is left out");
                        style
                            .dasharray
                            .clone()?
                            .into_iter()
                            .map(|d| Animated::Static(d * scale))
                            .collect()
                    }
                }
            }
            None => style
                .dasharray
                .clone()?
                .into_iter()
                .map(|d| Animated::Static(d * scale))
                .collect(),
        };
        Some(Dash { array, offset })
    }

    /// The outline of a shape element.
    fn geometry(
        &mut self,
        node: XmlNode<'a, 'input>,
        declared: &Declared,
        animations: &HashMap<String, Vec<Keyframe<String>>>,
    ) -> Option<Geometry> {
        let (vw, vh) = (self.view.width, self.view.height);
        let diag = (vw * vw + vh * vh).sqrt() / 2f64.sqrt();
        let attr =
            |name: &str, reference: f64| node.attribute(name).and_then(|v| length(v, reference));
        match node.tag_name().name() {
            "rect" => {
                let (x, y) = (attr("x", vw).unwrap_or(0.0), attr("y", vh).unwrap_or(0.0));
                let (w, h) = (attr("width", vw)?, attr("height", vh)?);
                if w <= 0.0 || h <= 0.0 {
                    return None;
                }
                let (rx, ry) = match (attr("rx", vw), attr("ry", vh)) {
                    (Some(rx), Some(ry)) => (rx, ry),
                    (Some(r), None) | (None, Some(r)) => (r, r),
                    (None, None) => (0.0, 0.0),
                };
                Some(Geometry::Rect {
                    center: AnimatedPoint::fixed(x + w / 2.0, y + h / 2.0),
                    size: AnimatedPoint::fixed(w, h),
                    radius: AnimatedPoint::fixed(rx.max(0.0), ry.max(0.0)),
                    direction: Direction::Clockwise,
                    start: RectStart::TopLeft,
                })
            }
            "circle" => {
                let r = attr("r", diag)?;
                (r > 0.0).then(|| Geometry::Ellipse {
                    center: AnimatedPoint::fixed(
                        attr("cx", vw).unwrap_or(0.0),
                        attr("cy", vh).unwrap_or(0.0),
                    ),
                    radius: AnimatedPoint::fixed(r, r),
                    direction: Direction::Clockwise,
                    start: EllipseStart::Right,
                })
            }
            "ellipse" => {
                let (rx, ry) = match (attr("rx", vw), attr("ry", vh)) {
                    (Some(rx), Some(ry)) => (rx, ry),
                    (Some(r), None) | (None, Some(r)) => (r, r),
                    (None, None) => return None,
                };
                (rx > 0.0 && ry > 0.0).then(|| Geometry::Ellipse {
                    center: AnimatedPoint::fixed(
                        attr("cx", vw).unwrap_or(0.0),
                        attr("cy", vh).unwrap_or(0.0),
                    ),
                    radius: AnimatedPoint::fixed(rx, ry),
                    direction: Direction::Clockwise,
                    start: EllipseStart::Right,
                })
            }
            "line" => {
                let mut p = Path::new();
                p.move_to(Point::new(
                    attr("x1", vw).unwrap_or(0.0),
                    attr("y1", vh).unwrap_or(0.0),
                ));
                p.line_to(Point::new(
                    attr("x2", vw).unwrap_or(0.0),
                    attr("y2", vh).unwrap_or(0.0),
                ));
                Some(Geometry::path(p))
            }
            "polyline" | "polygon" => {
                let mut p = Path::new();
                for (i, (x, y)) in
                    svgtypes::PointsParser::from(node.attribute("points")?).enumerate()
                {
                    if i == 0 {
                        p.move_to(Point::new(x, y));
                    } else {
                        p.line_to(Point::new(x, y));
                    }
                }
                if p.elements().len() < 2 {
                    return None;
                }
                if node.tag_name().name() == "polygon" {
                    p.close();
                }
                Some(Geometry::path(p))
            }
            "path" => {
                let base = declared.get("d").and_then(|d| css_or_attr_path(d))?;
                match animations.get("d") {
                    Some(keys) => Some(Geometry::Path {
                        path: self.track(keys, "d", css_or_attr_path, base),
                    }),
                    None => (!base.is_empty()).then(|| Geometry::path(base)),
                }
            }
            _ => None,
        }
    }

    /// The scene paint of `value` (a gradient by reference included), with
    /// animated colors when `keys` animates it.
    fn paint(
        &mut self,
        value: &PaintValue,
        style: &Style,
        keys: Option<&Vec<Keyframe<String>>>,
        name: &str,
    ) -> Option<Paint> {
        if let Some(keys) = keys {
            let current = style.color;
            let track = self.track(
                keys,
                name,
                |v| match paint_value(v.trim())? {
                    PaintValue::Color(c) => Some(c),
                    PaintValue::CurrentColor => Some(current),
                    PaintValue::None => Some(Color::TRANSPARENT),
                    PaintValue::Url(..) => None,
                },
                Color::TRANSPARENT,
            );
            if track.is_animated() {
                return Some(Paint::Solid { color: track });
            }
        }
        match value {
            PaintValue::None => None,
            PaintValue::Color(c) => Some(Paint::solid(*c)),
            PaintValue::CurrentColor => Some(Paint::solid(style.color)),
            PaintValue::Url(id, fallback) => match self.gradient(id) {
                Some(Some(paint)) => Some(paint),
                Some(None) => None,
                None => match fallback.as_deref() {
                    Some(f) => self.paint(f, style, None, name),
                    None => None,
                },
            },
        }
    }

    /// The gradient `id` refers to: `Some(None)` for one that draws nothing,
    /// `None` when there is no gradient by that id.
    fn gradient(&mut self, id: &str) -> Option<Option<Paint>> {
        let node = *self.ids.get(id)?;
        let tag = node.tag_name().name();
        if tag == "pattern" {
            self.warn("pattern fills are left out");
            return Some(None);
        }
        if tag != "linearGradient" && tag != "radialGradient" {
            return None;
        }
        // The element and the ones its `href` chain inherits from.
        let mut chain = vec![node];
        while let Some(next) = href(*chain.last()?).and_then(|i| self.ids.get(i).copied()) {
            if chain.len() > 16 || chain.iter().any(|n| n.id() == next.id()) {
                break;
            }
            chain.push(next);
        }
        let attr = |name: &str| chain.iter().find_map(|n| n.attribute(name));
        let stops_node = chain
            .iter()
            .copied()
            .find(|n| n.children().any(|c| c.tag_name().name() == "stop"))?;
        let mut stops: Vec<GradientStop> = Vec::new();
        for stop in stops_node
            .children()
            .filter(|c| c.tag_name().name() == "stop")
        {
            let declared = self.cascade(stop);
            let offset = stop
                .attribute("offset")
                .and_then(fraction)
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let offset = stops.last().map_or(offset, |s| offset.max(s.offset));
            let color = declared
                .get("stop-color")
                .and_then(|c| color(c))
                .unwrap_or(Color::BLACK);
            let opacity = declared
                .get("stop-opacity")
                .and_then(|o| fraction(o))
                .unwrap_or(1.0);
            stops.push(GradientStop {
                offset,
                color: color.with_opacity(opacity),
            });
        }
        match stops.len() {
            0 => return Some(None),
            1 => return Some(Some(Paint::solid(stops[0].color))),
            _ => {}
        }
        let units = match attr("gradientUnits") {
            Some("userSpaceOnUse") => GradientUnits::UserSpace,
            _ => GradientUnits::BoundingBox,
        };
        let transform = attr("gradientTransform")
            .and_then(|t| transform_list(t, (0.0, 0.0)))
            .map(|l| list_matrix(&l))
            .unwrap_or_default();
        let spread = match attr("spreadMethod") {
            Some("reflect") => Spread::Reflect,
            Some("repeat") => Spread::Repeat,
            _ => Spread::Pad,
        };
        let (vw, vh) = match units {
            GradientUnits::UserSpace => (self.view.width, self.view.height),
            GradientUnits::BoundingBox => (1.0, 1.0),
        };
        let coord = |name: &str, default: &str, reference: f64| {
            attr(name)
                .or(Some(default))
                .and_then(|v| length(v, reference))
                .unwrap_or(0.0)
        };
        let stops = Animated::Static(stops);
        Some(Some(if tag == "linearGradient" {
            Paint::LinearGradient(LinearGradient {
                start: AnimatedPoint::fixed(coord("x1", "0%", vw), coord("y1", "0%", vh)),
                end: AnimatedPoint::fixed(coord("x2", "100%", vw), coord("y2", "0%", vh)),
                stops,
                units,
                transform,
                spread,
            })
        } else {
            let diag = (vw * vw + vh * vh).sqrt() / 2f64.sqrt();
            let (cx, cy) = (coord("cx", "50%", vw), coord("cy", "50%", vh));
            let fx = attr("fx").and_then(|v| length(v, vw)).unwrap_or(cx);
            let fy = attr("fy").and_then(|v| length(v, vh)).unwrap_or(cy);
            Paint::RadialGradient(RadialGradient {
                center: AnimatedPoint::fixed(cx, cy),
                radius: Animated::Static(coord("r", "50%", diag)),
                focal: AnimatedPoint::fixed(fx, fy),
                stops,
                units,
                transform,
                spread,
            })
        }))
    }

    /// The clip `clip-path: url(#id)` names.
    fn clip(&mut self, value: &str, bounds: Option<Rect>) -> Option<Clip> {
        let Some(id) = url_id(value) else {
            self.warn(format!(
                "clip-path `{value}` is left out (only url() is read)"
            ));
            return None;
        };
        let node = *self.ids.get(id)?;
        if node.tag_name().name() != "clipPath" {
            return None;
        }
        let mut outer = Vec::new();
        if node.attribute("clipPathUnits") == Some("objectBoundingBox") {
            let b = bounds?;
            outer.push(TransformOp::matrix(Matrix::new(
                b.width, 0.0, 0.0, b.height, b.x, b.y,
            )));
        }
        if let Some(list) = node
            .attribute("transform")
            .and_then(|t| transform_list(t, (0.0, 0.0)))
        {
            outer.extend(static_ops(&list));
        }
        let clip_style = {
            let declared = self.cascade(node);
            self.inherit(&Style::default(), &declared)
        };
        let mut shapes = Vec::new();
        for child in node.children().filter(|c| c.is_element()) {
            let (child, mut ops) = if child.tag_name().name() == "use" {
                let Some(target) = href(child).and_then(|i| self.ids.get(i).copied()) else {
                    continue;
                };
                let x = child
                    .attribute("x")
                    .and_then(|v| length(v, self.view.width))
                    .unwrap_or(0.0);
                let y = child
                    .attribute("y")
                    .and_then(|v| length(v, self.view.height))
                    .unwrap_or(0.0);
                (target, vec![TransformOp::translate(x, y)])
            } else {
                (child, Vec::new())
            };
            let declared = self.cascade(child);
            let style = self.inherit(&clip_style, &declared);
            let Some(geometry) = self.geometry(child, &declared, &HashMap::new()) else {
                if child.tag_name().name() == "text" {
                    self.warn("<text> in a clip path is left out");
                }
                continue;
            };
            if let Some(list) = declared
                .get("transform")
                .and_then(|t| transform_list(t, (0.0, 0.0)))
            {
                ops.extend(static_ops(&list));
            }
            let mut transform = outer.clone();
            transform.extend(ops);
            shapes.push(ClipShape {
                geometry,
                transform: simplify(transform),
                rule: style.clip_rule,
            });
        }
        Some(Clip { shapes })
    }

    /// The mask `mask: url(#id)` names.
    fn mask(&mut self, value: &str, bounds: Option<Rect>) -> Result<Option<Mask>, Error> {
        let Some(id) = url_id(value) else {
            self.warn(format!("mask `{value}` is left out (only url() is read)"));
            return Ok(None);
        };
        let Some(node) = self.ids.get(id).copied() else {
            return Ok(None);
        };
        if node.tag_name().name() != "mask" || self.use_stack.contains(&node.id()) {
            return Ok(None);
        }
        let declared = self.cascade(node);
        let kind = match declared.get("mask-type").map(|v| v.trim()) {
            Some("alpha") => MaskKind::Alpha,
            _ => MaskKind::Luminance,
        };
        let user_units = node.attribute("maskUnits") == Some("userSpaceOnUse");
        let region = if user_units {
            let (vw, vh) = (self.view.width, self.view.height);
            let get = |n: &str, d: &str, r: f64| {
                node.attribute(n)
                    .or(Some(d))
                    .and_then(|v| length(v, r))
                    .unwrap_or(0.0)
            };
            Some(Rect::new(
                get("x", "-10%", vw),
                get("y", "-10%", vh),
                get("width", "120%", vw),
                get("height", "120%", vh),
            ))
        } else {
            bounds.map(|b| {
                let get = |n: &str, d: f64| node.attribute(n).and_then(fraction).unwrap_or(d);
                Rect::new(
                    b.x + get("x", -0.1) * b.width,
                    b.y + get("y", -0.1) * b.height,
                    get("width", 1.2) * b.width,
                    get("height", 1.2) * b.height,
                )
            })
        };
        let style = self.inherit(&Style::default(), &declared);
        self.use_stack.push(node.id());
        let mut content = Group::new();
        for child in node.children() {
            if let Some(n) = self.element(child, &style)? {
                content.children.push(n);
            }
        }
        self.use_stack.pop();
        if node.attribute("maskContentUnits") == Some("objectBoundingBox")
            && let Some(b) = bounds
        {
            content.transform.push(TransformOp::matrix(Matrix::new(
                b.width, 0.0, 0.0, b.height, b.x, b.y,
            )));
        }
        let mut masked = Group {
            children: vec![Node::Group(content)],
            ..Group::new()
        };
        if let Some(r) = region {
            masked.clip = Some(Clip {
                shapes: vec![ClipShape {
                    geometry: Geometry::Rect {
                        center: AnimatedPoint::fixed(r.x + r.width / 2.0, r.y + r.height / 2.0),
                        size: AnimatedPoint::fixed(r.width, r.height),
                        radius: AnimatedPoint::fixed(0.0, 0.0),
                        direction: Direction::Clockwise,
                        start: RectStart::TopLeft,
                    },
                    transform: Vec::new(),
                    rule: FillRule::NonZero,
                }],
            });
        }
        Ok(Some(Mask {
            kind,
            inverted: false,
            content: masked,
        }))
    }
}

/// Elements drawn only where something refers to them.
fn is_reference_only(node: XmlNode<'_, '_>) -> bool {
    matches!(
        node.tag_name().name(),
        "defs"
            | "clipPath"
            | "mask"
            | "linearGradient"
            | "radialGradient"
            | "pattern"
            | "filter"
            | "symbol"
            | "marker"
            | "style"
            | "script"
            | "title"
            | "desc"
            | "metadata"
    )
}

fn href<'a>(node: XmlNode<'a, '_>) -> Option<&'a str> {
    node.attribute((XLINK, "href"))
        .or_else(|| node.attribute("href"))
        .and_then(|h| h.strip_prefix('#'))
}

/// The id of `url(#id)`.
fn url_id(value: &str) -> Option<&str> {
    let inner = value.trim().strip_prefix("url(")?.split(')').next()?;
    let inner = inner.trim().trim_matches(|c| c == '"' || c == '\'');
    inner.strip_prefix('#')
}

fn paint_value(s: &str) -> Option<PaintValue> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("transparent") {
        return Some(PaintValue::Color(Color::TRANSPARENT));
    }
    Some(match svgtypes::Paint::from_str(s).ok()? {
        svgtypes::Paint::None => PaintValue::None,
        svgtypes::Paint::CurrentColor => PaintValue::CurrentColor,
        svgtypes::Paint::Color(c) => {
            PaintValue::Color(Color::rgba8(c.red, c.green, c.blue, c.alpha))
        }
        svgtypes::Paint::FuncIRI(id, fallback) => PaintValue::Url(
            id.to_owned(),
            fallback.map(|f| {
                Box::new(match f {
                    svgtypes::PaintFallback::None => PaintValue::None,
                    svgtypes::PaintFallback::CurrentColor => PaintValue::CurrentColor,
                    svgtypes::PaintFallback::Color(c) => {
                        PaintValue::Color(Color::rgba8(c.red, c.green, c.blue, c.alpha))
                    }
                })
            }),
        ),
        _ => return None,
    })
}

/// A paint as CSS text, for keyframes that leave it out.
fn paint_css(p: &PaintValue) -> String {
    match p {
        PaintValue::Color(c) => format!(
            "rgba({},{},{},{})",
            (c.r * 255.0).round(),
            (c.g * 255.0).round(),
            (c.b * 255.0).round(),
            c.a
        ),
        PaintValue::CurrentColor => "currentColor".into(),
        PaintValue::None | PaintValue::Url(..) => "none".into(),
    }
}

fn rule(v: &str) -> FillRule {
    if v.trim() == "evenodd" {
        FillRule::EvenOdd
    } else {
        FillRule::NonZero
    }
}

fn blend_mode(v: &str) -> BlendMode {
    match v {
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        "overlay" => BlendMode::Overlay,
        "darken" => BlendMode::Darken,
        "lighten" => BlendMode::Lighten,
        "color-dodge" => BlendMode::ColorDodge,
        "color-burn" => BlendMode::ColorBurn,
        "hard-light" => BlendMode::HardLight,
        "soft-light" => BlendMode::SoftLight,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        "hue" => BlendMode::Hue,
        "saturation" => BlendMode::Saturation,
        "color" => BlendMode::Color,
        "luminosity" => BlendMode::Luminosity,
        "plus-lighter" => BlendMode::Add,
        _ => BlendMode::Normal,
    }
}

fn dash_array(v: &str, reference: f64) -> Option<Vec<f64>> {
    let v = v.trim();
    if v == "none" {
        return None;
    }
    let mut out: Vec<f64> = v
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| length(s, reference))
        .collect::<Option<_>>()?;
    if out.iter().any(|d| *d < 0.0) || out.iter().sum::<f64>() <= 0.0 {
        return None;
    }
    if out.len() % 2 == 1 {
        out.extend(out.clone());
    }
    Some(out)
}

/// A CSS position (`transform-origin`, `offset-anchor`) within `reference`.
fn position(v: &str, reference: Rect) -> Option<Point> {
    let parts: Vec<String> = v
        .split_whitespace()
        .map(|p| p.to_ascii_lowercase())
        .collect();
    let horizontal = |p: &str| match p {
        "left" => Some(0.0),
        "center" => Some(0.5),
        "right" => Some(1.0),
        _ => None,
    };
    let vertical = |p: &str| match p {
        "top" => Some(0.0),
        "center" => Some(0.5),
        "bottom" => Some(1.0),
        _ => None,
    };
    let x_of = |p: &str| {
        horizontal(p)
            .map(|f| f * reference.width)
            .or_else(|| length(p, reference.width))
    };
    let y_of = |p: &str| {
        vertical(p)
            .map(|f| f * reference.height)
            .or_else(|| length(p, reference.height))
    };
    let (x, y) = match parts.as_slice() {
        [one] => {
            if matches!(one.as_str(), "top" | "bottom") {
                (0.5 * reference.width, y_of(one)?)
            } else {
                (x_of(one)?, 0.5 * reference.height)
            }
        }
        [a, b, ..] => {
            // Keywords may come in either order (`top left`).
            if matches!(a.as_str(), "top" | "bottom") || matches!(b.as_str(), "left" | "right") {
                (x_of(b)?, y_of(a)?)
            } else {
                (x_of(a)?, y_of(b)?)
            }
        }
        [] => return None,
    };
    Some(Point::new(reference.x + x, reference.y + y))
}

/// Path data as the `d` attribute or CSS `path("…")` writes it. A path
/// with an error keeps what came before it, as SVG renders.
fn css_or_attr_path(v: &str) -> Option<Path> {
    let v = v.trim();
    if v == "none" {
        return Some(Path::new());
    }
    match css_path_function(v) {
        Some(p) => Some(p),
        None if v.starts_with("path(") => None,
        None => Some(path_data(v)),
    }
}

/// `path('…')`'s path.
fn css_path_function(v: &str) -> Option<Path> {
    let inner = v.trim().strip_prefix("path(")?.strip_suffix(')')?.trim();
    // An optional fill rule first: `path(evenodd, '…')`.
    let inner = inner.rsplit(',').next().map_or(inner, |last| {
        if last.trim_start().starts_with(['"', '\'']) {
            last
        } else {
            inner
        }
    });
    let data = inner.trim().trim_matches(|c| c == '"' || c == '\'');
    Some(path_data(data))
}

fn path_data(d: &str) -> Path {
    let mut path = Path::new();
    for seg in svgtypes::SimplifyingPathParser::from(d) {
        let Ok(seg) = seg else { break };
        match seg {
            svgtypes::SimplePathSegment::MoveTo { x, y } => path.move_to(Point::new(x, y)),
            svgtypes::SimplePathSegment::LineTo { x, y } => path.line_to(Point::new(x, y)),
            svgtypes::SimplePathSegment::Quadratic { x1, y1, x, y } => {
                path.quad_to(Point::new(x1, y1), Point::new(x, y))
            }
            svgtypes::SimplePathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => path.cubic_to(Point::new(x1, y1), Point::new(x2, y2), Point::new(x, y)),
            svgtypes::SimplePathSegment::ClosePath => path.close(),
        }
    }
    path
}

/// The matrix placing view box `vb` in a viewport of `width` × `height`.
fn view_box_matrix(vb: Rect, aspect: svgtypes::AspectRatio, width: f64, height: f64) -> Matrix {
    if vb.width <= 0.0 || vb.height <= 0.0 {
        return Matrix::IDENTITY;
    }
    let (sx, sy) = (width / vb.width, height / vb.height);
    use svgtypes::Align::*;
    if aspect.align == None {
        return Matrix::new(sx, 0.0, 0.0, sy, -vb.x * sx, -vb.y * sy);
    }
    let s = if aspect.slice { sx.max(sy) } else { sx.min(sy) };
    let (free_x, free_y) = (width - vb.width * s, height - vb.height * s);
    let (ax, ay) = match aspect.align {
        XMinYMin => (0.0, 0.0),
        XMidYMin => (0.5, 0.0),
        XMaxYMin => (1.0, 0.0),
        XMinYMid => (0.0, 0.5),
        XMidYMid | None => (0.5, 0.5),
        XMaxYMid => (1.0, 0.5),
        XMinYMax => (0.0, 1.0),
        XMidYMax => (0.5, 1.0),
        XMaxYMax => (1.0, 1.0),
    };
    Matrix::new(
        s,
        0.0,
        0.0,
        s,
        free_x * ax - vb.x * s,
        free_y * ay - vb.y * s,
    )
}

/// Transform operations of a constant list.
fn static_ops(list: &[TransformFn]) -> Vec<TransformOp> {
    list.iter()
        .map(|f| match *f {
            TransformFn::Translate(x, y) => TransformOp::translate(x, y),
            TransformFn::Scale(x, y) => TransformOp::scale(x, y),
            TransformFn::Rotate(a) => TransformOp::rotate(a),
            TransformFn::Skew(x, y) => TransformOp::Skew {
                x: Animated::Static(x),
                y: Animated::Static(y),
            },
            TransformFn::Matrix(m) => TransformOp::matrix(m),
        })
        .collect()
}

/// The operations of a keyframed transform list: function by function
/// when every keyframe has the same functions (shorter lists padded with
/// identities, as CSS does), else one interpolated matrix.
fn animated_list_ops(keys: &[Keyframe<Vec<TransformFn>>]) -> Vec<TransformOp> {
    let longest = keys.iter().map(|k| k.value.len()).max().unwrap_or(0);
    let Some(template) = keys
        .iter()
        .find(|k| k.value.len() == longest)
        .map(|k| k.value.clone())
    else {
        return Vec::new();
    };
    let compatible = keys
        .iter()
        .all(|k| k.value.iter().zip(&template).all(|(a, b)| a.same_kind(b)));
    if !compatible {
        let track = Track::new(
            keys.iter()
                .map(|k| Keyframe::new(k.time, list_matrix(&k.value), k.easing.clone()))
                .collect(),
        );
        return vec![TransformOp::Matrix {
            matrix: Animated::Track(track),
        }];
    }
    let padded: Vec<Vec<TransformFn>> = keys
        .iter()
        .map(|k| {
            let mut v = k.value.clone();
            v.extend(template[v.len()..].iter().map(TransformFn::identity));
            v
        })
        .collect();
    let arg = |j: usize, f: &dyn Fn(&TransformFn) -> f64| -> Animated<f64> {
        Animated::from_keyframes(
            keys.iter()
                .zip(&padded)
                .map(|(k, list)| Keyframe::new(k.time, f(&list[j]), k.easing.clone()))
                .collect(),
            0.0,
        )
    };
    template
        .iter()
        .enumerate()
        .map(|(j, f)| match f {
            TransformFn::Translate(..) => TransformOp::Translate {
                x: arg(j, &|f| {
                    if let TransformFn::Translate(x, _) = f {
                        *x
                    } else {
                        0.0
                    }
                }),
                y: arg(j, &|f| {
                    if let TransformFn::Translate(_, y) = f {
                        *y
                    } else {
                        0.0
                    }
                }),
            },
            TransformFn::Scale(..) => TransformOp::Scale {
                x: arg(j, &|f| {
                    if let TransformFn::Scale(x, _) = f {
                        *x
                    } else {
                        1.0
                    }
                }),
                y: arg(j, &|f| {
                    if let TransformFn::Scale(_, y) = f {
                        *y
                    } else {
                        1.0
                    }
                }),
            },
            TransformFn::Rotate(_) => TransformOp::Rotate {
                degrees: arg(j, &|f| {
                    if let TransformFn::Rotate(a) = f {
                        *a
                    } else {
                        0.0
                    }
                }),
            },
            TransformFn::Skew(..) => TransformOp::Skew {
                x: arg(j, &|f| {
                    if let TransformFn::Skew(x, _) = f {
                        *x
                    } else {
                        0.0
                    }
                }),
                y: arg(j, &|f| {
                    if let TransformFn::Skew(_, y) = f {
                        *y
                    } else {
                        0.0
                    }
                }),
            },
            TransformFn::Matrix(_) => TransformOp::Matrix {
                matrix: Animated::from_keyframes(
                    keys.iter()
                        .zip(&padded)
                        .map(|(k, list)| {
                            let m = match list[j] {
                                TransformFn::Matrix(m) => m,
                                _ => Matrix::IDENTITY,
                            };
                            Keyframe::new(k.time, m, k.easing.clone())
                        })
                        .collect(),
                    Matrix::IDENTITY,
                ),
            },
        })
        .collect()
}

/// `ops` without operations that change nothing, consecutive constant
/// translations merged.
pub(crate) fn simplify(ops: Vec<TransformOp>) -> Vec<TransformOp> {
    let mut out: Vec<TransformOp> = Vec::with_capacity(ops.len());
    for op in ops {
        let identity = match &op {
            TransformOp::Translate { x, y } | TransformOp::Skew { x, y } => {
                x.as_static() == Some(&0.0) && y.as_static() == Some(&0.0)
            }
            TransformOp::Scale { x, y } => {
                x.as_static() == Some(&1.0) && y.as_static() == Some(&1.0)
            }
            TransformOp::Rotate { degrees } => degrees.as_static() == Some(&0.0),
            TransformOp::Matrix { matrix } => matrix.as_static() == Some(&Matrix::IDENTITY),
            TransformOp::Motion(_) => false,
        };
        if identity {
            continue;
        }
        if let (
            Some(TransformOp::Translate {
                x: Animated::Static(ax),
                y: Animated::Static(ay),
            }),
            TransformOp::Translate {
                x: Animated::Static(bx),
                y: Animated::Static(by),
            },
        ) = (out.last_mut(), &op)
        {
            *ax += bx;
            *ay += by;
            if *ax == 0.0 && *ay == 0.0 {
                out.pop();
            }
            continue;
        }
        out.push(op);
    }
    out
}

/// The bounds of `node` at time 0, in its parent's space.
fn node_bounds(node: &Node) -> Option<Rect> {
    let scene = Scene {
        width: 0.0,
        height: 0.0,
        timeline: Timeline::default(),
        root: Group {
            children: vec![node.clone()],
            ..Group::new()
        },
    };
    let mut bounds: Option<(Point, Point)> = None;
    let mut add = |rect: Rect, m: Matrix| {
        for corner in [
            Point::new(rect.x, rect.y),
            Point::new(rect.x + rect.width, rect.y),
            Point::new(rect.x, rect.y + rect.height),
            Point::new(rect.x + rect.width, rect.y + rect.height),
        ] {
            let p = m.apply(corner);
            bounds = Some(match bounds {
                None => (p, p),
                Some((lo, hi)) => (
                    Point::new(lo.x.min(p.x), lo.y.min(p.y)),
                    Point::new(hi.x.max(p.x), hi.y.max(p.y)),
                ),
            });
        }
    };
    for item in scene.frame_at(0.0).items {
        match item {
            Item::Fill {
                path, transform, ..
            }
            | Item::Stroke {
                path, transform, ..
            } => {
                if let Some(b) = path.bounds() {
                    add(b, transform);
                }
            }
            Item::Image {
                rect, transform, ..
            } => add(rect, transform),
            _ => {}
        }
    }
    bounds.map(|(lo, hi)| Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y))
}

/// The media type and bytes of a base64 `data:` URI.
fn data_uri(uri: &str) -> Option<(String, Vec<u8>)> {
    use base64::Engine;
    let rest = uri.trim().strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mime = meta
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !meta.contains(";base64") {
        return None;
    }
    let cleaned: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .ok()?;
    Some((mime, bytes))
}

/// The pixel size a PNG or JPEG declares.
fn image_size(bytes: &[u8]) -> Option<(f64, f64)> {
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return Some((f64::from(w), f64::from(h)));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            let len = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
            if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]);
                return Some((f64::from(w), f64::from(h)));
            }
            i += 2 + len;
        }
    }
    None
}
