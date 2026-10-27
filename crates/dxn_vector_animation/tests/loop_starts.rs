//! A timeline's loop start: what comes before it plays once, the loop
//! repeats from it to the end, so frames past the end come from the loop.
//! Scenes that do not set it loop the whole timeline, as before.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, Color, Easing, Error, Fill, Geometry, Group, Keyframe, Limits, LoopMode, Node, Paint,
    Path, Point, Scene, Shape, ShapePath, Timeline, Track,
};

/// A 3 s timeline looping from `loop_start`, its group's opacity going
/// from 0 to 0.75 over it.
fn scene(loop_mode: LoopMode, loop_start: f64) -> Scene {
    let mut square = Path::new();
    square.move_to(Point::new(0.0, 0.0));
    square.line_to(Point::new(5.0, 0.0));
    square.line_to(Point::new(5.0, 5.0));
    square.close();
    Scene {
        width: 10.0,
        height: 10.0,
        timeline: Timeline {
            duration: 3.0,
            frame_rate: None,
            loop_start,
            loop_mode,
            markers: vec![],
            cues: Vec::new(),
        },
        root: Group {
            children: vec![Node::Group(Group {
                children: vec![Node::shape(Shape {
                    paths: vec![ShapePath::new(Geometry::path(square))],
                    fill: Some(Fill {
                        paint: Paint::solid(Color::rgba(1.0, 0.0, 0.0, 1.0)),
                        opacity: Animated::Static(1.0),
                        rule: Default::default(),
                    }),
                    ..Default::default()
                })],
                opacity: Animated::Track(Track::new(vec![
                    Keyframe::new(0.0, 0.0, Easing::Linear),
                    Keyframe::new(3.0, 0.75, Easing::Linear),
                ])),
                ..Group::new()
            })],
            ..Group::new()
        },
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_loop_plays_the_intro_once() {
    let t = scene(LoopMode::Loop, 1.0).timeline;
    assert!(close(t.time_at(0.5), 0.5));
    assert!(close(t.time_at(3.0), 3.0));
    // Past the end: from the loop start, 2 s at a time.
    assert!(close(t.time_at(3.5), 1.5));
    assert!(close(t.time_at(5.25), 1.25));
    assert!(close(t.time_at(9.5), 1.5));
}

#[test]
fn without_a_loop_start_the_whole_timeline_loops() {
    let t = scene(LoopMode::Loop, 0.0).timeline;
    assert!(close(t.time_at(3.5), 0.5));
    assert!(close(t.time_at(7.0), 1.0));
}

#[test]
fn ping_pong_turns_back_at_the_loop_start() {
    let t = scene(LoopMode::PingPong, 1.0).timeline;
    assert!(close(t.time_at(2.0), 2.0));
    // Back from the end to the loop start, then forwards again.
    assert!(close(t.time_at(4.0), 2.0));
    assert!(close(t.time_at(5.0), 1.0));
    assert!(close(t.time_at(6.5), 2.5));
}

#[test]
fn once_holds_the_end() {
    let t = scene(LoopMode::Once, 1.0).timeline;
    assert!(close(t.time_at(10.0), 3.0));
    assert!(close(t.time_at(-1.0), 0.0));
}

#[test]
fn frames_past_the_end_come_from_the_loop() {
    let s = scene(LoopMode::Loop, 1.0);
    let at = |elapsed: f64| s.frame_at(s.timeline.time_at(elapsed));
    // 4 s in is 2 s of the timeline, 7.5 s in 1.5 s.
    assert_eq!(at(4.0), s.frame_at(2.0));
    assert_eq!(at(7.5), s.frame_at(1.5));
    assert_ne!(at(4.0), s.frame_at(0.0));
}

#[test]
fn a_loop_start_outside_the_timeline_loops_it_whole() {
    let mut t = scene(LoopMode::Loop, 3.0).timeline;
    assert_eq!(t.loop_from(), 0.0);
    t.loop_start = 1.0;
    assert_eq!(t.loop_from(), 1.0);
}

#[test]
fn the_check_refuses_a_loop_start_outside_the_timeline() {
    for at in [-1.0, 3.5, f64::NAN] {
        let error = scene(LoopMode::Loop, at)
            .check(&Limits::default())
            .unwrap_err();
        assert!(matches!(error, Error::Invalid(_)), "{error:?}");
    }
    assert!(scene(LoopMode::Loop, 3.0).check(&Limits::default()).is_ok());
}

#[cfg(feature = "serde")]
mod compiled {
    use dxn_vector_animation::{Limits, LoopMode, compiled};

    use super::scene;

    #[test]
    fn it_is_written_only_when_set() {
        let json = compiled::to_json(&scene(LoopMode::Loop, 0.0), false).unwrap();
        assert!(!json.contains("loopStart"), "{json}");
        let json = compiled::to_json(&scene(LoopMode::Loop, 1.0), false).unwrap();
        assert!(json.contains(r#""loopStart":1.0"#), "{json}");
        let back = compiled::from_json(json.as_bytes(), &Limits::default()).unwrap();
        assert_eq!(back.timeline.loop_start, 1.0);
    }

    #[test]
    fn files_without_it_loop_whole() {
        let json = compiled::to_json(&scene(LoopMode::Loop, 0.0), false).unwrap();
        let back = compiled::from_json(json.as_bytes(), &Limits::default()).unwrap();
        assert_eq!(back.timeline.loop_start, 0.0);
    }
}
