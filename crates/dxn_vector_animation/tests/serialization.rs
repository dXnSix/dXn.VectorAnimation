//! The compiled scene format.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, Color, Easing, Fill, Geometry, Group, Keyframe, LoopMode, Node, Paint, Path, Point,
    Scene, Shape, ShapePath, Timeline, Track, TransformOp,
};

#[test]
fn scenes_round_trip_through_json() {
    let mut p = Path::new();
    p.move_to(Point::new(0.0, 0.5));
    p.cubic_to(
        Point::new(1.0, 2.0),
        Point::new(3.0, -4.25),
        Point::new(5.0, 6e-3),
    );
    p.close();
    let scene = Scene {
        width: 64.0,
        height: 32.0,
        timeline: Timeline {
            duration: 1.5,
            frame_rate: Some(60.0),
            loop_start: 0.0,
            loop_mode: LoopMode::PingPong,
            markers: vec![],
        },
        root: Group {
            transform: vec![TransformOp::Rotate {
                degrees: Animated::Track(Track::new(vec![
                    Keyframe::new(0.0, 0.0, Easing::EASE),
                    Keyframe::new(1.5, 90.0, Easing::Linear),
                ])),
            }],
            children: vec![Node::shape(Shape {
                paths: vec![ShapePath::new(Geometry::path(p))],
                fill: Some(Fill {
                    paint: Paint::solid(Color::rgba(0.1, 0.2, 0.3, 1.0)),
                    opacity: Animated::Static(1.0),
                    rule: Default::default(),
                }),
                ..Default::default()
            })],
            ..Group::new()
        },
    };
    let json = serde_json::to_string(&scene).unwrap();
    let back: Scene = serde_json::from_str(&json).unwrap();
    assert_eq!(back, scene);
}
