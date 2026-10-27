//! Outlines whose commands change many times play on the graph: the
//! samples of the same commands share one contour, its points each run's
//! in turn, shown during any of them; so it is how many different
//! commands an outline has that is bounded, not how often they change.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{differing, plan_pixels, reference_pixels};
use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Easing, Fill, Geometry, Group, Keyframe, LineCap,
    LineJoin, LoopMode, Matrix, Node, Paint, Scene, Shape, ShapePath, Star, StarKind, Stroke,
    Timeline, Track,
};
use dxn_vector_animation_nsh::plan;

/// Every tenth of a second, and either side of the switches of the morphs
/// below (halfway between keyframes).
fn times() -> Vec<f64> {
    let mut t: Vec<f64> = (0..=20).map(|k| f64::from(k) * 0.1).collect();
    t.extend([0.499, 0.501, 0.999, 1.001, 1.499, 1.501]);
    t
}

/// Asserts the plan plays `scene` on the graph alone and draws it as the
/// reference does.
fn assert_native(scene: &Scene) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    for t in times() {
        let share = differing(
            &plan_pixels(scene, t, 100, 100),
            &reference_pixels(scene, t, 100, 100),
        );
        assert!(
            share <= 0.004,
            "at t = {t}: {:.2}% of the pixels differ",
            share * 100.0
        );
    }
}

fn track(keys: &[(f64, f64)]) -> Animated<f64> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|(t, v)| Keyframe::new(*t, *v, Easing::EASE_IN_OUT))
            .collect(),
    ))
}

fn fill(color: Color) -> Option<Fill> {
    Some(Fill {
        paint: Paint::solid(color),
        opacity: Animated::Static(1.0),
        rule: Default::default(),
    })
}

fn stroke(color: Color) -> Option<Stroke> {
    Some(Stroke {
        paint: Paint::solid(color),
        opacity: Animated::Static(1.0),
        width: Animated::Static(4.0),
        cap: LineCap::Round,
        join: LineJoin::Round,
        miter_limit: 4.0,
        dash: None,
    })
}

fn scene(geometry: Geometry) -> Scene {
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
                paths: vec![ShapePath::new(geometry)],
                fill: fill(Color::rgba(0.9, 0.3, 0.1, 1.0)),
                stroke: stroke(Color::rgba(0.1, 0.2, 0.6, 1.0)),
                ..Default::default()
            })],
            ..Group::new()
        },
    }
}

fn star(kind: StarKind) -> Star {
    Star {
        kind,
        center: AnimatedPoint::fixed(50.0, 50.0),
        points: Animated::Static(5.0),
        outer_radius: Animated::Static(40.0),
        inner_radius: Animated::Static(18.0),
        outer_roundness: Animated::Static(0.0),
        inner_roundness: Animated::Static(0.0),
        rotation: Animated::Static(0.0),
        direction: Direction::Clockwise,
    }
}

#[test]
fn a_star_switching_points_many_times_plays_on_the_graph() {
    // Five points and six, forty times over, turning all the while.
    let mut s = star(StarKind::Star);
    s.points = Animated::Track(Track::new(
        (0..=40)
            .map(|k| {
                let points = if k % 2 == 0 { 5.0 } else { 6.0 };
                Keyframe::new(f64::from(k) * 0.05, points, Easing::Hold)
            })
            .collect(),
    ));
    s.rotation = track(&[(0.0, 0.0), (2.0, 144.0)]);
    let geometry = Geometry::Star(s);
    let plan = plan::compile(&scene(geometry.clone()), Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    assert_native(&scene(geometry));
}

#[test]
fn an_outline_of_many_different_commands_is_still_sampled() {
    let mut s = star(StarKind::Polygon);
    s.points = track(&[(0.0, 3.0), (2.0, 60.0)]);
    let plan = plan::compile(&scene(Geometry::Star(s)), Matrix::IDENTITY).unwrap();
    assert_eq!(
        plan.sampled,
        vec!["an outline whose commands change many times".to_owned()]
    );
}
