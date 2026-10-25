//! Layer effects: Drop Shadow and Gaussian Blur as the scene's effects,
//! converted as lottie-web converts them; others left out with a warning
//! (the color effects are in `color_effects.rs`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Color, Effect, Group, Import, Node};

/// A shape layer with the effects `ef` (JSON).
fn read(ef: &str) -> Import {
    let json = format!(
        r#"{{"v":"5.7.0","fr":30,"ip":0,"op":60,"w":100,"h":100,"layers":[
{{"ty":4,"ind":1,"ip":0,"op":60,"st":0,
 "ks":{{"o":{{"a":0,"k":100}},"p":{{"a":0,"k":[0,0]}},"a":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[100,100]}},"r":{{"a":0,"k":0}}}},
 "ef":{ef},
 "shapes":[{{"ty":"gr","it":[{{"ty":"rc","p":{{"a":0,"k":[50,50]}},"s":{{"a":0,"k":[40,40]}},"r":{{"a":0,"k":0}}}},
 {{"ty":"fl","c":{{"a":0,"k":[1,0,0,1]}},"o":{{"a":0,"k":100}}}},
 {{"ty":"tr","p":{{"a":0,"k":[0,0]}},"a":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[100,100]}},"r":{{"a":0,"k":0}},"o":{{"a":0,"k":100}}}}]}}]}}]}}"#
    );
    dxn_vector_animation_lottie::read(json.as_bytes()).unwrap()
}

fn layer(import: &Import) -> &Group {
    import
        .scene
        .root
        .children
        .iter()
        .find_map(|n| match n {
            Node::Group(g) if g.id.as_deref() == Some("1") => Some(g),
            _ => None,
        })
        .unwrap()
}

const SHADOW: &str = r#"[{"ty":25,"nm":"Drop Shadow","en":1,"ef":[
{"ty":2,"nm":"Shadow Color","v":{"a":0,"k":[0,0,1,1]}},
{"ty":0,"nm":"Opacity","v":{"a":0,"k":127.5}},
{"ty":0,"nm":"Direction","v":{"a":0,"k":180}},
{"ty":0,"nm":"Distance","v":{"a":0,"k":10}},
{"ty":0,"nm":"Softness","v":{"a":0,"k":8}},
{"ty":7,"nm":"Shadow Only","v":{"a":0,"k":0}}]}]"#;

#[test]
fn a_drop_shadow() {
    let import = read(SHADOW);
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let effects = &layer(&import).effects;
    let [Effect::DropShadow(shadow)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    // Half opaque blue, falling down (180° from up) 10, softness 8: a
    // deviation of 2.
    assert_eq!(shadow.color.sample(0.0), Color::rgba(0.0, 0.0, 1.0, 0.5));
    assert!(shadow.offset.x.sample(0.0).abs() < 1e-9);
    assert!((shadow.offset.y.sample(0.0) - 10.0).abs() < 1e-9);
    assert_eq!(shadow.blur.sample(0.0), 2.0);
}

#[test]
fn a_drop_shadow_turning() {
    let ef = SHADOW.replace(
        r#""v":{"a":0,"k":180}"#,
        r#""v":{"a":1,"k":[{"t":0,"s":[90],"o":{"x":[0],"y":[0]},"i":{"x":[1],"y":[1]}},{"t":30,"s":[180]}]}"#,
    );
    let import = read(&ef);
    let [Effect::DropShadow(shadow)] = layer(&import).effects.as_slice() else {
        panic!();
    };
    // At 90° it falls right, at 180° down.
    assert!((shadow.offset.x.sample(0.0) - 10.0).abs() < 1e-6);
    assert!((shadow.offset.y.sample(1.0) - 10.0).abs() < 1e-6);
}

#[test]
fn a_gaussian_blur() {
    let import = read(
        r#"[{"ty":29,"nm":"Gaussian Blur","ef":[{"ty":0,"nm":"Blurriness","v":{"a":0,"k":20}},{"ty":7,"nm":"Blur Dimensions","v":{"a":0,"k":1}},{"ty":7,"nm":"Repeat Edge Pixels","v":{"a":0,"k":0}}]}]"#,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let [Effect::Blur(blur)] = layer(&import).effects.as_slice() else {
        panic!();
    };
    assert!((blur.radius.sample(0.0) - 6.0).abs() < 1e-9);
}

#[test]
fn a_disabled_effect_is_skipped_and_others_warned_of() {
    let import = read(
        r#"[{"ty":29,"en":0,"ef":[{"ty":0,"v":{"a":0,"k":20}}]},{"ty":26,"nm":"Radial Wipe","ef":[]}]"#,
    );
    assert!(layer(&import).effects.is_empty());
    assert!(
        import.warnings.iter().any(|w| w.contains("`Radial Wipe`")),
        "{:?}",
        import.warnings
    );
}
