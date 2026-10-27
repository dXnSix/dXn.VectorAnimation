//! Data binding as Rive's runtime binds an artboard's default view model
//! instance (`File::createDefaultViewModelInstance`,
//! `DataBindContext::bindFromContext`): each binding to the target reads
//! the instance's value of the property its source path names (through
//! nested view models), runs it through its converter
//! (`DataBindContextValue::apply`) and sets its target's property to it, an
//! object's (its value from then on, until an animation keys it) or a state
//! machine's bindable property (what view model conditions and blends
//! read).

use std::collections::{BTreeSet, HashMap};

use crate::{
    convert::interpolator,
    defs::{self, Field, props as p, types as t},
    file::{BindTarget, Converter, File, Instance, Object, Value, is_a},
};

/// Directions and kinds of a binding (`DataBindFlags`).
const TO_SOURCE: u64 = 1;
const TWO_WAY: u64 = 1 << 1;
const NAME_BASED: u64 = 1 << 4;

/// How deep converter groups nest at most.
const MAX_DEPTH: usize = 32;

/// The view model an artboard is bound to, as its first instance holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewModel {
    /// Its name.
    pub name: String,
    /// The instance's name.
    pub instance: String,
    /// Its properties, in order, with the instance's values.
    pub properties: Vec<Property>,
}

/// A property of a view model and its value.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    /// Its name.
    pub name: String,
    /// Its value.
    pub value: PropertyValue,
}

/// A view model property's value.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    /// A number.
    Number(f64),
    /// A boolean.
    Bool(bool),
    /// A color (ARGB).
    Color(u32),
    /// A string.
    Text(String),
    /// An enum's value, by index.
    Enum(Option<u64>),
    /// A trigger.
    Trigger,
    /// A kind not read here (a list, a nested view model, an asset).
    Other,
}

/// A value as a binding carries it (Rive's `DataValue`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Data {
    Number(f32),
    Bool(bool),
    /// ARGB.
    Color(u32),
    Text(String),
    /// An enum's value by index, and its enum (an index of `File::enums`).
    Enum(u32, Option<usize>),
    /// A trigger's count.
    Trigger(u32),
    /// What bindings here do not carry (a list, an artboard, a view model).
    Other,
}

/// The kind of value a converter puts out (`DataType`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Number,
    Bool,
    Color,
    Text,
    Enum,
    Trigger,
    /// What it is given (or, from a converter not changing the kind, what
    /// the source holds).
    Input,
    /// What nothing here binds (a list).
    Other,
}

impl Data {
    fn kind(&self) -> Kind {
        match self {
            Data::Number(_) => Kind::Number,
            Data::Bool(_) => Kind::Bool,
            Data::Color(_) => Kind::Color,
            Data::Text(_) => Kind::Text,
            Data::Enum(..) => Kind::Enum,
            Data::Trigger(_) => Kind::Trigger,
            Data::Other => Kind::Other,
        }
    }

    /// The value of a kind a binding falls back on (each `DataValue`'s
    /// `defaultValue`).
    fn initial(kind: Kind) -> Data {
        match kind {
            Kind::Number => Data::Number(0.0),
            Kind::Bool => Data::Bool(false),
            Kind::Color => Data::Color(0),
            Kind::Text => Data::Text(String::new()),
            Kind::Enum => Data::Enum(0, None),
            Kind::Trigger => Data::Trigger(0),
            Kind::Input | Kind::Other => Data::Other,
        }
    }
}

/// The view model artboard `artboard` of `file` is bound to, if it has one.
pub(crate) fn view_model(file: &File, artboard: usize) -> Option<ViewModel> {
    let vm = file
        .artboards
        .get(artboard)?
        .object()
        .id(p::ARTBOARD_VIEW_MODEL_ID)?;
    let model = file.view_models.get(vm)?;
    let instance = model.instances.first();
    let properties = model
        .properties
        .iter()
        .enumerate()
        .map(|(k, property)| {
            let value = match data_of(property, instance.and_then(|i| value_in(i, k))) {
                Data::Number(v) => PropertyValue::Number(f64::from(v)),
                Data::Bool(b) => PropertyValue::Bool(b),
                Data::Color(c) => PropertyValue::Color(c),
                Data::Text(s) => PropertyValue::Text(s),
                Data::Enum(i, _) => PropertyValue::Enum((i != u32::MAX).then_some(u64::from(i))),
                Data::Trigger(_) => PropertyValue::Trigger,
                Data::Other => PropertyValue::Other,
            };
            Property {
                name: property.string(p::VIEW_MODEL_COMPONENT_NAME),
                value,
            }
        })
        .collect();
    Some(ViewModel {
        name: model.object.string(p::VIEW_MODEL_COMPONENT_NAME),
        instance: instance.map_or_else(String::new, |i| {
            i.object.string(p::VIEW_MODEL_COMPONENT_NAME)
        }),
        properties,
    })
}

/// A view model property's value in an instance: its view model, the
/// instance (an index of the view model's instances; `None`, a new one of
/// initial values) and the property's index.
type Slot = (usize, Option<usize>, usize);

/// What a binding sets: its target, property and value (`None`: nothing),
/// and the value it reads.
type Set = (BindTarget, u16, Option<Value>, Slot);

