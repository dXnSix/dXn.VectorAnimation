//! Dash patterns in frames: an odd list repeats to an even one, as SVG and
//! Canvas 2D repeat it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, Color, Dash, Geometry, Group, Item, LineCap, LineJoin, LoopMode, Node, Paint, Path,
    Point, Scene, Shape, ShapePath, Stroke, Timeline,
};

fn dashed(array: &[f64]) -> Scene {
    let mut p = Path::new();
    p.move_to(Point::new(0.0, 50.0));
    p.line_to(Point::new(100.0, 50.0));
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 1.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Loop,
            markers: vec![],
            cues: Vec::new(),
        },
        root: Group {
            children: vec![Node::shape(Shape {
                paths: vec![ShapePath::new(Geometry::path(p))],
                stroke: Some(Stroke {
                    paint: Paint::solid(Color::rgba(0.0, 0.0, 0.0, 1.0)),
                    opacity: Animated::Static(1.0),
                    width: Animated::Static(4.0),
                    cap: LineCap::Butt,
                    join: LineJoin::Miter,
                    miter_limit: 4.0,
                    dash: Some(Dash {
                        array: array.iter().map(|d| Animated::Static(*d)).collect(),
                        offset: Animated::Static(0.0),
                    }),
                }),
                ..Default::default()
            })],
            ..Group::new()
        },
    }
}

fn dash_of(scene: &Scene) -> Option<(Vec<f64>, f64)> {
    match scene.frame_at(0.0).items.as_slice() {
        [Item::Stroke { style, .. }] => style.dash.clone(),
        items => panic!("{items:?}"),
    }
}

#[test]
fn an_odd_list_repeats() {
    assert_eq!(
        dash_of(&dashed(&[10.0, 4.0, 2.0])),
        Some((vec![10.0, 4.0, 2.0, 10.0, 4.0, 2.0], 0.0))
    );
    assert_eq!(dash_of(&dashed(&[5.0])), Some((vec![5.0, 5.0], 0.0)));
}

#[test]
fn an_even_list_stays() {
    assert_eq!(dash_of(&dashed(&[10.0, 4.0])), Some((vec![10.0, 4.0], 0.0)));
}

#[test]
fn a_list_of_no_length_draws_solid() {
    assert_eq!(dash_of(&dashed(&[0.0, 0.0, 0.0])), None);
}
