//! After Effects' objects read from the document (compositions, layers,
//! transforms, effects, properties and their keyframes) and the functions
//! that sample a property: loops, wiggles, velocity, smoothing.

use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    rc::Rc,
};

use serde_json::Value;

use super::{
    engine::{
        Comp, Engine, Fail, PROPERTY_STEPS, PropInfo, R, Run, Src, TOTAL_STEPS, bound, error,
        unsupported,
    },
    keys::Keys,
    syntax::{self, Stmt, short},
    value::{self, Host, LayerId, Val, to_number},
};
use crate::props::{has_expression, num};

/// How many expressions read one another in a chain.
const MAX_CHAIN: usize = 16;
/// How many values of expressions read by other expressions are kept.
const MAX_CACHE: usize = 1 << 20;
/// How deep the search for expressions goes into a layer's JSON.
const MAX_WALK: usize = 64;

/// A property's expression evaluated at every frame its layer shows.
pub(super) struct Baked {
    /// Frames of the layer's own time, and the values there.
    pub(super) frames: Vec<(f64, Vec<f64>)>,
    /// Whether the values are arrays (not single numbers).
    pub(super) array: bool,
}

impl<'a> Engine<'a> {
    /// The engine for document `root`; `None` when it has no frame rate or
    /// layers (the conversion reports those).
    pub(super) fn new(root: &'a Value) -> Option<Engine<'a>> {
        let field = |v: &Value, k: &str| v.get(k).and_then(num);
        let fr = field(root, "fr").filter(|f| *f > 0.0)?;
        let layers = root.get("layers")?.as_array()?;
        let width = field(root, "w").unwrap_or(0.0);
        let height = field(root, "h").unwrap_or(0.0);
        let ip = field(root, "ip").unwrap_or(0.0);
        let op = field(root, "op").unwrap_or(ip);
        let name = |v: &'a Value, k: &str| v.get(k).and_then(Value::as_str);
        let mut comps = vec![Comp {
            layers,
            pointer: String::new(),
            name: name(root, "nm").unwrap_or(""),
            width,
            height,
        }];
        let assets = root.get("assets").and_then(Value::as_array);
        for (i, asset) in assets.into_iter().flatten().enumerate() {
            let Some(layers) = asset.get("layers").and_then(Value::as_array) else {
                continue;
            };
            comps.push(Comp {
                layers,
                pointer: format!("/assets/{i}"),
                name: name(asset, "nm")
                    .or_else(|| name(asset, "id"))
                    .unwrap_or(""),
                width: field(asset, "w").unwrap_or(width),
                height: field(asset, "h").unwrap_or(height),
            });
        }
        Some(Engine {
            fr,
            root_span: (ip.min(op), ip.max(op)),
            comps,
            props: Vec::new(),
            by_pointer: HashMap::new(),
            cache: HashMap::new(),
            failed: HashSet::new(),
            stack: Vec::new(),
            steps: 0,
            limit: 0,
            depth: 0,
        })
    }

    /// The properties of every layer that hold expressions.
    pub(super) fn expressions(&mut self) -> Vec<usize> {
        let mut found = Vec::new();
        for comp in 0..self.comps.len() {
            let layers = self.comps[comp].layers;
            for (index, layer) in layers.iter().enumerate() {
                let mut pointer = format!("{}/layers/{index}", self.comps[comp].pointer);
                let skip = text_expression_selectors(layer, &pointer);
                let mut list = Vec::new();
                walk(layer, &mut pointer, 0, &skip, &mut list);
                for (pointer, p) in list {
                    found.push(self.intern(Src::Json(p), LayerId { comp, index }, pointer));
                }
            }
        }
        found
    }

    /// The id of the property at `pointer` (JSON pointer), added the first
    /// time it is asked for.
    pub(super) fn intern(&mut self, src: Src<'a>, layer: LayerId, pointer: String) -> usize {
        if let Some(&id) = self.by_pointer.get(&pointer) {
            return id;
        }
        let id = self.props.len();
        self.props.push(PropInfo {
            src,
            layer,
            seed: value::hash_str(&pointer),
            pointer: pointer.clone(),
            keys: None,
            program: None,
        });
        self.by_pointer.insert(pointer, id);
        id
    }

    /// The JSON of property `id` (`None` for one the file leaves out).
    pub(super) fn json(&self, id: usize) -> Option<&'a Value> {
        match self.props[id].src {
            Src::Json(v) | Src::Split(v) => Some(v),
            Src::Const(_) => None,
        }
    }

