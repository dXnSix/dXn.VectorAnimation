//! The gallery's animated SVGs as a browser draws them: the importer and the
//! reference renderer put the same colors where a real browser does.
//!
//! The files (`fixtures/gallery/*.svg`, LottieFiles' exports) differ from
//! their Lottie JSON in places, and a browser draws those places as the
//! file says. So does the reference renderer, and this pins it. The table
//! is points of flat color read off Chromium's frames of the files
//! (headless, SMIL paused and set to the time with `setCurrentTime`, 400 ×
//! 400 pixels on white), about a third of them where the Lottie JSON
//! draws something else:
//!
//! - `alien-button`: the "o" of "Connect" is two paths in the file, each
//!   filled white (the JSON fills one compound path, whose inner contour
//!   cuts the counter out), so a browser draws it a dot; and the file has
//!   neither the purple glow under the button (a drop shadow of 101 and a
//!   blur of 15 in the JSON) nor anything else there.
//! - `navigation-bar-selection`: a bar's colored background is a rect at
//!   `x = 0` whose group moves from -half its width to 0 while its width
//!   grows (the JSON's rectangle moves its center and grows from the left
//!   edge), so a browser draws it half its width off its label.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::Color;
use dxn_vector_animation_render::{RenderOptions, render};

/// File, seconds, x, y (pixels of a 400 × 400 frame), the color a browser
/// draws there.
const POINTS: &[(&str, f64, u32, u32, [u8; 3])] = &[
    ("alien-button", 1.0, 231, 273, [24, 24, 27]),
    ("alien-button", 1.0, 276, 270, [24, 24, 27]),
    ("alien-button", 1.0, 171, 273, [24, 24, 27]),
    ("alien-button", 1.0, 321, 267, [24, 24, 27]),
    ("alien-button", 1.0, 108, 273, [24, 24, 27]),
    ("alien-button", 1.0, 330, 234, [24, 24, 27]),
    ("alien-button", 1.0, 258, 63, [1, 157, 145]),
    ("alien-button", 1.0, 177, 225, [255, 255, 255]),
    ("alien-button", 1.0, 357, 216, [24, 24, 27]),
    ("alien-button", 1.0, 102, 195, [86, 86, 114]),
    ("alien-button", 1.0, 255, 195, [51, 51, 56]),
    ("alien-button", 1.0, 198, 234, [46, 46, 50]),
    ("alien-button", 1.0, 273, 147, [158, 252, 160]),
    ("alien-button", 1.0, 276, 90, [129, 122, 130]),
    ("alien-button", 1.0, 258, 159, [146, 230, 91]),
    ("alien-button", 3.0, 330, 243, [24, 24, 27]),
    ("alien-button", 3.0, 189, 270, [24, 24, 27]),
    ("alien-button", 3.0, 321, 267, [24, 24, 27]),
    ("alien-button", 3.0, 279, 270, [24, 24, 27]),
    ("alien-button", 3.0, 231, 270, [24, 24, 27]),
    ("alien-button", 3.0, 348, 66, [1, 157, 145]),
    ("alien-button", 3.0, 153, 270, [24, 24, 27]),
    ("alien-button", 3.0, 117, 270, [24, 24, 27]),
    ("alien-button", 3.0, 126, 99, [24, 24, 27]),
    ("alien-button", 3.0, 228, 210, [50, 50, 55]),
    ("alien-button", 3.0, 171, 222, [255, 255, 255]),
    ("alien-button", 3.0, 192, 240, [45, 45, 49]),
    ("alien-button", 3.0, 114, 195, [86, 86, 113]),
    ("alien-button", 3.0, 297, 180, [146, 230, 91]),
    ("alien-button", 3.0, 348, 96, [129, 122, 130]),
    ("alien-button", 3.0, 318, 156, [158, 252, 160]),
    ("navigation-bar-selection", 1.2, 330, 159, [255, 92, 1]),
    ("navigation-bar-selection", 1.2, 174, 189, [255, 255, 255]),
    ("navigation-bar-selection", 1.2, 372, 162, [255, 92, 1]),
    ("navigation-bar-selection", 1.2, 144, 186, [255, 255, 255]),
    ("navigation-bar-selection", 1.2, 162, 150, [255, 255, 255]),
    ("navigation-bar-selection", 1.2, 315, 183, [255, 92, 1]),
    ("navigation-bar-selection", 1.2, 381, 189, [255, 92, 1]),
    ("navigation-bar-selection", 1.2, 195, 150, [255, 255, 255]),
    ("navigation-bar-selection", 1.2, 69, 195, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 261, 228, [255, 255, 255]),
    ("navigation-bar-selection", 1.2, 81, 285, [133, 175, 37]),
    ("navigation-bar-selection", 1.2, 105, 177, [85, 30, 0]),
    ("navigation-bar-selection", 1.2, 90, 96, [157, 157, 255]),
    ("navigation-bar-selection", 1.2, 96, 213, [255, 205, 4]),
    ("navigation-bar-selection", 1.2, 297, 186, [255, 92, 1]),
    ("navigation-bar-selection", 1.2, 99, 282, [59, 78, 15]),
    ("navigation-bar-selection", 1.2, 99, 111, [53, 53, 88]),
    ("navigation-bar-selection", 1.2, 102, 228, [89, 71, 0]),
    ("navigation-bar-selection", 2.0, 312, 225, [255, 205, 4]),
    ("navigation-bar-selection", 2.0, 162, 213, [255, 255, 255]),
    ("navigation-bar-selection", 2.0, 156, 243, [255, 255, 255]),
    ("navigation-bar-selection", 2.0, 285, 207, [255, 205, 4]),
    ("navigation-bar-selection", 2.0, 345, 231, [255, 205, 4]),
    ("navigation-bar-selection", 2.0, 198, 225, [255, 255, 255]),
    ("navigation-bar-selection", 2.0, 333, 207, [255, 205, 4]),
    ("navigation-bar-selection", 2.0, 189, 249, [255, 255, 255]),
    ("navigation-bar-selection", 2.0, 123, 321, [33, 27, 29]),
    ("navigation-bar-selection", 2.0, 177, 303, [255, 255, 255]),
    ("navigation-bar-selection", 2.0, 105, 219, [91, 73, 0]),
    ("navigation-bar-selection", 2.0, 120, 177, [255, 92, 1]),
    ("navigation-bar-selection", 2.0, 111, 294, [132, 174, 37]),
    ("navigation-bar-selection", 2.0, 342, 249, [255, 205, 4]),
    ("navigation-bar-selection", 2.0, 87, 126, [157, 157, 255]),
    ("navigation-bar-selection", 2.0, 99, 282, [60, 79, 15]),
    ("navigation-bar-selection", 2.0, 96, 174, [85, 30, 0]),
    ("navigation-bar-selection", 2.0, 96, 114, [53, 53, 88]),
    ("alphabet-a", 1.5, 186, 243, [194, 23, 22]),
    ("alphabet-a", 1.5, 159, 282, [186, 17, 16]),
    ("alphabet-a", 1.5, 159, 141, [255, 255, 255]),
    ("alphabet-a", 1.5, 273, 240, [196, 24, 23]),
    ("alphabet-a", 1.5, 213, 183, [207, 33, 32]),
    ("alphabet-a", 1.5, 36, 366, [127, 4, 3]),
    ("alphabet-a", 1.5, 141, 201, [0, 0, 0]),
    ("alphabet-a", 1.5, 207, 63, [244, 113, 112]),
    ("alphabet-a", 1.5, 228, 111, [222, 43, 42]),
    ("alphabet-a", 1.5, 225, 78, [229, 48, 47]),
    ("alphabet-a", 1.5, 363, 219, [248, 210, 215]),
    ("alphabet-a", 1.5, 303, 90, [235, 104, 103]),
    ("alphabet-a", 1.5, 270, 186, [174, 203, 210]),
    ("alphabet-a", 1.5, 252, 78, [229, 49, 48]),
    ("boxer", 0.75, 150, 303, [255, 255, 255]),
    ("boxer", 0.75, 213, 279, [0, 0, 0]),
    ("boxer", 0.75, 213, 207, [239, 66, 94]),
    ("boxer", 0.75, 189, 183, [102, 45, 142]),
    ("boxer", 0.75, 282, 156, [237, 146, 125]),
    ("boxer", 0.75, 285, 219, [245, 191, 153]),
    ("boxer", 0.75, 195, 150, [246, 192, 156]),
];

