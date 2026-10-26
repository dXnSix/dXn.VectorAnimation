//! A Rive state machine played as its runtime plays it with nothing
//! changing its inputs (`StateMachineLayerInstance`, `StateTransition`,
//! `LinearAnimationInstance`): each layer stepped a frame at a time, its
//! transitions taken when their conditions hold for the inputs' values and
//! their exit times have come, mixing from one state into the next. What
//! each layer applies at each frame is recorded for the converter. Times
//! are 32-bit floats, as the runtime keeps them, so a frame lands on the
//! same side of an exit time as there.

use dxn_vector_animation::LoopMode;

use crate::curve::Interp;

/// An animation as a state plays it: its work area in seconds, its speed
/// and its loop.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Anim {
    pub start: f64,
    pub end: f64,
    pub speed: f64,
    pub loop_mode: LoopMode,
}

impl Anim {
    fn duration(&self) -> f64 {
        (self.end - self.start).abs()
    }
}

/// A transition's condition on an input's value (bools 0 or 1).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Cond {
    /// Holds when the bool input `0` is (`true`) or is not (`false`) set.
    Bool(usize, bool),
    /// Holds when the number input compares (Rive's op 0 to 5) to the value.
    Number(usize, u64, f64),
    /// A trigger: holds only when fired, which nothing does here.
    Trigger(usize),
    /// A condition of a kind not read: holds as `true` says.
    Fixed(bool),
}

/// A state's transition.
#[derive(Debug, Clone)]
pub(crate) struct Trans {
    pub to: usize,
    pub flags: u64,
    pub duration: f64,
    pub exit_time: f64,
    pub conditions: Vec<Cond>,
    pub ease: Option<Interp>,
}

const DISABLED: u64 = 1;
const DURATION_IS_PERCENTAGE: u64 = 1 << 1;
const ENABLE_EXIT_TIME: u64 = 1 << 2;
const EXIT_TIME_IS_PERCENTAGE: u64 = 1 << 3;
const PAUSE_ON_EXIT: u64 = 1 << 4;
const ENABLE_EARLY_EXIT: u64 = 1 << 5;

/// What a state is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Entry,
    Exit,
    Any,
    /// Plays an animation (an index into the animations), or nothing.
    Animation(Option<usize>),
    /// A state of a kind not played here (a blend state): plays nothing.
    Other,
}

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub kind: Kind,
    pub transitions: Vec<Trans>,
    /// How fast it plays its animation (backwards when negative, from its
    /// end).
    pub speed: f64,
}

/// What a layer applies at a frame: the animation of its state and that
/// animation's time, and while it mixes in, the state it mixes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Applied {
    /// Counts the states the layer has entered: the same while it stays.
    pub serial: u32,
    pub current: Option<(usize, f64)>,
    /// The rate the current animation plays at, on its own speed: its
    /// state's speed, ping-pong turning it.
    pub direction: f64,
    pub from: Option<(usize, f64)>,
    pub mix: f64,
    pub from_mix: f64,
    pub ease: Option<Interp>,
    /// The frame the transition mixing now began at.
    pub began: usize,
}

/// An animation playing (`LinearAnimationInstance`).
#[derive(Debug, Clone, Copy)]
struct Playing {
    anim: usize,
    /// Its state's speed.
    speed: f32,
    time: f32,
    total: f32,
    last_total: f32,
    spilled: f32,
    direction: f32,
}

impl Playing {
    fn new(anim: usize, a: &Anim, speed: f64) -> Playing {
        Playing {
            anim,
            speed: speed as f32,
            time: if speed >= 0.0 { a.start } else { a.end } as f32,
            total: 0.0,
            last_total: 0.0,
            spilled: 0.0,
            direction: 1.0,
        }
    }

    fn keep_going(&self, a: &Anim, multiplier: f32) -> bool {
        let directed = a.speed as f32 * self.direction * multiplier;
        a.loop_mode != LoopMode::Once
            || (directed > 0.0 && self.time < a.end as f32)
            || (directed < 0.0 && self.time > a.start as f32)
    }