/// Binds the default instance of artboard `artboard`'s view model into the
/// file's objects, then makes `changes` to it over time: what they set and
/// when goes to `File::bound`. What it cannot bind, as warnings.
pub(crate) fn bind_default(
    file: &mut File,
    artboard: usize,
    changes: &[crate::ValueChange],
) -> BTreeSet<String> {
    let mut warnings = BTreeSet::new();
    if !file.binds.iter().any(|b| ours(b.target, artboard)) {
        if !changes.is_empty() {
            warnings.insert("view model changes: nothing in the artboard is bound".into());
        }
        return warnings;
    }
    let Some(vm) = file
        .artboards
        .get(artboard)
        .and_then(|a| a.object().id(p::ARTBOARD_VIEW_MODEL_ID))
        .filter(|&vm| vm < file.view_models.len())
    else {
        warnings.insert("data bindings: the artboard has no view model; left unbound".into());
        return warnings;
    };
    let instance = (!file.view_models[vm].instances.is_empty()).then_some(0);
    let empty = HashMap::new();
    let (converters, sets) = evaluate(file, artboard, vm, instance, &empty, &mut warnings);
    file.converters = converters;
    let mut previous: HashMap<(BindTarget, u16), Value> = HashMap::new();
    for (target, key, value, _) in sets {
        if let Some(value) = value {
            if let Some(object) = target_mut(file, target) {
                object.set(key, value.clone());
            }
            previous.insert((target, key), value);
        }
    }
    // The changes, each time they are made at in turn.
    let mut changes: Vec<&crate::ValueChange> =
        changes.iter().filter(|c| c.time.is_finite()).collect();
    changes.sort_by(|a, b| a.time.total_cmp(&b.time));
    let mut overlay: HashMap<Slot, Data> = HashMap::new();
    let mut k = 0;
    while k < changes.len() {
        let time = changes[k].time.max(0.0);
        let mut changed: BTreeSet<Slot> = BTreeSet::new();
        while k < changes.len() && changes[k].time.max(0.0) == time {
            let c = changes[k];
            k += 1;
            match change(file, vm, instance, &mut overlay, c) {
                Ok(Some(slot)) => {
                    changed.insert(slot);
                }
                Ok(None) => {}
                Err(warning) => {
                    warnings.insert(warning);
                }
            }
        }
        if changed.is_empty() {
            continue;
        }
        let mut ignored = BTreeSet::new();
        let (_, sets) = evaluate(file, artboard, vm, instance, &overlay, &mut ignored);
        for (target, key, value, slot) in sets {
            let at = match target {
                BindTarget::Object { artboard, index } => Some((artboard, index, usize::MAX)),
                BindTarget::Bindable {
                    artboard,
                    machine,
                    index,
                } => Some((artboard, machine, index)),
                BindTarget::Converter { .. } | BindTarget::Other => None,
            };
            let Some(at) = at else { continue };
            if changed.contains(&slot) && at.2 != usize::MAX {
                file.bound.changed.entry(at).or_default().push(time);
            }
            let Some(value) = value else { continue };
            if previous.get(&(target, key)) == Some(&value) {
                continue;
            }
            previous.insert((target, key), value.clone());
            if at.2 == usize::MAX {
                file.bound
                    .objects
                    .entry((at.0, at.1, key))
                    .or_default()
                    .push((time, value));
            } else {
                file.bound
                    .bindables
                    .entry(at)
                    .or_default()
                    .push((time, key, value));
            }
        }
    }
    warnings
}

/// Makes change `c` to `overlay` (the values view model `vm`'s instance
/// `instance` holds changed): the property it changes, if it changes it.
fn change(
    file: &File,
    vm: usize,
    instance: Option<usize>,
    overlay: &mut HashMap<Slot, Data>,
    c: &crate::ValueChange,
) -> Result<Option<Slot>, String> {
    let binder = Binder {
        file,
        converters: &file.converters,
        vm,
        instance,
        overlay,
    };
    let Some(slot) = binder.named(&c.path) else {
        return Err(format!("view model changes: no property {:?}", c.path));
    };
    let now = binder.slot_value(slot);
    let Some(value) = changed_to(file, &c.value, &now) else {
        return Err(format!(
            "view model changes: {:?} cannot be set to {:?}",
            c.path, c.value
        ));
    };
    if value == now {
        return Ok(None);
    }
    overlay.insert(slot, value);
    Ok(Some(slot))
}

/// The change a listener's `ListenerViewModelChange` of bindable property
/// `bindable` of state machine `machine` of artboard `artboard` makes at
/// `time`, with `values` made before (`file` read afresh): the value its
/// bindable property holds then, through its binding to the view model
/// and its converter, to the property that binding reads.
pub(crate) fn listener_change(
    file: &mut File,
    (artboard, machine, bindable): (usize, usize, usize),
    values: &[crate::ValueChange],
    time: f64,
) -> Option<crate::ValueChange> {
    let before: Vec<crate::ValueChange> =
        values.iter().filter(|c| c.time <= time).cloned().collect();
    bind_default(file, artboard, &before);
    let target = BindTarget::Bindable {
        artboard,
        machine,
        index: bindable,
    };
    // What the bindable property holds then.
    let mut object = self::target(file, target)?.clone();
    for (_, key, v) in file
        .bound
        .bindables
        .get(&(artboard, machine, bindable))
        .into_iter()
        .flatten()
        .filter(|c| c.0 <= time)
    {
        object.set(*key, v.clone());
    }
    let value = bindable_value(&object)?;
    let bind = file.binds.iter().find(|b| {
        b.target == target && b.object.uint(p::DATA_BIND_FLAGS) & (TO_SOURCE | TWO_WAY) != 0
    })?;
    let vm = file.artboards[artboard]
        .object()
        .id(p::ARTBOARD_VIEW_MODEL_ID)?;
    let instance = (!file.view_models.get(vm)?.instances.is_empty()).then_some(0);
    let mut overlay = HashMap::new();
    for c in &before {
        let _ = change(file, vm, instance, &mut overlay, c);
    }
    let binder = Binder {
        file,
        converters: &file.converters,
        vm,
        instance,
        overlay: &overlay,
    };
    let o = &bind.object;
    let flags = o.uint(p::DATA_BIND_FLAGS);
    let ids = varuints(o.bytes(p::DATA_BIND_CONTEXT_SOURCE_PATH_IDS));
    let slot = binder.resolve(&ids)?;
    // To the view model, the main direction converts, else the reverse.
    let value = match o
        .id(p::DATA_BIND_CONVERTER_ID)
        .filter(|&c| c < binder.converters.len())
    {
        Some(c) => binder
            .convert(c, value, flags & TO_SOURCE != 0, flags, 0)
            .ok()?,
        None => value,
    };
    let now = binder.slot_value(slot);
    let value = match (value, &now) {
        (Data::Number(v), Data::Number(_)) => PropertyValue::Number(f64::from(v)),
        (Data::Bool(b), Data::Bool(_)) => PropertyValue::Bool(b),
        (Data::Color(c), Data::Color(_)) => PropertyValue::Color(c),
        (Data::Text(s), Data::Text(_)) => PropertyValue::Text(s),
        (Data::Enum(i, _), Data::Enum(..)) => PropertyValue::Enum(Some(u64::from(i))),
        // A count other than the trigger's fires it.
        (Data::Trigger(n), Data::Trigger(m)) if n != *m => PropertyValue::Trigger,
        _ => return None,
    };
    Some(crate::ValueChange {
        time,
        path: binder.names(&ids)?,
        value,
    })
}

