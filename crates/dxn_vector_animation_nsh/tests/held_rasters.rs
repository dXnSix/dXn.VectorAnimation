//! A part drawn as an image per frame (a blur, a color change) keeps the
//! frame before it on the canvas under the new one, which replaces it
//! (`copy`): the web loads an image on the side, and a canvas drawn with
//! only the new frame's image is blank until it comes, a flicker on every
//! frame.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use dxn_vector_animation::{
    Animated, AnimatedPoint, Blur, Color, Direction, Easing, Effect, EllipseStart, Fill, Geometry,
    Group, Keyframe, LoopMode, Node, Paint, Scene, Shape, ShapePath, Timeline, Track,
};
use dxn_vector_animation_nsh::{AnimationInfo, Source, vector_animation};
use nsh_core::{
    MountItem, Size,
    canvas::{CanvasOp, Composite},
    element::Root,
    prelude::*,
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

/// A disc whose blur grows, playing once in half a second.
fn scene() -> Scene {
    let radius = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, 2.0, Easing::Linear),
        Keyframe::new(0.5, 8.0, Easing::Linear),
    ]));
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
            effects: vec![Effect::Blur(Blur { radius })],
            children: vec![Node::shape(Shape {
                paths: vec![ShapePath::new(Geometry::Ellipse {
                    center: AnimatedPoint::fixed(50.0, 50.0),
                    radius: AnimatedPoint::fixed(25.0, 25.0),
                    direction: Direction::Clockwise,
                    start: EllipseStart::Top,
                })],
                fill: Some(Fill {
                    paint: Paint::solid(Color::rgba(0.9, 0.2, 0.1, 1.0)),
                    opacity: Animated::Static(1.0),
                    rule: Default::default(),
                }),
                ..Default::default()
            })],
            ..Group::new()
        },
    }
}

/// The scene in a player; at its last frame without playing when `last`.
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

/// One drawing of the canvas: the images it draws, in order, and whether
/// `copy` stands before each.
#[derive(Clone, Debug, PartialEq)]
struct Drawing {
    images: Vec<(String, bool)>,
}

impl Drawing {
    fn newest(&self) -> Option<&str> {
        self.images.last().map(|(uri, _)| uri.as_str())
    }
}

/// Every drawing the canvases of `C` made, in order, until the app shows
/// `until`.
fn drawings<C: Component>(until: &str) -> Vec<Drawing>
where
    C::Props: Default,
{
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(200.0), px(200.0)),
        ..EngineConfig::new(Root::of::<C>())
    })
    .expect("the engine starts");
    let seen = Arc::new(Mutex::new(Vec::<Drawing>::new()));
    let log = seen.clone();
    app.observe(move |_, batch| {
        for item in &batch.items {
            if let MountItem::Canvas { drawing, .. } = item {
                let mut copy = false;
                let mut images = Vec::new();
                for op in &drawing.ops {
                    match op {
                        CanvasOp::Composite(c) => copy = *c == Composite::Copy,
                        CanvasOp::DrawImage { image, .. } => {
                            images.push((image.uri.to_string(), copy));
                        }
                        _ => {}
                    }
                }
                if !images.is_empty() {
                    log.lock().unwrap().push(Drawing { images });
                }
            }
        }
    });
    app.wait_for(until).unwrap();
    app.wait(Duration::from_millis(300)).unwrap();
    seen.lock().unwrap().clone()
}

#[test]
fn a_new_frame_is_drawn_over_the_one_before_and_replaces_it() {
    let all = drawings::<Player>("Ended");
    // The frames change: more than a few different images drawn.
    let mut newest: Vec<&str> = all.iter().filter_map(Drawing::newest).collect();
    newest.dedup();
    assert!(newest.len() > 3, "{} frames drawn", newest.len());
    let mut previous: Option<&Drawing> = None;
    for drawing in &all {
        if let Some(before) = previous.filter(|b| b.newest() != drawing.newest()) {
            // A new image: the one drawn last comes first, plain, and the
            // new one replaces what is there.
            assert_eq!(drawing.images.len(), 2, "{drawing:?}");
            assert_eq!(drawing.images[0].0, before.newest().unwrap());
            assert!(!drawing.images[0].1);
            assert!(drawing.images[1].1, "the new image replaces");
        }
        previous = Some(drawing);
    }
}

#[test]
fn a_run_that_ends_leaves_the_last_frame_drawn() {
    let played = drawings::<Player>("Ended");
    let last = drawings::<Last>("Loaded 0.5s");
    assert_eq!(
        played.last().and_then(Drawing::newest),
        last.last().and_then(Drawing::newest),
        "the image drawn is the last frame's"
    );
}
