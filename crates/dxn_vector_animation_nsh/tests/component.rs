//! The component on a headless engine: it reads its source, reports it
//! (its events too), sizes itself, plays what an event triggers, and
//! reports errors.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation_nsh::{AnimationInfo, Source, VectorAnimationApi, vector_animation};
use nsh_core::{
    Size,
    ui::{Root, prelude::*},
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

const FIGMA: &str = include_str!("../../../fixtures/figma-motion.svg");

/// The Figma fixture in a player, what it reports shown as text.
#[component]
fn Player() -> impl IntoElement {
    let status = signal(String::from("loading"));
    view()
        .child(
            vector_animation()
                .src(Some(Source::Text(FIGMA.into())))
                .on_load(move |info: &AnimationInfo| {
                    status.set(format!(
                        "Loaded {:.1}s {}x{} warnings {}",
                        info.duration,
                        info.size.width,
                        info.size.height,
                        info.warnings.len()
                    ))
                })
                .on_error(move |e: &String| status.set(format!("Error {e}"))),
        )
        .child(text(move || status.get()))
}

/// An icon that moves when it is clicked, its player clicked once loaded.
#[component]
fn Clicked() -> impl IntoElement {
    let status = signal(String::from("loading"));
    let player = component_ref::<dxn_vector_animation_nsh::VectorAnimation>();
    let triggered = signal(None::<std::time::Instant>);
    view()
        .child(
            vector_animation()
                .component_ref(&player)
                .src(Some(Source::Text(
                    r#"<svg width="40" height="40" xmlns="http://www.w3.org/2000/svg"><rect id="btn" width="10" height="10">
<animate attributeName="x" from="0" to="30" dur="0.3s" fill="freeze" begin="click"/></rect></svg>"#
                        .into(),
                )))
                .on_load(move |info: &AnimationInfo| {
                    // Its timeline is empty: it ends at once, before the
                    // event is triggered.
                    status.set(format!("Events {}", info.events.join(",")));
                    triggered.set(Some(std::time::Instant::now()));
                    if let Some(api) = player.api::<VectorAnimationApi>() {
                        api.trigger("btn.click".into());
                    }
                })
                .on_end(move || {
                    // The event's 0.3 s played since it was triggered.
                    if let Some(at) = triggered.get_untracked()
                        && at.elapsed().as_secs_f64() >= 0.25
                    {
                        status.set("Played the click".into());
                    }
                }),
        )
        .child(text(move || status.get()))
}

/// A scene whose timeline loops, its player on a marker that plays once (a
/// Rive animation's): it ends.
#[component]
fn OnceMarker() -> impl IntoElement {
    use dxn_vector_animation::{Group, LoopMode, Marker, Scene, Timeline, compiled};
    use dxn_vector_animation_nsh::Segment;

    let status = signal(String::from("playing"));
    let scene = Scene {
        width: 10.0,
        height: 10.0,
        timeline: Timeline {
            duration: 1.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Loop,
            markers: vec![Marker {
                loop_mode: Some(LoopMode::Once),
                ..Marker::new("tap", 2.0, 0.2)
            }],
        },
        root: Group::new(),
    };
    let json = compiled::to_json(&scene, false).unwrap();
    view()
        .child(
            vector_animation()
                .src(Some(Source::Text(json)))
                .segment(Some(Segment::Marker("tap".into())))
                .on_end(move || status.set("Ended".into())),
        )
        .child(text(move || status.get()))
}

/// A player of something that is not an animation.
#[component]
fn Broken() -> impl IntoElement {
    let status = signal(String::from("loading"));
    view()
        .child(
            vector_animation()
                .src(Some(Source::Text("<html></html>".into())))
                .on_error(move |e: &String| status.set(format!("Error {e}"))),
        )
        .child(text(move || status.get()))
}

fn start<C: Component>() -> Headless
where
    C::Props: Default,
{
    Headless::start(EngineConfig {
        viewport: Size::new(px(400.0), px(800.0)),
        ..EngineConfig::new(Root::of::<C>())
    })
    .expect("the engine starts")
}

#[test]
fn reports_what_it_loaded() {
    let mut app = start::<Player>();
    app.wait_for("Loaded 2.0s 200x100 warnings 0").unwrap();
}

#[test]
fn draws_canvases_and_layers_the_native_graph_moves() {
    use std::sync::{Arc, Mutex};

    use nsh_core::{MountItem, animation::AnimationOp};

    let mut app = start::<Player>();
    let seen = Arc::new(Mutex::new((0usize, 0usize, false)));
    let log = seen.clone();
    app.observe(move |_, batch| {
        let mut s = log.lock().unwrap();
        for item in &batch.items {
            match item {
                MountItem::Canvas { drawing, .. } => {
                    s.0 += 1;
                    s.1 = s.1.max(drawing.slots.len());
                }
                MountItem::Animation(AnimationOp::StartComposite { .. }) => s.2 = true,
                _ => {}
            }
        }
    });
    app.wait_for("Loaded 2.0s 200x100 warnings 0").unwrap();
    let drawn = seen.clone();
    let _ = app.wait_until(
        "canvases drawn and playback started",
        std::time::Duration::from_secs(10),
        move |_| matches!(*drawn.lock().unwrap(), (c, _, true) if c > 0),
    );
    let (canvases, slots, started) = *seen.lock().unwrap();
    assert!(canvases > 0, "the scene is drawn on canvases");
    // Its groups only move and fade: layers the graph moves, their
    // canvases still (other scenes' numbers are animated values).
    let scene = dxn_vector_animation_nsh::source::read(FIGMA.as_bytes())
        .unwrap()
        .scene;
    let plan =
        dxn_vector_animation_nsh::plan::compile(&scene, dxn_vector_animation::Matrix::IDENTITY)
            .unwrap();
    let moving = plan.units.iter().any(|u| {
        matches!(
            u,
            dxn_vector_animation_nsh::plan::Unit::Layer {
                transform: Some(_),
                ..
            }
        )
    });
    assert!(slots > 0 || moving, "the graph moves numbers or layers");
    assert!(
        started,
        "playback is a composite animation of the time value"
    );
}

#[test]
fn plays_what_an_event_triggers() {
    let mut app = start::<Clicked>();
    // Triggered when loaded: it plays its segment and ends.
    app.wait_for("Played the click").unwrap();
}

#[test]
fn plays_a_marker_as_its_loop_says() {
    let mut app = start::<OnceMarker>();
    app.wait_for("Ended").unwrap();
}

#[test]
fn reports_what_it_cannot_read() {
    let mut app = start::<Broken>();
    app.wait_for("Error neither SVG, Lottie, Rive nor a compiled scene")
        .unwrap();
}
