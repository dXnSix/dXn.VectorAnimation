//! A Rive artboard turned into a scene: its drawables in draw order, each
//! under its ancestors' transforms and clips, their fills and strokes, and
//! the artboard's linear animations laid one after another on the timeline.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, hash_map::Entry},
    f64::consts::PI,
    rc::Rc,
    sync::Arc,
};

use dxn_vector_animation::{
    Animated, AnimatedPoint, BlendMode, Blur, Clip, ClipShape, Color, Dash, Direction, Easing,
    Effect, EllipseStart, Error, Fill, FillRule, Geometry, GradientStop, GradientUnits, Group,
    Image, ImageData, Keyframe, Limits, LineCap, LineJoin, LinearGradient, LoopMode, Marker,
    Matrix, Node, Paint, Path, Point, RadialGradient, Rect, RectStart, Scene, Shape, ShapePath,
    Spread, Stroke, TimeMap, Timeline, Track, TransformOp, Trim, TrimMode,
    outline::{geometry_path, path_length},
    transform::transform_at,
};

use crate::{
    curve::{Baker, Break, Breaks, Curve, Interp, Key, Keyed, sub_easing},
    defs::{self, props as p, types as t},
    file::{Artboard, File, KeyedProperty, Object},
    geometry::{self, Vertex},
    machine, modifiers, order,
    rig::{self, Rig},
    text,
};

/// The properties the scene animates (of the objects it draws).
const DRAWN: &[u16] = &[
    p::NODE_X,
    p::NODE_Y,
    p::TRANSFORM_COMPONENT_ROTATION,
    p::TRANSFORM_COMPONENT_SCALE_X,
    p::TRANSFORM_COMPONENT_SCALE_Y,
    p::WORLD_TRANSFORM_COMPONENT_OPACITY,
    p::DRAWABLE_DRAWABLE_FLAGS,
    p::VERTEX_X,
    p::VERTEX_Y,
    p::STRAIGHT_VERTEX_RADIUS,
    p::CUBIC_MIRRORED_VERTEX_ROTATION,
    p::CUBIC_MIRRORED_VERTEX_DISTANCE,
    p::CUBIC_ASYMMETRIC_VERTEX_ROTATION,
    p::CUBIC_ASYMMETRIC_VERTEX_IN_DISTANCE,
    p::CUBIC_ASYMMETRIC_VERTEX_OUT_DISTANCE,
    p::CUBIC_DETACHED_VERTEX_IN_ROTATION,
    p::CUBIC_DETACHED_VERTEX_IN_DISTANCE,
    p::CUBIC_DETACHED_VERTEX_OUT_ROTATION,
    p::CUBIC_DETACHED_VERTEX_OUT_DISTANCE,
    p::PARAMETRIC_PATH_WIDTH,
    p::PARAMETRIC_PATH_HEIGHT,
    p::PARAMETRIC_PATH_ORIGIN_X,
    p::PARAMETRIC_PATH_ORIGIN_Y,
    p::RECTANGLE_CORNER_RADIUS_TL,
    p::RECTANGLE_CORNER_RADIUS_TR,
    p::RECTANGLE_CORNER_RADIUS_BL,
    p::RECTANGLE_CORNER_RADIUS_BR,
    p::POLYGON_POINTS,
    p::POLYGON_CORNER_RADIUS,
    p::STAR_INNER_RADIUS,
    p::SOLID_COLOR_COLOR_VALUE,
    p::GRADIENT_STOP_COLOR_VALUE,
    p::GRADIENT_STOP_POSITION,
    p::LINEAR_GRADIENT_START_X,
    p::LINEAR_GRADIENT_START_Y,
    p::LINEAR_GRADIENT_END_X,
    p::LINEAR_GRADIENT_END_Y,
    p::LINEAR_GRADIENT_OPACITY,
    p::SHAPE_PAINT_IS_VISIBLE,
    p::STROKE_THICKNESS,
    p::TRIM_PATH_START,
    p::TRIM_PATH_END,
    p::TRIM_PATH_OFFSET,
    p::DASH_LENGTH,
    p::DASH_PATH_OFFSET,
    p::CLIPPING_SHAPE_IS_VISIBLE,
    p::SOLO_ACTIVE_COMPONENT_ID,
];

/// Seconds between one animation's span on the timeline and the next's.
const GAP: f64 = 1.0;

/// Where an animation plays on the timeline: from `start` for `duration`,
/// its own seconds from `from` to `to` at `speed` (backwards from `to` when
/// negative, held at `from` when 0). The scene's timeline lays the
/// artboard's animations out one after another; a nested artboard plays its
/// animations in pieces of the parent's timeline.
#[derive(Debug, Clone)]
struct Segment {
    animation: usize,
    name: String,
    /// The timeline's seconds it starts at and lasts.
    start: f64,
    duration: f64,
    /// The animation's own seconds it plays from and to.
    from: f64,
    to: f64,
    speed: f64,
    fps: f64,
    loop_mode: LoopMode,
}

impl Segment {
    /// The timeline's time of the animation's own `s`.
    fn time(&self, s: f64) -> f64 {
        if self.speed > 0.0 {
            self.start + (s - self.from) / self.speed
        } else {
            self.start + (self.to - s) / -self.speed
        }
    }
}

/// What a text's layout depends on at a time: its runs' strings and the
/// numbers that size and space it.
type Layout = (Vec<String>, Vec<f64>);

/// How a nested animation plays over the scene's time.
#[derive(Debug, Clone)]
enum Pieces {
    /// At a rate (0 while it does not play).
    Rate(Curve<f64>),
    /// At the fraction of its duration keyed.
    Remap(Curve<f64>),
}

/// What an artboard plays over the scene's time.
#[derive(Debug, Clone)]
enum Clock {
    /// Its animations one after another, the one its default state machine
    /// enters first: the scene's own artboard.
    Timeline,
    /// Layers of pieces of its animations, the later overriding the
    /// earlier where they key the same property, over the parent's spans:
    /// a nested artboard.
    Layers {
        layers: Vec<Vec<Segment>>,
        spans: Vec<(f64, f64)>,
        machine: Option<NestedMachine>,
    },
}

/// The conversion of one artboard.
pub(crate) struct Converter<'a> {
    file: &'a File,
    ab: &'a Artboard,
    limits: Limits,
    /// How deep in nested artboards this one is.
    depth: usize,
    pub(crate) warnings: BTreeSet<String>,
    /// The timeline's animations (the scene's own artboard).
    segments: Vec<Segment>,
    /// What plays: each property is keyed by the last layer keying it.
    layers: Vec<Vec<Segment>>,
    /// The spans of the scene's animations, on which nested artboards play.
    spans: Vec<(f64, f64)>,
    /// The timeline of an artboard without animations whose nested
    /// artboards play: the longest of what they play, and its loop.
    nested_only: Option<(f64, LoopMode)>,
    /// What each animation keys: (object, property) to (animation, keys).
    keyed: HashMap<(usize, u16), Vec<(usize, &'a KeyedProperty)>>,
    /// The keyed properties the scene uses.
    used: HashSet<(usize, u16)>,
    parent: Vec<Option<usize>>,
    children: Vec<Vec<usize>>,
    baker: Baker,
    /// The components' world transforms with their constraints.
    rig: Rig,
    /// The transforms of the components constraints move, baked.
    constrained: HashMap<usize, Vec<TransformOp>>,
    /// The clips of the clipping shapes, made once.
    clips: HashMap<usize, Option<Clip>>,
    /// The default state machine as it plays, first on the timeline, and
    /// the changes made to its inputs.
    plan: Option<Rc<Plan>>,
    changes: Vec<crate::InputChange>,
    /// The nodes listeners moved to the pointer.
    aligned: Vec<crate::Aligned>,
    /// A nested artboard's state machine: the layer it is, and what it
    /// plays in each span.
    machine_layer: Option<usize>,
    nested_plans: Vec<Rc<Plan>>,
    /// The events the timeline reports: what the state machine fires and
    /// what the animations played key, as cues.
    cues: Vec<dxn_vector_animation::Cue>,
    /// What the default state machine's listeners listen on: their shapes
    /// are kept (unpainted when they paint nothing) for a player's pointer
    /// to hit.
    hit_targets: Vec<usize>,
}

/// Seconds a frame of a state machine played.
pub(crate) const DT: f64 = 1.0 / 60.0;

/// The frames a state machine is played over at most (ten minutes).
const MAX_FRAMES: usize = 10 * 60 * 60;

/// A state machine ready to play: its layers' states, the animations they
/// play, their frame rates, and its inputs' values.
struct Machine {
    name: String,
    layers: Vec<Vec<machine::State>>,
    anims: Vec<machine::Anim>,
    fps: Vec<f64>,
    inputs: Vec<Option<f64>>,
    /// Each input's name.
    names: Vec<String>,
}

/// A nested artboard's state machine, as its parent plays it: the layer of
/// its nested animations it is, the machine, its inputs the parent sets
/// (an index and its value on the parent's clock), the parent's time each
/// span's inputs are read from, and its speed.
#[derive(Debug, Clone)]
struct NestedMachine {
    layer: usize,
    machine: usize,
    inputs: Vec<(usize, Curve<f64>)>,
    /// `None`: the inputs at their own values.
    sources: Vec<Option<f64>>,
    speed: f64,
    /// Played in the first span alone.
    first_only: bool,
}

/// A state machine as Rive's runtime plays it: what each layer applies at
/// each frame from `offset`.
struct Plan {
    name: String,
    layers: Vec<Vec<machine::Applied>>,
    anims: Vec<machine::Anim>,
    /// Each animation's frame rate.
    fps: Vec<f64>,
    /// The scene's seconds a frame, and the machine's seconds a scene second.
    dt: f64,
    speed: f64,
    /// The scene time of its first frame.
    offset: f64,
    /// The frames where a layer enters a state, starts or stops mixing, or
    /// mixes (each frame of a mix), and the end.
    bounds: Vec<usize>,
    /// The seconds it plays over, and where it repeats from (`None`: it
    /// holds at the end).
    span: f64,
    loop_start: Option<f64>,
}

/// Each glyph of a modified text at a time: where it goes and its opacity.
type Placed = Vec<(Matrix, f64)>;

/// The number properties of constraints.
const CONSTRAINT_KEYS: &[u16] = &[
    p::CONSTRAINT_STRENGTH,
    p::IK_CONSTRAINT_INVERT_DIRECTION,
    p::DISTANCE_CONSTRAINT_DISTANCE,
    p::DISTANCE_CONSTRAINT_MODE_VALUE,
    p::TRANSFORM_SPACE_CONSTRAINT_SOURCE_SPACE_VALUE,
    p::TRANSFORM_SPACE_CONSTRAINT_DEST_SPACE_VALUE,
    p::TRANSFORM_COMPONENT_CONSTRAINT_COPY_FACTOR,
    p::TRANSFORM_COMPONENT_CONSTRAINT_MIN_VALUE,
    p::TRANSFORM_COMPONENT_CONSTRAINT_MAX_VALUE,
    p::TRANSFORM_COMPONENT_CONSTRAINT_Y_COPY_FACTOR_Y,
    p::TRANSFORM_COMPONENT_CONSTRAINT_Y_MIN_VALUE_Y,
    p::TRANSFORM_COMPONENT_CONSTRAINT_Y_MAX_VALUE_Y,
    p::TRANSFORM_COMPONENT_CONSTRAINT_OFFSET,
    p::TRANSFORM_COMPONENT_CONSTRAINT_DOES_COPY,
    p::TRANSFORM_COMPONENT_CONSTRAINT_MIN,
    p::TRANSFORM_COMPONENT_CONSTRAINT_MAX,
    p::TRANSFORM_COMPONENT_CONSTRAINT_Y_DOES_COPY_Y,
    p::TRANSFORM_COMPONENT_CONSTRAINT_Y_MIN_Y,
    p::TRANSFORM_COMPONENT_CONSTRAINT_Y_MAX_Y,
    p::TRANSFORM_COMPONENT_CONSTRAINT_MIN_MAX_SPACE_VALUE,
    p::FOLLOW_PATH_CONSTRAINT_DISTANCE,
    p::FOLLOW_PATH_CONSTRAINT_ORIENT,
    p::FOLLOW_PATH_CONSTRAINT_OFFSET,
];

/// The objects of types a phase of the importer to come draws, by what the
/// warning calls them.
const LATER: &[(u16, &str)] = &[
    (t::DRAGGABLE_CONSTRAINT, "draggable constraints"),
    (t::SCROLL_CONSTRAINT, "scroll constraints"),
    (
        t::LIST_FOLLOW_PATH_CONSTRAINT,
        "list follow-path constraints",
    ),
    (t::LAYOUT_COMPONENT, "layout (drawn where the file puts it)"),
];

impl<'a> Converter<'a> {
    /// The scene of the artboard named `name` (the first without one).
    pub(crate) fn run(
        file: &'a File,
        options: &crate::Options,
        limits: Limits,
    ) -> Result<(Scene, BTreeSet<String>), Error> {
        let index = match options.artboard.as_deref() {
            Some(name) => file
                .artboards
                .iter()
                .position(|a| a.name() == name)
                .ok_or_else(|| Error::Invalid(format!("no artboard named {name:?}")))?,
            None if file.artboards.is_empty() => {
                return Err(Error::Invalid("the file has no artboard".into()));
            }
            None => 0,
        };
        let (scene, mut warnings) =
            Self::convert(file, index, Clock::Timeline, options, 0, limits)?;
        let others = file.artboards.len() - 1;
        if others > 0 {
            warnings.insert(format!(
                "{others} other artboard(s) in the file (`Options::artboard` reads one)"
            ));
        }
        Ok((scene, warnings))
    }

    /// The scene of artboard `index` playing what `clock` says, and its
    /// warnings.
    fn convert(
        file: &'a File,
        index: usize,
        clock: Clock,
        options: &crate::Options,
        depth: usize,
        limits: Limits,
    ) -> Result<(Scene, BTreeSet<String>), Error> {
        let ab = file
            .artboards
            .get(index)
            .ok_or_else(|| Error::Invalid(format!("no artboard {index}")))?;
        let n = ab.objects.len();
        let mut parent = vec![None; n];
        let mut children = vec![Vec::new(); n];
        for (i, o) in ab.objects.iter().enumerate().skip(1) {
            let Some(o) = o else { continue };
            if !o.is(t::COMPONENT) {
                continue;
            }
            if let Some(pi) = o
                .id(p::COMPONENT_PARENT_ID)
                .filter(|pi| *pi < n && *pi != i)
            {
                parent[i] = Some(pi);
                children[pi].push(i);
            }
        }
        let mut c = Converter {
            file,
            ab,
            limits,
            depth,
            warnings: BTreeSet::new(),
            segments: Vec::new(),
            layers: Vec::new(),
            spans: Vec::new(),
            nested_only: None,
            keyed: HashMap::new(),
            used: HashSet::new(),
            parent,
            children,
            baker: Baker {
                rate: 60.0,
                budget: limits.max_keyframes / 2,
                over: false,
            },
            rig: Rig::default(),
            constrained: HashMap::new(),
            clips: HashMap::new(),
            plan: None,
            changes: options.inputs.clone(),
            aligned: options.aligned.clone(),
            machine_layer: None,
            nested_plans: Vec::new(),
            hit_targets: Vec::new(),
            cues: Vec::new(),
        };
        c.index_keys();
        match clock {
            Clock::Timeline => {
                c.plan = c.machine_plan().map(Rc::new);
                if c.plan.is_some() {
                    c.hit_targets = c.listener_targets();
                }
                c.lay_out_animations();
                c.layers = vec![c.segments.clone()];
                c.spans = c
                    .plan
                    .iter()
                    .map(|p| (0.0, p.span))
                    .chain(c.segments.iter().map(|s| (s.start, s.duration)))
                    .collect();
                if c.segments.is_empty() && !ab.state_machines.is_empty() {
                    // Without animations of its own, the timeline of an
                    // artboard playing a state machine is what its nested
                    // artboards play (with nothing playing, Rive advances
                    // nothing).
                    c.nested_only = c.nested_span();
                    c.spans = c.nested_only.iter().map(|(d, _)| (0.0, *d)).collect();
                }
            }
            Clock::Layers {
                layers,
                spans,
                machine,
            } => {
                c.layers = layers;
                c.spans = spans;
                if let Some(m) = machine {
                    c.machine_layer = Some(m.layer);
                    c.hit_targets = c.listener_targets_of(m.machine);
                    c.nested_plans = c.nested_machine_plans(&m);
                }
            }
        }
        c.add_joysticks()?;
        c.build_rig();
        let scene = c.scene()?;
        c.left_out();
        Ok((scene, c.warnings))
    }