/// The property of the view model instance bound that bindable property
/// `bindable` of state machine `machine` of artboard `artboard` is bound to
/// (by a binding to the view model), by name.
pub(crate) fn bindable_source(
    file: &File,
    (artboard, machine, bindable): (usize, usize, usize),
) -> Option<String> {
    let target = BindTarget::Bindable {
        artboard,
        machine,
        index: bindable,
    };
    let bind = file.binds.iter().find(|b| {
        b.target == target && b.object.uint(p::DATA_BIND_FLAGS) & (TO_SOURCE | TWO_WAY) != 0
    })?;
    let vm = file
        .artboards
        .get(artboard)?
        .object()
        .id(p::ARTBOARD_VIEW_MODEL_ID)?;
    let overlay = HashMap::new();
    let binder = Binder {
        file,
        converters: &file.converters,
        vm,
        instance: None,
        overlay: &overlay,
    };
    binder.names(&varuints(
        bind.object.bytes(p::DATA_BIND_CONTEXT_SOURCE_PATH_IDS),
    ))
}

/// The value bindable property `o` holds.
fn bindable_value(o: &Object) -> Option<Data> {
    Some(match o.ty {
        t::BINDABLE_PROPERTY_NUMBER => {
            Data::Number(f32_of(o, p::BINDABLE_PROPERTY_NUMBER_PROPERTY_VALUE))
        }
        t::BINDABLE_PROPERTY_BOOLEAN => {
            Data::Bool(o.flag(p::BINDABLE_PROPERTY_BOOLEAN_PROPERTY_VALUE))
        }
        t::BINDABLE_PROPERTY_STRING => {
            Data::Text(o.string(p::BINDABLE_PROPERTY_STRING_PROPERTY_VALUE))
        }
        t::BINDABLE_PROPERTY_COLOR => {
            Data::Color(o.color(p::BINDABLE_PROPERTY_COLOR_PROPERTY_VALUE))
        }
        t::BINDABLE_PROPERTY_ENUM => Data::Enum(
            o.uint(p::BINDABLE_PROPERTY_ENUM_PROPERTY_VALUE) as u32,
            None,
        ),
        t::BINDABLE_PROPERTY_TRIGGER => {
            Data::Trigger(o.uint(p::BINDABLE_PROPERTY_INTEGER_PROPERTY_VALUE) as u32)
        }
        _ => return None,
    })
}

/// Whether binding target `target` is in artboard `artboard` (or the file's,
/// a converter).
fn ours(target: BindTarget, artboard: usize) -> bool {
    match target {
        BindTarget::Object { artboard: a, .. } | BindTarget::Bindable { artboard: a, .. } => {
            a == artboard
        }
        BindTarget::Converter { .. } => true,
        BindTarget::Other => false,
    }
}

/// What the bindings of artboard `artboard` set with view model `vm`'s
/// instance `instance` holding the values `overlay` changes: the
/// converters as their own bindings set them, and the sets (theirs
/// first).
fn evaluate(
    file: &File,
    artboard: usize,
    vm: usize,
    instance: Option<usize>,
    overlay: &HashMap<Slot, Data>,
    warnings: &mut BTreeSet<String>,
) -> (Vec<Converter>, Vec<Set>) {
    let mut converters = file.converters.clone();
    let mut all = Vec::new();
    // The converters' own bindings first, as what the others convert
    // with.
    for of_converters in [true, false] {
        let binder = Binder {
            file,
            converters: &converters,
            vm,
            instance,
            overlay,
        };
        let mut sets = Vec::new();
        for bind in file.binds.iter().filter(|b| {
            ours(b.target, artboard)
                && matches!(b.target, BindTarget::Converter { .. }) == of_converters
        }) {
            match binder.bound(&bind.object, bind.target) {
                Ok(Some(set)) => sets.push(set),
                Ok(None) => {}
                Err(warning) => {
                    warnings.insert(warning);
                }
            }
        }
        if of_converters {
            for (target, key, value, _) in &sets {
                if let (BindTarget::Converter { index }, Some(value)) = (target, value)
                    && let Some(c) = converters.get_mut(*index)
                {
                    c.object.set(*key, value.clone());
                }
            }
        }
        all.extend(sets);
    }
    (converters, all)
}

/// The value change `value` makes to a property holding `now` (a trigger
/// fired counts once more; an enum's value is by index or name); `None`
/// when it is of another kind.
fn changed_to(file: &File, value: &PropertyValue, now: &Data) -> Option<Data> {
    Some(match (value, now) {
        (PropertyValue::Number(v), Data::Enum(_, e)) if *v >= 0.0 && v.fract() == 0.0 => {
            Data::Enum(*v as u32, *e)
        }
        (PropertyValue::Text(s), Data::Enum(_, e)) => {
            let values = e.and_then(|e| file.enums.get(e))?;
            let i = values.iter().position(|v| {
                v.string(p::DATA_ENUM_VALUE_KEY) == *s || v.string(p::DATA_ENUM_VALUE_VALUE) == *s
            })?;
            Data::Enum(i as u32, *e)
        }
        (PropertyValue::Number(v), Data::Number(_)) => Data::Number(*v as f32),
        (PropertyValue::Bool(b), Data::Bool(_)) => Data::Bool(*b),
        (PropertyValue::Color(c), Data::Color(_)) => Data::Color(*c),
        (PropertyValue::Text(s), Data::Text(_)) => Data::Text(s.clone()),
        (PropertyValue::Enum(i), Data::Enum(_, e)) => Data::Enum(
            i.map_or(u32::MAX, |i| u32::try_from(i).unwrap_or(u32::MAX)),
            *e,
        ),
        (PropertyValue::Trigger, Data::Trigger(n)) => Data::Trigger(n.wrapping_add(1)),
        _ => return None,
    })
}