    /// `seconds` on (`AnimationStateInstance::advance`).
    fn advance(&mut self, seconds: f32, a: &Anim) {
        let elapsed = seconds * self.speed;
        let delta = elapsed * a.speed as f32 * self.direction;
        self.spilled = 0.0;
        if delta == 0.0 {
            return;
        }
        self.last_total = self.total;
        self.total += delta.abs();
        let kill_spilled = !self.keep_going(a, elapsed);
        self.time += delta;
        let (start, end) = (a.start as f32, a.end as f32);
        let range = end - start;
        let forward = delta > 0.0;
        match a.loop_mode {
            LoopMode::Once => {
                if forward && self.time > end {
                    self.spilled = (self.time - end) / delta * elapsed;
                    self.time = end;
                } else if !forward && self.time < start {
                    self.spilled = (start - self.time) / delta.abs() * elapsed;
                    self.time = start;
                }
            }
            LoopMode::Loop if range > 0.0 => {
                if forward && self.time >= end {
                    let remainder = (self.time - start) % range;
                    self.spilled = remainder / delta * elapsed;
                    self.time = start + remainder;
                } else if !forward && self.time <= start {
                    let remainder = ((start - self.time) % range).abs();
                    self.spilled = (remainder / delta).abs() * elapsed;
                    self.time = end - remainder;
                }
            }
            LoopMode::PingPong if range > 0.0 => {
                let mut forward = forward;
                for _ in 0..64 {
                    if forward && self.time >= end {
                        self.spilled = self.time - end;
                        self.time = end + (end - self.time);
                    } else if !forward && self.time < start {
                        self.spilled = start - self.time;
                        self.time = start + (start - self.time);
                    } else {
                        break;
                    }
                    self.direction = -self.direction;
                    forward = !forward;
                }
            }
            _ => {}
        }
        if kill_spilled {
            self.spilled = 0.0;
        }
    }
}

/// A state entered.
#[derive(Debug, Clone, Copy)]
struct Entered {
    state: usize,
    playing: Option<Playing>,
}

/// A layer playing.
struct Layer<'a> {
    states: &'a [State],
    anims: &'a [Anim],
    inputs: Vec<Option<f64>>,
    current: Option<Entered>,
    from: Option<Entered>,
    any: Option<usize>,
    transition: Option<Trans>,
    mix: f32,
    mix_from: f32,
    hold_from: bool,
    serial: u32,
    began: usize,
}

