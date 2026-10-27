//! JSON values (an app's imports of `.json` files) are read as they are,
//! to the scene their text reads to.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation_nsh::source::{read, read_json};

const LOTTIE: &[u8] = include_bytes!("../../../fixtures/lottie-motion.json");
const FIGMA: &[u8] = include_bytes!("../../../fixtures/figma-motion.svg");

fn parsed(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn a_lottie_value_reads_as_its_text() {
    let (text, value) = (read(LOTTIE).unwrap(), read_json(&parsed(LOTTIE)).unwrap());
    assert_eq!(text.scene, value.scene);
    assert_eq!(text.warnings, value.warnings);
}

#[test]
fn a_compiled_scene_value_reads_as_its_text() {
    let scene = read(FIGMA).unwrap().scene;
    let json = dxn_vector_animation::compiled::to_json(&scene, false).unwrap();
    let (text, value) = (
        read(json.as_bytes()).unwrap(),
        read_json(&parsed(json.as_bytes())).unwrap(),
    );
    assert_eq!(text.scene, scene);
    assert_eq!(value.scene, scene);
    // Of a newer version: refused as its text is.
    let newer = json.replace("\"version\":1", "\"version\":9");
    assert_eq!(
        read_json(&parsed(newer.as_bytes())).unwrap_err(),
        read(newer.as_bytes()).unwrap_err()
    );
}

#[test]
fn a_value_that_is_neither_is_refused() {
    for v in [serde_json::json!([1, 2]), serde_json::json!("<svg/>")] {
        let text = serde_json::to_vec(&v).unwrap();
        assert_eq!(read_json(&v).unwrap_err(), read(&text).unwrap_err());
    }
}

#[test]
fn a_lottie_value_with_expressions_reads_as_its_text() {
    // A square moved by an expression, baked into the value read.
    let doc = serde_json::json!({ "v": "5.7.0", "fr": 10, "ip": 0, "op": 20, "w": 100, "h": 80,
        "layers": [{ "ty": 4, "ind": 1, "ip": 0, "op": 20, "st": 0,
            "ks": { "p": { "a": 0, "k": [10, 10, 0], "x": "[time * 30, 10]" } },
            "shapes": [{ "ty": "rc", "p": { "a": 0, "k": [0, 0] }, "s": { "a": 0, "k": [10, 10] }, "r": { "a": 0, "k": 0 } },
                       { "ty": "fl", "c": { "a": 0, "k": [1, 0, 0, 1] }, "o": { "a": 0, "k": 100 } }] }] });
    let text = serde_json::to_vec(&doc).unwrap();
    let (text, value) = (read(&text).unwrap(), read_json(&doc).unwrap());
    assert!(
        text.scene
            .root
            .children
            .iter()
            .any(|n| matches!(n, dxn_vector_animation::Node::Group(g)
            if g.transform.iter().any(dxn_vector_animation::TransformOp::is_animated))),
        "the expression is baked"
    );
    assert_eq!(text.scene, value.scene);
    assert_eq!(text.warnings, value.warnings);
}
