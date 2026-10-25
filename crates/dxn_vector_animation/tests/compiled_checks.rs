//! What the compiled format checks: a file's format and version before its
//! scene, and, when writing, numbers JSON cannot hold.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, Easing, Error, Group, Keyframe, Limits, LoopMode, Node, Scene, Timeline, Track,
    compiled,
};

fn scene(opacity: Animated<f64>) -> Scene {
    Scene {
        width: 10.0,
        height: 10.0,
        timeline: Timeline {
            duration: 1.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Once,
            markers: vec![],
        },
        root: Group {
            children: vec![Node::Group(Group {
                opacity,
                ..Group::new()
            })],
            ..Group::new()
        },
    }
}

fn read(json: &str) -> Result<Scene, Error> {
    compiled::from_json(json.as_bytes(), &Limits::default())
}

#[test]
fn a_newer_version_is_unsupported_even_when_its_scene_does_not_parse() {
    let error =
        read(r#"{"format":"dxn-vector-animation","version":2,"scene":{"shapes":[]}}"#).unwrap_err();
    assert_eq!(
        error.to_string(),
        "unsupported: format version 2 (this library reads up to 1)"
    );
    // The version may come after the scene.
    let error =
        read(r#"{"scene":{"layers":7},"format":"dxn-vector-animation","version":3}"#).unwrap_err();
    assert!(matches!(error, Error::Unsupported(_)), "{error}");
}

#[test]
fn another_format_is_invalid_before_its_scene_is_read() {
    let error = read(r#"{"format":"other","version":1,"scene":{}}"#).unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid file: not a dxn-vector-animation file"
    );
}

#[test]
fn a_scene_of_this_version_that_does_not_parse_is_invalid() {
    let error = read(r#"{"format":"dxn-vector-animation","version":1,"scene":{}}"#).unwrap_err();
    assert!(
        error.to_string().starts_with("invalid file: missing field"),
        "{error}"
    );
    assert!(matches!(read("not json"), Err(Error::Invalid(_))));
    assert!(matches!(read(r#"{"scene":{}}"#), Err(Error::Invalid(_))));
}

#[test]
fn finite_scenes_round_trip() {
    let still = scene(Animated::Static(0.5));
    let json = compiled::to_json(&still, true).unwrap();
    assert_eq!(read(&json).unwrap(), still);
}

#[test]
fn numbers_json_cannot_hold_are_refused_when_writing() {
    let error = compiled::to_json(&scene(Animated::Static(f64::NAN)), false).unwrap_err();
    assert_eq!(
        error.to_string(),
        "unsupported: `scene.root.children[0].opacity` is NaN, which the compiled format cannot hold"
    );
    let moving = scene(Animated::Track(Track::new(vec![
        Keyframe::new(0.0, 0.0, Easing::Linear),
        Keyframe::new(1.0, f64::INFINITY, Easing::Linear),
    ])));
    let error = compiled::to_json(&moving, false).unwrap_err();
    assert_eq!(
        error,
        Error::Unsupported(
            "`scene.root.children[0].opacity.keyframes[1].value` is inf, which the compiled \
             format cannot hold"
                .into()
        )
    );
    let mut endless = scene(Animated::Static(1.0));
    endless.timeline.duration = f64::NEG_INFINITY;
    let error = compiled::to_json(&endless, false).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("`scene.timeline.duration` is -inf"),
        "{error}"
    );
}
