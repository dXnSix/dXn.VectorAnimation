//! Repeaters: what comes before them in their group in copies, each
//! through the repeater's transform once more (offset included), faded
//! from the start opacity to the end one, later copies above or below.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{FramePaint, Import, Item, Scene};

/// A shape layer of group items `items` (JSON) with no transform.
fn read(items: &str) -> Import {
    let json = format!(
        r#"{{"v":"5.7.0","fr":10,"ip":0,"op":20,"w":200,"h":100,"layers":[
{{"ty":4,"ind":1,"ip":0,"op":20,"st":0,
 "ks":{{"o":{{"a":0,"k":100}},"p":{{"a":0,"k":[0,0]}},"a":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[100,100]}},"r":{{"a":0,"k":0}}}},
 "shapes":[{items}]}}]}}"#
    );
    dxn_vector_animation_lottie::read(json.as_bytes()).unwrap()
}

const SQUARE: &str =
    r#"{"ty":"rc","p":{"a":0,"k":[5,5]},"s":{"a":0,"k":[10,10]},"r":{"a":0,"k":0}}"#;
const RED: &str = r#"{"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}}"#;

fn repeater(copies: &str, offset: &str, composite: u8, tr: &str) -> String {
    format!(r#"{{"ty":"rp","c":{copies},"o":{offset},"m":{composite},"tr":{{"ty":"tr",{tr}}}}}"#)
}

fn still(v: f64) -> String {
    format!(r#"{{"a":0,"k":{v}}}"#)
}

/// The fills of a frame: where each one's path starts (x, y), and its
/// alpha, bottom first.
fn fills(scene: &Scene, t: f64) -> Vec<(f64, f64, f64)> {
    let frame = scene.frame_at(t);
    let mut out = Vec::new();
    let mut opacity = vec![1.0];
    for item in &frame.items {
        match item {
            Item::PushLayer(layer) => {
                let o = opacity.last().copied().unwrap_or(1.0) * layer.opacity;
                opacity.push(o);
            }
            Item::PopLayer => {
                opacity.pop();
            }
            Item::Fill {
                path,
                transform,
                paint: FramePaint::Solid(c),
                ..
            } => {
                let b = path.transformed(*transform).bounds().unwrap();
                out.push((
                    (b.x * 1000.0).round() / 1000.0,
                    (b.y * 1000.0).round() / 1000.0,
                    c.a * opacity.last().copied().unwrap_or(1.0),
                ));
            }
            _ => {}
        }
    }
    out
}

const STEP_RIGHT: &str = r#""p":{"a":0,"k":[20,0]},"a":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"so":{"a":0,"k":100},"eo":{"a":0,"k":100}"#;

#[test]
fn copies_step_through_the_transform() {
    let import = read(&format!(
        "{SQUARE},{RED},{}",
        repeater(&still(3.0), &still(0.0), 1, STEP_RIGHT)
    ));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let xs: Vec<f64> = fills(&import.scene, 0.0).iter().map(|f| f.0).collect();
    // Above: the last copy on top (last drawn).
    assert_eq!(xs, vec![0.0, 20.0, 40.0]);
}

#[test]
fn copies_below_and_an_offset() {
    let import = read(&format!(
        "{SQUARE},{RED},{}",
        repeater(&still(3.0), &still(1.0), 2, STEP_RIGHT)
    ));
    let xs: Vec<f64> = fills(&import.scene, 0.0).iter().map(|f| f.0).collect();
    assert_eq!(xs, vec![60.0, 40.0, 20.0]);
}

#[test]
fn copies_fade_from_start_to_end() {
    let tr = STEP_RIGHT
        .replace(r#""so":{"a":0,"k":100}"#, r#""so":{"a":0,"k":20}"#)
        .replace(r#""eo":{"a":0,"k":100}"#, r#""eo":{"a":0,"k":80}"#);
    let import = read(&format!(
        "{SQUARE},{RED},{}",
        repeater(&still(4.0), &still(0.0), 1, &tr)
    ));
    let alphas: Vec<f64> = fills(&import.scene, 0.0)
        .iter()
        .map(|f| (f.2 * 100.0).round() / 100.0)
        .collect();
    assert_eq!(alphas, vec![0.2, 0.4, 0.6, 0.8]);
}

#[test]
fn a_moving_offset_turns_the_copies() {
    // Turning 90° a step about (5, 5), the offset going 0 to 1: the first
    // copy turns from where it was to a quarter turn.
    let tr = r#""p":{"a":0,"k":[0,0]},"a":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":90},"so":{"a":0,"k":100},"eo":{"a":0,"k":100}"#;
    let offset = r#"{"a":1,"k":[{"t":0,"s":[0],"o":{"x":[0],"y":[0]},"i":{"x":[1],"y":[1]}},{"t":10,"s":[1]}]}"#;
    let import = read(&format!(
        "{SQUARE},{RED},{}",
        repeater(&still(1.0), offset, 1, tr)
    ));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(fills(&import.scene, 0.0)[0].0, 0.0);
    // A quarter turn about the origin takes the square to x from -10.
    let at_end = fills(&import.scene, 1.0)[0];
    assert!((at_end.0 + 10.0).abs() < 1e-6, "{at_end:?}");
}

#[test]
fn outlines_styled_after_the_repeater_repeat_too() {
    let import = read(&format!(
        "{SQUARE},{},{RED}",
        repeater(&still(3.0), &still(0.0), 1, STEP_RIGHT)
    ));
    assert_eq!(
        fills(&import.scene, 0.0).len(),
        1,
        "one shape of three paths"
    );
    let shape = match &import.scene.root.children[0] {
        dxn_vector_animation::Node::Group(g) => g
            .children
            .iter()
            .find_map(|n| match n {
                dxn_vector_animation::Node::Shape(s) => Some(s.paths.len()),
                _ => None,
            })
            .unwrap(),
        _ => panic!(),
    };
    assert_eq!(shape, 3);
}

#[test]
fn copies_count_against_the_limits() {
    let json = format!(
        r#"{{"v":"5.7.0","fr":10,"ip":0,"op":20,"w":200,"h":100,"layers":[{{"ty":4,"ind":1,"ip":0,"op":20,"st":0,"ks":{{}},"shapes":[{SQUARE},{RED},{}]}}]}}"#,
        repeater(&still(1000.0), &still(0.0), 1, STEP_RIGHT)
    );
    let limits = dxn_vector_animation::Limits {
        max_nodes: 500,
        ..Default::default()
    };
    assert!(dxn_vector_animation_lottie::read_with(json.as_bytes(), &limits).is_err());
}
