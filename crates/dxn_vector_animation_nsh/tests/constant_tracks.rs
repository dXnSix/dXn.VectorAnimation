//! Tracks and lookups of one value throughout are constants, not nodes
//! the graph evaluates each frame.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Animated, Color, Easing, Keyframe, Track};
use dxn_vector_animation_nsh::graph::{Graph, Num, Paint, TIME};

#[test]
fn a_track_whose_values_map_alike_is_a_constant() {
    // Opacities above one, clamped.
    let track = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, 1.2, Easing::Linear),
        Keyframe::new(1.0, 1.5, Easing::Hold),
        Keyframe::new(2.0, 1.0, Easing::Linear),
    ]));
    let mut g = Graph::default();
    assert!(matches!(g.track(&track, TIME, |o| o.clamp(0.0, 1.0)), Num::Const(v) if v == 1.0));
    assert_eq!(g.nodes().len(), 1, "the time alone");
}

#[test]
fn a_color_track_of_one_color_is_a_constant() {
    let c = Color::rgba(0.2, 0.4, 0.6, 1.0);
    let track = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, c, Easing::Linear),
        Keyframe::new(1.0, c, Easing::Linear),
    ]));
    let mut g = Graph::default();
    assert!(matches!(g.color_track(&track, TIME, 1.0), Paint::Const(_)));
}

#[test]
fn a_lookup_of_one_output_is_a_constant() {
    let mut g = Graph::default();
    assert!(matches!(
        g.lookup(Num::Node(TIME), &[0.0, 1.0, 2.0], &[3.0, 3.0, 3.0]),
        Num::Const(v) if v == 3.0
    ));
    // Others are nodes still.
    assert!(matches!(
        g.lookup(Num::Node(TIME), &[0.0, 1.0], &[3.0, 4.0]),
        Num::Node(_)
    ));
}