    fn object(&self, i: usize) -> Option<&'a Object> {
        self.ab.objects.get(i).and_then(Option::as_ref)
    }

    fn warn(&mut self, message: impl Into<String>) {
        self.warnings.insert(message.into());
    }

    // ---- The timeline -------------------------------------------------

    /// Lays the animations on the timeline: the one played first (the
    /// default state machine's entry) at 0, the others after it in the
    /// file's order, each under a marker of its name.
    fn lay_out_animations(&mut self) {
        let animations = &self.ab.animations;
        // After the state machine when it plays first; else the animation
        // its entry plays first.
        let first = match self.plan {
            Some(_) => 0,
            None => entry_animation(self.ab, None).unwrap_or(0),
        };
        let order =
            std::iter::once(first).chain((0..animations.len()).filter(move |i| *i != first));
        let mut start = self.plan.as_ref().map_or(0.0, |p| p.span + GAP);
        for i in order {
            let Some(spec) = animations.get(i).map(|a| Spec::of(&a.object)) else {
                continue;
            };
            let duration = spec.duration();
            self.segments.push(Segment {
                animation: i,
                name: spec.name,
                start,
                duration,
                from: spec.from,
                to: spec.to,
                speed: spec.speed,
                fps: spec.fps,
                loop_mode: spec.loop_mode,
            });
            start += duration + GAP;
        }
    }

    /// Indexes what each animation keys.
    fn index_keys(&mut self) {
        for (ai, a) in self.ab.animations.iter().enumerate() {
            for k in &a.keyed {
                let Some(object) = k.object else { continue };
                for prop in &k.properties {
                    self.keyed
                        .entry((object, prop.key))
                        .or_default()
                        .push((ai, prop));
                }
            }
        }
    }

    fn timeline(&self) -> Timeline {
        let first = self.segments.first();
        if let (None, Some((duration, loop_mode))) = (first, self.nested_only) {
            return Timeline {
                duration,
                frame_rate: None,
                loop_start: 0.0,
                loop_mode,
                markers: Vec::new(),
                cues: Vec::new(),
            };
        }
        let markers = self.segments.iter().map(|s| Marker {
            loop_mode: Some(s.loop_mode),
            ..Marker::new(s.name.clone(), s.start, s.duration)
        });
        if let Some(plan) = &self.plan {
            let loop_mode = match plan.loop_start {
                Some(_) => LoopMode::Loop,
                None => LoopMode::Once,
            };
            return Timeline {
                duration: plan.span,
                frame_rate: None,
                loop_start: plan.loop_start.unwrap_or(0.0),
                loop_mode,
                markers: std::iter::once(Marker {
                    loop_mode: Some(loop_mode),
                    ..Marker::new(plan.name.clone(), 0.0, plan.span)
                })
                .chain(markers)
                .collect(),
                cues: self.cues.clone(),
            };
        }
        Timeline {
            duration: first.map_or(0.0, |s| s.duration),
            frame_rate: first.map(|s| s.fps),
            loop_start: 0.0,
            loop_mode: first.map_or(LoopMode::Once, |s| s.loop_mode),
            markers: markers.collect(),
            cues: self.cues.clone(),
        }
    }

    // ---- Properties ----------------------------------------------------

    /// How keyframe `k` moves on to the next.
    fn interp(&self, k: &Object) -> Interp {
        if !(k.ty == t::KEY_FRAME_DOUBLE || k.ty == t::KEY_FRAME_COLOR) {
            return Interp::Hold;
        }
        if k.uint(p::INTERPOLATING_KEY_FRAME_INTERPOLATION_TYPE) == 0 {
            return Interp::Hold;
        }
        match k
            .id(p::INTERPOLATING_KEY_FRAME_INTERPOLATOR_ID)
            .and_then(|i| self.object(i))
        {
            Some(i) => interpolator(i),
            None => Interp::Linear,
        }
    }

    // ---- The state machine -----------------------------------------------

    /// The default state machine: the artboard's, else its first.
    fn default_machine(&self) -> Option<usize> {
        let ab = self.ab;
        ab.object()
            .id(p::ARTBOARD_DEFAULT_STATE_MACHINE_ID)
            .filter(|i| *i < ab.state_machines.len())
            .or((!ab.state_machines.is_empty()).then_some(0))
    }

    /// The objects the default state machine's listeners listen on.
    fn listener_targets(&self) -> Vec<usize> {
        match self.default_machine() {
            Some(i) => self.listener_targets_of(i),
            None => Vec::new(),
        }
    }

    /// The objects state machine `index`'s listeners listen on.
    fn listener_targets_of(&self, index: usize) -> Vec<usize> {
        let Some(sm) = self.ab.state_machines.get(index) else {
            return Vec::new();
        };
        sm.listeners
            .iter()
            .filter_map(|l| l.object.id(p::STATE_MACHINE_LISTENER_TARGET_ID))
            .filter(|&i| i < self.ab.objects.len())
            .collect()
    }

    /// The default state machine (the artboard's, else its first) played
    /// with its inputs as they are, a frame each 1/60 s until it settles
    /// into what repeats.
    fn machine_plan(&mut self) -> Option<Plan> {
        let index = self.default_machine()?;
        let Machine {
            name,
            layers,
            anims,
            fps,
            inputs,
            names,
        } = self.machine(index)?;
        // The changes made, each at the first frame at or after its time:
        // (frame, input, value; `None` fires a trigger).
        let mut changes: Vec<(usize, usize, Option<f64>)> = Vec::new();
        for c in self
            .changes
            .clone()
            .into_iter()
            .filter(|c| c.path.is_empty())
        {
            let Some(input) = names.iter().position(|n| *n == c.input) else {
                self.warn(format!(
                    "no input named {:?} in state machine {name:?}",
                    c.input
                ));
                continue;
            };
            let frame = (c.time.max(0.0) / DT - 1e-6).ceil() as usize;
            let value = match c.value {
                crate::InputValue::Bool(b) => Some(f64::from(u8::from(b))),
                crate::InputValue::Number(n) => Some(n),
                crate::InputValue::Fire => None,
            };
            changes.push((frame.min(MAX_FRAMES), input, value));
        }
        changes.sort_by_key(|c| c.0);
        let inputs_at = |k: usize| {
            let mut v = inputs.clone();
            for &(frame, input, value) in &changes {
                if frame > k {
                    break;
                }
                let Some(slot) = v.get_mut(input) else {
                    continue;
                };
                match value {
                    Some(value) => *slot = Some(value),
                    // Fired: on in its frame alone.
                    None if frame == k => *slot = Some(1.0),
                    None => {}
                }
            }
            v
        };
        let play = |frames: usize| -> (Vec<Vec<machine::Applied>>, Vec<machine::Fired>) {
            let mut fired = Vec::new();
            let applied = layers
                .iter()
                .map(|states| {
                    let played = machine::play(states, &anims, &inputs_at, frames, DT);
                    fired.extend(played.fired);
                    played.applied
                })
                .collect();
            (applied, fired)
        };
        // Where each layer settles: after its last change of state or mix,
        // half a minute past the last change of an input at most.
        // A nested artboard's machine changed plays on past it too.
        let last_change = self
            .changes
            .iter()
            .map(|c| ((c.time.max(0.0) / DT - 1e-6).ceil() as usize).min(MAX_FRAMES))
            .chain(
                self.aligned
                    .iter()
                    .map(|a| ((a.time.max(0.0) / DT - 1e-6).ceil() as usize).min(MAX_FRAMES)),
            )
            .max()
            .unwrap_or(0);
        let horizon = last_change + 30 * 60;
        let (mut played, mut fired) = play(horizon.min(MAX_FRAMES));
        let (mut settled, mut periods, mut steady) = settle(&played, &anims);
        // Nested artboards' machines a player changed play on: where they
        // settle, and what they repeat over.
        for (n_settled, n_periods, n_steady) in self.changed_nested_settles(horizon) {
            settled = settled.max(n_settled);
            periods.extend(n_periods);
            steady &= n_steady;
        }
        // Nested artboards play on: what they repeat over repeats too, and
        // what they play once is played out.
        match self.nested_span() {
            Some((d, LoopMode::Once)) => settled = settled.max((d / DT).ceil() as usize),
            Some((d, _)) if d > 0.0 => periods.push(d),
            _ => {}
        }
        // What repeats starts no earlier than the last change.
        settled = settled.max(last_change);
        let (span, loop_start) = if !steady {
            self.warn(
                "state machines: layers that keep changing states play their first 30 s, then from the start",
            );
            ((horizon - 1) as f64 * DT, Some(last_change as f64 * DT))
        } else if periods.is_empty() {
            (settled as f64 * DT, None)
        } else {
            let (period, exact) = common_period(&periods);
            if !exact {
                self.warn(format!(
                    "state machines: layers loop over lengths with no common one within a minute: the scene repeats every {period:.2} s"
                ));
            }
            (settled as f64 * DT + period, Some(settled as f64 * DT))
        };
        let frames = (span / DT).round() as usize + 1;
        if frames > played.first().map_or(0, Vec::len) {
            (played, fired) = play(frames);
        }
        for l in &mut played {
            l.truncate(frames);
        }
        // What it fires, as cues where they happened.
        let objects = &self.ab.objects;
        let mut cues: Vec<dxn_vector_animation::Cue> = fired
            .iter()
            .filter(|f| f.frame < frames)
            .filter_map(|f| {
                let time = (f.frame as f64 * DT - f64::from(f.delay).max(0.0)).max(0.0);
                crate::cues::cue(objects, f.event, time)
            })
            .collect();
        cues.sort_by(|a, b| a.time.total_cmp(&b.time));
        let most = self.limits.max_keyframes / 4;
        if cues.len() > most {
            self.warn(format!(
                "state machines: the events past the first {most} fired (the limit's)"
            ));
            cues.truncate(most);
        }
        self.cues = cues;
        Some(Plan {
            name,
            bounds: bounds(&played),
            layers: played,
            anims,
            fps,
            dt: DT,
            speed: 1.0,
            offset: 0.0,
            span,
            loop_start,
        })
    }

    /// State machine `index` of the artboard, ready to play: its layers'
    /// states, the animations they play and its inputs' values.
    fn machine(&mut self, index: usize) -> Option<Machine> {
        self.machine_in(self.ab, index)
    }

    /// State machine `index` of artboard `ab` (this one, or one it nests),
    /// ready to play.
    fn machine_in(&mut self, ab: &'a Artboard, index: usize) -> Option<Machine> {
        let sm = ab.state_machines.get(index)?;
        let specs: Vec<Spec> = ab.animations.iter().map(|a| Spec::of(&a.object)).collect();
        let anims: Vec<machine::Anim> = specs
            .iter()
            .zip(&ab.animations)
            .map(|(s, a)| machine::Anim {
                start: s.from,
                end: s.to,
                speed: s.speed,
                loop_mode: s.loop_mode,
                callbacks: a
                    .keyed
                    .iter()
                    .filter_map(|k| {
                        let event = k.object?;
                        let p = k
                            .properties
                            .iter()
                            .find(|p| p.key == crate::cues::EVENT_TRIGGER)?;
                        let mut times: Vec<f32> = p
                            .keyframes
                            .iter()
                            .map(|k| (k.num(p::KEY_FRAME_FRAME) / s.fps) as f32)
                            .collect();
                        times.sort_by(f32::total_cmp);
                        Some((event, times))
                    })
                    .collect(),
            })
            .collect();
        let inputs: Vec<Option<f64>> = sm
            .inputs
            .iter()
            .map(|i| {
                let o = i.as_ref()?;
                match o.ty {
                    t::STATE_MACHINE_BOOL => Some(o.num(p::STATE_MACHINE_BOOL_VALUE)),
                    t::STATE_MACHINE_NUMBER => Some(o.num(p::STATE_MACHINE_NUMBER_VALUE)),
                    t::STATE_MACHINE_TRIGGER => Some(0.0),
                    _ => None,
                }
            })
            .collect();
        let mut unread: BTreeSet<&'static str> = BTreeSet::new();
        let mut layers: Vec<Vec<machine::State>> = Vec::new();
        for layer in &sm.layers {
            let mut states = Vec::new();
            for st in &layer.states {
                let kind = match st.object.as_ref().map(|o| o.ty) {
                    Some(t::ENTRY_STATE) => machine::Kind::Entry,
                    Some(t::EXIT_STATE) => machine::Kind::Exit,
                    Some(t::ANY_STATE) => machine::Kind::Any,
                    Some(t::ANIMATION_STATE) => machine::Kind::Animation(
                        st.object
                            .as_ref()
                            .and_then(|o| o.id(p::ANIMATION_STATE_ANIMATION_ID))
                            .filter(|a| *a < anims.len()),
                    ),
                    Some(t::BLEND_STATE1_D_INPUT | t::BLEND_STATE_DIRECT) => machine::Kind::Blend,
                    Some(t::BLEND_STATE1_D_VIEW_MODEL) => machine::Kind::Blend,
                    Some(ty) => {
                        unread.extend(defs::type_name(ty));
                        machine::Kind::Other
                    }
                    None => machine::Kind::Other,
                };
                let mut transitions = Vec::new();
                for tr in &st.transitions {
                    let o = &tr.object;
                    let Some(to) = o.id(p::STATE_TRANSITION_STATE_TO_ID) else {
                        continue;
                    };
                    let mut conditions = Vec::new();
                    let mut parts = tr.conditions.iter();
                    while let Some(c) = parts.next() {
                        // A view model condition and its two comparators.
                        if c.ty == t::TRANSITION_VIEW_MODEL_CONDITION {
                            let (Some(left), Some(right)) = (parts.next(), parts.next()) else {
                                conditions.push(machine::Cond::Fixed(false));
                                break;
                            };
                            conditions.push(machine::Cond::Fixed(view_model_holds(
                                &sm.bindables,
                                c.uint(p::TRANSITION_VIEW_MODEL_CONDITION_OP_VALUE),
                                left,
                                right,
                                &mut unread,
                            )));
                            continue;
                        }
                        let input = c
                            .id(p::TRANSITION_INPUT_CONDITION_INPUT_ID)
                            .unwrap_or(usize::MAX);
                        let op = c.uint(p::TRANSITION_VALUE_CONDITION_OP_VALUE);
                        conditions.push(match c.ty {
                            t::TRANSITION_BOOL_CONDITION if op <= 1 => {
                                machine::Cond::Bool(input, op == 0)
                            }
                            t::TRANSITION_BOOL_CONDITION => machine::Cond::Fixed(false),
                            t::TRANSITION_NUMBER_CONDITION => machine::Cond::Number(
                                input,
                                op,
                                c.num(p::TRANSITION_NUMBER_CONDITION_VALUE),
                            ),
                            t::TRANSITION_TRIGGER_CONDITION => machine::Cond::Trigger(input),
                            ty => {
                                unread.extend(defs::type_name(ty));
                                machine::Cond::Fixed(false)
                            }
                        });
                    }
                    let ease = o
                        .id(p::STATE_TRANSITION_INTERPOLATOR_ID)
                        .and_then(|i| ab.objects.get(i).and_then(Option::as_ref))
                        .map(interpolator);
                    transitions.push(machine::Trans {
                        to,
                        flags: o.uint(p::STATE_TRANSITION_FLAGS),
                        duration: o.num(p::STATE_TRANSITION_DURATION),
                        exit_time: o.num(p::STATE_TRANSITION_EXIT_TIME),
                        conditions,
                        ease,
                        exit_part: (o.ty == t::BLEND_STATE_TRANSITION)
                            .then(|| o.id(p::BLEND_STATE_TRANSITION_EXIT_BLEND_ANIMATION_ID))
                            .map(|id| id.unwrap_or(usize::MAX)),
                        events: fires(&tr.events),
                    });
                }
                let speed = st
                    .object
                    .as_ref()
                    .filter(|o| o.is(t::ADVANCEABLE_STATE))
                    .map_or(1.0, |o| o.num(p::ADVANCEABLE_STATE_SPEED));
                let blend = (kind == machine::Kind::Blend).then(|| {
                    let o = st.object.as_ref();
                    let direct = o.is_some_and(|o| o.ty == t::BLEND_STATE_DIRECT);
                    let parts = st
                        .parts
                        .iter()
                        .map(|part| {
                            let anim = part
                                .id(p::BLEND_ANIMATION_ANIMATION_ID)
                                .filter(|a| *a < anims.len());
                            if !direct {
                                return machine::Part {
                                    anim,
                                    value: part.num(p::BLEND_ANIMATION1_D_VALUE),
                                    input: None,
                                };
                            }
                            let source = part.uint(p::BLEND_ANIMATION_DIRECT_BLEND_SOURCE);
                            if source == 2 {
                                unread.insert(
                                    "BlendAnimationDirect bound to a view model (mixing at 0)",
                                );
                            }
                            machine::Part {
                                anim,
                                value: if source == 1 {
                                    part.num(p::BLEND_ANIMATION_DIRECT_MIX_VALUE)
                                } else {
                                    0.0
                                },
                                input: (source == 0).then(|| {
                                    part.id(p::BLEND_ANIMATION_DIRECT_INPUT_ID)
                                        .unwrap_or(usize::MAX)
                                }),
                            }
                        })
                        .collect();
                    machine::Blend {
                        parts,
                        by: if direct {
                            machine::By::Direct
                        } else if let Some(o) = o.filter(|o| o.ty == t::BLEND_STATE1_D_VIEW_MODEL) {
                            // At the value bound to its bindable property.
                            machine::By::Value(
                                sm.bindables
                                    .get(o.uint(crate::file::BINDABLE_INDEX) as usize)
                                    .map_or(0.0, |b| {
                                        b.num(p::BINDABLE_PROPERTY_NUMBER_PROPERTY_VALUE)
                                    }),
                            )
                        } else {
                            machine::By::OneD(
                                o.and_then(|o| o.id(p::BLEND_STATE1_D_INPUT_INPUT_ID)),
                            )
                        },
                        reset: !direct && o.is_some_and(|o| o.uint(p::LAYER_STATE_FLAGS) & 2 != 0),
                    }
                });
                states.push(machine::State {
                    kind,
                    transitions,
                    speed,
                    blend,
                    events: fires(&st.events),
                });
            }
            layers.push(states);
        }
        for name in unread {
            self.warn(format!(
                "state machines: left out: {name} (playing nothing, or never holding)"
            ));
        }
        Some(Machine {
            name: sm.object.string(p::ANIMATION_NAME),
            names: sm
                .inputs
                .iter()
                .map(|i| {
                    i.as_ref()
                        .map(|o| o.string(p::STATE_MACHINE_COMPONENT_NAME))
                        .unwrap_or_default()
                })
                .collect(),
            layers,
            fps: specs.iter().map(|s| s.fps).collect(),
            anims,
            inputs,
        })
    }

    /// Where the machines of this artboard's nested artboards that a player
    /// changed settle, each played on its own with its inputs as they are
    /// and the changes made to it, `horizon` frames at most: as [`settle`]
    /// has it.
    fn changed_nested_settles(&mut self, horizon: usize) -> Vec<(usize, Vec<f64>, bool)> {
        let mut out = Vec::new();
        for (i, o) in self.ab.objects.iter().enumerate() {
            let Some(o) = o.as_ref().filter(|o| o.ty == t::NESTED_ARTBOARD) else {
                continue;
            };
            let Some(target) = o
                .id(p::NESTED_ARTBOARD_ARTBOARD_ID)
                .and_then(|a| self.file.artboards.get(a))
            else {
                continue;
            };
            let name = o.string(p::COMPONENT_NAME);
            let own = format!("#{i}");
            let changes: Vec<&crate::InputChange> = self
                .changes
                .iter()
                .filter(|c| {
                    let first = c.path.split('/').next().unwrap_or("");
                    !c.path.contains('/') && (first == own || (!name.is_empty() && first == name))
                })
                .collect();
            if changes.is_empty() {
                continue;
            }
            let Some(sm) = self.children[i].iter().find_map(|&c| {
                let co = self.object(c)?;
                (co.ty == t::NESTED_STATE_MACHINE)
                    .then(|| co.uint(p::NESTED_ANIMATION_ANIMATION_ID) as usize)
            }) else {
                continue;
            };
            let changes: Vec<(usize, String, crate::InputValue)> = changes
                .iter()
                .map(|c| {
                    (
                        ((c.time.max(0.0) / DT - 1e-6).ceil() as usize).min(MAX_FRAMES),
                        c.input.clone(),
                        c.value,
                    )
                })
                .collect();
            let Some(Machine {
                layers,
                anims,
                inputs,
                names,
                ..
            }) = self.machine_in(target, sm)
            else {
                continue;
            };
            let inputs_at = |k: usize| {
                let mut v = inputs.clone();
                for (frame, input, value) in &changes {
                    if *frame > k {
                        break;
                    }
                    let Some(slot) = names
                        .iter()
                        .position(|n| n == input)
                        .and_then(|j| v.get_mut(j))
                    else {
                        continue;
                    };
                    match value {
                        crate::InputValue::Bool(b) => *slot = Some(f64::from(u8::from(*b))),
                        crate::InputValue::Number(n) => *slot = Some(*n),
                        crate::InputValue::Fire if *frame == k => *slot = Some(1.0),
                        crate::InputValue::Fire => {}
                    }
                }
                v
            };
            let played: Vec<Vec<machine::Applied>> = layers
                .iter()
                .map(|states| {
                    machine::play(states, &anims, &inputs_at, horizon.min(MAX_FRAMES), DT).applied
                })
                .collect();
            out.push(settle(&played, &anims));
        }
        out
    }

    /// What nested state machine `m` plays in each of the scene's spans, its
    /// inputs at the parent's values there.
    fn nested_machine_plans(&mut self, m: &NestedMachine) -> Vec<Rc<Plan>> {
        let Some(Machine {
            name,
            layers,
            anims,
            fps,
            inputs,
            ..
        }) = self.machine(m.machine)
        else {
            return Vec::new();
        };
        let mut plans = Vec::new();
        let spans = if m.first_only {
            &self.spans[..self.spans.len().min(1)]
        } else {
            &self.spans[..]
        };
        // The changes made to it (a player's), each at the first frame of a
        // span at or after its time.
        let names: Vec<String> = self.ab.state_machines[m.machine]
            .inputs
            .iter()
            .map(|i| {
                i.as_ref()
                    .map(|o| o.string(p::STATE_MACHINE_COMPONENT_NAME))
                    .unwrap_or_default()
            })
            .collect();
        let changes: Vec<(f64, usize, Option<f64>)> = self
            .changes
            .iter()
            .filter(|c| c.path.is_empty())
            .filter_map(|c| {
                let input = names.iter().position(|n| *n == c.input)?;
                let value = match c.value {
                    crate::InputValue::Bool(b) => Some(f64::from(u8::from(b))),
                    crate::InputValue::Number(n) => Some(n),
                    crate::InputValue::Fire => None,
                };
                Some((c.time, input, value))
            })
            .collect();
        for (s, &(start, duration)) in spans.iter().enumerate() {
            let frames = ((duration / DT).ceil() as usize + 1).min(self.limits.max_keyframes);
            let source = m.sources.get(s).copied().unwrap_or(Some(start));
            let frame_of = |t: f64| ((t - start).max(0.0) / DT - 1e-6).ceil() as usize;
            let inputs_at = |k: usize| {
                let mut v = inputs.clone();
                if let Some(source) = source {
                    let t = source + k as f64 * DT;
                    for (i, curve) in &m.inputs {
                        if let Some(slot) = v.get_mut(*i) {
                            *slot = Some(curve.at(t));
                        }
                    }
                }
                for &(t, input, value) in &changes {
                    if t < start - 1e-9 || t > start + duration {
                        continue;
                    }
                    let frame = frame_of(t);
                    if frame > k {
                        break;
                    }
                    if let Some(slot) = v.get_mut(input) {
                        match value {
                            Some(value) => *slot = Some(value),
                            None if frame == k => *slot = Some(1.0),
                            None => {}
                        }
                    }
                }
                v
            };
            let played: Vec<Vec<machine::Applied>> = layers
                .iter()
                .map(|states| {
                    machine::play(states, &anims, &inputs_at, frames, DT * m.speed).applied
                })
                .collect();
            plans.push(Rc::new(Plan {
                name: name.clone(),
                bounds: bounds(&played),
                layers: played,
                anims: anims.clone(),
                fps: fps.clone(),
                dt: DT,
                speed: m.speed,
                offset: start,
                span: duration,
                loop_start: None,
            }));
        }
        plans
    }

    /// The keys of a property keyed by `list` over the state machine's span:
    /// in each frame, what the last layer applying it applies (its state's
    /// animation, or while it mixes, its two states mixed from the value
    /// the property had when the mix began); what it last had where none
    /// does.
    fn machine_keys<T: Keyed>(
        &self,
        plans: &[Rc<Plan>],
        list: &[(usize, &KeyedProperty)],
        setup: &T,
        value: &impl Fn(&Object) -> Option<T>,
        keys: &mut Vec<Key<T>>,
    ) {
        let Some(first) = plans.first() else {
            return;
        };
        let mut curves: HashMap<usize, Curve<T>> = HashMap::new();
        for (a, kp) in list {
            let fps = first.fps.get(*a).copied().unwrap_or(60.0);
            let mut own: Vec<Key<T>> = kp
                .keyframes
                .iter()
                .filter_map(|k| {
                    Some(Key {
                        t: k.num(p::KEY_FRAME_FRAME) / fps,
                        value: value(k)?,
                        interp: self.interp(k),
                    })
                })
                .collect();
            own.sort_by(|a, b| a.t.total_cmp(&b.t));
            if !own.is_empty() {
                curves.insert(
                    *a,
                    Curve {
                        setup: setup.clone(),
                        keys: own,
                    },
                );
            }
        }
        if curves.is_empty() {
            return;
        }
        for plan in plans {
            self.plan_keys(plan, &curves, setup, keys);
        }
    }

    /// The keys of a property whose animations' keys are `curves` over what
    /// `plan` plays: interval by interval between the frames where a layer
    /// changes state or mixes.
    fn plan_keys<T: Keyed>(
        &self,
        plan: &Plan,
        curves: &HashMap<usize, Curve<T>>,
        setup: &T,
        keys: &mut Vec<Key<T>>,
    ) {
        let keyed = |x: Option<(usize, f64)>| x.filter(|(a, _)| curves.contains_key(a));
        let blends = |b: &Option<Rc<machine::Blended>>| {
            b.as_ref().is_some_and(|b| {
                b.parts
                    .iter()
                    .flatten()
                    .any(|b| curves.contains_key(&b.anim))
            })
        };
        let applies = |ap: &machine::Applied| {
            keyed(ap.current).is_some()
                || keyed(ap.from).is_some()
                || blends(&ap.blend)
                || blends(&ap.from_blend)
        };
        let frames = plan.layers.first().map_or(0, Vec::len);
        let effective = |k: usize| {
            plan.layers
                .iter()
                .rposition(|l| l.get(k).is_some_and(applies))
        };
        let dt = plan.dt;
        let at = |k: usize| plan.offset + k as f64 * dt;
        let ease = |e: Option<Interp>, m: f64| match e {
            Some(interp) => Curve {
                setup: 0.0,
                keys: vec![
                    Key {
                        t: 0.0,
                        value: 0.0,
                        interp,
                    },
                    Key {
                        t: 1.0,
                        value: 1.0,
                        interp: Interp::Hold,
                    },
                ],
            }
            .at(m),
            None => m,
        };
        let hold_last = |keys: &mut Vec<Key<T>>| {
            if let Some(k) = keys.last_mut() {
                k.interp = Interp::Hold;
            }
        };
        let mut resets: HashMap<(usize, usize), T> = HashMap::new();
        for w in plan.bounds.windows(2) {
            let (k, e) = (w[0], w[1].min(frames));
            if k >= frames {
                break;
            }
            let Some(l) = effective(k) else {
                hold_last(keys);
                continue;
            };
            let ap = &plan.layers[l][k];
            let mixing = ap.from.is_some() || ap.from_blend.is_some();
            let until = if e >= frames {
                (plan.offset + plan.span).max(at(k))
            } else {
                at(e)
            };
            // What the state plays until a layer changes: the animation's
            // own keys; a blend's that alone weighs 1 where the property
            // is keyed.
            let single = if mixing {
                None
            } else if let Some(b) = &ap.blend {
                let on: Vec<&machine::BlendedPart> = b
                    .parts
                    .iter()
                    .flatten()
                    .filter(|b| b.weight != 0.0 && curves.contains_key(&b.anim))
                    .collect();
                match on[..] {
                    [only] if only.weight == 1.0 => Some((only.anim, only.time, only.direction)),
                    _ => None,
                }
            } else {
                keyed(ap.current).map(|(a, t)| (a, t, ap.direction))
            };
            if let Some((a, t0, direction)) = single {
                let anim = &plan.anims[a];
                // To the last frame it applies: in the frame a layer
                // changes, what the next state leaves alone keeps the
                // frame before's value.
                let last = if e < frames {
                    at(e - 1).max(at(k))
                } else {
                    until
                };
                let pieces = steady_pieces(
                    anim,
                    a,
                    plan.fps.get(a).copied().unwrap_or(60.0),
                    t0,
                    anim.speed * direction * plan.speed,
                    at(k),
                    last,
                );
                if pieces.is_empty() {
                    keys.push(Key {
                        t: at(k),
                        value: curves[&a].at(t0),
                        interp: Interp::Hold,
                    });
                }
                for seg in pieces {
                    piece_keys(&seg, &curves[&a], keys);
                    hold_last(keys);
                }
                continue;
            }
            let reset = resets
                .entry((l, ap.began))
                .or_insert_with(|| {
                    let t = at(ap.began) - 1e-9;
                    if keys.first().is_none_or(|f| t < f.t) {
                        setup.clone()
                    } else {
                        let curve = Curve {
                            setup: setup.clone(),
                            keys: std::mem::take(keys),
                        };
                        let v = curve.at(t);
                        *keys = curve.keys;
                        v
                    }
                })
                .clone();
            // Each blended animation in turn, its weight times the state's
            // mix; a 1D blend resetting first (to its first animation's
            // first keyframes, else the values it was entered with).
            let blend = |v: &mut T, b: &machine::Blended, m: f64| {
                if b.reset {
                    let first = b.parts.first().copied().flatten();
                    *v = match first.and_then(|b| curves.get(&b.anim)) {
                        Some(c) => c
                            .keys
                            .first()
                            .map_or_else(|| reset.clone(), |k| k.value.clone()),
                        None => reset.clone(),
                    };
                }
                for b in b.parts.iter().flatten() {
                    if let Some(c) = curves.get(&b.anim)
                        && m * b.weight != 0.0
                    {
                        *v = v.mix(&c.at(b.time), m * b.weight);
                    }
                }
            };
            // A blend weighing nothing the property is keyed by: what it
            // resets to, else what it had, held.
            if let (false, Some(b)) = (mixing, &ap.blend)
                && !b
                    .parts
                    .iter()
                    .flatten()
                    .any(|b| b.weight != 0.0 && curves.contains_key(&b.anim))
            {
                let mut v = keys
                    .last()
                    .map_or_else(|| setup.clone(), |k| k.value.clone());
                blend(&mut v, b, 1.0);
                keys.push(Key {
                    t: at(k),
                    value: v,
                    interp: Interp::Hold,
                });
                continue;
            }
            for j in k..e {
                let ap = &plan.layers[l][j];
                // A mix starts from the value when it began; a blend alone
                // from the value it had a frame before.
                let mut v = if mixing {
                    reset.clone()
                } else {
                    keys.last()
                        .map_or_else(|| setup.clone(), |k| k.value.clone())
                };
                if let Some(b) = &ap.from_blend {
                    blend(&mut v, b, ease(ap.ease, ap.from_mix));
                } else if let Some((a, t)) = keyed(ap.from) {
                    v = v.mix(&curves[&a].at(t), ease(ap.ease, ap.from_mix));
                }
                if let Some(b) = &ap.blend {
                    blend(&mut v, b, ease(ap.ease, ap.mix));
                } else if let Some((a, t)) = keyed(ap.current) {
                    v = v.mix(&curves[&a].at(t), ease(ap.ease, ap.mix));
                }
                keys.push(Key {
                    t: at(j),
                    value: v,
                    interp: Interp::Linear,
                });
            }
        }
        hold_last(keys);
    }

    /// Property `key` of object `i` over the timeline: what the last layer
    /// keying it plays, in each of its segments; the setup value where
    /// that layer plays another animation, and where nothing keys it.
    fn curve<T: Keyed>(
        &mut self,
        i: usize,
        key: u16,
        setup: T,
        value: impl Fn(&Object) -> Option<T>,
    ) -> Curve<T> {
        let Some(list) = self.keyed.get(&(i, key)).cloned() else {
            return Curve::fixed(setup);
        };
        self.used.insert((i, key));
        let keys_in = |seg: &Segment| list.iter().find(|(a, _)| *a == seg.animation);
        let Some(layer) = self
            .layers
            .iter()
            .rposition(|l| l.iter().any(|seg| keys_in(seg).is_some()))
        else {
            return Curve::fixed(setup);
        };
        let segments = self.layers[layer].clone();
        let mut keys: Vec<Key<T>> = Vec::new();
        // A nested state machine: what it plays in each span.
        if self.machine_layer == Some(layer) {
            let plans = self.nested_plans.clone();
            self.machine_keys(&plans, &list, &setup, &value, &mut keys);
            if keys.first().is_none_or(|k| k.t > 0.0) {
                keys.insert(
                    0,
                    Key {
                        t: 0.0,
                        value: setup.clone(),
                        interp: Interp::Hold,
                    },
                );
            }
            return Curve { setup, keys };
        }
        // The state machine first, where the timeline's animations play.
        if layer == 0
            && let Some(plan) = self.plan.clone()
        {
            self.machine_keys(&[plan], &list, &setup, &value, &mut keys);
            // Its setup value until the machine first applies it.
            if keys.first().is_none_or(|k| k.t > 0.0) {
                keys.insert(
                    0,
                    Key {
                        t: 0.0,
                        value: setup.clone(),
                        interp: Interp::Hold,
                    },
                );
            }
        }
        // The setup value from `t` on, unless it holds already.
        let hold = |keys: &mut Vec<Key<T>>, t: f64| {
            if keys.last().is_none_or(|k| k.value != setup) {
                keys.push(Key {
                    t,
                    value: setup.clone(),
                    interp: Interp::Hold,
                });
            }
        };
        for seg in &segments {
            let Some((_, kp)) = keys_in(seg) else {
                hold(&mut keys, seg.start);
                continue;
            };
            let mut own: Vec<Key<T>> = kp
                .keyframes
                .iter()
                .filter_map(|k| {
                    Some(Key {
                        t: k.num(p::KEY_FRAME_FRAME) / seg.fps,
                        value: value(k)?,
                        interp: self.interp(k),
                    })
                })
                .collect();
            own.sort_by(|a, b| a.t.total_cmp(&b.t));
            if own.is_empty() {
                hold(&mut keys, seg.start);
                continue;
            }
            let at = keys.len();
            piece_keys(
                seg,
                &Curve {
                    setup: setup.clone(),
                    keys: own,
                },
                &mut keys,
            );
            // What follows the span is held to the next one.
            if let Some(k) = keys.get_mut(at..).and_then(<[Key<T>]>::last_mut) {
                k.interp = Interp::Hold;
            }
        }
        Curve { setup, keys }
    }

    /// A number, bool, id or uint property.
    fn num_curve(&mut self, i: usize, key: u16) -> Curve<f64> {
        let setup = self
            .object(i)
            .map_or_else(|| defs::initial(key).unwrap_or(0.0), |o| o.num(key));
        self.curve(i, key, setup, |k| match k.ty {
            t::KEY_FRAME_DOUBLE => Some(k.num(p::KEY_FRAME_DOUBLE_VALUE)),
            t::KEY_FRAME_BOOL => Some(k.num(p::KEY_FRAME_BOOL_VALUE)),
            t::KEY_FRAME_ID => Some(k.num(p::KEY_FRAME_ID_VALUE)),
            t::KEY_FRAME_UINT => Some(k.num(p::KEY_FRAME_UINT_VALUE)),
            _ => None,
        })
    }

    /// A color property.
    fn color_curve(&mut self, i: usize, key: u16) -> Curve<Color> {
        let setup = argb(self.object(i).map_or(0, |o| o.color(key)));
        self.curve(i, key, setup, |k| {
            (k.ty == t::KEY_FRAME_COLOR).then(|| argb(k.color(p::KEY_FRAME_COLOR_VALUE)))
        })
    }

    /// A number property as the scene animates it, times `scale`.
    fn num(&mut self, i: usize, key: u16, scale: f64) -> Animated<f64> {
        let curve = self.num_curve(i, key);
        scaled(self.baker.curve(&curve), scale)
    }

    // ---- The tree ------------------------------------------------------

    /// The objects from the artboard's child down to `i`.
    fn chain(&self, i: usize) -> Result<Vec<usize>, Error> {
        let mut chain = vec![i];
        let mut at = i;
        while let Some(pi) = self.parent[at] {
            if pi == 0 {
                break;
            }
            if chain.len() >= self.limits.max_depth {
                return Err(Error::Limit {
                    what: "levels of nesting",
                    limit: self.limits.max_depth as f64,
                });
            }
            chain.push(pi);
            at = pi;
        }
        chain.reverse();
        Ok(chain)
    }

    fn is_ancestor_or_self(&self, ancestor: usize, mut i: usize) -> bool {
        for _ in 0..=self.limits.max_depth {
            if i == ancestor {
                return true;
            }
            match self.parent[i] {
                Some(pi) => i = pi,
                None => return false,
            }
        }
        false
    }

    /// Object `i`'s own transform, as Rive's runtime reads it: a bone at
    /// the tip of its parent bone, a root bone and a node where they say.
    fn local(&mut self, i: usize) -> Option<rig::Local> {
        let o = self.object(i)?;
        if !o.is(t::TRANSFORM_COMPONENT) {
            return None;
        }
        let (x, y) = if o.is(t::ROOT_BONE) {
            (
                self.num_curve(i, p::ROOT_BONE_X),
                self.num_curve(i, p::ROOT_BONE_Y),
            )
        } else if o.is(t::BONE) {
            let x = match self.parent[i] {
                Some(pi) if self.object(pi).is_some_and(|o| o.is(t::BONE)) => {
                    self.num_curve(pi, p::BONE_LENGTH)
                }
                _ => Curve::fixed(0.0),
            };
            (x, Curve::fixed(0.0))
        } else if o.is(t::NODE) {
            let (x, y) = (self.num_curve(i, p::NODE_X), self.num_curve(i, p::NODE_Y));
            self.aligned_to(i, x, y)
        } else {
            (Curve::fixed(0.0), Curve::fixed(0.0))
        };
        Some(rig::Local {
            x,
            y,
            rotation: self.num_curve(i, p::TRANSFORM_COMPONENT_ROTATION),
            scale_x: self.num_curve(i, p::TRANSFORM_COMPONENT_SCALE_X),
            scale_y: self.num_curve(i, p::TRANSFORM_COMPONENT_SCALE_Y),
            length: o.is(t::BONE).then(|| self.num_curve(i, p::BONE_LENGTH)),
        })
    }

    /// Node `i`'s position `x`, `y` as listeners move it to the pointer
    /// (`ListenerAlignTarget::perform`): each move held until the next key.
    fn aligned_to(
        &mut self,
        i: usize,
        mut x: Curve<f64>,
        mut y: Curve<f64>,
    ) -> (Curve<f64>, Curve<f64>) {
        let moves: Vec<crate::Aligned> = self
            .aligned
            .iter()
            .filter(|a| a.node == i && a.path.is_empty())
            .cloned()
            .collect();
        if moves.is_empty() {
            return (x, y);
        }
        let Ok(chain) = self.chain(i) else {
            return (x, y);
        };
        let parent = self.ops(&chain[..chain.len() - 1]);
        // The scene's coordinates are the artboard's moved by its origin.
        let ab = self.ab.object();
        let origin = Point::new(
            ab.num(p::LAYOUT_COMPONENT_WIDTH) * ab.num(p::ARTBOARD_ORIGIN_X),
            ab.num(p::LAYOUT_COMPONENT_HEIGHT) * ab.num(p::ARTBOARD_ORIGIN_Y),
        );
        if !(x.keys.is_empty() && y.keys.is_empty()) {
            self.warn(
                "nodes listeners move that animations move too: each move holds to the next key",
            );
        }
        for m in moves {
            let Some(inverse) = transform_at(&parent, m.time).invert() else {
                continue;
            };
            let local = |p: Point| inverse.apply(Point::new(p.x - origin.x, p.y - origin.y));
            let to = local(m.to);
            let (vx, vy) = match m.from {
                Some(from) => {
                    let from = local(from);
                    (x.at(m.time) + to.x - from.x, y.at(m.time) + to.y - from.y)
                }
                None => (to.x, to.y),
            };
            hold_from(&mut x, m.time, vx);
            hold_from(&mut y, m.time, vy);
        }
        (x, y)
    }

    /// The transform operations of object `i` itself.
    fn own_ops(&mut self, i: usize) -> Vec<TransformOp> {
        if self.rig.moves(i) {
            return self.constrained_ops(i);
        }
        let Some(o) = self.object(i) else {
            return Vec::new();
        };
        if !o.is(t::NODE) && !o.is(t::BONE) {
            return Vec::new();
        }
        let Some(local) = self.local(i) else {
            return Vec::new();
        };
        let x = self.baker.curve(&local.x);
        let y = self.baker.curve(&local.y);
        let rotate = scaled(self.baker.curve(&local.rotation), 180.0 / PI);
        let sx = self.baker.curve(&local.scale_x);
        let sy = self.baker.curve(&local.scale_y);
        let mut ops = Vec::new();
        if !(is_const(&x, 0.0) && is_const(&y, 0.0)) {
            ops.push(TransformOp::Translate { x, y });
        }
        if !is_const(&rotate, 0.0) {
            ops.push(TransformOp::Rotate { degrees: rotate });
        }
        if !(is_const(&sx, 1.0) && is_const(&sy, 1.0)) {
            ops.push(TransformOp::Scale { x: sx, y: sy });
        }
        ops
    }

    /// The transform of component `i`, which constraints move: its world
    /// as Rive's runtime updates it, in its parent's.
    fn constrained_ops(&mut self, i: usize) -> Vec<TransformOp> {
        if let Some(ops) = self.constrained.get(&i) {
            return ops.clone();
        }
        let closure = self.rig.closure(i);
        let breaks = self.rig.breaks(&closure);
        let rig = &self.rig;
        let parent = rig.parent.get(i).copied().flatten();
        let matrix = self.baker.derive(&Breaks::all(&breaks), |t| {
            let worlds = rig.worlds(&closure, t);
            let parent = parent
                .and_then(|p| worlds.get(&p).copied())
                .unwrap_or(Matrix::IDENTITY);
            parent.invert().unwrap_or(Matrix::IDENTITY)
                * worlds.get(&i).copied().unwrap_or(Matrix::IDENTITY)
        });
        let ops = vec![TransformOp::Matrix { matrix }];
        self.constrained.insert(i, ops.clone());
        ops
    }

    /// Gathers what Rive's runtime updates to apply the artboard's
    /// constraints.
    fn build_rig(&mut self) {
        let n = self.ab.objects.len();
        let constraints: Vec<usize> = (1..n)
            .filter(|&i| self.object(i).is_some_and(|o| o.is(t::CONSTRAINT)))
            .collect();
        if constraints.is_empty() {
            return;
        }
        let mut rig = Rig::new(n);
        for i in 1..n {
            let Some(local) = self.local(i) else { continue };
            rig.parent[i] = self.parent[i].filter(|&pi| {
                pi != 0
                    && self
                        .object(pi)
                        .is_some_and(|o| o.is(t::WORLD_TRANSFORM_COMPONENT))
            });
            rig.locals.insert(i, local);
        }
        for ci in constraints {
            let Some(o) = self.object(ci) else { continue };
            let Some(owner) = self.parent[ci].filter(|pi| rig.locals.contains_key(pi)) else {
                continue;
            };
            let target = o
                .id(p::TARGETED_CONSTRAINT_TARGET_ID)
                .filter(|ti| rig.locals.contains_key(ti));
            let mut breaks = Vec::new();
            let kind = match o.ty {
                t::TRANSLATION_CONSTRAINT => rig::Kind::Translation,
                t::ROTATION_CONSTRAINT => rig::Kind::Rotation,
                t::SCALE_CONSTRAINT => rig::Kind::Scale,
                t::TRANSFORM_CONSTRAINT => rig::Kind::Transform,
                t::DISTANCE_CONSTRAINT => rig::Kind::Distance,
                t::IK_CONSTRAINT => {
                    // Rive's IK runs on a bone: it and as many bones above
                    // it as the constraint says.
                    if !self.object(owner).is_some_and(|o| o.is(t::BONE)) {
                        continue;
                    }
                    let mut chain = vec![owner];
                    let mut count = o.uint(p::IK_CONSTRAINT_PARENT_BONE_COUNT);
                    let mut bone = owner;
                    while count > 0
                        && let Some(pi) = self.parent[bone]
                        && self.object(pi).is_some_and(|o| o.is(t::BONE))
                        && chain.len() <= self.limits.max_depth
                    {
                        count -= 1;
                        bone = pi;
                        chain.push(bone);
                    }
                    chain.reverse();
                    rig::Kind::Ik { chain }
                }
                t::FOLLOW_PATH_CONSTRAINT => {
                    let mut paths = Vec::new();
                    if let Some(ti) = target {
                        let target_is_shape = self.object(ti).is_some_and(|o| o.is(t::SHAPE));
                        let candidates: Vec<usize> = if target_is_shape {
                            (ti + 1..n)
                                .filter(|&pi| {
                                    self.object(pi).is_some_and(|o| o.is(t::PATH))
                                        && self.nearest_shape(pi) == Some(ti)
                                })
                                .collect()
                        } else if self.object(ti).is_some_and(|o| o.is(t::PATH)) {
                            vec![ti]
                        } else {
                            Vec::new()
                        };
                        for pi in candidates {
                            let geometry = self.geometry(pi, true);
                            geometry_breaks(&geometry, &mut breaks);
                            paths.push((pi, geometry));
                        }
                    }
                    rig::Kind::FollowPath { paths }
                }
                _ => continue,
            };
            let mut values = HashMap::new();
            for key in CONSTRAINT_KEYS {
                if o.has(*key) || self.keyed.contains_key(&(ci, *key)) {
                    values.insert(*key, self.num_curve(ci, *key));
                }
            }
            rig.constraints
                .entry(owner)
                .or_default()
                .push(rig::Constraint {
                    kind,
                    target,
                    values,
                    breaks,
                });
        }
        rig.sort(&self.children);
        self.rig = rig;
    }

    /// The shape path `i` draws in: its nearest ancestor shape.
    fn nearest_shape(&self, mut i: usize) -> Option<usize> {
        for _ in 0..=self.limits.max_depth {
            i = self.parent[i]?;
            if self.object(i).is_some_and(|o| o.is(t::SHAPE)) {
                return Some(i);
            }
        }
        None
    }

    /// The transform operations of objects `chain`, outermost first.
    fn ops(&mut self, chain: &[usize]) -> Vec<TransformOp> {
        chain.iter().flat_map(|i| self.own_ops(*i)).collect()
    }

    /// The opacity factors of `chain` and the artboard, whether the
    /// drawable at its end is shown, and whether the solos over it show it.
    fn opacity_curves(&mut self, chain: &[usize]) -> Vec<Curve<f64>> {
        let mut curves = Vec::new();
        for &i in std::iter::once(&0).chain(chain) {
            let Some(o) = self.object(i) else { continue };
            if o.is(t::WORLD_TRANSFORM_COMPONENT) {
                curves.push(self.num_curve(i, p::WORLD_TRANSFORM_COMPONENT_OPACITY));
            }
            // A solo shows the child it names and collapses the others.
            if let Some(solo) =
                self.parent[i].filter(|&pi| self.object(pi).is_some_and(|o| o.ty == t::SOLO))
            {
                let active = self.num_curve(solo, p::SOLO_ACTIVE_COMPONENT_ID);
                curves.push(map_curve(active, |a| if a == i as f64 { 1.0 } else { 0.0 }));
            }
        }
        if let Some(&last) = chain.last()
            && self.object(last).is_some_and(|o| o.is(t::DRAWABLE))
        {
            let flags = self.num_curve(last, p::DRAWABLE_DRAWABLE_FLAGS);
            curves.push(map_curve(flags, |f| {
                if (f as u64) & 1 == 1 { 0.0 } else { 1.0 }
            }));
        }
        curves
    }

    /// The product of `curves` as the scene animates it.
    fn product(&mut self, curves: &[Curve<f64>]) -> Animated<f64> {
        let breaks: Vec<Breaks> = curves.iter().map(Curve::breaks).collect();
        self.baker.derive(&Breaks::all(&breaks), |t| {
            curves
                .iter()
                .map(|c| c.at(t))
                .product::<f64>()
                .clamp(0.0, 1.0)
        })
    }

    // ---- The scene -----------------------------------------------------

    fn scene(&mut self) -> Result<Scene, Error> {
        let ab = self.ab.object();
        let w = ab.num(p::LAYOUT_COMPONENT_WIDTH);
        let h = ab.num(p::LAYOUT_COMPONENT_HEIGHT);
        let (ox, oy) = (ab.num(p::ARTBOARD_ORIGIN_X), ab.num(p::ARTBOARD_ORIGIN_Y));
        // Artboards clip unless they say not to.
        let clip = !ab.has(p::LAYOUT_COMPONENT_CLIP) || ab.flag(p::LAYOUT_COMPONENT_CLIP);

        let mut content = Vec::new();
        // The artboard's own fills and strokes, under everything.
        let background = Geometry::path(geometry::build(
            &geometry::rectangle(w, h, ox, oy, [0.0; 4]),
            true,
        ));
        let bg_opacity = self.opacity_curves(&[]);
        let background = [(ShapePath::new(background), Winding::default())];
        let painted = self.paints(0, &background, &bg_opacity, &[])?;
        if !painted.is_empty() {
            content.push(self.painted_group(0, Vec::new(), painted));
        }

        // Drawables: the first in the file on top.
        let drawables: Vec<usize> = (1..self.ab.objects.len())
            .filter(|&i| self.object(i).is_some_and(|o| o.is(t::DRAWABLE)))
            .collect();
        let mut unsupported: BTreeMap<&str, usize> = BTreeMap::new();
        let mut run: Vec<(Vec<usize>, Vec<Node>)> = Vec::new();
        for (i, shown) in self.draw_order(&drawables) {
            let Some(o) = self.object(i) else { continue };
            let node = if o.is(t::SHAPE) {
                self.shape(i)?
            } else if o.is(t::NESTED_ARTBOARD) {
                self.nested(i)?
            } else if o.ty == t::TEXT {
                self.text(i)?
            } else if o.ty == t::IMAGE {
                self.image(i)?
            } else {
                if o.ty != t::LAYOUT_COMPONENT {
                    *unsupported
                        .entry(defs::type_name(o.ty).unwrap_or("?"))
                        .or_default() += 1;
                }
                None
            };
            if let Some(node) = node {
                let node = match shown {
                    // Drawn here only while the draw rules put it here.
                    Some(shown) => Node::Group(Group {
                        opacity: self.baker.curve(&shown),
                        children: vec![node],
                        ..Group::new()
                    }),
                    None => node,
                };
                // Drawables one after another under the same clipping
                // shapes are clipped together, as Rive clips them.
                let clips = self.clips_of(i);
                match run.last_mut() {
                    Some((last, nodes)) if *last == clips => nodes.push(node),
                    _ => run.push((clips, vec![node])),
                }
            }
        }
        for (clips, nodes) in run {
            content.extend(self.clipped(&clips, nodes)?);
        }
        for (name, n) in unsupported {
            self.warn(format!("left out: {n} {name} (drawn in a phase to come)"));
        }

        let root = Group {
            name: Some(self.ab.name()).filter(|n| !n.is_empty()),
            clip: clip.then(|| Clip {
                shapes: vec![ClipShape {
                    geometry: Geometry::Rect {
                        center: AnimatedPoint::fixed(w / 2.0, h / 2.0),
                        size: AnimatedPoint::fixed(w, h),
                        radius: AnimatedPoint::fixed(0.0, 0.0),
                        direction: Direction::Clockwise,
                        start: RectStart::TopLeft,
                    },
                    transform: Vec::new(),
                    rule: FillRule::NonZero,
                }],
                one_path: false,
            }),
            children: vec![Node::Group(Group {
                transform: vec![TransformOp::translate(w * ox, h * oy)],
                children: content,
                ..Group::new()
            })],
            ..Group::new()
        };
        Ok(Scene {
            width: w,
            height: h,
            timeline: self.timeline(),
            root,
        })
    }

    /// `drawables` (in the file's order) in the order Rive draws them, the
    /// bottom first (the first in the file on top, as draw rules move
    /// them); a drawable the rules move over time is in each place it
    /// takes, with when it is drawn there.
    fn draw_order(&mut self, drawables: &[usize]) -> Vec<(usize, Option<Curve<f64>>)> {
        let n = self.ab.objects.len();
        // The rules of each component (the last when it holds several).
        let mut held: BTreeMap<usize, usize> = BTreeMap::new();
        for r in 1..n {
            if self.object(r).is_some_and(|o| o.ty == t::DRAW_RULES)
                && let Some(pi) = self.parent[r]
            {
                held.insert(pi, r);
            }
        }
        if held.is_empty() {
            return drawables.iter().rev().map(|&d| (d, None)).collect();
        }
        let mut of = HashMap::new();
        for &d in drawables {
            let mut c = Some(d);
            for _ in 0..=self.limits.max_depth {
                let Some(ci) = c else { break };
                if let Some(&r) = held.get(&ci) {
                    of.insert(d, r);
                    break;
                }
                c = self.parent[ci];
            }
        }
        let is_drawable: HashSet<usize> = drawables.iter().copied().collect();
        let rules: Vec<(usize, Vec<(usize, usize)>)> = held
            .values()
            .map(|&r| {
                let targets = self.children[r]
                    .iter()
                    .filter_map(|&c| {
                        let o = self.object(c).filter(|o| o.ty == t::DRAW_TARGET)?;
                        let d = o
                            .id(p::DRAW_TARGET_DRAWABLE_ID)
                            .filter(|d| is_drawable.contains(d))?;
                        Some((c, d))
                    })
                    .collect();
                (r, targets)
            })
            .collect();
        let sorted = order::Rules::new(of, &rules);
        // What may change the order: the rules' active targets and the
        // targets' placements.
        let active: HashMap<usize, Curve<f64>> = held
            .values()
            .map(|&r| (r, self.num_curve(r, p::DRAW_RULES_DRAW_TARGET_ID)))
            .collect();
        let placement: HashMap<usize, Curve<f64>> = sorted
            .targets
            .iter()
            .map(|&(t, _)| (t, self.num_curve(t, p::DRAW_TARGET_PLACEMENT_VALUE)))
            .collect();
        let mut times = vec![0.0];
        for c in active.values().chain(placement.values()) {
            times.extend(c.keys.iter().map(|k| k.t));
        }
        times.sort_by(f64::total_cmp);
        times.dedup();
        let mut orders: Vec<Vec<usize>> = Vec::new();
        let mut which = Vec::with_capacity(times.len());
        for &t in &times {
            let order = sorted.order(
                drawables,
                |r| {
                    let id = active.get(&r)?.at(t);
                    (id >= 0.0 && id < n as f64).then_some(id as usize)
                },
                |target| placement.get(&target).is_some_and(|c| c.at(t) == 1.0),
            );
            let k = match orders.iter().position(|o| *o == order) {
                Some(k) => k,
                None => {
                    orders.push(order);
                    orders.len() - 1
                }
            };
            which.push(k);
        }
        if orders.len() == 1 {
            return orders[0].iter().map(|&d| (d, None)).collect();
        }
        order::merge(&orders)
            .into_iter()
            .map(|(d, shown)| {
                let value = |k: usize| if shown.contains(&k) { 1.0 } else { 0.0 };
                let curve = Curve {
                    setup: value(which[0]),
                    keys: times
                        .iter()
                        .zip(&which)
                        .map(|(&t, &k)| Key {
                            t,
                            value: value(k),
                            interp: Interp::Hold,
                        })
                        .collect(),
                };
                (d, (shown.len() < orders.len()).then_some(curve))
            })
            .collect()
    }

    /// The clipping shapes over drawable `i` that clip, in the file's order.
    fn clips_of(&mut self, i: usize) -> Vec<usize> {
        let clips: Vec<usize> = (1..self.ab.objects.len())
            .filter(|&c| {
                self.object(c).is_some_and(|o| o.ty == t::CLIPPING_SHAPE)
                    && self.parent[c].is_some_and(|pi| self.is_ancestor_or_self(pi, i))
            })
            .collect();
        clips
            .into_iter()
            .filter(|&c| {
                let visible = self.num_curve(c, p::CLIPPING_SHAPE_IS_VISIBLE);
                if visible.is_animated() {
                    self.warn("clipping shapes shown and hidden over time: they always clip");
                    return true;
                }
                visible.at(0.0) != 0.0
            })
            .collect()
    }

    /// `nodes` inside the clips of clipping shapes `clips`.
    fn clipped(&mut self, clips: &[usize], mut nodes: Vec<Node>) -> Result<Vec<Node>, Error> {
        for &c in clips {
            let Some(clip) = self.clip(c)? else { continue };
            nodes = vec![Node::Group(Group {
                clip: Some(clip),
                children: nodes,
                ..Group::new()
            })];
        }
        Ok(nodes)
    }

    /// The clip of clipping shape `c`: one path of all its source's
    /// outlines under its fill rule.
    fn clip(&mut self, c: usize) -> Result<Option<Clip>, Error> {
        if let Some(clip) = self.clips.get(&c) {
            return Ok(clip.clone());
        }
        let Some(o) = self.object(c) else {
            return Ok(None);
        };
        let rule = match o.uint(p::CLIPPING_SHAPE_FILL_RULE) {
            1 => FillRule::EvenOdd,
            _ => FillRule::NonZero,
        };
        let Some(source) = o.id(p::CLIPPING_SHAPE_SOURCE_ID) else {
            return Ok(None);
        };
        let mut shapes = Vec::new();
        for s in 1..self.ab.objects.len() {
            if !(self.object(s).is_some_and(|o| o.is(t::SHAPE))
                && self.is_ancestor_or_self(source, s))
            {
                continue;
            }
            let chain = self.chain(s)?;
            let outer = self.ops(&chain);
            for path in self.shape_paths(s, false)? {
                let mut transform = outer.clone();
                transform.extend(path.transform);
                shapes.push(ClipShape {
                    geometry: path.geometry,
                    transform,
                    rule,
                });
            }
        }
        // Rive clips by one path of all the source's outlines under the
        // clip's fill rule (two outlines even-odd are a ring), not by the
        // union of each one filled.
        let clip = Some(Clip {
            shapes,
            one_path: true,
        });
        self.clips.insert(c, clip.clone());
        Ok(clip)
    }

    /// Drawable `i`'s group: its transform and the shapes of its paints.
    fn shape(&mut self, i: usize) -> Result<Option<Node>, Error> {
        let chain = self.chain(i)?;
        let opacity = self.opacity_curves(&chain);
        let effects = self.children[i].iter().any(|&c| {
            self.children[c].iter().any(|&e| {
                self.object(e)
                    .is_some_and(|o| o.ty == t::TRIM_PATH || o.ty == t::DASH_PATH)
            })
        });
        let paths = self.shape_paths_with(i, effects)?;
        if paths.is_empty() {
            return Ok(None);
        }
        let transform = self.ops(&chain);
        let painted = self.paints(i, &paths, &opacity, &transform)?;
        if painted.is_empty() {
            // Rive hits a listener's shapes by their outlines, painted or
            // not.
            if !self
                .hit_targets
                .iter()
                .any(|&t| self.is_ancestor_or_self(t, i))
            {
                return Ok(None);
            }
            let outline = Node::shape(Shape {
                paths: paths.into_iter().map(|(s, _)| s).collect(),
                ..Default::default()
            });
            return Ok(Some(self.group(i, transform, vec![outline])));
        }
        Ok(Some(self.painted_group(i, transform, painted)))
    }

    /// Drawable `i`'s group of `painted` under `transform`; paints placed
    /// otherwise get groups of their own, in turn.
    fn painted_group(
        &self,
        i: usize,
        transform: Vec<TransformOp>,
        painted: Vec<(Node, Placement)>,
    ) -> Node {
        if painted
            .iter()
            .all(|(_, at)| matches!(at, Placement::Inside))
        {
            let children = painted.into_iter().map(|(n, _)| n).collect();
            return self.group(i, transform, children);
        }
        let mut groups = Vec::new();
        let mut run: Vec<Node> = Vec::new();
        for (node, at) in painted {
            let own = match at {
                Placement::Inside => {
                    run.push(node);
                    continue;
                }
                Placement::Moved(op) => {
                    let mut moved = vec![op];
                    moved.extend(transform.iter().cloned());
                    moved
                }
                Placement::World(op) => op.into_iter().collect(),
            };
            if !run.is_empty() {
                groups.push(self.group(i, transform.clone(), std::mem::take(&mut run)));
            }
            groups.push(self.group(i, own, vec![node]));
        }
        if !run.is_empty() {
            groups.push(self.group(i, transform, run));
        }
        Node::Group(Group {
            children: groups,
            ..Group::new()
        })
    }

    fn group(&self, i: usize, transform: Vec<TransformOp>, children: Vec<Node>) -> Node {
        let name = self.object(i).map(|o| o.string(p::COMPONENT_NAME));
        Node::Group(Group {
            id: Some(i.to_string()),
            name: name.filter(|n| !n.is_empty()),
            transform,
            children,
            ..Group::new()
        })
    }

    /// The paths of shape `i`: those under it whose nearest shape it is, in
    /// its space. `exact` builds rectangles from their corners, starting
    /// where Rive's do (for trims and dashes).
    fn shape_paths(&mut self, i: usize, exact: bool) -> Result<Vec<ShapePath>, Error> {
        Ok(self
            .shape_paths_with(i, exact)?
            .into_iter()
            .map(|(s, _)| s)
            .collect())
    }

    /// The paths of shape `i`, each with how the clockwise fill rule takes
    /// it.
    fn shape_paths_with(
        &mut self,
        i: usize,
        exact: bool,
    ) -> Result<Vec<(ShapePath, Winding)>, Error> {
        let mut out = Vec::new();
        for pi in i + 1..self.ab.objects.len() {
            if !self.object(pi).is_some_and(|o| o.is(t::PATH)) || !self.is_ancestor_or_self(i, pi) {
                continue;
            }
            let chain = self.chain(pi)?;
            let Some(at) = chain.iter().position(|&c| c == i) else {
                continue;
            };
            // Its nearest shape must be `i`.
            if chain[at + 1..chain.len() - 1]
                .iter()
                .any(|&c| self.object(c).is_some_and(|o| o.is(t::SHAPE)))
            {
                continue;
            }
            // A skinned path's vertices are deformed into the world: drawn
            // in the shape's space, they are moved back from it.
            let (transform, geometry, det) = match self.skin(pi)? {
                Some(skin) => {
                    let shape = self.ops(&chain[..=at]);
                    let geometry = self.points(pi, Some((&skin, &shape)));
                    let back = transform_at(&shape, 0.0)
                        .invert()
                        .unwrap_or(Matrix::IDENTITY);
                    (Vec::new(), geometry, back.determinant())
                }
                None => {
                    let transform = self.ops(&chain[at + 1..]);
                    let det = transform_at(&transform, 0.0).determinant();
                    (transform, self.geometry(pi, exact), det)
                }
            };
            let Some(o) = self.object(pi) else { continue };
            // Rive's `localClockwise` path: points paths drawn
            // counter-clockwise turned round, and holes the other way.
            let flagged_ccw = o.uint(p::PATH_PATH_FLAGS) & 2 != 0;
            let not_clockwise =
                o.is(t::POINTS_PATH) && det * if flagged_ccw { -1.0 } else { 1.0 } < 0.0;
            let hole = o.flag(p::PATH_IS_HOLE);
            out.push((
                ShapePath {
                    geometry,
                    transform,
                },
                Winding {
                    backwards: not_clockwise != hole,
                    hole,
                },
            ));
        }
        Ok(out)
    }

    /// Path `i`'s outline.
    fn geometry(&mut self, i: usize, exact: bool) -> Geometry {
        let Some(o) = self.object(i) else {
            return Geometry::path(Path::default());
        };
        let mut inputs = Inputs::default();
        let parametric = o.is(t::PARAMETRIC_PATH);
        if parametric {
            for key in [
                p::PARAMETRIC_PATH_WIDTH,
                p::PARAMETRIC_PATH_HEIGHT,
                p::PARAMETRIC_PATH_ORIGIN_X,
                p::PARAMETRIC_PATH_ORIGIN_Y,
            ] {
                inputs.add(self, i, key);
            }
        }
        match o.ty {
            t::ELLIPSE => {
                let center = |v: &Inputs, t: f64, size: u16, origin: u16| {
                    v.at(i, size, t) * (0.5 - v.at(i, origin, t))
                };
                let cx = self.derive(&inputs, |v, t| {
                    center(v, t, p::PARAMETRIC_PATH_WIDTH, p::PARAMETRIC_PATH_ORIGIN_X)
                });
                let cy = self.derive(&inputs, |v, t| {
                    center(v, t, p::PARAMETRIC_PATH_HEIGHT, p::PARAMETRIC_PATH_ORIGIN_Y)
                });
                let rx = self.derive(&inputs, |v, t| v.at(i, p::PARAMETRIC_PATH_WIDTH, t) / 2.0);
                let ry = self.derive(&inputs, |v, t| v.at(i, p::PARAMETRIC_PATH_HEIGHT, t) / 2.0);
                return Geometry::Ellipse {
                    center: AnimatedPoint { x: cx, y: cy },
                    radius: AnimatedPoint { x: rx, y: ry },
                    direction: Direction::Clockwise,
                    start: EllipseStart::Top,
                };
            }
            t::RECTANGLE => {
                let linked = o.flag(p::RECTANGLE_LINK_CORNER_RADIUS);
                let radii = [
                    p::RECTANGLE_CORNER_RADIUS_TL,
                    p::RECTANGLE_CORNER_RADIUS_TR,
                    p::RECTANGLE_CORNER_RADIUS_BR,
                    p::RECTANGLE_CORNER_RADIUS_BL,
                ];
                for key in radii {
                    inputs.add(self, i, key);
                }
                // The scene's rectangle starts on its top edge, Rive's on
                // its left: the same unless trimmed or dashed.
                let tl = inputs.curve(i, radii[0]);
                let same = linked
                    || radii[1..].iter().all(|k| {
                        let c = inputs.curve(i, *k);
                        !c.is_animated() && !tl.is_animated() && c.at(0.0) == tl.at(0.0)
                    });
                let cut_in = tl.setup < 0.0 || tl.keys.iter().any(|k| k.value < 0.0);
                if same && !cut_in && !exact {
                    return self.rect(i, &inputs);
                }
                let built = self.derive_path(&inputs, |v, t| {
                    let w = v.at(i, p::PARAMETRIC_PATH_WIDTH, t);
                    let h = v.at(i, p::PARAMETRIC_PATH_HEIGHT, t);
                    let r = |k: u16| v.at(i, if linked { radii[0] } else { k }, t);
                    let verts = geometry::rectangle(
                        w,
                        h,
                        v.at(i, p::PARAMETRIC_PATH_ORIGIN_X, t),
                        v.at(i, p::PARAMETRIC_PATH_ORIGIN_Y, t),
                        [r(radii[0]), r(radii[1]), r(radii[2]), r(radii[3])],
                    );
                    geometry::build(&verts, true)
                });
                return Geometry::Path { path: built };
            }
            t::POLYGON | t::STAR => {
                inputs.add(self, i, p::POLYGON_POINTS);
                inputs.add(self, i, p::POLYGON_CORNER_RADIUS);
                let star = o.ty == t::STAR;
                if star {
                    inputs.add(self, i, p::STAR_INNER_RADIUS);
                }
                let built = self.derive_path(&inputs, |v, t| {
                    let verts = geometry::polygon(
                        v.at(i, p::PARAMETRIC_PATH_WIDTH, t),
                        v.at(i, p::PARAMETRIC_PATH_HEIGHT, t),
                        v.at(i, p::PARAMETRIC_PATH_ORIGIN_X, t),
                        v.at(i, p::PARAMETRIC_PATH_ORIGIN_Y, t),
                        (v.at(i, p::POLYGON_POINTS, t).round().max(0.0) as usize).min(4096),
                        v.at(i, p::POLYGON_CORNER_RADIUS, t),
                        star.then(|| v.at(i, p::STAR_INNER_RADIUS, t)),
                    );
                    geometry::build(&verts, true)
                });
                return Geometry::Path { path: built };
            }
            t::TRIANGLE => {
                let built = self.derive_path(&inputs, |v, t| {
                    let verts = geometry::triangle(
                        v.at(i, p::PARAMETRIC_PATH_WIDTH, t),
                        v.at(i, p::PARAMETRIC_PATH_HEIGHT, t),
                        v.at(i, p::PARAMETRIC_PATH_ORIGIN_X, t),
                        v.at(i, p::PARAMETRIC_PATH_ORIGIN_Y, t),
                    );
                    geometry::build(&verts, true)
                });
                return Geometry::Path { path: built };
            }
            _ => {}
        }
        if !o.is(t::POINTS_COMMON_PATH) {
            if let Some(name) = defs::type_name(o.ty) {
                self.warn(format!("left out: {name} outlines"));
            }
            return Geometry::path(Path::default());
        }
        self.points(i, None)
    }

    /// Points path `i`'s outline; `skinned`, its skin and the world
    /// transform of the shape it is drawn in.
    fn points(&mut self, i: usize, skinned: Option<(&Skin, &[TransformOp])>) -> Geometry {
        let Some(o) = self.object(i) else {
            return Geometry::path(Path::default());
        };
        let mut inputs = Inputs::default();
        let closed = o.flag(p::POINTS_COMMON_PATH_IS_CLOSED);
        let vertices: Vec<usize> = self.children[i]
            .iter()
            .copied()
            .filter(|&v| self.object(v).is_some_and(|o| o.is(t::PATH_VERTEX)))
            .collect();
        for &v in &vertices {
            let Some(vo) = self.object(v) else { continue };
            let keys: &[u16] = match vo.ty {
                t::STRAIGHT_VERTEX => &[p::STRAIGHT_VERTEX_RADIUS],
                t::CUBIC_MIRRORED_VERTEX => &[
                    p::CUBIC_MIRRORED_VERTEX_ROTATION,
                    p::CUBIC_MIRRORED_VERTEX_DISTANCE,
                ],
                t::CUBIC_ASYMMETRIC_VERTEX => &[
                    p::CUBIC_ASYMMETRIC_VERTEX_ROTATION,
                    p::CUBIC_ASYMMETRIC_VERTEX_IN_DISTANCE,
                    p::CUBIC_ASYMMETRIC_VERTEX_OUT_DISTANCE,
                ],
                t::CUBIC_DETACHED_VERTEX => &[
                    p::CUBIC_DETACHED_VERTEX_IN_ROTATION,
                    p::CUBIC_DETACHED_VERTEX_IN_DISTANCE,
                    p::CUBIC_DETACHED_VERTEX_OUT_ROTATION,
                    p::CUBIC_DETACHED_VERTEX_OUT_DISTANCE,
                ],
                _ => &[],
            };
            inputs.add(self, v, p::VERTEX_X);
            inputs.add(self, v, p::VERTEX_Y);
            for &key in keys {
                inputs.add(self, v, key);
            }
        }
        let objects: Vec<(usize, u16)> = vertices
            .iter()
            .filter_map(|&v| Some((v, self.object(v)?.ty)))
            .collect();
        let Some((skin, shape)) = skinned else {
            let built = self.derive_path(&inputs, |v, t| {
                let verts: Vec<Vertex> =
                    objects.iter().map(|&(o, ty)| vertex(v, o, ty, t)).collect();
                geometry::build(&verts, closed)
            });
            return Geometry::Path { path: built };
        };
        let weights: Vec<[(u32, u32); 3]> = vertices.iter().map(|&v| self.weights(v)).collect();
        let mut breaks = inputs.breaks();
        skin.breaks(&mut breaks);
        ops_breaks(shape, &mut breaks);
        // Built in the world, as Rive builds it (corners round there), then
        // moved into the shape.
        let built = self.baker.derive(&Breaks::all(&breaks), |t| {
            let bones = skin.bones(t);
            let at = |p: Point, (indices, values): (u32, u32)| {
                deform(p, indices, values, skin.world, &bones)
            };
            let verts: Vec<Vertex> = objects
                .iter()
                .zip(&weights)
                .map(|(&(o, ty), w)| match vertex(&inputs, o, ty, t) {
                    Vertex::Straight { p, radius } => Vertex::Straight {
                        p: at(p, w[0]),
                        radius,
                    },
                    Vertex::Cubic { p, r#in, out } => Vertex::Cubic {
                        p: at(p, w[0]),
                        r#in: at(r#in, w[1]),
                        out: at(out, w[2]),
                    },
                })
                .collect();
            let back = transform_at(shape, t).invert().unwrap_or(Matrix::IDENTITY);
            geometry::build(&verts, closed).transformed(back)
        });
        Geometry::Path { path: built }
    }

    /// The skin of path or mesh `i`, if it has one.
    fn skin(&mut self, i: usize) -> Result<Option<Skin>, Error> {
        let Some(s) = self.children[i]
            .iter()
            .copied()
            .find(|&c| self.object(c).is_some_and(|o| o.ty == t::SKIN))
        else {
            return Ok(None);
        };
        let Some(so) = self.object(s) else {
            return Ok(None);
        };
        let matrix = |o: &Object, keys: [u16; 6]| {
            let [a, b, c, d, e, f] = keys.map(|k| o.num(k));
            Matrix::new(a, b, c, d, e, f)
        };
        let world = matrix(
            so,
            [
                p::SKIN_XX,
                p::SKIN_XY,
                p::SKIN_YX,
                p::SKIN_YY,
                p::SKIN_TX,
                p::SKIN_TY,
            ],
        );
        let mut tendons = Vec::new();
        for c in self.children[s].clone() {
            let Some(to) = self.object(c).filter(|o| o.ty == t::TENDON) else {
                continue;
            };
            // Rive keeps the identity when the bind does not invert.
            let inverse = matrix(
                to,
                [
                    p::TENDON_XX,
                    p::TENDON_XY,
                    p::TENDON_YX,
                    p::TENDON_YY,
                    p::TENDON_TX,
                    p::TENDON_TY,
                ],
            )
            .invert()
            .unwrap_or(Matrix::IDENTITY);
            let bone = to
                .id(p::TENDON_BONE_ID)
                .filter(|&b| self.object(b).is_some_and(|o| o.is(t::BONE)));
            let ops = match bone {
                Some(b) => {
                    let chain = self.chain(b)?;
                    self.ops(&chain)
                }
                None => Vec::new(),
            };
            tendons.push((ops, inverse));
        }
        Ok(Some(Skin { world, tendons }))
    }

    /// The bone indices and weights of vertex `v`: of its point, its in
    /// handle and its out handle. A vertex without them follows no bone.
    fn weights(&self, v: usize) -> [(u32, u32); 3] {
        let weight = self.children[v]
            .iter()
            .find_map(|&w| self.object(w).filter(|o| o.is(t::WEIGHT)));
        let Some(w) = weight else {
            return [(0, 255); 3];
        };
        let pair = |indices: u16, values: u16| (w.uint(indices) as u32, w.uint(values) as u32);
        let point = pair(p::WEIGHT_INDICES, p::WEIGHT_VALUES);
        if w.ty != t::CUBIC_WEIGHT {
            return [point; 3];
        }
        [
            point,
            pair(p::CUBIC_WEIGHT_IN_INDICES, p::CUBIC_WEIGHT_IN_VALUES),
            pair(p::CUBIC_WEIGHT_OUT_INDICES, p::CUBIC_WEIGHT_OUT_VALUES),
        ]
    }

    /// Rectangle `i` as the scene's rectangle.
    fn rect(&mut self, i: usize, inputs: &Inputs) -> Geometry {
        let at = |v: &Inputs, k: u16, t: f64| v.at(i, k, t);
        let cx = self.derive(inputs, |v, t| {
            at(v, p::PARAMETRIC_PATH_WIDTH, t) * (0.5 - at(v, p::PARAMETRIC_PATH_ORIGIN_X, t))
        });
        let cy = self.derive(inputs, |v, t| {
            at(v, p::PARAMETRIC_PATH_HEIGHT, t) * (0.5 - at(v, p::PARAMETRIC_PATH_ORIGIN_Y, t))
        });
        let w = self.derive(inputs, |v, t| at(v, p::PARAMETRIC_PATH_WIDTH, t));
        let h = self.derive(inputs, |v, t| at(v, p::PARAMETRIC_PATH_HEIGHT, t));
        // Rive rounds by at most half the shorter side, alike on both axes.
        let r = self.derive(inputs, |v, t| {
            let (w, h) = (
                at(v, p::PARAMETRIC_PATH_WIDTH, t),
                at(v, p::PARAMETRIC_PATH_HEIGHT, t),
            );
            at(v, p::RECTANGLE_CORNER_RADIUS_TL, t)
                .min(w.abs() / 2.0)
                .min(h.abs() / 2.0)
                .max(0.0)
        });
        Geometry::Rect {
            center: AnimatedPoint { x: cx, y: cy },
            size: AnimatedPoint { x: w, y: h },
            radius: AnimatedPoint { x: r.clone(), y: r },
            direction: Direction::Clockwise,
            start: RectStart::TopLeft,
        }
    }

    fn derive(&mut self, inputs: &Inputs, at: impl Fn(&Inputs, f64) -> f64) -> Animated<f64> {
        let breaks = inputs.breaks();
        self.baker.derive(&Breaks::all(&breaks), |t| at(inputs, t))
    }

    fn derive_path(
        &mut self,
        inputs: &Inputs,
        at: impl Fn(&Inputs, f64) -> Path,
    ) -> Animated<Path> {
        let breaks = inputs.breaks();
        self.baker.derive(&Breaks::all(&breaks), |t| at(inputs, t))
    }

    /// The shapes of container `i`'s fills and strokes, in its order, over
    /// `paths`; `opacity` multiplies them, `outer` is the container's
    /// transform. Each comes with where it goes.
    fn paints(
        &mut self,
        i: usize,
        with: &[(ShapePath, Winding)],
        opacity: &[Curve<f64>],
        outer: &[TransformOp],
    ) -> Result<Vec<(Node, Placement)>, Error> {
        let paths: Vec<ShapePath> = with.iter().map(|(s, _)| s.clone()).collect();
        let paths = &paths[..];
        let mut out = Vec::new();
        let paints: Vec<usize> = self.children[i]
            .iter()
            .copied()
            .filter(|&c| self.object(c).is_some_and(|o| o.is(t::SHAPE_PAINT)))
            .collect();
        let drawable_blend = self
            .object(i)
            .filter(|o| o.is(t::DRAWABLE))
            .map_or(3, |o| o.uint(p::DRAWABLE_BLEND_MODE_VALUE));
        for c in paints {
            let Some(o) = self.object(c) else { continue };
            let Some((paint, own_opacity)) = self.paint(c) else {
                continue;
            };
            let mut factors = opacity.to_vec();
            factors.push(self.num_curve(c, p::SHAPE_PAINT_IS_VISIBLE));
            if let Some(g) = own_opacity {
                factors.push(g);
            }
            let alpha = self.product(&factors);
            if is_const(&alpha, 0.0) {
                continue;
            }
            let mut shape = Shape {
                id: Some(c.to_string()),
                paths: paths.to_vec(),
                ..Shape::default()
            };
            let world = o.ty == t::STROKE && !o.flag(p::STROKE_TRANSFORM_AFFECTS_STROKE);
            let mut dash = None;
            let mut feather = None;
            for &e in &self.children[c].clone() {
                let Some(eo) = self.object(e) else { continue };
                match eo.ty {
                    t::TRIM_PATH => {
                        shape.trim = Some(Trim {
                            start: self.num(e, p::TRIM_PATH_START, 1.0),
                            end: self.num(e, p::TRIM_PATH_END, 1.0),
                            offset: self.num(e, p::TRIM_PATH_OFFSET, 1.0),
                            mode: match eo.uint(p::TRIM_PATH_MODE_VALUE) {
                                1 => TrimMode::Combined,
                                _ => TrimMode::EachPath,
                            },
                        });
                    }
                    t::DASH_PATH => dash = self.dash(e, paths),
                    t::FEATHER => feather = Some(e),
                    _ => {}
                }
            }
            // Rive's renderer draws feathered fills only under the clockwise
            // rule.
            if o.ty == t::FILL && feather.is_some() && o.uint(p::FILL_FILL_RULE) != 2 {
                continue;
            }
            if o.ty == t::FILL {
                let rule = match o.uint(p::FILL_FILL_RULE) {
                    1 => FillRule::EvenOdd,
                    2 => {
                        // With every outline turned clockwise and holes the
                        // other way, the clockwise rule fills what non-zero
                        // does, unless holes reach past the outlines.
                        if with.iter().any(|(_, w)| w.hole) {
                            self.warn("holes under the clockwise fill rule are drawn as non-zero");
                        }
                        shape.paths = with
                            .iter()
                            .map(|(s, w)| if w.backwards { reversed(s) } else { s.clone() })
                            .collect();
                        FillRule::NonZero
                    }
                    _ => FillRule::NonZero,
                };
                shape.fill = Some(Fill {
                    paint,
                    opacity: alpha,
                    rule,
                });
            } else {
                // A stroke the transform does not scale is drawn on the
                // outline moved into the artboard's space, untransformed
                // (Rive's world path): its width and its gradient are the
                // artboard's.
                let paint = if world {
                    for path in &mut shape.paths {
                        let mut transform = outer.to_vec();
                        transform.append(&mut path.transform);
                        path.transform = transform;
                    }
                    self.world_paint(paint, outer)
                } else {
                    paint
                };
                shape.stroke = Some(Stroke {
                    paint,
                    opacity: alpha,
                    width: self.num(c, p::STROKE_THICKNESS, 1.0),
                    cap: match o.uint(p::STROKE_CAP) {
                        1 => LineCap::Round,
                        2 => LineCap::Square,
                        _ => LineCap::Butt,
                    },
                    join: match o.uint(p::STROKE_JOIN) {
                        1 => LineJoin::Round,
                        2 => LineJoin::Bevel,
                        _ => LineJoin::Miter,
                    },
                    miter_limit: 4.0,
                    dash,
                });
            }
            let blend = match o.uint(p::SHAPE_PAINT_BLEND_MODE_VALUE) {
                127 => drawable_blend,
                b => b,
            };
            let (node, outside) = match feather {
                Some(f) => {
                    let space = if world { &[][..] } else { outer };
                    self.feathered(f, shape, o.ty == t::FILL, space)
                }
                None => (Node::shape(shape), None),
            };
            let node = match blend_mode(blend) {
                BlendMode::Normal => node,
                blend => Node::Group(Group {
                    blend,
                    children: vec![node],
                    ..Group::new()
                }),
            };
            let at = match (world, outside) {
                (true, op) => Placement::World(op),
                (false, Some(op)) => Placement::Moved(op),
                (false, None) => Placement::Inside,
            };
            out.push((node, at));
        }
        Ok(out)
    }

    /// `paint`'s gradient in the artboard's space, from the space `outer`
    /// transforms (Rive's gradients of world paths).
    fn world_paint(&mut self, paint: Paint, outer: &[TransformOp]) -> Paint {
        if outer.is_empty() {
            return paint;
        }
        match paint {
            Paint::LinearGradient(mut g) => {
                g.start = self.world_point(&g.start, outer);
                g.end = self.world_point(&g.end, outer);
                Paint::LinearGradient(g)
            }
            Paint::RadialGradient(mut g) => {
                g.center = self.world_point(&g.center, outer);
                g.focal = self.world_point(&g.focal, outer);
                let mut b = Vec::new();
                ops_breaks(outer, &mut b);
                breaks(&g.radius, &mut b);
                let radius = g.radius.clone();
                g.radius = self.baker.derive(&Breaks::all(&b), |t| {
                    radius.sample(t) * transform_at(outer, t).determinant().abs().sqrt()
                });
                Paint::RadialGradient(g)
            }
            other => other,
        }
    }

    /// `point` moved by `outer`.
    fn world_point(&mut self, point: &AnimatedPoint, outer: &[TransformOp]) -> AnimatedPoint {
        let mut b = Vec::new();
        ops_breaks(outer, &mut b);
        breaks(&point.x, &mut b);
        breaks(&point.y, &mut b);
        let at = |t: f64| transform_at(outer, t).apply(point.sample(t));
        AnimatedPoint {
            x: self.baker.derive(&Breaks::all(&b), |t| at(t).x),
            y: self.baker.derive(&Breaks::all(&b), |t| at(t).y),
        }
    }

    /// `shape` feathered by feather `f`, and the translation to apply
    /// outside the drawable's transform when the feather's offset is in the
    /// artboard's space.
    ///
    /// Rive's renderer feathers by a Gaussian whose standard deviation is
    /// half the strength (strengths are two deviations, as blurs are in
    /// design tools), in the shape's lengths. An inner feather fills the
    /// area around the outline (a rectangle past it with the outline cut
    /// out, moved by the offset), feathered and clipped to the outline: a
    /// glow or shadow inside it.
    fn feathered(
        &mut self,
        f: usize,
        mut shape: Shape,
        fill: bool,
        outer: &[TransformOp],
    ) -> (Node, Option<TransformOp>) {
        let Some(fo) = self.object(f) else {
            return (Node::shape(shape), None);
        };
        let sigma = self.num(f, p::FEATHER_STRENGTH, 0.5);
        let x = self.num(f, p::FEATHER_OFFSET_X, 1.0);
        let y = self.num(f, p::FEATHER_OFFSET_Y, 1.0);
        let offset =
            (!(is_const(&x, 0.0) && is_const(&y, 0.0))).then_some(TransformOp::Translate { x, y });
        // 0 world (the artboard's), 1 local (the shape's).
        let world = fo.uint(p::FEATHER_SPACE_VALUE) == 0;
        let inner = fill && fo.flag(p::FEATHER_INNER);
        let effects = if is_const(&sigma, 0.0) {
            Vec::new()
        } else {
            vec![Effect::Blur(Blur {
                radius: sigma.clone(),
            })]
        };
        if !inner {
            let (local, outside) = if world {
                (None, offset)
            } else {
                (offset, None)
            };
            let node = Node::Group(Group {
                transform: local.into_iter().collect(),
                effects,
                children: vec![Node::shape(shape)],
                ..Group::new()
            });
            return (node, outside);
        }
        // The outline as the clip, then the area around it.
        let rule = shape.fill.as_ref().map_or(FillRule::NonZero, |f| f.rule);
        let clip: Vec<ClipShape> = shape
            .paths
            .iter()
            .map(|path| ClipShape {
                geometry: path.geometry.clone(),
                transform: path.transform.clone(),
                rule,
            })
            .collect();
        let reach = sigma
            .keyframes()
            .iter()
            .map(|k| k.value)
            .fold(sigma.as_static().copied().unwrap_or(0.0), f64::max)
            * 3.0;
        let mut moved = shape.paths.clone();
        if let Some(mut offset) = offset {
            // An offset in the artboard's space, turned into the shape's.
            if world
                && !outer.is_empty()
                && let TransformOp::Translate { x, y } = &offset
            {
                let mut b = Vec::new();
                breaks(x, &mut b);
                breaks(y, &mut b);
                ops_breaks(outer, &mut b);
                let local = |t: f64| {
                    transform_at(outer, t)
                        .invert()
                        .unwrap_or(Matrix::IDENTITY)
                        .apply_vector(Point::new(x.sample(t), y.sample(t)))
                };
                let lx = self.baker.derive(&Breaks::all(&b), |t| local(t).x);
                let ly = self.baker.derive(&Breaks::all(&b), |t| local(t).y);
                offset = TransformOp::Translate { x: lx, y: ly };
            }
            for path in &mut moved {
                path.transform.insert(0, offset.clone());
            }
        }
        let Some(bounds) = paths_bounds(&shape.paths) else {
            return (Node::shape(shape), None);
        };
        let pad = reach + offset_reach(&moved);
        let around = Rect {
            x: bounds.x - pad,
            y: bounds.y - pad,
            width: bounds.width + 2.0 * pad,
            height: bounds.height + 2.0 * pad,
        };
        // The rectangle with the moved outline cut out of it: Rive fills the
        // rectangle and the outline turned back under its clockwise rule
        // (where the winding is positive), so where outlines overlap
        // nothing is shaded either.
        let mut cut = shape.clone();
        cut.paths = moved;
        cut.fill = Some(Fill {
            paint: Paint::solid(Color::BLACK),
            opacity: Animated::Static(1.0),
            rule,
        });
        shape.paths = vec![ShapePath::new(Geometry::path(
            geometry::build(
                &geometry::rectangle(around.width, around.height, 0.0, 0.0, [0.0; 4]),
                true,
            )
            .transformed(Matrix::translate(around.x, around.y)),
        ))];
        let node = Node::Group(Group {
            clip: Some(Clip {
                shapes: clip,
                one_path: true,
            }),
            children: vec![Node::Group(Group {
                effects,
                children: vec![
                    Node::shape(shape),
                    Node::Group(Group {
                        blend: BlendMode::DestinationOut,
                        children: vec![Node::shape(cut)],
                        ..Group::new()
                    }),
                ],
                ..Group::new()
            })],
            ..Group::new()
        });
        (node, None)
    }

    /// Paint `i`'s color or gradient, and the gradient's own opacity.
    fn paint(&mut self, i: usize) -> Option<(Paint, Option<Curve<f64>>)> {
        let m = self.children[i].iter().copied().find(|&c| {
            self.object(c)
                .is_some_and(|o| o.ty == t::SOLID_COLOR || o.is(t::LINEAR_GRADIENT))
        })?;
        let o = self.object(m)?;
        if o.ty == t::SOLID_COLOR {
            let curve = self.color_curve(m, p::SOLID_COLOR_COLOR_VALUE);
            return Some((
                Paint::Solid {
                    color: self.baker.curve(&curve),
                },
                None,
            ));
        }
        let mut inputs = Inputs::default();
        for key in [
            p::LINEAR_GRADIENT_START_X,
            p::LINEAR_GRADIENT_START_Y,
            p::LINEAR_GRADIENT_END_X,
            p::LINEAR_GRADIENT_END_Y,
        ] {
            inputs.add(self, m, key);
        }
        let stops: Vec<usize> = self.children[m]
            .iter()
            .copied()
            .filter(|&s| self.object(s).is_some_and(|o| o.ty == t::GRADIENT_STOP))
            .collect();
        let mut colors = Vec::new();
        for &s in &stops {
            inputs.add(self, s, p::GRADIENT_STOP_POSITION);
            colors.push(self.color_curve(s, p::GRADIENT_STOP_COLOR_VALUE));
        }
        let mut breaks = inputs.breaks();
        breaks.extend(colors.iter().map(Curve::breaks));
        let stops_at = |t: f64| {
            let mut v: Vec<GradientStop> = stops
                .iter()
                .zip(&colors)
                .map(|(&s, c)| GradientStop {
                    offset: inputs.at(s, p::GRADIENT_STOP_POSITION, t).clamp(0.0, 1.0),
                    color: c.at(t),
                })
                .collect();
            v.sort_by(|a, b| a.offset.total_cmp(&b.offset));
            v
        };
        let stops = self.baker.derive(&Breaks::all(&breaks), stops_at);
        let point = |c: &mut Self, x: u16, y: u16| AnimatedPoint {
            x: c.derive(&inputs, |v, t| v.at(m, x, t)),
            y: c.derive(&inputs, |v, t| v.at(m, y, t)),
        };
        let start = point(self, p::LINEAR_GRADIENT_START_X, p::LINEAR_GRADIENT_START_Y);
        let opacity = Some(self.num_curve(m, p::LINEAR_GRADIENT_OPACITY));
        if o.ty == t::RADIAL_GRADIENT {
            let radius = self.derive(&inputs, |v, t| {
                let s = Point::new(
                    v.at(m, p::LINEAR_GRADIENT_START_X, t),
                    v.at(m, p::LINEAR_GRADIENT_START_Y, t),
                );
                let e = Point::new(
                    v.at(m, p::LINEAR_GRADIENT_END_X, t),
                    v.at(m, p::LINEAR_GRADIENT_END_Y, t),
                );
                s.distance(e)
            });
            return Some((
                Paint::RadialGradient(RadialGradient {
                    focal: start.clone(),
                    center: start,
                    radius,
                    stops,
                    units: GradientUnits::UserSpace,
                    transform: Matrix::IDENTITY,
                    spread: Spread::Pad,
                }),
                opacity,
            ));
        }
        let end = point(self, p::LINEAR_GRADIENT_END_X, p::LINEAR_GRADIENT_END_Y);
        Some((
            Paint::LinearGradient(LinearGradient {
                start,
                end,
                stops,
                units: GradientUnits::UserSpace,
                transform: Matrix::IDENTITY,
                spread: Spread::Pad,
            }),
            opacity,
        ))
    }

    /// Dash path `i`'s dashes over `paths`.
    fn dash(&mut self, i: usize, paths: &[ShapePath]) -> Option<Dash> {
        let o = self.object(i)?;
        let dashes: Vec<usize> = self.children[i]
            .iter()
            .copied()
            .filter(|&d| self.object(d).is_some_and(|o| o.ty == t::DASH))
            .collect();
        if dashes.is_empty() {
            return None;
        }
        // Percentages are of the outline's length.
        let length = || {
            paths
                .iter()
                .map(|s| {
                    path_length(&dxn_vector_animation::outline::geometry_path(
                        &s.geometry,
                        0.0,
                    ))
                })
                .sum::<f64>()
        };
        let mut array = Vec::new();
        for d in dashes {
            let percent = self
                .object(d)
                .is_some_and(|o| o.flag(p::DASH_LENGTH_IS_PERCENTAGE));
            let scale = if percent {
                self.warn("dashes in percentages measured on the outline at its start");
                length()
            } else {
                1.0
            };
            array.push(self.num(d, p::DASH_LENGTH, scale));
        }
        let scale = if o.flag(p::DASH_PATH_OFFSET_IS_PERCENTAGE) {
            length()
        } else {
            1.0
        };
        Some(Dash {
            array,
            offset: self.num(i, p::DASH_PATH_OFFSET, scale),
        })
    }

    /// Text `i`: its runs shaped with their styles' fonts and laid out as
    /// Rive lays them out, each style's glyph outlines filled and stroked
    /// with the style's paints. Text whose runs or layout change over time
    /// is laid out at each change (each frame where a size or spacing moves)
    /// and each layout shown while it holds.
    fn text(&mut self, i: usize) -> Result<Option<Node>, Error> {
        let Some(o) = self.object(i) else {
            return Ok(None);
        };
        let runs: Vec<usize> = self.children[i]
            .iter()
            .copied()
            .filter(|&c| self.object(c).is_some_and(|o| o.ty == t::TEXT_VALUE_RUN))
            .collect();
        let groups = self.text_modifiers(i, &runs);
        // The styles the runs use, in the order they first do.
        let mut style_objects: Vec<usize> = Vec::new();
        for &r in &runs {
            if let Some(s) = self
                .object(r)
                .and_then(|o| o.id(p::TEXT_VALUE_RUN_STYLE_ID))
                && self.object(s).is_some_and(|o| o.is(t::TEXT_STYLE))
                && !style_objects.contains(&s)
            {
                style_objects.push(s);
            }
        }
        // What changes the layout over time.
        let mut texts: Vec<(Curve<f64>, Vec<String>)> = Vec::new();
        for &r in &runs {
            texts.push(self.string_curve(r, p::TEXT_VALUE_RUN_TEXT));
        }
        let mut numbers: Vec<Curve<f64>> = Vec::new();
        for key in [
            p::TEXT_WIDTH,
            p::TEXT_HEIGHT,
            p::TEXT_ORIGIN_X,
            p::TEXT_ORIGIN_Y,
            p::TEXT_PARAGRAPH_SPACING,
        ] {
            numbers.push(self.num_curve(i, key));
        }
        for &s in &style_objects {
            for key in [
                p::TEXT_STYLE_FONT_SIZE,
                p::TEXT_STYLE_LINE_HEIGHT,
                p::TEXT_STYLE_LETTER_SPACING,
            ] {
                numbers.push(self.num_curve(s, key));
            }
        }
        // The fonts, embedded.
        let mut fonts: Vec<Option<&'a [u8]>> = Vec::new();
        for &s in &style_objects {
            let font = self
                .object(s)
                .and_then(|o| o.id(p::TEXT_STYLE_FONT_ASSET_ID))
                .and_then(|a| self.file.assets.get(a))
                .and_then(|a| a.contents.as_deref());
            if font.is_none() {
                self.warn("left out: text in fonts the file does not embed");
            }
            fonts.push(font);
        }
        let axes: Vec<Vec<([u8; 4], f32)>> = style_objects
            .iter()
            .map(|&s| {
                self.children[s]
                    .iter()
                    .filter_map(|&a| self.object(a).filter(|o| o.ty == t::TEXT_STYLE_AXIS))
                    .map(|a| {
                        (
                            (a.uint(p::TEXT_STYLE_AXIS_TAG) as u32).to_be_bytes(),
                            a.num(p::TEXT_STYLE_AXIS_AXIS_VALUE) as f32,
                        )
                    })
                    .collect()
            })
            .collect();
        if o.uint(p::TEXT_OVERFLOW_VALUE) == 3 {
            self.warn("text ellipses are drawn as hidden overflow");
        }
        // When the layout changes: at each keyframe, and each frame where a
        // number moves.
        let mut times: Vec<f64> = vec![0.0];
        for (curve, _) in &texts {
            times.extend(curve.keys.iter().map(|k| k.t));
        }
        for curve in &numbers {
            times.extend(curve.keys.iter().map(|k| k.t));
            for w in curve.keys.windows(2) {
                if w[0].interp != Interp::Hold && w[0].value != w[1].value {
                    let n = ((w[1].t - w[0].t) * 60.0).ceil() as usize;
                    times.extend(
                        (1..n.min(3600)).map(|j| w[0].t + (w[1].t - w[0].t) * j as f64 / n as f64),
                    );
                }
            }
        }
        times.sort_by(f64::total_cmp);
        times.dedup();
        times.truncate(4096);
        let run_styles: Vec<Option<usize>> = runs
            .iter()
            .map(|&r| {
                let style = self.object(r)?.id(p::TEXT_VALUE_RUN_STYLE_ID)?;
                style_objects.iter().position(|s| *s == style)
            })
            .collect();
        let layout_at = |t: f64| -> (Vec<text::Run>, Vec<f64>) {
            let runs_at = run_styles
                .iter()
                .zip(&texts)
                .filter_map(|(style, (curve, table))| {
                    let style = (*style)?;
                    let text = table.get(curve.at(t).max(0.0) as usize)?.clone();
                    Some(text::Run { text, style })
                })
                .collect();
            (runs_at, numbers.iter().map(|c| c.at(t)).collect())
        };
        // Each distinct layout and the times it shows.
        let mut states: Vec<(Layout, Vec<f64>)> = Vec::new();
        let mut shown: Vec<usize> = Vec::new();
        for &t in &times {
            let (runs_at, values) = layout_at(t);
            let key = (
                runs_at.iter().map(|r| r.text.clone()).collect::<Vec<_>>(),
                values,
            );
            let at = match states.iter().position(|(k, _)| *k == key) {
                Some(s) => s,
                None => {
                    states.push((key, Vec::new()));
                    states.len() - 1
                }
            };
            if shown.last() != Some(&at) {
                states[at].1.push(t);
                shown.push(at);
            }
        }
        let chain = self.chain(i)?;
        let opacity = self.opacity_curves(&chain);
        let transform = self.ops(&chain);
        let mut versions = Vec::new();
        for (state, ((_, values), starts)) in states.iter().enumerate() {
            let t = starts[0];
            let (runs_at, _) = layout_at(t);
            let styles: Vec<text::Style<'_>> = fonts
                .iter()
                .enumerate()
                .map(|(k, font)| text::Style {
                    font: font.unwrap_or(&[]),
                    size: values[5 + k * 3],
                    line_height: values[6 + k * 3],
                    letter_spacing: values[7 + k * 3],
                    axes: axes[k].clone(),
                })
                .collect();
            let params = text::Params {
                sizing: o.uint(p::TEXT_SIZING_VALUE),
                width: values[0],
                height: values[1],
                align: o.uint(p::TEXT_ALIGN_VALUE),
                wrap: o.uint(p::TEXT_WRAP_VALUE),
                overflow: o.uint(p::TEXT_OVERFLOW_VALUE),
                origin_x: values[2],
                origin_y: values[3],
                paragraph_spacing: values[4],
                baseline_origin: o.uint(p::TEXT_ORIGIN_VALUE) == 1,
                vertical_align: o.uint(p::TEXT_VERTICAL_ALIGN_VALUE),
                fit_from_baseline: o.flag(p::TEXT_FIT_FROM_BASELINE),
            };
            let laid = text::lay_out(&runs_at, &styles, &params);
            let mut children = Vec::new();
            if groups.is_empty() {
                for (style, path) in &laid.styles {
                    let paths = [(
                        ShapePath::new(Geometry::path(path.clone())),
                        Winding::default(),
                    )];
                    let painted =
                        self.paints(style_objects[*style], &paths, &opacity, &transform)?;
                    children.extend(painted.into_iter().map(|(n, _)| n));
                }
            } else {
                children =
                    self.modified_glyphs(&groups, &laid, &style_objects, &opacity, &transform)?;
            }
            let mut group = Group {
                clip: laid.clip.map(|r| Clip {
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
                    one_path: false,
                }),
                children,
                ..Group::new()
            };
            if states.len() > 1 {
                // Shown from each of its starts to the next change.
                let mut keys = Vec::new();
                for (k, &t) in times.iter().enumerate() {
                    let (runs_at, values) = layout_at(t);
                    let key = (
                        runs_at.iter().map(|r| r.text.clone()).collect::<Vec<_>>(),
                        values,
                    );
                    let on = states[state].0 == key;
                    if k == 0
                        || keys
                            .last()
                            .is_none_or(|l: &Keyframe<f64>| (l.value == 1.0) != on)
                    {
                        keys.push(Keyframe::new(t, if on { 1.0 } else { 0.0 }, Easing::Hold));
                    }
                }
                group.opacity = Animated::from_keyframes(keys, 0.0);
            }
            versions.push(Node::Group(group));
        }
        let mut node = self.group(i, transform, versions);
        if let Node::Group(g) = &mut node {
            g.blend = blend_mode(o.uint(p::DRAWABLE_BLEND_MODE_VALUE));
        }
        Ok(Some(node))
    }

    /// The modifier groups of text `i` (its runs `runs`), as they move.
    fn text_modifiers(&mut self, i: usize, runs: &[usize]) -> Vec<modifiers::Group> {
        let mut groups = Vec::new();
        for g in self.children[i].clone() {
            if !self
                .object(g)
                .is_some_and(|o| o.ty == t::TEXT_MODIFIER_GROUP)
            {
                continue;
            }
            let mut values = modifiers::Values::default();
            for key in [
                p::TEXT_MODIFIER_GROUP_MODIFIER_FLAGS,
                p::TEXT_MODIFIER_GROUP_ORIGIN_X,
                p::TEXT_MODIFIER_GROUP_ORIGIN_Y,
                p::TEXT_MODIFIER_GROUP_OPACITY,
                p::TEXT_MODIFIER_GROUP_X,
                p::TEXT_MODIFIER_GROUP_Y,
                p::TEXT_MODIFIER_GROUP_ROTATION,
                p::TEXT_MODIFIER_GROUP_SCALE_X,
                p::TEXT_MODIFIER_GROUP_SCALE_Y,
            ] {
                values.0.insert(key, self.num_curve(g, key));
            }
            let mut ranges = Vec::new();
            for r in self.children[g].clone() {
                let Some(ro) = self.object(r) else { continue };
                if ro.is(t::TEXT_MODIFIER) {
                    if let Some(name) = defs::type_name(ro.ty) {
                        self.warn(format!(
                            "left out: {name}s (text drawn as the others modify it)"
                        ));
                    }
                    continue;
                }
                if ro.ty != t::TEXT_MODIFIER_RANGE {
                    continue;
                }
                let mut range = modifiers::Values::default();
                for key in [
                    p::TEXT_MODIFIER_RANGE_UNITS_VALUE,
                    p::TEXT_MODIFIER_RANGE_TYPE_VALUE,
                    p::TEXT_MODIFIER_RANGE_MODE_VALUE,
                    p::TEXT_MODIFIER_RANGE_CLAMP,
                    p::TEXT_MODIFIER_RANGE_MODIFY_FROM,
                    p::TEXT_MODIFIER_RANGE_MODIFY_TO,
                    p::TEXT_MODIFIER_RANGE_STRENGTH,
                    p::TEXT_MODIFIER_RANGE_FALLOFF_FROM,
                    p::TEXT_MODIFIER_RANGE_FALLOFF_TO,
                    p::TEXT_MODIFIER_RANGE_OFFSET,
                ] {
                    range.0.insert(key, self.num_curve(r, key));
                }
                let run = ro
                    .id(p::TEXT_MODIFIER_RANGE_RUN_ID)
                    .map(|id| runs.iter().position(|&x| x == id).unwrap_or(usize::MAX));
                let interpolator = self.children[r].iter().find_map(|&c| {
                    let o = self.object(c)?;
                    o.is(t::CUBIC_INTERPOLATOR_COMPONENT).then(|| {
                        [
                            p::CUBIC_INTERPOLATOR_COMPONENT_X1,
                            p::CUBIC_INTERPOLATOR_COMPONENT_Y1,
                            p::CUBIC_INTERPOLATOR_COMPONENT_X2,
                            p::CUBIC_INTERPOLATOR_COMPONENT_Y2,
                        ]
                        .map(|k| o.num(k))
                    })
                });
                ranges.push(modifiers::Range {
                    values: range,
                    run,
                    interpolator,
                });
            }
            groups.push(modifiers::Group { values, ranges });
        }
        groups
    }

    /// The glyphs of `laid` one by one, moved and faded by `groups` over
    /// time, in their styles' paints.
    fn modified_glyphs(
        &mut self,
        groups: &[modifiers::Group],
        laid: &text::Laid,
        style_objects: &[usize],
        opacity: &[Curve<f64>],
        transform: &[TransformOp],
    ) -> Result<Vec<Node>, Error> {
        let mut breaks = Vec::new();
        for g in groups {
            g.breaks(&mut breaks);
        }
        // Each glyph's place and opacity at a time, made once for all.
        let cache: RefCell<HashMap<u64, Rc<Placed>>> = RefCell::default();
        let at = |t: f64| {
            cache
                .borrow_mut()
                .entry(t.to_bits())
                .or_insert_with(|| {
                    Rc::new(
                        laid.glyphs
                            .iter()
                            .map(|g| modifiers::glyph_at(groups, laid, g, t))
                            .collect(),
                    )
                })
                .clone()
        };
        let mut out = Vec::new();
        for (k, glyph) in laid.glyphs.iter().enumerate() {
            let path = self.baker.derive(&Breaks::all(&breaks), |t| {
                glyph.outline.transformed(at(t)[k].0)
            });
            let shown = self.baker.derive(&Breaks::all(&breaks), |t| at(t)[k].1);
            if is_const(&shown, 0.0) {
                continue;
            }
            let paths = [(ShapePath::new(Geometry::Path { path }), Winding::default())];
            let painted = self.paints(style_objects[glyph.style], &paths, opacity, transform)?;
            out.push(Node::Group(Group {
                opacity: shown,
                children: painted.into_iter().map(|(n, _)| n).collect(),
                ..Group::new()
            }));
        }
        Ok(out)
    }

    /// String property `key` of object `i` over the timeline: indices into
    /// the strings it takes (its setup value first).
    fn string_curve(&mut self, i: usize, key: u16) -> (Curve<f64>, Vec<String>) {
        let setup = self.object(i).map(|o| o.string(key)).unwrap_or_default();
        let mut table = vec![setup];
        if let Some(list) = self.keyed.get(&(i, key)) {
            for (_, kp) in list {
                for k in &kp.keyframes {
                    let v = k.string(p::KEY_FRAME_STRING_VALUE);
                    if !table.contains(&v) {
                        table.push(v);
                    }
                }
            }
        }
        let lookup = table.clone();
        let curve = self.curve(i, key, 0.0, |k| {
            (k.ty == t::KEY_FRAME_STRING)
                .then(|| {
                    lookup
                        .iter()
                        .position(|s| *s == k.string(p::KEY_FRAME_STRING_VALUE))
                })
                .flatten()
                .map(|v| v as f64)
        });
        (curve, table)
    }

    /// Nested artboard `i`: the artboard it shows, converted on its own and
    /// placed at its transform (its origin there, as Rive draws nested
    /// artboards), playing its nested animations on the parent's clock in
    /// layers, the later overriding the earlier as Rive applies them: a
    /// simple animation from the start of each of the parent's animations
    /// while it plays, at its speed, repeating as its loop says; a state
    /// machine's entry animation so; a remap animation at the time keyed.
    /// Without one it shows its setup pose.
    fn nested(&mut self, i: usize) -> Result<Option<Node>, Error> {
        let Some(o) = self.object(i) else {
            return Ok(None);
        };
        let Some(index) = o
            .id(p::NESTED_ARTBOARD_ARTBOARD_ID)
            .filter(|a| *a < self.file.artboards.len())
        else {
            return Ok(None);
        };
        if self.depth + 1 >= self.limits.max_depth / 4 {
            self.warn("left out: artboards nested past the depth limit");
            return Ok(None);
        }
        let target = &self.file.artboards[index];
        let nested_speed = if o.flag(p::NESTED_ARTBOARD_IS_PAUSED) {
            0.0
        } else {
            o.num(p::NESTED_ARTBOARD_SPEED)
        };
        let animations: Vec<usize> = self.children[i]
            .iter()
            .copied()
            .filter(|&c| self.object(c).is_some_and(|o| o.is(t::NESTED_ANIMATION)))
            .collect();
        // What plays, before laying it out.
        let mut plays = Vec::new();
        let mut machine: Option<NestedMachine> = None;
        for c in animations {
            let Some(a) = self.object(c) else { continue };
            let id = a.uint(p::NESTED_ANIMATION_ANIMATION_ID) as usize;
            // A state machine plays as Rive plays it, its inputs as the
            // parent sets them (the first one; others their entry).
            if a.ty == t::NESTED_STATE_MACHINE
                && machine.is_none()
                && id < target.state_machines.len()
            {
                let mut inputs = Vec::new();
                for n in self.children[c].clone() {
                    let Some(no) = self.object(n) else { continue };
                    let key = match no.ty {
                        t::NESTED_BOOL => p::NESTED_BOOL_NESTED_VALUE,
                        t::NESTED_NUMBER => p::NESTED_NUMBER_NESTED_VALUE,
                        _ => continue,
                    };
                    let Some(input) = no.id(p::NESTED_INPUT_INPUT_ID) else {
                        continue;
                    };
                    inputs.push((input, self.num_curve(n, key)));
                }
                machine = Some(NestedMachine {
                    layer: plays.len(),
                    machine: id,
                    inputs,
                    sources: Vec::new(),
                    speed: nested_speed,
                    first_only: false,
                });
                continue;
            }
            let (animation, pieces) = match a.ty {
                t::NESTED_STATE_MACHINE => {
                    let Some(entry) = entry_animation(target, Some(id)) else {
                        continue;
                    };
                    (entry, Pieces::Rate(Curve::fixed(nested_speed)))
                }
                t::NESTED_SIMPLE_ANIMATION => {
                    let speed = a.num(p::NESTED_SIMPLE_ANIMATION_SPEED) * nested_speed;
                    let playing = self.num_curve(c, p::NESTED_SIMPLE_ANIMATION_IS_PLAYING);
                    let rate = map_curve(playing, |v| if v != 0.0 { speed } else { 0.0 });
                    (id, Pieces::Rate(rate))
                }
                t::NESTED_REMAP_ANIMATION => (
                    id,
                    Pieces::Remap(self.num_curve(c, p::NESTED_REMAP_ANIMATION_TIME)),
                ),
                _ => continue,
            };
            let Some(spec) = target
                .animations
                .get(animation)
                .map(|a| Spec::of(&a.object))
            else {
                continue;
            };
            plays.push((animation, spec, pieces));
        }
        // Played alike in the scene's spans (no rate or time the parent
        // keys, a state machine's inputs held at the same values), the
        // nested artboard's keys cover the longest of those spans once and
        // its time restarts there at each one's start; a span where the
        // inputs move has keys of its own. Otherwise they are laid out over
        // every span.
        // What a player changes in it: the changes whose path starts at it.
        let name = o.string(p::COMPONENT_NAME);
        let own = format!("#{i}");
        let inside = |path: &str| -> Option<String> {
            let (first, rest) = path.split_once('/').unwrap_or((path, ""));
            (first == own || (!name.is_empty() && first == name)).then(|| rest.to_owned())
        };
        let options = crate::Options {
            artboard: None,
            unbound: true,
            inputs: self
                .changes
                .iter()
                .filter_map(|c| {
                    Some(crate::InputChange {
                        path: inside(&c.path)?,
                        ..c.clone()
                    })
                })
                .collect(),
            aligned: self
                .aligned
                .iter()
                .filter_map(|a| {
                    Some(crate::Aligned {
                        path: inside(&a.path)?,
                        ..a.clone()
                    })
                })
                .collect(),
        };
        let changed = !options.inputs.is_empty() || !options.aligned.is_empty();
        let alike = !changed
            && !self.spans.is_empty()
            && plays
                .iter()
                .all(|(_, _, how)| matches!(how, Pieces::Rate(r) if r.keys.is_empty()));
        // Each window of the nested artboard's own time: its length, the
        // parent's time its inputs are read from, and what they hold.
        let mut windows: Vec<(f64, Option<f64>, Option<Vec<u64>>)> = Vec::new();
        let mut window_of = Vec::new();
        if alike {
            for &(start, d) in &self.spans {
                let held: Option<Vec<u64>> = match &machine {
                    None => Some(Vec::new()),
                    Some(m) => m
                        .inputs
                        .iter()
                        .map(|(_, c)| held_over(c, start, start + d).map(f64::to_bits))
                        .collect(),
                };
                let found = held
                    .as_ref()
                    .and_then(|h| windows.iter().position(|w| w.2.as_ref() == Some(h)));
                let w = match found {
                    Some(w) => {
                        windows[w].0 = windows[w].0.max(d);
                        w
                    }
                    None => {
                        windows.push((d, Some(start), held));
                        windows.len() - 1
                    }
                };
                window_of.push(w);
            }
            // Inputs set differently in many spans: they follow the parent
            // in its state machine (the first span), and hold their own
            // values in the parent's animations played alone.
            if windows.len() > 4 {
                let first = self.spans[0].1;
                let rest = self.spans[1..].iter().map(|s| s.1).fold(0.0, f64::max);
                windows = vec![(first, Some(self.spans[0].0), None)];
                if self.spans.len() > 1 {
                    windows.push((rest, None, None));
                }
                window_of = (0..self.spans.len()).map(|k| k.min(1)).collect();
                self.warn(
                    "nested state machines whose inputs the parent sets in many animations: they follow it in its state machine, and hold their own values in its animations",
                );
            }
        }
        let mut local = Vec::with_capacity(windows.len());
        let mut at = 0.0;
        for (d, _, _) in &windows {
            local.push((at, *d));
            at += d + GAP;
        }
        let (spans, sources): (Vec<(f64, f64)>, Vec<Option<f64>>) = if alike {
            (local.clone(), windows.iter().map(|w| w.1).collect())
        } else {
            (
                self.spans.clone(),
                self.spans.iter().map(|s| Some(s.0)).collect(),
            )
        };
        // Laid out over many spans, a state machine plays in the parent's
        // state machine alone (the first span).
        if let Some(m) = &mut machine
            && !alike
            && self.spans.len() > 4
        {
            m.first_only = true;
            self.warn(
                "nested state machines in artboards laid out over many spans: they play in the parent's state machine, and hold their setup in its animations",
            );
        }
        if let Some(m) = &mut machine {
            m.sources = sources;
        }
        let mut layers: Vec<Vec<Segment>> = plays
            .iter()
            .map(|(animation, spec, how)| self.pieces(*animation, spec, how, &spans))
            .collect();
        // The machine's layer names the animations its states play, so that
        // what they key is found in it; what it plays is its own.
        if let Some(m) = &machine {
            let start = spans.first().map_or(0.0, |s| s.0);
            let mut named: Vec<usize> = target.state_machines[m.machine]
                .layers
                .iter()
                .flat_map(|l| &l.states)
                .filter_map(|st| {
                    let o = st.object.as_ref().filter(|o| o.ty == t::ANIMATION_STATE)?;
                    o.id(p::ANIMATION_STATE_ANIMATION_ID)
                })
                .filter(|a| *a < target.animations.len())
                .collect();
            named.sort_unstable();
            named.dedup();
            let placeholder = named
                .into_iter()
                .map(|animation| Segment {
                    animation,
                    name: String::new(),
                    start,
                    duration: 0.0,
                    from: 0.0,
                    to: 0.0,
                    speed: 1.0,
                    fps: 60.0,
                    loop_mode: LoopMode::Once,
                })
                .collect();
            layers.insert(m.layer.min(layers.len()), placeholder);
        }
        let restart = alike.then(|| {
            let keys = self
                .spans
                .iter()
                .zip(&window_of)
                .flat_map(|((start, d), &w)| {
                    let from = local[w].0;
                    [
                        Keyframe::new(*start, from, Easing::Linear),
                        Keyframe::new(start + d, from + d, Easing::Hold),
                    ]
                })
                .collect();
            Animated::Track(Track::new(keys))
        });
        let clock = Clock::Layers {
            layers,
            spans,
            machine,
        };
        let (mut scene, warnings) = Self::convert(
            self.file,
            index,
            clock,
            &options,
            self.depth + 1,
            self.limits,
        )?;
        // Its ids under its own, so that each object is named once.
        scoped(&mut scene.root, &format!("{i}/"), 0);
        for w in warnings {
            self.warn(format!("nested artboard: {w}"));
        }
        let (w, h) = (
            target.object().num(p::LAYOUT_COMPONENT_WIDTH),
            target.object().num(p::LAYOUT_COMPONENT_HEIGHT),
        );
        let (ox, oy) = (
            target.object().num(p::ARTBOARD_ORIGIN_X),
            target.object().num(p::ARTBOARD_ORIGIN_Y),
        );
        let chain = self.chain(i)?;
        let curves = self.opacity_curves(&chain);
        let opacity = self.product(&curves);
        let transform = self.ops(&chain);
        let content = Node::Group(Group {
            transform: vec![TransformOp::translate(-w * ox, -h * oy)],
            time: restart.map(|remap| TimeMap {
                offset: 0.0,
                scale: 1.0,
                remap: Some(remap),
            }),
            children: vec![Node::Group(scene.root)],
            ..Group::new()
        });
        let mut node = self.group(i, transform, vec![content]);
        if let Node::Group(g) = &mut node {
            g.opacity = opacity;
        }
        Ok(Some(node))
    }

    /// Layers of the artboard's joysticks over what plays: each applies its
    /// x and y animations, after the others, at the fraction of their
    /// duration its x and y (−1 to 1) say: its own (keyed or not), or where
    /// its handle node is within its bounds, as Rive's `Joystick` applies
    /// them.
    fn add_joysticks(&mut self) -> Result<(), Error> {
        let joysticks: Vec<usize> = (1..self.ab.objects.len())
            .filter(|&j| self.object(j).is_some_and(|o| o.ty == t::JOYSTICK))
            .collect();
        for j in joysticks {
            let Some(o) = self.object(j) else { continue };
            let flags = o.uint(p::JOYSTICK_JOYSTICK_FLAGS);
            let handle = o.id(p::JOYSTICK_HANDLE_SOURCE_ID).filter(|h| {
                self.object(*h)
                    .is_some_and(|h| h.is(t::TRANSFORM_COMPONENT))
            });
            let axes = [
                (p::JOYSTICK_X_ID, p::JOYSTICK_X, flags & 1 != 0),
                (p::JOYSTICK_Y_ID, p::JOYSTICK_Y, flags & 2 != 0),
            ];
            for (axis, (id_key, value_key, invert)) in axes.into_iter().enumerate() {
                let Some(animation) = o.id(id_key).filter(|a| *a < self.ab.animations.len()) else {
                    continue;
                };
                let value = match handle {
                    Some(h) => self.handle_value(j, h, axis)?,
                    None => self.num_curve(j, value_key),
                };
                let fraction =
                    map_curve_straight(value, |v| ((if invert { -v } else { v }) + 1.0) / 2.0);
                // Rive applies the animation at the fraction of its
                // duration from 0, not from its work area's start.
                let mut spec = Spec::of(&self.ab.animations[animation].object);
                let length = spec.to - spec.from;
                spec.from = 0.0;
                spec.to = length;
                spec.speed = spec.speed.abs();
                let spans = self.spans.clone();
                let pieces = self.pieces(animation, &spec, &Pieces::Remap(fraction), &spans);
                self.layers.push(pieces);
            }
        }
        Ok(())
    }

    /// Joystick `j`'s value on `axis` (0 x, 1 y) where handle `h` is: its
    /// position in the joystick's bounds, −1 to 1.
    fn handle_value(&mut self, j: usize, h: usize, axis: usize) -> Result<Curve<f64>, Error> {
        let Some(o) = self.object(j) else {
            return Ok(Curve::fixed(0.0));
        };
        let (w, hh) = (o.num(p::JOYSTICK_WIDTH), o.num(p::JOYSTICK_HEIGHT));
        let (ox, oy) = (o.num(p::JOYSTICK_ORIGIN_X), o.num(p::JOYSTICK_ORIGIN_Y));
        let (px, py) = (o.num(p::JOYSTICK_POS_X), o.num(p::JOYSTICK_POS_Y));
        let handle_chain = self.chain(h)?;
        let handle_ops = self.ops(&handle_chain);
        let parent_ops = match self.parent[j].filter(|&pi| pi != 0) {
            Some(pi) => {
                let chain = self.chain(pi)?;
                self.ops(&chain)
            }
            None => Vec::new(),
        };
        let mut breaks = Vec::new();
        ops_breaks(&handle_ops, &mut breaks);
        ops_breaks(&parent_ops, &mut breaks);
        let at = |t: f64| {
            let world = transform_at(&parent_ops, t) * Matrix::translate(px, py);
            let pos = transform_at(&handle_ops, t).apply(Point::ZERO);
            let local = world.invert().map_or(pos, |m| m.apply(pos));
            if axis == 0 {
                if w == 0.0 {
                    0.0
                } else {
                    (local.x + w * ox) * 2.0 / w - 1.0
                }
            } else if hh == 0.0 {
                -1.0
            } else {
                (local.y + hh * oy) * 2.0 / hh - 1.0
            }
        };
        let value = self.baker.derive(&Breaks::all(&breaks), at);
        Ok(curve_of(&value))
    }

    /// The longest span the artboard's nested artboards play their
    /// animations over (at their speed), and whether one of them repeats.
    fn nested_span(&self) -> Option<(f64, LoopMode)> {
        let mut longest: Option<(f64, LoopMode)> = None;
        for (i, o) in self.ab.objects.iter().enumerate() {
            let Some(o) = o.as_ref().filter(|o| o.is(t::NESTED_ARTBOARD)) else {
                continue;
            };
            let Some(target) = o
                .id(p::NESTED_ARTBOARD_ARTBOARD_ID)
                .and_then(|a| self.file.artboards.get(a))
            else {
                continue;
            };
            for &c in &self.children[i] {
                let Some(a) = self.object(c).filter(|a| a.is(t::NESTED_ANIMATION)) else {
                    continue;
                };
                let id = a.uint(p::NESTED_ANIMATION_ANIMATION_ID) as usize;
                let (animation, speed) = match a.ty {
                    t::NESTED_STATE_MACHINE => (entry_animation(target, Some(id)), 1.0),
                    t::NESTED_SIMPLE_ANIMATION if a.flag(p::NESTED_SIMPLE_ANIMATION_IS_PLAYING) => {
                        (Some(id), a.num(p::NESTED_SIMPLE_ANIMATION_SPEED).abs())
                    }
                    _ => (None, 0.0),
                };
                let Some(spec) = animation
                    .and_then(|a| target.animations.get(a))
                    .map(|a| Spec::of(&a.object))
                else {
                    continue;
                };
                if speed <= 0.0 {
                    continue;
                }
                let d = spec.duration() / speed;
                let repeats = spec.loop_mode != LoopMode::Once;
                let mode = match longest {
                    Some((_, LoopMode::Loop)) => LoopMode::Loop,
                    _ if repeats => LoopMode::Loop,
                    _ => LoopMode::Once,
                };
                longest = Some((longest.map_or(d, |(l, _)| l.max(d)), mode));
            }
        }
        longest
    }

    /// The pieces of the scene's spans in which animation `animation`
    /// (`spec`) plays as `how` says: from the start of each span, at the
    /// rate (0 while it does not play), repeating as its loop says and held
    /// at its end when it plays once; or at the remapped time.
    fn pieces(
        &self,
        animation: usize,
        spec: &Spec,
        how: &Pieces,
        spans: &[(f64, f64)],
    ) -> Vec<Segment> {
        let piece = |start: f64, duration: f64, from: f64, to: f64, speed: f64| Segment {
            animation,
            name: spec.name.clone(),
            start,
            duration,
            from,
            to,
            speed,
            fps: spec.fps,
            loop_mode: spec.loop_mode,
        };
        let (lo, hi) = (spec.from, spec.to);
        let length = hi - lo;
        let budget = self.limits.max_keyframes / 64;
        let mut out = Vec::new();
        match how {
            Pieces::Remap(time) => {
                // The animation's seconds a fraction `f` of the way through
                // it.
                let at = |f: f64| {
                    let f = f.clamp(0.0, 1.0);
                    if spec.speed > 0.0 {
                        lo + length * f
                    } else {
                        hi - length * f
                    }
                };
                let mut times: Vec<f64> = time.keys.iter().map(|k| k.t).collect();
                times.insert(0, 0.0);
                // To the end of what plays: the last keyframe's value holds.
                times.push(spans.iter().map(|(s, d)| s + d).fold(1.0, f64::max));
                times.sort_by(f64::total_cmp);
                times.dedup();
                for w in times.windows(2) {
                    let (t0, t1) = (w[0], w[1]);
                    // Straight between remapped keyframes, frames between
                    // others.
                    let straight = time
                        .keys
                        .iter()
                        .rfind(|k| k.t <= t0)
                        .is_none_or(|k| matches!(k.interp, Interp::Linear | Interp::Hold));
                    let n = if straight {
                        1
                    } else {
                        (((t1 - t0) * 60.0).ceil() as usize).max(1)
                    };
                    for j in 0..n {
                        let (a, b) = (
                            t0 + (t1 - t0) * j as f64 / n as f64,
                            t0 + (t1 - t0) * (j + 1) as f64 / n as f64,
                        );
                        let (sa, sb) = (at(time.at(a)), at(time.at(b - 1e-9)));
                        let speed = (sb - sa) / (b - a);
                        out.push(if speed >= 0.0 {
                            piece(a, b - a, sa, sb.max(sa), speed)
                        } else {
                            piece(a, b - a, sb, sa, speed)
                        });
                    }
                }
            }
            Pieces::Rate(rate) => {
                let spans: Vec<(f64, f64)> = if spans.is_empty() {
                    vec![(0.0, spec.duration() / rate.at(0.0).abs().max(1e-9))]
                } else {
                    spans.to_vec()
                };
                let changes: Vec<f64> = rate.keys.iter().map(|k| k.t).collect();
                // The direction it plays its own seconds in.
                let sign = spec.speed.signum();
                for (start, span) in spans {
                    let end = start + span;
                    // Where it is in its own seconds, and which way it runs.
                    let (mut t, mut s, mut forward) =
                        (start, if sign > 0.0 { lo } else { hi }, sign > 0.0);
                    while t < end - 1e-9 && out.len() < budget {
                        let change = changes
                            .iter()
                            .copied()
                            .find(|c| *c > t + 1e-9)
                            .unwrap_or(end)
                            .min(end);
                        let r = rate.at(t).abs() * spec.speed.abs();
                        let ended = spec.loop_mode == LoopMode::Once
                            && if forward {
                                s >= hi - 1e-9
                            } else {
                                s <= lo + 1e-9
                            };
                        if r == 0.0 || ended || length <= 0.0 {
                            out.push(piece(t, change - t, s, s, 0.0));
                            t = change;
                            continue;
                        }
                        // To the end of the run, or the change of rate.
                        let left = if forward { hi - s } else { s - lo };
                        let next = (t + left / r).min(change);
                        let moved = (next - t) * r;
                        let reached = if forward { s + moved } else { s - moved };
                        out.push(if forward {
                            piece(t, next - t, s, reached, r)
                        } else {
                            piece(t, next - t, reached, s, -r)
                        });
                        t = next;
                        s = reached;
                        let at_end = if forward {
                            s >= hi - 1e-9
                        } else {
                            s <= lo + 1e-9
                        };
                        if at_end {
                            match spec.loop_mode {
                                LoopMode::Loop => s = if forward { lo } else { hi },
                                LoopMode::PingPong => forward = !forward,
                                LoopMode::Once => {}
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Image `i`, drawn about its origin.
    fn image(&mut self, i: usize) -> Result<Option<Node>, Error> {
        let Some(o) = self.object(i) else {
            return Ok(None);
        };
        let Some(asset) = o
            .id(p::IMAGE_ASSET_ID)
            .and_then(|a| self.file.assets.get(a))
        else {
            return Ok(None);
        };
        let Some(bytes) = &asset.contents else {
            self.warn("left out: images the file does not embed (loaded by the app or a CDN)");
            return Ok(None);
        };
        let Some((mime, size)) = image_info(bytes) else {
            self.warn("left out: images of a format this does not read");
            return Ok(None);
        };
        let (aw, ah) = (
            asset.object.num(p::DRAWABLE_ASSET_WIDTH),
            asset.object.num(p::DRAWABLE_ASSET_HEIGHT),
        );
        let (w, h) = if aw > 0.0 && ah > 0.0 { (aw, ah) } else { size };
        let (ox, oy) = (o.num(p::IMAGE_ORIGIN_X), o.num(p::IMAGE_ORIGIN_Y));
        let chain = self.chain(i)?;
        let curves = self.opacity_curves(&chain);
        let opacity = self.product(&curves);
        let mut transform = self.ops(&chain);
        let data = Arc::new(ImageData {
            mime: mime.to_owned(),
            bytes: bytes.clone(),
        });
        let mesh = self.children[i]
            .iter()
            .copied()
            .find(|&c| self.object(c).is_some_and(|o| o.ty == t::MESH));
        let children = match mesh {
            Some(m) => {
                let (triangles, skinned) = self.mesh(m, data, (w, h))?;
                // A skinned mesh is deformed into the world.
                if skinned {
                    transform.clear();
                }
                triangles
            }
            None => vec![Node::Image(Image {
                id: Some(i.to_string()),
                name: None,
                data,
                rect: Rect {
                    x: -w * ox,
                    y: -h * oy,
                    width: w,
                    height: h,
                },
            })],
        };
        let mut node = self.group(i, transform, children);
        if let Node::Group(g) = &mut node {
            g.opacity = opacity;
            g.blend = blend_mode(o.uint(p::DRAWABLE_BLEND_MODE_VALUE));
        }
        Ok(Some(node))
    }

    /// Image mesh `m` (the image `w` × `h` pixels): a group per triangle
    /// moving the image's triangle onto the mesh's, clipped to it, in the
    /// image's space (in the world when skinned, as the second says).
    fn mesh(
        &mut self,
        m: usize,
        data: Arc<ImageData>,
        (w, h): (f64, f64),
    ) -> Result<(Vec<Node>, bool), Error> {
        let Some(mo) = self.object(m) else {
            return Ok((Vec::new(), false));
        };
        let indices = varuints(mo.bytes(p::MESH_TRIANGLE_INDEX_BYTES));
        let vertices: Vec<usize> = self.children[m]
            .iter()
            .copied()
            .filter(|&v| self.object(v).is_some_and(|o| o.is(t::MESH_VERTEX)))
            .collect();
        // Rive draws no mesh with an index past its vertices.
        if indices.iter().any(|&k| k as usize >= vertices.len()) {
            self.warn("left out: image meshes whose triangles name missing vertices");
            return Ok((Vec::new(), false));
        }
        if indices.len() / 3 > self.limits.max_nodes {
            return Err(Error::Limit {
                what: "mesh triangles",
                limit: self.limits.max_nodes as f64,
            });
        }
        let mut inputs = Inputs::default();
        for &v in &vertices {
            inputs.add(self, v, p::VERTEX_X);
            inputs.add(self, v, p::VERTEX_Y);
        }
        let uv: Vec<Point> = vertices
            .iter()
            .filter_map(|&v| self.object(v))
            .map(|o| Point::new(o.num(p::MESH_VERTEX_U) * w, o.num(p::MESH_VERTEX_V) * h))
            .collect();
        let skin = self.skin(m)?;
        let weights: Vec<(u32, u32)> = vertices.iter().map(|&v| self.weights(v)[0]).collect();
        let mut breaks = inputs.breaks();
        if let Some(skin) = &skin {
            skin.breaks(&mut breaks);
        }
        let at = |v: usize, t: f64, bones: &[Matrix], skin: Option<&Skin>| {
            let o = vertices[v];
            let p = Point::new(inputs.at(o, p::VERTEX_X, t), inputs.at(o, p::VERTEX_Y, t));
            match skin {
                Some(s) => deform(p, weights[v].0, weights[v].1, s.world, bones),
                None => p,
            }
        };
        let mut nodes = Vec::new();
        for tri in indices.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(|k| k as usize);
            let Some(from) = triangle(uv[a], uv[b], uv[c]) else {
                continue;
            };
            let Some(from_inverse) = from.invert() else {
                continue;
            };
            let matrix = self.baker.derive(&Breaks::all(&breaks), |t| {
                let bones = skin.as_ref().map(|s| s.bones(t)).unwrap_or_default();
                let to = |v| at(v, t, &bones, skin.as_ref());
                triangle(to(a), to(b), to(c)).unwrap_or(Matrix::scale(0.0, 0.0)) * from_inverse
            });
            nodes.push(Node::Group(Group {
                transform: vec![TransformOp::Matrix { matrix }],
                clip: Some(Clip {
                    shapes: vec![ClipShape {
                        geometry: Geometry::path(dilated([uv[a], uv[b], uv[c]])),
                        transform: Vec::new(),
                        rule: FillRule::NonZero,
                    }],
                    one_path: false,
                }),
                children: vec![Node::Image(Image {
                    id: None,
                    name: None,
                    data: data.clone(),
                    rect: Rect::new(0.0, 0.0, w, h),
                })],
                ..Group::default()
            }));
        }
        Ok((nodes, skin.is_some()))
    }

    /// Warns of what the artboard holds that the scene leaves out.
    fn left_out(&mut self) {
        let mut later = BTreeSet::new();
        for o in self.ab.objects.iter().skip(1).flatten() {
            for (ty, what) in LATER {
                if o.is(*ty) {
                    later.insert(*what);
                }
            }
        }
        for what in later {
            self.warn(format!("left out: {what}"));
        }
        // A nested artboard's state machine reports its events to its
        // parent's listeners, not to the app: those its listeners fire
        // reach them (`Interaction`), those it fires as it plays do not.
        if self.depth > 0 && self.ab.objects.iter().flatten().any(|o| o.is(t::EVENT)) {
            self.warn("left out: events a nested artboard's state machine fires as it plays (those its listeners fire reach its parent's listeners)");
        }
        let machines = self.ab.state_machines.len();
        if self.plan.is_some() {
            self.warn(
                "state machines: the default one plays first, its inputs as the options change them (its listeners act through a player's pointer); each animation is a marker",
            );
            if machines > 1 {
                self.warn(format!("left out: {} other state machine(s)", machines - 1));
            }
        } else if self.machine_layer.is_some() {
            if machines > 1 {
                self.warn(format!("left out: {} other state machine(s)", machines - 1));
            }
        } else if machines > 0 {
            self.warn(format!(
                "left out: {machines} state machine(s); the animation its entry plays plays, and each animation is a marker"
            ));
        }
        // Properties of kinds the scene draws are left out only with their
        // objects (warned of above).
        let drawn: HashSet<u16> = self.used.iter().map(|(_, key)| *key).collect();
        let mut unused: BTreeMap<&str, usize> = BTreeMap::new();
        for key in self.keyed.keys() {
            // What an event's keys fire is an event (warned of above).
            if self.object(key.0).is_some_and(|o| o.is(t::EVENT)) {
                continue;
            }
            if !self.used.contains(key) && !drawn.contains(&key.1) && !DRAWN.contains(&key.1) {
                *unused
                    .entry(defs::prop_name(key.1).unwrap_or("properties newer than the runtime"))
                    .or_default() += 1;
            }
        }
        for (name, n) in unused {
            self.warn(format!("left out: {n} animated {name}"));
        }
        if self.baker.over {
            self.warn("past the sampling budget: some values go straight between keyframes");
        }
        // The editor's own objects (a mesh's forced edges) the runtime skips
        // too.
        let unknown: usize = self
            .file
            .unknown
            .iter()
            .filter(|(ty, _)| !u16::try_from(**ty).is_ok_and(defs::is_editor_only))
            .map(|(_, n)| n)
            .sum();
        if unknown > 0 {
            self.warn(format!(
                "left out: {unknown} object(s) of types newer than rive-runtime {} (format {}.{})",
                defs::RUNTIME_TAG,
                self.file.major,
                self.file.minor
            ));
        }
    }
}

/// The bounds of `paths` (in their shape's space) over the times their
/// outlines and transforms have keyframes.
fn paths_bounds(paths: &[ShapePath]) -> Option<Rect> {
    let mut breaks_list = Vec::new();
    for path in paths {
        geometry_breaks(&path.geometry, &mut breaks_list);
        ops_breaks(&path.transform, &mut breaks_list);
    }
    let mut times: Vec<f64> = breaks_list
        .iter()
        .flat_map(|b| b.0.iter().map(|k| k.t))
        .collect();
    times.push(0.0);
    times.sort_by(f64::total_cmp);
    times.dedup();
    let mut bounds: Option<Rect> = None;
    for t in times.iter().take(4096) {
        for path in paths {
            let m = transform_at(&path.transform, *t);
            if let Some(b) = geometry_path(&path.geometry, *t).transformed(m).bounds() {
                bounds = Some(bounds.map_or(b, |a| a.union(b)));
            }
        }
    }
    bounds
}

/// How far the translations first in `paths`' transforms reach (an inner
/// feather's offset).
fn offset_reach(paths: &[ShapePath]) -> f64 {
    let mut reach: f64 = 0.0;
    for path in paths {
        if let Some(TransformOp::Translate { x, y }) = path.transform.first() {
            for a in [x, y] {
                for v in a
                    .keyframes()
                    .iter()
                    .map(|k| k.value)
                    .chain(a.as_static().copied())
                {
                    reach = reach.max(v.abs());
                }
            }
        }
    }
    reach
}

/// The keyframes of an animated value, as an input to a computed one.
fn breaks<T: PartialEq>(a: &Animated<T>, out: &mut Vec<Breaks>) {
    let k = a.keyframes();
    if k.is_empty() {
        return;
    }
    out.push(Breaks(
        k.iter()
            .enumerate()
            .map(|(i, key)| Break {
                t: key.time,
                easing: Some(key.easing.clone()),
                moves: k.get(i + 1).is_some_and(|next| next.value != key.value),
            })
            .collect(),
    ));
}

fn point_breaks(p: &AnimatedPoint, out: &mut Vec<Breaks>) {
    breaks(&p.x, out);
    breaks(&p.y, out);
}

/// The keyframes of what moves `geometry`.
fn geometry_breaks(geometry: &Geometry, out: &mut Vec<Breaks>) {
    match geometry {
        Geometry::Path { path } => breaks(path, out),
        Geometry::Rect {
            center,
            size,
            radius,
            ..
        } => {
            point_breaks(center, out);
            point_breaks(size, out);
            point_breaks(radius, out);
        }
        Geometry::Ellipse { center, radius, .. } => {
            point_breaks(center, out);
            point_breaks(radius, out);
        }
        Geometry::Star(star) => {
            point_breaks(&star.center, out);
            for a in [
                &star.points,
                &star.outer_radius,
                &star.inner_radius,
                &star.outer_roundness,
                &star.inner_roundness,
                &star.rotation,
            ] {
                breaks(a, out);
            }
        }
    }
}

/// The keyframes of what moves `ops`.
fn ops_breaks(ops: &[TransformOp], out: &mut Vec<Breaks>) {
    for op in ops {
        match op {
            TransformOp::Translate { x, y }
            | TransformOp::Scale { x, y }
            | TransformOp::Skew { x, y } => {
                breaks(x, out);
                breaks(y, out);
            }
            TransformOp::Rotate { degrees } => breaks(degrees, out),
            TransformOp::Matrix { matrix } => breaks(matrix, out),
            TransformOp::Motion(m) => breaks(&m.progress, out),
        }
    }
}

/// The keys of `anim` (in the animation's own seconds) as segment `seg`
/// plays them, in the timeline's seconds: keyframes inside its span as they
/// are (a cubic easing turned round when it plays backwards), its ends at
/// their values, and the frames of what a cut leaves of an easing that is
/// not straight or held.
/// `curve` holding `value` from `t` to its next key, as it was before.
fn hold_from(curve: &mut Curve<f64>, t: f64, value: f64) {
    let keys = &curve.keys;
    let i = keys.partition_point(|k| k.t <= t);
    let mut out: Vec<Key<f64>> = keys[..i].to_vec();
    match out.last() {
        None if t > 0.0 => out.push(Key {
            t: 0.0,
            value: curve.at(0.0),
            interp: Interp::Hold,
        }),
        Some(k) if k.t < t && k.interp != Interp::Hold => out.push(Key {
            t,
            value: curve.at(t),
            interp: Interp::Hold,
        }),
        _ => {}
    }
    out.push(Key {
        t,
        value,
        interp: Interp::Hold,
    });
    out.extend_from_slice(&keys[i..]);
    curve.keys = out;
}

fn piece_keys<T: Keyed>(seg: &Segment, anim: &Curve<T>, out: &mut Vec<Key<T>>) {
    let (from, to) = (seg.from.min(seg.to), seg.from.max(seg.to));
    if seg.speed == 0.0 || to - from <= 0.0 {
        out.push(Key {
            t: seg.start,
            value: anim.at(seg.from),
            interp: Interp::Hold,
        });
        return;
    }
    let forward = seg.speed > 0.0;
    // The animation's seconds where something changes, in playing order.
    let mut points: Vec<f64> = vec![from];
    points.extend(
        anim.keys
            .iter()
            .map(|k| k.t)
            .filter(|t| *t > from && *t < to),
    );
    points.push(to);
    points.dedup();
    if !forward {
        points.reverse();
    }
    let k = &anim.keys;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (lo, hi) = (a.min(b), a.max(b));
        // The keyframe whose segment holds (lo, hi).
        let i = k.partition_point(|key| key.t <= lo);
        let interp = match (i.checked_sub(1).and_then(|j| k.get(j)), k.get(i)) {
            (Some(start), Some(end)) => {
                let whole = start.t == lo && end.t == hi;
                match start.interp {
                    Interp::Hold | Interp::Linear => Some(start.interp),
                    Interp::Cubic(_) | Interp::Value(_) | Interp::Elastic { .. }
                        if whole && forward =>
                    {
                        Some(start.interp)
                    }
                    Interp::Cubic(c) if whole => Some(Interp::Cubic(turned(c))),
                    // A cut of a cubic easing is a cubic easing too.
                    Interp::Cubic(_) | Interp::Value(_) => {
                        let full = match start.value.easing(&end.value, start.interp) {
                            Some(Easing::CubicBezier { x1, y1, x2, y2 }) => Some([x1, y1, x2, y2]),
                            _ => None,
                        };
                        let span = end.t - start.t;
                        full.and_then(|c| {
                            sub_easing(c, (lo - start.t) / span, (hi - start.t) / span)
                        })
                        .map(|c| Interp::Cubic(if forward { c } else { turned(c) }))
                    }
                    _ => None,
                }
            }
            // Before the first keyframe or after the last: still.
            _ => Some(Interp::Hold),
        };
        match interp {
            // A hold played backwards shows the earlier value all through.
            Some(Interp::Hold) if !forward => out.push(Key {
                t: seg.time(a),
                value: anim.at(lo),
                interp: Interp::Hold,
            }),
            Some(interp) => out.push(Key {
                t: seg.time(a),
                value: anim.at(a),
                interp,
            }),
            None => {
                let n = (((hi - lo) * seg.fps).ceil() as usize).max(1);
                for j in 0..n {
                    let s = a + (b - a) * j as f64 / n as f64;
                    out.push(Key {
                        t: seg.time(s),
                        value: anim.at(s),
                        interp: Interp::Linear,
                    });
                }
            }
        }
    }
    let last = *points.last().unwrap_or(&to);
    out.push(Key {
        t: seg.time(last),
        value: anim.at(last),
        interp: Interp::Hold,
    });
}

/// How interpolator `i` eases.
pub(crate) fn interpolator(i: &Object) -> Interp {
    let c = || {
        [
            i.num(p::CUBIC_INTERPOLATOR_X1),
            i.num(p::CUBIC_INTERPOLATOR_Y1),
            i.num(p::CUBIC_INTERPOLATOR_X2),
            i.num(p::CUBIC_INTERPOLATOR_Y2),
        ]
    };
    match i.ty {
        t::CUBIC_EASE_INTERPOLATOR => Interp::Cubic(c()),
        t::CUBIC_VALUE_INTERPOLATOR => Interp::Value(c()),
        t::ELASTIC_INTERPOLATOR => Interp::Elastic {
            easing: i.uint(p::ELASTIC_INTERPOLATOR_EASING_VALUE),
            amplitude: i.num(p::ELASTIC_INTERPOLATOR_AMPLITUDE),
            period: i.num(p::ELASTIC_INTERPOLATOR_PERIOD),
        },
        _ => Interp::Linear,
    }
}

/// `group` and what it holds with their ids under `prefix`.
fn scoped(group: &mut Group, prefix: &str, depth: usize) {
    if depth > 256 {
        return;
    }
    if let Some(id) = &mut group.id {
        *id = format!("{prefix}{id}");
    }
    for child in &mut group.children {
        if let Node::Group(g) = child {
            scoped(g, prefix, depth + 1);
        }
    }
}

/// Where the layers `played` (of animations `anims`) settle: the frame
/// after which they stop changing states and mixing (a layer holding still,
/// after its last frame that moved), the lengths they repeat over from
/// there, and whether they settle before their end at all.
fn settle(played: &[Vec<machine::Applied>], anims: &[machine::Anim]) -> (usize, Vec<f64>, bool) {
    let mut settled = 0;
    let mut steady = true;
    let mut periods = Vec::new();
    for l in played {
        let changed = |what: &dyn Fn(usize) -> bool| {
            (1..l.len()).rev().find(|&k| what(k)).map_or(0, |k| k + 1)
        };
        let last_change = changed(&|k| l[k].serial != l[k - 1].serial || l[k].mix < 1.0);
        if last_change + 120 >= l.len() {
            steady = false;
        }
        let period = |a: usize, rate: f64| {
            let an = &anims[a];
            let d = (an.end - an.start).abs() / (an.speed * rate).abs();
            match an.loop_mode {
                LoopMode::Loop if d > 0.0 => Some(d),
                LoopMode::PingPong if d > 0.0 => Some(2.0 * d),
                _ => None,
            }
        };
        let looping: Option<Vec<f64>> = l.last().and_then(|ap| match &ap.blend {
            // A blend repeats over what its animations that loop do.
            Some(b) => {
                let all: Vec<f64> = b
                    .parts
                    .iter()
                    .flatten()
                    .filter_map(|b| period(b.anim, 1.0))
                    .collect();
                (!all.is_empty()).then_some(all)
            }
            None => period(ap.current?.0, ap.direction).map(|p| vec![p]),
        });
        match looping {
            Some(all) => {
                periods.extend(all);
                settled = settled.max(last_change);
            }
            // Still from the last frame that moved.
            None => settled = settled.max(changed(&|k| l[k] != l[k - 1]).saturating_sub(1)),
        }
    }
    (settled, periods, steady)
}

/// A comparand of a view model condition as the instance bound holds it.
#[derive(Debug, Clone, PartialEq)]
enum Comparand {
    Number(f64),
    Bool(bool),
    Id(u64),
    Text(String),
    Color(u32),
}

/// Whether a view model condition (`TransitionViewModelCondition`)
/// comparing `left` with `right` by operation `op` holds for the values
/// bound (`bindables`, the state machine's bindable properties): held still,
/// a comparison of a trigger or of a value with itself changing never does.
fn view_model_holds(
    bindables: &[Object],
    op: u64,
    left: &Object,
    right: &Object,
    unread: &mut BTreeSet<&'static str>,
) -> bool {
    let comparand = |c: &Object| -> Option<Comparand> {
        Some(match c.ty {
            t::TRANSITION_PROPERTY_VIEW_MODEL_COMPARATOR => {
                let b = bindables.get(c.uint(crate::file::BINDABLE_INDEX) as usize)?;
                match b.ty {
                    t::BINDABLE_PROPERTY_NUMBER => {
                        Comparand::Number(b.num(p::BINDABLE_PROPERTY_NUMBER_PROPERTY_VALUE))
                    }
                    t::BINDABLE_PROPERTY_BOOLEAN => {
                        Comparand::Bool(b.flag(p::BINDABLE_PROPERTY_BOOLEAN_PROPERTY_VALUE))
                    }
                    t::BINDABLE_PROPERTY_ENUM => {
                        Comparand::Id(b.uint(p::BINDABLE_PROPERTY_ENUM_PROPERTY_VALUE))
                    }
                    t::BINDABLE_PROPERTY_STRING => {
                        Comparand::Text(b.string(p::BINDABLE_PROPERTY_STRING_PROPERTY_VALUE))
                    }
                    t::BINDABLE_PROPERTY_COLOR => {
                        Comparand::Color(b.color(p::BINDABLE_PROPERTY_COLOR_PROPERTY_VALUE))
                    }
                    _ => return None,
                }
            }
            t::TRANSITION_VALUE_NUMBER_COMPARATOR => {
                Comparand::Number(c.num(p::TRANSITION_VALUE_NUMBER_COMPARATOR_VALUE))
            }
            t::TRANSITION_VALUE_BOOLEAN_COMPARATOR => {
                Comparand::Bool(c.flag(p::TRANSITION_VALUE_BOOLEAN_COMPARATOR_VALUE))
            }
            t::TRANSITION_VALUE_ENUM_COMPARATOR => {
                Comparand::Id(c.uint(p::TRANSITION_VALUE_ID_COMPARATOR_VALUE))
            }
            t::TRANSITION_VALUE_STRING_COMPARATOR => {
                Comparand::Text(c.string(p::TRANSITION_VALUE_STRING_COMPARATOR_VALUE))
            }
            t::TRANSITION_VALUE_COLOR_COMPARATOR => {
                Comparand::Color(c.color(p::TRANSITION_VALUE_COLOR_COMPARATOR_VALUE))
            }
            _ => return None,
        })
    };
    // A value changing (`TransitionSelfComparator`) or a trigger fired:
    // nothing changes the instance here.
    if right.ty == t::TRANSITION_SELF_COMPARATOR
        || right.ty == t::TRANSITION_VALUE_TRIGGER_COMPARATOR
    {
        return false;
    }
    let (Some(a), Some(b)) = (comparand(left), comparand(right)) else {
        for c in [left, right] {
            if comparand(c).is_none() {
                unread.extend(defs::type_name(c.ty));
            }
        }
        return false;
    };
    let order = match (&a, &b) {
        (Comparand::Number(x), Comparand::Number(y)) => x.partial_cmp(y),
        (x, y) if std::mem::discriminant(x) == std::mem::discriminant(y) => Some(if x == y {
            std::cmp::Ordering::Equal
        } else {
            std::cmp::Ordering::Less
        }),
        _ => None,
    };
    let Some(order) = order else {
        return false;
    };
    use std::cmp::Ordering::{Equal, Greater, Less};
    // Only numbers order; the others are equal or not.
    let ordered = matches!(a, Comparand::Number(_));
    match op {
        0 => order == Equal,
        1 => order != Equal,
        2 => ordered && order != Greater,
        3 => ordered && order != Less,
        4 => ordered && order == Less,
        5 => ordered && order == Greater,
        _ => false,
    }
}

/// The events `objects` (a state's or a transition's fire actions) fire.
fn fires(objects: &[Object]) -> Vec<machine::Fire> {
    objects
        .iter()
        .filter(|o| o.is(t::STATE_MACHINE_FIRE_EVENT))
        .filter_map(|o| {
            Some(machine::Fire {
                event: o.id(p::STATE_MACHINE_FIRE_EVENT_EVENT_ID)?,
                at_end: o.uint(p::STATE_MACHINE_FIRE_ACTION_OCCURS_VALUE) == 1,
            })
        })
        .collect()
}

/// The value `c` holds all through from `start` to `end`, if it does.
fn held_over(c: &Curve<f64>, start: f64, end: f64) -> Option<f64> {
    let v = c.at(start);
    let still = c
        .keys
        .iter()
        .filter(|k| k.t > start && k.t <= end)
        .all(|k| k.value == v)
        && c.at(end) == v;
    still.then_some(v)
}

/// The frames of `layers` where one enters a state, starts or stops mixing,
/// or mixes; the first and one past the last.
fn bounds(layers: &[Vec<machine::Applied>]) -> Vec<usize> {
    let frames = layers.first().map_or(0, Vec::len);
    let mut out = vec![0];
    for k in 1..frames {
        if layers.iter().any(|l| {
            l[k].from.is_some()
                || l[k].serial != l[k - 1].serial
                || l[k - 1].from.is_some()
                || l[k].from_blend.is_some()
                || match (&l[k].blend, &l[k - 1].blend) {
                    (Some(a), Some(b)) => !a.same_weights(b),
                    (a, b) => a.is_some() != b.is_some(),
                }
        }) {
            out.push(k);
        }
    }
    out.push(frames);
    out
}

/// The shortest length all of `periods` repeat over, within a minute; else
/// the longest of them (and `false`).
fn common_period(periods: &[f64]) -> (f64, bool) {
    let longest = periods.iter().copied().fold(0.0, f64::max);
    for m in 1..=3600 {
        let c = longest * f64::from(m);
        if c > 60.0 {
            break;
        }
        if periods.iter().all(|p| {
            let r = c / p;
            (r - r.round()).abs() < 1e-4
        }) {
            return (c, true);
        }
    }
    (longest, false)
}

/// Animation `index` played from its own second `t0` at `rate` (its seconds
/// a second, backwards when negative) from scene time `from` to `until`,
/// looping, turning or holding at its ends as its loop says: one piece for
/// each pass.
fn steady_pieces(
    anim: &machine::Anim,
    index: usize,
    fps: f64,
    t0: f64,
    rate: f64,
    from: f64,
    until: f64,
) -> Vec<Segment> {
    let piece = |start: f64, duration: f64, a: f64, b: f64, speed: f64| Segment {
        animation: index,
        name: String::new(),
        start,
        duration,
        from: a.min(b),
        to: a.max(b),
        speed,
        fps,
        loop_mode: LoopMode::Once,
    };
    let mut out = Vec::new();
    let (mut t, mut at, mut v) = (t0, from, rate);
    while at < until - 1e-9 && out.len() < 4096 {
        if v == 0.0 || anim.end <= anim.start {
            out.push(piece(at, until - at, t, t, 0.0));
            break;
        }
        let bound = if v > 0.0 { anim.end } else { anim.start };
        let next = (at + ((bound - t) / v).max(0.0)).min(until);
        let t_next = t + (next - at) * v;
        if next > at {
            out.push(piece(at, next - at, t, t_next, v));
        }
        at = next;
        t = t_next;
        if at >= until - 1e-9 {
            break;
        }
        match anim.loop_mode {
            LoopMode::Loop => t = if v > 0.0 { anim.start } else { anim.end },
            LoopMode::PingPong => v = -v,
            LoopMode::Once => {
                out.push(piece(at, until - at, bound, bound, 0.0));
                break;
            }
        }
    }
    out
}

/// Cubic easing `c` run backwards.
fn turned(c: [f64; 4]) -> [f64; 4] {
    [1.0 - c[2], 1.0 - c[3], 1.0 - c[0], 1.0 - c[1]]
}

/// The animation state machine `machine` of `ab` (its default one when
/// `None`) plays first.
fn entry_animation(ab: &Artboard, machine: Option<usize>) -> Option<usize> {
    let machines = &ab.state_machines;
    let sm = match machine {
        Some(k) => machines.get(k)?,
        None => ab
            .object()
            .id(p::ARTBOARD_DEFAULT_STATE_MACHINE_ID)
            .and_then(|i| machines.get(i))
            .or(machines.first())?,
    };
    let layer = sm.layers.first()?;
    let entry = layer
        .states
        .iter()
        .find(|s| s.object.as_ref().is_some_and(|o| o.ty == t::ENTRY_STATE))?;
    let to = entry
        .transitions
        .first()?
        .object
        .id(p::STATE_TRANSITION_STATE_TO_ID)?;
    let state = layer.states.get(to)?.object.as_ref()?;
    (state.ty == t::ANIMATION_STATE)
        .then(|| state.id(p::ANIMATION_STATE_ANIMATION_ID))
        .flatten()
        .filter(|i| *i < ab.animations.len())
}

/// What a linear animation plays: its name, the span of its own seconds
/// (its work area when it has one), its speed, frame rate and loop.
#[derive(Debug, Clone)]
struct Spec {
    name: String,
    from: f64,
    to: f64,
    speed: f64,
    fps: f64,
    loop_mode: LoopMode,
}

impl Spec {
    fn of(a: &Object) -> Spec {
        let fps = a.num(p::LINEAR_ANIMATION_FPS).max(1.0);
        let frames = a.num(p::LINEAR_ANIMATION_DURATION).max(0.0);
        let (from, to) = if a.flag(p::LINEAR_ANIMATION_ENABLE_WORK_AREA) {
            let s = a.num(p::LINEAR_ANIMATION_WORK_START).clamp(0.0, frames);
            let e = a.num(p::LINEAR_ANIMATION_WORK_END).clamp(s, frames.max(s));
            (s / fps, e / fps)
        } else {
            (0.0, frames / fps)
        };
        let speed = match a.num(p::LINEAR_ANIMATION_SPEED) {
            s if s == 0.0 || !s.is_finite() => 1.0,
            s => s,
        };
        let loop_mode = match a.uint(p::LINEAR_ANIMATION_LOOP_VALUE) {
            1 => LoopMode::Loop,
            2 => LoopMode::PingPong,
            _ => LoopMode::Once,
        };
        Spec {
            name: a.string(p::ANIMATION_NAME),
            from,
            to,
            speed,
            fps,
            loop_mode,
        }
    }

    /// Seconds one play-through lasts.
    fn duration(&self) -> f64 {
        (self.to - self.from) / self.speed.abs()
    }
}

/// Where a paint's shape goes, from its drawable's transform.
#[derive(Debug, Clone)]
enum Placement {
    /// Under it.
    Inside,
    /// Under it, moved first by a translation in the artboard's space (a
    /// feather's offset).
    Moved(TransformOp),
    /// Outside it: the outline moved into the artboard's space, the stroke
    /// not scaled; moved by a translation when there is one.
    World(Option<TransformOp>),
}

/// How a path is taken by the clockwise fill rule.
#[derive(Debug, Clone, Copy, Default)]
struct Winding {
    /// Turned round.
    backwards: bool,
    /// A hole (drawn counter-clockwise).
    hole: bool,
}

/// `path` running the other way.
fn reversed(path: &ShapePath) -> ShapePath {
    let geometry = match &path.geometry {
        Geometry::Path { path } => Geometry::Path {
            path: match path {
                Animated::Static(p) => Animated::Static(geometry::reverse(p)),
                Animated::Track(track) => {
                    let mut track = track.clone();
                    for k in &mut track.keyframes {
                        k.value = geometry::reverse(&k.value);
                    }
                    Animated::Track(track)
                }
            },
        },
        Geometry::Rect {
            center,
            size,
            radius,
            direction,
            start,
        } => Geometry::Rect {
            center: center.clone(),
            size: size.clone(),
            radius: radius.clone(),
            direction: flip(*direction),
            start: *start,
        },
        Geometry::Ellipse {
            center,
            radius,
            direction,
            start,
        } => Geometry::Ellipse {
            center: center.clone(),
            radius: radius.clone(),
            direction: flip(*direction),
            start: *start,
        },
        Geometry::Star(star) => {
            let mut star = star.clone();
            star.direction = flip(star.direction);
            Geometry::Star(star)
        }
    };
    ShapePath {
        geometry,
        transform: path.transform.clone(),
    }
}

fn flip(d: Direction) -> Direction {
    match d {
        Direction::Clockwise => Direction::CounterClockwise,
        Direction::CounterClockwise => Direction::Clockwise,
    }
}

/// The curves a computed value reads.
#[derive(Default)]
struct Inputs {
    curves: HashMap<(usize, u16), Curve<f64>>,
}

impl Inputs {
    fn add(&mut self, c: &mut Converter<'_>, i: usize, key: u16) {
        if let Entry::Vacant(e) = self.curves.entry((i, key)) {
            e.insert(c.num_curve(i, key));
        }
    }

    fn curve(&self, i: usize, key: u16) -> Curve<f64> {
        self.curves
            .get(&(i, key))
            .cloned()
            .unwrap_or_else(|| Curve::fixed(defs::initial(key).unwrap_or(0.0)))
    }

    fn at(&self, i: usize, key: u16, t: f64) -> f64 {
        match self.curves.get(&(i, key)) {
            Some(c) => c.at(t),
            None => defs::initial(key).unwrap_or(0.0),
        }
    }

    fn breaks(&self) -> Vec<Breaks> {
        self.curves.values().map(Curve::breaks).collect()
    }
}

/// What deforms a path's or a mesh's vertices with bones.
struct Skin {
    /// The skinned object's world transform when it was bound.
    world: Matrix,
    /// Each tendon's bone, as its world transform, and its inverse bind.
    tendons: Vec<(Vec<TransformOp>, Matrix)>,
}

impl Skin {
    /// The bones' transforms at `t`, the first (index 0) none.
    fn bones(&self, t: f64) -> Vec<Matrix> {
        std::iter::once(Matrix::IDENTITY)
            .chain(
                self.tendons
                    .iter()
                    .map(|(ops, inverse)| transform_at(ops, t) * *inverse),
            )
            .collect()
    }

    fn breaks(&self, out: &mut Vec<Breaks>) {
        for (ops, _) in &self.tendons {
            ops_breaks(ops, out);
        }
    }
}

/// Point `p` bound in `world`, moved by `bones` as the four byte-sized
/// `indices` and `values` weigh them (Rive's `Weight::deform`).
fn deform(p: Point, indices: u32, values: u32, world: Matrix, bones: &[Matrix]) -> Point {
    let mut m = [0.0; 6];
    for k in 0..4 {
        let value = (values >> (k * 8)) & 0xFF;
        let Some(b) = bones.get(((indices >> (k * 8)) & 0xFF) as usize) else {
            continue;
        };
        if value == 0 {
            continue;
        }
        let w = f64::from(value) / 255.0;
        for (sum, v) in m.iter_mut().zip([b.a, b.b, b.c, b.d, b.e, b.f]) {
            *sum += v * w;
        }
    }
    Matrix::new(m[0], m[1], m[2], m[3], m[4], m[5]).apply(world.apply(p))
}

/// Vertex `o` of type `ty` at `t`.
fn vertex(v: &Inputs, o: usize, ty: u16, t: f64) -> Vertex {
    let at = |key| v.at(o, key, t);
    let pt = Point::new(at(p::VERTEX_X), at(p::VERTEX_Y));
    let polar =
        |angle: f64, distance: f64| Point::new(angle.cos() * distance, angle.sin() * distance);
    let (r#in, out) = match ty {
        t::CUBIC_MIRRORED_VERTEX => {
            let d = polar(
                at(p::CUBIC_MIRRORED_VERTEX_ROTATION),
                at(p::CUBIC_MIRRORED_VERTEX_DISTANCE),
            );
            (
                Point::new(pt.x - d.x, pt.y - d.y),
                Point::new(pt.x + d.x, pt.y + d.y),
            )
        }
        t::CUBIC_ASYMMETRIC_VERTEX => {
            let r = at(p::CUBIC_ASYMMETRIC_VERTEX_ROTATION);
            let di = polar(r, at(p::CUBIC_ASYMMETRIC_VERTEX_IN_DISTANCE));
            let d_out = polar(r, at(p::CUBIC_ASYMMETRIC_VERTEX_OUT_DISTANCE));
            (
                Point::new(pt.x - di.x, pt.y - di.y),
                Point::new(pt.x + d_out.x, pt.y + d_out.y),
            )
        }
        t::CUBIC_DETACHED_VERTEX => {
            let di = polar(
                at(p::CUBIC_DETACHED_VERTEX_IN_ROTATION),
                at(p::CUBIC_DETACHED_VERTEX_IN_DISTANCE),
            );
            let d_out = polar(
                at(p::CUBIC_DETACHED_VERTEX_OUT_ROTATION),
                at(p::CUBIC_DETACHED_VERTEX_OUT_DISTANCE),
            );
            (
                Point::new(pt.x + di.x, pt.y + di.y),
                Point::new(pt.x + d_out.x, pt.y + d_out.y),
            )
        }
        _ => {
            return Vertex::Straight {
                p: pt,
                radius: at(p::STRAIGHT_VERTEX_RADIUS),
            };
        }
    };
    Vertex::Cubic { p: pt, r#in, out }
}

/// The matrix taking the unit triangle (0, 0), (1, 0), (0, 1) onto `a`,
/// `b`, `c`; none when they are in a line.
fn triangle(a: Point, b: Point, c: Point) -> Option<Matrix> {
    let m = Matrix::new(b.x - a.x, b.y - a.y, c.x - a.x, c.y - a.y, a.x, a.y);
    (m.determinant().abs() > 1e-9).then_some(m)
}

/// Triangle `points` with each side moved half a pixel out, so neighbors
/// meet without a seam.
fn dilated(points: [Point; 3]) -> Path {
    const OUT: f64 = 0.5;
    let [a, b, c] = points;
    // Outward is right of each side when the triangle turns left.
    let turn = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let normal = |p: Point, q: Point| {
        let (dx, dy) = (q.x - p.x, q.y - p.y);
        let len = dx.hypot(dy).max(1e-9);
        let s = if turn > 0.0 { 1.0 } else { -1.0 };
        Point::new(dy / len * s, -dx / len * s)
    };
    let mut path = Path::default();
    for k in 0..3 {
        let (prev, p, next) = (points[(k + 2) % 3], points[k], points[(k + 1) % 3]);
        let (n1, n2) = (normal(prev, p), normal(p, next));
        // The sides' offsets meet this far along their normals' sum; capped
        // at sharp corners.
        let scale = OUT / (1.0 + n1.x * n2.x + n1.y * n2.y).max(0.25);
        let q = Point::new(p.x + (n1.x + n2.x) * scale, p.y + (n1.y + n2.y) * scale);
        if k == 0 {
            path.move_to(q);
        } else {
            path.line_to(q);
        }
    }
    path.close();
    path
}

/// The variable-length unsigned integers of `bytes` (LEB128).
fn varuints(bytes: &[u8]) -> Vec<u64> {
    let mut out = Vec::new();
    let (mut value, mut shift) = (0u64, 0u32);
    for &b in bytes {
        if shift < 64 {
            value |= u64::from(b & 0x7F) << shift;
        }
        shift += 7;
        if b & 0x80 == 0 {
            out.push(value);
            (value, shift) = (0, 0);
        }
    }
    out
}

/// A color of Rive's ARGB words.
fn argb(v: u32) -> Color {
    let [a, r, g, b] = v.to_be_bytes();
    Color::rgba8(r, g, b, a)
}

/// `curve` with its values mapped by `f` (which keeps holds holds).
fn map_curve(mut curve: Curve<f64>, f: impl Fn(f64) -> f64) -> Curve<f64> {
    curve.setup = f(curve.setup);
    for k in &mut curve.keys {
        k.value = f(k.value);
        k.interp = Interp::Hold;
    }
    curve
}

/// `curve` with its values mapped by `f`, a straight map keeping its
/// interpolation.
fn map_curve_straight(mut curve: Curve<f64>, f: impl Fn(f64) -> f64) -> Curve<f64> {
    curve.setup = f(curve.setup);
    for k in &mut curve.keys {
        k.value = f(k.value);
    }
    curve
}

/// The keyed curve drawing animated value `a`.
fn curve_of(a: &Animated<f64>) -> Curve<f64> {
    match a {
        Animated::Static(v) => Curve::fixed(*v),
        Animated::Track(track) => Curve {
            setup: track.keyframes.first().map_or(0.0, |k| k.value),
            keys: track
                .keyframes
                .iter()
                .map(|k| Key {
                    t: k.time,
                    value: k.value,
                    interp: match &k.easing {
                        Easing::Hold => Interp::Hold,
                        Easing::CubicBezier { x1, y1, x2, y2 } => {
                            Interp::Cubic([*x1, *y1, *x2, *y2])
                        }
                        _ => Interp::Linear,
                    },
                })
                .collect(),
        },
    }
}

/// `a` times `scale` (exact: interpolation is straight in the values).
fn scaled(a: Animated<f64>, scale: f64) -> Animated<f64> {
    if scale == 1.0 {
        return a;
    }
    match a {
        Animated::Static(v) => Animated::Static(v * scale),
        Animated::Track(mut track) => {
            for k in &mut track.keyframes {
                k.value *= scale;
            }
            Animated::Track(track)
        }
    }
}

fn is_const(a: &Animated<f64>, v: f64) -> bool {
    a.as_static() == Some(&v)
}

/// Rive's blend modes (Skia's numbers).
fn blend_mode(v: u64) -> BlendMode {
    match v {
        14 => BlendMode::Screen,
        15 => BlendMode::Overlay,
        16 => BlendMode::Darken,
        17 => BlendMode::Lighten,
        18 => BlendMode::ColorDodge,
        19 => BlendMode::ColorBurn,
        20 => BlendMode::HardLight,
        21 => BlendMode::SoftLight,
        22 => BlendMode::Difference,
        23 => BlendMode::Exclusion,
        24 => BlendMode::Multiply,
        25 => BlendMode::Hue,
        26 => BlendMode::Saturation,
        27 => BlendMode::Color,
        28 => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
}

/// An embedded image's media type and size in pixels, from its header.
fn image_info(bytes: &[u8]) -> Option<(&'static str, (f64, f64))> {
    let be32 = |at: usize| -> Option<f64> {
        let b = bytes.get(at..at + 4)?;
        Some(f64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])))
    };
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(("image/png", (be32(16)?, be32(20)?)));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        // The first start-of-frame segment holds the size.
        let mut at = 2;
        while at + 9 < bytes.len() {
            if bytes[at] != 0xFF {
                return None;
            }
            let marker = bytes[at + 1];
            let len = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
            if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                let h = u16::from_be_bytes([bytes[at + 5], bytes[at + 6]]);
                let w = u16::from_be_bytes([bytes[at + 7], bytes[at + 8]]);
                return Some(("image/jpeg", (f64::from(w), f64::from(h))));
            }
            at += 2 + len;
        }
        return None;
    }
    if bytes.len() > 30 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let le16 = |at: usize| f64::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]));
        let size = match &bytes[12..16] {
            b"VP8X" => {
                let le24 = |at: usize| {
                    f64::from(u32::from_le_bytes([
                        bytes[at],
                        bytes[at + 1],
                        bytes[at + 2],
                        0,
                    ]))
                };
                (le24(24) + 1.0, le24(27) + 1.0)
            }
            b"VP8 " => (
                f64::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3FFF),
                f64::from(u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3FFF),
            ),
            b"VP8L" => {
                let b = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
                (
                    f64::from(b & 0x3FFF) + 1.0,
                    f64::from((b >> 14) & 0x3FFF) + 1.0,
                )
            }
            _ => (le16(0), le16(0)),
        };
        return Some(("image/webp", size));
    }
    None
}
