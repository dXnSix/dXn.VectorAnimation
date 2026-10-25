//! Pattern fills play on the canvas: the outline clips and the tiles over
//! it draw their still content; a pattern in the shape's box, with its own
//! transform, Figma's image fills. Moving content, moving boxes, strokes
//! and too many tiles are drawn as the reference draws them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{Matrix, Scene};
use dxn_vector_animation_nsh::plan;

fn read(body: &str) -> Scene {
    let svg = format!(
        r##"<svg width="100" height="100" viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
<rect width="100" height="100" fill="#f4f0e8"/>{body}</svg>"##
    );
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    import.scene
}

fn assert_native(scene: &Scene, by: u8, within: f64) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    for t in [0.0, 0.5, 1.0] {
        let share = common::differing_by(
            &common::plan_pixels(scene, t, 100, 100),
            &common::reference_pixels(scene, t, 100, 100),
            by,
        );
        assert!(share <= within, "at {t}: {:.2}% differ", share * 100.0);
    }
}

const CHECKS: &str = r##"<defs><pattern id="p" x="3" y="3" width="12" height="12" patternUnits="userSpaceOnUse" patternTransform="rotate(20)">
<rect width="6" height="6" fill="#2050c0"/><circle cx="9" cy="9" r="3" fill="#e04020"/></pattern></defs>"##;

#[test]
fn a_tiling_fills_a_moving_shape() {
    let scene = read(&format!(
        r#"{CHECKS}<g><animateTransform attributeName="transform" type="translate" from="0 0" to="20 10" dur="1s" fill="freeze"/>
<circle cx="45" cy="45" r="35" fill="url(#p)" fill-opacity="0.8"/></g>"#
    ));
    // The reference draws a tile once and resamples it (turned here):
    // softer edges than the canvas draws, the same tiles.
    assert_native(&scene, 96, 0.01);
}

#[test]
fn a_pattern_in_the_shapes_box_and_figmas_image_fill() {
    let scene = read(
        r##"<defs><pattern id="b" width="0.25" height="0.5" patternContentUnits="objectBoundingBox">
<rect width="0.125" height="0.25" fill="#10a040"/></pattern></defs>
<rect x="10" y="10" width="80" height="40" fill="url(#b)"/>"##,
    );
    assert_native(&scene, 24, 0.01);
    // Figma's image fill: one tile, the image scaled into the box.
    let png = {
        let mut p = tiny_skia::Pixmap::new(4, 2).unwrap();
        p.fill(tiny_skia::Color::from_rgba8(200, 80, 40, 255));
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(p.encode_png().unwrap())
    };
    let scene = read(&format!(
        r##"<rect x="10" y="60" width="80" height="30" fill="url(#pattern0)"/>
<defs><pattern id="pattern0" patternContentUnits="objectBoundingBox" width="1" height="1">
<use xlink:href="#image0" transform="scale(0.25 0.5)"/></pattern>
<image id="image0" width="4" height="2" xlink:href="data:image/png;base64,{png}"/></defs>"##
    ));
    assert_native(&scene, 24, 0.01);
}

#[test]
fn what_patterns_the_canvas_does_not_draw_is_drawn_as_the_reference_draws_it() {
    let moving = read(
        r##"<defs><pattern id="m" width="10" height="10" patternUnits="userSpaceOnUse">
<rect width="5" height="5" fill="#2050c0"><animate attributeName="width" from="2" to="8" dur="1s"/></rect></pattern></defs>
<rect width="100" height="100" fill="url(#m)"/>"##,
    );
    let plan = plan::compile(&moving, Matrix::IDENTITY).unwrap();
    assert_eq!(
        plan.sampled,
        vec!["a pattern whose content moves".to_owned()]
    );
    let fine = read(
        r##"<defs><pattern id="f" width="2" height="2" patternUnits="userSpaceOnUse"><rect width="1" height="1"/></pattern></defs>
<rect width="100" height="100" fill="url(#f)"/>"##,
    );
    let plan = plan::compile(&fine, Matrix::IDENTITY).unwrap();
    assert_eq!(plan.sampled, vec!["a pattern of many tiles".to_owned()]);
}
