//! Pattern fills as the reference draws them: the tile drawn once at the
//! pixels it shows at, repeated from its corner, transformed with the
//! shape and the pattern.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation_render::{RenderOptions, render};

fn pixels(svg: &str) -> tiny_skia::Pixmap {
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    render(&import.scene.frame_at(0.0), &RenderOptions::default()).unwrap()
}

fn rgb(p: &tiny_skia::Pixmap, x: u32, y: u32) -> [u8; 3] {
    let c = p.pixel(x, y).unwrap().demultiply();
    [c.red(), c.green(), c.blue()]
}

const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

#[test]
fn a_checkerboard_repeats_from_the_tiles_corner() {
    let p = pixels(
        r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
<defs><pattern id="p" x="5" y="5" width="20" height="20" patternUnits="userSpaceOnUse">
<rect width="10" height="10" fill="#0f0"/><rect x="10" y="10" width="10" height="10" fill="#00f"/></pattern></defs>
<rect width="100" height="100" fill="white"/><rect width="100" height="100" fill="url(#p)"/></svg>"##,
    );
    for (x, y, want) in [
        (10, 10, GREEN),
        (20, 20, BLUE),
        (20, 10, WHITE),
        (50, 50, GREEN),
        (60, 60, BLUE),
        // Before the first tile's corner, the tiles go on.
        (2, 2, BLUE),
        (95, 2, BLUE),
    ] {
        assert_eq!(rgb(&p, x, y), want, "at {x}, {y}");
    }
}

#[test]
fn a_box_pattern_follows_the_shapes_box() {
    // Two tiles across and one down the box: left half green.
    let p = pixels(
        r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
<defs><pattern id="p" width="0.5" height="1" patternContentUnits="objectBoundingBox">
<rect width="0.25" height="1" fill="#0f0"/></pattern></defs>
<rect width="100" height="100" fill="white"/><rect x="20" y="40" width="60" height="20" fill="url(#p)"/></svg>"##,
    );
    for (x, y, want) in [
        (25, 50, GREEN),
        (40, 50, WHITE),
        (55, 50, GREEN),
        (70, 50, WHITE),
        (25, 30, WHITE),
    ] {
        assert_eq!(rgb(&p, x, y), want, "at {x}, {y}");
    }
}

#[test]
fn a_pattern_turns_with_its_transform() {
    // Stripes ten wide, turned a quarter: across, not down.
    let p = pixels(
        r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
<defs><pattern id="p" width="20" height="100" patternUnits="userSpaceOnUse" patternTransform="rotate(90)">
<rect width="10" height="100" fill="#00f"/></pattern></defs>
<rect width="100" height="100" fill="white"/><rect width="100" height="100" fill="url(#p)"/></svg>"##,
    );
    assert_eq!(rgb(&p, 50, 5), BLUE);
    assert_eq!(rgb(&p, 50, 15), WHITE);
    assert_eq!(rgb(&p, 5, 25), BLUE);
    assert_eq!(rgb(&p, 95, 25), BLUE);
}
