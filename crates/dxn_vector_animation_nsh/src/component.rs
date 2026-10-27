//! `VectorAnimation`: plays animated SVG, Lottie and Rive.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

use dxn_vector_animation::{Frame, ImageData, Matrix, Point, Rect, Scene};
use dxn_vector_animation_rive::{
    InputKind, InputValue, Interaction, Pointer, PropertyValue, ValueChange,
};
use nsh_core::{
    HoverEvent, TouchEvent, TouchPhase,
    element::{SignalEquals, signal_with},
    geometry::{Bounds, Points, Size, px},
    prelude::*,
};

use crate::{
    graph::Graph,
    lower::{self, Nodes},
    plan,
    playback::{Direction, Playback},
    source::{self, At, InputSetting, Loaded, PosterFrame, Repeat, Segment, Source, ValueSetting},
};

/// How the animation fits its view (`resizeMode`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum ResizeMode {
    /// All of it, as large as fits, centered.
    #[default]
    Contain,
    /// The view filled, centered, cut where it does not fit.
    Cover,
    /// Stretched to the view's size.
    Stretch,
    /// At its own size, centered.
    Center,
}

/// The props of [`VectorAnimation`].
#[derive(Debug, Clone, PartialEq, Props)]
pub struct VectorAnimationProps {
    /// What it plays: a URL, an imported file, the text of an SVG or Lottie
    /// file or a compiled scene, its bytes (a Rive file's too), or a Lottie
    /// or compiled scene's JSON (a `.json` import).
    pub src: Prop<Option<Source>>,
    /// Stops playback where it is; `false` plays on from there.
    #[prop(default)]
    pub paused: Prop<bool>,
    /// `true` plays forever, a number that many times in all, `false` once;
    /// the file's own loop mode when left out.
    pub repeat: Prop<Option<Repeat>>,
    /// Which way it plays.
    #[prop(default)]
    pub direction: Prop<Direction>,
    /// Playback speed (1 as made).
    #[prop(default = 1.0)]
    pub rate: Prop<f64>,
    /// What part plays: a marker's name or `[start, end]` in seconds; all of
    /// it when left out.
    pub segment: Prop<Option<Segment>>,
    /// How it fits its view.
    #[prop(default)]
    pub resize_mode: Prop<ResizeMode>,
    /// 0 to 1: the frame shown, set by the app (scrubbing); it stops
    /// playback while set.
    pub progress: Prop<Option<f64>>,
    /// The frame shown when the user reduces motion: `"first"` (the
    /// default), `"last"`, seconds or a marker's name.
    pub poster_frame: Prop<Option<PosterFrame>>,
    /// Plays when the user reduces motion too.
    #[prop(default)]
    pub ignore_reduce_motion: Prop<bool>,
    /// What assistive technologies read for it.
    pub accessibility_label: Prop<Option<SharedString>>,
}

/// The size of an animation, in points.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct AnimationSize {
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// A named time or span of an animation.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct AnimationMarker {
    /// Its name.
    pub name: String,
    /// Where it starts, in seconds.
    pub start: f64,
    /// How long it lasts, in seconds.
    pub duration: f64,
}

/// A cue playback passed (a Rive event).
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct AnimationCue {
    /// Its name.
    pub name: String,
    /// Where it is on the timeline, in seconds.
    pub time: f64,
    /// The link it opens (a Rive open-URL event).
    pub url: Option<String>,
    /// Where it opens it: `_blank`, `_parent`, `_self` or `_top`.
    pub target: Option<String>,
    /// What it carries (a Rive event's custom properties), by name.
    pub properties: serde_json::Value,
}

impl AnimationCue {
    fn of(c: &dxn_vector_animation::Cue) -> AnimationCue {
        use dxn_vector_animation::CueValue;
        let properties = c
            .properties
            .iter()
            .map(|(name, value)| {
                let value = match value {
                    CueValue::Number(n) => serde_json::json!(n),
                    CueValue::Text(t) => serde_json::json!(t),
                    CueValue::Bool(b) => serde_json::json!(b),
                };
                (name.clone(), value)
            })
            .collect::<serde_json::Map<_, _>>();
        AnimationCue {
            name: c.name.clone(),
            time: c.time,
            url: c.url.as_ref().map(|(u, _)| u.clone()),
            target: c.url.as_ref().map(|(_, t)| t.clone()),
            properties: serde_json::Value::Object(properties),
        }
    }
}

/// The cues of `cues` playback passes going from `from` to `to` (both
/// included when `first`, else `from` left out): over the end and back to
/// `loop_start` when it went forwards and came back.
fn passed(
    cues: &[dxn_vector_animation::Cue],
    from: f64,
    to: f64,
    first: bool,
    wraps: Option<(f64, f64)>,
) -> impl Iterator<Item = &dxn_vector_animation::Cue> {
    let within = move |t: f64| {
        let after = |a: f64| if first { t >= a } else { t > a };
        match wraps {
            Some((loop_start, end)) if to < from => {
                (after(from) && t <= end) || (t >= loop_start && t <= to)
            }
            _ if to >= from => after(from) && t <= to,
            _ => t >= to && t < from,
        }
    };
    cues.iter().filter(move |c| within(c.time))
}

