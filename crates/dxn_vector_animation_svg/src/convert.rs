//! The SVG document turned into a scene: the cascade, shapes, groups,
//! paints, clips, masks, `use`, images, and their CSS animations.

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use dxn_vector_animation::{
    Animated, AnimatedPoint, BlendMode, Blur, Clip, ClipShape, Color, Dash, Direction, DropShadow,
    EVENT_PREFIX, Effect, EllipseStart, Error, Fill, FillRule, Font, Geometry, GradientStop,
    GradientUnits, Group, Image, ImageData, Interpolate, Item, Keyframe, Limits, LineCap, LineJoin,
    LinearGradient, Marker, Mask, MaskKind, Matrix, MotionPath, MotionRotate, Node, Paint,
    PaintOrder, Path, Pattern, Point, RadialGradient, Rect, RectStart, Scene, Shape, ShapePath,
    Spread, Stroke, Text, TextAnchor, TextSpan, Timeline, Track, TransformOp, ViewBox,
    outline::path_length,
};
use roxmltree::Node as XmlNode;

use crate::{
    animate::{TimelineSpan, property_keyframes},
    css::{Stylesheet, parse_declarations},
    filter,
    smil::{self, Smil},
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
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "text-anchor",
    "letter-spacing",
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
    "visibility",
    "x",
    "y",
    "width",
    "height",
    "rx",
    "ry",
    "cx",
    "cy",
    "r",
    "x1",
    "y1",
    "x2",
    "y2",
    "points",
    "filter",
    "font-size",
    "letter-spacing",
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
    font_families: Vec<String>,
    font_size: f64,
    font_weight: u16,
    italic: bool,
    anchor: TextAnchor,
    letter_spacing: f64,
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
            font_families: vec!["sans-serif".to_owned()],
            font_size: 16.0,
            font_weight: 400,
            italic: false,
            anchor: TextAnchor::Start,
            letter_spacing: 0.0,
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
    /// The patterns being read (one filling with itself is left out), and
    /// those read, by their element.
    pattern_stack: Vec<roxmltree::NodeId>,
    /// The baseline of the text being read.
    y: f64,
    patterns: HashMap<roxmltree::NodeId, Option<Paint>>,
    depth: usize,
    nodes: usize,
    declared: HashMap<roxmltree::NodeId, Declared>,
    /// The document's SMIL animations, their begin times, and which
    /// animate each element.
    smil: Vec<Smil>,
    smil_timing: smil::Timing,
    smil_targets: HashMap<roxmltree::NodeId, Vec<usize>>,
    /// The SMIL animations whose element was drawn.
    smil_used: std::collections::HashSet<usize>,
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
            pattern_stack: Vec::new(),
            y: 0.0,
            patterns: HashMap::new(),
            depth: 0,
            nodes: 0,
            declared: HashMap::new(),
            smil: Vec::new(),
            smil_timing: smil::Timing::default(),
            smil_targets: HashMap::new(),
            smil_used: std::collections::HashSet::new(),
        };
        // SMIL: its elements, and when each begins.
        let mut smil_warnings = Vec::new();
        converter.smil = smil::collect(
            doc,
            &converter.ids,
            &|n| n.attribute("d").map(path_data),
            &mut |w| smil_warnings.push(w),
        );
        for w in smil_warnings {
            converter.warn(w);
        }
        let mut smil_warnings = Vec::new();
        converter.smil_timing = smil::resolve(&converter.smil, limits.max_duration, &mut |w| {
            smil_warnings.push(w);
        });
        for w in smil_warnings {
            converter.warn(w);
        }
        for (i, a) in converter.smil.iter().enumerate() {
            converter.smil_targets.entry(a.target).or_default().push(i);
        }
        // The timeline every animation fits in.
        for node in doc.descendants().filter(|n| n.is_element()) {
            for spec in converter.animation_specs(node) {
                converter.span.add(&spec);
            }
        }
        let smil_span = (
            converter.smil_timing.duration,
            converter.smil_timing.endless,
        );
        if converter.smil_timing.cyclic && converter.span.duration <= smil_span.0 {
            // A cycle of SMIL animations is the loop.
            converter.span.duration = smil_span.0;
        } else {
            converter.smil_timing.cyclic = false;
            converter.span.duration = converter.span.duration.max(smil_span.0);
            for &(start, period) in &converter.smil_timing.repeats {
                converter.span.repeat(start, period);
            }
            converter.span.ends = converter.span.ends.max(converter.smil_timing.ends);
            if let Some(w) = converter.span.close_loop(limits.max_duration) {
                converter.warn(w);
            }
        }
        converter.span.endless |= smil_span.1;
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
        let unused: Vec<String> = converter
            .smil
            .iter()
            .enumerate()
            .filter(|(i, _)| !converter.smil_used.contains(i))
            .map(|(_, a)| {
                format!(
                    "SMIL animation of `{}` on <{}> is left out",
                    a.attribute, a.target_tag
                )
            })
            .collect();
        for w in unused {
            converter.warn(w);
        }
        let scene = Scene {
            width,
            height,
            timeline: Timeline {
                duration: converter.span.duration,
                frame_rate: None,
                loop_start: converter.span.loop_start,
                loop_mode: if converter.span.endless {
                    dxn_vector_animation::LoopMode::Loop
                } else {
                    dxn_vector_animation::LoopMode::Once
                },
                markers: converter
                    .smil_timing
                    .cues
                    .iter()
                    .map(|(name, start, duration)| Marker {
                        name: format!("{EVENT_PREFIX}{name}"),
                        start: *start,
                        duration: *duration,
                    })
                    .collect(),
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
        if let Some(v) = get("font-family") {
            let families: Vec<String> = v
                .split(',')
                .map(|f| f.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
                .filter(|f| !f.is_empty())
                .collect();
            if !families.is_empty() {
                s.font_families = families;
            }
        }
        if let Some(v) = get("font-size") {
            let parent = parent.font_size;
            let size = match v.to_ascii_lowercase().as_str() {
                "xx-small" => Some(9.0),
                "x-small" => Some(10.0),
                "small" => Some(13.0),
                "medium" => Some(16.0),
                "large" => Some(18.0),
                "x-large" => Some(24.0),
                "xx-large" => Some(32.0),
                "larger" => Some(parent * 1.2),
                "smaller" => Some(parent / 1.2),
                v => match v.strip_suffix("em").and_then(number) {
                    Some(em) if !v.ends_with("rem") => Some(em * parent),
                    _ => length(v, parent),
                },
            };
            if let Some(size) = size.filter(|v| v.is_finite()) {
                s.font_size = size.max(0.0);
            }
        }
        if let Some(v) = get("font-weight") {
            s.font_weight = match v {
                "normal" => 400,
                "bold" => 700,
                "bolder" => (parent.font_weight + 300).min(900),
                "lighter" => parent.font_weight.saturating_sub(300).max(100),
                n => number(n).map_or(s.font_weight, |w| w.clamp(1.0, 1000.0) as u16),
            };
        }
        if let Some(v) = get("font-style") {
            s.italic = matches!(v, "italic" | "oblique") || v.starts_with("oblique");
        }
        if let Some(v) = get("text-anchor") {
            s.anchor = match v {
                "middle" => TextAnchor::Middle,
                "end" => TextAnchor::End,
                _ => TextAnchor::Start,
            };
        }
        if let Some(v) = get("letter-spacing") {
            s.letter_spacing = if v == "normal" {
                0.0
            } else {
                match v.strip_suffix("em").and_then(number) {
                    Some(em) => em * s.font_size,
                    None => length(v, s.font_size).unwrap_or(0.0),
                }
            };
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
            "text" => self.text(node, &style),
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

    /// A `<text>`: its spans (its own text and its `<tspan>`s'), placed
    /// where they say, whitespace collapsed as SVG does unless
    /// `xml:space="preserve"`.
    fn text(&mut self, node: XmlNode<'a, 'input>, style: &Style) -> Option<Node> {
        if !style.visible {
            return None;
        }
        let preserve = node.attribute((roxmltree::NS_XML_URI, "space")) == Some("preserve");
        let mut spans: Vec<TextSpan> = Vec::new();
        self.y = 0.0;
        self.text_spans(node, style, true, preserve, &mut spans);
        if !preserve {
            // Spaces where one span ends and the next begins collapse too;
            // none before the text or after it.
            let mut space = true;
            for span in &mut spans {
                if space && span.text.starts_with(' ') {
                    span.text.remove(0);
                }
                if !span.text.is_empty() {
                    space = span.text.ends_with(' ');
                }
            }
            if let Some(last) = spans.iter_mut().rev().find(|s| !s.text.is_empty()) {
                last.text.truncate(last.text.trim_end().len());
            }
        }
        // Empty spans that place nothing go.
        spans.retain(|s| !s.text.is_empty() || s.position.is_some());
        if spans.iter().all(|s| s.text.trim().is_empty())
            || spans.iter().all(|s| s.fill.is_none() && s.stroke.is_none())
        {
            return None;
        }
        Some(Node::Text(Box::new(Text {
            id: node.attribute("id").map(str::to_owned),
            name: None,
            spans,
        })))
    }

    /// The spans of the text content element `node` (its style `style`),
    /// in order, into `out`.
    fn text_spans(
        &mut self,
        node: XmlNode<'a, 'input>,
        style: &Style,
        first: bool,
        preserve: bool,
        out: &mut Vec<TextSpan>,
    ) {
        // The first of a list of lengths; ems are the element's font size.
        let coord = |name: &str, reference: f64| {
            node.attribute(name).and_then(|v| {
                let first = v
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .find(|p| !p.is_empty())?;
                match first.strip_suffix("em").and_then(number) {
                    Some(em) if !first.ends_with("rem") => Some(em * style.font_size),
                    _ => length(first, reference),
                }
            })
        };
        if node
            .attribute("x")
            .or(node.attribute("y"))
            .is_some_and(|v| {
                v.split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|p| !p.is_empty())
                    .count()
                    > 1
            })
        {
            self.warn("a position for each character of text is left out: the first places it");
        }
        let (x, y) = (coord("x", self.view.width), coord("y", self.view.height));
        if let Some(y) = y {
            self.y = y;
        }
        // What CSS and SMIL animate of it.
        let declared = self.cascade(node);
        let animations = self.animations(node, &declared, style);
        let font_size = style.font_size;
        let ems = move |v: &str| match v.trim().strip_suffix("em").and_then(number) {
            Some(em) if !v.trim().ends_with("rem") => Some(em * font_size),
            _ => length(v, font_size),
        };
        let track = |s: &mut Self, name: &str, fallback: f64| match animations.get(name) {
            Some(keys) => s.track(keys, name, ems, fallback),
            None => Animated::Static(fallback),
        };
        let moving_x = track(self, "x", x.unwrap_or(0.0));
        let moving_y = track(self, "y", y.unwrap_or(self.y));
        let size = track(self, "font-size", style.font_size);
        let spacing = track(self, "letter-spacing", style.letter_spacing);
        let placed =
            first || x.is_some() || y.is_some() || moving_x.is_animated() || moving_y.is_animated();
        let mut position = placed.then_some(AnimatedPoint {
            x: moving_x,
            y: moving_y,
        });
        let mut shift = Point::new(
            coord("dx", self.view.width).unwrap_or(0.0),
            coord("dy", self.view.height).unwrap_or(0.0),
        );
        let opacity = |s: &mut Self, name: &str, fallback: f64| match animations.get(name) {
            Some(keys) => s.track(
                keys,
                name,
                |v| fraction(v).map(|o| o.clamp(0.0, 1.0)),
                fallback,
            ),
            None => Animated::Static(fallback),
        };
        let fill_opacity = opacity(self, "fill-opacity", style.fill_opacity);
        let fill = self
            .paint(&style.fill, style, animations.get("fill"), "fill")
            .map(|paint| Fill {
                paint,
                opacity: fill_opacity,
                rule: FillRule::NonZero,
            });
        let stroke_opacity = opacity(self, "stroke-opacity", style.stroke_opacity);
        let stroke = self
            .paint(&style.stroke, style, animations.get("stroke"), "stroke")
            .filter(|_| style.stroke_width > 0.0)
            .map(|paint| Stroke {
                paint,
                opacity: stroke_opacity,
                width: Animated::Static(style.stroke_width),
                cap: style.cap,
                join: style.join,
                miter_limit: style.miter_limit,
                dash: None,
            });
        for child in node.children() {
            if let Some(text) = child.text().filter(|_| child.is_text()) {
                let text = if preserve {
                    text.replace(['\n', '\r', '\t'], " ")
                } else {
                    let mut collapsed = String::with_capacity(text.len());
                    for c in text.chars().filter(|c| *c != '\n' && *c != '\r') {
                        let c = if c == '\t' { ' ' } else { c };
                        if !(c == ' ' && collapsed.ends_with(' ')) {
                            collapsed.push(c);
                        }
                    }
                    collapsed
                };
                out.push(TextSpan {
                    text,
                    position: position.take(),
                    shift: std::mem::take(&mut shift),
                    anchor: style.anchor,
                    font: Font {
                        families: style.font_families.clone(),
                        size: size.clone(),
                        weight: style.font_weight,
                        italic: style.italic,
                    },
                    letter_spacing: spacing.clone(),
                    fill: fill.clone(),
                    stroke: stroke.clone(),
                    paint_order: style.paint_order,
                });
                continue;
            }
            if !child.is_element() {
                continue;
            }
            let declared = self.cascade(child);
            if declared.get("display").is_some_and(|d| d.trim() == "none") {
                continue;
            }
            let inner = self.inherit(style, &declared);
            match child.tag_name().name() {
                "tspan" | "a" => {}
                "textPath" => {
                    self.warn("text on a path is laid out on a line");
                }
                _ => continue,
            }
            // A placed span with nothing before it still places what
            // follows.
            if position.is_some() || shift != Point::new(0.0, 0.0) {
                out.push(TextSpan {
                    text: String::new(),
                    position: position.take(),
                    shift: std::mem::take(&mut shift),
                    anchor: style.anchor,
                    font: Font::default(),
                    ..TextSpan::default()
                });
            }
            self.text_spans(child, &inner, false, preserve, out);
        }
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
        let smil = self
            .smil_targets
            .get(&node.id())
            .cloned()
            .unwrap_or_default();
        if specs.is_empty() && smil.is_empty() {
            return HashMap::new();
        }
        let base = |name: &str| -> String {
            if let Some(v) = declared.get(name) {
                return v.clone();
            }
            if let Some(v) = node.attribute(name) {
                return v.to_owned();
            }
            match name {
                "opacity" | "fill-opacity" | "stroke-opacity" => "1".into(),
                "transform" | "translate" | "rotate" | "scale" => "none".into(),
                "offset-distance" | "stroke-dashoffset" => "0".into(),
                // Geometry left out is at 0.
                "x" | "y" | "cx" | "cy" | "r" | "x1" | "y1" | "x2" | "y2" | "width" | "height" => {
                    "0".into()
                }
                "fill" => paint_css(&style.fill),
                "stroke" => paint_css(&style.stroke),
                "stroke-width" => style.stroke_width.to_string(),
                _ => String::new(),
            }
        };
        let mut warnings = Vec::new();
        let mut tracks =
            property_keyframes(&specs, &self.sheet, &base, self.span.duration, &mut |w| {
                warnings.push(w)
            });
        let from_css: Vec<String> = tracks.keys().cloned().collect();
        // SMIL animations (several of one attribute merged by when each
        // began); CSS animations win.
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for i in smil {
            self.smil_used.insert(i);
            let a = &self.smil[i];
            let key = match a.kind {
                smil::Kind::Transform(_) => format!("smil-transform:{i}"),
                smil::Kind::Motion { .. } => format!("smil-motion:{i}"),
                _ => a.attribute.clone(),
            };
            if from_css.contains(&key) {
                continue;
            }
            match groups.iter_mut().find(|g| g.0 == key) {
                Some(g) => g.1.push(i),
                None => groups.push((key, vec![i])),
            }
        }
        for (key, members) in groups {
            let first = &self.smil[members[0]];
            let start = match first.kind {
                smil::Kind::Transform(_) | smil::Kind::Motion { .. } => first.identity(),
                _ => base(&first.attribute),
            };
            let anims: Vec<(&Smil, &[f64], &[f64])> = members
                .iter()
                .map(|&i| {
                    (
                        &self.smil[i],
                        self.smil_timing.instances[i].as_slice(),
                        self.smil_timing.lengths[i].as_slice(),
                    )
                })
                .collect();
            let keys = smil::keyframes(
                &anims,
                self.span.duration,
                self.smil_timing.cyclic,
                &start,
                &mut |w| warnings.push(w),
            );
            tracks.insert(key, keys);
        }
        for w in warnings {
            self.warn(w);
        }
        for name in tracks.keys() {
            if !ANIMATABLE.contains(&name.as_str()) && !name.starts_with("smil-") {
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
        // Figma Motion writes an element's position twice: as its `transform`
        // attribute and as CSS (`translate`, a motion path), meaning the CSS
        // to replace the attribute (as Safari draws it, and as the design's
        // guides line up). The attribute goes when CSS positions the element.
        let positioned = ["translate", "rotate", "scale", "offset-path"]
            .iter()
            .any(|p| declared.contains_key(*p) || animations.contains_key(*p));
        let from_attribute =
            declared.get("transform").map(String::as_str) == node.attribute("transform");
        let declared = &if positioned && from_attribute && !animations.contains_key("transform") {
            let mut d = declared.clone();
            d.remove("transform");
            d
        } else {
            declared.clone()
        };
        let visibility = animations
            .get("visibility")
            .map(|keys| visibility_track(keys));
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
        let effects = self.effects(
            declared.get("filter").map(String::as_str),
            animations.get("filter"),
        );
        let clip = match declared.get("clip-path").map(|v| v.trim()) {
            Some(v) if v != "none" => self.clip(v, bounds(&content)),
            _ => None,
        };
        let mask = match declared.get("mask").map(|v| v.trim()) {
            Some(v) if v != "none" => self.mask(v, bounds(&content))?,
            _ => None,
        };
        let trivial = transform.is_empty()
            && visibility.is_none()
            && opacity == Animated::Static(1.0)
            && blend == BlendMode::Normal
            && clip.is_none()
            && mask.is_none()
            && effects.is_empty();
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
        group.effects.extend(effects);
        if let Some(visible) = visibility {
            // Inside the element's own opacity: the two multiply.
            group.children = vec![Node::Group(Group {
                opacity: visible,
                children: std::mem::take(&mut group.children),
                ..Group::new()
            })];
        }
        Ok(Node::Group(group))
    }

    /// The effects of the `filter` property: still, or animated through
    /// `keys` (each keyframe's functions alike).
    fn effects(
        &mut self,
        value: Option<&str>,
        keys: Option<&Vec<Keyframe<String>>>,
    ) -> Vec<Effect> {
        if let Some(keys) = keys {
            let mut parsed = Vec::with_capacity(keys.len());
            for k in keys.iter().filter(|k| k.time.is_finite()) {
                match filter::functions(&k.value) {
                    Ok(f) => parsed.push(Keyframe::new(k.time, f, k.easing.clone())),
                    Err(what) => {
                        self.warn(format!(
                            "animation of `filter` holds {what} and is left out"
                        ));
                        parsed.clear();
                        break;
                    }
                }
            }
            if !parsed.is_empty() {
                match filter::animated(&parsed) {
                    Some(effects) => return effects,
                    None => self.warn(
                        "animation of `filter` between different functions (or of url()) is left out",
                    ),
                }
            }
        }
        let Some(value) = value.map(str::trim).filter(|v| *v != "none") else {
            return Vec::new();
        };
        let functions = match filter::functions(value) {
            Ok(f) => f,
            Err(what) => {
                self.warn(format!("filter `{value}` is left out: it holds {what}"));
                return Vec::new();
            }
        };
        let mut effects = Vec::new();
        for function in functions {
            match function {
                filter::Function::Blur(sigma) => effects.push(Effect::Blur(Blur {
                    radius: Animated::Static(sigma.max(0.0)),
                })),
                filter::Function::DropShadow {
                    dx,
                    dy,
                    sigma,
                    color,
                } => effects.push(Effect::DropShadow(DropShadow {
                    color: Animated::Static(color),
                    offset: AnimatedPoint::fixed(dx, dy),
                    blur: Animated::Static(sigma),
                    knockout: false,
                    hard: false,
                })),
                filter::Function::Color(f, amount) => {
                    filter::push_color(&mut effects, f.op(Animated::Static(amount)))
                }
                filter::Function::Url(id) => {
                    let node = self
                        .ids
                        .get(id.as_str())
                        .copied()
                        .filter(|n| n.has_tag_name("filter"));
                    match node.map(filter::element) {
                        Some(Ok(list)) => effects.extend(list),
                        Some(Err(why)) => {
                            self.warn(format!("filter `#{id}` is left out: it holds {why}"))
                        }
                        None => {
                            self.warn(format!("filter `#{id}` is not a <filter> and is left out"))
                        }
                    }
                }
            }
        }
        effects
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
        for k in keys.iter().filter(|k| k.time.is_finite()) {
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
        // SMIL: motion outside the transform attribute, transforms after it
        // (a `replace` one instead of it).
        let mut smil_keys: Vec<(usize, bool)> = animations
            .keys()
            .filter_map(|k| {
                k.strip_prefix("smil-transform:")
                    .map(|i| (i, false))
                    .or_else(|| k.strip_prefix("smil-motion:").map(|i| (i, true)))
                    .and_then(|(i, motion)| i.parse().ok().map(|i| (i, motion)))
            })
            .collect();
        smil_keys.sort_unstable();
        for &(i, motion) in &smil_keys {
            if !motion {
                continue;
            }
            let (path, rotate) = match &self.smil[i].kind {
                smil::Kind::Motion { path, rotate, .. } => (path.clone(), *rotate),
                _ => continue,
            };
            let Some(keys) = animations.get(&format!("smil-motion:{i}")) else {
                continue;
            };
            let progress = self.track(keys, "animateMotion", number, 0.0);
            ops.push(TransformOp::Motion(MotionPath {
                path,
                progress,
                rotate,
            }));
        }
        let replaced = !animations.contains_key("transform")
            && smil_keys
                .iter()
                .any(|&(i, motion)| !motion && !self.smil[i].additive);
        // transform
        let base_list = declared
            .get("transform")
            .filter(|_| !replaced)
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
        for &(i, motion) in &smil_keys {
            if motion {
                continue;
            }
            let Some(keys) = animations.get(&format!("smil-transform:{i}")) else {
                continue;
            };
            let lists: Option<Vec<Keyframe<Vec<TransformFn>>>> = keys
                .iter()
                .map(|k| {
                    transform_list(&k.value, box_size)
                        .map(|l| Keyframe::new(k.time, l, k.easing.clone()))
                })
                .collect();
            match lists {
                Some(lists) if !lists.is_empty() => ops.extend(animated_list_ops(&lists)),
                _ => self.warn("an <animateTransform> with values it cannot read is left out"),
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
        let moving = [
            "x", "y", "width", "height", "rx", "ry", "cx", "cy", "r", "x1", "y1", "x2", "y2",
            "points",
        ]
        .iter()
        .any(|a| animations.contains_key(*a));
        if moving {
            return self.moving_geometry(node, animations);
        }
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

    /// A length attribute: its keyframes when animated, else its value
    /// (`default` without one).
    fn length_track(
        &mut self,
        node: XmlNode<'a, 'input>,
        animations: &HashMap<String, Vec<Keyframe<String>>>,
        name: &str,
        reference: f64,
        default: f64,
    ) -> Animated<f64> {
        let fixed = node
            .attribute(name)
            .and_then(|v| length(v, reference))
            .unwrap_or(default);
        match animations.get(name) {
            Some(keys) => self.track(keys, name, |v| length(v, reference), fixed),
            None => Animated::Static(fixed),
        }
    }

    /// The outline of a shape element whose geometry attributes animate
    /// (SMIL `<animate>` of `width`, `r`, `points`, ...).
    fn moving_geometry(
        &mut self,
        node: XmlNode<'a, 'input>,
        animations: &HashMap<String, Vec<Keyframe<String>>>,
    ) -> Option<Geometry> {
        let (vw, vh) = (self.view.width, self.view.height);
        let diag = (vw * vw + vh * vh).sqrt() / 2f64.sqrt();
        let mut len = |name: &str, reference: f64, default: f64| {
            self.length_track(node, animations, name, reference, default)
        };
        let point = |x: &Animated<f64>, y: &Animated<f64>| AnimatedPoint {
            x: x.clone(),
            y: y.clone(),
        };
        match node.tag_name().name() {
            "rect" => {
                let (x, y) = (len("x", vw, 0.0), len("y", vh, 0.0));
                let (w, h) = (len("width", vw, 0.0), len("height", vh, 0.0));
                let has = |n: &str| node.attribute(n).is_some() || animations.contains_key(n);
                let rx = len("rx", vw, 0.0);
                let ry = len("ry", vh, 0.0);
                let (rx, ry) = match (has("rx"), has("ry")) {
                    (true, false) => (rx.clone(), rx),
                    (false, true) => (ry.clone(), ry),
                    _ => (rx, ry),
                };
                Some(Geometry::Rect {
                    center: AnimatedPoint {
                        x: combine(&[&x, &w], |v| v[0] + v[1] / 2.0),
                        y: combine(&[&y, &h], |v| v[0] + v[1] / 2.0),
                    },
                    size: point(&w, &h),
                    radius: point(&rx, &ry),
                    direction: Direction::Clockwise,
                    start: RectStart::TopLeft,
                })
            }
            "circle" | "ellipse" => {
                let (cx, cy) = (len("cx", vw, 0.0), len("cy", vh, 0.0));
                let (rx, ry) = if node.tag_name().name() == "circle" {
                    let r = len("r", diag, 0.0);
                    (r.clone(), r)
                } else {
                    (len("rx", vw, 0.0), len("ry", vh, 0.0))
                };
                Some(Geometry::Ellipse {
                    center: point(&cx, &cy),
                    radius: point(&rx, &ry),
                    direction: Direction::Clockwise,
                    start: EllipseStart::Right,
                })
            }
            "line" => {
                let parts = [
                    len("x1", vw, 0.0),
                    len("y1", vh, 0.0),
                    len("x2", vw, 0.0),
                    len("y2", vh, 0.0),
                ];
                let refs: Vec<&Animated<f64>> = parts.iter().collect();
                Some(Geometry::Path {
                    path: combine(&refs, |v| {
                        let mut p = Path::new();
                        p.move_to(Point::new(v[0], v[1]));
                        p.line_to(Point::new(v[2], v[3]));
                        p
                    }),
                })
            }
            "polyline" | "polygon" => {
                let closed = node.tag_name().name() == "polygon";
                let parse = move |v: &str| -> Option<Path> {
                    let mut p = Path::new();
                    for (i, (x, y)) in svgtypes::PointsParser::from(v).enumerate() {
                        if i == 0 {
                            p.move_to(Point::new(x, y));
                        } else {
                            p.line_to(Point::new(x, y));
                        }
                    }
                    if closed && !p.is_empty() {
                        p.close();
                    }
                    Some(p)
                };
                let base = node.attribute("points").and_then(parse).unwrap_or_default();
                let path = match animations.get("points") {
                    Some(keys) => self.track(keys, "points", parse, base),
                    None => Animated::Static(base),
                };
                Some(Geometry::Path { path })
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
            if let Some(read) = self.patterns.get(&node.id()) {
                return Some(read.clone());
            }
            let paint = self.pattern(node);
            self.patterns.insert(node.id(), paint.clone());
            return Some(paint);
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

    /// The `<pattern>` `node` (with what its `href` chain gives it) as a
    /// paint, or `None` when it draws nothing.
    fn pattern(&mut self, node: XmlNode<'a, 'input>) -> Option<Paint> {
        if self.pattern_stack.contains(&node.id()) || self.pattern_stack.len() >= 8 {
            self.warn("a pattern that fills with itself is left out");
            return None;
        }
        let mut chain = vec![node];
        while let Some(next) = href(*chain.last()?).and_then(|i| self.ids.get(i).copied()) {
            if chain.len() > 16
                || chain.iter().any(|n| n.id() == next.id())
                || !next.has_tag_name("pattern")
            {
                break;
            }
            chain.push(next);
        }
        let attr = |name: &str| chain.iter().find_map(|n| n.attribute(name));
        let units = match attr("patternUnits") {
            Some("userSpaceOnUse") => GradientUnits::UserSpace,
            _ => GradientUnits::BoundingBox,
        };
        let content_units = match attr("patternContentUnits") {
            Some("objectBoundingBox") => GradientUnits::BoundingBox,
            _ => GradientUnits::UserSpace,
        };
        let (vw, vh) = (self.view.width, self.view.height);
        let coord = |name: &str, reference: f64| {
            attr(name)
                .and_then(|v| match units {
                    GradientUnits::BoundingBox => fraction(v),
                    GradientUnits::UserSpace => length(v, reference),
                })
                .unwrap_or(0.0)
        };
        let tile = Rect::new(
            coord("x", vw),
            coord("y", vh),
            coord("width", vw),
            coord("height", vh),
        );
        if !(tile.width > 0.0 && tile.height > 0.0) {
            return None;
        }
        let transform = attr("patternTransform")
            .and_then(|t| transform_list(t, (0.0, 0.0)))
            .map(|l| list_matrix(&l))
            .unwrap_or_default();
        let view_box = attr("viewBox")
            .and_then(|v| v.parse::<svgtypes::ViewBox>().ok())
            .map(|vb| {
                let aspect: svgtypes::AspectRatio = attr("preserveAspectRatio")
                    .and_then(|a| a.parse().ok())
                    .unwrap_or_default();
                ViewBox {
                    rect: Rect::new(vb.x, vb.y, vb.w, vb.h),
                    align: align(aspect.align),
                    slice: aspect.slice,
                }
            });
        let holder = chain
            .iter()
            .copied()
            .find(|n| n.children().any(|c| c.is_element()))?;
        let style = self.style_of(holder);
        self.pattern_stack.push(node.id());
        let mut content = Group::new();
        for child in holder.children() {
            match self.element(child, &style) {
                Ok(Some(n)) => content.children.push(n),
                Ok(None) => {}
                Err(e) => {
                    self.warn(format!("a pattern's content is left out: {e}"));
                    content.children.clear();
                    break;
                }
            }
        }
        self.pattern_stack.pop();
        if content.children.is_empty() {
            return None;
        }
        Some(Paint::Pattern(Box::new(Pattern {
            tile,
            units,
            content_units,
            view_box,
            transform,
            content,
        })))
    }

    /// The style `node`'s children inherit: its ancestors' and its own, from
    /// the root down.
    fn style_of(&mut self, node: XmlNode<'a, 'input>) -> Style {
        let mut chain: Vec<XmlNode<'a, 'input>> =
            node.ancestors().filter(|n| n.is_element()).collect();
        chain.reverse();
        let mut style = Style::default();
        for n in chain {
            let declared = self.cascade(n);
            style = self.inherit(&style, &declared);
        }
        style
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

/// CSS `visibility` keyframes as an opacity of 0 or 1. Between a visible
/// and a hidden keyframe the element is visible (CSS interpolates
/// `visibility` so), so it shows from the start of a segment that ends
/// visible and until the end of one that starts visible.
fn visibility_track(keys: &[Keyframe<String>]) -> Animated<f64> {
    let shown = |v: &str| if v.trim() == "visible" { 1.0 } else { 0.0 };
    let frames = keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let (now, next) = (
                shown(&k.value),
                keys.get(i + 1).map_or(0.0, |n| shown(&n.value)),
            );
            let easing = if now == 0.0 && next == 1.0 {
                // Hidden at the keyframe itself, visible right after.
                dxn_vector_animation::Easing::LinearPoints {
                    points: vec![(0.0, 0.0), (1e-6, 1.0), (1.0, 1.0)],
                }
            } else {
                dxn_vector_animation::Easing::Hold
            };
            Keyframe::new(k.time, now, easing)
        })
        .collect();
    Animated::from_keyframes(frames, 1.0)
}

/// `f` of several tracks as one: keyframe by keyframe when they share
/// their keyframe times and easings (or all but one are constant), else
/// sampled finely between every keyframe time of any.
fn combine<T: Interpolate + PartialEq>(
    tracks: &[&Animated<f64>],
    f: impl Fn(&[f64]) -> T,
) -> Animated<T> {
    const SAMPLES: usize = 12;
    let at = |t: f64| f(&tracks.iter().map(|a| a.sample(t)).collect::<Vec<_>>());
    let moving: Vec<&[Keyframe<f64>]> = tracks
        .iter()
        .map(|a| a.keyframes())
        .filter(|k| !k.is_empty())
        .collect();
    let Some(first) = moving.first() else {
        return Animated::Static(at(0.0));
    };
    let same = moving.iter().all(|k| {
        k.len() == first.len()
            && k.iter()
                .zip(first.iter())
                .all(|(a, b)| a.time == b.time && a.easing == b.easing)
    });
    if same {
        // Keyframe by keyframe: two at one time (a jump) keep both values.
        return Animated::from_keyframes(
            first
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    let values: Vec<f64> = tracks
                        .iter()
                        .map(|a| match a.keyframes() {
                            [] => a.sample(0.0),
                            keys => keys[i].value,
                        })
                        .collect();
                    Keyframe::new(k.time, f(&values), k.easing.clone())
                })
                .collect(),
            at(0.0),
        );
    }
    // Where a track jumps (two keyframes at one time), what shows just
    // before.
    let jumps: Vec<f64> = moving
        .iter()
        .flat_map(|k| {
            k.windows(2)
                .filter(|w| w[0].time == w[1].time)
                .map(|w| w[0].time)
        })
        .collect();
    let mut times: Vec<f64> = moving
        .iter()
        .flat_map(|k| k.iter().map(|f| f.time))
        .collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    let mut frames = Vec::new();
    for w in times.windows(2) {
        if jumps.contains(&w[0]) {
            frames.push(Keyframe::new(
                w[0],
                at(w[0] - 1e-9),
                dxn_vector_animation::Easing::Hold,
            ));
        }
        for i in 0..SAMPLES {
            let t = w[0] + (w[1] - w[0]) * i as f64 / SAMPLES as f64;
            frames.push(Keyframe::new(
                t,
                at(t),
                dxn_vector_animation::Easing::Linear,
            ));
        }
    }
    if let Some(&last) = times.last() {
        if jumps.contains(&last) {
            frames.push(Keyframe::new(
                last,
                at(last - 1e-9),
                dxn_vector_animation::Easing::Hold,
            ));
        }
        frames.push(Keyframe::new(
            last,
            at(last),
            dxn_vector_animation::Easing::Linear,
        ));
    }
    Animated::from_keyframes(frames, at(0.0))
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

pub(crate) fn path_data(d: &str) -> Path {
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

/// Where an alignment puts a view box in the room left along each axis;
/// `None` stretches it.
fn align(a: svgtypes::Align) -> Option<(f64, f64)> {
    use svgtypes::Align::*;
    Some(match a {
        None => return Option::None,
        XMinYMin => (0.0, 0.0),
        XMidYMin => (0.5, 0.0),
        XMaxYMin => (1.0, 0.0),
        XMinYMid => (0.0, 0.5),
        XMidYMid => (0.5, 0.5),
        XMaxYMid => (1.0, 0.5),
        XMinYMax => (0.0, 1.0),
        XMidYMax => (0.5, 1.0),
        XMaxYMax => (1.0, 1.0),
    })
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