    pub(super) fn layer(&self, l: LayerId) -> &'a Value {
        let layers: &'a [Value] = self.comps[l.comp].layers;
        &layers[l.index]
    }

    fn layer_pointer(&self, l: LayerId) -> String {
        format!("{}/layers/{}", self.comps[l.comp].pointer, l.index)
    }

    /// A layer's start time (frames) and stretch.
    fn timing(&self, l: LayerId) -> (f64, f64) {
        let layer = self.layer(l);
        let st = layer.get("st").and_then(num).unwrap_or(0.0);
        let sr = layer
            .get("sr")
            .and_then(num)
            .filter(|s| *s != 0.0)
            .unwrap_or(1.0);
        (st, sr)
    }

    /// The frame of property `id`'s layer at composition time `time`
    /// (seconds): the time its keyframes are at.
    pub(super) fn local_frame(&self, id: usize, time: f64) -> f64 {
        let (st, sr) = self.timing(self.props[id].layer);
        (time * self.fr - st) / sr
    }

    /// The composition time (seconds) of frame `frame` of property `id`'s
    /// layer.
    fn comp_time(&self, id: usize, frame: f64) -> f64 {
        let (st, sr) = self.timing(self.props[id].layer);
        (frame * sr + st) / self.fr
    }

    /// The in (`ip`) or out (`op`) point of property `id`'s layer, in the
    /// frames of the layer's time.
    fn layer_frame(&self, id: usize, key: &str) -> f64 {
        let l = self.props[id].layer;
        let (st, sr) = self.timing(l);
        (self.layer(l).get(key).and_then(num).unwrap_or(0.0) - st) / sr
    }

    pub(super) fn keys(&mut self, id: usize) -> R<Rc<Keys>> {
        if let Some(k) = &self.props[id].keys {
            return k.clone();
        }
        let keys = match &self.props[id].src {
            Src::Json(v) => Keys::read(v).map(Rc::new).map_err(Fail::Kind),
            Src::Const(v) => Ok(Rc::new(Keys::constant(v.clone()))),
            Src::Split(_) => Err(error("the keyframes of a split position")),
        };
        self.props[id].keys = Some(keys.clone());
        keys
    }

    fn program(&mut self, id: usize) -> R<Rc<Vec<Stmt>>> {
        if let Some(p) = &self.props[id].program {
            return p.clone();
        }
        let source = match self.props[id].src {
            Src::Json(v) => v.get("x").and_then(Value::as_str),
            _ => None,
        };
        let program = match source {
            Some(source) => syntax::parse(source).map(Rc::new).map_err(Fail::Syntax),
            None => Err(error("no expression")),
        };
        self.props[id].program = Some(program.clone());
        program
    }

    /// Whether property `id` has an expression that has not failed.
    fn has_expression(&self, id: usize) -> bool {
        matches!(self.props[id].src, Src::Json(v) if has_expression(v))
            && !self.failed.contains(&id)
    }

    /// Property `id`'s value at `time` (composition seconds) from its
    /// keyframes, before any expression.
    pub(super) fn pre_value(&mut self, id: usize, time: f64) -> R<Val> {
        if matches!(self.props[id].src, Src::Split(_)) {
            return self.post_value(id, time);
        }
        let keys = self.keys(id)?;
        let f = self.local_frame(id, time);
        Ok(Val::numbers(&keys.sample(f), keys.array()))
    }

    /// Property `id`'s value at `time`, its expression evaluated (by its
    /// keyframes when the expression fails, or when it is the one running).
    fn post_value(&mut self, id: usize, time: f64) -> R<Val> {
        match &self.props[id].src {
            Src::Const(v) => return Ok(Val::numbers(v, v.len() > 1)),
            Src::Split(p) => {
                let p: &'a Value = p;
                let (layer, base) = (self.props[id].layer, self.props[id].pointer.clone());
                let mut out = Vec::new();
                for axis in ["x", "y", "z"] {
                    let Some(part) = p.get(axis) else { continue };
                    let part = self.intern(Src::Json(part), layer, format!("{base}/{axis}"));
                    let v = self.post_value(part, time)?;
                    out.push(
                        value::to_numbers(&v).map_or(0.0, |n| n.first().copied().unwrap_or(0.0)),
                    );
                }
                return Ok(Val::numbers(&out, true));
            }
            Src::Json(_) => {}
        }
        if !self.has_expression(id) || self.stack.last() == Some(&id) {
            return self.pre_value(id, time);
        }
        let key = (id, time.to_bits());
        if let Some(v) = self.cache.get(&key) {
            return Ok(v.clone());
        }
        match self.evaluate(id, time) {
            Ok(v) => {
                if self.cache.len() < MAX_CACHE {
                    self.cache.insert(key, v.clone());
                }
                Ok(v)
            }
            Err(fail @ (Fail::Budget | Fail::Cycle)) => Err(fail),
            Err(_) => {
                self.failed.insert(id);
                self.pre_value(id, time)
            }
        }
    }

    /// Runs property `id`'s expression at `time`: its value as numbers of
    /// the property's dimensions.
    fn evaluate(&mut self, id: usize, time: f64) -> R<Val> {
        if self.stack.contains(&id) {
            return Err(Fail::Cycle);
        }
        if self.stack.len() >= MAX_CHAIN {
            return Err(error("expressions reading expressions too deeply"));
        }
        let program = self.program(id)?;
        let keys = self.keys(id)?;
        let before = self.pre_value(id, time)?;
        self.stack.push(id);
        let result = self.run_program(id, time, before.clone(), &program);
        self.stack.pop();
        settle(&result?, &before, &keys)
    }

    /// Property `id`'s value at the run's time.
    pub(super) fn prop_now(&mut self, run: &mut Run, id: usize) -> R<Val> {
        if id == run.prop {
            return Ok(run.value.clone());
        }
        self.post_value(id, run.time)
    }

    /// Property `id`'s value at `t`: the running property's own before its
    /// expression, another's after.
    fn prop_at(&mut self, run: &Run, id: usize, t: f64) -> R<Val> {
        if id == run.prop {
            self.pre_value(id, t)
        } else {
            self.post_value(id, t)
        }
    }

    /// Property `id`'s value at `t` before its expression (the running
    /// value when it is the running property at the run's time).
    fn before(&mut self, run: &Run, id: usize, t: f64) -> R<Val> {
        if id == run.prop && t == run.time {
            return Ok(run.value.clone());
        }
        self.pre_value(id, t)
    }

    pub(super) fn key_value(&mut self, id: usize, k: usize) -> R<Val> {
        let keys = self.keys(id)?;
        let frame = keys
            .frames
            .get(k)
            .ok_or_else(|| error("a keyframe past the last"))?;
        Ok(Val::numbers(&frame.v, keys.array()))
    }

    pub(super) fn host_member(&mut self, run: &mut Run, h: Host, name: &str) -> R<Val> {
        match h {
            Host::Math => Ok(Engine::math_member(name)),
            Host::Comp(c) => self.comp_member(c, name),
            Host::Layer(l) => self.layer_member(run, l, name),
            Host::Transform(l) => self
                .transform_prop(run, l, name)?
                .ok_or_else(|| unsupported(name)),
            Host::Effects(l) => match name {
                "numProperties" => Ok(Val::Num(self.effects_of(l).len() as f64)),
                _ => Err(unsupported(name)),
            },
            Host::Effect(l, e) => self.effect_member(l, e, name),
            Host::Prop(id) => self.prop_member(run, id, name),
            Host::Key(id, k) => match name {
                "time" => {
                    let keys = self.keys(id)?;
                    let t = keys.frames.get(k).map_or(f64::NAN, |f| f.t);
                    Ok(Val::Num(self.comp_time(id, t)))
                }
                "value" => self.key_value(id, k),
                "index" => Ok(Val::Num(k as f64 + 1.0)),
                _ => {
                    let v = self.key_value(id, k)?;
                    self.member(run, v, name)
                }
            },
        }
    }

    /// An object called as a function: a layer's property group, an
    /// effect, an effect's property, a transform property.
    pub(super) fn call_host(&mut self, run: &mut Run, h: Host, args: Vec<Val>) -> R<Val> {
        let args = self.plain_all(run, args)?;
        let key = args.into_iter().next().unwrap_or(Val::Undef);
        match h {
            Host::Layer(l) => self.layer_call(run, l, &key),
            Host::Effects(l) => self.effect(l, &key),
            Host::Effect(l, e) => self.effect_param(l, e, &key),
            Host::Transform(l) => match &key {
                Val::Str(s) => self
                    .transform_prop(run, l, s)?
                    .ok_or_else(|| unsupported(s)),
                _ => Err(Fail::Unsupported("transform properties by number".into())),
            },
            _ => Err(error("an object called as a function")),
        }
    }

    pub(super) fn layer_member(&mut self, run: &mut Run, l: LayerId, name: &str) -> R<Val> {
        let layer = self.layer(l);
        let get = |k: &str| layer.get(k).and_then(num);
        let fr = self.fr;
        Ok(match name {
            "transform" => Val::Host(Host::Transform(l)),
            "position" | "scale" | "rotation" | "opacity" | "anchorPoint" | "xPosition"
            | "yPosition" | "zPosition" | "skew" | "skewAxis" => {
                return self
                    .transform_prop(run, l, name)?
                    .ok_or_else(|| unsupported(name));
            }
            "index" => Val::Num(get("ind").unwrap_or(l.index as f64 + 1.0)),
            "name" => Val::str(layer.get("nm").and_then(Value::as_str).unwrap_or(""))?,
            "inPoint" => Val::Num(get("ip").unwrap_or(0.0) / fr),
            "outPoint" => Val::Num(get("op").unwrap_or(0.0) / fr),
            "startTime" => Val::Num(get("st").unwrap_or(0.0) / fr),
            "stretch" => Val::Num(self.timing(l).1 * 100.0),
            "hasParent" => Val::Bool(self.parent(l).is_some()),
            "parent" => match self.parent(l) {
                Some(p) => Val::Host(Host::Layer(p)),
                None => return Err(error("the parent of a layer without one")),
            },
            "width" => Val::Num(self.layer_size(l, true)),
            "height" => Val::Num(self.layer_size(l, false)),
            "timeRemap" => match layer.get("tm") {
                Some(tm) => {
                    let pointer = format!("{}/tm", self.layer_pointer(l));
                    Val::Host(Host::Prop(self.intern(Src::Json(tm), l, pointer)))
                }
                None => return Err(error("the time remapping of a layer without it")),
            },
            "enabled" => Val::Bool(layer.get("hd").and_then(Value::as_bool) != Some(true)),
            "active" => {
                let f = run.time * fr;
                Val::Bool(
                    f >= get("ip").unwrap_or(f64::NEG_INFINITY)
                        && f < get("op").unwrap_or(f64::INFINITY),
                )
            }
            "audioActive" => Val::Bool(false),
            "effect" => bound(Val::Host(Host::Layer(l)), "effect"),
            _ => return Err(unsupported(name)),
        })
    }

    fn parent(&self, l: LayerId) -> Option<LayerId> {
        let p = self.layer(l).get("parent").and_then(num)?;
        self.comps[l.comp]
            .layers
            .iter()
            .position(|x| x.get("ind").and_then(num) == Some(p))
            .map(|index| LayerId {
                comp: l.comp,
                index,
            })
    }

    fn layer_size(&self, l: LayerId, width: bool) -> f64 {
        let layer = self.layer(l);
        let get = |k: &str| layer.get(k).and_then(num);
        let size = match layer.get("ty").and_then(num) {
            Some(0.0) => get(if width { "w" } else { "h" }),
            Some(1.0) => get(if width { "sw" } else { "sh" }),
            _ => None,
        };
        let comp = &self.comps[l.comp];
        size.unwrap_or(if width { comp.width } else { comp.height })
    }

    fn layer_call(&mut self, run: &mut Run, l: LayerId, key: &Val) -> R<Val> {
        let Val::Str(s) = key else {
            return Err(Fail::Unsupported("layer properties by number".into()));
        };
        match &**s {
            "ADBE Transform Group" | "Transform" => Ok(Val::Host(Host::Transform(l))),
            "ADBE Effect Parade" | "Effects" => Ok(Val::Host(Host::Effects(l))),
            "ADBE Time Remapping" | "Time Remap" => self.layer_member(run, l, "timeRemap"),
            "ADBE Root Vectors Group" | "Contents" => Err(unsupported("content")),
            "ADBE Text Properties" | "Text" => Err(unsupported("text")),
            "ADBE Mask Parade" | "Masks" => Err(unsupported("mask")),
            "ADBE Marker" | "Marker" => Err(unsupported("marker")),
            other => Err(unsupported(other)),
        }
    }

    fn comp_member(&self, c: usize, name: &str) -> R<Val> {
        let comp = &self.comps[c];
        Ok(match name {
            "layer" => bound(Val::Host(Host::Comp(c)), "layer"),
            "numLayers" => Val::Num(comp.layers.len() as f64),
            "width" => Val::Num(comp.width),
            "height" => Val::Num(comp.height),
            "frameDuration" => Val::Num(1.0 / self.fr),
            "duration" => Val::Num((self.root_span.1 - self.root_span.0) / self.fr),
            "name" => Val::str(comp.name)?,
            "displayStartTime" => Val::Num(0.0),
            "pixelAspect" => Val::Num(1.0),
            _ => return Err(unsupported(name)),
        })
    }

    /// `thisComp.layer(name or index)`.
    pub(super) fn comp_layer(&self, c: usize, args: &[Val]) -> R<Val> {
        if args.len() > 1 {
            return Err(Fail::Unsupported("layers relative to other layers".into()));
        }
        let layers = self.comps[c].layers;
        let key = args.first().unwrap_or(&Val::Undef);
        let found = match key {
            Val::Host(Host::Layer(l)) => return Ok(Val::Host(Host::Layer(*l))),
            Val::Str(s) => layers
                .iter()
                .position(|l| l.get("nm").and_then(Value::as_str) == Some(&**s)),
            Val::Num(n) => layers
                .iter()
                .position(|l| l.get("ind").and_then(num) == Some(*n))
                .or_else(|| ordinal(*n, layers.len())),
            _ => None,
        };
        found
            .map(|index| Val::Host(Host::Layer(LayerId { comp: c, index })))
            .ok_or_else(|| error(format!("no layer {}", describe_key(key))))
    }

    /// `comp(name)`.
    pub(super) fn comp_named(&self, key: &Val) -> R<Val> {
        let Val::Str(s) = key else {
            return Err(error("`comp` of something other than a name"));
        };
        self.comps
            .iter()
            .position(|c| c.name == &**s)
            .map(|i| Val::Host(Host::Comp(i)))
            .ok_or_else(|| error(format!("no composition `{}`", short(s))))
    }

    /// A transform property of layer `l` by its name or match name; `None`
    /// for a name that is not one.
    fn transform_prop(&mut self, run: &mut Run, l: LayerId, name: &str) -> R<Option<Val>> {
        let ks = self.layer(l).get("ks");
        let field = |k: &str| ks.and_then(|ks| ks.get(k));
        let (key, default): (&str, &[f64]) = match name {
            "anchorPoint" | "ADBE Anchor Point" | "Anchor Point" => ("a", &[0.0, 0.0, 0.0]),
            "position" | "ADBE Position" | "Position" => ("p", &[0.0, 0.0, 0.0]),
            "scale" | "ADBE Scale" | "Scale" => ("s", &[100.0, 100.0, 100.0]),
            "rotation" | "zRotation" | "ADBE Rotate Z" | "Rotation" | "Z Rotation" => {
                let key = if field("r").is_none() && field("rz").is_some() {
                    "rz"
                } else {
                    "r"
                };
                (key, &[0.0])
            }
            "opacity" | "ADBE Opacity" | "Opacity" => ("o", &[100.0]),
            "skew" | "ADBE Skew" | "Skew" => ("sk", &[0.0]),
            "skewAxis" | "ADBE Skew Axis" | "Skew Axis" => ("sa", &[0.0]),
            "xPosition" | "ADBE Position_0" | "X Position" => {
                return self.position_part(run, l, 0).map(Some);
            }
            "yPosition" | "ADBE Position_1" | "Y Position" => {
                return self.position_part(run, l, 1).map(Some);
            }
            "zPosition" | "ADBE Position_2" | "Z Position" => {
                return self.position_part(run, l, 2).map(Some);
            }
            "orientation" | "xRotation" | "yRotation" | "ADBE Orientation" | "ADBE Rotate X"
            | "ADBE Rotate Y" | "Orientation" | "X Rotation" | "Y Rotation" => {
                return Err(Fail::Unsupported("3D rotations".into()));
            }
            _ => return Ok(None),
        };
        let pointer = format!("{}/ks/{key}", self.layer_pointer(l));
        let src = match field(key) {
            Some(p) if key == "p" && p.get("s").and_then(Value::as_bool) == Some(true) => {
                Src::Split(p)
            }
            Some(p) => Src::Json(p),
            None => Src::Const(default.to_vec()),
        };
        Ok(Some(Val::Host(Host::Prop(self.intern(src, l, pointer)))))
    }

    /// `xPosition`, `yPosition`, `zPosition`: a split position's own
    /// property, or a component of the position.
    fn position_part(&mut self, run: &mut Run, l: LayerId, axis: usize) -> R<Val> {
        let p = self.layer(l).get("ks").and_then(|ks| ks.get("p"));
        let name = ["x", "y", "z"][axis];
        if let Some(p) = p
            && p.get("s").and_then(Value::as_bool) == Some(true)
        {
            let Some(part) = p.get(name) else {
                return Ok(Val::Num(0.0));
            };
            let pointer = format!("{}/ks/p/{name}", self.layer_pointer(l));
            return Ok(Val::Host(Host::Prop(self.intern(
                Src::Json(part),
                l,
                pointer,
            ))));
        }
        let Some(position) = self.transform_prop(run, l, "position")? else {
            return Ok(Val::Num(0.0));
        };
        let v = self.plain(run, position)?;
        Ok(v.items().get(axis).cloned().unwrap_or(Val::Num(0.0)))
    }

    fn effects_of(&self, l: LayerId) -> &'a [Value] {
        self.layer(l)
            .get("ef")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
    }

    /// `effect(name or index)` of layer `l`.
    pub(super) fn effect(&self, l: LayerId, key: &Val) -> R<Val> {
        let list = self.effects_of(l);
        let found = match key {
            Val::Str(s) => list.iter().position(|e| named(e, s)),
            Val::Num(n) => ordinal(*n, list.len()),
            Val::Host(Host::Effect(_, e)) => Some(*e),
            _ => None,
        };
        found
            .map(|e| Val::Host(Host::Effect(l, e)))
            .ok_or_else(|| error(format!("no effect {}", describe_key(key))))
    }

    /// A property of effect `e` of layer `l`, by name or index: an
    /// expression control's slider, angle, color, point or checkbox, or the
    /// layer a layer control names.
    pub(super) fn effect_param(&mut self, l: LayerId, e: usize, key: &Val) -> R<Val> {
        let params = self
            .effects_of(l)
            .get(e)
            .and_then(|x| x.get("ef"))
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        let found = match key {
            Val::Str(s) => params.iter().position(|p| named(p, s)),
            Val::Num(n) => params
                .iter()
                .position(|p| p.get("ix").and_then(num) == Some(*n))
                .or_else(|| ordinal(*n, params.len())),
            _ => None,
        };
        let Some(j) = found else {
            return Err(error(format!("no effect property {}", describe_key(key))));
        };
        let param = &params[j];
        let Some(v) = param.get("v") else {
            return Err(error("an effect property without a value"));
        };
        if param.get("ty").and_then(num) == Some(10.0) {
            let index = v.get("k").and_then(num).unwrap_or(f64::NAN);
            return self.comp_layer(l.comp, &[Val::Num(index)]);
        }
        let pointer = format!("{}/ef/{e}/ef/{j}/v", self.layer_pointer(l));
        Ok(Val::Host(Host::Prop(self.intern(Src::Json(v), l, pointer))))
    }

    fn effect_member(&mut self, l: LayerId, e: usize, name: &str) -> R<Val> {
        let effect = self.effects_of(l).get(e);
        let field = |k: &str| effect.and_then(|x| x.get(k));
        Ok(match name {
            "param" => bound(Val::Host(Host::Effect(l, e)), "param"),
            "name" => Val::str(field("nm").and_then(Value::as_str).unwrap_or(""))?,
            "active" | "enabled" => Val::Bool(field("en").and_then(num) != Some(0.0)),
            "numProperties" => {
                Val::Num(field("ef").and_then(Value::as_array).map_or(0, Vec::len) as f64)
            }
            _ => return Err(unsupported(name)),
        })
    }

    pub(super) fn prop_member(&mut self, run: &mut Run, id: usize, name: &str) -> R<Val> {
        Ok(match name {
            "value" => return self.prop_now(run, id),
            "velocity" => {
                let t = run.time;
                return self.velocity(run, id, t);
            }
            "speed" => {
                let t = run.time;
                let v = self.velocity(run, id, t)?;
                Val::Num(value::length(&v, None))
            }
            "numKeys" => Val::Num(self.keys(id).map_or(0, |k| k.frames.len()) as f64),
            "name" => {
                let name = self
                    .json(id)
                    .and_then(|v| v.get("nm"))
                    .and_then(Value::as_str);
                Val::str(name.unwrap_or(""))?
            }
            "propertyIndex" | "propertyGroup" | "expression" | "canSetExpression" => {
                return Err(unsupported(name));
            }
            "valueAtTime" | "velocityAtTime" | "speedAtTime" | "key" | "nearestKey" | "loopIn"
            | "loopOut" | "loopInDuration" | "loopOutDuration" | "wiggle" | "temporalWiggle"
            | "smooth" => bound(Val::Host(Host::Prop(id)), name),
            _ => {
                let v = self.prop_now(run, id)?;
                return self.member(run, v, name);
            }
        })
    }

    pub(super) fn prop_method(
        &mut self,
        run: &mut Run,
        id: usize,
        name: &str,
        args: Vec<Val>,
    ) -> R<Val> {
        let args = self.plain_all(run, args)?;
        let time = |i: usize| arg_or(&args, i, run.time);
        match name {
            "valueAtTime" => {
                let t = time(0);
                self.prop_at(run, id, t)
            }
            "velocityAtTime" => {
                let t = time(0);
                self.velocity(run, id, t)
            }
            "speedAtTime" => {
                let t = time(0);
                let v = self.velocity(run, id, t)?;
                Ok(Val::Num(value::length(&v, None)))
            }
            "key" => self.key(id, arg_or(&args, 0, f64::NAN)),
            "nearestKey" => {
                let t = time(0);
                self.nearest_key(id, t)
            }
            "loopIn" | "loopOut" | "loopInDuration" | "loopOutDuration" => {
                self.loop_value(run, id, name, &args)
            }
            "wiggle" => self.wiggle(run, id, &args),
            "temporalWiggle" => self.temporal_wiggle(run, id, &args),
            "smooth" => self.smooth(run, id, &args),
            _ => {
                let v = self.prop_now(run, id)?;
                self.call_method(run, v, name, args, None)
            }
        }
    }

    /// The change per second at `t` (as lottie-web measures it, over the
    /// millisecond before).
    fn velocity(&mut self, run: &Run, id: usize, t: f64) -> R<Val> {
        const DT: f64 = 0.001;
        let a = self.prop_at(run, id, t)?;
        let b = self.prop_at(run, id, t - DT)?;
        Ok(value::div(&value::sub(&a, &b)?, &Val::Num(DT))?)
    }

    /// `key(n)`, from 1.
    fn key(&mut self, id: usize, n: f64) -> R<Val> {
        let count = self.keys(id)?.frames.len();
        match ordinal(n, count) {
            Some(k) => Ok(Val::Host(Host::Key(id, k))),
            None => Err(error(format!("no keyframe {}", value::number_to_string(n)))),
        }
    }

    fn nearest_key(&mut self, id: usize, t: f64) -> R<Val> {
        let keys = self.keys(id)?;
        let f = self.local_frame(id, t);
        keys.frames
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1.t - f).abs().total_cmp(&(b.1.t - f).abs()))
            .map(|(i, _)| Val::Host(Host::Key(id, i)))
            .ok_or_else(|| error("the nearest keyframe of a property without keyframes"))
    }

    /// `loopOut`, `loopIn`, `loopOutDuration` and `loopInDuration`: the
    /// keyframes repeated past the last one (before the first one) as a
    /// cycle, back and forth, each repeat offset by the change, or carried
    /// on at the speed they end with.
    fn loop_value(&mut self, run: &mut Run, id: usize, name: &str, args: &[Val]) -> R<Val> {
        let kind = match args.first() {
            None | Some(Val::Undef) => "cycle".to_owned(),
            Some(v) => value::to_string(v).to_lowercase(),
        };
        if !matches!(kind.as_str(), "cycle" | "pingpong" | "offset" | "continue") {
            return Err(error(format!("the loop type `{}`", short(&kind))));
        }
        let keys = self.keys(id)?;
        let now = self.local_frame(id, run.time);
        let current = self.before(run, id, run.time)?;
        let frames = &keys.frames;
        let count = frames.len();
        if count < 2 {
            return Ok(current);
        }
        let out = name.starts_with("loopOut");
        let by_duration = name.ends_with("Duration");
        let amount = arg_or(args, 1, 0.0);
        let amount = if amount.is_finite() { amount } else { 0.0 };
        // How many keyframes back (forward) the loop starts.
        let span = |n: f64| -> usize {
            let n = n.max(0.0) as usize;
            if n == 0 || n > count - 1 {
                count - 1
            } else {
                n
            }
        };
        let (first, last) = if out {
            let last = frames[count - 1].t;
            if now <= last {
                return Ok(current);
            }
            let first = if !by_duration {
                frames[count - 1 - span(amount)].t
            } else if amount > 0.0 {
                last - amount * self.fr
            } else {
                self.layer_frame(id, "ip")
            };
            (first, last)
        } else {
            let first = frames[0].t;
            if now >= first {
                return Ok(current);
            }
            let last = if !by_duration {
                frames[span(amount)].t
            } else if amount > 0.0 {
                first + amount * self.fr
            } else {
                self.layer_frame(id, "op")
            };
            (first, last)
        };
        let cycle = last - first;
        if cycle.is_nan() || cycle <= 0.0 {
            return Ok(current);
        }
        let at = |f: f64| Val::numbers(&keys.sample(f), keys.array());
        let repeats = ((now - first) / cycle).floor();
        let within = (now - first).rem_euclid(cycle);
        Ok(match kind.as_str() {
            "cycle" => at(first + within),
            "pingpong" => {
                if repeats.rem_euclid(2.0) == 1.0 {
                    at(last - within)
                } else {
                    at(first + within)
                }
            }
            "offset" => {
                let change = value::sub(&at(last), &at(first))?;
                value::sum(
                    &at(first + within),
                    &value::mul(&change, &Val::Num(repeats))?,
                )?
            }
            _ => {
                const EPS: f64 = 0.001;
                let (end, inside, distance) = if out {
                    (last, last - EPS, now - last)
                } else {
                    (first, first + EPS, first - now)
                };
                let edge = at(end);
                let slope = value::sub(&edge, &at(inside))?;
                value::sum(&edge, &value::mul(&slope, &Val::Num(distance / EPS))?)?
            }
        })
    }

    /// `wiggle(freq, amp, octaves = 1, amp_mult = 0.5, t = time)`: the value
    /// at `t` shaken by smooth noise seeded by the property, within `amp`.
    fn wiggle(&mut self, run: &mut Run, id: usize, args: &[Val]) -> R<Val> {
        let (freq, amp, octaves, amp_mult, t) = wiggle_args(args, run.time)?;
        let base = self.before(run, id, t)?;
        let seed = self.props[id].seed;
        let shake = |d: usize| {
            amp * value::fractal(
                value::mix64(seed ^ (d as u64 + 1)),
                t * freq,
                octaves,
                amp_mult,
            )
        };
        Ok(match &base {
            Val::Arr(l) => Val::list(
                l.items
                    .iter()
                    .enumerate()
                    .map(|(d, x)| Val::Num(to_number(x) + shake(d)))
                    .collect(),
            )?,
            other => Val::Num(to_number(other) + shake(0)),
        })
    }

    /// `temporalWiggle(freq, amp, …)`: the value at a time shaken by up to
    /// `amp` seconds.
    fn temporal_wiggle(&mut self, run: &mut Run, id: usize, args: &[Val]) -> R<Val> {
        let (freq, amp, octaves, amp_mult, t) = wiggle_args(args, run.time)?;
        let seed = value::mix64(self.props[id].seed ^ 0x7e3a_11d5);
        let shifted = t + amp * value::fractal(seed, t * freq, octaves, amp_mult);
        self.prop_at(run, id, shifted)
    }

    /// `smooth(width = 0.2, samples = 5, t = time)`: the mean of samples
    /// spread over `width` seconds around `t`.
    fn smooth(&mut self, run: &mut Run, id: usize, args: &[Val]) -> R<Val> {
        let width = arg_or(args, 0, 0.2);
        let samples = arg_or(args, 1, 5.0);
        let t = arg_or(args, 2, run.time);
        let samples = if samples.is_finite() {
            samples.round().clamp(1.0, 64.0) as usize
        } else {
            5
        };
        if samples == 1 || !width.is_finite() {
            return self.prop_at(run, id, t);
        }
        let mut total = self.prop_at(run, id, t - width / 2.0)?;
        for i in 1..samples {
            let at = t - width / 2.0 + width * i as f64 / (samples - 1) as f64;
            let v = self.prop_at(run, id, at)?;
            total = value::sum(&total, &v)?;
        }
        Ok(value::div(&total, &Val::Num(samples as f64))?)
    }

    /// Evaluates property `id`'s expression at every frame its layer shows
    /// (within the root composition for its layers); refuses when its
    /// keyframes would take more than `keyframes_left`.
    pub(super) fn bake(&mut self, id: usize, keyframes_left: usize) -> R<Baked> {
        let baked = self.bake_frames(id, keyframes_left);
        if baked.is_err() {
            self.failed.insert(id);
        }
        baked
    }

    fn bake_frames(&mut self, id: usize, keyframes_left: usize) -> R<Baked> {
        let keys = self.keys(id)?;
        self.program(id)?;
        let layer = self.props[id].layer;
        let json = self.layer(layer);
        let ip = json.get("ip").and_then(num).unwrap_or(self.root_span.0);
        let op = json.get("op").and_then(num).unwrap_or(self.root_span.1);
        let (mut start, mut end) = (ip.min(op), ip.max(op));
        if layer.comp == 0 {
            start = start.max(self.root_span.0);
            end = end.min(self.root_span.1);
        }
        let start = start.floor();
        let end = end.ceil().max(start);
        let count = (end - start + 1.0) as usize;
        if count.saturating_mul(keys.dims.max(1)) > keyframes_left {
            return Err(Fail::Keyframes);
        }
        self.limit = self.steps.saturating_add(PROPERTY_STEPS).min(TOTAL_STEPS);
        if self.steps >= self.limit {
            return Err(Fail::Budget);
        }
        let (st, sr) = self.timing(layer);
        let mut frames = Vec::with_capacity(count);
        for i in 0..count {
            let c = start + i as f64;
            let time = c / self.fr;
            let v = match self.cache.get(&(id, time.to_bits())) {
                Some(v) => v.clone(),
                None => self.evaluate(id, time)?,
            };
            frames.push(((c - st) / sr, value::to_numbers(&v).unwrap_or_default()));
        }
        Ok(Baked {
            frames,
            array: keys.array(),
        })
    }
}

