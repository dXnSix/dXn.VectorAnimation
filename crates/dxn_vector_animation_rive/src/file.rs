//! The `.riv` binary format: the header, its table of contents and the
//! objects, put together as rive-runtime's import stack puts them.

use std::collections::{BTreeMap, HashMap};

use dxn_vector_animation::Error;

use crate::defs::{self, Field, types};

/// The bytes every Rive file starts with.
pub(crate) const FINGERPRINT: &[u8] = b"RIVE";

/// The major version of the format this importer reads.
const MAJOR_VERSION: u64 = 7;

/// A property's value as the file writes it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Uint(u64),
    Bool(bool),
    Double(f32),
    Bytes(Vec<u8>),
    Color(u32),
}

/// An object of the file: its type and the properties it writes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Object {
    pub ty: u16,
    pub props: Vec<(u16, Value)>,
}

/// The value ids take when they point at nothing (`Core.missingId`).
const MISSING_ID: u64 = 0xFFFF_FFFF;

impl Object {
    /// Whether the object is of type `base` or one extending it.
    pub fn is(&self, base: u16) -> bool {
        is_a(self.ty, base)
    }

    fn get(&self, key: u16) -> Option<&Value> {
        self.props.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    /// Whether the object writes property `key`.
    pub fn has(&self, key: u16) -> bool {
        self.get(key).is_some()
    }

    /// A number, bool (0 or 1) or color property, its initial value when
    /// the object does not write it.
    pub fn num(&self, key: u16) -> f64 {
        match self.get(key) {
            Some(Value::Uint(v)) => *v as f64,
            Some(Value::Bool(v)) => f64::from(u8::from(*v)),
            Some(Value::Double(v)) => shortest(*v),
            Some(Value::Color(v)) => f64::from(*v),
            _ => defs::initial(key).unwrap_or(0.0),
        }
    }

    /// A uint property (its initial value when not written).
    pub fn uint(&self, key: u16) -> u64 {
        match self.get(key) {
            Some(Value::Uint(v)) => *v,
            Some(Value::Bool(v)) => u64::from(*v),
            _ => defs::initial(key).map_or(0, |v| v as u64),
        }
    }

    /// An id property: an index into a list, `None` when it points at
    /// nothing.
    pub fn id(&self, key: u16) -> Option<usize> {
        let v = self.uint(key);
        (v != MISSING_ID).then(|| usize::try_from(v).ok()).flatten()
    }

    /// A bool property.
    pub fn flag(&self, key: u16) -> bool {
        self.num(key) != 0.0
    }

    /// A color property (ARGB).
    pub fn color(&self, key: u16) -> u32 {
        match self.get(key) {
            Some(Value::Color(v)) => *v,
            Some(Value::Uint(v)) => *v as u32,
            _ => defs::initial(key).map_or(0, |v| v as u32),
        }
    }

    /// A string property.
    pub fn string(&self, key: u16) -> String {
        match self.get(key) {
            Some(Value::Bytes(b)) => String::from_utf8_lossy(b).into_owned(),
            _ => String::new(),
        }
    }

    /// A bytes property.
    pub fn bytes(&self, key: u16) -> &[u8] {
        match self.get(key) {
            Some(Value::Bytes(b)) => b,
            _ => &[],
        }
    }
}

/// The 32-bit float `v` as the shortest decimal reading back as it (0.42,
/// not 0.41999998688697815): what the author typed.
fn shortest(v: f32) -> f64 {
    v.to_string().parse().unwrap_or(f64::from(v))
}

/// Whether type `ty` is `base` or extends it.
pub(crate) fn is_a(ty: u16, base: u16) -> bool {
    let mut t = Some(ty);
    // Definitions extend one another a few levels deep.
    for _ in 0..32 {
        match t {
            Some(x) if x == base => return true,
            Some(x) => t = defs::parent(x),
            None => return false,
        }
    }
    false
}

/// A Rive file: its artboards and assets.
#[derive(Debug, Default)]
pub(crate) struct File {
    pub major: u64,
    pub minor: u64,
    pub artboards: Vec<Artboard>,
    pub assets: Vec<Asset>,
    /// Objects of types the runtime of the tables' tag does not know, by
    /// type key.
    pub unknown: BTreeMap<u64, usize>,
}

/// An artboard: its objects (index 0 is the artboard), animations and state
/// machines.
#[derive(Debug)]
pub(crate) struct Artboard {
    pub objects: Vec<Option<Object>>,
    pub animations: Vec<Animation>,
    pub state_machines: Vec<StateMachine>,
}

impl Artboard {
    /// The artboard's own object.
    pub fn object(&self) -> &Object {
        // The reader opens an artboard with its object.
        self.objects
            .first()
            .and_then(Option::as_ref)
            .unwrap_or(&ARTBOARD)
    }

