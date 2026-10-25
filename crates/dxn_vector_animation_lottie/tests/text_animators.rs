//! Text animators, as lottie-web animates text: range selectors (start,
//! end and offset in percent or by index; characters, words or lines;
//! their shapes, eases and amount; a randomized order) and what they
//! change character by character, several animators together, and text
//! laid along a mask's path.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Color, FramePaint, Import, Item, Limits, Node, Path, PathEl, Scene, TransformOp,
};

/// A square glyph `ch` of advance 60 (at size 100): 0 to 50 wide, 50 up
/// from the baseline.
fn glyph(ch: &str) -> String {
    format!(
        r#"{{"ch":"{ch}","size":100,"style":"Regular","w":60,"fFamily":"Test","data":{{"shapes":[{{"ty":"gr","it":[{{"ty":"sh","ks":{{"a":0,"k":{{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[0,0],[50,0],[50,-50],[0,-50]]}}}}}}]}}]}}}}"#
    )
}

/// A text layer at (0, 0) showing `text` at size 100 in red (with
/// `extra` document fields), animated by `animators` (JSON), laid along
/// path options `path` of masks `masks`, read within `limits`.
fn read_with(
    text: &str,
    extra: &str,
    animators: &str,
    path: &str,
    masks: &str,
    limits: &Limits,
) -> Import {
    let chars = ["a", "b", "c", "d"].map(glyph).join(",");
    // A space: advance 30, nothing drawn.
    let space = r#"{"ch":" ","size":100,"style":"Regular","w":30,"fFamily":"Test","data":{}}"#;
    let json = format!(
        r#"{{"v":"5.7.0","fr":10,"ip":0,"op":100,"w":400,"h":400,
"fonts":{{"list":[{{"fName":"Test-Regular","fFamily":"Test","fStyle":"Regular","ascent":70}}]}},
"chars":[{chars},{space}],
"layers":[{{"ty":5,"ind":1,"ip":0,"op":100,"st":0,
 "ks":{{"o":{{"a":0,"k":100}},"p":{{"a":0,"k":[0,0]}},"a":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[100,100]}},"r":{{"a":0,"k":0}}}},
 "masksProperties":[{masks}],
 "t":{{"d":{{"k":[{{"t":0,"s":{{"s":100,"f":"Test-Regular","t":"{text}","j":0,"tr":0,"lh":120,"fc":[1,0,0]{extra}}}}}]}},
  "p":{{{path}}},"m":{{"g":1,"a":{{"a":0,"k":[0,0]}}}},"a":[{animators}]}}}}]}}"#
    );
    dxn_vector_animation_lottie::read_with(json.as_bytes(), limits).unwrap()
}

fn read(text: &str, animators: &str) -> Import {
    read_with(text, "", animators, "", "", &Limits::default())
}