/// A property of the view model bound to a Rive file's artboard, and its
/// value.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct AnimationValue {
    /// Its name (nested view models' names first, `/` between): what
    /// `setValue` and `fireValue` take.
    pub path: String,
    /// What it holds: `"number"`, `"boolean"`, `"color"`, `"string"`,
    /// `"enum"`, `"trigger"` or `"other"` (a list, a nested view model).
    pub kind: String,
    /// Its value (a color's ARGB; an enum's index; `null` for a trigger,
    /// fired as a change).
    pub value: serde_json::Value,
}

impl AnimationValue {
    fn of(c: &ValueChange) -> AnimationValue {
        let (kind, value) = json_of(&c.value);
        AnimationValue {
            path: c.path.clone(),
            kind: kind.into(),
            value,
        }
    }
}

/// A view model property's value, by kind.
fn json_of(v: &PropertyValue) -> (&'static str, serde_json::Value) {
    use serde_json::{Value, json};
    match v {
        PropertyValue::Number(n) => ("number", json!(n)),
        PropertyValue::Bool(b) => ("boolean", json!(b)),
        PropertyValue::Color(c) => ("color", json!(c)),
        PropertyValue::Text(s) => ("string", json!(s)),
        PropertyValue::Enum(i) => ("enum", i.map_or(Value::Null, |i| json!(i))),
        PropertyValue::Trigger => ("trigger", Value::Null),
        PropertyValue::Other => ("other", Value::Null),
    }
}

/// An input of a Rive file's state machine.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct AnimationInput {
    /// Its name.
    pub name: String,
    /// The nested artboards whose state machine it is (`#` and each one's
    /// index, `/` between), empty for the file's own: what `setInputAt`
    /// and `fireAt` take.
    pub path: String,
    /// What it holds: `"bool"`, `"number"` or `"trigger"`.
    pub kind: String,
    /// Its value at first (a bool's 0 or 1; a trigger's 0).
    pub value: f64,
}

/// What a loaded animation is.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct AnimationInfo {
    /// Seconds one play-through lasts.
    pub duration: f64,
    /// Its own size.
    pub size: AnimationSize,
    /// Its markers.
    pub markers: Vec<AnimationMarker>,
    /// The events it has something to play on (`btn.click`): what
    /// [`VectorAnimationApi::trigger`] takes.
    pub events: Vec<String>,
    /// A Rive file's state machine's inputs: what
    /// [`VectorAnimationApi::set_input`] and [`VectorAnimationApi::fire`]
    /// take.
    pub inputs: Vec<AnimationInput>,
    /// The properties of the view model bound to a Rive file's artboard,
    /// with its instance's values: what [`VectorAnimationApi::set_value`]
    /// and [`VectorAnimationApi::fire_value`] take.
    pub values: Vec<AnimationValue>,
    /// What the file holds that is left out (text, filters, effects, ...).
    pub warnings: Vec<String>,
}

/// The events of a [`VectorAnimation`].
#[derive(Debug, Clone, PartialEq, Event)]
pub enum VectorAnimationEvent {
    /// The animation was read: its duration, size and markers.
    Load(AnimationInfo),
    /// Playback reached its end (never while repeating forever).
    End,
    /// Playback passed a cue: a Rive event its state machine fired (or a
    /// listener did, as the pointer acted).
    Cue(AnimationCue),
    /// An input of a Rive file's state machine changed: a listener set it
    /// as the pointer acted, or `setInput` or `fire` did (a trigger fired
    /// is 1).
    Input(AnimationInput),
    /// A property of the view model bound to a Rive file's artboard
    /// changed: a listener set it as the pointer acted, or `setValue` or
    /// `fireValue` did.
    Value(AnimationValue),
    /// The animation could not be loaded or read.
    Error(String),
}

/// The methods of a [`VectorAnimation`]'s ref.
pub struct VectorAnimationApi {
    seek_to: Signal<Option<(f64, u64)>>,
    event: Signal<Option<(String, u64)>>,
    act: Rc<dyn Fn(Act)>,
}

#[methods]
impl VectorAnimationApi {
    /// Moves to `seconds` on the timeline; playback goes on from there
    /// unless paused (what an event played gives way to the timeline).
    pub fn seek(&self, seconds: f64) {
        let generation = self.seek_to.get_untracked().map_or(0, |(_, g)| g + 1);
        self.event.set(None);
        self.seek_to.set(Some((seconds, generation)));
    }

    /// Plays what the animation has for `event` (an SVG's `begin="click"`
    /// animations: `btn.click`, as the load info's `events` names them)
    /// once from its start, and holds its end; the timeline's own
    /// animations play from their start meanwhile.
    pub fn trigger(&self, event: String) {
        let generation = self.event.get_untracked().map_or(0, |(_, g)| g + 1);
        self.event.set(Some((event, generation)));
    }

    /// Sets a Rive file's state machine input `name` to `value` (a bool or
    /// a number, as the load info's `inputs` has it) now: the machine plays
    /// on from here as Rive plays it with the change.
    pub fn set_input(&self, name: String, value: InputSetting) {
        self.set_input_at(String::new(), name, value);
    }

    /// Fires a Rive file's state machine trigger `name` now.
    pub fn fire(&self, name: String) {
        self.fire_at(String::new(), name);
    }

    /// Sets input `name` of the state machine a nested artboard plays,
    /// at `path` (the nested artboards' names, or `#` and their indices,
    /// `/` between, as the load info's `inputs` has it), to `value` now.
    pub fn set_input_at(&self, path: String, name: String, value: InputSetting) {
        (self.act)(Act::Set(
            path,
            name,
            match value {
                InputSetting::Bool(b) => InputValue::Bool(b),
                InputSetting::Number(n) => InputValue::Number(n),
            },
        ));
    }

