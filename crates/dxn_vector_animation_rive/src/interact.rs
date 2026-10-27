//! A Rive file's default state machine as a user plays it: its inputs, its
//! listeners on the artboard's shapes, and pointer events turned into
//! changes of its inputs as Rive's runtime turns them; each change read
//! into the scene again.

use dxn_vector_animation::{Cue, Error, Import, Limits, Point, Rect};

use crate::{
    Aligned, InputChange, InputValue, Options,
    defs::{props as p, types as t},
    file::{self, Object},
    read_options,
};

/// The default state machine of an artboard: what a player sets and what
/// pointers do to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    /// Its name.
    pub name: String,
    /// Its inputs, in order.
    pub inputs: Vec<Input>,
    /// Its listeners.
    pub listeners: Vec<Listener>,
}

/// An input of a state machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    /// Its name.
    pub name: String,
    /// What it holds, and its value at first.
    pub kind: InputKind,
}

/// What an input holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputKind {
    /// On or off, at first as given.
    Bool(bool),
    /// A number, at first as given.
    Number(f64),
    /// A trigger, fired.
    Trigger,
}

/// A listener: what pointer events over its target do.
#[derive(Debug, Clone, PartialEq)]
pub struct Listener {
    /// The scene's groups it listens on: the ids of the shapes it hits
    /// (the target's own, or those of the shapes under it), whose outlines
    /// a pointer falls in.
    pub targets: Vec<String>,
    /// The events it acts on.
    pub on: Vec<PointerEvent>,
    /// What it does to the inputs.
    pub actions: Vec<Action>,
}

/// A pointer event over a listener's target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEvent {
    /// Moved onto it.
    Enter,
    /// Moved off it.
    Exit,
    /// Pressed on it.
    Down,
    /// Released on it.
    Up,
    /// Moved on it.
    Move,
    /// Pressed and released on it.
    Click,
}

/// What a listener does.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Changes an input.
    Input {
        /// The input's name.
        input: String,
        /// What it sets it to.
        change: Change,
    },
    /// Fires an event (reported as a cue).
    Fire {
        /// The event, by its index in the artboard.
        event: usize,
    },
    /// Moves a node to the pointer.
    Align {
        /// The node, by its index in the artboard.
        node: usize,
        /// Whether it keeps its offset from the pointer, moving as far as
        /// the pointer does.
        keep_offset: bool,
    },
}

/// A change a listener makes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    /// Sets it (or fires a trigger).
    Set(InputValue),
    /// Turns a bool input over.
    Toggle,
}

/// What a pointer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// Moves to the point.
    Move,
    /// Presses there.
    Down,
    /// Releases there.
    Up,
    /// Leaves the scene (a mouse leaving its view).
    Exit,
    /// Releases there and leaves (a finger lifted).
    Lift,
}