/// The JSON pointers of text layer `layer`'s expression selectors (`t: 1`
/// in an animator's `s`): their expressions run per character
/// (`textIndex`, `selectorValue`), so the text conversion reads them.
fn text_expression_selectors(layer: &Value, pointer: &str) -> Vec<String> {
    let mut out = Vec::new();
    if layer.get("ty").and_then(num) != Some(5.0) {
        return out;
    }
    let is_expression = |s: &Value| s.get("t").and_then(num) == Some(1.0);
    let animators = layer
        .get("t")
        .and_then(|t| t.get("a"))
        .and_then(Value::as_array);
    for (i, animator) in animators.into_iter().flatten().enumerate() {
        match animator.get("s") {
            Some(Value::Array(list)) => {
                for (j, s) in list.iter().enumerate() {
                    if is_expression(s) {
                        out.push(format!("{pointer}/t/a/{i}/s/{j}"));
                    }
                }
            }
            Some(s) if is_expression(s) => out.push(format!("{pointer}/t/a/{i}/s")),
            _ => {}
        }
    }
    out
}

/// Collects the animatable properties holding expressions (`x` a string)
/// under `v`, with their JSON pointers, leaving out the subtrees at `skip`.
fn walk<'v>(
    v: &'v Value,
    pointer: &mut String,
    depth: usize,
    skip: &[String],
    out: &mut Vec<(String, &'v Value)>,
) {
    if depth > MAX_WALK {
        return;
    }
    match v {
        Value::Object(map) => {
            if has_expression(v) && map.contains_key("k") {
                out.push((pointer.clone(), v));
                return;
            }
            for (key, child) in map {
                if key.contains(['~', '/']) {
                    let key = key.replace('~', "~0").replace('/', "~1");
                    descend(child, &key, pointer, depth, skip, out);
                } else {
                    descend(child, key, pointer, depth, skip, out);
                }
            }
        }
        Value::Array(list) => {
            for (i, child) in list.iter().enumerate() {
                descend(child, &i, pointer, depth, skip, out);
            }
        }
        _ => {}
    }
}

