//! Easing curves.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Easing, StepPosition};

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn linear_and_ends() {
    assert_eq!(Easing::Linear.apply(0.3), 0.3);
    for e in [
        Easing::EASE,
        Easing::EASE_IN,
        Easing::EASE_OUT,
        Easing::EASE_IN_OUT,
    ] {
        assert!(close(e.apply(0.0), 0.0));
        assert!(close(e.apply(1.0), 1.0));
    }
}

#[test]
fn cubic_bezier_matches_css() {
    // Values browsers compute for `ease` and `ease-in-out`.
    assert!(close(Easing::EASE.apply(0.5), 0.8024));
    assert!(close(Easing::EASE_IN_OUT.apply(0.5), 0.5));
    assert!(close(Easing::EASE_IN.apply(0.25), 0.093_464_7));
    // Overshoot (Back easings).
    let back = Easing::cubic_bezier(0.34, 1.56, 0.64, 1.0);
    assert!(back.apply(0.6) > 1.0);
}

#[test]
fn cubic_bezier_on_the_diagonal_is_linear() {
    assert_eq!(Easing::cubic_bezier(0.2, 0.2, 0.8, 0.8), Easing::Linear);
}

#[test]
fn hold_jumps_at_the_end() {
    assert_eq!(Easing::Hold.apply(0.999), 0.0);
    assert_eq!(Easing::Hold.apply(1.0), 1.0);
}

#[test]
fn steps_follow_css() {
    let end = Easing::Steps {
        count: 4,
        position: StepPosition::JumpEnd,
    };
    assert_eq!(end.apply(0.0), 0.0);
    assert_eq!(end.apply(0.3), 0.25);
    assert_eq!(end.apply(1.0), 1.0);
    let start = Easing::Steps {
        count: 4,
        position: StepPosition::JumpStart,
    };
    assert_eq!(start.apply(0.0), 0.25);
    assert_eq!(start.apply(0.3), 0.5);
    let none = Easing::Steps {
        count: 5,
        position: StepPosition::JumpNone,
    };
    assert_eq!(none.apply(0.0), 0.0);
    assert_eq!(none.apply(0.5), 0.5);
    assert_eq!(none.apply(1.0), 1.0);
    let both = Easing::Steps {
        count: 3,
        position: StepPosition::JumpBoth,
    };
    assert_eq!(both.apply(0.0), 0.25);
    assert_eq!(both.apply(1.0), 1.0);
}

#[test]
fn linear_points_interpolate() {
    let e = Easing::LinearPoints {
        points: vec![(0.0, 0.0), (0.5, 0.8), (1.0, 1.0)],
    };
    assert!(close(e.apply(0.25), 0.4));
    assert!(close(e.apply(0.75), 0.9));
}
