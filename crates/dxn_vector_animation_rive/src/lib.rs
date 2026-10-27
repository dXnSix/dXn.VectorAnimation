//! Reads Rive files (`.riv`) into a [`dxn_vector_animation`] scene.
//!
//! The file's format is rive-runtime's (format 7): its objects are read
//! with the types and properties of the runtime tag [`RUNTIME_TAG`], and
//! put together as the runtime's importers put them. One artboard (the
//! first, or one named in [`Options`]) becomes the scene: its drawables in
//! Rive's draw order (the first in the file on top), each under its
//! ancestors' transforms (translation, rotation, scale) and the clipping
//! shapes over it; their fills and strokes (solid colors, linear and radial
//! gradients, caps, joins, fill rules, trim paths, dashes, blend modes) over
//! outlines of points (straight vertices rounding their corners, mirrored,
//! asymmetric and detached cubic vertices), rectangles, ellipses, polygons,
//! stars and triangles; embedded images; and opacity, multiplied down the
//! tree into each fill and stroke as Rive applies it.
//!
//! The artboard's default state machine plays first, as Rive's runtime plays
//! it with its inputs as they are (its layers together, transitions whose
//! conditions hold, exit times and mixes), under a marker of its name: the
//! timeline's span, repeating from where it settles. The linear animations
//! lie after it one after another, each under a marker of its name that
//! carries its loop; without a state machine the timeline's span is the
//! first. A property an animation does not key holds its setup value in
//! that animation's span. Keyframes keep their holds, straight
//! lines and cubic easings; value curves become cubic easings of the change;
//! elastic easings, products of moving opacities, and outlines whose
//! tangents turn are sampled at 60 frames a second, straight between
//! samples, within half of [`Limits::max_keyframes`].
//!
//! Feathered fills and strokes are blurred as Rive's renderer blurs them (a
//! Gaussian of half their strength), offset in the artboard's or the
//! shape's space; inner feathers shade the inside of their outline.
//!
//! Nested artboards are the artboards they show, drawn at their transform
//! and playing their nested animations on the parent's clock in layers, as
//! Rive applies them.
//!
//! Text is shaped with the file's embedded fonts and laid out as Rive lays
//! it out (runs, paragraphs, wrapping, alignment, sizing, origins,
//! overflow); joysticks apply their animations at their values.
//!
//! Bones carry what is under them; skins deform outlines and image meshes
//! by their bones' weights, as Rive's `Weight::deform` does, and a mesh
//! draws its image through its triangles.
//!
//! Constraints (translation, rotation, scale, transform, distance, IK and
//! follow-path) move what they constrain as Rive's runtime moves it: each
//! component's world transform in the runtime's update order, baked into
//! a matrix where a constraint changes it.
//!
//! Draw rules move drawables before or after their targets, as Rive sorts
//! its draw order; where the active target is keyed, a drawable is drawn in
//! each place it takes while it is there.
//!
//! Text modifier groups move, turn, scale and fade each glyph as much as
//! their ranges cover its characters (by characters, words or lines, with
//! falloffs), as Rive's runtime applies them.
//!
//! A nested artboard's state machine plays the same way, its inputs as the
//! parent's animations set them.
//!
//! What a file holds beyond that (state machines' listeners and input
//! changes, text variation and
//! follow-path modifiers, text ellipses, layout,
//! data binding, scripting, audio) is left out with a warning in
//! [`Import::warnings`].

mod convert;
mod cues;
mod curve;
mod data;
mod defs;
mod file;
mod geometry;
mod interact;
mod machine;
mod modifiers;
mod order;
mod rig;
mod text;

use dxn_vector_animation::{Error, Import, Limits, Point};

pub use data::{Property, PropertyValue, ViewModel};
pub use defs::RUNTIME_TAG;
pub use interact::{
    Action, Change, Input, InputKind, Interaction, Listener, Machine, Pointer, PointerEvent,
    machine,
};

/// What to read of a file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    /// The artboard to read, by name; the file's first when `None`.
    pub artboard: Option<String>,
    /// What is done to the artboard's default state machine, in time order:
    /// the machine plays as Rive plays it with these changes made to its
    /// inputs (what a user's clicks and taps would make).
    pub inputs: Vec<InputChange>,
    /// The nodes its listeners moved to the pointer, in time order.
    pub aligned: Vec<Aligned>,
    /// Reads the file as a player binding no view model instance to the
    /// artboard (Rive's `autoBind: false`): data bindings leave their
    /// targets as the file sets them. By default the artboard's view
    /// model's first instance is bound, as Rive's editor and players that
    /// bind automatically do.
    pub unbound: bool,
    /// Changes made to the view model instance bound, in time order (what
    /// an app sets through Rive's view model instance API): what they are
    /// bound to changes with them, and the state machine's conditions on
    /// them hold as they compare.
    pub values: Vec<ValueChange>,
}

