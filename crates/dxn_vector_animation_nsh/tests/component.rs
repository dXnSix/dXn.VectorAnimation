//! The component on a headless engine: it reads its source, reports it,
//! sizes itself, and reports errors.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation_nsh::{AnimationInfo, Source, vector_animation};
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
fn draws_canvases_the_native_graph_moves() {
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
    app.wait(std::time::Duration::from_millis(100)).unwrap();
    let (canvases, slots, started) = *seen.lock().unwrap();
    assert!(canvases > 0, "the scene is drawn on canvases");
    assert!(slots > 0, "their numbers are animated values");
    assert!(
        started,
        "playback is a composite animation of the time value"
    );
}

#[test]
fn reports_what_it_cannot_read() {
    let mut app = start::<Broken>();
    app.wait_for("Error neither SVG, Lottie nor a compiled scene")
        .unwrap();
}
