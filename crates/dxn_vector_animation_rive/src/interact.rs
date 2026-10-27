//! A Rive file's default state machine as a user plays it: its inputs, its
//! listeners on the artboard's shapes, and pointer events turned into
//! changes of its inputs as Rive's runtime turns them; each change read
//! into the scene again.

use dxn_vector_animation::{Cue, Error, Import, Limits, Point, Rect};

use crate::{
    Aligned, InputChange, InputValue, Options, PropertyValue, ValueChange,
    defs::{props as p, types as t},
    file::{self, Object},
    read_options,
};

/// A state machine of an artboard: what a player sets and what pointers do
/// to it, and the state machines of its nested artboards.
#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    /// Its name.
    pub name: String,
    /// The nested artboards it plays in, from the file's artboard: each
    /// `#` and its index in the artboard holding it, `/` between (as
    /// [`InputChange::path`] takes them); empty for the artboard's own.
    pub path: String,
    /// Its inputs, in order.
    pub inputs: Vec<Input>,
    /// Its listeners.
    pub listeners: Vec<Listener>,
    /// Its artboard's index in the file, and its own in the artboard.
    at: (usize, usize),
    /// The state machines its nested artboards play (each the first they
    /// play).
    pub nested: Vec<Machine>,
}

impl Machine {
    /// It and the machines nested in it, depth first.
    pub fn all(&self) -> Vec<&Machine> {
        let mut out = vec![self];
        for n in &self.nested {
            out.extend(n.all());
        }
        out
    }
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

/// A listener: what pointer events over its target, or Rive events, do.
#[derive(Debug, Clone, PartialEq)]
pub struct Listener {
    /// What it listens on: the object's index in its artboard.
    pub node: usize,
    /// The scene's groups it listens on: the ids of the shapes it hits
    /// (the target's own, or those of the shapes under it), whose outlines
    /// a pointer falls in.
    pub targets: Vec<String>,
    /// Those of its targets a pointer stops at (Rive's opaque hit targets):
    /// what is behind them is not hit.
    pub opaque: Vec<String>,
    /// The pointer events it acts on.
    pub on: Vec<PointerEvent>,
    /// The Rive events it acts on (`ListenerType::event`): each an event of
    /// the artboard its target is, by index (a nested artboard's, or its
    /// own).
    pub events: Vec<usize>,
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
    /// Moved, pressed on it and not released yet (over it or not), past
    /// the move that started the drag.
    Drag,
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
    /// Changes a property of the view model instance bound
    /// (`ListenerViewModelChange`): to the value its bindable property
    /// holds then, through its binding (a trigger fired, often).
    Value {
        /// The property, by name (as [`ValueChange::path`] has it).
        property: String,
        /// Its bindable property, by index in the state machine.
        bindable: usize,
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

/// How deep nested artboards' state machines are read.
const MAX_NESTING: usize = 8;

/// The default state machine of the artboard `options` names in the Rive
/// file `data`, if it has one, with those of its nested artboards.
pub fn machine(data: &[u8], options: &Options) -> Result<Option<Machine>, Error> {
    let file = file::read(data)?;
    let Some(artboard) = chosen(&file, options)? else {
        return Ok(None);
    };
    let machines = &artboard.state_machines;
    let Some(index) = artboard
        .object()
        .id(p::ARTBOARD_DEFAULT_STATE_MACHINE_ID)
        .filter(|&i| i < machines.len())
        .or((!machines.is_empty()).then_some(0))
    else {
        return Ok(None);
    };
    Ok(Some(read_machine(&file, artboard, index, "", "", 0)))
}

/// State machine `index` of `artboard`, at `path`, its scene ids under
/// `prefix`.
fn read_machine(
    file: &file::File,
    artboard: &file::Artboard,
    index: usize,
    path: &str,
    prefix: &str,
    depth: usize,
) -> Machine {
    let sm = &artboard.state_machines[index];
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
        12 => Some(PointerEvent::Drag),
        _ => None,
    };
    let objects = &artboard.objects;
    let parent_of = |i: usize| {
        objects
            .get(i)
            .and_then(Option::as_ref)
            .filter(|o| o.is(t::COMPONENT))
            .and_then(|o| o.id(p::COMPONENT_PARENT_ID))
    };
    // Whether `ancestor` is object `i` or holds it.
    let under = |ancestor: usize, mut i: usize| {
        for _ in 0..=Limits::default().max_depth {
            if i == ancestor {
                return true;
            }
            match parent_of(i) {
                Some(pi) if pi != i => i = pi,
                _ => return false,
            }
        }
        false
    };
    let mut listeners = Vec::new();
    for l in &sm.listeners {
        let Some(node) = l.object.id(p::STATE_MACHINE_LISTENER_TARGET_ID) else {
            continue;
        };
        // Rive hits the shapes under a target, or the target itself.
        let shapes: Vec<usize> = (1..objects.len())
            .filter(|&i| objects[i].as_ref().is_some_and(|o| o.is(t::SHAPE)) && under(node, i))
            .collect();
        let targets: Vec<String> = shapes.iter().map(|i| format!("{prefix}{i}")).collect();
        let opaque: Vec<String> = shapes
            .iter()
            .filter(|&&i| {
                objects[i]
                    .as_ref()
                    .is_some_and(|o| o.uint(p::DRAWABLE_DRAWABLE_FLAGS) & 8 != 0)
            })
            .map(|i| format!("{prefix}{i}"))
            .collect();
        let mut on = Vec::new();
        let mut events = Vec::new();
        if l.object.ty == t::STATE_MACHINE_LISTENER_SINGLE {
            let kind = l
                .object
                .uint(p::STATE_MACHINE_LISTENER_SINGLE_LISTENER_TYPE_VALUE);
            on.extend(event(kind));
            if kind == 5 {
                events.extend(l.object.id(p::STATE_MACHINE_LISTENER_SINGLE_EVENT_ID));
            }
        }
        let mut actions = Vec::new();
        for a in &l.actions {
            if a.is(t::LISTENER_INPUT_TYPE) {
                on.extend(event(a.uint(p::LISTENER_INPUT_TYPE_LISTENER_TYPE_VALUE)));
                if a.ty == t::LISTENER_INPUT_TYPE_EVENT {
                    events.extend(a.id(p::LISTENER_INPUT_TYPE_EVENT_EVENT_ID));
                }
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
            if a.ty == t::LISTENER_VIEW_MODEL_CHANGE {
                let bindable = a.uint(file::BINDABLE_INDEX) as usize;
                let at = file
                    .artboards
                    .iter()
                    .position(|x| std::ptr::eq(x, artboard));
                if let Some(property) =
                    at.and_then(|at| crate::data::bindable_source(file, (at, index, bindable)))
                {
                    actions.push(Action::Value { property, bindable });
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
        let hears = (!on.is_empty() && !targets.is_empty()) || !events.is_empty();
        if hears && !actions.is_empty() {
            listeners.push(Listener {
                node,
                targets,
                opaque,
                on,
                events,
                actions,
            });
        }
    }
    // The first state machine each nested artboard plays.
    let mut nested = Vec::new();
    if depth < MAX_NESTING {
        for (j, o) in objects.iter().enumerate() {
            let Some(o) = o.as_ref().filter(|o| o.ty == t::NESTED_ARTBOARD) else {
                continue;
            };
            let Some(target) = o
                .id(p::NESTED_ARTBOARD_ARTBOARD_ID)
                .and_then(|a| file.artboards.get(a))
            else {
                continue;
            };
            let Some(sm) = objects.iter().enumerate().find_map(|(c, co)| {
                let co = co.as_ref()?;
                (co.ty == t::NESTED_STATE_MACHINE && parent_of(c) == Some(j))
                    .then(|| co.uint(p::NESTED_ANIMATION_ANIMATION_ID) as usize)
            }) else {
                continue;
            };
            if sm < target.state_machines.len() {
                let path = if path.is_empty() {
                    format!("#{j}")
                } else {
                    format!("{path}/#{j}")
                };
                nested.push(read_machine(
                    file,
                    target,
                    sm,
                    &path,
                    &format!("{prefix}{j}/"),
                    depth + 1,
                ));
            }
        }
    }
    Machine {
        name: sm.object.string(p::ANIMATION_NAME),
        path: path.to_owned(),
        at: (
            file.artboards
                .iter()
                .position(|x| std::ptr::eq(x, artboard))
                .unwrap_or(usize::MAX),
            index,
        ),
        inputs,
        listeners,
        nested,
    }
}

/// Where each group of `scene` with an id is in the order it draws (later
/// is in front).
fn draw_order(scene: &dxn_vector_animation::Scene) -> std::collections::HashMap<&str, usize> {
    fn walk<'a>(
        g: &'a dxn_vector_animation::Group,
        out: &mut std::collections::HashMap<&'a str, usize>,
        depth: usize,
    ) {
        if depth > 256 {
            return;
        }
        if let Some(id) = &g.id {
            let n = out.len();
            out.entry(id.as_str()).or_insert(n);
        }
        for c in &g.children {
            if let dxn_vector_animation::Node::Group(c) = c {
                walk(c, out, depth + 1);
            }
        }
    }
    let mut out = std::collections::HashMap::new();
    walk(&scene.root, &mut out, 0);
    out
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
/// state machines' inputs (its default one's, and those its nested artboards
/// play), by hand or by pointer events over their listeners. Each change is
/// made at a time of the scene's timeline at or after the changes before it,
/// and the file is read again with it.
#[derive(Debug, Clone)]
pub struct Interaction {
    data: Vec<u8>,
    limits: Limits,
    options: Options,
    machine: Option<Machine>,
    import: Import,
    /// Each machine's path, and its listeners, in turn.
    listeners: Vec<(String, Listener)>,
    /// Whether the pointer was over each listener's target, and pressed
    /// there, and where it was at the listener's last event.
    over: Vec<bool>,
    pressed: Vec<bool>,
    previous: Vec<Point>,
    /// Whether each listener's drag has started since it was pressed.
    dragging: Vec<bool>,
    /// The events the artboard's listeners fired, not yet taken.
    fired: Vec<Cue>,
    /// The artboard's objects, for the events' names and properties.
    objects: Vec<Option<file::Object>>,
    /// The artboard's index and its default state machine's.
    root: Option<(usize, usize)>,
}

/// An event a listener fired: the path of its machine, the event's index
/// in that machine's artboard, and the time.
struct Fired {
    path: String,
    event: usize,
}

impl Interaction {
    /// The file `data` read as `options` say.
    pub fn new(data: Vec<u8>, limits: Limits, options: Options) -> Result<Interaction, Error> {
        let import = read_options(&data, &limits, &options)?;
        let machine = machine(&data, &options)?;
        let listeners: Vec<(String, Listener)> = machine
            .as_ref()
            .map(|m| {
                m.all()
                    .into_iter()
                    .flat_map(|m| m.listeners.iter().map(|l| (m.path.clone(), l.clone())))
                    .collect()
            })
            .unwrap_or_default();
        let n = listeners.len();
        let objects = artboard_objects(&data, &options)?;
        let root = {
            let file = file::read(&data)?;
            chosen(&file, &options)?.and_then(|a| {
                let at = file.artboards.iter().position(|x| std::ptr::eq(x, a))?;
                let machines = a.state_machines.len();
                let m = a
                    .object()
                    .id(p::ARTBOARD_DEFAULT_STATE_MACHINE_ID)
                    .filter(|&i| i < machines)
                    .or((machines > 0).then_some(0))?;
                Some((at, m))
            })
        };
        Ok(Interaction {
            data,
            limits,
            options,
            machine,
            import,
            listeners,
            over: vec![false; n],
            pressed: vec![false; n],
            previous: vec![Point::new(0.0, 0.0); n],
            dragging: vec![false; n],
            fired: Vec::new(),
            objects,
            root,
        })
    }

    /// The events the artboard's listeners fired since they were last
    /// taken, each a cue at the time it was fired (a nested artboard's go
    /// to its parent's listeners instead, as in Rive).
    pub fn take_fired(&mut self) -> Vec<Cue> {
        std::mem::take(&mut self.fired)
    }

    /// The scene as the changes so far make it.
    pub fn import(&self) -> &Import {
        &self.import
    }

    /// The default state machine, with those its nested artboards play.
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

    /// The changes made to the view model instance bound so far.
    pub fn values(&self) -> &[ValueChange] {
        &self.options.values
    }

    /// The view model bound to the artboard, as its first instance holds
    /// it (before the changes made).
    pub fn view_model(&self) -> Option<crate::ViewModel> {
        crate::view_model(&self.data, &self.options).ok().flatten()
    }

    /// Sets property `path` of the view model instance bound (see
    /// [`ValueChange::path`]) to `value` at `time`
    /// ([`PropertyValue::Trigger`] fires a trigger), reading the scene
    /// again.
    pub fn set_value(&mut self, time: f64, path: &str, value: PropertyValue) -> Result<(), Error> {
        self.make(
            Vec::new(),
            Vec::new(),
            vec![ValueChange {
                time,
                path: path.to_owned(),
                value,
            }],
        )
    }

    /// Sets input `input` of the default state machine to `value` at
    /// `time`, reading the scene again.
    pub fn set(&mut self, time: f64, input: &str, value: InputValue) -> Result<(), Error> {
        self.set_at(time, "", input, value)
    }

    /// Sets input `input` of the state machine at `path` (see
    /// [`Machine::path`]) to `value` at `time`, reading the scene again.
    pub fn set_at(
        &mut self,
        time: f64,
        path: &str,
        input: &str,
        value: InputValue,
    ) -> Result<(), Error> {
        self.make(
            vec![InputChange {
                time,
                path: path.to_owned(),
                input: input.to_owned(),
                value,
            }],
            Vec::new(),
            Vec::new(),
        )
    }

    /// A pointer doing `kind` at `point` (in the scene's coordinates) at
    /// `time`: the listeners over whose targets it is act as Rive's runtime
    /// has them act (entering, leaving, moving, pressing, releasing,
    /// clicking, dragging), those of nested artboards' state machines too;
    /// the events they fire reach the listeners that listen for them, up
    /// through the nested artboards. Whether the inputs changed (and the
    /// scene was read again).
    ///
    /// A press or a release (a lift too) acts at once, as Rive's web runtime
    /// has it (advancing the state machine by nothing): from the frame the
    /// pointer fell in, where a move or a leaving, like an input set by
    /// hand, acts from the frame after.
    pub fn pointer(&mut self, time: f64, kind: Pointer, point: Point) -> Result<bool, Error> {
        if self.listeners.is_empty() {
            return Ok(false);
        }
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
        let mut values = Vec::new();
        let mut fired = Vec::new();
        let steps: &[Pointer] = match kind {
            Pointer::Lift => &[Pointer::Up, Pointer::Exit],
            _ => std::slice::from_ref(&kind),
        };
        let listeners = std::mem::take(&mut self.listeners);
        // What the pointer is over, front to back as the scene draws it: a
        // shape that stops it hides what is behind it.
        let hits: Vec<bool> = if kind == Pointer::Exit {
            vec![false; listeners.len()]
        } else {
            let scene = &self.import.scene;
            let order = &draw_order(scene);
            let mut shapes: Vec<(usize, usize, &String)> = listeners
                .iter()
                .enumerate()
                .flat_map(|(k, (_, l))| {
                    l.targets
                        .iter()
                        .map(move |id| (order.get(id.as_str()).copied().unwrap_or(0), k, id))
                })
                .collect();
            shapes.sort_by_key(|s| std::cmp::Reverse(s.0));
            let mut hits = vec![false; listeners.len()];
            let mut blocked = None;
            for (at, k, id) in shapes {
                if blocked.is_some_and(|b| at < b) {
                    break;
                }
                if scene.hit_rect(id, around, time) {
                    hits[k] = true;
                    if listeners[k].1.opaque.contains(id) {
                        blocked = Some(at);
                    }
                }
            }
            hits
        };
        for &kind in steps {
            for (k, (path, l)) in listeners.iter().enumerate() {
                let hit = kind != Pointer::Exit && hits[k];
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
                // Pressed on it, the pointer drags it wherever it moves: its
                // first move starts the drag, as Rive's web runtime has it.
                if kind == Pointer::Move && self.pressed[k] {
                    if self.dragging[k] {
                        events.push(PointerEvent::Drag);
                    }
                    self.dragging[k] = true;
                }
                if kind != Pointer::Move {
                    self.dragging[k] = false;
                }
                if kind != Pointer::Move && kind != Pointer::Down {
                    self.pressed[k] = false;
                }
                self.over[k] = hit;
                let from = self.previous[k];
                for event in events {
                    if l.on.contains(&event) {
                        self.perform(
                            path,
                            l,
                            (time, acts),
                            (point, from),
                            &mut changes,
                            &mut aligned,
                            &mut values,
                            &mut fired,
                        );
                    }
                }
                self.previous[k] = point;
            }
        }
        // The events fired reach the listeners listening for them: the
        // machine's own (its target the artboard or the event), then its
        // parent's (its target the nested artboard), and on up.
        let mut queue: std::collections::VecDeque<Fired> = fired.into();
        let mut reached = 0;
        while let Some(f) = queue.pop_front() {
            reached += 1;
            if reached > 256 {
                break;
            }
            if f.path.is_empty() {
                self.fired
                    .extend(crate::cues::cue(&self.objects, f.event, time));
            }
            let (parent, node) = match f.path.rsplit_once('/') {
                Some((parent, last)) => (Some(parent), last),
                None if f.path.is_empty() => (None, ""),
                None => (Some(""), f.path.as_str()),
            };
            let node: Option<usize> = node.strip_prefix('#').and_then(|n| n.parse().ok());
            let mut more = Vec::new();
            for (path, l) in &listeners {
                if !l.events.contains(&f.event) {
                    continue;
                }
                let own = *path == f.path && !self.is_nested_node(path, l.node);
                let above = Some(path.as_str()) == parent && Some(l.node) == node;
                if own || above {
                    self.perform(
                        path,
                        l,
                        (time, acts),
                        (point, point),
                        &mut changes,
                        &mut aligned,
                        &mut values,
                        &mut more,
                    );
                }
            }
            queue.extend(more);
        }
        self.listeners = listeners;
        if changes.is_empty() && aligned.is_empty() && values.is_empty() {
            return Ok(false);
        }
        self.make(changes, aligned, values)?;
        Ok(true)
    }

    /// Whether `node` of the machine at `path` is a nested artboard.
    fn is_nested_node(&self, path: &str, node: usize) -> bool {
        let nested = if path.is_empty() {
            format!("#{node}")
        } else {
            format!("{path}/#{node}")
        };
        self.machine
            .as_ref()
            .is_some_and(|m| m.all().iter().any(|n| n.path == nested))
    }

    /// What listener `l` of the machine at `path` does, at `time` (its
    /// inputs changed from `acts`), the pointer at `point` (`from` at its
    /// last event).
    #[allow(clippy::too_many_arguments)]
    fn perform(
        &self,
        path: &str,
        l: &Listener,
        (time, acts): (f64, f64),
        (point, from): (Point, Point),
        changes: &mut Vec<InputChange>,
        aligned: &mut Vec<Aligned>,
        values: &mut Vec<ValueChange>,
        fired: &mut Vec<Fired>,
    ) {
        for a in &l.actions {
            match a {
                Action::Input { input, change } => {
                    let value = match change {
                        Change::Set(v) => *v,
                        Change::Toggle => {
                            InputValue::Bool(!self.bool_at(path, input, acts, changes))
                        }
                    };
                    changes.push(InputChange {
                        time: acts,
                        path: path.to_owned(),
                        input: input.clone(),
                        value,
                    });
                }
                &Action::Fire { event } => fired.push(Fired {
                    path: path.to_owned(),
                    event,
                }),
                &Action::Value { bindable, .. } => {
                    let Some((root, _)) = self.root else {
                        continue;
                    };
                    let Some((artboard, machine)) = self
                        .machine
                        .as_ref()
                        .and_then(|m| m.all().into_iter().find(|m| m.path == path))
                        .map(|m| m.at)
                    else {
                        continue;
                    };
                    let Ok(mut file) = file::read(&self.data) else {
                        continue;
                    };
                    let before: Vec<ValueChange> = self
                        .options
                        .values
                        .iter()
                        .chain(&*values)
                        .cloned()
                        .collect();
                    values.extend(crate::data::listener_change(
                        &mut file,
                        root,
                        (artboard, machine, bindable),
                        &before,
                        acts,
                    ));
                }
                &Action::Align { node, keep_offset } => aligned.push(Aligned {
                    time,
                    path: path.to_owned(),
                    node,
                    to: point,
                    from: keep_offset.then_some(from),
                }),
            }
        }
    }

    /// The value of bool input `input` of the machine at `path` at `time`,
    /// with `pending` changes made after those so far.
    fn bool_at(&self, path: &str, input: &str, time: f64, pending: &[InputChange]) -> bool {
        let mut v = self
            .machine
            .as_ref()
            .and_then(|m| m.all().into_iter().find(|m| m.path == path))
            .and_then(|m| m.inputs.iter().find(|i| i.name == input))
            .is_some_and(|i| matches!(i.kind, InputKind::Bool(true)));
        for c in self.options.inputs.iter().chain(pending) {
            if c.path == path
                && c.input == input
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
        mut values: Vec<ValueChange>,
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
        let last = self.options.values.last().map_or(0.0, |v| v.time);
        for v in &mut values {
            v.time = v.time.max(last);
        }
        let mut options = self.options.clone();
        options.inputs.extend(changes);
        options.aligned.extend(aligned);
        options.values.extend(values);
        self.import = read_options(&self.data, &self.limits, &options)?;
        self.options = options;
        Ok(())
    }
}
