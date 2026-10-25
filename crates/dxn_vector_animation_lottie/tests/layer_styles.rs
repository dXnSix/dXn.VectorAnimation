//! Lottie's layer styles: drop and inner shadows (the light from their
//! angle) and color overlays; other styles are left out with a warning.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Color, ColorOp, Effect, Group, Import, Node};

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

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn style(ty: u8, extra: &str) -> String {
    format!(
        r#"[{{"ty":{ty},"nm":"style","c":{{"a":0,"k":[0,0,1,1]}},"o":{{"a":0,"k":50}},"a":{{"a":0,"k":90}},"s":{{"a":0,"k":8}},"d":{{"a":0,"k":10}}{extra}}}]"#
    )
}

#[test]
fn a_drop_shadow_falls_away_from_the_light() {
    let import = read("[]", &style(1, ""));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let effects = &layer(&import).effects;
    let [Effect::DropShadow(s)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    // The light from above (90°): the shadow 10 below, half opaque, a
    // deviation of half the size.
    assert_eq!(s.color.sample(0.0), Color::rgba(0.0, 0.0, 1.0, 0.5));
    assert!(close(s.offset.x.sample(0.0), 0.0));
    assert!(close(s.offset.y.sample(0.0), 10.0));
    assert!(close(s.blur.sample(0.0), 4.0));
}

#[test]
fn an_inner_shadow() {
    let import = read("[]", &style(2, ""));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let [Effect::InnerShadow(s)] = layer(&import).effects.as_slice() else {
        panic!();
    };
    assert!(close(s.offset.y.sample(0.0), 10.0));
    assert!(!s.hard);
}

#[test]
fn a_color_overlay_mixes_its_color_in() {
    let import = read(
        "[]",
        r#"[{"ty":7,"nm":"overlay","c":{"a":0,"k":[0,1,0,1]},"so":{"a":0,"k":40}}]"#,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let effects = &layer(&import).effects;
    let [Effect::Color(c)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let [ColorOp::Overlay { color, amount }] = c.ops.as_slice() else {
        panic!();
    };
    assert_eq!(color.sample(0.0), Color::rgba(0.0, 1.0, 0.0, 1.0));
    assert!(close(amount.sample(0.0), 0.4));
}

#[test]
fn effects_come_before_styles_and_others_are_warned_of() {
    let import = read(
        r#"[{"ty":29,"nm":"Gaussian Blur","ef":[{"ty":0,"v":{"a":0,"k":20}},{"ty":7,"v":{"a":0,"k":1}}]}]"#,
        &style(1, "").replace("[{", r#"[{"ty":3,"nm":"glow"},{"#),
    );
    let effects = &layer(&import).effects;
    assert!(matches!(
        effects.as_slice(),
        [Effect::Blur(_), Effect::DropShadow(_)]
    ));
    assert!(
        import.warnings.iter().any(|w| w.contains("outer glow")),
        "{:?}",
        import.warnings
    );
}