/// The scene of a gallery file, drawn at `time` into 400 × 400 pixels on
/// white.
fn frame(name: &str, time: f64) -> tiny_skia::Pixmap {
    let data = std::fs::read(format!(
        "{}/../../fixtures/gallery/{name}.svg",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let scene = dxn_vector_animation_svg::read(&data).unwrap().scene;
    render(
        &scene.frame_at(time),
        &RenderOptions {
            width: Some(400),
            height: Some(400),
            background: Some(Color::rgba(1.0, 1.0, 1.0, 1.0)),
            ..Default::default()
        },
    )
    .unwrap()
}

fn pixel(frame: &tiny_skia::Pixmap, x: u32, y: u32) -> [u8; 3] {
    let p = frame.pixel(x, y).unwrap();
    [p.red(), p.green(), p.blue()]
}

#[test]
fn flat_colors_are_the_browsers() {
    let mut cache: Vec<(&str, u64, tiny_skia::Pixmap)> = Vec::new();
    for &(name, time, x, y, want) in POINTS {
        let key = time.to_bits();
        if !cache.iter().any(|(n, k, _)| *n == name && *k == key) {
            cache.push((name, key, frame(name, time)));
        }
        let (_, _, f) = cache
            .iter()
            .find(|(n, k, _)| *n == name && *k == key)
            .unwrap();
        let got = pixel(f, x, y);
        for c in 0..3 {
            assert!(
                got[c].abs_diff(want[c]) <= 14,
                "{name} at {time} s, ({x}, {y}): a browser draws {want:?}, we draw {got:?}"
            );
        }
    }
}

#[test]
fn the_o_of_connect_is_a_dot_as_the_file_has_it() {
    // The middle of the "o": white in a browser, dark in the Lottie JSON.
    let f = frame("alien-button", 1.0);
    assert_eq!(pixel(&f, 172, 225), [255, 255, 255]);
}

#[test]
fn the_file_has_no_glow_under_the_button() {
    // Below the button the Lottie JSON glows purple; the file draws the
    // page's background (#18181b) there, in a browser too.
    let f = frame("alien-button", 1.0);
    for (x, y) in [(190, 275), (260, 274), (140, 272)] {
        let got = pixel(&f, x, y);
        for (c, want) in [24u8, 24, 27].into_iter().enumerate() {
            assert!(got[c].abs_diff(want) <= 6, "({x}, {y}): {got:?}");
        }
    }
}
