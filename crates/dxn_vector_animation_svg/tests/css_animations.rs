//! CSS animation timing: delays, iterations, directions, fill modes, easings.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Group, Node};

fn animated(css_animation: &str, keyframes: &str) -> Group {
    let svg = format!(
        r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg"><style>#a {{ animation: {css_animation} }} @keyframes k {{ {keyframes} }} #b {{ animation: long 4s linear }} @keyframes long {{ to {{ opacity: 1 }} }}</style><g id="a"><rect width="1" height="1"/></g><g id="b"><rect width="1" height="1"/></g></svg>"#
    );
    let scene = dxn_vector_animation_svg::read(svg.as_bytes())
        .unwrap()
        .scene;
    scene
        .root
        .children
        .into_iter()
        .find_map(|n| match n {
            Node::Group(g) if g.id.as_deref() == Some("a") => Some(g),
            _ => None,
        })
        .unwrap()
}

#[test]
fn missing_stops_use_the_static_value() {
    let g = animated("k 1s linear", "50% { opacity: 0 }");
    assert_eq!(g.opacity.sample(0.0), 1.0);
    assert_eq!(g.opacity.sample(0.5), 0.0);
    assert!((g.opacity.sample(0.75) - 0.5).abs() < 1e-9);
}

#[test]
fn delays_and_fill_modes() {
    let g = animated(
        "k 1s linear 1s both",
        "from { opacity: 0.2 } to { opacity: 0.6 }",
    );
    assert_eq!(
        g.opacity.sample(0.5),
        0.2,
        "backwards fill shows the first keyframe"
    );
    assert!((g.opacity.sample(1.5) - 0.4).abs() < 1e-9);
    assert_eq!(g.opacity.sample(3.0), 0.6, "forwards fill keeps the last");
    let none = animated(
        "k 1s linear 1s",
        "from { opacity: 0.2 } to { opacity: 0.6 }",
    );
    assert_eq!(none.opacity.sample(0.5), 1.0);
    assert_eq!(none.opacity.sample(3.0), 1.0);
}

#[test]
fn iterations_and_alternate_direction_unroll() {
    let g = animated(
        "k 1s linear 3 alternate",
        "from { opacity: 0 } to { opacity: 1 }",
    );
    assert!((g.opacity.sample(0.25) - 0.25).abs() < 1e-9);
    assert!(
        (g.opacity.sample(1.25) - 0.75).abs() < 1e-9,
        "the second run goes back"
    );
    assert!((g.opacity.sample(2.25) - 0.25).abs() < 1e-9);
}

#[test]
fn steps_and_linear_functions() {
    let g = animated(
        "k 1s steps(2, end)",
        "from { opacity: 0 } to { opacity: 1 }",
    );
    assert_eq!(g.opacity.sample(0.4), 0.0);
    assert_eq!(g.opacity.sample(0.6), 0.5);
    let l = animated(
        "k 1s linear(0, 0.8 50%, 1)",
        "from { opacity: 0 } to { opacity: 1 }",
    );
    assert!((l.opacity.sample(0.25) - 0.4).abs() < 1e-9);
}

#[test]
fn mismatched_transform_lists_animate_as_matrices() {
    let g = animated(
        "k 1s linear",
        "from { transform: rotate(0deg) } to { transform: translate(10px) rotate(90deg) }",
    );
    assert!(matches!(
        g.transform.as_slice(),
        [dxn_vector_animation::TransformOp::Matrix { .. }]
    ));
}
