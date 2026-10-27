//! Still translations one after another (Lottie's position, then less its
//! anchor) are one translation of the canvas.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Fill, Geometry, Group, Matrix, Node, Paint,
    RectStart, Scene, Shape, ShapePath, Timeline, TransformOp,
};
use dxn_vector_animation_nsh::plan::{self, Op, Unit};

fn square(x: f64, y: f64, transform: Vec<TransformOp>) -> Node {
    Node::Group(Group {
        transform,
        children: vec![Node::shape(Shape {
            paths: vec![ShapePath::new(Geometry::Rect {
                center: AnimatedPoint::fixed(x, y),
                size: AnimatedPoint::fixed(20.0, 20.0),
                radius: AnimatedPoint::fixed(0.0, 0.0),
                direction: Direction::Clockwise,
                start: RectStart::TopLeft,
            })],
            fill: Some(Fill {
                paint: Paint::solid(Color::rgba(0.0, 0.5, 0.5, 1.0)),
                opacity: Animated::Static(1.0),
                rule: Default::default(),
            }),
            ..Default::default()
        })],
        ..Group::new()
    })
}

#[test]
fn still_translations_in_a_row_are_one() {
    let scene = Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 1.0,
            ..Default::default()
        },
        root: Group {
            children: vec![
                square(
                    10.0,
                    10.0,
                    vec![
                        TransformOp::translate(40.0, 30.0),
                        TransformOp::translate(-10.0, -5.0),
                    ],
                ),
                // Ones that undo each other: none.
                square(
                    70.0,
                    70.0,
                    vec![
                        TransformOp::translate(10.0, 10.0),
                        TransformOp::translate(-10.0, -10.0),
                    ],
                ),
            ],
            ..Group::new()
        },
    };
    let p = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    let Some(Unit::Canvas(ops)) = p.units.first() else {
        panic!("a canvas");
    };
    let translations = ops
        .iter()
        .filter(|o| matches!(o, Op::Translate(..)))
        .count();
    assert_eq!(translations, 1, "{ops:?}");
    common::assert_plan_matches(&scene, &[0.0], 100, 100);
}
