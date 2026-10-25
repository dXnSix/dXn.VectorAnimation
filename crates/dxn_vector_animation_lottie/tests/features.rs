//! Motion paths, mattes, nested compositions, gradients, polystars, masks.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{FramePaint, Group, Item, MaskKind, Node, Point, TransformOp};
use serde_json::json;

fn comp(layers: serde_json::Value, assets: serde_json::Value) -> Vec<u8> {
    json!({ "v": "5.7.0", "fr": 10, "ip": 0, "op": 20, "w": 100, "h": 100, "assets": assets, "layers": layers })
        .to_string()
        .into_bytes()
}

fn ks(p: serde_json::Value) -> serde_json::Value {
    json!({ "a": { "a": 0, "k": [0, 0] }, "p": p, "s": { "a": 0, "k": [100, 100] }, "r": { "a": 0, "k": 0 }, "o": { "a": 0, "k": 100 } })
}

fn square(color: [f64; 4]) -> serde_json::Value {
    json!([{ "ty": "rc", "p": { "a": 0, "k": [0, 0] }, "s": { "a": 0, "k": [10, 10] }, "r": { "a": 0, "k": 0 } },
           { "ty": "fl", "c": { "a": 0, "k": color }, "o": { "a": 0, "k": 100 } }])
}

fn first_group(nodes: &[Node]) -> &Group {
    nodes
        .iter()
        .find_map(|n| match n {
            Node::Group(g) => Some(g),
            _ => None,
        })
        .unwrap()
}

#[test]
fn spatial_tangents_make_a_motion_path() {
    let p = json!({ "a": 1, "k": [
        { "t": 0, "s": [0, 50], "to": [33, -30], "ti": [-33, -30], "o": { "x": 0, "y": 0 }, "i": { "x": 1, "y": 1 } },
        { "t": 20, "s": [100, 50] } ] });
    let data = comp(
        json!([{ "ty": 4, "ind": 1, "ip": 0, "op": 20, "st": 0, "ks": ks(p), "shapes": square([1.0, 0.0, 0.0, 1.0]) }]),
        json!([]),
    );
    let scene = dxn_vector_animation_lottie::read(&data).unwrap().scene;
    let layer = first_group(&scene.root.children);
    assert!(
        layer
            .transform
            .iter()
            .any(|op| matches!(op, TransformOp::Motion(_)))
    );
    let Item::Fill { transform, .. } = &scene.frame_at(1.0).items[0] else {
        panic!()
    };
    let mid = transform.apply(Point::ZERO);
    assert!(
        (mid.x - 50.0).abs() < 0.5 && mid.y < 50.0,
        "arcs above the line: {mid:?}"
    );
}

#[test]
fn track_mattes_mask_the_next_layer() {
    let data = comp(
        json!([
            { "ty": 4, "ind": 1, "td": 1, "ip": 0, "op": 20, "st": 0, "ks": ks(json!({ "a": 0, "k": [5, 5] })), "shapes": square([1.0, 1.0, 1.0, 1.0]) },
            { "ty": 4, "ind": 2, "tt": 3, "ip": 0, "op": 20, "st": 0, "ks": ks(json!({ "a": 0, "k": [5, 5] })), "shapes": square([1.0, 0.0, 0.0, 1.0]) }
        ]),
        json!([]),
    );
    let scene = dxn_vector_animation_lottie::read(&data).unwrap().scene;
    assert_eq!(
        scene.root.children.len(),
        1,
        "the matte layer is not drawn itself"
    );
    let wrapper = first_group(&scene.root.children);
    assert_eq!(wrapper.mask.as_ref().unwrap().kind, MaskKind::Luminance);
}