/// The object binding target `target` names.
fn target(file: &File, target: BindTarget) -> Option<&Object> {
    match target {
        BindTarget::Object { artboard, index } => {
            file.artboards.get(artboard)?.objects.get(index)?.as_ref()
        }
        BindTarget::Bindable {
            artboard,
            machine,
            index,
        } => file
            .artboards
            .get(artboard)?
            .state_machines
            .get(machine)?
            .bindables
            .get(index),
        BindTarget::Converter { index } => file.converters.get(index).map(|c| &c.object),
        BindTarget::Other => None,
    }
}

fn target_mut(file: &mut File, target: BindTarget) -> Option<&mut Object> {
    match target {
        BindTarget::Object { artboard, index } => file
            .artboards
            .get_mut(artboard)?
            .objects
            .get_mut(index)?
            .as_mut(),
        BindTarget::Bindable {
            artboard,
            machine,
            index,
        } => file
            .artboards
            .get_mut(artboard)?
            .state_machines
            .get_mut(machine)?
            .bindables
            .get_mut(index),
        BindTarget::Converter { index } => file.converters.get_mut(index).map(|c| &mut c.object),
        BindTarget::Other => None,
    }
}

/// What bindings read: a view model's instance in a file.
struct Binder<'a> {
    file: &'a File,
    /// The file's converters, as bindings to them set them.
    converters: &'a [Converter],
    vm: usize,
    /// The instance bound (an index of the view model's instances).
    instance: Option<usize>,
    /// Values changed from the instances'.
    overlay: &'a HashMap<Slot, Data>,
}