fn descend<'v>(
    child: &'v Value,
    key: &dyn std::fmt::Display,
    pointer: &mut String,
    depth: usize,
    skip: &[String],
    out: &mut Vec<(String, &'v Value)>,
) {
    if !(child.is_object() || child.is_array()) {
        return;
    }
    let len = pointer.len();
    let _ = write!(pointer, "/{key}");
    if !skip.contains(pointer) {
        walk(child, pointer, depth + 1, skip, out);
    }
    pointer.truncate(len);
}

/// An expression's result as numbers of the property's dimensions: arrays
/// filled up with the value before the expression.
fn settle(v: &Val, before: &Val, keys: &Keys) -> R<Val> {
    let dims = keys.dims.max(1);
    let numbers = value::to_numbers(v).filter(|n| !n.is_empty());
    let Some(mut n) = numbers else {
        return Err(error(format!("a result that is {}", describe(v))));
    };
    if dims > 1 && !matches!(v, Val::Arr(_)) {
        return Err(error(format!("a number for a property of {dims} values")));
    }
    if n.len() < dims {
        let b = value::to_numbers(before).unwrap_or_default();
        let fill: Vec<f64> = (n.len()..dims)
            .map(|d| b.get(d).copied().unwrap_or(0.0))
            .collect();
        n.extend(fill);
    }
    if dims == 1 {
        n.truncate(1);
    }
    if n.iter().any(|x| !x.is_finite()) {
        return Err(error("a result that is not a number"));
    }
    Ok(Val::numbers(&n, keys.array()))
}

