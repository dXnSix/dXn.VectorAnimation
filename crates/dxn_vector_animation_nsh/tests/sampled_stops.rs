//! Gradient stops that move as bands cannot show (changing color too, or
//! in a radial gradient) play on the graph as gradients sampled over time
//! and faded into one another: close to the reference between samples,
//! exact at them.

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
fn linear_stops_moving_and_changing_color_play_on_the_graph() {
    let paint = linear(stop_track(vec![
        (
            0.0,
            stops(&[(0.0, RED), (0.3, GOLD), (1.0, BLUE)]),
            Easing::EASE_IN_OUT,
        ),
        (
            1.0,
            stops(&[(0.2, BLUE), (0.6, RED), (0.8, GOLD)]),
            Easing::Linear,
        ),
        (
            2.0,
            stops(&[(0.0, GOLD), (0.9, CLEAR), (1.0, RED)]),
            Easing::Linear,
        ),
    ]));
    assert_native(&filled(paint));
}

#[test]
fn radial_stops_that_move_play_on_the_graph() {
    let paint = radial(
        AnimatedPoint::fixed(50.0, 50.0),
        Animated::Static(45.0),
        stop_track(vec![
            (
                0.0,
                stops(&[(0.0, GOLD), (0.2, RED), (1.0, BLUE)]),
                Easing::EASE_IN_OUT,
            ),
            (
                2.0,
                stops(&[(0.0, GOLD), (0.8, RED), (1.0, BLUE)]),
                Easing::Linear,
            ),
        ]),
    );
    assert_native(&filled(paint));
}

#[test]
fn radial_stops_moving_in_a_moving_gradient_play_on_the_graph() {
    let paint = radial(
        AnimatedPoint {
            x: Animated::Track(Track::new(vec![
                Keyframe::new(0.0, 35.0, Easing::Linear),
                Keyframe::new(2.0, 65.0, Easing::Linear),
            ])),
            y: Animated::Static(50.0),
        },
        Animated::Track(Track::new(vec![
            Keyframe::new(0.0, 20.0, Easing::Linear),
            Keyframe::new(2.0, 50.0, Easing::Linear),
        ])),
        stop_track(vec![
            (0.0, stops(&[(0.1, RED), (0.5, CLEAR)]), Easing::Linear),
            (2.0, stops(&[(0.5, GOLD), (1.0, BLUE)]), Easing::Linear),
        ]),
    );
    assert_native(&filled(paint));
}

#[test]
fn held_stops_jump() {
    let paint = linear(stop_track(vec![
        (0.0, stops(&[(0.2, RED), (0.4, BLUE)]), Easing::Hold),
        (1.0, stops(&[(0.6, GOLD), (0.9, RED)]), Easing::Linear),
        (2.0, stops(&[(0.1, BLUE), (0.3, GOLD)]), Easing::Linear),
    ]));
    assert_native(&filled(paint));
}

#[test]
fn stops_moving_far_many_times_are_still_sampled() {
    let keys = (0..12)
        .map(|k| {
            let at = if k % 2 == 0 { 0.0 } else { 0.9 };
            let color = if k % 2 == 0 { RED } else { BLUE };
            (
                f64::from(k) / 6.0,
                stops(&[(at, color), (at + 0.1, GOLD)]),
                Easing::Linear,
            )
        })
        .collect();
    let plan = plan::compile(&filled(linear(stop_track(keys))), Matrix::IDENTITY).unwrap();
    assert_eq!(
        plan.sampled,
        vec!["gradient stops that move far as they change".to_owned()]
    );
}

#[test]
fn stops_fading_from_and_to_clear() {
    // Clear blue at first (its color shows nowhere), then gold as it moves.
    let paint = linear(stop_track(vec![
        (
            0.0,
            stops(&[(0.0, RED), (0.4, CLEAR), (1.0, RED)]),
            Easing::Linear,
        ),
        (
            2.0,
            stops(&[(0.0, RED), (0.7, GOLD), (1.0, RED)]),
            Easing::Linear,
        ),
    ]));
    assert_native(&filled(paint));
    // Changing color only, eased.
    let paint = linear(stop_track(vec![
        (
            0.0,
            stops(&[(0.0, RED), (0.5, CLEAR), (1.0, RED)]),
            Easing::EASE_IN_OUT,
        ),
        (
            1.0,
            stops(&[(0.0, RED), (0.5, GOLD), (1.0, RED)]),
            Easing::EASE_IN_OUT,
        ),
        (
            2.0,
            stops(&[(0.0, RED), (0.5, CLEAR), (1.0, RED)]),
            Easing::Linear,
        ),
    ]));
    assert_native(&filled(paint));
    // And out again to clear, then back.
    let paint = linear(stop_track(vec![
        (
            0.0,
            stops(&[(0.0, RED), (0.7, GOLD), (1.0, RED)]),
            Easing::Linear,
        ),
        (
            1.0,
            stops(&[(0.0, RED), (0.4, CLEAR), (1.0, RED)]),
            Easing::Linear,
        ),
        (
            2.0,
            stops(&[(0.0, RED), (0.6, BLUE), (1.0, RED)]),
            Easing::Linear,
        ),
    ]));
    assert_native(&filled(paint));
}