impl Binder<'_> {
    /// What binding `bind` sets on `target`, if it reads a value; a
    /// warning for what it cannot bind.
    fn bound(&self, bind: &Object, target: BindTarget) -> Result<Option<Set>, String> {
        let flags = bind.uint(p::DATA_BIND_FLAGS);
        if flags & NAME_BASED != 0 {
            return Err("data bindings: bindings by name left unbound".into());
        }
        if flags & TWO_WAY == 0 && flags & TO_SOURCE != 0 {
            // To the view model alone: nothing reads it back here.
            return Ok(None);
        }
        // The main direction to the target converts, else the reverse.
        let main = flags & TO_SOURCE == 0;
        let path = varuints(bind.bytes(p::DATA_BIND_CONTEXT_SOURCE_PATH_IDS));
        let Some(slot) = self.resolve(&path) else {
            return Ok(None);
        };
        let Ok(key) = u16::try_from(bind.uint(p::DATA_BIND_PROPERTY_KEY)) else {
            return Ok(None);
        };
        let key = defs::canonical(key);
        let source = self.slot_value(slot);
        let converter = bind
            .id(p::DATA_BIND_CONVERTER_ID)
            .filter(|&c| c < self.converters.len());
        let kind = match converter.map(|c| self.output(c, 0)) {
            None | Some(Kind::Input) => source.kind(),
            Some(kind) => kind,
        };
        if kind == Kind::Other {
            return Ok(Some((target, key, None, slot)));
        }
        let value = match converter {
            Some(c) => self.convert(c, source, main, flags, 0)?,
            None => source,
        };
        let value = if value.kind() == kind {
            value
        } else {
            Data::initial(kind)
        };
        Ok(Some((target, key, self.applied(value, key, target), slot)))
    }

    /// The property source path `path` names: the view model's index, then
    /// property indices, through nested view models.
    fn resolve(&self, path: &[u64]) -> Option<Slot> {
        let (&first, rest) = path.split_first()?;
        if usize::try_from(first).ok()? != self.vm {
            return None;
        }
        let (&last, through) = rest.split_last()?;
        let (mut vm, mut instance) = (self.vm, self.instance);
        for &k in through {
            (vm, instance) = self.nested(vm, instance, usize::try_from(k).ok()?)?;
        }
        let slot = (vm, instance, usize::try_from(last).ok()?);
        self.exists(slot).then_some(slot)
    }

    /// The names of the properties source path `ids` goes through, `/`
    /// between (as [`Binder::named`] reads them).
    fn names(&self, ids: &[u64]) -> Option<String> {
        let (&first, rest) = ids.split_first()?;
        let mut vm = usize::try_from(first).ok()?;
        let mut names = Vec::new();
        for (n, &k) in rest.iter().enumerate() {
            let property = self
                .file
                .view_models
                .get(vm)?
                .properties
                .get(usize::try_from(k).ok()?)?;
            names.push(property.string(p::VIEW_MODEL_COMPONENT_NAME));
            if n + 1 < rest.len() {
                vm = property.id(p::VIEW_MODEL_PROPERTY_VIEW_MODEL_VIEW_MODEL_REFERENCE_ID)?;
            }
        }
        Some(names.join("/"))
    }

    /// The property named by `path` (property names, nested view models'
    /// first, `/` between).
    fn named(&self, path: &str) -> Option<Slot> {
        let names: Vec<&str> = path.split('/').collect();
        let (last, through) = names.split_last()?;
        let (mut vm, mut instance) = (self.vm, self.instance);
        let index = |vm: usize, name: &str| {
            self.file
                .view_models
                .get(vm)?
                .properties
                .iter()
                .position(|p| p.string(defs::props::VIEW_MODEL_COMPONENT_NAME) == name)
        };
        for name in through {
            (vm, instance) = self.nested(vm, instance, index(vm, name)?)?;
        }
        let slot = (vm, instance, index(vm, last)?);
        self.exists(slot).then_some(slot)
    }

    /// The view model and instance nested view model property `k` of view
    /// model `vm`'s instance `instance` refers to (`None` within a new
    /// instance: a new one of initial values).
    fn nested(
        &self,
        vm: usize,
        instance: Option<usize>,
        k: usize,
    ) -> Option<(usize, Option<usize>)> {
        let property = self.file.view_models.get(vm)?.properties.get(k)?;
        if property.ty != t::VIEW_MODEL_PROPERTY_VIEW_MODEL || !self.exists((vm, instance, k)) {
            return None;
        }
        let next = property.id(p::VIEW_MODEL_PROPERTY_VIEW_MODEL_VIEW_MODEL_REFERENCE_ID)?;
        let model = self.file.view_models.get(next)?;
        let Some(i) = instance else {
            return Some((next, None));
        };
        let i = value_in(&self.file.view_models[vm].instances[i], k)?
            .id(p::VIEW_MODEL_INSTANCE_VIEW_MODEL_PROPERTY_VALUE)
            .filter(|&i| i < model.instances.len())?;
        Some((next, Some(i)))
    }

    /// Whether the instance of slot `slot` has its property: a new
    /// instance has each; one of the file's, those it writes a value of
    /// (as Rive copies it).
    fn exists(&self, (vm, instance, k): Slot) -> bool {
        let Some(model) = self.file.view_models.get(vm) else {
            return false;
        };
        k < model.properties.len()
            && instance.is_none_or(|i| {
                model
                    .instances
                    .get(i)
                    .is_some_and(|i| value_in(i, k).is_some())
            })
    }

    /// The value slot `slot` holds.
    fn slot_value(&self, slot: Slot) -> Data {
        if let Some(v) = self.overlay.get(&slot) {
            return v.clone();
        }
        let (vm, instance, k) = slot;
        let model = &self.file.view_models[vm];
        let set = instance
            .and_then(|i| model.instances.get(i))
            .and_then(|i| value_in(i, k));
        model
            .properties
            .get(k)
            .map_or(Data::Other, |property| data_of(property, set))
    }

    /// What converter `c` puts out (`DataConverter::outputType`).
    fn output(&self, c: usize, depth: usize) -> Kind {
        let Some(converter) = self.converters.get(c) else {
            return Kind::Input;
        };
        match converter.object.ty {
            t::DATA_CONVERTER_GROUP if depth < MAX_DEPTH => converter
                .items
                .iter()
                .rev()
                .map(|&i| self.output(i, depth + 1))
                .find(|&k| k != Kind::Input)
                .unwrap_or(Kind::Input),
            t::DATA_CONVERTER_OPERATION_VALUE
            | t::DATA_CONVERTER_OPERATION_VIEW_MODEL
            | t::DATA_CONVERTER_SYSTEM_DEGS_TO_RADS
            | t::DATA_CONVERTER_SYSTEM_NORMALIZER
            | t::DATA_CONVERTER_ROUNDER
            | t::DATA_CONVERTER_RANGE_MAPPER
            | t::DATA_CONVERTER_TO_NUMBER
            | t::DATA_CONVERTER_FORMULA
            | t::DATA_CONVERTER_LIST_TO_LENGTH => Kind::Number,
            t::DATA_CONVERTER_TO_STRING
            | t::DATA_CONVERTER_STRING_PAD
            | t::DATA_CONVERTER_STRING_TRIM
            | t::DATA_CONVERTER_STRING_REMOVE_ZEROS => Kind::Text,
            t::DATA_CONVERTER_BOOLEAN_NEGATE => Kind::Bool,
            t::DATA_CONVERTER_TRIGGER => Kind::Trigger,
            t::DATA_CONVERTER_NUMBER_TO_LIST => Kind::Other,
            _ => Kind::Input,
        }
    }

    /// `input` through converter `c` (its `convert`, or its
    /// `reverseConvert` when not `main`) for a binding of flags `flags`.
    fn convert(
        &self,
        c: usize,
        input: Data,
        main: bool,
        flags: u64,
        depth: usize,
    ) -> Result<Data, String> {
        let Some(converter) = self.converters.get(c) else {
            return Ok(input);
        };
        let o = &converter.object;
        let unbound = |what: &str| Err(format!("data bindings: {what} converters left unbound"));
        Ok(match o.ty {
            t::DATA_CONVERTER_GROUP => {
                if depth >= MAX_DEPTH {
                    return unbound("deeply nested");
                }
                let mut value = input;
                if main {
                    for &i in &converter.items {
                        value = self.convert(i, value, main, flags, depth + 1)?;
                    }
                } else {
                    for &i in converter.items.iter().rev() {
                        value = self.convert(i, value, main, flags, depth + 1)?;
                    }
                }
                value
            }
            t::DATA_CONVERTER_OPERATION_VALUE
            | t::DATA_CONVERTER_SYSTEM_DEGS_TO_RADS
            | t::DATA_CONVERTER_SYSTEM_NORMALIZER => {
                // The system converters go forward when the binding's main
                // direction is to the target, whichever way they run.
                let forward = if o.ty == t::DATA_CONVERTER_OPERATION_VALUE {
                    main
                } else {
                    flags & TO_SOURCE == 0
                };
                let value = f32_of(o, p::DATA_CONVERTER_OPERATION_VALUE_OPERATION_VALUE);
                operate(o, input, value, forward)
            }
            t::DATA_CONVERTER_OPERATION_VIEW_MODEL => {
                let path =
                    varuints(o.bytes(p::DATA_CONVERTER_OPERATION_VIEW_MODEL_SOURCE_PATH_IDS));
                let value = match self.resolve(&path).map(|slot| self.slot_value(slot)) {
                    Some(Data::Number(v)) => v,
                    _ => 0.0,
                };
                operate(o, input, value, main)
            }
            t::DATA_CONVERTER_ROUNDER if main => match input {
                Data::Number(v) => {
                    let decimals = o.uint(p::DATA_CONVERTER_ROUNDER_DECIMALS).min(64) as i32;
                    let rounder = 10f32.powi(decimals);
                    Data::Number((v * rounder).round() / rounder)
                }
                _ => Data::Number(0.0),
            },
            t::DATA_CONVERTER_TO_STRING if main => to_string(self.file, o, input),
            t::DATA_CONVERTER_TO_NUMBER if main => Data::Number(to_number(input)),
            t::DATA_CONVERTER_BOOLEAN_NEGATE => match input {
                Data::Bool(b) => Data::Bool(!b),
                _ => Data::Bool(false),
            },
            t::DATA_CONVERTER_TRIGGER if main => match input {
                Data::Enum(v, _) | Data::Trigger(v) => Data::Trigger(v.wrapping_add(1)),
                _ => Data::Trigger(0),
            },
            t::DATA_CONVERTER_STRING_PAD if main => match input {
                Data::Text(s) => Data::Text(pad(o, s)),
                _ => Data::Text(String::new()),
            },
            t::DATA_CONVERTER_STRING_TRIM if main => match input {
                Data::Text(s) => Data::Text(
                    match o.uint(p::DATA_CONVERTER_STRING_TRIM_TRIM_TYPE) {
                        1 => s.trim_start_matches(is_space),
                        2 => s.trim_end_matches(is_space),
                        3 => s.trim_matches(is_space),
                        _ => &s,
                    }
                    .to_owned(),
                ),
                _ => Data::Text(String::new()),
            },
            t::DATA_CONVERTER_STRING_REMOVE_ZEROS if main => match input {
                Data::Text(s) => Data::Text(remove_zeros(s)),
                _ => Data::Text(String::new()),
            },
            t::DATA_CONVERTER_RANGE_MAPPER => {
                let [a, b, c, d] = [
                    p::DATA_CONVERTER_RANGE_MAPPER_MIN_INPUT,
                    p::DATA_CONVERTER_RANGE_MAPPER_MAX_INPUT,
                    p::DATA_CONVERTER_RANGE_MAPPER_MIN_OUTPUT,
                    p::DATA_CONVERTER_RANGE_MAPPER_MAX_OUTPUT,
                ]
                .map(|k| f32_of(o, k));
                if main {
                    self.map_range(o, input, a, b, c, d)
                } else {
                    self.map_range(o, input, c, d, a, b)
                }
            }
            // As it first converts a value: the value, until it changes.
            t::DATA_CONVERTER_INTERPOLATOR => input,
            t::DATA_CONVERTER_FORMULA => return unbound("formula"),
            t::DATA_CONVERTER_NUMBER_TO_LIST | t::DATA_CONVERTER_LIST_TO_LENGTH => {
                return unbound("list");
            }
            ty if is_a(ty, t::SCRIPTED_DATA_CONVERTER) => return unbound("scripted"),
            // Converters without a reverse give their input back.
            _ => input,
        })
    }

    /// `input` mapped from the range `min_in`..`max_in` to
    /// `min_out`..`max_out` by range mapper `o`
    /// (`DataConverterRangeMapper::calculateRange`).
    fn map_range(
        &self,
        o: &Object,
        input: Data,
        min_in: f32,
        max_in: f32,
        min_out: f32,
        max_out: f32,
    ) -> Data {
        const CLAMP_LOWER: u64 = 1;
        const CLAMP_UPPER: u64 = 1 << 1;
        const MODULO: u64 = 1 << 2;
        const REVERSE: u64 = 1 << 3;
        let Data::Number(mut value) = input else {
            return Data::Number(0.0);
        };
        if min_out == max_out {
            return Data::Number(min_out);
        }
        let flags = o.uint(p::DATA_CONVERTER_RANGE_MAPPER_FLAGS);
        if value < min_in && flags & CLAMP_LOWER != 0 {
            value = min_in;
        } else if value > max_in && flags & CLAMP_UPPER != 0 {
            value = max_in;
        }
        if (value < min_in || value > max_in) && flags & MODULO != 0 {
            value = (positive_mod(value, max_in - min_in) + min_in).abs();
        }
        let mut f = (value - min_in) / (max_in - min_in);
        if flags & REVERSE != 0 {
            f = 1.0 - f;
        }
        let eased = o
            .id(p::DATA_CONVERTER_RANGE_MAPPER_INTERPOLATOR_ID)
            .and_then(|i| self.file.interpolators.get(i));
        match eased {
            Some(i) if f > 0.0 && f < 1.0 => f = interpolator(i).transform(f64::from(f)) as f32,
            _ if o.uint(p::DATA_CONVERTER_RANGE_MAPPER_INTERPOLATION_TYPE) == 0 => {
                f = if f <= 0.0 { 0.0 } else { 1.0 };
            }
            _ => {}
        }
        Data::Number(f * max_out + (1.0 - f) * min_out)
    }

    /// The value `value` (to the property `key` of the target `target`) sets
    /// it to, as each `DataBindContextValue` applies its kind; `None` where
    /// it sets nothing.
    fn applied(&self, value: Data, key: u16, target: BindTarget) -> Option<Value> {
        let field = defs::field(key)?;
        let object = self::target(self.file, target)?;
        // A solo's active child is set by its index or name.
        let solo = object.ty == t::SOLO && field == Field::Uint;
        let children = || -> Vec<(usize, &Object)> {
            let BindTarget::Object { artboard, index } = target else {
                return Vec::new();
            };
            self.file
                .artboards
                .get(artboard)
                .map_or_else(Vec::new, |a| {
                    a.objects
                        .iter()
                        .enumerate()
                        .filter_map(|(i, o)| o.as_ref().map(|o| (i, o)))
                        .filter(|(_, o)| o.id(p::COMPONENT_PARENT_ID) == Some(index))
                        .collect()
                })
        };
        let at = |k: usize| children().get(k).map(|&(i, _)| Value::Uint(i as u64));
        let named = |name: &str| {
            children()
                .into_iter()
                .find(|(_, o)| o.string(p::COMPONENT_NAME) == name)
                .map(|(i, _)| Value::Uint(i as u64))
        };
        match value {
            Data::Number(v) => match field {
                Field::Double => Some(Value::Double(v)),
                Field::Uint if solo => at(v.round().max(0.0) as usize),
                Field::Uint => Some(Value::Uint(v.round().max(0.0) as u64)),
                _ => None,
            },
            Data::Text(s) => match field {
                Field::Uint if solo => named(&s),
                Field::String | Field::Bytes => Some(Value::Bytes(s.into_bytes())),
                _ => None,
            },
            Data::Bool(b) => (field == Field::Bool).then_some(Value::Bool(b)),
            Data::Color(c) => (field == Field::Color).then_some(Value::Color(c)),
            Data::Enum(i, e) if solo => named(&enum_value(self.file, e, i)),
            Data::Enum(i, _) | Data::Trigger(i) => {
                (field == Field::Uint).then_some(Value::Uint(u64::from(i)))
            }
            Data::Other => None,
        }
    }
}

