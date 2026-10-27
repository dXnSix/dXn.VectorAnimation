//! Dash patterns whose lengths move round an ellipse: each dash an arc
//! between two angles the graph moves (a few nodes a dash), where a part of
//! any other outline moves each of its points; drawn as the reference draws
//! the dashes, either way round and from either start.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Dash, Direction, Easing, EllipseStart, Geometry, Group,
    Keyframe, LineCap, LineJoin, LoopMode, Matrix, Node, Paint, Scene, Shape, ShapePath, Stroke,
    Timeline, Track,
};
use dxn_vector_animation_nsh::plan;

const TIMES: &[f64] = &[0.0, 0.1, 0.35, 0.6, 0.71, 0.9, 1.2, 1.45, 1.7, 2.0];

/// The plan plays `scene` on the graph alone, with at most `per_dash`
/// nodes for each of `dashes` dashes, and draws what the reference does,
/// at three times the size, to `pixels` pixels of 90 000.
fn assert_arcs(scene: &Scene, dashes: usize, per_dash: usize, pixels: f64) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    assert!(
        plan.graph.len() <= dashes * per_dash,
        "{} nodes for {dashes} dashes",
        plan.graph.len()
    );
    let (w, h) = (300, 300);
    for &t in TIMES {
        let ours = common::plan_pixels(scene, t, w, h);
        let reference = common::reference_pixels(scene, t, w, h);
        let count = (common::differing(&ours, &reference) * f64::from(w * h)).round();
        assert!(count <= pixels, "at t = {t}: {count} pixels differ");
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
    path: ShapePath,
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
            loop_start: 0.0,
            loop_mode: LoopMode::Loop,
            markers: vec![],
            cues: Vec::new(),
        },
        root: Group {
            children: vec![Node::shape(Shape {
                paths: vec![path],
                stroke: Some(Stroke {
                    paint: Paint::solid(Color::rgba(0.1, 0.3, 0.8, 1.0)),
                    opacity: Animated::Static(1.0),
                    width: Animated::Static(4.0),
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

fn ellipse(rx: f64, ry: f64, direction: Direction, start: EllipseStart) -> ShapePath {
    ShapePath::new(Geometry::Ellipse {
        center: AnimatedPoint::fixed(50.0, 50.0),
        radius: AnimatedPoint::fixed(rx, ry),
        direction,
        start,
    })
}

/// The docs' `dash-slots`: a growing dash, a fixed gap, the offset moving.
fn growing(path: ShapePath, cap: LineCap) -> Scene {
    dashed(
        path,
        cap,
        vec![track(&[(0.0, 0.5), (2.0, 3.0)]), Animated::Static(2.5)],
        track(&[(0.0, 0.0), (2.0, -60.0)]),
    )
}

#[test]
fn dashes_round_a_circle_are_arcs_either_way_and_from_either_start() {
    // 251 long, periods from 3 to 5.5: about 86 slots.
    for direction in [Direction::Clockwise, Direction::CounterClockwise] {
        for start in [EllipseStart::Top, EllipseStart::Right] {
            let s = growing(ellipse(40.0, 40.0, direction, start), LineCap::Round);
            assert_arcs(&s, 86, 16, 90.0);
        }
    }
}

#[test]
fn dashes_round_an_ellipse_follow_its_length() {
    // Lengths along it are not angles: each end looks its angle up. The
    // reference measures them along its flattened outline, whose lengths
    // drift from the ellipse's a little round the way: dashes far along it
    // land a fraction of a pixel apart (as they did when a part of the
    // outline moved each of its points, up to 320 pixels).
    for direction in [Direction::Clockwise, Direction::CounterClockwise] {
        let s = growing(
            ellipse(44.0, 22.0, direction, EllipseStart::Top),
            LineCap::Butt,
        );
        assert_arcs(&s, 86, 16, 320.0);
    }
}

#[test]
fn a_dash_of_no_length_draws_its_caps_and_one_off_the_outline_draws_nothing() {
    // The dash shrinks to nothing and grows again: round caps make dots.
    let s = dashed(
        ellipse(40.0, 40.0, Direction::Clockwise, EllipseStart::Top),
        LineCap::Round,
        vec![
            track(&[(0.0, 6.0), (1.0, 0.0), (2.0, 6.0)]),
            Animated::Static(10.0),
        ],
        Animated::Static(0.0),
    );
    assert_arcs(&s, 26, 16, 90.0);
}
