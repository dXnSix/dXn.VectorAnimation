//! Lottie's color effects (Tint, Fill, Tritone, Levels) as color
//! effects, as lottie-web reads their parameters; expression controls draw
//! nothing.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Color, ColorMatrix, ColorOp, Effect, Group, Import, Node, Transfer, filter::gradient_color,
};

/// A shape layer with the effects `ef` and layer styles `sy` (JSON).
fn read(ef: &str, sy: &str) -> Import {
    let json = format!(
        r#"{{"v":"5.7.0","fr":30,"ip":0,"op":60,"w":100,"h":100,"layers":[
{{"ty":4,"ind":1,"ip":0,"op":60,"st":0,
 "ks":{{"o":{{"a":0,"k":100}},"p":{{"a":0,"k":[0,0]}},"a":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[100,100]}},"r":{{"a":0,"k":0}}}},
 "ef":{ef},"sy":{sy},
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

/// The color ops of the layer's one color effect.
fn color_ops(import: &Import) -> Vec<ColorOp> {
    let effects = &layer(import).effects;
    let [Effect::Color(c)] = effects.as_slice() else {
        panic!("{effects:?} ({:?})", import.warnings);
    };
    assert!(!c.linear);
    c.ops.clone()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn tint_maps_black_and_white() {
    let import = read(
        r#"[{"ty":20,"nm":"Tint","ef":[
{"ty":2,"nm":"Map Black To","v":{"a":0,"k":[0,0,0.5,1]}},
{"ty":2,"nm":"Map White To","v":{"a":0,"k":[1,1,0.5,1]}},
{"ty":0,"nm":"Amount to Tint","v":{"a":0,"k":50}}]}]"#,
        "[]",
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let ops = color_ops(&import);
    let [
        ColorOp::Tint {
            black,
            white,
            amount,
        },
    ] = ops.as_slice()
    else {
        panic!();
    };
    assert_eq!(black.sample(0.0), Color::rgba(0.0, 0.0, 0.5, 1.0));
    assert_eq!(white.sample(0.0), Color::rgba(1.0, 1.0, 0.5, 1.0));
    assert!(close(amount.sample(0.0), 0.5));
}

#[test]
fn fill_colors_the_layer_at_its_opacity() {
    let import = read(
        r#"[{"ty":21,"nm":"Fill","ef":[
{"ty":10,"nm":"Fill Mask","v":{"a":0,"k":0}},
{"ty":7,"nm":"All Masks","v":{"a":0,"k":0}},
{"ty":2,"nm":"Color","v":{"a":0,"k":[0,1,0,1]}},
{"ty":7,"nm":"Invert","v":{"a":0,"k":0}},
{"ty":0,"nm":"Horizontal Feather","v":{"a":0,"k":0}},
{"ty":0,"nm":"Vertical Feather","v":{"a":0,"k":0}},
{"ty":0,"nm":"Opacity","v":{"a":0,"k":0.5}}]}]"#,
        "[]",
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let ops = color_ops(&import);
    let m = ops[0].matrix_at(0.0).unwrap();
    assert_eq!(m, ColorMatrix::fill(Color::rgba(0.0, 1.0, 0.0, 1.0), 0.5));
}

#[test]
fn tritone_maps_the_luminance_to_three_colors() {
    let import = read(
        r#"[{"ty":23,"nm":"Tritone","ef":[
{"ty":2,"nm":"Highlights","v":{"a":0,"k":[1,1,1,1]}},
{"ty":2,"nm":"Midtones","v":{"a":0,"k":[1,0,0,1]}},
{"ty":2,"nm":"Shadows","v":{"a":0,"k":[0,0,0,1]}},
{"ty":0,"nm":"Blend With Original","v":{"a":0,"k":25}}]}]"#,
        "[]",
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let ops = color_ops(&import);
    let [ColorOp::GradientMap { stops, amount }] = ops.as_slice() else {
        panic!();
    };
    let stops = stops.sample(0.0);
    assert_eq!(gradient_color(&stops, 0.5), Color::rgba(1.0, 0.0, 0.0, 1.0));
    assert_eq!(gradient_color(&stops, 0.0), Color::rgba(0.0, 0.0, 0.0, 1.0));
    assert!(close(amount.sample(0.0), 0.75));
}

#[test]
fn levels_are_tables_of_each_channels_curve() {
    // The composite: inputs 0.2 to 0.8 spread over the whole range; red's
    // output up to 0.5 only.
    let mut ef: Vec<String> = (0..36)
        .map(|i| format!(r#"{{"ty":0,"nm":"p{i}","v":{{"a":0,"k":0}}}}"#))
        .collect();
    let set = |ef: &mut Vec<String>, i: usize, v: f64| {
        ef[i] = format!(r#"{{"ty":0,"nm":"p{i}","v":{{"a":0,"k":{v}}}}}"#);
    };
    for (i, v) in [(3, 0.2), (4, 0.8), (5, 1.0), (6, 0.0), (7, 1.0)] {
        set(&mut ef, i, v);
    }
    for base in [10, 17, 24, 31] {
        for (k, v) in [(0, 0.0), (1, 1.0), (2, 1.0), (3, 0.0), (4, 1.0)] {
            set(&mut ef, base + k, v);
        }
    }
    set(&mut ef, 14, 0.5);
    let import = read(
        &format!(r#"[{{"ty":24,"nm":"Levels","ef":[{}]}}]"#, ef.join(",")),
        "[]",
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let ops = color_ops(&import);
    let [
        ColorOp::Transfer {
            red, green, alpha, ..
        },
    ] = ops.as_slice()
    else {
        panic!();
    };
    assert!(close(green.apply(0.5), 0.5));
    assert!(close(green.apply(0.1), 0.0));
    assert!(close(green.apply(0.9), 1.0));
    assert!((red.apply(0.9) - 0.5).abs() < 1e-6);
    assert!(close(alpha.apply(0.3), 0.3) || matches!(alpha, Transfer::Table { .. }));
}

#[test]
fn expression_controls_draw_nothing() {
    let import = read(
        r#"[{"ty":5,"nm":"Slider Control","ef":[{"ty":0,"nm":"Slider","v":{"a":0,"k":3}}]}]"#,
        "[]",
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert!(layer(&import).effects.is_empty());
}