/// The value of an instance `set` of view model property `property` (its
/// instance value, else its initial one).
fn data_of(property: &Object, set: Option<&Object>) -> Data {
    let initial = Object {
        ty: 0,
        props: Vec::new(),
    };
    let o = set.unwrap_or(&initial);
    match property.ty {
        t::VIEW_MODEL_PROPERTY_NUMBER => {
            Data::Number(f32_of(o, p::VIEW_MODEL_INSTANCE_NUMBER_PROPERTY_VALUE))
        }
        t::VIEW_MODEL_PROPERTY_BOOLEAN => {
            Data::Bool(o.flag(p::VIEW_MODEL_INSTANCE_BOOLEAN_PROPERTY_VALUE))
        }
        t::VIEW_MODEL_PROPERTY_COLOR => {
            Data::Color(o.color(p::VIEW_MODEL_INSTANCE_COLOR_PROPERTY_VALUE))
        }
        t::VIEW_MODEL_PROPERTY_STRING => {
            Data::Text(o.string(p::VIEW_MODEL_INSTANCE_STRING_PROPERTY_VALUE))
        }
        ty if is_a(ty, t::VIEW_MODEL_PROPERTY_ENUM) => Data::Enum(
            o.uint(p::VIEW_MODEL_INSTANCE_ENUM_PROPERTY_VALUE) as u32,
            (ty == t::VIEW_MODEL_PROPERTY_ENUM_CUSTOM)
                .then(|| property.id(p::VIEW_MODEL_PROPERTY_ENUM_CUSTOM_ENUM_ID))
                .flatten(),
        ),
        t::VIEW_MODEL_PROPERTY_TRIGGER => {
            Data::Trigger(o.uint(p::VIEW_MODEL_INSTANCE_TRIGGER_PROPERTY_VALUE) as u32)
        }
        _ => Data::Other,
    }
}

