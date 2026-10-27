//! Data binding as Rive's runtime binds an artboard's default view model
//! instance (`File::createDefaultViewModelInstance`,
//! `DataBindContext::bindFromContext`): each binding to the target reads
//! the instance's value of the property its source path names and sets its
//! target's property to it, an object's (its value from then on, until an
//! animation keys it) or a state machine's bindable property (what view
//! model conditions and blends read).

use std::collections::BTreeSet;

use crate::{
    defs::{self, Field, props as p, types as t},
    file::{BindTarget, File, Object, Value},
};

/// Directions and kinds of a binding (`DataBindFlags`).
const TO_SOURCE: u64 = 1;
const TWO_WAY: u64 = 1 << 1;
const NAME_BASED: u64 = 1 << 4;

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
            let set = instance.and_then(|i| {
                i.values
                    .iter()
                    .find(|v| v.id(p::VIEW_MODEL_INSTANCE_VALUE_VIEW_MODEL_PROPERTY_ID) == Some(k))
            });
            let value = match (property.ty, value_of(property, set)) {
                (t::VIEW_MODEL_PROPERTY_TRIGGER, _) => PropertyValue::Trigger,
                (_, Some(Value::Double(d))) => PropertyValue::Number(f64::from(d)),
                (_, Some(Value::Bool(b))) => PropertyValue::Bool(b),
                (_, Some(Value::Color(c))) => PropertyValue::Color(c),
                (_, Some(Value::Bytes(b))) => {
                    PropertyValue::Text(String::from_utf8_lossy(&b).into_owned())
                }
                (_, Some(Value::Uint(u))) => PropertyValue::Enum((u != 0xFFFF_FFFF).then_some(u)),
                (_, None) => PropertyValue::Other,
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

/// Binds the default instance of artboard `artboard`'s view model into the
/// file's objects; what it cannot bind, as warnings.
pub(crate) fn bind_default(file: &mut File, artboard: usize) -> BTreeSet<String> {
    let mut warnings = BTreeSet::new();
    let ours = |target: &BindTarget| match *target {
        BindTarget::Object { artboard: a, .. } | BindTarget::Bindable { artboard: a, .. } => {
            a == artboard
        }
        BindTarget::Other => false,
    };
    if !file.binds.iter().any(|b| ours(&b.target)) {
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
    let model = &file.view_models[vm];
    let instance = model.instances.first();
    // Each property's value in the instance, else its type's initial one.
    let values: Vec<Option<Value>> = model
        .properties
        .iter()
        .enumerate()
        .map(|(k, property)| {
            let set = instance.and_then(|i| {
                i.values
                    .iter()
                    .find(|v| v.id(p::VIEW_MODEL_INSTANCE_VALUE_VIEW_MODEL_PROPERTY_ID) == Some(k))
            });
            value_of(property, set)
        })
        .collect();
    let mut sets: Vec<(BindTarget, u16, Value)> = Vec::new();
    for bind in file.binds.iter().filter(|b| ours(&b.target)) {
        let o = &bind.object;
        let flags = o.uint(p::DATA_BIND_FLAGS);
        if flags & NAME_BASED != 0 {
            warnings.insert("data bindings: bindings by name left unbound".into());
            continue;
        }
        if flags & TWO_WAY == 0 && flags & TO_SOURCE != 0 {
            // To the view model alone: nothing reads it back here.
            continue;
        }
        if o.id(p::DATA_BIND_CONVERTER_ID).is_some() {
            warnings.insert("data bindings: bindings through converters left unbound".into());
            continue;
        }
        let path = varuints(o.bytes(p::DATA_BIND_CONTEXT_SOURCE_PATH_IDS));
        let value = match path[..] {
            [m, property] if m as usize == vm => values.get(property as usize).cloned().flatten(),
            [m, ..] if m as usize == vm => {
                warnings.insert("data bindings: nested view models left unbound".into());
                continue;
            }
            _ => None,
        };
        let Some(value) = value else {
            continue;
        };
        let Ok(key) = u16::try_from(o.uint(p::DATA_BIND_PROPERTY_KEY)) else {
            continue;
        };
        let key = defs::canonical(key);
        if let Some(value) = coerce(value, key) {
            sets.push((bind.target, key, value));
        }
    }
    for (target, key, value) in sets {
        let object = match target {
            BindTarget::Object { artboard, index } => file
                .artboards
                .get_mut(artboard)
                .and_then(|a| a.objects.get_mut(index))
                .and_then(Option::as_mut),
            BindTarget::Bindable {
                artboard,
                machine,
                index,
            } => file
                .artboards
                .get_mut(artboard)
                .and_then(|a| a.state_machines.get_mut(machine))
                .and_then(|m| m.bindables.get_mut(index)),
            BindTarget::Other => None,
        };
        if let Some(object) = object {
            object.set(key, value);
        }
    }
    warnings
}

/// The value view model property `property` holds: `set`'s (its instance
/// value), else its type's initial one; `None` for kinds not bound here.
fn value_of(property: &Object, set: Option<&Object>) -> Option<Value> {
    let (key, initial) = match property.ty {
        t::VIEW_MODEL_PROPERTY_NUMBER => (
            p::VIEW_MODEL_INSTANCE_NUMBER_PROPERTY_VALUE,
            Value::Double(0.0),
        ),
        t::VIEW_MODEL_PROPERTY_BOOLEAN => (
            p::VIEW_MODEL_INSTANCE_BOOLEAN_PROPERTY_VALUE,
            Value::Bool(false),
        ),
        t::VIEW_MODEL_PROPERTY_COLOR => (
            p::VIEW_MODEL_INSTANCE_COLOR_PROPERTY_VALUE,
            Value::Color(0xFF00_0000),
        ),
        t::VIEW_MODEL_PROPERTY_STRING => (
            p::VIEW_MODEL_INSTANCE_STRING_PROPERTY_VALUE,
            Value::Bytes(Vec::new()),
        ),
        ty if crate::file::is_a(ty, t::VIEW_MODEL_PROPERTY_ENUM) => (
            p::VIEW_MODEL_INSTANCE_ENUM_PROPERTY_VALUE,
            Value::Uint(0xFFFF_FFFF),
        ),
        t::VIEW_MODEL_PROPERTY_TRIGGER => (
            p::VIEW_MODEL_INSTANCE_TRIGGER_PROPERTY_VALUE,
            Value::Uint(0),
        ),
        _ => return None,
    };
    Some(set.and_then(|s| s.value(key).cloned()).unwrap_or(initial))
}

/// `value` as property `key` is written.
fn coerce(value: Value, key: u16) -> Option<Value> {
    let number = |v: &Value| match *v {
        Value::Double(d) => Some(d),
        Value::Uint(u) => Some(u as f32),
        Value::Bool(b) => Some(f32::from(u8::from(b))),
        _ => None,
    };
    Some(match defs::field(key)? {
        Field::Double => Value::Double(number(&value)?),
        Field::Uint => match value {
            Value::Uint(u) => Value::Uint(u),
            ref v => Value::Uint(number(v)?.max(0.0) as u64),
        },
        Field::Bool => match value {
            Value::Bool(b) => Value::Bool(b),
            ref v => Value::Bool(number(v)? != 0.0),
        },
        Field::Color => match value {
            Value::Color(c) => Value::Color(c),
            _ => return None,
        },
        Field::String | Field::Bytes => match value {
            Value::Bytes(b) => Value::Bytes(b),
            ref v => Value::Bytes(number(v)?.to_string().into_bytes()),
        },
    })
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