    /// Its name.
    pub fn name(&self) -> String {
        self.object().string(defs::props::COMPONENT_NAME)
    }
}

static ARTBOARD: Object = Object {
    ty: types::ARTBOARD,
    props: Vec::new(),
};

/// A linear animation and the properties it keys.
#[derive(Debug)]
pub(crate) struct Animation {
    pub object: Object,
    pub keyed: Vec<KeyedObject>,
}

/// The properties an animation keys on one object.
#[derive(Debug)]
pub(crate) struct KeyedObject {
    /// The object's index in the artboard.
    pub object: Option<usize>,
    pub properties: Vec<KeyedProperty>,
}

/// One property's keyframes.
#[derive(Debug)]
pub(crate) struct KeyedProperty {
    pub key: u16,
    pub keyframes: Vec<Object>,
}

/// A state machine as the file writes it (its own object is read with its
/// states, by phase 3's state machines).
#[derive(Debug)]
pub(crate) struct StateMachine {
    pub inputs: Vec<Option<Object>>,
    pub layers: Vec<Layer>,
    pub listeners: Vec<Listener>,
}

/// A layer of a state machine: its states (index into it by the
/// transitions' `stateToId`).
#[derive(Debug)]
pub(crate) struct Layer {
    pub states: Vec<State>,
}

/// A state, its transitions and its parts (a blend state's animations).
#[derive(Debug)]
pub(crate) struct State {
    /// `None` for a state of a type the runtime does not know: it can be
    /// gone to and from but plays nothing.
    pub object: Option<Object>,
    pub transitions: Vec<Transition>,
    pub parts: Vec<Object>,
}

/// A transition and its conditions.
#[derive(Debug)]
pub(crate) struct Transition {
    pub object: Object,
    pub conditions: Vec<Object>,
}

/// A listener and its actions.
#[derive(Debug)]
pub(crate) struct Listener {
    pub actions: Vec<Object>,
}

/// An asset, and its bytes when the file embeds them.
#[derive(Debug)]
pub(crate) struct Asset {
    pub object: Object,
    pub contents: Option<Vec<u8>>,
}

/// What an object of a type the runtime does not know goes to: the most
/// recently opened of these (rive-runtime's `readNullObject`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Open {
    Artboard,
    KeyedProperty,
    StateMachine,
    Layer,
}

/// Reads the file `data`.
pub(crate) fn read(data: &[u8]) -> Result<File, Error> {
    let mut r = Reader { data, at: 0 };
    if r.take(4)? != FINGERPRINT {
        return Err(Error::Invalid("not a Rive file".into()));
    }
    let major = r.varuint()?;
    let minor = r.varuint()?;
    if major != MAJOR_VERSION {
        return Err(Error::Unsupported(format!(
            "Rive format {major}.{minor} (this reads {MAJOR_VERSION})"
        )));
    }
    let _file_id = r.varuint()?;
    let toc = r.toc()?;

    let mut file = File {
        major,
        minor,
        ..File::default()
    };
    let mut recent: Vec<Open> = Vec::new();
    let open = |recent: &mut Vec<Open>, o: Open| {
        recent.retain(|r| *r != o);
        recent.push(o);
    };
    while !r.done() {
        let (ty, object) = r.object(&toc)?;
        let Some(object) = object else {
            *file.unknown.entry(ty).or_default() += 1;
            let target = recent.last().copied();
            match target {
                Some(Open::Artboard) => {
                    if let Some(a) = file.artboards.last_mut() {
                        a.objects.push(None);
                    }
                }
                Some(Open::StateMachine) => {
                    if let Some(sm) = last_machine(&mut file) {
                        sm.inputs.push(None);
                    }
                }
                Some(Open::Layer) => {
                    if let Some(layer) = last_machine(&mut file).and_then(|sm| sm.layers.last_mut())
                    {
                        layer.states.push(State {
                            object: None,
                            transitions: Vec::new(),
                            parts: Vec::new(),
                        });
                    }
                }
                Some(Open::KeyedProperty) | None => {}
            }
            continue;
        };
        place(&mut file, &mut recent, &open, object);
    }
    Ok(file)
}