/// Instance `instance`'s value of property `k`.
fn value_in(instance: &Instance, k: usize) -> Option<&Object> {
    instance
        .values
        .iter()
        .find(|v| v.id(p::VIEW_MODEL_INSTANCE_VALUE_VIEW_MODEL_PROPERTY_ID) == Some(k))
}

/// A double property as the file writes it (else its initial value).
fn f32_of(o: &Object, key: u16) -> f32 {
    match o.value(key) {
        Some(Value::Double(v)) => *v,
        _ => defs::initial(key).unwrap_or(0.0) as f32,
    }
}

/// `input` and `value` under operation `o`'s operation, forward or back
/// (`DataConverterOperation::convertValue`, `reverseConvertValue`).
fn operate(o: &Object, input: Data, value: f32, forward: bool) -> Data {
    let x = match input {
        Data::Number(x) => x,
        _ => return Data::Number(0.0),
    };
    let op = o.uint(p::DATA_CONVERTER_OPERATION_OPERATION_TYPE);
    Data::Number(if forward {
        match op {
            0 => x + value,
            1 => x - value,
            2 => x * value,
            3 => x / value,
            4 => positive_mod(x, value),
            5 => x.sqrt(),
            6 => x.powf(value),
            7 => x.exp(),
            8 => x.ln(),
            9 => x.cos(),
            10 => x.sin(),
            11 => x.tan(),
            12 => x.acos(),
            13 => x.asin(),
            14 => x.atan(),
            15 => x.atan2(value),
            16 => x.round(),
            17 => x.floor(),
            18 => x.ceil(),
            _ => value,
        }
    } else {
        match op {
            0 => x - value,
            1 => x + value,
            2 => x / value,
            3 => x * value,
            5 => x.powi(2),
            6 => x.powf(1.0 / value),
            7 => x.ln(),
            8 => x.exp(),
            9 => x.acos(),
            10 => x.asin(),
            11 => x.atan(),
            12 => x.cos(),
            13 => x.sin(),
            14 => x.tan(),
            4 | 15..=18 => x,
            _ => value,
        }
    })
}

/// Rive's `math::positive_mod`.
fn positive_mod(value: f32, range: f32) -> f32 {
    let range = range.abs();
    let v = value % range;
    if v < 0.0 { v + range } else { v }
}

/// `input` as text (`DataConverterToString::convert`).
fn to_string(file: &File, o: &Object, input: Data) -> Data {
    const ROUND: u64 = 1;
    const TRAILING_ZEROS: u64 = 1 << 1;
    const COMMAS: u64 = 1 << 2;
    Data::Text(match input {
        Data::Number(v) => {
            let flags = o.uint(p::DATA_CONVERTER_TO_STRING_FLAGS);
            let decimals = if flags & ROUND != 0 {
                o.uint(p::DATA_CONVERTER_TO_STRING_DECIMALS).min(64) as usize
            } else {
                6
            };
            let mut s = fixed(v, decimals);
            if flags & TRAILING_ZEROS != 0 {
                s = remove_zeros(s);
            }
            if flags & COMMAS != 0 {
                s = with_commas(&s);
            }
            s
        }
        Data::Enum(i, e) => enum_value(file, e, i),
        Data::Text(s) => s,
        Data::Color(c) => color_text(&o.string(p::DATA_CONVERTER_TO_STRING_COLOR_FORMAT), c),
        Data::Bool(b) => if b { "1" } else { "0" }.to_owned(),
        Data::Trigger(v) => v.to_string(),
        Data::Other => String::new(),
    })
}

/// `v` with `decimals` decimals, as C's `%.*f` prints it.
fn fixed(v: f32, decimals: usize) -> String {
    if v.is_nan() {
        if v.is_sign_negative() { "-nan" } else { "nan" }.to_owned()
    } else if v.is_infinite() {
        if v < 0.0 { "-inf" } else { "inf" }.to_owned()
    } else {
        format!("{:.*}", decimals, f64::from(v))
    }
}