fn describe(v: &Val) -> &'static str {
    match v {
        Val::Undef => "undefined",
        Val::Null => "null",
        Val::Str(_) => "a string",
        Val::Arr(_) => "an array of something other than numbers",
        Val::Func(_) | Val::Native(_) | Val::Bound(_) => "a function",
        _ => "an object",
    }
}

fn describe_key(key: &Val) -> String {
    match key {
        Val::Str(s) => format!("`{}`", short(s)),
        Val::Num(n) => value::number_to_string(*n),
        other => other.type_of().into(),
    }
}

/// Whether JSON object `v` is called `name` (its name or match name).
fn named(v: &Value, name: &str) -> bool {
    ["nm", "mn"]
        .iter()
        .any(|k| v.get(k).and_then(Value::as_str) == Some(name))
}

/// Index `n - 1` when `n` counts one of `len` things from 1.
fn ordinal(n: f64, len: usize) -> Option<usize> {
    (n.fract() == 0.0 && n >= 1.0 && n <= len as f64).then(|| n as usize - 1)
}

/// Argument `i` as a number, `default` when it is left out.
fn arg_or(args: &[Val], i: usize, default: f64) -> f64 {
    match args.get(i) {
        None | Some(Val::Undef) => default,
        Some(v) => to_number(v),
    }
}

/// `freq, amp, octaves, amp_mult, t` of `wiggle` and `temporalWiggle`.
fn wiggle_args(args: &[Val], time: f64) -> R<(f64, f64, f64, f64, f64)> {
    let (freq, amp) = (arg_or(args, 0, f64::NAN), arg_or(args, 1, f64::NAN));
    if !freq.is_finite() || !amp.is_finite() {
        return Err(error("a wiggle without a frequency and an amplitude"));
    }
    Ok((
        freq,
        amp,
        arg_or(args, 2, 1.0),
        arg_or(args, 3, 0.5),
        arg_or(args, 4, time),
    ))
}