    /// Fires trigger `name` of the state machine a nested artboard plays,
    /// at `path`, now.
    pub fn fire_at(&self, path: String, name: String) {
        (self.act)(Act::Set(path, name, InputValue::Fire));
    }

    /// Sets property `path` of the view model bound to a Rive file's
    /// artboard (its name, nested view models' names first, `/` between,
    /// as the load info's `values` has it) to `value` now: what is bound to
    /// it, and the state machine's conditions on it, follow as in Rive.
    pub fn set_value(&self, path: String, value: ValueSetting) {
        (self.act)(Act::Value(
            path,
            match value {
                ValueSetting::Bool(b) => PropertyValue::Bool(b),
                ValueSetting::Number(n) => PropertyValue::Number(n),
                ValueSetting::Text(s) => PropertyValue::Text(s),
            },
        ));
    }

    /// Fires trigger `path` of the view model bound to a Rive file's
    /// artboard now.
    pub fn fire_value(&self, path: String) {
        (self.act)(Act::Value(path, PropertyValue::Trigger));
    }
}

/// The test id of the view over a Rive file whose state machine has
/// listeners: the pointer over it acts on them.
pub const POINTER_TEST_ID: &str = "vector-animation-pointer";

/// A change to a Rive file's state machine.
enum Act {
    /// A pointer over the scene (in its coordinates).
    Pointer(Pointer, Point),
    /// An input set: the path of its machine, its name.
    Set(String, String, InputValue),
    /// A view model property set: its path.
    Value(String, PropertyValue),
}

/// A Rive file's state machine as the user plays it: the changes still to
/// make, made a batch at a time off the engine thread, and the file read
/// with them.
#[derive(Default)]
struct Interacting {
    /// The file's (none while a batch is made, or for other files).
    play: Option<Interaction>,
    /// The changes to make, each at the time it came.
    queue: Vec<(f64, Act)>,
    /// A batch is being made.
    busy: bool,
    /// Counts the sources read: a batch of one before is dropped.
    generation: u64,
    /// Where the pointer last was, in the scene's coordinates.
    last: Option<Point>,
}

/// Makes `batch` on `play`: whether it changed the scene.
fn make(play: &mut Interaction, batch: Vec<(f64, Act)>) -> bool {
    let mut changed = false;
    for (t, act) in batch {
        let made = match act {
            Act::Pointer(kind, at) => play.pointer(t, kind, at),
            Act::Set(path, name, value) => play.set_at(t, &path, &name, value).map(|()| true),
            Act::Value(path, value) => play.set_value(t, &path, value).map(|()| true),
        };
        match made {
            Ok(c) => changed |= c,
            Err(e) => log::warn!("VectorAnimation: {e}"),
        }
    }
    changed
}

/// `batch` with each run of moves but its last left out.
fn coalesced(batch: Vec<(f64, Act)>) -> Vec<(f64, Act)> {
    let mut out: Vec<(f64, Act)> = Vec::with_capacity(batch.len());
    for (t, act) in batch {
        if matches!(act, Act::Pointer(Pointer::Move, _))
            && matches!(out.last(), Some((_, Act::Pointer(Pointer::Move, _))))
        {
            out.pop();
        }
        out.push((t, act));
    }
    out
}

/// A scene ready to play: whether the native graph plays it.
struct Ready {
    loaded: Loaded,
    native: bool,
    /// Parts of it are sampled frame by frame, on the engine's clock.
    mixed: bool,
    /// A Rive file whose state machine has listeners: pointers act on it.
    listens: bool,
}

impl Ready {
    /// `loaded` compiled for the native graph where it can be.
    fn of(loaded: Loaded, listens: bool) -> Ready {
        let (native, mixed) = match plan::compile(&loaded.scene, Matrix::IDENTITY) {
            Ok(plan) => {
                for why in &plan.sampled {
                    log::info!("VectorAnimation: a part sampled frame by frame ({why})");
                }
                (true, !plan.sampled.is_empty())
            }
            Err(plan::Unsupported(why)) => {
                log::info!("VectorAnimation: sampled frame by frame ({why})");
                (false, false)
            }
        };
        Ready {
            loaded,
            native,
            mixed,
            listens,
        }
    }
}

