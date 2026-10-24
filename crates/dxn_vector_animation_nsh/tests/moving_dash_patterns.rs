//! Dash patterns whose lengths move, of several dashes or repeating along
//! the outline, play on the graph: each dash of each repetition that
//! reaches the outline is a part whose ends the graph moves.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Dash, Direction, Easing, EllipseStart, Geometry, Group,
    Keyframe, LineCap, LineJoin, LoopMode, Matrix, Node, Paint, Path, PathEl, Point, Scene, Shape,
    ShapePath, Stroke, Timeline, Track,
};
use dxn_vector_animation_nsh::plan;

const TIMES: &[f64] = &[0.0, 0.1, 0.35, 0.6, 0.71, 0.9, 1.2, 1.45, 1.7, 2.0];

/// The plan plays `scene` on the graph alone and draws what the reference
/// does, at three times the size, to a tenth of a percent of the pixels:
/// dashes far along a curve end a fraction of a pixel apart, as the
/// reference places them along its flattened outline.
fn assert_native(scene: &Scene) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    let (w, h) = (300, 300);
    common::assert_plan_matches(scene, TIMES, w, h);
    for &t in TIMES {
        let ours = common::plan_pixels(scene, t, w, h);
        let reference = common::reference_pixels(scene, t, w, h);
        let count = (common::differing(&ours, &reference) * f64::from(w * h)).round();
        assert!(count <= 90.0, "at t = {t}: {count} pixels differ");
    }
}

fn track(keys: &[(f64, f64)]) -> Animated<f64> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|(t, v)| Keyframe::new(*t, *v, Easing::EASE_IN_OUT))
            .collect(),
    ))
}

fn dashed(
    paths: Vec<ShapePath>,
    cap: LineCap,
    array: Vec<Animated<f64>>,
    offset: Animated<f64>,
) -> Scene {
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
            children: vec![Node::shape(Shape {
                paths,
                stroke: Some(Stroke {
                    paint: Paint::solid(Color::rgba(0.1, 0.3, 0.8, 1.0)),
                    opacity: Animated::Static(1.0),
                    width: Animated::Static(5.0),
                    cap,
                    join: LineJoin::Round,
                    miter_limit: 4.0,
                    dash: Some(Dash { array, offset }),
                }),
                ..Default::default()
            })],
            ..Group::new()
        },
    }
}

fn circle(r: f64) -> ShapePath {
    ShapePath::new(Geometry::Ellipse {
        center: AnimatedPoint::fixed(50.0, 50.0),
        radius: AnimatedPoint::fixed(r, r),
        direction: Direction::Clockwise,
        start: EllipseStart::Top,
    })
}

fn zigzag() -> ShapePath {
    let p = Point::new;
    ShapePath::new(Geometry::path(Path(vec![
        PathEl::MoveTo(p(10.0, 80.0)),
        PathEl::LineTo(p(30.0, 20.0)),
        PathEl::LineTo(p(50.0, 80.0)),
        PathEl::CubicTo(p(60.0, 10.0), p(80.0, 10.0), p(90.0, 80.0)),
        PathEl::MoveTo(p(10.0, 90.0)),
        PathEl::LineTo(p(90.0, 92.0)),
    ])))
}

#[test]
fn two_dashes_that_grow_play_on_the_graph() {
    let s = dashed(
        vec![circle(35.0)],
        LineCap::Butt,
        vec![
            track(&[(0.0, 10.0), (2.0, 40.0)]),
            Animated::Static(8.0),
            track(&[(0.0, 30.0), (1.0, 5.0), (2.0, 20.0)]),
            track(&[(0.0, 15.0), (2.0, 4.0)]),
        ],
        track(&[(0.0, 0.0), (2.0, -60.0)]),
    );
    assert_native(&s);
}

#[test]
fn a_pattern_repeating_along_the_outline_plays_on_the_graph() {
    let s = dashed(
        vec![circle(30.0), zigzag()],
        LineCap::Round,
        vec![
            track(&[(0.0, 2.0), (2.0, 25.0)]),
            track(&[(0.0, 20.0), (2.0, 6.0)]),
        ],
        track(&[(0.0, 0.0), (2.0, 45.0)]),
    );
    assert_native(&s);
}

#[test]
fn an_odd_list_repeats_to_an_even_one() {
    let s = dashed(
        vec![zigzag()],
        LineCap::Square,
        vec![
            track(&[(0.0, 12.0), (2.0, 30.0)]),
            Animated::Static(6.0),
            track(&[(0.0, 3.0), (2.0, 10.0)]),
        ],
        Animated::Static(5.0),
    );
    assert_native(&s);
}

#[test]
fn many_dashes_along_the_outline_are_still_sampled() {
    let s = dashed(
        vec![circle(45.0)],
        LineCap::Round,
        vec![track(&[(0.0, 0.5), (2.0, 2.0)]), Animated::Static(2.0)],
        Animated::Static(0.0),
    );
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert_eq!(
        plan.sampled,
        vec!["an animated dash pattern of many dashes along the outline".to_owned()]
    );
}
