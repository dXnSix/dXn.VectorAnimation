//! The gallery's animated SVGs (`fixtures/gallery/`, free LottieFiles
//! downloads written as SMIL): they read, and none of their SMIL is left out.

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
    // Every animation of these files is read: gradients, clip paths and
    // masks that animate, transforms, `display` and a path's `d` among them.
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
}
