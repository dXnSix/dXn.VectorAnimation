//! Gradient stops that change in number play on the graph: the stops held
//! until they switch (halfway through the eased change, as the reference
//! switches them), then the new ones, moving and fading as before and
//! after.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Easing, EllipseStart, Fill, Geometry, GradientStop,
    GradientUnits, Group, Keyframe, LinearGradient, LoopMode, Matrix, Node, Paint, RadialGradient,
    Scene, Shape, ShapePath, Spread, Timeline, Track,
};
use dxn_vector_animation_nsh::plan;

const RED: Color = Color::rgba(0.9, 0.1, 0.1, 1.0);
const GOLD: Color = Color::rgba(1.0, 0.8, 0.1, 1.0);
const BLUE: Color = Color::rgba(0.1, 0.3, 0.9, 1.0);
const CLEAR: Color = Color::rgba(0.1, 0.3, 0.9, 0.0);

/// Times between samples as well as at keyframes.
fn times() -> Vec<f64> {
    (0..=40).map(|k| f64::from(k) * 0.05 + 0.013).collect()
}

/// The plan plays `scene` on the graph alone and draws what the reference
/// does, colors within a tenth.
fn assert_native(scene: &Scene) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    for t in times() {
        let share = common::differing_by(
            &common::plan_pixels(scene, t, 100, 100),
            &common::reference_pixels(scene, t, 100, 100),
            24,
        );
        assert!(
            share <= 0.004,
            "at t = {t}: {:.2}% of the pixels differ",
            share * 100.0
        );
    }
}

fn stops(list: &[(f64, Color)]) -> Vec<GradientStop> {
    list.iter()
        .map(|&(offset, color)| GradientStop { offset, color })
        .collect()
}

fn stop_track(keys: Vec<(f64, Vec<GradientStop>, Easing)>) -> Animated<Vec<GradientStop>> {
    Animated::Track(Track::new(
        keys.into_iter()
            .map(|(t, v, e)| Keyframe::new(t, v, e))
            .collect(),
    ))
}

fn filled(paint: Paint) -> Scene {
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
        root: Group {
            children: vec![Node::shape(Shape {
                paths: vec![ShapePath::new(Geometry::Ellipse {
                    center: AnimatedPoint::fixed(50.0, 50.0),
                    radius: AnimatedPoint::fixed(45.0, 40.0),
                    direction: Direction::Clockwise,
                    start: EllipseStart::Top,
                })],
                fill: Some(Fill {
                    paint,
                    opacity: Animated::Static(1.0),
                    rule: Default::default(),
                }),
                ..Default::default()
            })],
            ..Group::new()
        },
    }
}

fn linear(stops: Animated<Vec<GradientStop>>) -> Paint {
    Paint::LinearGradient(LinearGradient {
        start: AnimatedPoint::fixed(5.0, 0.0),
        end: AnimatedPoint::fixed(95.0, 0.0),
        stops,
        units: GradientUnits::UserSpace,
        transform: Matrix::IDENTITY,
        spread: Spread::Pad,
    })
}

fn radial(
    center: AnimatedPoint,
    radius: Animated<f64>,
    stops: Animated<Vec<GradientStop>>,
) -> Paint {
    Paint::RadialGradient(RadialGradient {
        focal: center.clone(),
        center,
        radius,
        stops,
        units: GradientUnits::UserSpace,
        transform: Matrix::IDENTITY,
        spread: Spread::Pad,
    })
}

#[test]
fn stops_gaining_and_losing_some_switch_halfway() {
    // Two stops, three (eased in and out: the switch where the easing is
    // halfway), then two again moving.
    let paint = linear(stop_track(vec![
        (0.0, stops(&[(0.0, RED), (1.0, BLUE)]), Easing::EASE_IN_OUT),
        (
            0.8,
            stops(&[(0.0, RED), (0.5, GOLD), (1.0, BLUE)]),
            Easing::Linear,
        ),
        (1.3, stops(&[(0.2, GOLD), (0.9, BLUE)]), Easing::Linear),
        (2.0, stops(&[(0.5, RED), (1.0, GOLD)]), Easing::Linear),
    ]));
    assert_native(&filled(paint));
}

#[test]
fn radial_stops_changing_in_number_play_on_the_graph() {
    let paint = radial(
        AnimatedPoint::fixed(50.0, 50.0),
        Animated::Static(45.0),
        stop_track(vec![
            (0.0, stops(&[(0.0, GOLD), (1.0, BLUE)]), Easing::Linear),
            (
                1.0,
                stops(&[(0.0, GOLD), (0.4, RED), (0.6, RED), (1.0, CLEAR)]),
                Easing::Hold,
            ),
            (2.0, stops(&[(0.0, BLUE), (1.0, RED)]), Easing::Linear),
        ]),
    );
    assert_native(&filled(paint));
}
