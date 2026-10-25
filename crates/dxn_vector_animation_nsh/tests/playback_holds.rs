//! A hold (`progress`) stops the run playing as a whole: a stop of the time
//! value alone ends only the leg moving it, and a run that loops goes on to
//! its next, playing on through the hold.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use dxn_vector_animation_nsh::{AnimationInfo, Source, vector_animation};
use nsh_core::{
    MountItem, Size,
    animation::AnimationOp,
    ui::{Root, prelude::*},
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

const FIGMA: &str = include_str!("../../../fixtures/figma-motion.svg");

/// The Figma fixture (a loop) playing, held halfway when "hold" is pressed.
#[component]
fn Held() -> impl IntoElement {
    let status = signal(String::from("loading"));
    let progress = signal(None::<f64>);
    view()
        .child(
            vector_animation()
                .src(Some(Source::Text(FIGMA.into())))
                .progress(move || progress.get())
                .on_load(move |_: &AnimationInfo| status.set("Loaded".into())),
        )
        .child(
            view()
                .on_press(move || progress.set(Some(0.5)))
                .child(text("hold")),
        )
        .child(text(move || status.get()))
}

/// The animations started (composites) and stopped (whole runs), in order.
#[derive(Default)]
struct Runs {
    started: Vec<u64>,
    stopped: Vec<u64>,
}

#[test]
fn a_hold_stops_the_looping_run_as_a_whole() {
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(400.0), px(800.0)),
        ..EngineConfig::new(Root::of::<Held>())
    })
    .expect("the engine starts");
    let runs = Arc::new(Mutex::new(Runs::default()));
    let log = runs.clone();
    app.observe(move |_, batch| {
        let mut r = log.lock().unwrap();
        for item in &batch.items {
            match item {
                MountItem::Animation(AnimationOp::StartComposite { animation, .. }) => {
                    r.started.push(u64::from(*animation));
                }
                MountItem::Animation(AnimationOp::StopAnimation { animation }) => {
                    r.stopped.push(u64::from(*animation));
                }
                _ => {}
            }
        }
    });
    app.wait_for("Loaded").unwrap();
    let seen = runs.clone();
    app.wait_until("a run playing", Duration::from_secs(10), move |_| {
        !seen.lock().unwrap().started.is_empty()
    })
    .unwrap();
    let playing = *runs.lock().unwrap().started.last().expect("it plays a run");
    app.press("hold").unwrap();
    let seen = runs.clone();
    // Stopped soon; whatever starts after the hold would come with it.
    let _ = app.wait_until("the run stopped", Duration::from_secs(10), move |_| {
        seen.lock().unwrap().stopped.contains(&playing)
    });
    app.wait(Duration::from_millis(200)).unwrap();
    let r = runs.lock().unwrap();
    assert!(
        r.stopped.contains(&playing),
        "the run playing is stopped: started {:?}, stopped {:?}",
        r.started,
        r.stopped
    );
    assert_eq!(
        r.started.last(),
        Some(&playing),
        "nothing plays after the hold"
    );
}
