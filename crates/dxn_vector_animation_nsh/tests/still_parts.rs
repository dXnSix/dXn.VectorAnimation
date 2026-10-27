//! A scene draws the same at every time when nothing in it changes over
//! time and nothing in it shows only for a time; else its frames say.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Fill, Geometry, Group, Keyframe, Mask, MaskKind,
    Node, Paint, RectStart, Scene, Shape, ShapePath, TimeRange, Timeline, Track,
};
use dxn_vector_animation_nsh::plan;

fn square(opacity: Animated<f64>) -> Node {
    Node::shape(Shape {
        paths: vec![ShapePath::new(Geometry::Rect {
            center: AnimatedPoint::fixed(10.0, 10.0),
            size: AnimatedPoint::fixed(10.0, 10.0),
            radius: AnimatedPoint::fixed(0.0, 0.0),
            direction: Direction::Clockwise,
            start: RectStart::TopLeft,
        })],
        fill: Some(Fill {
            paint: Paint::solid(Color::rgba(0.0, 0.5, 0.5, 1.0)),
            opacity,
            rule: Default::default(),
        }),
        ..Default::default()
    })
}

fn scene(children: Vec<Node>) -> Scene {
    Scene {
        width: 20.0,
        height: 20.0,
        timeline: Timeline {
            duration: 2.0,
            ..Default::default()
        },
        root: Group {
            children,
            ..Group::new()
        },
    }
}

fn shown(from: f64, to: f64, child: Node) -> Node {
    Node::Group(Group {
        active: Some(TimeRange {
            start: from,
            end: to,
        }),
        children: vec![child],
        ..Group::new()
    })
}

#[test]
fn a_scene_of_nothing_that_changes_is_still() {
    assert!(plan::still(&scene(vec![square(Animated::Static(1.0))])));
}

#[test]
fn a_scene_that_moves_or_shows_for_a_time_is_not() {
    let fading = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, 1.0, Default::default()),
        Keyframe::new(2.0, 0.0, Default::default()),
    ]));
    assert!(!plan::still(&scene(vec![square(fading)])));
    assert!(!plan::still(&scene(vec![shown(
        0.5,
        1.5,
        square(Animated::Static(1.0))
    )])));
    // In a mask too.
    let masked = Node::Group(Group {
        mask: Some(Box::new(Mask {
            kind: MaskKind::Alpha,
            inverted: false,
            content: Group {
                children: vec![shown(0.5, 1.5, square(Animated::Static(1.0)))],
                ..Group::new()
            },
        })),
        children: vec![square(Animated::Static(1.0))],
        ..Group::new()
    });
    assert!(!plan::still(&scene(vec![masked])));
}

#[test]
fn a_part_shown_throughout_is_still() {
    // A range wider than the timeline draws alike at every time: its
    // frames say so.
    assert!(plan::still(&scene(vec![shown(
        -1.0,
        10.0,
        square(Animated::Static(1.0))
    )])));
}
