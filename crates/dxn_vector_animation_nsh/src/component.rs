//! `VectorAnimation`: plays animated SVG and Lottie.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

use dxn_vector_animation::{Frame, ImageData, Matrix, Rect, Scene};
use nsh_core::{
    element::{SignalEquals, signal_with},
    geometry::{Bounds, Points, Size, px},
    prelude::*,
};

use crate::{
    graph::Graph,
    lower::{self, Nodes},
    plan,
    playback::{Direction, Playback},
    source::{self, At, Loaded, PosterFrame, Repeat, Segment, Source},
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
    /// file or a compiled scene, its bytes, or a Lottie or compiled scene's
    /// JSON (a `.json` import).
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
    /// The animation could not be loaded or read.
    Error(String),
}

/// The methods of a [`VectorAnimation`]'s ref.
pub struct VectorAnimationApi {
    seek_to: Signal<Option<(f64, u64)>>,
    event: Signal<Option<(String, u64)>>,
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
}

/// A scene ready to play: whether the native graph plays it.
struct Ready {
    loaded: Loaded,
    native: bool,
    /// Parts of it are sampled frame by frame, on the engine's clock.
    mixed: bool,
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
    cx.methods(VectorAnimationApi { seek_to, event });
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
    // Stops what plays: its tasks, its run, the time value.
    let halt = move || {
        if let Some(task) = playing.replace(None) {
            task.cancel();
        }
        if let Some(task) = ticking.replace(None) {
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
        let Some(src) = src else { return };
        loading.replace(Some(spawn(async move {
            match load(src).await {
                Ok(loaded) => {
                    for w in &loaded.warnings {
                        log::warn!("VectorAnimation: left out: {w}");
                    }
                    let (native, mixed) = match plan::compile(&loaded.scene, Matrix::IDENTITY) {
                        Ok(plan) => {
                            for why in &plan.sampled {
                                log::info!(
                                    "VectorAnimation: a part sampled frame by frame ({why})"
                                );
                            }
                            (true, !plan.sampled.is_empty())
                        }
                        Err(plan::Unsupported(why)) => {
                            log::info!("VectorAnimation: sampled frame by frame ({why})");
                            (false, false)
                        }
                    };
                    let info = info(&loaded);
                    ready.set(Some(Rc::new(Ready {
                        loaded,
                        native,
                        mixed,
                    })));
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
                let span = segment(scene, chosen);
                let repeat = props
                    .repeat
                    .get()
                    .unwrap_or(match scene.timeline.loop_mode {
                        dxn_vector_animation::LoopMode::Once => Repeat::Once,
                        _ => Repeat::Forever,
                    });
                let direction = match (props.direction.get(), scene.timeline.loop_mode) {
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
        untracked(move || {
            halt();
            let show = |t: f64| {
                time.set(t as f32);
                clock.set(t);
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
                let from = if native {
                    f64::from(time.value().await)
                } else {
                    clock.get_untracked()
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
                    let end = animation.start();
                    run.replace(Some(animation));
                    let ended = end.await;
                    over.set(true);
                    if ended {
                        clock.set(playback.last());
                        cx.emit(VectorAnimationEvent::End);
                    }
                } else {
                    // The engine's clock, frame by frame.
                    clock.set(from);
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
    let made = RefCell::new(Rasters::new(!still, 0.0));
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

/// The share of the time the parts that move may take making their rasters,
/// all together: slower than that, each is made less often, held a few
/// frames, so that the rest of the page (and on the web, the page itself)
/// keeps its time.
const RASTER_SHARE: f64 = 0.25;

thread_local! {
    /// How long a raster of each part that moves takes, summed over the
    /// parts (in seconds).
    static RASTER_COST: Cell<f64> = const { Cell::new(0.0) };
}

/// The rasters a part made, by frame.
struct Rasters {
    ratio: f64,
    by_frame: HashMap<i64, Option<(Arc<ImageData>, Rect)>>,
    bytes: usize,
    /// How long one takes to make, in seconds (smoothed).
    cost: f64,
    /// It moves: what it costs counts in [`RASTER_COST`].
    moving: bool,
}

impl Drop for Rasters {
    fn drop(&mut self) {
        if self.moving {
            RASTER_COST.with(|c| c.set((c.get() - self.cost).max(0.0)));
        }
    }
}

impl Rasters {
    /// None yet, of a part that moves or not, at `ratio`.
    fn new(moving: bool, ratio: f64) -> Rasters {
        Rasters {
            ratio,
            by_frame: HashMap::new(),
            bytes: 0,
            cost: 0.0,
            moving,
        }
    }

    /// The raster at `t` (to the nearest frame) at `ratio`, made by `make`
    /// if it is not kept.
    fn get(
        &mut self,
        t: f64,
        ratio: f64,
        make: impl FnOnce(f64) -> Option<(Arc<ImageData>, Rect)>,
    ) -> Option<(Arc<ImageData>, Rect)> {
        if ratio != self.ratio {
            *self = Rasters::new(self.moving, ratio);
        }
        // As often as the parts' share of the time allows, at most each
        // frame.
        let all = RASTER_COST.with(Cell::get).max(self.cost);
        let all = if self.moving { all } else { self.cost };
        let step = (all / RASTER_SHARE).max(1.0 / RASTER_RATE);
        let frame = ((t / step).floor() * step * RASTER_RATE).round() as i64;
        if let Some(kept) = self.by_frame.get(&frame) {
            return kept.clone();
        }
        let started = nsh_core::time::now();
        let made = make(frame as f64 / RASTER_RATE);
        let took = nsh_core::time::now()
            .saturating_duration_since(started)
            .as_secs_f64();
        let cost = if self.cost == 0.0 {
            took
        } else {
            0.7 * self.cost + 0.3 * took
        };
        if self.moving {
            RASTER_COST.with(|c| c.set((c.get() + cost - self.cost).max(0.0)));
        }
        self.cost = cost;
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
        let mut rasters = Rasters::new(true, 0.0);
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
        let mut rasters = Rasters::new(true, 0.0);
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
