//! Dash patterns whose lengths move, many dashes round the outline or an
//! offset running on for many periods: each repetition a slot going round
//! the outline as the offset moves (wrapping past its end), so as many
//! dashes as reach it at once are drawn, whatever the offset's travel.

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
fn a_hundred_dashes_round_a_circle_play_on_the_graph() {
    let s = dashed(
        vec![circle(45.0)],
        LineCap::Round,
        vec![track(&[(0.0, 0.5), (2.0, 2.0)]), Animated::Static(2.0)],
        Animated::Static(0.0),
    );
    assert_native(&s);
}

#[test]
fn an_offset_running_on_for_many_periods_plays_on_the_graph() {
    // Ten periods of travel along a zigzag, the dash growing.
    let s = dashed(
        vec![zigzag()],
        LineCap::Butt,
        vec![track(&[(0.0, 4.0), (2.0, 10.0)]), Animated::Static(8.0)],
        track(&[(0.0, 0.0), (2.0, -150.0)]),
    );
    assert_native(&s);
}