#[test]
fn nested_compositions_remap_time_and_clip() {
    let inner = json!([{ "ty": 4, "ind": 1, "ip": 0, "op": 100, "st": 0,
        "ks": ks(json!({ "a": 1, "k": [ { "t": 0, "s": [0, 0] }, { "t": 10, "s": [100, 0] } ] })),
        "shapes": square([0.0, 1.0, 0.0, 1.0]) }]);
    let data = comp(
        json!([{ "ty": 0, "ind": 1, "refId": "inner", "w": 50, "h": 50, "ip": 0, "op": 20, "st": 0, "ks": ks(json!({ "a": 0, "k": [0, 0] })),
            "tm": { "a": 1, "k": [ { "t": 0, "s": [0.5] }, { "t": 20, "s": [0.5] } ] } }]),
        json!([{ "id": "inner", "layers": inner }]),
    );
    let scene = dxn_vector_animation_lottie::read(&data).unwrap().scene;
    let items = scene.frame_at(1.7).items;
    assert!(matches!(&items[0], Item::PushLayer(l) if l.clip.is_some()));
    let x = items.iter().find_map(|i| match i {
        Item::Fill { transform, .. } => Some(transform.e),
        _ => None,
    });
    assert_eq!(x, Some(50.0), "frozen at the remapped 0.5 s");
}

#[test]
fn gradients_carry_their_alpha_stops() {
    let shapes = json!([{ "ty": "rc", "p": { "a": 0, "k": [0, 0] }, "s": { "a": 0, "k": [10, 10] }, "r": { "a": 0, "k": 0 } },
        { "ty": "gf", "t": 1, "o": { "a": 0, "k": 100 }, "s": { "a": 0, "k": [0, 0] }, "e": { "a": 0, "k": [10, 0] },
          "g": { "p": 2, "k": { "a": 0, "k": [0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 0.2] } } }]);
    let data = comp(
        json!([{ "ty": 4, "ind": 1, "ip": 0, "op": 20, "st": 0, "ks": ks(json!({ "a": 0, "k": [0, 0] })), "shapes": shapes }]),
        json!([]),
    );
    let scene = dxn_vector_animation_lottie::read(&data).unwrap().scene;
    let Item::Fill {
        paint: FramePaint::Linear { stops, .. },
        ..
    } = &scene.frame_at(0.0).items[0]
    else {
        panic!()
    };
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color.a, 1.0);
    assert!((stops[1].color.a - 0.2).abs() < 1e-9);
}

#[test]
fn polystars_and_inverted_masks() {
    let shapes = json!([{ "ty": "sr", "sy": 1, "p": { "a": 0, "k": [50, 50] }, "pt": { "a": 0, "k": 5 }, "or": { "a": 0, "k": 40 }, "ir": { "a": 0, "k": 20 },
        "os": { "a": 0, "k": 0 }, "is": { "a": 0, "k": 0 }, "r": { "a": 0, "k": 0 } },
        { "ty": "fl", "c": { "a": 0, "k": [1, 1, 0, 1] }, "o": { "a": 0, "k": 100 } }]);
    let mask = json!([{ "mode": "a", "inv": true, "o": { "a": 0, "k": 100 },
        "pt": { "a": 0, "k": { "c": true, "v": [[0, 0], [10, 0], [10, 10]], "i": [[0, 0], [0, 0], [0, 0]], "o": [[0, 0], [0, 0], [0, 0]] } } }]);
    let data = comp(
        json!([{ "ty": 4, "ind": 1, "ip": 0, "op": 20, "st": 0, "hasMask": true, "masksProperties": mask,
        "ks": ks(json!({ "a": 0, "k": [0, 0] })), "shapes": shapes }]),
        json!([]),
    );
    let scene = dxn_vector_animation_lottie::read(&data).unwrap().scene;
    let items = scene.frame_at(0.0).items;
    let Item::PushLayer(layer) = &items[0] else {
        panic!("{items:?}")
    };
    assert!(layer.mask.is_some());
    let Some(Item::Fill { path, .. }) = items.get(1) else {
        panic!()
    };
    assert_eq!(path.elements().len(), 12, "a five-point star: ten corners");
}

