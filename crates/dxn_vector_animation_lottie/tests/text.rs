//! Text layers drawn with the file's glyphs (`chars`), laid out as
//! lottie-web lays them out; without glyphs they are left out with a
//! warning. Their animators are in `text_animators.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Import, Item, Path, PathEl, Point, Rect, Scene};

/// A square glyph `ch` of advance `w` (at size 100): 0 to 50 wide, 50 up
/// from the baseline.
fn glyph(ch: &str, w: f64) -> String {
    format!(
        r#"{{"ch":"{ch}","size":100,"style":"Regular","w":{w},"fFamily":"Test","data":{{"shapes":[{{"ty":"gr","it":[{{"ty":"sh","ks":{{"a":0,"k":{{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[0,0],[50,0],[50,-50],[0,-50]]}}}}}}]}}]}}}}"#
    )
}

/// A text layer at (0, 0) of documents `docs` (JSON keyframes), with the
/// glyphs `a` and `b` (or none), and text animators `animators`.
fn read(docs: &str, glyphs: bool, animators: &str) -> Import {
    let chars = if glyphs {
        format!(r#","chars":[{},{}]"#, glyph("a", 60.0), glyph("b", 60.0))
    } else {
        String::new()
    };
    let json = format!(
        r#"{{"v":"5.7.0","fr":10,"ip":0,"op":20,"w":400,"h":400,
"fonts":{{"list":[{{"fName":"Test-Regular","fFamily":"Test","fStyle":"Regular","ascent":70}}]}}{chars},
"layers":[{{"ty":5,"ind":1,"ip":0,"op":20,"st":0,
 "ks":{{"o":{{"a":0,"k":100}},"p":{{"a":0,"k":[0,0]}},"a":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[100,100]}},"r":{{"a":0,"k":0}}}},
 "t":{{"d":{{"k":[{docs}]}},"p":{{}},"m":{{"g":1,"a":{{"a":0,"k":[0,0]}}}},"a":[{animators}]}}}}]}}"#
    );
    dxn_vector_animation_lottie::read(json.as_bytes()).unwrap()
}

fn doc(t: f64, text: &str, extra: &str) -> String {
    format!(
        r#"{{"t":{t},"s":{{"s":100,"f":"Test-Regular","t":"{text}","j":0,"tr":0,"lh":120,"fc":[1,0,0]{extra}}}}}"#
    )
}

/// The bounds of each glyph a frame fills, left to right, top to bottom.
fn glyphs(scene: &Scene, t: f64) -> Vec<Rect> {
    let frame = scene.frame_at(t);
    let mut out = Vec::new();
    for item in &frame.items {
        if let Item::Fill {
            path, transform, ..
        } = item
        {
            // One fill of all glyphs: each contour a glyph.
            let whole = path.transformed(*transform);
            let mut contour = Vec::new();
            for el in whole
                .0
                .iter()
                .chain([&PathEl::MoveTo(Point::new(0.0, 0.0))])
            {
                if matches!(el, PathEl::MoveTo(_)) && !contour.is_empty() {
                    out.push(Path(std::mem::take(&mut contour)).bounds().unwrap());
                }
                contour.push(*el);
            }
        }
    }
    out
}

#[test]
fn point_text_advances_glyph_by_glyph() {
    let import = read(&doc(0.0, "aba", ""), true, "");
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let xs: Vec<f64> = glyphs(&import.scene, 0.0).iter().map(|r| r.x).collect();
    assert_eq!(xs, vec![0.0, 60.0, 120.0]);
    // On the baseline, 50 tall.
    let r = glyphs(&import.scene, 0.0)[0];
    assert_eq!((r.y, r.height), (-50.0, 50.0));
}

#[test]
fn centered_lines_break_and_track() {
    // Tracking 100: a tenth of the size between glyphs.
    let import = read(&doc(0.0, "ab\\rb", r#","j":2,"tr":100"#), true, "");
    let g = glyphs(&import.scene, 0.0);
    // Line one: 60 + 10 + 60 - 10 = 120 wide... centered on 0.
    assert_eq!(g[0].x, -65.0);
    assert_eq!(g[1].x, 5.0);
    // Line two 120 down, its glyph centered.
    assert_eq!((g[2].x, g[2].y), (-30.0, 70.0));
}

#[test]
fn box_text_wraps_at_spaces() {
    let import = read(
        &doc(0.0, "ab ab ab", r#","sz":[260,400],"ps":[10,20]"#),
        true,
        "",
    );
    let g = glyphs(&import.scene, 0.0);
    // Two words a line; the first baseline the ascent below the box top.
    assert_eq!(g.len(), 6);
    assert_eq!((g[0].x, g[0].y + 50.0), (10.0, 20.0 + 70.0));
    assert_eq!(g[4].y, g[0].y + 120.0);
    assert_eq!(g[4].x, 10.0);
}

#[test]
fn keyframed_documents_take_turns() {
    let docs = format!("{},{}", doc(0.0, "a", ""), doc(10.0, "abab", ""));
    let import = read(&docs, true, "");
    assert_eq!(glyphs(&import.scene, 0.5).len(), 1);
    assert_eq!(glyphs(&import.scene, 1.5).len(), 4);
}

#[test]
fn text_without_glyphs_is_left_out_with_a_warning() {
    let import = read(&doc(0.0, "ab", ""), false, "");
    assert!(glyphs(&import.scene, 0.0).is_empty());
    assert!(
        import.warnings.iter().any(|w| w.contains("without glyphs")),
        "{:?}",
        import.warnings
    );
    // An animator that changes nothing leaves the text as it is.
    let import = read(&doc(0.0, "ab", ""), true, r#"{"s":{},"a":{}}"#);
    assert_eq!(glyphs(&import.scene, 0.0).len(), 2);
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
}
