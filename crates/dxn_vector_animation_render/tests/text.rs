//! Live text as the reference draws it: set in the system's fonts, placed
//! by its anchor, spaced, filled and stroked. (Without fonts on the system
//! there is nothing to check.)

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation_render::{RenderOptions, fonts_available, render};

fn pixels(body: &str) -> tiny_skia::Pixmap {
    let svg =
        format!(r#"<svg width="200" height="100" xmlns="http://www.w3.org/2000/svg">{body}</svg>"#);
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    render(&import.scene.frame_at(0.0), &RenderOptions::default()).unwrap()
}

/// The box of the pixels with any ink: left, top, right, bottom.
fn ink(p: &tiny_skia::Pixmap) -> Option<(u32, u32, u32, u32)> {
    let mut b: Option<(u32, u32, u32, u32)> = None;
    for y in 0..p.height() {
        for x in 0..p.width() {
            if p.pixel(x, y).unwrap().alpha() > 64 {
                b = Some(match b {
                    None => (x, y, x, y),
                    Some((l, t, r, bo)) => (l.min(x), t.min(y), r.max(x), bo.max(y)),
                });
            }
        }
    }
    b
}

#[test]
fn text_sits_on_its_baseline_from_its_position() {
    if !fonts_available() {
        return;
    }
    let p = pixels(r#"<text x="10" y="50" font-size="24" font-family="sans-serif">Hello</text>"#);
    let (l, t, r, b) = ink(&p).expect("ink");
    assert!((10..=13).contains(&l), "starts at {l}");
    assert!(r > 50 && r < 90, "ends at {r}");
    // Capitals reach about 0.7 of the size above the baseline; nothing
    // hangs below it.
    assert!((30..=36).contains(&t), "top {t}");
    assert!((48..=50).contains(&b), "bottom {b}");
}

#[test]
fn a_middle_anchor_centers_the_line_on_its_position() {
    if !fonts_available() {
        return;
    }
    let p = pixels(
        r#"<text x="100" y="50" font-size="20" text-anchor="middle">Middle <tspan font-weight="bold">line</tspan></text>"#,
    );
    let (l, _, r, _) = ink(&p).expect("ink");
    let center = f64::from(l + r) / 2.0;
    assert!((center - 100.0).abs() < 3.0, "{l}..{r}");
}

#[test]
fn letter_spacing_spreads_the_characters() {
    if !fonts_available() {
        return;
    }
    let tight = ink(&pixels(
        r#"<text x="10" y="50" font-size="20">spaced</text>"#,
    ))
    .unwrap();
    let wide = ink(&pixels(
        r#"<text x="10" y="50" font-size="20" letter-spacing="5">spaced</text>"#,
    ))
    .unwrap();
    // Five gaps between six characters.
    let grown = i64::from(wide.2) - i64::from(tight.2);
    assert!((23..=27).contains(&grown), "{grown}");
}

#[test]
fn a_stroked_text_draws_its_outline() {
    if !fonts_available() {
        return;
    }
    let p = pixels(
        r##"<text x="10" y="70" font-size="60" font-weight="bold" fill="none" stroke="#00f" stroke-width="2">O</text>"##,
    );
    let (l, t, r, b) = ink(&p).expect("ink");
    // The middle of the O is empty.
    let (cx, cy) = ((l + r) / 2, (t + b) / 2);
    assert_eq!(p.pixel(cx, cy).unwrap().alpha(), 0);
}
