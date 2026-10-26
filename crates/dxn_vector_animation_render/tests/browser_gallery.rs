//! The gallery's animated SVGs against a browser and against their Lottie
//! JSON: the three draw the same.
//!
//! Each gallery animation is three downloads of one animation
//! (`fixtures/gallery/`: Lottie JSON, dotLottie and an animated SVG written
//! as SMIL). The SVGs are the JSON's animation edited where LottieFiles'
//! SVG export had drawn it differently (see `SOURCES.md`):
//!
//! - `alien-button`: the "o" of "Connect" is one compound path (its inner
//!   contour cuts the counter out), the button's back casts the JSON's
//!   purple drop shadow and its outline the JSON's blur, the alien's
//!   highlights are blurred as the JSON blurs them, and the UFO's radial
//!   gradients move as the JSON's do.
//! - `navigation-bar-selection`: a bar's colored background grows from its
//!   left edge, as the JSON's rectangle does.
//!
//! The table is points of flat color read off Chromium's frames of the
//! files (headless, SMIL paused and set to the time with `setCurrentTime`,
//! 400 x 400 pixels on white). The importers and the reference renderer
//! put the same colors there, from the SVG and from the Lottie JSON alike:
//! one in the second loop of each file, where the JSON is drawn at the
//! time within its loop.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use dxn_vector_animation::{Color, Scene};
use dxn_vector_animation_render::{RenderOptions, render};

