//! The scene fits into the box the player draws in: inside its border.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use dxn_vector_animation_nsh::{Source, vector_animation};
use nsh_core::{
    MountItem, Size,
    canvas::{CanvasOp, Num},
    element::Root,
    prelude::*,
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

const FIGMA: &str = include_str!("../../../fixtures/figma-motion.svg");

/// The Figma fixture (200 × 100) in a 200 × 100 player with a border of 10.
#[component]
fn Bordered() -> impl IntoElement {
    let status = signal(String::from("loading"));
    view()
        .child(
            vector_animation()
                .src(Some(Source::Text(FIGMA.into())))
                .w(px(200.0))
                .h(px(100.0))
                .border(px(10.0))
                .on_load(move |_: &dxn_vector_animation_nsh::AnimationInfo| {
                    status.set("Loaded".into())
                }),
        )
        .child(text(move || status.get()))
}

#[test]
fn the_scene_fits_inside_the_border() {
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(400.0), px(400.0)),
        ..EngineConfig::new(Root::of::<Bordered>())
    })
    .expect("the engine starts");
    let first = Arc::new(Mutex::new(None));
    let seen = first.clone();
    app.observe(move |_, batch| {
        for item in &batch.items {
            if let MountItem::Canvas { drawing, .. } = item
                && let Some(CanvasOp::Transform(m)) = drawing.ops.first()
            {
                seen.lock().unwrap().get_or_insert(*m);
            }
        }
    });
    app.wait_for("Loaded").unwrap();
    let drawn = first.clone();
    let _ = app.wait_until(
        "a canvas drawing the scene",
        std::time::Duration::from_secs(10),
        move |_| drawn.lock().unwrap().is_some(),
    );
    let fit = first.lock().unwrap().expect("a canvas drawing the scene");
    // 180 × 80 inside the border: the scene scaled by 0.8, centered.
    let values: Vec<f32> = fit
        .iter()
        .map(|n| match n {
            Num::Const(v) => *v,
            Num::Slot(_) => f32::NAN,
        })
        .collect();
    let expected = [0.8, 0.0, 0.0, 0.8, 10.0, 0.0];
    for (v, e) in values.iter().zip(expected) {
        assert!((v - e).abs() < 1e-4, "{values:?}");
    }
}
