//! Values the graph samples (nonlinear functions of tracks: a skew's
//! tangent, how far a gradient reaches, a mask's luminance): only the
//! samples a line between their neighbors does not stand for are kept.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation_nsh::graph::{Expr, Graph, Num, TIME};

fn breakpoints(g: &Graph) -> usize {
    g.nodes()
        .iter()
        .map(|n| match n {
            Expr::Interp { inputs, .. } => inputs.len(),
            _ => 0,
        })
        .sum()
}

/// Keyframe times a frame apart over three seconds.
fn frames() -> Vec<f64> {
    (0..=180).map(|k| f64::from(k) / 60.0).collect()
}

#[test]
fn a_value_that_holds_is_a_constant() {
    let mut g = Graph::default();
    assert!(matches!(g.sampled(&frames(), TIME, |_| 465.1), Num::Const(v) if v == 465.1));
}

#[test]
fn a_value_that_moves_straight_keeps_its_ends() {
    let mut g = Graph::default();
    let Num::Node(n) = g.sampled(&frames(), TIME, |t| 2.0 * t + 1.0) else {
        panic!("a node");
    };
    assert_eq!(breakpoints(&g), 2);
    for t in [0.0, 0.7, 1.5, 3.0] {
        assert!((g.eval(n, t) - (2.0 * t + 1.0)).abs() < 1e-9);
    }
}

#[test]
fn a_curve_keeps_what_single_precision_tells_apart() {
    // Held for a second, then moving: the hold is one segment.
    let f = |t: f64| 1000.0 / (1.0 + (t - 1.0).max(0.0).powi(2));
    let mut g = Graph::default();
    let Num::Node(n) = g.sampled(&frames(), TIME, f) else {
        panic!("a node");
    };
    assert!(breakpoints(&g) < 180 * 24 * 2 / 3, "{}", breakpoints(&g));
    for k in 0..=3000 {
        let t = 3.0 * f64::from(k) / 3000.0;
        // Within the sampling's own error of the curve (24 samples a
        // keyframe segment), as before.
        assert!((g.eval(n, t) - f(t)).abs() < 1e-2, "at {t}");
    }
}