#[test]
fn unsupported_features_are_reported() {
    let shapes = json!([{ "ty": "rc", "p": { "a": 0, "k": [0, 0] }, "s": { "a": 0, "k": [10, 10] }, "r": { "a": 0, "k": 0 } },
        { "ty": "fl", "c": { "a": 0, "k": [1, 0, 0, 1] }, "o": { "a": 0, "k": 100 } }]);
    let data = comp(
        json!([
        { "ty": 5, "ind": 2, "ip": 0, "op": 20, "st": 0, "ks": ks(json!({ "a": 0, "k": [0, 0] })), "t": {} },
        { "ty": 4, "ind": 1, "ip": 0, "op": 20, "st": 0, "ks": ks(json!({ "a": 0, "k": [0, 0], "x": "wiggle(1,2)" })), "shapes": shapes }]),
        json!([]),
    );
    let import = dxn_vector_animation_lottie::read(&data).unwrap();
    let w = import.warnings.join("\n");
    assert!(
        w.contains("text layers") && w.contains("expressions"),
        "{w}"
    );
}

#[test]
fn sniffs_lottie_and_archives() {
    assert!(dxn_vector_animation_lottie::sniff(
        br#"{"v":"5.7","fr":30,"layers":[]}"#
    ));
    assert!(!dxn_vector_animation_lottie::sniff(b"<svg/>"));
    assert!(dxn_vector_animation_lottie::sniff(b"PK\x03\x04rest"));
    assert!(dxn_vector_animation_lottie::read(b"PK\x03\x04rest").is_err());
}

#[test]
fn sniffs_exports_that_write_their_header_last() {
    let padding = "x".repeat(70_000);
    let doc =
        format!(r#"{{"layers":[],"nm":"{padding}","v":"5.7","fr":30,"ip":0,"op":10,"w":1,"h":1}}"#);
    assert!(dxn_vector_animation_lottie::sniff(doc.as_bytes()));
}

#[test]
fn layers_playing_backwards_show() {
    let p = json!({ "a": 1, "k": [ { "t": 0, "s": [0, 0] }, { "t": 20, "s": [100, 0] } ] });
    let data = comp(
        json!([{ "ty": 4, "ind": 1, "ip": 20, "op": 0, "st": 20, "sr": -1, "ks": ks(p), "shapes": square([1.0, 0.0, 0.0, 1.0]) }]),
        json!([]),
    );
    let scene = dxn_vector_animation_lottie::read(&data).unwrap().scene;
    let items = scene.frame_at(0.5).items;
    let x = items.iter().find_map(|i| match i {
        Item::Fill { transform, .. } => Some(transform.e),
        _ => None,
    });
    // Frame 5 of the scene is frame 15 of the layer: three quarters along.
    assert_eq!(x, Some(75.0));
}

#[test]
fn remapped_time_on_the_out_point_shows_the_frame_before() {
    let inner = json!([{ "ty": 4, "ind": 1, "ip": 0, "op": 10, "st": 0,
        "ks": ks(json!({ "a": 1, "k": [ { "t": 0, "s": [0, 0] }, { "t": 10, "s": [100, 0] } ] })),
        "shapes": square([0.0, 1.0, 0.0, 1.0]) }]);
    let data = comp(
        json!([{ "ty": 0, "ind": 1, "refId": "inner", "ip": 0, "op": 10, "st": 0, "ks": ks(json!({ "a": 0, "k": [0, 0] })),
            "tm": { "a": 0, "k": 1.0 } }]),
        json!([{ "id": "inner", "layers": inner }]),
    );
    let scene = dxn_vector_animation_lottie::read(&data).unwrap().scene;
    let x = scene.frame_at(0.5).items.iter().find_map(|i| match i {
        Item::Fill { transform, .. } => Some(transform.e),
        _ => None,
    });
    assert_eq!(x, Some(90.0), "frame 9, not nothing at frame 10");
}

#[test]
fn unknown_blend_modes_are_named() {
    let data = comp(
        json!([{ "ty": 4, "ind": 1, "bm": 28, "ip": 0, "op": 20, "st": 0, "ks": ks(json!({ "a": 0, "k": [0, 0] })), "shapes": square([1.0, 0.0, 0.0, 1.0]) }]),
        json!([]),
    );
    let w = dxn_vector_animation_lottie::read(&data)
        .unwrap()
        .warnings
        .join("\n");
    assert!(w.contains("blend mode 28"), "{w}");
}
