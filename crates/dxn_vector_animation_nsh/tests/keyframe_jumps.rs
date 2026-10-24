//! Keyframes at one time: the graph holds the last of them there, as the
//! scene does (a loop starting again, a value set at once).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Animated, Color, Easing, Keyframe, Track};
use dxn_vector_animation_nsh::graph::{Graph, Num, Paint, TIME};

#[test]
fn numbers_take_the_last_keyframe_at_its_time() {
    let track = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, 0.0, Easing::Linear),
        Keyframe::new(1.5, 42.0, Easing::Linear),
        Keyframe::new(1.5, 0.0, Easing::Linear),
        Keyframe::new(3.0, 42.0, Easing::Linear),
    ]));
    let mut g = Graph::default();
    let Num::Node(n) = g.track(&track, TIME, |v| v) else {
        panic!("a node");
    };
    for t in [0.0, 0.75, 1.4999, 1.5, 1.5001, 2.25, 3.0] {
        assert!(
            (g.eval(n, t) - track.sample(t)).abs() < 1e-3,
            "at {t}: {} against {}",
            g.eval(n, t),
            track.sample(t)
        );
    }
}

#[test]
fn colors_take_the_last_keyframe_at_its_time() {
    let (red, blue) = (
        Color::rgba(1.0, 0.0, 0.0, 1.0),
        Color::rgba(0.0, 0.0, 1.0, 1.0),
    );
    let track = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, red, Easing::Linear),
        Keyframe::new(1.0, blue, Easing::Linear),
        Keyframe::new(1.0, red, Easing::Linear),
        Keyframe::new(2.0, blue, Easing::Linear),
    ]));
    let mut g = Graph::default();
    let Paint::Node(n) = g.color_track(&track, TIME, 1.0) else {
        panic!("a node");
    };
    assert_eq!(
        g.eval_color(n, 1.0),
        dxn_vector_animation_nsh::graph::pack(red)
    );
}