/// The default state machine of the artboard `options` names in the Rive
/// file `data`, if it has one.
pub fn machine(data: &[u8], options: &Options) -> Result<Option<Machine>, Error> {
    let file = file::read(data)?;
    let Some(artboard) = chosen(&file, options)? else {
        return Ok(None);
    };
    let machines = &artboard.state_machines;
    let Some(sm) = artboard
        .object()
        .id(p::ARTBOARD_DEFAULT_STATE_MACHINE_ID)
        .and_then(|i| machines.get(i))
        .or(machines.first())
    else {
        return Ok(None);
    };
    let inputs: Vec<Input> = sm
        .inputs
        .iter()
        .map(|i| match i {
            Some(o) => Input {
                name: o.string(p::STATE_MACHINE_COMPONENT_NAME),
                kind: match o.ty {
                    t::STATE_MACHINE_BOOL => InputKind::Bool(o.flag(p::STATE_MACHINE_BOOL_VALUE)),
                    t::STATE_MACHINE_NUMBER => {
                        InputKind::Number(o.num(p::STATE_MACHINE_NUMBER_VALUE))
                    }
                    _ => InputKind::Trigger,
                },
            },
            None => Input {
                name: String::new(),
                kind: InputKind::Trigger,
            },
        })
        .collect();
    let name_of = |o: &Object| {
        o.id(p::LISTENER_INPUT_CHANGE_INPUT_ID)
            .and_then(|i| inputs.get(i))
            .map(|i| i.name.clone())
    };
    let event = |v: u64| match v {
        0 => Some(PointerEvent::Enter),
        1 => Some(PointerEvent::Exit),
        2 => Some(PointerEvent::Down),
        3 => Some(PointerEvent::Up),
        4 => Some(PointerEvent::Move),
        6 => Some(PointerEvent::Click),
        _ => None,
    };
    let objects = &artboard.objects;
    // Whether `ancestor` is object `i` or holds it.
    let under = |ancestor: usize, mut i: usize| {
        for _ in 0..=Limits::default().max_depth {
            if i == ancestor {
                return true;
            }
            match objects
                .get(i)
                .and_then(Option::as_ref)
                .filter(|o| o.is(t::COMPONENT))
                .and_then(|o| o.id(p::COMPONENT_PARENT_ID))
            {
                Some(pi) if pi != i => i = pi,
                _ => return false,
            }
        }
        false
    };
    let mut listeners = Vec::new();
    for l in &sm.listeners {
        let Some(target) = l.object.id(p::STATE_MACHINE_LISTENER_TARGET_ID) else {
            continue;
        };
        // Rive hits the shapes under a target, or the target itself.
        let targets: Vec<String> = (1..objects.len())
            .filter(|&i| objects[i].as_ref().is_some_and(|o| o.is(t::SHAPE)) && under(target, i))
            .map(|i| i.to_string())
            .collect();
        let mut on = Vec::new();
        if l.object.ty == t::STATE_MACHINE_LISTENER_SINGLE {
            on.extend(event(
                l.object
                    .uint(p::STATE_MACHINE_LISTENER_SINGLE_LISTENER_TYPE_VALUE),
            ));
        }
        let mut actions = Vec::new();
        for a in &l.actions {
            if a.is(t::LISTENER_INPUT_TYPE) {
                on.extend(event(a.uint(p::LISTENER_INPUT_TYPE_LISTENER_TYPE_VALUE)));
                continue;
            }
            if a.is(t::LISTENER_FIRE_EVENT) {
                if let Some(event) = a.id(p::LISTENER_FIRE_EVENT_EVENT_ID) {
                    actions.push(Action::Fire { event });
                }
                continue;
            }
            if a.is(t::LISTENER_ALIGN_TARGET) {
                if let Some(node) = a.id(p::LISTENER_ALIGN_TARGET_TARGET_ID).filter(|&n| {
                    objects
                        .get(n)
                        .and_then(Option::as_ref)
                        .is_some_and(|o| o.is(t::NODE))
                }) {
                    actions.push(Action::Align {
                        node,
                        keep_offset: a.flag(p::LISTENER_ALIGN_TARGET_PRESERVE_OFFSET),
                    });
                }
                continue;
            }
            let Some(input) = name_of(a) else { continue };
            let change = match a.ty {
                t::LISTENER_BOOL_CHANGE => match a.uint(p::LISTENER_BOOL_CHANGE_VALUE) {
                    0 => Change::Set(InputValue::Bool(false)),
                    1 => Change::Set(InputValue::Bool(true)),
                    _ => Change::Toggle,
                },
                t::LISTENER_NUMBER_CHANGE => {
                    Change::Set(InputValue::Number(a.num(p::LISTENER_NUMBER_CHANGE_VALUE)))
                }
                t::LISTENER_TRIGGER_CHANGE => Change::Set(InputValue::Fire),
                _ => continue,
            };
            actions.push(Action::Input { input, change });
        }
        on.dedup();
        if !on.is_empty() && !actions.is_empty() && !targets.is_empty() {
            listeners.push(Listener {
                targets,
                on,
                actions,
            });
        }
    }
    Ok(Some(Machine {
        name: sm.object.string(p::ANIMATION_NAME),
        inputs,
        listeners,
    }))
}