/// A change made to a property of the view model instance bound at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueChange {
    /// The timeline's seconds it is made at.
    pub time: f64,
    /// The property, by name: the names of the nested view model
    /// properties it is in first, `/` between (Rive's property paths).
    pub path: String,
    /// What it is set to, of the property's kind:
    /// [`PropertyValue::Trigger`] fires a trigger.
    pub value: PropertyValue,
}

/// A node a listener moved to the pointer at a time (Rive's
/// `ListenerAlignTarget`): from then on it sits where the pointer is, or
/// moves as far as the pointer did since its last event when it keeps its
/// offset.
#[derive(Debug, Clone, PartialEq)]
pub struct Aligned {
    /// The timeline's seconds it is moved at.
    pub time: f64,
    /// The nested artboards it is in, as [`InputChange::path`] has them.
    pub path: String,
    /// The node, by its index in the artboard (as the scene's group ids
    /// name objects).
    pub node: usize,
    /// Where the pointer is, in the scene's coordinates.
    pub to: Point,
    /// Where it was at its last event, when the node keeps its offset from
    /// it.
    pub from: Option<Point>,
}

/// A change made to a state machine's input at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct InputChange {
    /// The timeline's seconds it is made at.
    pub time: f64,
    /// The nested artboards whose state machine it is made to, from the
    /// artboard read: each the nested artboard's name, or `#` and its index
    /// in the artboard holding it, `/` between (Rive's `setInputAtPath`);
    /// empty for the artboard's default state machine.
    pub path: String,
    /// The input's name.
    pub input: String,
    /// What it is set to.
    pub value: InputValue,
}

/// What an input is set to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputValue {
    /// A bool input on or off.
    Bool(bool),
    /// A number input's value.
    Number(f64),
    /// A trigger fired.
    Fire,
}

/// Whether `data` looks like a Rive file.
pub fn sniff(data: &[u8]) -> bool {
    data.starts_with(file::FINGERPRINT)
}

/// Reads the Rive file `data` with the default [`Limits`].
pub fn read(data: &[u8]) -> Result<Import, Error> {
    read_with(data, &Limits::default())
}

/// Reads the Rive file `data`'s first artboard, refusing it when it is over
/// `limits`.
pub fn read_with(data: &[u8], limits: &Limits) -> Result<Import, Error> {
    read_options(data, limits, &Options::default())
}

/// Reads the artboard of the Rive file `data` that `options` names.
pub fn read_options(data: &[u8], limits: &Limits, options: &Options) -> Result<Import, Error> {
    if data.len() > limits.max_bytes {
        return Err(Error::Limit {
            what: "bytes",
            limit: limits.max_bytes as f64,
        });
    }
    let mut file = file::read(data)?;
    let mut bound = std::collections::BTreeSet::new();
    if !options.unbound {
        let index = match &options.artboard {
            Some(name) => file.artboards.iter().position(|a| a.name() == *name),
            None => Some(0),
        };
        if let Some(index) = index.filter(|&i| i < file.artboards.len()) {
            bound = data::bind_default(&mut file, index, &options.values);
        }
    }
    let (scene, mut warnings) = convert::Converter::run(&file, options, *limits)?;
    warnings.extend(bound);
    scene.check(limits)?;
    Ok(Import {
        scene,
        warnings: warnings.into_iter().collect(),
    })
}

/// The view model the artboard `options` names in the Rive file `data` is
/// bound to (its first instance's values), if it has one.
pub fn view_model(data: &[u8], options: &Options) -> Result<Option<ViewModel>, Error> {
    let file = file::read(data)?;
    let index = match &options.artboard {
        Some(name) => file.artboards.iter().position(|a| a.name() == *name),
        None => Some(0),
    };
    Ok(index.and_then(|i| data::view_model(&file, i)))
}

/// The names of the artboards in the Rive file `data`.
pub fn artboards(data: &[u8]) -> Result<Vec<String>, Error> {
    Ok(file::read(data)?
        .artboards
        .iter()
        .map(file::Artboard::name)
        .collect())
}
