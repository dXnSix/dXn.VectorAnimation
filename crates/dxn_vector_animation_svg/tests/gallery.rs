//! The gallery's animated SVGs (`fixtures/gallery/`, free LottieFiles
//! downloads written as SMIL, edited to draw what their Lottie JSON does:
//! `SOURCES.md`): they read, and none of their SMIL is left out, the
//! gradients, rectangle sizes and positions and blur filters the edits wrote
//! among it. The reference renderer's `browser_gallery` tests check the
//! drawing against a browser and the JSON.

#![allow(clippy::unwrap_used, clippy::expect_used)]

fn read(name: &str) -> dxn_vector_animation::Import {
    let data = std::fs::read(format!(
        "{}/../../fixtures/gallery/{name}.svg",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    dxn_vector_animation_svg::read(&data).unwrap()
}

fn kinds(name: &str) -> Vec<String> {
    let mut warnings = read(name).warnings;
    warnings.sort();
    warnings.dedup();
    warnings
}

#[test]
fn files_read_at_their_size_and_length() {
    // name, size, seconds: the same as the Lottie JSON of the animation.
    for (name, size, seconds) in [
        ("alien-button", 1379.0, 6.0),
        ("alphabet-a", 512.0, 3.0),
        ("navigation-bar-selection", 1000.0, 182.0 / 60.0),
        ("boxer", 1080.0, 1.502),
    ] {
        let scene = read(name).scene;
        assert_eq!((scene.width, scene.height), (size, size), "{name}");
        assert!((scene.timeline.duration - seconds).abs() < 0.01, "{name}");
        scene
            .check(&dxn_vector_animation::Limits::default())
            .unwrap();
    }
}

#[test]
fn nothing_is_left_out() {
    // Every animation of these files is read: gradients (linear and radial,
    // their geometry and stops), a rectangle's `x` and `width`, clip paths
    // and masks that animate, transforms, `display` and a path's `d` among
    // them; and every filter they use, blurs and drop shadows.
    for name in [
        "alien-button",
        "alphabet-a",
        "navigation-bar-selection",
        "boxer",
    ] {
        assert!(kinds(name).is_empty(), "{name}: {:?}", kinds(name));
    }
}

#[test]
fn what_they_animate_moves() {
    // Alien Button's gradient line and boxer's layers change over time.
    let scene = read("alien-button").scene;
    assert_ne!(scene.frame_at(0.5).items, scene.frame_at(2.5).items);
    let scene = read("boxer").scene;
    assert_ne!(scene.frame_at(0.2).items, scene.frame_at(0.7).items);
    // Navigation Bar Selection's backgrounds grow as the selection moves.
    let scene = read("navigation-bar-selection").scene;
    assert_ne!(scene.frame_at(0.2).items, scene.frame_at(0.5).items);
    assert_ne!(scene.frame_at(1.2).items, scene.frame_at(2.0).items);
}

#[test]
fn edited_filters_are_all_read() {
    // Alien Button's blurs and its drop shadow are written once each, with
    // an id of their own: of two filters with one id, a browser applies the
    // first to every part that names it.
    let text = std::fs::read_to_string(format!(
        "{}/../../fixtures/gallery/alien-button.svg",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut ids: Vec<&str> = text
        .match_indices("<filter ")
        .filter_map(|(at, _)| text[at..].split("id=\"").nth(1)?.split('"').next())
        .collect();
    let all = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), all, "a filter id is written twice");
    assert!(read("alien-button").warnings.is_empty());
}