/// The artboard `options` names in `file` (its first by default).
fn chosen<'a>(
    file: &'a file::File,
    options: &Options,
) -> Result<Option<&'a file::Artboard>, Error> {
    match &options.artboard {
        Some(name) => file
            .artboards
            .iter()
            .find(|a| a.name() == *name)
            .map(Some)
            .ok_or_else(|| Error::Invalid(format!("no artboard named {name:?}"))),
        None => Ok(file.artboards.first()),
    }
}

/// The objects of the artboard `options` names in the Rive file `data`.
fn artboard_objects(data: &[u8], options: &Options) -> Result<Vec<Option<Object>>, Error> {
    let file = file::read(data)?;
    Ok(chosen(&file, options)?
        .map(|a| a.objects.clone())
        .unwrap_or_default())
}

/// A Rive file being played: its scene with the changes made so far to its
/// default state machine's inputs, by hand or by pointer events over its
/// listeners. Each change is made at a time of the scene's timeline at or
/// after the changes before it, and the file is read again with it.
#[derive(Debug, Clone)]
pub struct Interaction {
    data: Vec<u8>,
    limits: Limits,
    options: Options,
    machine: Option<Machine>,
    import: Import,
    /// Whether the pointer was over each listener's target, and pressed
    /// there, and where it was at the listener's last event.
    over: Vec<bool>,
    pressed: Vec<bool>,
    previous: Vec<Point>,
    /// The events listeners fired, not yet taken.
    fired: Vec<Cue>,
    /// The artboard's objects, for the events' names and properties.
    objects: Vec<Option<file::Object>>,
}

impl Interaction {
    /// The file `data` read as `options` say.
    pub fn new(data: Vec<u8>, limits: Limits, options: Options) -> Result<Interaction, Error> {
        let import = read_options(&data, &limits, &options)?;
        let machine = machine(&data, &options)?;
        let n = machine.as_ref().map_or(0, |m| m.listeners.len());
        let objects = artboard_objects(&data, &options)?;
        Ok(Interaction {
            data,
            limits,
            options,
            machine,
            import,
            over: vec![false; n],
            pressed: vec![false; n],
            previous: vec![Point::new(0.0, 0.0); n],
            fired: Vec::new(),
            objects,
        })
    }

    /// The events the listeners fired since they were last taken, each a
    /// cue at the time it was fired.
    pub fn take_fired(&mut self) -> Vec<Cue> {
        std::mem::take(&mut self.fired)
    }

    /// The scene as the changes so far make it.
    pub fn import(&self) -> &Import {
        &self.import
    }

    /// The default state machine.
    pub fn machine(&self) -> Option<&Machine> {
        self.machine.as_ref()
    }

    /// The changes made so far.
    pub fn changes(&self) -> &[InputChange] {
        &self.options.inputs
    }

    /// The nodes moved to the pointer so far.
    pub fn aligned(&self) -> &[Aligned] {
        &self.options.aligned
    }

    /// Sets input `input` to `value` at `time`, reading the scene again.
    pub fn set(&mut self, time: f64, input: &str, value: InputValue) -> Result<(), Error> {
        self.make(
            vec![InputChange {
                time,
                input: input.to_owned(),
                value,
            }],
            Vec::new(),
        )
    }