/// Plays an animated SVG (CSS animations, as Figma Motion exports them) or a
/// Lottie animation, natively: a scene's motion runs on the platform's UI
/// thread, with no JavaScript per frame. It is as large as the animation
/// unless the app sizes it, and fits it as `resizeMode` says.
#[component(methods = VectorAnimationApi)]
pub fn VectorAnimation(
    props: VectorAnimationProps,
    cx: Scope<VectorAnimationEvent>,
) -> impl IntoElement {
    let ready: Signal<Option<Rc<Ready>>> = signal_with(None, SignalEquals::Never);
    let size = signal(Size::new(px(0.0), px(0.0)));
    let time = animated_value(0.0);
    let clock = signal(0.0f64);
    let seek_to = signal(None::<(f64, u64)>);
    let event = signal(None::<(String, u64)>);
    // The time playback holds still at, when it does (a run ended, a
    // progress, a still frame, a seek): read so, as the graph's time is
    // read on the UI thread's next frame, and the web draws none while
    // nothing moves.
    let held = stored(None::<f64>);
    let interacting = stored(Interacting::default());
    // A change to a Rive file's state machine, made at the time playback
    // is at: queued, and made with those queued after it off the engine
    // thread, the scene swapped for what they make (playback goes on from
    // where it is).
    let act = move |what: Act| {
        spawn(async move {
            let native = ready.get_untracked().is_some_and(|r| r.native);
            let now = match held.with(|h| *h) {
                Some(t) => t,
                None if native => f64::from(time.value().await),
                None => clock.get_untracked(),
            };
            let go = interacting.with(|i| {
                i.queue.push((now, what));
                let go = !i.busy && i.play.is_some();
                i.busy |= go;
                go
            });
            if !go {
                return;
            }
            loop {
                let (play, batch, generation) = interacting
                    .with(|i| (i.play.take(), std::mem::take(&mut i.queue), i.generation));
                let Some(mut play) = play else { break };
                if batch.is_empty() {
                    interacting.with(|i| {
                        i.play = Some(play);
                        i.busy = false;
                    });
                    break;
                }
                let batch = coalesced(batch);
                let listens = ready.get_untracked().is_some_and(|r| r.listens);
                let changed_at = batch.first().map_or(0.0, |(t, _)| *t);
                let made = move || {
                    let before = play.changes().len();
                    let values_before = play.values().len();
                    let changed = make(&mut play, batch);
                    let values: Vec<AnimationValue> = play.values()
                        [values_before.min(play.values().len())..]
                        .iter()
                        .map(AnimationValue::of)
                        .collect();
                    let inputs: Vec<AnimationInput> = play.changes()
                        [before.min(play.changes().len())..]
                        .iter()
                        .map(|c| {
                            let (kind, value) = match c.value {
                                InputValue::Bool(b) => ("bool", f64::from(u8::from(b))),
                                InputValue::Number(n) => ("number", n),
                                InputValue::Fire => ("trigger", 1.0),
                            };
                            AnimationInput {
                                name: c.input.clone(),
                                path: c.path.clone(),
                                kind: kind.into(),
                                value,
                            }
                        })
                        .collect();
                    let swapped = changed.then(|| {
                        let import = play.import();
                        Ready::of(
                            Loaded {
                                scene: import.scene.clone(),
                                warnings: import.warnings.clone(),
                                interaction: None,
                            },
                            listens,
                        )
                    });
                    let fired = play.take_fired();
                    (play, swapped, inputs, values, fired)
                };
                #[cfg(not(target_arch = "wasm32"))]
                let Ok((play, swapped, inputs, values, fired)) = blocking(made).await else {
                    break;
                };
                #[cfg(target_arch = "wasm32")]
                let (play, swapped, inputs, values, fired) = made();
                if interacting.with(|i| i.generation) != generation {
                    // Another source is read meanwhile.
                    break;
                }
                for cue in &fired {
                    cx.emit(VectorAnimationEvent::Cue(AnimationCue::of(cue)));
                }
                if let Some(r) = swapped {
                    // What the new scene fires between the change and now
                    // has passed already.
                    let now = match held.with(|h| *h) {
                        Some(t) => t,
                        None if r.native => f64::from(time.value().await),
                        None => clock.get_untracked(),
                    };
                    for c in passed(&r.loaded.scene.timeline.cues, changed_at, now, false, None) {
                        cx.emit(VectorAnimationEvent::Cue(AnimationCue::of(c)));
                    }
                    ready.set(Some(Rc::new(r)));
                }
                for input in inputs {
                    cx.emit(VectorAnimationEvent::Input(input));
                }
                for value in values {
                    cx.emit(VectorAnimationEvent::Value(value));
                }
                interacting.with(|i| i.play = Some(play));
            }
        });
    };
    cx.methods(VectorAnimationApi {
        seek_to,
        event,
        act: Rc::new(act),
    });
    // The scene played: the file's, or what it plays on the event last
    // triggered, moved to its start.
    let played: Signal<Option<(Rc<Scene>, Option<f64>)>> = signal_with(None, SignalEquals::Never);
    effect(move || {
        let Some(r) = ready.get() else {
            played.set(None);
            return;
        };
        let scene = &r.loaded.scene;
        let on = event.get().and_then(|(name, _)| {
            let m = scene.timeline.markers.iter().find(|m| {
                m.name
                    .strip_prefix(dxn_vector_animation::EVENT_PREFIX)
                    .is_some_and(|n| n == name)
            })?;
            Some((
                Rc::new(scene.with_events(&[(name.as_str(), 0.0)])),
                m.duration,
            ))
        });
        played.set(Some(match on {
            Some((scene, duration)) => (scene, Some(duration)),
            None => (Rc::new(scene.clone()), None),
        }));
    });
    let loading = stored(None::<Spawned<()>>);
    let playing = stored(None::<Spawned<()>>);
    // The run playing, stopped as a whole: a stop of the time value ends
    // only the leg moving it now, and a run that loops goes on to its next.
    let run = stored(None::<Animation>);
    // The engine's clock beside the graph's, for parts sampled frame by
    // frame.
    let ticking = stored(None::<Spawned<()>>);
    // What reports the cues playback passes.
    let cueing = stored(None::<Spawned<()>>);
    // Stops what plays: its tasks, its run, the time value.
    let halt = move || {
        if let Some(task) = playing.replace(None) {
            task.cancel();
        }
        if let Some(task) = ticking.replace(None) {
            task.cancel();
        }
        if let Some(task) = cueing.replace(None) {
            task.cancel();
        }
        if let Some(run) = run.replace(None) {
            run.stop();
        }
        time.stop();
    };

    // Reading the source.
    effect(move || {
        let src = props.src.get();
        if let Some(task) = loading.replace(None) {
            task.cancel();
        }
        ready.set(None);
        event.set(None);
        let generation = interacting.with(|i| i.generation) + 1;
        interacting.replace(Interacting {
            generation,
            ..Interacting::default()
        });
        let Some(src) = src else { return };
        loading.replace(Some(spawn(async move {
            match load(src).await {
                Ok(mut loaded) => {
                    for w in &loaded.warnings {
                        log::warn!("VectorAnimation: left out: {w}");
                    }
                    let info = info(&loaded);
                    let play = loaded.interaction.take();
                    let listens = play
                        .as_ref()
                        .and_then(Interaction::machine)
                        .is_some_and(|m| m.all().iter().any(|m| !m.listeners.is_empty()));
                    interacting.with(|i| i.play = play);
                    ready.set(Some(Rc::new(Ready::of(loaded, listens))));
                    cx.emit(VectorAnimationEvent::Load(info));
                }
                Err(e) => cx.emit(VectorAnimationEvent::Error(e)),
            }
        })));
    });

    // Playing.
    effect(move || {
        let (Some(r), Some((scene, on_event))) = (ready.get(), played.get()) else {
            // Nothing to play (a source loading, or none): what played stops.
            untracked(halt);
            return;
        };
        let scene = scene.as_ref();
        // An event's: once, from its start.
        let (span, loop_start, repeat, direction) = match on_event {
            Some(duration) => (
                (0.0, duration.max(0.0)),
                0.0,
                Repeat::Once,
                Direction::Normal,
            ),
            None => {
                let chosen = props.segment.get();
                // A segment loops whole; the whole timeline from its loop
                // start.
                let loop_start = match chosen {
                    None => scene.timeline.loop_from(),
                    Some(_) => 0.0,
                };
                // A marker's own loop (a Rive animation's), else the
                // timeline's.
                let loop_mode = match &chosen {
                    Some(Segment::Marker(name)) => scene
                        .timeline
                        .markers
                        .iter()
                        .find(|m| &m.name == name)
                        .and_then(|m| m.loop_mode),
                    _ => None,
                }
                .unwrap_or(scene.timeline.loop_mode);
                let span = segment(scene, chosen);
                let repeat = props.repeat.get().unwrap_or(match loop_mode {
                    dxn_vector_animation::LoopMode::Once => Repeat::Once,
                    _ => Repeat::Forever,
                });
                let direction = match (props.direction.get(), loop_mode) {
                    (Direction::Normal, dxn_vector_animation::LoopMode::PingPong)
                        if props.repeat.get().is_none() =>
                    {
                        Direction::Alternate
                    }
                    (d, _) => d,
                };
                (span, loop_start, repeat, direction)
            }
        };
        let playback = Playback {
            start: span.0,
            end: span.1,
            loop_start,
            rate: props.rate.get().abs().max(1e-3),
            direction,
            repeat,
        };
        let paused = props.paused.get();
        let progress = props.progress.get();
        let still = (reduce_motion() && !props.ignore_reduce_motion.get()).then(|| {
            match props.poster_frame.get() {
                None | Some(PosterFrame::First) => playback.first(),
                Some(PosterFrame::Last) => playback.last(),
                Some(PosterFrame::At(at)) => at_time(scene, &at),
            }
        });
        let seek = seek_to.get();
        // An event plays from its start, as it was triggered.
        let seek = match on_event {
            Some(_) => Some((0.0, 0)),
            None => seek,
        };
        let (native, mixed) = (r.native, r.mixed);
        let cues: Rc<[dxn_vector_animation::Cue]> = scene.timeline.cues.clone().into();
        let wraps = (direction == Direction::Normal && repeat != Repeat::Once)
            .then_some((playback.loop_start, playback.end));
        untracked(move || {
            halt();
            let show = |t: f64| {
                time.set(t as f32);
                clock.set(t);
                held.replace(Some(t));
            };
            if let Some(p) = progress {
                show(playback.start + (playback.end - playback.start) * p.clamp(0.0, 1.0));
                return;
            }
            if let Some(t) = still {
                show(t);
                return;
            }
            if let Some((t, _)) = seek {
                show(t);
            }
            if paused {
                return;
            }
            playing.replace(Some(spawn(async move {
                let from = match held.with(|h| *h) {
                    Some(t) => t,
                    None if native => f64::from(time.value().await),
                    None => clock.get_untracked(),
                };
                // A run that ended starts over.
                let at_end = (from - playback.last()).abs() < 1e-9 && seek.is_none();
                let from = if at_end
                    || !(playback.start.min(playback.end)..=playback.start.max(playback.end))
                        .contains(&from)
                {
                    playback.first()
                } else {
                    from
                };
                if !cues.is_empty() {
                    let cues = cues.clone();
                    let first = from == playback.first();
                    cueing.replace(Some(spawn(async move {
                        let (mut last, mut first) = (from, first);
                        loop {
                            next_frame().await;
                            let t = if native {
                                f64::from(time.value().await)
                            } else {
                                clock.get_untracked()
                            };
                            for c in passed(&cues, last, t, first, wraps) {
                                cx.emit(VectorAnimationEvent::Cue(AnimationCue::of(c)));
                            }
                            (last, first) = (t, false);
                        }
                    })));
                }
                if native {
                    // The parts sampled frame by frame read the graph's
                    // time each frame, so they stay with the rest: a clock
                    // of their own counts from its first engine frame, not
                    // from when the graph starts, and what it draws would
                    // stay behind the rest by the difference.
                    let over = Rc::new(Cell::new(false));
                    if mixed {
                        let over = over.clone();
                        ticking.replace(Some(spawn(async move {
                            clock.set(from);
                            while !over.get() {
                                next_frame().await;
                                let t = f64::from(time.value().await);
                                if !over.get() {
                                    clock.set(t);
                                }
                            }
                        })));
                    }
                    time.set(from as f32);
                    let animation = playback.animation(&time, from);
                    held.replace(None);
                    let end = animation.start();
                    run.replace(Some(animation));
                    let ended = end.await;
                    over.set(true);
                    if ended {
                        clock.set(playback.last());
                        held.replace(Some(playback.last()));
                        cx.emit(VectorAnimationEvent::End);
                    }
                } else {
                    // The engine's clock, frame by frame.
                    clock.set(from);
                    held.replace(None);
                    let started = next_frame().await;
                    loop {
                        let now = next_frame().await;
                        let (t, done) =
                            playback.position(from, now.duration_since(started).as_secs_f64());
                        clock.set(t);
                        if done {
                            break;
                        }
                    }
                    cx.emit(VectorAnimationEvent::End);
                }
            })));
        });
    });

    let natural = move || {
        ready
            .get()
            .map(|r| (r.loaded.scene.width, r.loaded.scene.height))
    };
    let mut root = view()
        .relative()
        .overflow_hidden()
        .w(move || px(natural().map_or(0.0, |s| s.0) as f32))
        .h(move || px(natural().map_or(0.0, |s| s.1) as f32))
        .role(AccessibilityRole::Image);
    root = root.optional_label(move || props.accessibility_label.get());
    // The canvases' box (inside the view's border), measured: the scene
    // fits into it.
    let stage = view()
        .absolute()
        .size_full()
        .on_layout(move |frame: &Bounds<Points>| size.set(frame.size));
    // Where a point of the view is in the scene.
    let in_scene = move |at: nsh_core::geometry::Point<Points>| {
        let scene = &ready.get_untracked()?.loaded.scene;
        let size = size.get_untracked();
        let base = fit(
            scene,
            f64::from(size.width.0),
            f64::from(size.height.0),
            props.resize_mode.get_untracked(),
        );
        Some(
            base.invert()?
                .apply(Point::new(f64::from(at.x.0), f64::from(at.y.0))),
        )
    };
    // A Rive file's listeners hear the pointer over it, as Rive's web
    // runtime has them: a finger (or a button) going down presses, moving
    // moves, lifting releases and leaves; the mouse moves over it, and
    // leaves.
    let listener = dynamic(move || {
        if !ready.get().is_some_and(|r| r.listens) {
            return Vec::new();
        }
        let touch = move |e: &TouchEvent| {
            let Some(p) = e.points.iter().find(|p| p.changed).or(e.points.first()) else {
                return;
            };
            let Some(at) = in_scene(p.location) else {
                return;
            };
            interacting.with(|i| i.last = Some(at));
            let kind = match e.phase {
                TouchPhase::Start => Pointer::Down,
                TouchPhase::Move => Pointer::Move,
                TouchPhase::End | TouchPhase::Cancel => Pointer::Lift,
            };
            act(Act::Pointer(kind, at));
        };
        let hover = move |e: &HoverEvent| {
            if let Some(at) = in_scene(e.position) {
                interacting.with(|i| i.last = Some(at));
                act(Act::Pointer(Pointer::Move, at));
            }
        };
        let left = move || {
            if let Some(at) = interacting.with(|i| i.last) {
                act(Act::Pointer(Pointer::Exit, at));
            }
        };
        vec![
            view()
                .absolute()
                .size_full()
                .test_id(POINTER_TEST_ID)
                .on_touch(touch)
                .on_hover_move(hover)
                .on_hover_out(left)
                .into_element(),
        ]
    });
    root.child(stage.child(dynamic(move || {
        let Some(r) = ready.get() else {
            return Vec::new();
        };
        let Some((scene, _)) = played.get() else {
            return Vec::new();
        };
        let size = size.get();
        let (w, h) = (f64::from(size.width.0), f64::from(size.height.0));
        if w <= 0.0 || h <= 0.0 {
            return Vec::new();
        }
        let base = fit(&scene, w, h, props.resize_mode.get());
        let pixels = f64::from(pixel_ratio()).max(1.0);
        if r.native
            && let Ok(plan) = plan::compile_for(&scene, base, pixels)
        {
            let nodes = Nodes::new(&plan.graph, time);
            let part = move |unit: &plan::Unit| match unit {
                plan::Unit::Raster(part) => rastered(part, base, (w, h), clock, time),
                plan::Unit::Sampled(part) => {
                    let part = part.clone();
                    sampled(move |t| part.frame_at(t), base, clock, time)
                }
                _ => view().into_element(),
            };
            return lower::elements(&plan.units, &nodes, &part);
        }
        if plan::needs_raster(&dxn_vector_animation::Node::Group(scene.root.clone())) {
            let scene = Arc::new(scene.as_ref().clone());
            return vec![rastered(&scene, base, (w, h), clock, time)];
        }
        vec![sampled(move |t| scene.frame_at(t), base, clock, time)]
    })))
    .child(listener)
}

