//! Tracks that repeat (looping animations of different lengths over the
//! timeline they share): the graph reads one repetition, modulo its
//! period, with a breakpoint a keyframe of it, and gives what the track
//! gives everywhere, before it and after it too.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{Animated, Color, Easing, Keyframe, Track};
use dxn_vector_animation_nsh::graph::{Expr, Graph, Num, Paint, TIME};

/// A repetition of `period` seconds from `start`, `times` of it, as an
/// importer unrolls it: each repetition's end where the next starts.
fn unrolled<T: Clone>(start: f64, period: f64, times: usize, one: &[(f64, T, Easing)]) -> Track<T> {
    let mut keys = Vec::new();
    for k in 0..times {
        let at = start + period * k as f64;
        for (x, v, e) in one {
            keys.push(Keyframe::new(at + period * x, v.clone(), e.clone()));
        }
    }
    Track::new(keys)
}

fn breakpoints(g: &Graph) -> usize {
    g.nodes()
        .iter()
        .map(|n| match n {
            Expr::Interp { inputs, .. } | Expr::InterpColor { inputs, .. } => inputs.len(),
            _ => 0,
        })
        .sum()
}

#[test]
fn a_repeating_track_is_one_repetition_read_modulo_its_period() {
    let one = [
        (0.0, -5.0, Easing::Linear),
        (
            0.25,
            10.0,
            Easing::CubicBezier {
                x1: 0.4,
                y1: 0.0,
                x2: 0.2,
                y2: 1.0,
            },
        ),
        (0.5, 2.0, Easing::Hold),
        (0.75, 7.0, Easing::Linear),
        (1.0, -5.0, Easing::Linear),
    ];
    let track = Animated::Track(unrolled(0.5, 1.11, 700, &one));
    let mut g = Graph::default();
    let Num::Node(n) = g.track(&track, TIME, |v| v) else {
        panic!("a node");
    };
    assert!(
        breakpoints(&g) < 20,
        "{} breakpoints for 3500 keyframes",
        breakpoints(&g)
    );
    let end = 0.5 + 1.11 * 700.0;
    for k in 0..20_000 {
        let t = -1.0 + (end + 2.0) * f64::from(k) / 20_000.0;
        let (ours, theirs) = (g.eval(n, t), track.sample(t));
        assert!(
            (ours - theirs).abs() < 1e-3,
            "at {t}: {ours} against {theirs}"
        );
    }
}

#[test]
fn a_repeating_color_track_is_one_repetition_too() {
    let (orange, white) = (
        Color::rgba(1.0, 0.65, 0.0, 1.0),
        Color::rgba(1.0, 1.0, 1.0, 1.0),
    );
    let one = [
        (0.0, orange, Easing::Linear),
        (0.4, orange, Easing::Linear),
        (0.6, white, Easing::Linear),
        (1.0, orange, Easing::Linear),
    ];
    let track = Animated::Track(unrolled(0.0, 1.11, 700, &one));
    let mut g = Graph::default();
    let Paint::Node(n) = g.color_track(&track, TIME, 1.0) else {
        panic!("a node");
    };
    assert!(breakpoints(&g) < 20);
    for k in 0..5_000 {
        let t = 777.0 * f64::from(k) / 5_000.0;
        let ours = dxn_vector_animation_nsh::graph::unpack(g.eval_color(n, t));
        let theirs = track.sample(t);
        assert!(
            (ours.g - theirs.g).abs() < 0.01 && (ours.b - theirs.b).abs() < 0.01,
            "at {t}: {ours:?} against {theirs:?}"
        );
    }
}

#[test]
fn a_track_that_stops_short_of_a_whole_repetition_is_not_folded() {
    // It ends halfway through its last repetition: the end holds a value
    // a repetition does not start with.
    let one = [
        (0.0, 0.0, Easing::Linear),
        (0.5, 1.0, Easing::Linear),
        (1.0, 0.0, Easing::Linear),
    ];
    let mut keys = unrolled(0.0, 1.0, 10, &one).keyframes;
    keys.pop();
    let track = Animated::Track(Track::new(keys));
    let mut g = Graph::default();
    let Num::Node(n) = g.track(&track, TIME, |v| v) else {
        panic!("a node");
    };
    for t in [9.5, 9.75, 10.0, 12.0] {
        assert!((g.eval(n, t) - track.sample(t)).abs() < 1e-3, "at {t}");
    }
}

#[test]
fn css_loops_of_different_lengths_play_as_the_reference_draws_them() {
    // Three loops whose timeline is their lengths' least common multiple
    // (7 s, 3 s and 1.11 s: 777 s).
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64">
<style>
@keyframes a { 0% { transform: translate(-5px, 0) } 50% { transform: translate(10px, 0) } 100% { transform: translate(-5px, 0) } }
@keyframes b { 0% { opacity: 1 } 50% { opacity: 0.2 } 100% { opacity: 1 } }
@keyframes c { 0% { fill: orange } 40% { fill: orange } 60% { fill: white } 100% { fill: orange } }
#a { animation: a 7s linear infinite }
#b { animation: b 3s ease-in-out infinite }
#c { animation: c 1.11s linear infinite }
</style>
<rect width="64" height="64" fill="navy"/>
<g id="a"><rect id="b" x="10" y="10" width="30" height="20" fill="teal"/></g>
<circle id="c" cx="40" cy="44" r="12" fill="orange"/>
</svg>"#;
    let scene = dxn_vector_animation_svg::read(svg.as_bytes())
        .unwrap()
        .scene;
    assert!(
        scene.timeline.duration > 700.0,
        "{}",
        scene.timeline.duration
    );
    let plan =
        dxn_vector_animation_nsh::plan::compile(&scene, dxn_vector_animation::Matrix::IDENTITY)
            .unwrap();
    assert!(
        breakpoints(&plan.graph) < 200,
        "{} breakpoints",
        breakpoints(&plan.graph)
    );
    common::assert_plan_matches(
        &scene,
        &[0.0, 1.3, 2.9, 100.05, 388.4, 776.5, 777.0],
        64,
        64,
    );
}
