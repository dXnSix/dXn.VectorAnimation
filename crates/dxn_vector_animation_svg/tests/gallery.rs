//! The gallery's animated SVGs (`fixtures/gallery/`, free LottieFiles
//! downloads written as SMIL): they read, and the SMIL they leave out is
//! pinned.

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
fn what_smil_leaves_out_is_said() {
    assert!(kinds("alphabet-a").is_empty());
    assert_eq!(
        kinds("boxer"),
        ["animation of `display` is left out"],
        "a layer shown and hidden"
    );
    assert_eq!(
        kinds("alien-button"),
        [
            "SMIL animation of `d` on <path> is left out",
            "SMIL animation of `stop-color` on <stop> is left out",
            "SMIL animation of `transform` on <g> is left out",
            "SMIL animation of `x1` on <linearGradient> is left out",
            "SMIL animation of `x2` on <linearGradient> is left out",
            "SMIL animation of `y1` on <linearGradient> is left out",
            "SMIL animation of `y2` on <linearGradient> is left out",
        ]
    );
    assert_eq!(
        kinds("navigation-bar-selection"),
        [
            "SMIL animation of `transform` on <g> is left out",
            "SMIL animation of `width` on <rect> is left out",
            "SMIL animation of `x` on <rect> is left out",
        ]
    );
}