fn last_machine(file: &mut File) -> Option<&mut StateMachine> {
    file.artboards.last_mut()?.state_machines.last_mut()
}

/// Puts `object` where rive-runtime's importers put it.
fn place(
    file: &mut File,
    recent: &mut Vec<Open>,
    open: &impl Fn(&mut Vec<Open>, Open),
    object: Object,
) {
    let ty = object.ty;
    if ty == types::ARTBOARD {
        file.artboards.push(Artboard {
            objects: vec![Some(object)],
            animations: Vec::new(),
            state_machines: Vec::new(),
        });
        open(recent, Open::Artboard);
        return;
    }
    if is_a(ty, types::FILE_ASSET) {
        file.assets.push(Asset {
            object,
            contents: None,
        });
        return;
    }
    if ty == types::FILE_ASSET_CONTENTS {
        if let Some(asset) = file.assets.last_mut() {
            asset.contents = Some(
                object
                    .bytes(defs::props::FILE_ASSET_CONTENTS_BYTES)
                    .to_vec(),
            );
        }
        return;
    }
    let Some(artboard) = file.artboards.last_mut() else {
        // Backboard, view models, enums and converters: the file's, not an
        // artboard's.
        return;
    };
    if is_a(ty, types::COMPONENT) || is_a(ty, types::KEY_FRAME_INTERPOLATOR) {
        artboard.objects.push(Some(object));
    } else if ty == types::LINEAR_ANIMATION {
        artboard.animations.push(Animation {
            object,
            keyed: Vec::new(),
        });
    } else if ty == types::KEYED_OBJECT {
        if let Some(a) = artboard.animations.last_mut() {
            a.keyed.push(KeyedObject {
                object: object.id(defs::props::KEYED_OBJECT_OBJECT_ID),
                properties: Vec::new(),
            });
        }
    } else if ty == types::KEYED_PROPERTY {
        if let Some(k) = artboard
            .animations
            .last_mut()
            .and_then(|a| a.keyed.last_mut())
        {
            let key = u16::try_from(object.uint(defs::props::KEYED_PROPERTY_PROPERTY_KEY))
                .map_or(0, defs::canonical);
            k.properties.push(KeyedProperty {
                key,
                keyframes: Vec::new(),
            });
        }
        open(recent, Open::KeyedProperty);
    } else if is_a(ty, types::KEY_FRAME) {
        if let Some(p) = artboard
            .animations
            .last_mut()
            .and_then(|a| a.keyed.last_mut())
            .and_then(|k| k.properties.last_mut())
        {
            p.keyframes.push(object);
        }
    } else if ty == types::STATE_MACHINE {
        artboard.state_machines.push(StateMachine {
            inputs: Vec::new(),
            layers: Vec::new(),
            listeners: Vec::new(),
        });
        open(recent, Open::StateMachine);
    } else if let Some(sm) = artboard.state_machines.last_mut() {
        if is_a(ty, types::STATE_MACHINE_INPUT) {
            sm.inputs.push(Some(object));
        } else if ty == types::STATE_MACHINE_LAYER {
            sm.layers.push(Layer { states: Vec::new() });
            open(recent, Open::Layer);
        } else if is_a(ty, types::LAYER_STATE) {
            if let Some(layer) = sm.layers.last_mut() {
                layer.states.push(State {
                    object: Some(object),
                    transitions: Vec::new(),
                    parts: Vec::new(),
                });
            }
        } else if is_a(ty, types::STATE_TRANSITION) {
            if let Some(state) = sm.layers.last_mut().and_then(|l| l.states.last_mut()) {
                state.transitions.push(Transition {
                    object,
                    conditions: Vec::new(),
                });
            }
        } else if is_a(ty, types::TRANSITION_CONDITION) || is_a(ty, types::TRANSITION_COMPARATOR) {
            if let Some(t) = sm
                .layers
                .last_mut()
                .and_then(|l| l.states.last_mut())
                .and_then(|s| s.transitions.last_mut())
            {
                t.conditions.push(object);
            }
        } else if is_a(ty, types::STATE_MACHINE_LISTENER) {
            sm.listeners.push(Listener {
                actions: Vec::new(),
            });
        } else if is_a(ty, types::LISTENER_ACTION) || is_a(ty, types::LISTENER_INPUT_TYPE) {
            if let Some(l) = sm.listeners.last_mut() {
                l.actions.push(object);
            }
        } else if is_a(ty, types::BLEND_ANIMATION)
            && let Some(state) = sm.layers.last_mut().and_then(|l| l.states.last_mut())
        {
            state.parts.push(object);
        }
    }
}

