//! A timeline with a loop start (a staggered loader's delays): playing
//! forever plays from the start to the end once, then from the loop start
//! to the end over and over; alternating turns back at the loop start;
//! reversed runs and other parts play whole.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use dxn_vector_animation_nsh::{Direction, Repeat, Source, playback::Playback, vector_animation};
use nsh_core::{
    MountItem, Size,
    animation::{AnimationOp, Composite, Driver},
    ui::{Root, prelude::*},
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

/// A 3 s part looping from 1 s, at speed 1.
fn playback(direction: Direction, repeat: Repeat) -> Playback {
    Playback {
        start: 0.0,
        end: 3.0,
        loop_start: 1.0,
        rate: 1.0,
        direction,
        repeat,
    }
}

fn close(a: (f64, bool), b: (f64, bool)) -> bool {
    (a.0 - b.0).abs() < 1e-9 && a.1 == b.1
}

#[test]
fn forever_plays_the_intro_once() {
    let p = playback(Direction::Normal, Repeat::Forever);
    assert!(close(p.position(0.0, 0.5), (0.5, false)));
    assert!(close(p.position(0.0, 2.5), (2.5, false)));
    // Past the end: from the loop start, 2 s at a time.
    assert!(close(p.position(0.0, 3.5), (1.5, false)));
    assert!(close(p.position(0.0, 5.5), (1.5, false)));
    // Resumed in the intro, it plays the rest of it first.
    assert!(close(p.position(0.5, 3.0), (1.5, false)));
}

#[test]
fn counted_runs_after_the_first_start_at_the_loop() {
    let p = playback(Direction::Normal, Repeat::Times(2));
    assert!(close(p.position(0.0, 4.0), (2.0, false)));
    assert!(close(p.position(0.0, 5.0), (3.0, true)));
}

#[test]
fn alternate_turns_back_at_the_loop_start() {
    let p = playback(Direction::Alternate, Repeat::Forever);
    assert!(close(p.position(0.0, 4.0), (2.0, false)));
    assert!(close(p.position(0.0, 5.5), (1.5, false)));
    let two = playback(Direction::Alternate, Repeat::Times(2));
    assert!(close(two.position(0.0, 5.0), (1.0, true)));
    assert_eq!(two.last(), 1.0);
}

#[test]
fn reversed_runs_play_whole() {
    let p = playback(Direction::Reverse, Repeat::Forever);
    assert_eq!(p.loop_from(), 0.0);
    assert!(close(p.position(3.0, 3.5), (2.5, false)));
}

#[test]
fn a_loop_start_outside_the_part_plays_it_whole() {
    let mut p = playback(Direction::Normal, Repeat::Forever);
    p.loop_start = 3.0;
    assert_eq!(p.loop_from(), 0.0);
    assert!(close(p.position(0.0, 3.5), (0.5, false)));
}

/// Three dots fading in over 1 s, forever, 0.2 s apart.
const LOADER: &str = r#"<svg width="30" height="10" xmlns="http://www.w3.org/2000/svg"><style>
#a { animation: k 1s linear infinite }
#b { animation: k 1s linear 0.2s infinite }
#c { animation: k 1s linear 0.4s infinite }
@keyframes k { from { opacity: 0 } to { opacity: 1 } }
</style><g id="a"><rect width="5" height="5"/></g><g id="b"><rect x="10" width="5" height="5"/></g><g id="c"><rect x="20" width="5" height="5"/></g></svg>"#;

#[component]
fn Loader() -> impl IntoElement {
    view().child(vector_animation().src(Some(Source::Text(LOADER.into()))))
}

/// Each step a composite takes: `to/ms`, loops as `loop(…)`.
fn steps(c: &Composite) -> String {
    match c {
        Composite::Run {
            driver: Driver::Timing {
                to, duration_ms, ..
            },
            ..
        } => format!("{to:.1}/{duration_ms:.0}"),
        Composite::Run { .. } => "run".into(),
        Composite::Sequence(list) => list.iter().map(steps).collect::<Vec<_>>().join(" "),
        Composite::Parallel { children, .. } => {
            format!(
                "[{}]",
                children.iter().map(steps).collect::<Vec<_>>().join(" ")
            )
        }
        Composite::Delay(ms) => format!("wait/{ms:.0}"),
        Composite::Loop {
            child, iterations, ..
        } => format!("loop{iterations:?}({})", steps(child)),
    }
}

#[test]
fn the_native_graph_plays_the_intro_once() {
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(400.0), px(800.0)),
        ..EngineConfig::new(Root::of::<Loader>())
    })
    .expect("the engine starts");
    let seen = Arc::new(Mutex::new(None::<String>));
    let log = seen.clone();
    app.observe(move |_, batch| {
        for item in &batch.items {
            if let MountItem::Animation(AnimationOp::StartComposite { spec, .. }) = item {
                *log.lock().unwrap() = Some(steps(spec));
            }
        }
    });
    let started = seen.clone();
    let _ = app.wait_until(
        "playback started",
        std::time::Duration::from_secs(10),
        move |_| started.lock().unwrap().is_some(),
    );
    let played = seen.lock().unwrap().clone().expect("playback started");
    // To the end once (1.4 s), then from the loop start (0.4 s) to the end.
    assert_eq!(played, "1.4/1400 loopNone(0.4/0 1.4/1000)");
}