    /// A pointer doing `kind` at `point` (in the scene's coordinates) at
    /// `time`: the listeners over whose targets it is act as Rive's runtime
    /// has them act (entering, leaving, moving, pressing, releasing,
    /// clicking). Whether the inputs changed (and the scene was read
    /// again).
    ///
    /// A press or a release (a lift too) acts at once, as Rive's web runtime
    /// has it (advancing the state machine by nothing): from the frame the
    /// pointer fell in, where a move or a leaving, like an input set by
    /// hand, acts from the frame after.
    pub fn pointer(&mut self, time: f64, kind: Pointer, point: Point) -> Result<bool, Error> {
        let Some(machine) = &self.machine else {
            return Ok(false);
        };
        let acts = match kind {
            Pointer::Move | Pointer::Exit => time,
            Pointer::Down | Pointer::Up | Pointer::Lift => {
                let frame = (time.max(0.0) / crate::convert::DT - 1e-6).ceil();
                (frame - 1.0).max(0.0) * crate::convert::DT
            }
        };
        // Rive hits the box 2 units around the pointer, its sides rounded
        // (`Shape::hitTestHiFi`).
        let (x0, y0) = ((point.x - 2.0).round(), (point.y - 2.0).round());
        let around = Rect::new(
            x0,
            y0,
            (point.x + 2.0).round() - x0,
            (point.y + 2.0).round() - y0,
        );
        let mut changes = Vec::new();
        let mut aligned = Vec::new();
        let steps: &[Pointer] = match kind {
            Pointer::Lift => &[Pointer::Up, Pointer::Exit],
            _ => std::slice::from_ref(&kind),
        };
        for (&kind, (k, l)) in steps
            .iter()
            .flat_map(|s| std::iter::repeat(s).zip(machine.listeners.iter().enumerate()))
        {
            let scene = &self.import.scene;
            let hit = kind != Pointer::Exit
                && l.targets.iter().any(|id| scene.hit_rect(id, around, time));
            let mut events = Vec::new();
            if hit && !self.over[k] {
                events.push(PointerEvent::Enter);
                // Entering starts its moves afresh.
                self.previous[k] = point;
            }
            if !hit && self.over[k] {
                events.push(PointerEvent::Exit);
            }
            if hit {
                match kind {
                    Pointer::Move => events.push(PointerEvent::Move),
                    Pointer::Down => {
                        events.push(PointerEvent::Down);
                        self.pressed[k] = true;
                    }
                    Pointer::Up => {
                        events.push(PointerEvent::Up);
                        if self.pressed[k] {
                            events.push(PointerEvent::Click);
                        }
                    }
                    Pointer::Exit | Pointer::Lift => {}
                }
            }
            if kind != Pointer::Move && kind != Pointer::Down {
                self.pressed[k] = false;
            }
            self.over[k] = hit;
            for event in events {
                if !l.on.contains(&event) {
                    continue;
                }
                for a in &l.actions {
                    match a {
                        Action::Input { input, change } => {
                            let value = match change {
                                Change::Set(v) => *v,
                                Change::Toggle => {
                                    InputValue::Bool(!self.bool_at(input, acts, &changes))
                                }
                            };
                            changes.push(InputChange {
                                time: acts,
                                input: input.clone(),
                                value,
                            });
                        }
                        &Action::Fire { event } => {
                            self.fired
                                .extend(crate::cues::cue(&self.objects, event, time));
                        }
                        &Action::Align { node, keep_offset } => aligned.push(Aligned {
                            time,
                            node,
                            to: point,
                            from: keep_offset.then_some(self.previous[k]),
                        }),
                    }
                }
            }
            self.previous[k] = point;
        }
        if changes.is_empty() && aligned.is_empty() {
            return Ok(false);
        }
        self.make(changes, aligned)?;
        Ok(true)
    }

    /// The value of bool input `input` at `time`, with `pending` changes
    /// made after those so far.
    fn bool_at(&self, input: &str, time: f64, pending: &[InputChange]) -> bool {
        let mut v = self
            .machine
            .as_ref()
            .and_then(|m| m.inputs.iter().find(|i| i.name == input))
            .is_some_and(|i| matches!(i.kind, InputKind::Bool(true)));
        for c in self.options.inputs.iter().chain(pending) {
            if c.input == input
                && c.time <= time
                && let InputValue::Bool(b) = c.value
            {
                v = b;
            }
        }
        v
    }

    fn make(
        &mut self,
        mut changes: Vec<InputChange>,
        mut aligned: Vec<Aligned>,
    ) -> Result<(), Error> {
        // Never before what was made already.
        let last = self.options.inputs.last().map_or(0.0, |c| c.time);
        for c in &mut changes {
            c.time = c.time.max(last);
        }
        let last = self.options.aligned.last().map_or(0.0, |a| a.time);
        for a in &mut aligned {
            a.time = a.time.max(last);
        }
        let mut options = self.options.clone();
        options.inputs.extend(changes);
        options.aligned.extend(aligned);
        self.import = read_options(&self.data, &self.limits, &options)?;
        self.options = options;
        Ok(())
    }
}