/// File, seconds, x, y (pixels of a 400 x 400 frame), the color a browser
/// draws there.
const POINTS: &[(&str, f64, u32, u32, [u8; 3])] = &[
    ("alien-button", 1.0, 312, 351, [24, 24, 27]),
    ("alien-button", 1.0, 396, 72, [24, 24, 27]),
    ("alien-button", 1.0, 183, 318, [24, 24, 27]),
    ("alien-button", 1.0, 375, 186, [24, 24, 27]),
    ("alien-button", 1.0, 171, 78, [24, 24, 27]),
    ("alien-button", 1.0, 108, 393, [24, 24, 27]),
    ("alien-button", 1.0, 27, 249, [24, 24, 27]),
    ("alien-button", 1.0, 96, 138, [24, 24, 27]),
    ("alien-button", 1.0, 297, 120, [24, 24, 27]),
    ("alien-button", 1.0, 6, 147, [24, 24, 27]),
    ("alien-button", 1.0, 300, 30, [24, 24, 27]),
    ("alien-button", 1.0, 396, 288, [24, 24, 27]),
    ("alien-button", 1.0, 42, 60, [24, 24, 27]),
    ("alien-button", 1.0, 12, 360, [24, 24, 27]),
    ("alien-button", 1.0, 294, 240, [24, 24, 27]),
    ("alien-button", 3.0, 42, 192, [24, 24, 27]),
    ("alien-button", 3.0, 240, 33, [24, 24, 27]),
    ("alien-button", 3.0, 21, 84, [24, 24, 27]),
    ("alien-button", 3.0, 354, 36, [24, 24, 27]),
    ("alien-button", 3.0, 303, 345, [24, 24, 27]),
    ("alien-button", 3.0, 393, 381, [24, 24, 27]),
    ("alien-button", 3.0, 348, 144, [24, 24, 27]),
    ("alien-button", 3.0, 201, 135, [24, 24, 27]),
    ("alien-button", 3.0, 120, 66, [24, 24, 27]),
    ("alien-button", 3.0, 315, 231, [24, 24, 27]),
    ("alien-button", 3.0, 36, 294, [24, 24, 27]),
    ("alien-button", 3.0, 189, 345, [24, 24, 27]),
    ("alien-button", 3.0, 108, 195, [86, 86, 114]),
    ("alien-button", 3.0, 111, 396, [24, 24, 27]),
    ("alien-button", 3.0, 15, 387, [24, 24, 27]),
    ("alphabet-a", 1.5, 210, 30, [255, 255, 255]),
    ("alphabet-a", 1.5, 294, 306, [255, 255, 255]),
    ("alphabet-a", 1.5, 99, 372, [255, 255, 255]),
    ("alphabet-a", 1.5, 45, 204, [255, 255, 255]),
    ("alphabet-a", 1.5, 60, 318, [178, 10, 9]),
    ("alphabet-a", 1.5, 81, 27, [255, 255, 255]),
    ("alphabet-a", 1.5, 147, 264, [190, 20, 19]),
    ("alphabet-a", 1.5, 300, 258, [192, 21, 20]),
    ("alphabet-a", 1.5, 390, 237, [255, 255, 255]),
    ("alphabet-a", 1.5, 375, 126, [255, 255, 255]),
    ("alphabet-a", 1.5, 174, 195, [255, 106, 105]),
    ("alphabet-a", 1.5, 234, 81, [229, 48, 47]),
    ("alphabet-a", 1.5, 315, 9, [255, 255, 255]),
    ("alphabet-a", 1.5, 195, 63, [243, 112, 111]),
    ("alphabet-a", 4.5, 51, 102, [255, 255, 255]),
    ("alphabet-a", 4.5, 186, 267, [190, 20, 19]),
    ("alphabet-a", 4.5, 261, 84, [228, 48, 47]),
    ("alphabet-a", 4.5, 75, 234, [255, 255, 255]),
    ("alphabet-a", 4.5, 162, 306, [255, 255, 255]),
    ("alphabet-a", 4.5, 285, 249, [194, 23, 22]),
    ("alphabet-a", 4.5, 243, 168, [211, 35, 34]),
    ("alphabet-a", 4.5, 330, 108, [255, 255, 255]),
    ("alphabet-a", 4.5, 300, 360, [132, 9, 8]),
    ("alphabet-a", 4.5, 276, 9, [255, 255, 255]),
    ("alphabet-a", 4.5, 153, 15, [255, 255, 255]),
    ("alphabet-a", 4.5, 258, 222, [0, 0, 0]),
    ("alphabet-a", 4.5, 393, 306, [255, 255, 255]),
    ("alphabet-a", 4.5, 24, 327, [176, 9, 8]),
    ("boxer", 0.75, 384, 222, [255, 255, 255]),
    ("boxer", 0.75, 120, 249, [255, 255, 255]),
    ("boxer", 0.75, 120, 357, [255, 255, 255]),
    ("boxer", 0.75, 48, 84, [255, 255, 255]),
    ("boxer", 0.75, 282, 327, [255, 255, 255]),
    ("boxer", 0.75, 264, 15, [255, 255, 255]),
    ("boxer", 0.75, 54, 180, [255, 255, 255]),
    ("boxer", 2.0, 78, 249, [255, 255, 255]),
    ("boxer", 2.0, 12, 69, [255, 255, 255]),
    ("boxer", 2.0, 303, 375, [255, 255, 255]),
    ("boxer", 2.0, 312, 162, [255, 255, 255]),
    ("boxer", 2.0, 390, 282, [255, 255, 255]),
    ("boxer", 2.0, 123, 87, [255, 255, 255]),
    ("boxer", 2.0, 216, 51, [255, 255, 255]),
    ("navigation-bar-selection", 1.2, 171, 30, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 345, 339, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 267, 291, [255, 255, 255]),
    ("navigation-bar-selection", 1.2, 111, 111, [157, 157, 255]),
    ("navigation-bar-selection", 1.2, 66, 321, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 384, 12, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 42, 42, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 93, 267, [133, 175, 37]),
    ("navigation-bar-selection", 1.2, 348, 132, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 201, 315, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 255, 69, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 291, 231, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 54, 222, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 156, 396, [33, 27, 29]),
    ("navigation-bar-selection", 1.2, 177, 210, [255, 255, 255]),
    ("navigation-bar-selection", 2.0, 339, 204, [33, 27, 29]),
    ("navigation-bar-selection", 2.0, 324, 57, [33, 27, 29]),
    ("navigation-bar-selection", 2.0, 21, 342, [33, 27, 29]),
    ("navigation-bar-selection", 2.0, 396, 321, [33, 27, 29]),
    ("navigation-bar-selection", 2.0, 150, 357, [33, 27, 29]),
    ("navigation-bar-selection", 2.0, 261, 300, [255, 255, 255]),
    ("navigation-bar-selection", 2.0, 51, 126, [33, 27, 29]),
    ("navigation-bar-selection", 2.0, 270, 384, [33, 27, 29]),
    ("navigation-bar-selection", 2.0, 264, 150, [255, 255, 255]),
    ("navigation-bar-selection", 2.0, 168, 138, [255, 255, 255]),
    ("navigation-bar-selection", 2.0, 24, 225, [33, 27, 29]),
    ("navigation-bar-selection", 2.0, 207, 18, [33, 27, 29]),
    ("navigation-bar-selection", 2.0, 96, 294, [132, 174, 37]),
    ("navigation-bar-selection", 2.0, 237, 126, [33, 27, 29]),
    ("navigation-bar-selection", 2.0, 252, 216, [255, 205, 4]),
    ("navigation-bar-selection", 4.0, 186, 327, [33, 27, 29]),
    ("navigation-bar-selection", 4.0, 15, 285, [33, 27, 29]),
    ("navigation-bar-selection", 4.0, 333, 90, [33, 27, 29]),
    ("navigation-bar-selection", 4.0, 345, 354, [33, 27, 29]),
    ("navigation-bar-selection", 4.0, 66, 195, [33, 27, 29]),
    ("navigation-bar-selection", 4.0, 180, 189, [255, 92, 1]),
    ("navigation-bar-selection", 4.0, 201, 69, [33, 27, 29]),
    ("navigation-bar-selection", 4.0, 18, 102, [33, 27, 29]),
    ("navigation-bar-selection", 4.0, 381, 258, [33, 27, 29]),
    ("navigation-bar-selection", 4.0, 108, 213, [255, 205, 4]),
    ("navigation-bar-selection", 4.0, 102, 192, [85, 30, 0]),
    ("navigation-bar-selection", 4.0, 168, 249, [255, 255, 255]),
    ("navigation-bar-selection", 4.0, 246, 300, [255, 255, 255]),
    ("navigation-bar-selection", 4.0, 78, 117, [157, 157, 255]),
    ("navigation-bar-selection", 4.0, 87, 363, [33, 27, 29]),
];

