//! Compiled plans draw what the scene's frames show, at every time.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::assert_plan_matches;
use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Easing, EllipseStart, Fill, Geometry, Group,
    Keyframe, LineCap, LoopMode, Matrix, MotionPath, MotionRotate, Node, Paint, Path, Point,
    RectStart, Scene, Shape, ShapePath, Stroke, TimeMap, TimeRange, Timeline, Track, TransformOp,
    Trim, TrimMode,
};

fn track(keys: &[(f64, f64, Easing)]) -> Animated<f64> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|(t, v, e)| Keyframe::new(*t, *v, e.clone()))
            .collect(),
    ))
}

fn scene(children: Vec<Node>) -> Scene {
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Loop,
            markers: vec![],
            cues: Vec::new(),
        },
        root: Group {
            children,
            ..Group::new()
        },
    }
}

fn rect(x: f64, y: f64, w: f64, h: f64, color: Color) -> Node {
    Node::shape(Shape {
        paths: vec![ShapePath::new(Geometry::Rect {
            center: AnimatedPoint::fixed(x + w / 2.0, y + h / 2.0),
            size: AnimatedPoint::fixed(w, h),
            radius: AnimatedPoint::fixed(0.0, 0.0),
            direction: Direction::Clockwise,
            start: RectStart::TopLeft,
        })],
        fill: Some(Fill {
            paint: Paint::solid(color),
            opacity: Animated::Static(1.0),
            rule: Default::default(),
        }),
        ..Default::default()
    })
}

const TIMES: &[f64] = &[0.0, 0.3, 0.5, 0.77, 1.0, 1.4, 2.0];

#[test]
fn figma_svg() {
    let scene =
        dxn_vector_animation_svg::read(include_bytes!("../../../fixtures/figma-motion.svg"))
            .unwrap()
            .scene;
    assert_plan_matches(&scene, TIMES, 200, 100);
}

#[test]
fn lottie() {
    let scene =
        dxn_vector_animation_lottie::read(include_bytes!("../../../fixtures/lottie-motion.json"))
            .unwrap()
            .scene;
    assert_plan_matches(&scene, TIMES, 100, 100);
}

#[test]
fn mixed_and_overshooting_easings() {
    let x = track(&[
        (0.0, 0.0, Easing::Hold),
        (0.4, 30.0, Easing::cubic_bezier(0.34, 1.56, 0.64, 1.0)),
        (1.2, 60.0, Easing::Linear),
        (2.0, 10.0, Easing::Linear),
    ]);
    let group = Group {
        transform: vec![TransformOp::Translate {
            x,
            y: Animated::Static(0.0),
        }],
        children: vec![rect(
            10.0,
            10.0,
            20.0,
            20.0,
            Color::rgba(1.0, 0.0, 0.0, 1.0),
        )],
        ..Group::new()
    };
    assert_plan_matches(
        &scene(vec![Node::Group(group)]),
        &[0.2, 0.5, 0.8, 1.0, 1.3, 1.9],
        100,
        100,
    );
}

#[test]
fn faded_overlapping_shapes_fade_as_one() {
    let group = Group {
        opacity: track(&[(0.0, 0.2, Easing::Linear), (2.0, 0.9, Easing::Linear)]),
        children: vec![
            rect(10.0, 10.0, 50.0, 50.0, Color::rgba(1.0, 0.0, 0.0, 1.0)),
            rect(30.0, 30.0, 50.0, 50.0, Color::rgba(0.0, 0.0, 1.0, 1.0)),
        ],
        ..Group::new()
    };
    let s = scene(vec![Node::Group(group)]);
    let plan = dxn_vector_animation_nsh::plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert!(
        plan.units
            .iter()
            .any(|u| matches!(u, dxn_vector_animation_nsh::plan::Unit::Layer { .. }))
    );
    assert_plan_matches(&s, &[0.0, 1.0, 2.0], 100, 100);
}

#[test]
fn colors_paths_and_matrices() {
    let mut a = Path::new();
    a.move_to(Point::new(10.0, 10.0));
    a.line_to(Point::new(90.0, 10.0));
    a.line_to(Point::new(50.0, 40.0));
    a.close();
    let mut b = Path::new();
    b.move_to(Point::new(10.0, 60.0));
    b.line_to(Point::new(90.0, 40.0));
    b.line_to(Point::new(50.0, 90.0));
    b.close();
    let morph = Node::shape(Shape {
        paths: vec![ShapePath::new(Geometry::Path {
            path: Animated::Track(Track::new(vec![
                Keyframe::new(0.0, a, Easing::EASE_IN_OUT),
                Keyframe::new(2.0, b, Easing::Linear),
            ])),
        })],
        fill: Some(Fill {
            paint: Paint::Solid {
                color: Animated::Track(Track::new(vec![
                    Keyframe::new(0.0, Color::rgba(1.0, 0.0, 0.0, 1.0), Easing::Hold),
                    Keyframe::new(0.5, Color::rgba(0.0, 1.0, 0.0, 1.0), Easing::EASE),
                    Keyframe::new(2.0, Color::rgba(0.0, 0.0, 1.0, 0.5), Easing::Linear),
                ])),
            },
            opacity: Animated::Static(1.0),
            rule: Default::default(),
        }),
        ..Default::default()
    });
    let spun = Group {
        transform: vec![
            TransformOp::translate(50.0, 50.0),
            TransformOp::Matrix {
                matrix: Animated::Track(Track::new(vec![
                    Keyframe::new(0.0, Matrix::IDENTITY, Easing::Linear),
                    Keyframe::new(
                        2.0,
                        Matrix::rotate(170.0) * Matrix::scale(0.5, 1.5),
                        Easing::Linear,
                    ),
                ])),
            },
            TransformOp::Skew {
                x: track(&[(0.0, 0.0, Easing::Linear), (2.0, 30.0, Easing::Linear)]),
                y: Animated::Static(0.0),
            },
        ],
        children: vec![rect(-10.0, -10.0, 20.0, 20.0, Color::BLACK)],
        ..Group::new()
    };
    assert_plan_matches(
        &scene(vec![morph, Node::Group(spun)]),
        &[0.0, 0.25, 0.6, 1.1, 1.7, 2.0],
        100,
        100,
    );
}