/// `value` without the zeros ending its fraction
/// (`DataConverterStringRemoveZeros::removeZeros`).
fn remove_zeros(value: String) -> String {
    if !value.contains('.') {
        return value;
    }
    let mut s = value.trim_end_matches('0').to_owned();
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// `num` with commas between its thousands
/// (`DataConverterToString::formatWithCommas`).
fn with_commas(num: &str) -> String {
    let (int, frac) = num.split_at(num.find('.').unwrap_or(num.len()));
    let mut int = int.as_bytes().to_vec();
    let mut at = int.len() as isize - 3;
    while at > 0 && int[at as usize - 1].is_ascii_digit() {
        int.insert(at as usize, b',');
        at -= 3;
    }
    String::from_utf8_lossy(&int).into_owned() + frac
}

/// Color `c` in format `format` (`%r`, `%g`, `%b`, `%a` its channels, `%R`
/// … `%A` them in hex, `%h`, `%s`, `%l` its hue, saturation and
/// luminance; `\` escapes), else its signed integer.
fn color_text(format: &str, c: u32) -> String {
    if format.is_empty() {
        return (c as i32).to_string();
    }
    let [a, r, g, b] = c.to_be_bytes();
    let (h, s, l) = hsl(c, r, g, b);
    let mut out = String::new();
    let (mut escaped, mut marker) = (false, false);
    for ch in format.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            if marker {
                out.push('%');
                marker = false;
            }
            escaped = true;
        } else if ch == '%' {
            if marker {
                out.push('%');
            }
            marker = true;
        } else if marker {
            match ch {
                'r' => out += &r.to_string(),
                'g' => out += &g.to_string(),
                'b' => out += &b.to_string(),
                'a' => out += &a.to_string(),
                'R' => out += &format!("{r:02X}"),
                'G' => out += &format!("{g:02X}"),
                'B' => out += &format!("{b:02X}"),
                'A' => out += &format!("{a:02X}"),
                'h' => out += &h.to_string(),
                'l' => out += &l.to_string(),
                's' => out += &s.to_string(),
                _ => {
                    out.push('%');
                    out.push(ch);
                }
            }
            marker = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// A color's hue (degrees), saturation and luminance (percent), rounded
/// (`_ColorConverter::calculateHSL`; black is all 0).
fn hsl(c: u32, r: u8, g: u8, b: u8) -> (i32, i32, i32) {
    if c == 0 {
        return (0, 0, 0);
    }
    let [r, g, b] = [r, g, b].map(|v| f32::from(v) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta) % 6.0
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    let mut h = (hue * 60.0).round() as i32;
    if h < 0 {
        h += 360;
    }
    let lum = (max + min) / 2.0;
    let sat = if delta == 0.0 {
        0.0
    } else {
        delta / (1.0 - (2.0 * lum - 1.0).abs())
    };
    (
        h,
        (sat * 100.0).round() as i32,
        (lum * 100.0).round() as i32,
    )
}

/// Enum `e`'s value `i` (its value, else its key).
fn enum_value(file: &File, e: Option<usize>, i: u32) -> String {
    e.and_then(|e| file.enums.get(e))
        .and_then(|values| values.get(i as usize))
        .map(|v| {
            let value = v.string(p::DATA_ENUM_VALUE_VALUE);
            if value.is_empty() {
                v.string(p::DATA_ENUM_VALUE_KEY)
            } else {
                value
            }
        })
        .unwrap_or_default()
}

/// `input` as a number (`DataConverterToNumber::convert`).
fn to_number(input: Data) -> f32 {
    match input {
        Data::Number(v) => v,
        Data::Text(s) => atof(&s),
        Data::Enum(i, _) => i as f32,
        Data::Color(c) => c as i32 as f32,
        Data::Bool(b) => f32::from(u8::from(b)),
        Data::Trigger(_) | Data::Other => 0.0,
    }
}

/// The number `s` starts with, as C's `atof` reads it (0 without one).
fn atof(s: &str) -> f32 {
    let s = s.trim_start_matches(is_space);
    let b = s.as_bytes();
    let mut end = 0;
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    if matches!(b.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    let int = digits(end);
    end += int;
    let mut frac = 0;
    if b.get(end) == Some(&b'.') {
        frac = digits(end + 1);
        end += 1 + frac;
    }
    if int + frac == 0 {
        return 0.0;
    }
    if matches!(b.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(b.get(end + 1), Some(b'+' | b'-')));
        let exp = digits(end + 1 + sign);
        if exp > 0 {
            end += 1 + sign + exp;
        }
    }
    s[..end].parse::<f64>().map_or(0.0, |v| v as f32)
}

/// What C's `isspace` takes for space.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0B' | '\x0C' | '\r')
}

/// Text `s` padded by string pad `o` (`DataConverterStringPad::convert`):
/// its pattern repeated to its length, at the start or (pad type 1) the
/// end.
fn pad(o: &Object, s: String) -> String {
    let length = o.uint(p::DATA_CONVERTER_STRING_PAD_LENGTH).min(1 << 16) as usize;
    let pattern = o.bytes(p::DATA_CONVERTER_STRING_PAD_TEXT);
    let mut bytes = s.into_bytes();
    if bytes.len() < length && !pattern.is_empty() {
        let padding: Vec<u8> = pattern
            .iter()
            .copied()
            .cycle()
            .take(length - bytes.len())
            .collect();
        if o.uint(p::DATA_CONVERTER_STRING_PAD_PAD_TYPE) == 1 {
            bytes.extend(padding);
        } else {
            bytes.splice(0..0, padding);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The varuints of `bytes`.
fn varuints(bytes: &[u8]) -> Vec<u64> {
    let mut out = Vec::new();
    let (mut v, mut shift) = (0u64, 0u32);
    for &b in bytes {
        if shift < 64 {
            v |= u64::from(b & 0x7F) << shift;
        }
        shift += 7;
        if b & 0x80 == 0 {
            out.push(v);
            v = 0;
            shift = 0;
        }
    }
    out
}
