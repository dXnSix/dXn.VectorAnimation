//! The parts drawn frame by frame follow the native graph's time: they read
//! its value each frame (a clock of their own starts when the engine's first
//! frame comes, not when the graph starts, and on the web the parts drawn
//! with it stayed behind the rest by a fifth of a second and more), and a
//! run that ends leaves them on its last frame.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Easing, Fill, Geometry, Group, Keyframe, LoopMode,
    Node, Paint, Scene, Shape, ShapePath, Star, StarKind, Timeline, Track, Trim, TrimMode,
};
use dxn_vector_animation_nsh::{AnimationInfo, Source, vector_animation};
use nsh_core::{MountItem, Size, element::Root, prelude::*};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

fn track(keys: &[(f64, f64)]) -> Animated<f64> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|(t, v)| Keyframe::new(*t, *v, Easing::Linear))
            .collect(),
    ))
}

/// A turning, growing, trimmed star: what the graph does not draw, so the
/// player draws it frame by frame. It plays once, in half a second.
fn scene() -> Scene {
    let star = Star {
        kind: StarKind::Star,
        center: AnimatedPoint::fixed(50.0, 50.0),
        points: track(&[(0.0, 5.0), (0.5, 7.0)]),
        outer_radius: Animated::Static(30.0),
        inner_radius: track(&[(0.0, 8.0), (0.5, 20.0)]),
        outer_roundness: Animated::Static(0.0),
        inner_roundness: Animated::Static(0.0),
        rotation: track(&[(0.0, 0.0), (0.5, 72.0)]),
        direction: Direction::Clockwise,
    };
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 0.5,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Once,
            markers: vec![],
        },
        root: Group {
            children: vec![Node::shape(Shape {
                paths: vec![ShapePath::new(Geometry::Star(star))],
                fill: Some(Fill {
                    paint: Paint::solid(Color::rgba(0.2, 0.6, 0.3, 1.0)),
                    opacity: Animated::Static(1.0),
                    rule: Default::default(),
                }),
                trim: Some(Trim {
                    start: Animated::Static(0.0),
                    end: track(&[(0.0, 0.4), (0.5, 1.0)]),
                    offset: Animated::Static(0.0),
                    mode: TrimMode::Combined,
                }),
                ..Default::default()
            })],
            ..Group::new()
        },
    }
}

/// The scene in a player; at `progress` when one is given, else playing.
#[component]
fn Player() -> impl IntoElement {
    let status = signal(String::from("loading"));
    let json = dxn_vector_animation::compiled::to_json(&scene(), false).unwrap();
    view()
        .child(
            vector_animation()
                .src(Some(Source::Text(json)))
                .on_load(move |info: &AnimationInfo| {
                    status.set(format!("Loaded {:.1}s", info.duration))
                })
                .on_end(move || status.set("Ended".into())),
        )
        .child(text(move || status.get()))
}

/// The same, shown at its last frame without playing.
#[component]
fn Last() -> impl IntoElement {
    let status = signal(String::from("loading"));
    let json = dxn_vector_animation::compiled::to_json(&scene(), false).unwrap();
    view()
        .child(
            vector_animation()
                .src(Some(Source::Text(json)))
                .progress(Some(1.0))
                .on_load(move |info: &AnimationInfo| {
                    status.set(format!("Loaded {:.1}s", info.duration))
                }),
        )
        .child(text(move || status.get()))
}

fn start<C: Component>() -> Headless
where
    C::Props: Default,
{
    Headless::start(EngineConfig {
        viewport: Size::new(px(200.0), px(200.0)),
        ..EngineConfig::new(Root::of::<C>())
    })
    .expect("the engine starts")
}

/// What the last drawing of each canvas is, once the app has run `until`.
fn last_drawings<C: Component>(until: &str) -> Vec<String>
where
    C::Props: Default,
{
    let mut app = start::<C>();
    let seen = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let log = seen.clone();
    app.observe(move |_, batch| {
        let mut s = log.lock().unwrap();
        for item in &batch.items {
            if let MountItem::Canvas { id, drawing, .. } = item {
                let key = format!("{id:?}");
                let ops = format!("{:?}", drawing.ops);
                match s.iter_mut().find(|(k, _)| *k == key) {
                    Some(entry) => entry.1 = ops,
                    None => s.push((key, ops)),
                }
            }
        }
    });
    app.wait_for(until).unwrap();
    app.wait(Duration::from_millis(300)).unwrap();
    let mut drawings: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|(_, o)| o.clone())
        .collect();
    drawings.sort();
    drawings
}

#[test]
fn a_run_that_ends_leaves_the_sampled_part_on_its_last_frame() {
    let played = last_drawings::<Player>("Ended");
    let last = last_drawings::<Last>("Loaded 0.5s");
    assert!(!played.is_empty());
    assert_eq!(played, last, "the part drawn is the last frame's");
}