#[test]
fn motion_time_maps_and_trims() {
    let mut circle_path = Path::new();
    circle_path.move_to(Point::new(50.0, 20.0));
    circle_path.cubic_to(
        Point::new(70.0, 20.0),
        Point::new(80.0, 30.0),
        Point::new(80.0, 50.0),
    );
    circle_path.cubic_to(
        Point::new(80.0, 70.0),
        Point::new(70.0, 80.0),
        Point::new(50.0, 80.0),
    );
    circle_path.cubic_to(
        Point::new(30.0, 80.0),
        Point::new(20.0, 70.0),
        Point::new(20.0, 50.0),
    );
    circle_path.cubic_to(
        Point::new(20.0, 30.0),
        Point::new(30.0, 20.0),
        Point::new(50.0, 20.0),
    );
    circle_path.close();
    let rider = Group {
        transform: vec![TransformOp::Motion(MotionPath {
            path: circle_path,
            progress: track(&[(0.0, 0.0, Easing::Linear), (2.0, 1.25, Easing::Linear)]),
            rotate: MotionRotate::Auto { offset: 0.0 },
        })],
        children: vec![rect(-6.0, -3.0, 12.0, 6.0, Color::rgba(1.0, 0.5, 0.0, 1.0))],
        ..Group::new()
    };
    let delayed = Group {
        active: Some(TimeRange {
            start: 0.5,
            end: 1.8,
        }),
        time: Some(TimeMap {
            offset: 0.5,
            scale: 2.0,
            remap: None,
        }),
        transform: vec![TransformOp::Translate {
            x: track(&[(0.0, 0.0, Easing::Linear), (1.0, 40.0, Easing::Linear)]),
            y: Animated::Static(0.0),
        }],
        children: vec![rect(5.0, 85.0, 10.0, 10.0, Color::rgba(0.0, 0.5, 0.0, 1.0))],
        ..Group::new()
    };
    let ring = Node::shape(Shape {
        paths: vec![ShapePath::new(Geometry::Ellipse {
            center: AnimatedPoint::fixed(50.0, 50.0),
            radius: AnimatedPoint::fixed(12.0, 12.0),
            direction: Direction::Clockwise,
            start: EllipseStart::Top,
        })],
        stroke: Some(Stroke {
            paint: Paint::solid(Color::rgba(0.0, 0.0, 1.0, 1.0)),
            opacity: Animated::Static(1.0),
            width: Animated::Static(3.0),
            cap: LineCap::Butt,
            join: Default::default(),
            miter_limit: 4.0,
            dash: None,
        }),
        trim: Some(Trim {
            start: Animated::Static(0.0),
            end: track(&[(0.0, 0.0, Easing::Linear), (2.0, 1.0, Easing::Linear)]),
            offset: Animated::Static(0.0),
            mode: TrimMode::EachPath,
        }),
        ..Default::default()
    });
    assert_plan_matches(
        &scene(vec![Node::Group(rider), Node::Group(delayed), ring]),
        &[0.0, 0.4, 0.6, 1.0, 1.5, 1.85],
        100,
        100,
    );
}

#[test]
fn what_moves_only_in_a_marker_past_the_timeline_is_drawn() {
    // Still over the timeline's second, shown in a marker's span past it (a
    // Rive animation's): the plan is not one still frame.
    let mut shown = Group::new();
    shown.opacity = track(&[(0.0, 0.0, Easing::Hold), (2.0, 1.0, Easing::Hold)]);
    shown.children.push(rect(
        20.0,
        20.0,
        60.0,
        60.0,
        Color::rgba(1.0, 0.0, 0.0, 1.0),
    ));
    let mut s = scene(vec![Node::Group(shown)]);
    s.timeline.duration = 1.0;
    s.timeline.markers = vec![dxn_vector_animation::Marker::new("later", 2.0, 1.0)];
    assert!(!dxn_vector_animation_nsh::plan::still(&s));
    assert_plan_matches(&s, &[0.5, 2.5], 100, 100);
}

#[test]
fn the_comparison_sees_a_wrong_frame() {
    let scene =
        dxn_vector_animation_lottie::read(include_bytes!("../../../fixtures/lottie-motion.json"))
            .unwrap()
            .scene;
    let ours = common::plan_pixels(&scene, 0.0, 100, 100);
    let later = common::reference_pixels(&scene, 1.0, 100, 100);
    let share = common::differing(&ours, &later);
    assert!(share > 0.01, "{share}");
    let drawn = ours.pixels().iter().filter(|p| p.alpha() > 0).count();
    assert!(drawn > 300, "the plan draws: {drawn} pixels");
}
