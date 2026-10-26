//! The gallery's files (`fixtures/gallery/`, free LottieFiles downloads): each
//! Lottie JSON reads without a warning, and the dotLottie archive of the same
//! animation reads to the same scene.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::Scene;

fn file(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../fixtures/gallery/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn json(name: &str) -> Scene {
    let import = dxn_vector_animation_lottie::read(&file(&format!("{name}.json"))).unwrap();
    assert_eq!(import.warnings, Vec::<String>::new(), "{name}");
    import.scene
}

/// name, width, height, seconds, frame rate
const FILES: &[(&str, f64, f64, f64, f64)] = &[
    ("alien-button", 1379.0, 1379.0, 6.0, 60.0),
    ("alphabet-a", 512.0, 512.0, 3.0, 60.0),
    (
        "navigation-bar-selection",
        1000.0,
        1000.0,
        182.0 / 60.0,
        60.0,
    ),
    ("boxer", 1080.0, 1080.0, 45.0 / 29.97, 29.97),
];

#[test]
fn json_files_read_whole() {
    for (name, w, h, seconds, rate) in FILES {
        let scene = json(name);
        assert_eq!((scene.width, scene.height), (*w, *h), "{name}");
        assert!((scene.timeline.duration - seconds).abs() < 0.01, "{name}");
        let fr = scene.timeline.frame_rate.unwrap();
        assert!((fr - rate).abs() < 0.01, "{name}");
        scene
            .check(&dxn_vector_animation::Limits::default())
            .unwrap();
    }
}

#[test]
fn markers_come_through() {
    // Alien Button and the navigation bar name the parts they play.
    assert_eq!(json("alien-button").timeline.markers.len(), 7);
    let bar = json("navigation-bar-selection");
    let names: Vec<_> = bar
        .timeline
        .markers
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["Chat", "Question", "Articles", "Communities", "Idle"]
    );
}

/// The v2 layout (`a/<id>.json`, state machines under `s/`) is not read yet:
/// the importer looks for dotLottie 1's `animations/<id>.json`.
#[test]
#[ignore = "dotLottie 2 archives (a/<id>.json) are not read yet"]
fn dotlottie_archives_match_their_json() {
    for name in ["alien-button", "alphabet-a", "navigation-bar-selection"] {
        let from_json = json(name);
        let import = dxn_vector_animation_lottie::read(&file(&format!("{name}.lottie"))).unwrap();
        assert_eq!(import.scene.width, from_json.width, "{name}");
        assert_eq!(
            import.scene.timeline.duration, from_json.timeline.duration,
            "{name}"
        );
    }
}