impl Layer<'_> {
    fn enter(&mut self, state: usize) {
        let playing = match self.states.get(state) {
            Some(&State {
                kind: Kind::Animation(Some(a)),
                speed,
                ..
            }) => self.anims.get(a).map(|an| Playing::new(a, an, speed)),
            _ => None,
        };
        self.current = Some(Entered { state, playing });
        self.serial += 1;
    }

    fn advance_state(&self, e: &mut Entered, seconds: f32) {
        if let Some(p) = &mut e.playing
            && let Some(a) = self.anims.get(p.anim)
        {
            p.advance(seconds, a);
        }
    }

    fn mix_time(&self, t: &Trans) -> f32 {
        if t.duration == 0.0 {
            return 0.0;
        }
        if t.flags & DURATION_IS_PERCENTAGE != 0 {
            let d = self
                .from
                .and_then(|f| f.playing)
                .and_then(|p| self.anims.get(p.anim))
                .map_or(0.0, Anim::duration);
            t.duration as f32 / 100.0 * d as f32
        } else {
            t.duration as f32 / 1000.0
        }
    }

    fn update_mix(&mut self, seconds: f32) {
        match &self.transition {
            Some(t) if self.from.is_some() && t.duration != 0.0 => {
                let mix_time = self.mix_time(t);
                self.mix = if mix_time == 0.0 {
                    1.0
                } else {
                    (self.mix + seconds / mix_time).clamp(0.0, 1.0)
                };
            }
            _ => self.mix = 1.0,
        }
    }

    fn is_transitioning(&self) -> bool {
        self.transition
            .as_ref()
            .is_some_and(|t| self.from.is_some() && t.duration != 0.0 && self.mix < 1.0)
    }

    fn holds(&self, c: &Cond) -> bool {
        // Rive takes a condition on an input it does not know as holding.
        let input = |i: usize| self.inputs.get(i).copied().flatten();
        match *c {
            Cond::Bool(i, equal) => input(i).is_none_or(|v| (v != 0.0) == equal),
            Cond::Number(i, op, value) => input(i).is_none_or(|v| match op {
                0 => v == value,
                1 => v != value,
                2 => v <= value,
                3 => v >= value,
                4 => v < value,
                5 => v > value,
                _ => false,
            }),
            Cond::Trigger(i) => input(i).is_none(),
            Cond::Fixed(b) => b,
        }
    }

    /// Whether `t` may be taken from `from` now (`StateTransition::allowed`).
    fn allowed(&self, t: &Trans, from: &Entered) -> bool {
        if t.flags & DISABLED != 0 || !t.conditions.iter().all(|c| self.holds(c)) {
            return false;
        }
        if t.flags & ENABLE_EXIT_TIME != 0
            && let Some(p) = from.playing
            && let Some(a) = self.anims.get(p.anim)
        {
            let duration = a.duration() as f32;
            let mut exit = if t.flags & EXIT_TIME_IS_PERCENTAGE != 0 {
                t.exit_time as f32 / 100.0 * duration
            } else {
                t.exit_time as f32 / 1000.0
            };
            if exit <= duration && a.loop_mode != LoopMode::Once && duration > 0.0 {
                exit += (p.last_total / duration).floor() * duration;
            }
            if p.total < exit {
                return false;
            }
        }
        true
    }

    /// Takes the first transition allowed from `from`
    /// (`StateMachineLayerInstance::tryChangeState`).
    fn try_change(&mut self, from: Entered, frame: usize) -> bool {
        let current = self.current.map(|c| c.state);
        let Some(t) = self
            .states
            .get(from.state)
            .and_then(|s| {
                s.transitions
                    .iter()
                    .find(|t| Some(t.to) != current && self.allowed(t, &from))
            })
            .cloned()
        else {
            return false;
        };
        let out = self.current;
        self.enter(t.to);
        let mut out = out;
        // Paused at the exit time when the transition says.
        if let Some(o) = &mut out
            && t.flags & ENABLE_EXIT_TIME != 0
            && t.flags & PAUSE_ON_EXIT != 0
            && let Some(p) = &mut o.playing
            && let Some(a) = self.anims.get(p.anim)
        {
            p.time = if t.flags & EXIT_TIME_IS_PERCENTAGE != 0 {
                (a.start + t.exit_time / 100.0 * a.duration()) as f32
            } else {
                t.exit_time as f32 / 1000.0
            };
        }
        self.from = out;
        self.mix_from = self.mix;
        if self.mix != 0.0 {
            self.hold_from = t.flags & PAUSE_ON_EXIT != 0;
        }
        let spilled = self.from.and_then(|f| f.playing).map_or(0.0, |p| p.spilled);
        self.transition = Some(t);
        if let Some(mut c) = self.current {
            self.advance_state(&mut c, spilled);
            self.current = Some(c);
        }
        self.mix = 0.0;
        self.update_mix(0.0);
        self.began = frame;
        true
    }

    fn update_state(&mut self, frame: usize) -> bool {
        if self.is_transitioning()
            && self
                .transition
                .as_ref()
                .is_some_and(|t| t.flags & ENABLE_EARLY_EXIT == 0)
        {
            return false;
        }
        if let Some(any) = self.any
            && self.try_change(
                Entered {
                    state: any,
                    playing: None,
                },
                frame,
            )
        {
            return true;
        }
        match self.current {
            Some(c) => self.try_change(c, frame),
            None => false,
        }
    }

    /// One frame of `seconds` (`StateMachineLayerInstance::advance`).
    fn advance(&mut self, seconds: f32, frame: usize) -> Applied {
        if let Some(mut c) = self.current {
            self.advance_state(&mut c, seconds);
            self.current = Some(c);
        }
        self.update_mix(seconds);
        if self.mix < 1.0
            && !self.hold_from
            && let Some(mut f) = self.from
        {
            self.advance_state(&mut f, seconds);
            self.from = Some(f);
        }
        for _ in 0..100 {
            if !self.update_state(frame) {
                break;
            }
        }
        if let Some(c) = &mut self.current
            && let Some(p) = &mut c.playing
        {
            p.spilled = 0.0;
        }
        let playing = self.current.and_then(|c| c.playing);
        Applied {
            serial: self.serial,
            current: playing.map(|p| (p.anim, f64::from(p.time))),
            direction: playing.map_or(1.0, |p| f64::from(p.direction * p.speed)),
            from: if self.mix < 1.0 {
                self.from
                    .and_then(|f| f.playing)
                    .map(|p| (p.anim, f64::from(p.time)))
            } else {
                None
            },
            mix: f64::from(self.mix),
            from_mix: f64::from(self.mix_from),
            ease: self.transition.as_ref().and_then(|t| t.ease),
            began: self.began,
        }
    }
}

/// What layer `states` applies at each of `frames` frames of `dt` seconds
/// from its entry, the inputs at each frame as `inputs` says (`None`: an
/// input of a kind not read).
pub(crate) fn play(
    states: &[State],
    anims: &[Anim],
    inputs: &dyn Fn(usize) -> Vec<Option<f64>>,
    frames: usize,
    dt: f64,
) -> Vec<Applied> {
    let mut layer = Layer {
        states,
        anims,
        inputs: Vec::new(),
        current: None,
        from: None,
        any: states.iter().position(|s| s.kind == Kind::Any),
        transition: None,
        mix: 1.0,
        mix_from: 1.0,
        hold_from: false,
        serial: 0,
        began: 0,
    };
    if let Some(entry) = states.iter().position(|s| s.kind == Kind::Entry) {
        layer.enter(entry);
    }
    (0..frames)
        .map(|k| {
            layer.inputs = inputs(k);
            layer.advance(if k == 0 { 0.0 } else { dt as f32 }, k)
        })
        .collect()
}
