//! Drop shadows and blurs of groups: sampled into layers in the frame's
//! space, and kept by the compiled scene format.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, AnimatedPoint, Blur, Color, DropShadow, Easing, Effect, Fill, FrameEffect, Geometry,
    Group, Item, Keyframe, LoopMode, Node, Paint, Path, Point, Scene, Shape, ShapePath, Timeline,
    Track, TransformOp, compiled,
};

fn square() -> Node {
    let mut p = Path::new();
    p.move_to(Point::new(0.0, 0.0));
    p.line_to(Point::new(10.0, 0.0));
    p.line_to(Point::new(10.0, 10.0));
    p.close();
    Node::shape(Shape {
        paths: vec![ShapePath::new(Geometry::path(p))],
        fill: Some(Fill {
            paint: Paint::solid(Color::rgba(1.0, 0.0, 0.0, 1.0)),
            opacity: Animated::Static(1.0),
            rule: Default::default(),
        }),
        ..Default::default()
    })
}

fn shadowed(transform: Vec<TransformOp>) -> Scene {
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_mode: LoopMode::Loop,
            markers: vec![],
        },
        root: Group {
            children: vec![Node::Group(Group {
                transform,
                effects: vec![
                    Effect::Blur(Blur {
                        radius: Animated::Track(Track::new(vec![
                            Keyframe::new(0.0, 1.0, Easing::Linear),
                            Keyframe::new(2.0, 3.0, Easing::Linear),
                        ])),
                    }),
                    Effect::DropShadow(DropShadow {
                        color: Animated::Static(Color::rgba(0.0, 0.0, 0.0, 0.25)),
                        offset: AnimatedPoint::fixed(0.0, 4.0),
                        blur: Animated::Static(2.0),
                        knockout: true,
                        hard: true,
                    }),
                ],
                children: vec![square()],
                ..Group::new()
            })],
            ..Group::new()
        },
    }
}

#[test]
fn a_group_with_effects_draws_in_a_layer_of_them() {
    let frame = shadowed(Vec::new()).frame_at(1.0);
    let Some(Item::PushLayer(layer)) = frame.items.first() else {
        panic!("{:?}", frame.items);
    };
    assert_eq!(
        layer.effects,
        vec![
            FrameEffect::Blur { sigma: 2.0 },
            FrameEffect::DropShadow {
                color: Color::rgba(0.0, 0.0, 0.0, 0.25),
                offset: Point::new(0.0, 4.0),
                sigma: 2.0,
                knockout: true,
                hard: true,
            },
        ]
    );
}

#[test]
fn effects_follow_the_groups_transform() {
    // Turned a quarter and doubled: the shadow falls to the left, twice as
    // far and as blurred.
    let frame = shadowed(vec![
        TransformOp::Scale {
            x: Animated::Static(2.0),
            y: Animated::Static(2.0),
        },
        TransformOp::Rotate {
            degrees: Animated::Static(90.0),
        },
    ])
    .frame_at(0.0);
    let Some(Item::PushLayer(layer)) = frame.items.first() else {
        panic!("{:?}", frame.items);
    };
    let FrameEffect::DropShadow { offset, sigma, .. } = &layer.effects[1] else {
        panic!("{:?}", layer.effects);
    };
    assert!(
        (offset.x + 8.0).abs() < 1e-9 && offset.y.abs() < 1e-9,
        "{offset:?}"
    );
    assert!((sigma - 4.0).abs() < 1e-9);
    assert_eq!(layer.effects[0], FrameEffect::Blur { sigma: 2.0 });
}

#[test]
fn effects_survive_the_compiled_format() {
    let scene = shadowed(Vec::new());
    let json = compiled::to_json(&scene, false).unwrap();
    let back = compiled::from_json(json.as_bytes(), &Default::default()).unwrap();
    assert_eq!(back.root, scene.root);
}
