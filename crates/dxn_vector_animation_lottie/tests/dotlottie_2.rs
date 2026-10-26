//! dotLottie 2 archives (`a/<id>.json`, the manifest's ids, state machines
//! under `s/`): the three of `fixtures/gallery/` read to the scene of their
//! Lottie JSON, and say what they leave out.

#![allow(clippy::unwrap_used, clippy::expect_used)]

fn file(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../fixtures/gallery/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn archives_read_to_the_scene_of_their_json() {
    for name in ["alien-button", "alphabet-a", "navigation-bar-selection"] {
        let archive = dxn_vector_animation_lottie::read(&file(&format!("{name}.lottie"))).unwrap();
        let json = dxn_vector_animation_lottie::read(&file(&format!("{name}.json"))).unwrap();
        assert_eq!(archive.scene, json.scene, "{name}");
    }
}

#[test]
fn state_machines_are_left_out_and_said() {
    let warnings = |name: &str| {
        dxn_vector_animation_lottie::read(&file(name))
            .unwrap()
            .warnings
    };
    assert_eq!(
        warnings("alien-button.lottie"),
        ["the archive holds 1 state machines: they are left out"]
    );
    assert_eq!(
        warnings("navigation-bar-selection.lottie"),
        ["the archive holds 1 state machines: they are left out"]
    );
    assert!(warnings("alphabet-a.lottie").is_empty());
}
