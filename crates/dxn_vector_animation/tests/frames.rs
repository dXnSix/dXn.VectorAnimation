//! Sampling a scene into frames.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, BlendMode, Color, Easing, Fill, FramePaint, Geometry, Group, Item, Keyframe,
    LoopMode, Node, Paint, Path, Point, Scene, Shape, ShapePath, TimeMap, TimeRange, Timeline,
    Track, TransformOp,
};

fn square() -> Shape {
    let mut p = Path::new();
    p.move_to(Point::new(0.0, 0.0));
    p.line_to(Point::new(10.0, 0.0));
    p.line_to(Point::new(10.0, 10.0));
    p.close();
    Shape {
        paths: vec![ShapePath::new(Geometry::path(p))],
        fill: Some(Fill {
            paint: Paint::solid(Color::rgba(1.0, 0.0, 0.0, 1.0)),
            opacity: Animated::Static(0.5),
            rule: Default::default(),
        }),
        ..Default::default()
    }
}

fn scene(root: Group) -> Scene {
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Loop,
            markers: vec![],
        },
        root,
    }
}

#[test]
fn fills_fold_their_opacity() {
    let s = scene(Group {
        children: vec![Node::shape(square())],
        ..Group::new()
    });
    let frame = s.frame_at(0.0);
    let [
        Item::Fill {
            paint: FramePaint::Solid(c),
            ..
        },
    ] = frame.items.as_slice()
    else {
        panic!("{:?}", frame.items);
    };
    assert_eq!(c.a, 0.5);
}

#[test]
fn faded_groups_draw_in_layers() {
    let s = scene(Group {
        opacity: Animated::Static(0.5),
        blend: BlendMode::Multiply,
        children: vec![Node::shape(square())],
        ..Group::new()
    });
    let items = s.frame_at(0.0).items;
    assert!(
        matches!(&items[0], Item::PushLayer(l) if l.opacity == 0.5 && l.blend == BlendMode::Multiply)
    );
    assert!(matches!(items.last(), Some(Item::PopLayer)));
}

#[test]
fn transforms_are_sampled_at_the_time() {
    let s = scene(Group {
        transform: vec![TransformOp::Translate {
            x: Animated::Track(Track::new(vec![
                Keyframe::new(0.0, 0.0, Easing::Linear),
                Keyframe::new(2.0, 100.0, Easing::Linear),
            ])),
            y: Animated::Static(0.0),
        }],
        children: vec![Node::shape(square())],
        ..Group::new()
    });
    let Item::Fill { transform, .. } = &s.frame_at(1.0).items[0] else {
        panic!()
    };
    assert_eq!(transform.e, 50.0);
}

#[test]
fn inactive_groups_are_left_out_and_time_maps() {
    let inner = Group {
        active: Some(TimeRange {
            start: 1.0,
            end: 2.0,
        }),
        time: Some(TimeMap {
            offset: 1.0,
            scale: 1.0,
            remap: None,
        }),
        opacity: Animated::Track(Track::new(vec![
            Keyframe::new(0.0, 0.2, Easing::Linear),
            Keyframe::new(1.0, 0.4, Easing::Linear),
        ])),
        children: vec![Node::shape(square())],
        ..Group::new()
    };
    let s = scene(Group {
        children: vec![Node::Group(inner)],
        ..Group::new()
    });
    assert!(s.frame_at(0.5).items.is_empty());
    let items = s.frame_at(1.5).items;
    let Item::PushLayer(layer) = &items[0] else {
        panic!()
    };
    assert!((layer.opacity - 0.3).abs() < 1e-9);
}