fn path(name: &str, extension: &str) -> String {
    format!(
        "{}/../../fixtures/gallery/{name}.{extension}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// The scene of a gallery animation's animated SVG.
fn svg(name: &str) -> Scene {
    let data = std::fs::read(path(name, "svg")).unwrap();
    dxn_vector_animation_svg::read(&data).unwrap().scene
}

/// The scene of a gallery animation's Lottie JSON.
fn json(name: &str) -> Scene {
    let data = std::fs::read(path(name, "json")).unwrap();
    dxn_vector_animation_lottie::read(&data).unwrap().scene
}

/// The scene at `time`, drawn into 400 x 400 pixels on white. A Lottie JSON
/// plays once, so it is drawn at the time within its loop; an SVG loops
/// itself.
fn frame(scene: &Scene, time: f64, looped: bool) -> tiny_skia::Pixmap {
    let time = if looped {
        time % scene.timeline.duration
    } else {
        time
    };
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

/// Whether `got` is within `tolerance` of `want` in every channel.
fn near(got: [u8; 3], want: [u8; 3], tolerance: u8) -> bool {
    (0..3).all(|c| got[c].abs_diff(want[c]) <= tolerance)
}

/// A frame of the SVG and of the JSON of one animation, at `time`; drawn once
/// for all the tests.
fn both(name: &str, time: f64) -> Arc<(tiny_skia::Pixmap, tiny_skia::Pixmap)> {
    type Frames = Arc<(tiny_skia::Pixmap, tiny_skia::Pixmap)>;
    static CACHE: Mutex<Vec<(String, u64, Frames)>> = Mutex::new(Vec::new());
    let mut cache = CACHE.lock().unwrap();
    let key = time.to_bits();
    if let Some((_, _, frames)) = cache.iter().find(|(n, k, _)| n == name && *k == key) {
        return frames.clone();
    }
    let frames = Arc::new((
        frame(&svg(name), time, false),
        frame(&json(name), time, true),
    ));
    cache.push((name.to_string(), key, frames.clone()));
    frames
}

#[test]
fn flat_colors_are_the_browsers() {
    // The same points in the SVG's frame and in the JSON's.
    for &(name, time, x, y, want) in POINTS {
        let frames = both(name, time);
        let (svg, json) = (pixel(&frames.0, x, y), pixel(&frames.1, x, y));
        assert!(
            near(svg, want, 14),
            "{name} at {time} s, ({x}, {y}): a browser draws {want:?}, we draw {svg:?} from the SVG"
        );
        assert!(
            near(json, want, 14),
            "{name} at {time} s, ({x}, {y}): a browser draws {want:?}, we draw {json:?} from the Lottie JSON"
        );
    }
}

#[test]
fn the_o_of_connect_is_a_ring() {
    // The middle of the "o" shows the button's face (a browser draws
    // [56, 56, 61] there), not the letter's white.
    let frames = both("alien-button", 1.0);
    let (svg, json) = (&frames.0, &frames.1);
    for (what, f) in [("SVG", svg), ("JSON", json)] {
        let got = pixel(f, 172, 225);
        assert!(near(got, [56, 56, 61], 10), "{what}: {got:?}");
    }
}

#[test]
fn the_button_glows_purple_below() {
    // Below the button the drop shadow of its back glows (a browser draws
    // these colors; the page's background is [24, 24, 27]).
    let frames = both("alien-button", 1.0);
    let (svg, json) = (&frames.0, &frames.1);
    for (x, y, want) in [
        (200, 276, [43, 17, 86]),
        (260, 276, [43, 17, 86]),
        (140, 274, [48, 14, 105]),
        (100, 262, [42, 22, 77]),
    ] {
        for (what, f) in [("SVG", svg), ("JSON", json)] {
            let got = pixel(f, x, y);
            assert!(near(got, want, 10), "{what} ({x}, {y}): {got:?}");
            assert!(got[2] > got[1] + 40, "{what} ({x}, {y}) is not purple");
        }
    }
}

#[test]
fn colored_backgrounds_sit_under_their_labels() {
    // The Articles bar's yellow at 2 s and the Question bar's orange at
    // 1.2 s cover their whole labels, left end to right end; a background
    // half its width off its label leaves one end of the label white and
    // the other end past the label's edge.
    for (time, points, want) in [
        (
            2.0,
            [(144, 215), (276, 215), (144, 240), (278, 240)],
            [255, 205, 4],
        ),
        (
            1.2,
            [(144, 160), (300, 160), (144, 185), (300, 185)],
            [255, 92, 1],
        ),
    ] {
        let frames = both("navigation-bar-selection", time);
        let (svg, json) = (&frames.0, &frames.1);
        for (what, f) in [("SVG", svg), ("JSON", json)] {
            for (x, y) in points {
                let got = pixel(f, x, y);
                assert!(
                    near(got, want, 14),
                    "{what} at {time} s ({x}, {y}): {got:?}"
                );
            }
        }
    }
}

/// A frame of one scene at `time` and `size` pixels, on white.
fn small(scene: &Scene, time: f64, looped: bool) -> tiny_skia::Pixmap {
    let time = if looped {
        time % scene.timeline.duration
    } else {
        time
    };
    render(
        &scene.frame_at(time),
        &RenderOptions {
            width: Some(200),
            height: Some(200),
            background: Some(Color::rgba(1.0, 1.0, 1.0, 1.0)),
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn svg_and_json_agree_everywhere() {
    // Whole frames, not points: the mean difference and the share of pixels
    // that differ clearly (`name`, mean and share limits in percent). The
    // still scenes agree to antialiasing; the edges of fast-moving shapes
    // (alphabet-a's eyes, boxer's limbs) differ by a fraction of a pixel,
    // because the two time lines meet within a millisecond.
    for (name, mean_limit, share_limit) in [
        ("alien-button", 0.5, 0.3),
        ("navigation-bar-selection", 0.5, 0.3),
        ("alphabet-a", 1.6, 1.6),
        ("boxer", 1.6, 1.6),
    ] {
        let (svg, json) = (svg(name), json(name));
        let duration = json.timeline.duration;
        for i in 0..4 {
            let time = (f64::from(i) + 0.37) * duration / 4.0;
            let (a, b) = (small(&svg, time, false), small(&json, time, true));
            let (mut sum, mut clear) = (0u64, 0u64);
            for (p, q) in a.pixels().iter().zip(b.pixels()) {
                let d = [
                    p.red().abs_diff(q.red()),
                    p.green().abs_diff(q.green()),
                    p.blue().abs_diff(q.blue()),
                ];
                sum += d.iter().map(|&v| u64::from(v)).sum::<u64>();
                clear += u64::from(d.iter().any(|&v| v > 48));
            }
            let pixels = u64::from(a.width()) * u64::from(a.height());
            let mean = sum as f64 / (3 * pixels) as f64;
            let share = clear as f64 / pixels as f64 * 100.0;
            assert!(
                mean < mean_limit && share < share_limit,
                "{name} at {time:.3} s: mean difference {mean:.3}, {share:.2} % of the pixels differ clearly"
            );
        }
    }
}
