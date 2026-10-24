//! Keyframe tracks.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Animated, Color, Easing, Keyframe, Path, Point, Track};

fn track(keys: &[(f64, f64)], easing: Easing) -> Animated<f64> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|&(t, v)| Keyframe::new(t, v, easing.clone()))
            .collect(),
    ))
}

#[test]
fn holds_before_and_after() {
    let a = track(&[(1.0, 10.0), (2.0, 20.0)], Easing::Linear);
    assert_eq!(a.sample(0.0), 10.0);
    assert_eq!(a.sample(1.5), 15.0);
    assert_eq!(a.sample(5.0), 20.0);
}

#[test]
fn each_segment_has_its_easing() {
    let a = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, 0.0, Easing::Hold),
        Keyframe::new(1.0, 10.0, Easing::Linear),
        Keyframe::new(2.0, 20.0, Easing::Linear),
    ]));
    assert_eq!(a.sample(0.9), 0.0);
    assert_eq!(a.sample(1.0), 10.0);
    assert_eq!(a.sample(1.5), 15.0);
}

#[test]
fn unsorted_keyframes_are_sorted() {
    let a = Animated::Track(Track::new(vec![
        Keyframe::new(2.0, 20.0, Easing::Linear),
        Keyframe::new(0.0, 0.0, Easing::Linear),
    ]));
    assert_eq!(a.sample(1.0), 10.0);
}

#[test]
fn equal_keyframes_make_a_constant() {
    let a = Animated::from_keyframes(
        vec![
            Keyframe::new(0.0, 3.0, Easing::Linear),
            Keyframe::new(1.0, 3.0, Easing::Linear),
        ],
        0.0,
    );
    assert_eq!(a, Animated::Static(3.0));
}

#[test]
fn colors_mix() {
    let a = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, Color::rgba(1.0, 0.0, 0.0, 1.0), Easing::Linear),
        Keyframe::new(1.0, Color::rgba(0.0, 0.0, 1.0, 1.0), Easing::Linear),
    ]));
    let c = a.sample(0.5);
    assert!((c.r - 0.5).abs() < 1e-9 && (c.b - 0.5).abs() < 1e-9);
}

#[test]
fn paths_of_the_same_commands_morph() {
    let p = |x: f64| {
        let mut p = Path::new();
        p.move_to(Point::new(0.0, 0.0));
        p.line_to(Point::new(x, 0.0));
        p
    };
    let a = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, p(10.0), Easing::Linear),
        Keyframe::new(1.0, p(20.0), Easing::Linear),
    ]));
    assert_eq!(a.sample(0.5), p(15.0));
}