/// A canvas drawing the frame `frame_at` the clock's time, drawn again as
/// the clock moves.
fn sampled(
    frame_at: impl Fn(f64) -> Frame + 'static,
    base: Matrix,
    clock: Signal<f64>,
    time: AnimatedValue,
) -> AnyElement {
    let empty = Graph::default();
    canvas(move || {
        let plan = plan::from_frame(&frame_at(clock.get()), base);
        let ops = match plan.units.first() {
            Some(plan::Unit::Canvas(ops)) => ops.clone(),
            _ => Vec::new(),
        };
        lower::painter(&ops, &Nodes::new(&empty, time))
    })
    .absolute()
    .size_full()
    .into_element()
}

/// A canvas drawing `scene` as the reference renderer draws it, at the
/// display's pixels, into a view of `size`: once where it does not move,
/// again as the clock moves where it does.
fn rastered(
    scene: &Arc<Scene>,
    base: Matrix,
    size: (f64, f64),
    clock: Signal<f64>,
    time: AnimatedValue,
) -> AnyElement {
    let view = plan::in_view(scene, base, size.0, size.1);
    let still = plan::still(scene);
    let empty = Graph::default();
    let made = RefCell::new(Rasters::default());
    let held = RefCell::new(Held::default());
    canvas(move || {
        let t = if still { 0.0 } else { clock.get() };
        let ratio = f64::from(pixel_ratio()).max(1.0);
        let raster = made
            .borrow_mut()
            .get(t, ratio, |t| plan::raster(&view, t, ratio));
        let ops = held.borrow_mut().ops(raster);
        lower::painter(&ops, &Nodes::new(&empty, time))
    })
    .absolute()
    .size_full()
    .into_element()
}