/// Reads bytes in turn.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

fn truncated() -> Error {
    Error::Invalid("the file ends early".into())
}

impl Reader<'_> {
    fn done(&self) -> bool {
        self.at >= self.data.len()
    }

    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let end = self.at.checked_add(n).ok_or_else(truncated)?;
        let bytes = self.data.get(self.at..end).ok_or_else(truncated)?;
        self.at = end;
        Ok(bytes)
    }

    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A LEB128 unsigned integer.
    fn varuint(&mut self) -> Result<u64, Error> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            value |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(Error::Invalid("a number longer than 64 bits".into()))
    }

    fn key(&mut self) -> Result<u16, Error> {
        u16::try_from(self.varuint()?)
            .map_err(|_| Error::Invalid("a property key over 65535".into()))
    }

    /// The table of contents: how each property the file writes is
    /// written, for those the importer does not know.
    fn toc(&mut self) -> Result<HashMap<u16, Field>, Error> {
        let mut keys = Vec::new();
        loop {
            let key = self.key()?;
            if key == 0 {
                break;
            }
            keys.push(key);
        }
        // Two bits a key, four keys a 32-bit word (in its lowest byte), as
        // rive-runtime reads them.
        let mut toc = HashMap::with_capacity(keys.len());
        let (mut word, mut bit) = (0u32, 8);
        for key in keys {
            if bit == 8 {
                word = self.u32()?;
                bit = 0;
            }
            let field = match (word >> bit) & 3 {
                0 => Field::Uint,
                1 => Field::Bytes,
                2 => Field::Double,
                _ => Field::Color,
            };
            toc.insert(key, field);
            bit += 2;
        }
        Ok(toc)
    }

    /// An object: its type and, when the runtime knows the type, the object.
    fn object(&mut self, toc: &HashMap<u16, Field>) -> Result<(u64, Option<Object>), Error> {
        let ty = self.varuint()?;
        let known = u16::try_from(ty)
            .ok()
            .filter(|t| defs::type_name(*t).is_some() && !defs::is_abstract(*t));
        let mut props = Vec::new();
        loop {
            let key = self.key()?;
            if key == 0 {
                break;
            }
            let field = defs::field(key)
                .or_else(|| toc.get(&key).copied())
                .ok_or_else(|| {
                    Error::Invalid(format!(
                        "property {key} is in neither the runtime nor the file's table"
                    ))
                })?;
            let value = match field {
                Field::Uint => Value::Uint(self.varuint()?),
                Field::Bool => Value::Bool(self.byte()? == 1),
                Field::Double => Value::Double(f32::from_bits(self.u32()?)),
                Field::Color => Value::Color(self.u32()?),
                Field::String | Field::Bytes => {
                    let n = usize::try_from(self.varuint()?).map_err(|_| truncated())?;
                    Value::Bytes(self.take(n)?.to_vec())
                }
            };
            if known.is_some() {
                props.push((defs::canonical(key), value));
            }
        }
        Ok((ty, known.map(|ty| Object { ty, props })))
    }
}