/// An animator moving characters down by 100 (times how much its selector
/// `selector` selects them).
fn down(selector: &str) -> String {
    format!(r#"{{"s":{selector},"a":{{"p":{{"a":0,"k":[0,100]}}}}}}"#)
}

/// A drawn character: its bounds, color and opacity.
#[derive(Debug, Clone, Copy)]
struct Drawn {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    color: Color,
    alpha: f64,
}

fn round(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// The characters a frame fills, in drawing order.
fn drawn(scene: &Scene, t: f64) -> Vec<Drawn> {
    let frame = scene.frame_at(t);
    let mut out = Vec::new();
    let mut opacity = vec![1.0];
    for item in &frame.items {
        match item {
            Item::PushLayer(layer) => opacity.push(opacity.last().unwrap() * layer.opacity),
            Item::PopLayer => {
                opacity.pop();
            }
            Item::Fill {
                path,
                transform,
                paint: FramePaint::Solid(color),
                ..
            } => {
                // Text that does not move is one fill: a contour a glyph.
                let whole = path.transformed(*transform);
                for contour in whole.0.split_inclusive(|el| matches!(el, PathEl::Close)) {
                    let b = Path(contour.to_vec()).bounds().unwrap();
                    out.push(Drawn {
                        x: round(b.x),
                        y: round(b.y),
                        w: round(b.width),
                        h: round(b.height),
                        color: *color,
                        alpha: round(color.a * opacity.last().unwrap()),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// How far down each character is moved from the baseline (its top is 50
/// above it).
fn drops(scene: &Scene, t: f64) -> Vec<f64> {
    drawn(scene, t).iter().map(|d| round(d.y + 50.0)).collect()
}

/// The groups of the characters, in order.
fn characters(scene: &Scene) -> Vec<&dxn_vector_animation::Group> {
    fn walk<'s>(node: &'s Node, out: &mut Vec<&'s dxn_vector_animation::Group>) {
        if let Node::Group(g) = node {
            if g.children.len() == 1 && matches!(g.children[0], Node::Shape(_)) {
                out.push(g);
            }
            for child in &g.children {
                walk(child, out);
            }
        }
    }
    let mut out = Vec::new();
    for child in &scene.root.children {
        walk(child, &mut out);
    }
    out
}

fn assert_close(a: &[f64], b: &[f64]) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() < 1e-6, "{a:?} vs {b:?}");
    }
}

#[test]
fn a_moving_range_reveals_characters_one_by_one() {
    // The start moves from 0 % to 100 % in a second (ten frames): the
    // characters before it come back up, one after another.
    let selector = r#"{"t":0,"r":1,"b":1,"sh":1,"s":{"a":1,"k":[{"t":0,"s":[0],"o":{"x":[0],"y":[0]},"i":{"x":[1],"y":[1]}},{"t":10,"s":[100]}]},"e":{"a":0,"k":100},"o":{"a":0,"k":0},"a":{"a":0,"k":100}}"#;
    let import = read("abcd", &down(selector));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let scene = &import.scene;
    assert_close(&drops(scene, 0.0), &[100.0, 100.0, 100.0, 100.0]);
    // Frame 2: the start is at 0.8 characters, a fifth of the first left.
    assert_close(&drops(scene, 0.2), &[20.0, 100.0, 100.0, 100.0]);
    // Frame 3: 1.2, the second character partly selected.
    assert_close(&drops(scene, 0.3), &[0.0, 80.0, 100.0, 100.0]);
    assert_close(&drops(scene, 0.5), &[0.0, 0.0, 100.0, 100.0]);
    // Straight between frames.
    assert_close(&drops(scene, 0.25), &[10.0, 90.0, 100.0, 100.0]);
    assert_close(&drops(scene, 1.0), &[0.0, 0.0, 0.0, 0.0]);
    assert_close(&drops(scene, 5.0), &[0.0, 0.0, 0.0, 0.0]);
    // Advanced as laid out.
    let xs: Vec<f64> = drawn(scene, 0.0).iter().map(|d| d.x).collect();
    assert_close(&xs, &[0.0, 60.0, 120.0, 180.0]);
}

#[test]
fn characters_a_selector_never_reaches_stay_constant() {
    // The end moves from 0 % to 50 %: the last two are never selected.
    let selector = r#"{"s":{"a":0,"k":0},"e":{"a":1,"k":[{"t":0,"s":[0]},{"t":10,"s":[50]}]}}"#;
    let import = read("abcd", &down(selector));
    let groups = characters(&import.scene);
    assert_eq!(groups.len(), 4);
    let moves = |g: &dxn_vector_animation::Group| g.transform.iter().any(TransformOp::is_animated);
    assert!(moves(groups[0]) && moves(groups[1]));
    assert!(!moves(groups[2]) && !moves(groups[3]));
    assert_close(&drops(&import.scene, 1.0), &[100.0, 100.0, 0.0, 0.0]);
}

#[test]
fn opacity_by_words() {
    // Words: "ab" and "ab"; the first half of them is transparent.
    let animator = |amount: f64| {
        format!(
            r#"{{"s":{{"b":3,"s":{{"a":0,"k":0}},"e":{{"a":0,"k":50}},"a":{{"a":0,"k":{amount}}}}},"a":{{"o":{{"a":0,"k":0}}}}}}"#
        )
    };
    let import = read("ab ab", &animator(100.0));
    let d = drawn(&import.scene, 0.0);
    assert_eq!(d.len(), 2, "{d:?}");
    // The second word, after the space.
    assert_close(&[d[0].x, d[1].x, d[0].alpha], &[150.0, 210.0, 1.0]);
    // Half the amount: halfway to transparent.
    let import = read("ab ab", &animator(50.0));
    let alphas: Vec<f64> = drawn(&import.scene, 0.0).iter().map(|d| d.alpha).collect();
    assert_close(&alphas, &[0.5, 0.5, 1.0, 1.0]);
}

#[test]
fn each_shape_weights_characters_as_lottie_web_does() {
    let weights = |shape: u8, ease: &str| {
        let selector = format!(
            r#"{{"sh":{shape},"s":{{"a":0,"k":0}},"e":{{"a":0,"k":100}},"o":{{"a":0,"k":0}}{ease}}}"#
        );
        let import = read("abcd", &down(&selector));
        drops(&import.scene, 0.0)
            .iter()
            .map(|d| d / 100.0)
            .collect::<Vec<f64>>()
    };
    let (a, b) = (0.661_437_827_766_147_8, 0.968_245_836_551_854_2);
    let (c, d) = (0.146_446_609_406_726_24, 0.853_553_390_593_273_8);
    assert_close(&weights(1, ""), &[1.0, 1.0, 1.0, 1.0]);
    assert_close(&weights(2, ""), &[0.125, 0.375, 0.625, 0.875]);
    assert_close(&weights(3, ""), &[0.875, 0.625, 0.375, 0.125]);
    assert_close(&weights(4, ""), &[0.25, 0.75, 0.75, 0.25]);
    assert_close(&weights(5, ""), &[a, b, b, a]);
    assert_close(&weights(6, ""), &[c, d, d, c]);
    // Ease low: slower at first (below the straight ramp), still rising.
    let eased = weights(2, r#","ne":{"a":0,"k":80}"#);
    let linear = [0.125, 0.375, 0.625, 0.875];
    assert!(eased.windows(2).all(|w| w[0] < w[1]), "{eased:?}");
    assert!(eased.iter().zip(linear).all(|(e, l)| *e < l), "{eased:?}");
    // Ease high: faster at first.
    let eased = weights(2, r#","xe":{"a":0,"k":80}"#);
    assert!(eased.iter().zip(linear).all(|(e, l)| *e > l), "{eased:?}");
}

#[test]
fn index_units_and_offset() {
    let selector = |offset: f64| {
        format!(r#"{{"r":2,"s":{{"a":0,"k":1}},"e":{{"a":0,"k":3}},"o":{{"a":0,"k":{offset}}}}}"#)
    };
    let import = read("abcd", &down(&selector(0.0)));
    assert_close(&drops(&import.scene, 0.0), &[0.0, 100.0, 100.0, 0.0]);
    let import = read("abcd", &down(&selector(1.0)));
    assert_close(&drops(&import.scene, 0.0), &[0.0, 0.0, 100.0, 100.0]);
}

#[test]
fn rotation_turns_about_the_anchor() {
    // The anchor 25 above the baseline in the middle of the advance:
    // the glyph turns a quarter about it, and moves up by the anchor.
    let animator = r#"{"s":{},"a":{"a":{"a":0,"k":[0,-25]},"r":{"a":0,"k":90}}}"#;
    let import = read("a", animator);
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let d = drawn(&import.scene, 0.0)[0];
    assert_close(&[d.x, d.y, d.w, d.h], &[5.0, -30.0, 50.0, 50.0]);
}

#[test]
fn skew_and_stroke() {
    // A skew of 45 degrees along x leans the top right.
    let import = read(
        "a",
        r#"{"s":{},"a":{"sk":{"a":0,"k":45},"sa":{"a":0,"k":0}}}"#,
    );
    let d = drawn(&import.scene, 0.0)[0];
    assert_close(&[d.x, d.y, d.w, d.h], &[0.0, -50.0, 100.0, 50.0]);
    // Text without a stroke gets the animators' stroke where they select.
    let import = read(
        "ab",
        r#"{"s":{"e":{"a":0,"k":50}},"a":{"sw":{"a":0,"k":4},"sc":{"a":0,"k":[0,1,0,1]}}}"#,
    );
    let frame = import.scene.frame_at(0.0);
    let strokes: Vec<(f64, Color)> = frame
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Stroke {
                style,
                paint: FramePaint::Solid(c),
                ..
            } => Some((style.width, *c)),
            _ => None,
        })
        .collect();
    assert_eq!(
        strokes,
        vec![(4.0, Color::rgba(0.0, 1.0, 0.0, 1.0))],
        "only the first is stroked"
    );
}

#[test]
fn fill_color_mixes_toward_the_animators() {
    let blue = |amount: f64| {
        format!(
            r#"{{"s":{{"e":{{"a":0,"k":50}},"a":{{"a":0,"k":{amount}}}}},"a":{{"fc":{{"a":0,"k":[0,0,1,1]}}}}}}"#
        )
    };
    let colors = |import: &Import| -> Vec<[f64; 3]> {
        drawn(&import.scene, 0.0)
            .iter()
            .map(|d| [round(d.color.r), round(d.color.g), round(d.color.b)])
            .collect()
    };
    let import = read("abcd", &blue(100.0));
    let (b, r) = ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    assert_eq!(colors(&import), vec![b, b, r, r]);
    let import = read("abcd", &blue(50.0));
    assert_eq!(colors(&import)[0], [0.5, 0.0, 0.5]);
    // A third of the way round the hue: red turns green.
    let import = read("a", r#"{"s":{},"a":{"fh":{"a":0,"k":120}}}"#);
    assert_eq!(colors(&import), vec![[0.0, 1.0, 0.0]]);
}

#[test]
fn tracking_spreads_characters() {
    let animator = r#"{"s":{},"a":{"t":{"a":0,"k":10}}}"#;
    // Left: not before the first character of a line.
    let import = read("abcd", animator);
    let xs: Vec<f64> = drawn(&import.scene, 0.0).iter().map(|d| d.x).collect();
    assert_close(&xs, &[0.0, 70.0, 140.0, 210.0]);
    // Centered: still centered on the anchor.
    let import = read_with("ab", r#","j":2"#, animator, "", "", &Limits::default());
    let xs: Vec<f64> = drawn(&import.scene, 0.0).iter().map(|d| d.x).collect();
    assert_close(&xs, &[-65.0, 5.0]);
}

#[test]
fn two_animators_combine() {
    // Positions add up, scales multiply, opacities mix in turn.
    let first = r#"{"s":{},"a":{"p":{"a":0,"k":[0,100]},"o":{"a":0,"k":50}}}"#;
    let second = r#"{"s":{"a":{"a":0,"k":50}},"a":{"p":{"a":0,"k":[10,0]},"o":{"a":0,"k":50},"s":{"a":0,"k":[200,200]}}}"#;
    let import = read("a", &format!("{first},{second}"));
    let d = drawn(&import.scene, 0.0)[0];
    // Scaled by 1.5 about the anchor (30, 0), moved by (5, 100).
    assert_close(&[d.x, d.y, d.w, d.h], &[-10.0, 25.0, 75.0, 75.0]);
    // 1 → 0.5 by the first, then 0.5 + (0.25 − 0.5) × 0.5.
    assert_close(&[d.alpha], &[0.375]);
}

#[test]
fn randomized_order_is_deterministic() {
    // By index, the start moves one character a frame: the characters
    // come back up in a shuffled order, the same every time.
    let selector = r#"{"rn":1,"r":2,"s":{"a":1,"k":[{"t":0,"s":[0]},{"t":4,"s":[4]}]}}"#;
    let order = |import: &Import| -> Vec<usize> {
        (1..=4)
            .map(|frame| {
                let before = drops(&import.scene, (frame - 1) as f64 / 10.0);
                let after = drops(&import.scene, frame as f64 / 10.0);
                (0..4)
                    .find(|&i| before[i] == 100.0 && after[i] == 0.0)
                    .unwrap()
            })
            .collect()
    };
    let first = read("abcd", &down(selector));
    let again = read("abcd", &down(selector));
    assert_eq!(first.scene, again.scene);
    let mut seen = order(&first);
    assert_ne!(seen, vec![0, 1, 2, 3], "shuffled");
    seen.sort_unstable();
    assert_eq!(seen, vec![0, 1, 2, 3], "each once");
}

#[test]
fn wiggly_selectors_are_left_out_with_a_warning() {
    let import = read("ab", &down(r#"{"t":2,"a":{"a":0,"k":100}}"#));
    assert!(
        import.warnings.iter().any(|w| w.contains("wiggly")),
        "{:?}",
        import.warnings
    );
    assert_close(&drops(&import.scene, 0.0), &[0.0, 0.0]);
}

#[test]
fn the_default_expression_selector_selects_by_index() {
    let selector = r#"{"t":1,"a":{"a":0,"k":100,"x":"var $bm_rt;\n$bm_rt = $bm_mul(selectorValue, $bm_div(textIndex, textTotal));"}}"#;
    let import = read("abcd", &down(selector));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_close(&drops(&import.scene, 0.0), &[25.0, 50.0, 75.0, 100.0]);
    // Other expressions are left out, and their animators.
    let selector = r#"{"t":1,"a":{"a":0,"k":100,"x":"$bm_rt = wiggle(1, 50);"}}"#;
    let import = read("ab", &down(selector));
    assert!(
        import.warnings.iter().any(|w| w.contains("expression")),
        "{:?}",
        import.warnings
    );
    assert_close(&drops(&import.scene, 0.0), &[0.0, 0.0]);
}

/// A mask of mode none: a line from (0, 0) down to (0, 400).
const DOWN: &str = r#"{"mode":"n","inv":false,"o":{"a":0,"k":100},"pt":{"a":0,"k":{"c":false,"i":[[0,0],[0,0]],"o":[[0,0],[0,0]],"v":[[0,0],[0,400]]}}}"#;

/// The same line moving right by 100 in a second.
const MOVING: &str = r#"{"mode":"n","inv":false,"o":{"a":0,"k":100},"pt":{"a":1,"k":[{"t":0,"s":[{"c":false,"i":[[0,0],[0,0]],"o":[[0,0],[0,0]],"v":[[0,0],[0,400]]}]},{"t":10,"s":[{"c":false,"i":[[0,0],[0,0]],"o":[[0,0],[0,0]],"v":[[100,0],[100,400]]}]}]}}"#;

fn on_path_at(text: &str, options: &str, mask: &str, t: f64) -> Vec<Drawn> {
    let path = format!(
        r#""m":0,"f":{{"a":0,"k":0}},"l":{{"a":0,"k":0}},"r":{{"a":0,"k":0}},"a":{{"a":0,"k":0}},"p":{{"a":0,"k":1}},{options}"#
    );
    let import = read_with(text, "", "", &path, mask, &Limits::default());
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    drawn(&import.scene, t)
}

fn on_path(text: &str, options: &str) -> Vec<Drawn> {
    on_path_at(text, options, DOWN, 0.0)
}

#[test]
fn text_follows_a_mask_path() {
    // Perpendicular: each character's middle on the line, turned with it.
    let d = on_path("ab", r#""z":0"#);
    assert_close(
        &[d[0].x, d[0].y, d[0].w, d[0].h, d[1].x, d[1].y],
        &[0.0, 0.0, 50.0, 50.0, 0.0, 60.0],
    );
    // Upright.
    let d = on_path("a", r#""p":{"a":0,"k":0}"#);
    assert_close(&[d[0].x, d[0].y], &[-30.0, -20.0]);
    // From a first margin.
    let d = on_path("a", r#""f":{"a":0,"k":100}"#);
    assert_close(&[d[0].x, d[0].y], &[0.0, 100.0]);
    // Backwards, from the end up.
    let d = on_path("a", r#""r":{"a":0,"k":1}"#);
    assert_close(&[d[0].x, d[0].y], &[-50.0, 350.0]);
    // Spread over the path, to the last margin from its end.
    let d = on_path("ab", r#""a":{"a":0,"k":1}"#);
    assert_close(&[d[0].y, d[1].y], &[0.0, 340.0]);
    let d = on_path("ab", r#""a":{"a":0,"k":1},"l":{"a":0,"k":-100}"#);
    assert_close(&[d[0].y, d[1].y], &[0.0, 240.0]);
    // Along a path that moves.
    let d = on_path_at("a", r#""z":0"#, MOVING, 0.5);
    assert_close(&[d[0].x, d[0].y], &[50.0, 0.0]);
}

#[test]
fn keyframes_beyond_the_limit_are_sampled_less_often() {
    // A round selector whose offset moves for ten seconds: every
    // character moves every frame.
    let selector = r#"{"sh":5,"r":2,"s":{"a":0,"k":0},"e":{"a":0,"k":2},"o":{"a":1,"k":[{"t":0,"s":[-2]},{"t":100,"s":[4]}]}}"#;
    let full = read("abcd", &down(selector));
    assert!(full.warnings.is_empty(), "{:?}", full.warnings);
    let limits = Limits {
        max_keyframes: 200,
        ..Limits::default()
    };
    let cut = read_with("abcd", "", &down(selector), "", "", &limits);
    assert!(
        cut.warnings.iter().any(|w| w.contains("less often")),
        "{:?}",
        cut.warnings
    );
    assert!(cut.scene.stats().keyframes <= 200);
    assert!(full.scene.stats().keyframes > cut.scene.stats().keyframes);
}