/// The images a rastered part draws, each frame's new: a platform that
/// loads an image on the side (the web does) has nothing to draw of it for
/// a moment, and a canvas drawing only the new frame is blank until the
/// image comes, every frame, so that the part flickers. The frame before
/// it is drawn too, and the new one replaces it where it is ready.
#[derive(Default)]
struct Held {
    /// The raster drawn before the one drawn now.
    before: Option<(Arc<ImageData>, Rect)>,
    /// The raster drawn now.
    now: Option<(Arc<ImageData>, Rect)>,
}

impl Held {
    /// What to draw of `raster`, the part's frame now.
    fn ops(&mut self, raster: Option<(Arc<ImageData>, Rect)>) -> Vec<plan::Op> {
        let Some((image, at)) = raster else {
            // Nothing to draw this frame: nothing of the one before either.
            *self = Held::default();
            return Vec::new();
        };
        if !self
            .now
            .as_ref()
            .is_some_and(|(now, _)| Arc::ptr_eq(now, &image))
        {
            self.before = self.now.take();
            self.now = Some((image.clone(), at));
        }
        let mut ops = Vec::new();
        if let Some((before, from)) = &self.before {
            ops.push(plan::Op::Image(before.clone(), *from));
            ops.push(plan::Op::Copy);
        }
        ops.push(plan::Op::Image(image, at));
        ops
    }
}

