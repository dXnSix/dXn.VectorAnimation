//! What plays on events: segments past the timeline's end, marked
//! `on:<event>`, moved to when their events happened.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, Easing, Group, Keyframe, LoopMode, Marker, Scene, Timeline, Track,
};

fn scene(opacity: Animated<f64>) -> Scene {
    Scene {
        width: 10.0,
        height: 10.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Loop,
            markers: vec![
                Marker::new("intro", 0.0, 1.0),
                Marker::new("on:tap", 1.0e6, 1.0),
            ],
            cues: Vec::new(),
        },
        root: Group {
            opacity,
            ..Group::new()
        },
    }
}

fn opacity(scene: &Scene, t: f64) -> f64 {
    scene.root.opacity.sample(t)
}

fn track() -> Animated<f64> {
    Animated::Track(Track::new(vec![
        // On the timeline: 1 to 0.5 over two seconds.
        Keyframe::new(0.0, 1.0, Easing::Linear),
        Keyframe::new(2.0, 0.5, Easing::Hold),
        // On a tap: from 0 back to 1 in a second.
        Keyframe::new(1.0e6, 0.0, Easing::Linear),
        Keyframe::new(1.0e6 + 1.0, 1.0, Easing::Hold),
    ]))
}

#[test]
fn events_are_named_by_their_markers() {
    assert_eq!(scene(track()).events(), vec!["tap"]);
}

#[test]
fn what_plays_on_an_event_moves_to_when_it_happened() {
    let s = scene(track());
    // Without the event the timeline never reaches it.
    assert_eq!(opacity(&s, 1.0), 0.75);
    let tapped = s.with_events(&[("tap", 1.0)]);
    assert_eq!(opacity(&tapped, 0.5), 0.875);
    // From the tap on, what it plays overrides the timeline's.
    assert_eq!(opacity(&tapped, 1.0), 0.0);
    assert_eq!(opacity(&tapped, 1.5), 0.5);
    assert_eq!(opacity(&tapped, 3.0), 1.0);
    // Up to the tap, the timeline's value, then a jump.
    assert!((opacity(&tapped, 1.0 - 1e-6) - 0.75).abs() < 1e-5);
}

#[test]
fn events_the_scene_has_nothing_for_change_nothing() {
    let s = scene(track());
    let same = s.with_events(&[("hover", 1.0)]);
    assert_eq!(same, s);
    // A property without the event's keyframes keeps its own.
    let still = scene(Animated::Static(0.3)).with_events(&[("tap", 1.0)]);
    assert_eq!(opacity(&still, 1.5), 0.3);
}
