//! Text: runs shaped with the file's fonts and laid out as Rive lays them
//! out (sizing, wrapping, alignment, origins, line heights, styles), and
//! text keyed over time.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, inked, key, near, scene, ty};

const BLACK: u32 = 0xFF00_0000;
const RED: u32 = 0xFFFF_0000;
const FONT: &[u8] = include_bytes!("../../../fixtures/fonts/KaTeX_SansSerif-Regular.ttf");

/// A 200 × 100 artboard with the font embedded and text 1 at (`x`, `y`)
/// with properties `props`, then its styles (each a size and color; style
/// `k` is object 2 + 3k, its fill and color after it) and its runs (text and
/// style number).
fn text(
    x: f32,
    y: f32,
    props: &[(u16, V)],
    styles: &[(f32, u32)],
    runs: &[(&'static str, u64)],
) -> Riv {
    let mut text_props = vec![
        (key::PARENT_ID, V::U(0)),
        (key::X, V::D(x)),
        (key::Y, V::D(y)),
    ];
    text_props.extend(props.iter().cloned());
    let mut riv = Riv::new()
        .obj(ty::FONT_ASSET, &[(key::NAME, V::S("KaTeX"))])
        .obj(
            ty::FILE_ASSET_CONTENTS,
            &[(key::BYTES, V::Bytes(FONT.to_vec()))],
        )
        .artboard(200.0, 100.0)
        .obj(ty::TEXT, &text_props);
    for (size, color) in styles {
        let style = riv.next_index();
        riv = riv
            .obj(
                ty::TEXT_STYLE_PAINT,
                &[
                    (key::PARENT_ID, V::U(1)),
                    (key::FONT_SIZE, V::D(*size)),
                    (key::FONT_ASSET_ID, V::U(0)),
                ],
            )
            .fill(style, *color);
    }
    for (text, style) in runs {
        riv = riv.obj(
            ty::TEXT_VALUE_RUN,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::RUN_STYLE_ID, V::U(2 + 3 * style)),
                (key::RUN_TEXT, V::S(text)),
            ],
        );
    }
    riv
}

#[test]
fn a_line_is_set_from_its_top_left() {
    // 40 units: the caps reach from about y 12 to 38.
    let s = scene(&text(10.0, 10.0, &[], &[(40.0, BLACK)], &[("Hi", 0)]));
    assert!(inked(&s, 0.0, 10, 20, 20, 40));
    assert!(!inked(&s, 0.0, 0, 0, 200, 10));
    assert!(!inked(&s, 0.0, 80, 0, 200, 100));
}

#[test]
fn fixed_width_text_wraps_and_aligns() {
    // 100 wide, centered: two lines.
    let s = scene(&text(
        50.0,
        0.0,
        &[
            (key::TEXT_SIZING, V::U(2)),
            (key::TEXT_WIDTH, V::D(100.0)),
            (key::TEXT_HEIGHT, V::D(100.0)),
            (key::TEXT_ALIGN, V::U(2)),
        ],
        &[(20.0, BLACK)],
        &[("wrap these words", 0)],
    ));
    assert!(inked(&s, 0.0, 50, 0, 150, 25));
    assert!(inked(&s, 0.0, 50, 25, 150, 50));
    assert!(!inked(&s, 0.0, 0, 0, 50, 100));
}

#[test]
fn runs_keep_their_styles() {
    let s = scene(&text(
        10.0,
        10.0,
        &[],
        &[(30.0, BLACK), (30.0, RED)],
        &[("AA", 0), ("AA", 1)],
    ));
    // Red after black on the line.
    let red = (60..200)
        .step_by(2)
        .any(|x| near(color_at(&s, 0.0, x, 30), [255, 0, 0], 30));
    assert!(red);
}

#[test]
fn the_origin_places_the_box() {
    // Origin at the center of an auto-sized line, placed at (100, 50).
    let s = scene(&text(
        100.0,
        50.0,
        &[
            (key::TEXT_ORIGIN_X, V::D(0.5)),
            (key::TEXT_ORIGIN_Y, V::D(0.5)),
        ],
        &[(30.0, BLACK)],
        &[("HH", 0)],
    ));
    assert!(inked(&s, 0.0, 85, 40, 115, 60));
    assert!(!inked(&s, 0.0, 0, 0, 70, 100));
}

#[test]
fn paragraphs_and_line_heights_space_lines() {
    let s = scene(&text(
        10.0,
        0.0,
        &[(key::PARAGRAPH_SPACING, V::D(20.0))],
        &[(20.0, BLACK)],
        &[("one\ntwo", 0)],
    ));
    assert!(inked(&s, 0.0, 10, 0, 60, 25));
    assert!(inked(&s, 0.0, 10, 40, 60, 70));
}

#[test]
fn keyed_text_changes_over_time() {
    // "1", then from 1 s "1111" (before an animation's first keyframe Rive
    // shows its value: the animation keys "1" at 0).
    let riv = text(10.0, 10.0, &[], &[(30.0, BLACK)], &[("1", 0)])
        .obj(
            ty::LINEAR_ANIMATION,
            &[
                (key::ANIMATION_NAME, V::S("count")),
                (key::FPS, V::U(10)),
                (key::DURATION, V::U(20)),
            ],
        )
        .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(5))])
        .obj(
            ty::KEYED_PROPERTY,
            &[(key::PROPERTY_KEY, V::U(u64::from(key::RUN_TEXT)))],
        )
        .obj(
            ty::KEY_FRAME_STRING,
            &[(key::FRAME, V::U(0)), (key::STRING_VALUE, V::S("1"))],
        )
        .obj(
            ty::KEY_FRAME_STRING,
            &[(key::FRAME, V::U(10)), (key::STRING_VALUE, V::S("1111"))],
        );
    let s = scene(&riv);
    assert!(!inked(&s, 0.5, 40, 0, 200, 100));
    assert!(inked(&s, 1.5, 40, 0, 200, 100));
}