/// How many bytes of images a part keeps of the rasters it made.
const RASTER_BYTES: usize = 32 << 20;

/// How many rasters a second a part that moves makes, at most: each kept
/// for when its time comes again (a loop), within [`RASTER_BYTES`].
const RASTER_RATE: f64 = 60.0;

/// The rasters a part made, by frame.
#[derive(Default)]
struct Rasters {
    ratio: f64,
    by_frame: HashMap<i64, Option<(Arc<ImageData>, Rect)>>,
    bytes: usize,
}

impl Rasters {
    /// The raster at `t` (to the nearest frame) at `ratio`, made by `make`
    /// if it is not kept.
    fn get(
        &mut self,
        t: f64,
        ratio: f64,
        make: impl FnOnce(f64) -> Option<(Arc<ImageData>, Rect)>,
    ) -> Option<(Arc<ImageData>, Rect)> {
        if ratio != self.ratio {
            *self = Rasters {
                ratio,
                ..Default::default()
            };
        }
        let frame = (t * RASTER_RATE).round() as i64;
        if let Some(kept) = self.by_frame.get(&frame) {
            return kept.clone();
        }
        let made = make(frame as f64 / RASTER_RATE);
        let size = made.as_ref().map_or(0, |(image, _)| image.bytes.len());
        if self.bytes + size <= RASTER_BYTES {
            self.bytes += size;
            self.by_frame.insert(frame, made.clone());
        }
        made
    }
}

/// Reads `src` into a scene.
async fn load(src: Source) -> Result<Loaded, String> {
    match src {
        Source::Url(url) => {
            let response = fetch(url.as_str())
                .await
                .map_err(|e| format!("{url}: {e}"))?;
            if !response.ok() {
                return Err(format!("{url}: HTTP {}", response.status()));
            }
            parse(response.body).await
        }
        Source::Text(text) => parse(text.into_bytes()).await,
        Source::Bytes(bytes) => parse(bytes).await,
        Source::Json(value) => source::read_json(&value),
    }
}

/// Reads `bytes` off the engine thread where there are threads.
async fn parse(bytes: Vec<u8>) -> Result<Loaded, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        blocking(move || source::read(&bytes))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(target_arch = "wasm32")]
    {
        source::read(&bytes)
    }
}

