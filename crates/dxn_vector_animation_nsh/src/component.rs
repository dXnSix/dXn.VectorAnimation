//! `VectorAnimation`: plays animated SVG and Lottie.

use std::{rc::Rc, sync::Arc};

use dxn_vector_animation::{Frame, Matrix, Scene};
use nsh_core::{
    geometry::{Bounds, Points, Size, px},
    ui::{SignalEquals, prelude::*, signal_with},
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
}

#[methods]
impl VectorAnimationApi {
    /// Moves to `seconds` on the timeline; playback goes on from there
    /// unless paused.
    pub fn seek(&self, seconds: f64) {
        let generation = self.seek_to.get_untracked().map_or(0, |(_, g)| g + 1);
        self.seek_to.set(Some((seconds, generation)));
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
    cx.methods(VectorAnimationApi { seek_to });
    let loading = stored(None::<Spawned<()>>);
    let playing = stored(None::<Spawned<()>>);
    // The engine's clock beside the graph's, for parts sampled frame by
    // frame.
    let ticking = stored(None::<Spawned<()>>);

    // Reading the source.
    effect(move || {
        let src = props.src.get();
        if let Some(task) = loading.replace(None) {
            task.cancel();
        }
        ready.set(None);
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
        let Some(r) = ready.get() else { return };
        let scene = &r.loaded.scene;
        let span = segment(scene, props.segment.get());
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
        let playback = Playback {
            start: span.0,
            end: span.1,
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
        let (native, mixed) = (r.native, r.mixed);
        untracked(move || {
            if let Some(task) = playing.replace(None) {
                task.cancel();
            }
            if let Some(task) = ticking.replace(None) {
                task.cancel();
            }
            time.stop();
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
                // The engine's clock, frame by frame.
                let tick = move || async move {
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
                };
                if native {
                    if mixed {
                        ticking.replace(Some(spawn(tick())));
                    }
                    time.set(from as f32);
                    let run = playback.animation(&time, from);
                    if run.start().await {
                        cx.emit(VectorAnimationEvent::End);
                    }
                } else {
                    tick().await;
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
        let size = size.get();
        let (w, h) = (f64::from(size.width.0), f64::from(size.height.0));
        if w <= 0.0 || h <= 0.0 {
            return Vec::new();
        }
        let base = fit(&r.loaded.scene, w, h, props.resize_mode.get());
        if r.native
            && let Ok(plan) = plan::compile(&r.loaded.scene, base)
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
        if plan::has_effects(&dxn_vector_animation::Node::Group(
            r.loaded.scene.root.clone(),
        )) {
            let scene = Arc::new(r.loaded.scene.clone());
            return vec![rastered(&scene, base, (w, h), clock, time)];
        }
        let r = r.clone();
        vec![sampled(
            move |t| r.loaded.scene.frame_at(t),
            base,
            clock,
            time,
        )]
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
    canvas(move || {
        let t = if still { 0.0 } else { clock.get() };
        let ratio = f64::from(pixel_ratio()).max(1.0);
        let ops = match plan::raster(&view, t, ratio) {
            Some((image, at)) => vec![plan::Op::Image(image, at)],
            None => Vec::new(),
        };
        lower::painter(&ops, &Nodes::new(&empty, time))
    })
    .absolute()
    .size_full()
    .into_element()
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
