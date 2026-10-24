//! Radial gradients: their focal point, with and without a highlight.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Node, Paint, RadialGradient};
use serde_json::json;

/// A rectangle filled with a radial gradient whose circle moves (eased),
/// with highlight `h`.
fn radial(h: f64) -> RadialGradient {
    let eased = |from: [f64; 2], to: [f64; 2]| {
        json!({ "a": 1, "k": [
            { "t": 0, "s": from, "o": { "x": 0.7, "y": 0 }, "i": { "x": 0.3, "y": 1 } },
            { "t": 20, "s": to } ] })
    };
    let shapes = json!([{ "ty": "rc", "p": { "a": 0, "k": [0, 0] }, "s": { "a": 0, "k": [80, 80] }, "r": { "a": 0, "k": 0 } },
        { "ty": "gf", "t": 2, "o": { "a": 0, "k": 100 }, "s": eased([10.0, 10.0], [40.0, 30.0]), "e": eased([50.0, 10.0], [40.0, 90.0]),
          "h": { "a": 0, "k": h }, "a": { "a": 0, "k": 30 },
          "g": { "p": 2, "k": { "a": 0, "k": [0, 1, 1, 1, 1, 0, 0, 1] } } }]);
    let data = json!({ "v": "5.7.0", "fr": 10, "ip": 0, "op": 20, "w": 100, "h": 100, "assets": [],
        "layers": [{ "ty": 4, "ind": 1, "ip": 0, "op": 20, "st": 0,
            "ks": { "a": { "a": 0, "k": [0, 0] }, "p": { "a": 0, "k": [0, 0] }, "s": { "a": 0, "k": [100, 100] },
                    "r": { "a": 0, "k": 0 }, "o": { "a": 0, "k": 100 } },
            "shapes": shapes }] })
    .to_string();
    let scene = dxn_vector_animation_lottie::read(data.as_bytes())
        .unwrap()
        .scene;
    fn find(nodes: &[Node]) -> Option<RadialGradient> {
        nodes.iter().find_map(|n| match n {
            Node::Group(g) => find(&g.children),
            Node::Shape(s) => match s.fill.as_ref().map(|f| &f.paint) {
                Some(Paint::RadialGradient(r)) => Some(r.clone()),
                _ => None,
            },
            _ => None,
        })
    }
    find(&scene.root.children).expect("a radial gradient")
}

#[test]
fn without_a_highlight_the_focal_point_is_the_center() {
    let g = radial(0.0);
    for i in 0..=40 {
        let t = f64::from(i) * 0.05;
        assert_eq!(g.focal.sample(t), g.center.sample(t), "at {t}");
    }
}

#[test]
fn a_highlight_moves_the_focal_point_toward_the_edge() {
    let g = radial(50.0);
    let (c, f, r) = (
        g.center.sample(0.0),
        g.focal.sample(0.0),
        g.radius.sample(0.0),
    );
    let d = (f.x - c.x).hypot(f.y - c.y);
    assert!(
        (d - 0.5 * r).abs() < 1e-6,
        "half the radius out: {d} of {r}"
    );
}