fn info(loaded: &Loaded) -> AnimationInfo {
    let s = &loaded.scene;
    AnimationInfo {
        duration: s.timeline.duration,
        size: AnimationSize {
            width: s.width,
            height: s.height,
        },
        markers: s
            .timeline
            .markers
            .iter()
            .map(|m| AnimationMarker {
                name: m.name.clone(),
                start: m.start,
                duration: m.duration,
            })
            .collect(),
        events: s.events().into_iter().map(str::to_owned).collect(),
        inputs: loaded
            .interaction
            .as_ref()
            .and_then(Interaction::machine)
            .map(|m| {
                m.all()
                    .into_iter()
                    .flat_map(|m| {
                        m.inputs.iter().map(|i| {
                            let (kind, value) = match i.kind {
                                InputKind::Bool(b) => ("bool", f64::from(u8::from(b))),
                                InputKind::Number(n) => ("number", n),
                                InputKind::Trigger => ("trigger", 0.0),
                            };
                            AnimationInput {
                                name: i.name.clone(),
                                path: m.path.clone(),
                                kind: kind.into(),
                                value,
                            }
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        values: loaded
            .interaction
            .as_ref()
            .and_then(Interaction::view_model)
            .map(|vm| {
                vm.properties
                    .iter()
                    .map(|p| {
                        let (kind, value) = json_of(&p.value);
                        AnimationValue {
                            path: p.name.clone(),
                            kind: kind.into(),
                            value,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
        warnings: loaded.warnings.clone(),
    }
}

/// The span `segment` plays (all of the timeline without one).
fn segment(scene: &Scene, segment: Option<Segment>) -> (f64, f64) {
    let all = (0.0, scene.timeline.duration);
    match segment {
        None => all,
        Some(Segment::Span(a, b)) => (a.min(b).max(0.0), b.max(a).min(scene.timeline.duration)),
        Some(Segment::Marker(name)) => scene
            .timeline
            .markers
            .iter()
            .find(|m| m.name == name)
            .map_or(all, |m| (m.start, m.start + m.duration.max(0.0))),
    }
}

fn at_time(scene: &Scene, at: &At) -> f64 {
    match at {
        At::Seconds(s) => *s,
        At::Marker(name) => scene
            .timeline
            .markers
            .iter()
            .find(|m| &m.name == name)
            .map_or(0.0, |m| m.start),
    }
}

/// The matrix fitting `scene` into a view of `w` × `h` points.
pub fn fit(scene: &Scene, w: f64, h: f64, mode: ResizeMode) -> Matrix {
    let (sw, sh) = (scene.width.max(1e-9), scene.height.max(1e-9));
    let (sx, sy) = match mode {
        ResizeMode::Stretch => (w / sw, h / sh),
        ResizeMode::Contain => {
            let s = (w / sw).min(h / sh);
            (s, s)
        }
        ResizeMode::Cover => {
            let s = (w / sw).max(h / sh);
            (s, s)
        }
        ResizeMode::Center => (1.0, 1.0),
    };
    Matrix::new(sx, 0.0, 0.0, sy, (w - sw * sx) / 2.0, (h - sh * sy) / 2.0)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn image(bytes: usize) -> Option<(Arc<ImageData>, Rect)> {
        Some((
            Arc::new(ImageData {
                mime: "image/png".into(),
                bytes: vec![0; bytes],
            }),
            Rect::new(0.0, 0.0, 1.0, 1.0),
        ))
    }

    #[test]
    fn rasters_are_made_once_a_frame_and_kept() {
        let made = Cell::new(0);
        let mut rasters = Rasters::default();
        let make = |_| {
            made.set(made.get() + 1);
            image(10)
        };
        // A loop's frames, twice over: made the first time only.
        for _ in 0..2 {
            for k in 0..30 {
                rasters.get(f64::from(k) / 60.0 + 0.001, 2.0, make);
            }
        }
        assert_eq!(made.get(), 30);
        // At another ratio, made again.
        rasters.get(0.0, 3.0, make);
        assert_eq!(made.get(), 31);
    }

    #[test]
    fn a_raster_is_drawn_over_the_one_before_and_a_redraw_keeps_both() {
        let (a, b) = (image(1).unwrap(), image(2).unwrap());
        let mut held = Held::default();
        // The first has nothing before it.
        assert_eq!(held.ops(Some(a.clone())).len(), 1);
        // The next: the one before, then `copy`, then it.
        let ops = held.ops(Some(b.clone()));
        assert!(
            matches!(&ops[..], [plan::Op::Image(x, _), plan::Op::Copy, plan::Op::Image(y, _)]
            if Arc::ptr_eq(x, &a.0) && Arc::ptr_eq(y, &b.0))
        );
        // The same again (a redraw): the same.
        assert_eq!(held.ops(Some(b.clone())).len(), 3);
        // Nothing to draw: nothing is kept.
        assert!(held.ops(None).is_empty());
        assert_eq!(held.ops(Some(a)).len(), 1);
    }

    #[test]
    fn rasters_past_the_budget_are_not_kept() {
        let made = Cell::new(0);
        let mut rasters = Rasters::default();
        let make = |_| {
            made.set(made.get() + 1);
            image(RASTER_BYTES / 2 + 1)
        };
        for _ in 0..2 {
            for k in 0..3 {
                rasters.get(f64::from(k), 1.0, make);
            }
        }
        // The first kept; the others made each time.
        assert_eq!(made.get(), 5);
    }
}
