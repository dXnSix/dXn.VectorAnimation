//! Color effects and inner shadows in the scene: what each color op does
//! to a color, steps at a moment (matrices multiplied), and their frames.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, ColorEffect, ColorMatrix, ColorOp, Easing, Effect, FrameEffect,
    GradientStop, Group, Item, Keyframe, LoopMode, Node, Scene, Timeline, Track, Transfer,
    TransformOp,
    filter::{ColorStep, apply_steps, steps_at},
};

fn close(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

#[test]
fn css_functions_change_colors_as_the_spec_says() {
    let red = [1.0, 0.0, 0.0, 1.0];
    assert!(close(
        ColorMatrix::grayscale(1.0).apply(red),
        [0.2126, 0.2126, 0.2126, 1.0]
    ));
    assert!(close(ColorMatrix::saturate(1.0).apply(red), red));
    assert!(close(ColorMatrix::hue_rotate(360.0).apply(red), red));
    assert!(close(
        ColorMatrix::invert(1.0).apply(red),
        [0.0, 1.0, 1.0, 1.0]
    ));
    assert!(close(
        ColorMatrix::invert(0.5).apply(red),
        [0.5, 0.5, 0.5, 1.0]
    ));
    assert!(close(
        ColorMatrix::opacity(0.25).apply(red),
        [1.0, 0.0, 0.0, 0.25]
    ));
    assert!(close(
        ColorMatrix::contrast(0.5).apply(red),
        [0.75, 0.25, 0.25, 1.0]
    ));
    assert!(close(
        ColorMatrix::brightness(2.0).apply([0.3, 0.6, 0.1, 1.0]),
        [0.6, 1.0, 0.2, 1.0]
    ));
    assert!(ColorMatrix::sepia(1.0).keeps_alpha());
    assert!(!ColorMatrix::luminance_to_alpha().keeps_alpha());
    assert!(ColorMatrix::opacity(0.5).keeps_colors());
}

#[test]
fn lottie_tint_fill_and_overlay() {
    let black = Color::rgba(0.0, 0.0, 0.5, 1.0);
    let white = Color::rgba(1.0, 1.0, 0.5, 1.0);
    let tint = ColorMatrix::tint(black, white, 1.0);
    assert!(close(
        tint.apply([0.0, 0.0, 0.0, 0.7]),
        [0.0, 0.0, 0.5, 0.7]
    ));
    assert!(close(
        tint.apply([1.0, 1.0, 1.0, 0.7]),
        [1.0, 1.0, 0.5, 0.7]
    ));
    let overlay = ColorMatrix::overlay(Color::rgba(0.0, 1.0, 0.0, 1.0), 0.5);
    assert!(close(
        overlay.apply([1.0, 0.0, 0.0, 0.4]),
        [0.5, 0.5, 0.0, 0.4]
    ));
    // Lottie's Fill: the color, the alpha times the opacity.
    let fill = ColorMatrix::fill(Color::rgba(0.0, 1.0, 0.0, 1.0), 0.5);
    assert!(close(
        fill.apply([1.0, 0.0, 0.0, 0.4]),
        [0.0, 1.0, 0.0, 0.2]
    ));
}

#[test]
fn matrices_compose_in_order() {
    let a = ColorMatrix::hue_rotate(40.0);
    let b = ColorMatrix::contrast(1.3);
    let c = [0.2, 0.5, 0.7, 0.8];
    let both = a.then(&b).apply(c);
    let each = b.apply(a.apply(c));
    assert!(close(both, each), "{both:?} {each:?}");
}

#[test]
fn transfers_map_each_channel() {
    let table = Transfer::Table {
        values: vec![0.0, 1.0, 0.0],
    };
    assert!((table.apply(0.25) - 0.5).abs() < 1e-9);
    assert!((table.apply(0.75) - 0.5).abs() < 1e-9);
    let steps = Transfer::Discrete {
        values: vec![0.2, 0.8],
    };
    assert_eq!(steps.apply(0.49), 0.2);
    assert_eq!(steps.apply(0.5), 0.8);
    let gamma = Transfer::Gamma {
        amplitude: 1.0,
        exponent: 2.0,
        offset: 0.0,
    };
    assert!((gamma.apply(0.5) - 0.25).abs() < 1e-9);
    assert_eq!(
        Transfer::Linear {
            slope: 2.0,
            intercept: -0.5
        }
        .as_linear(),
        Some((2.0, -0.5))
    );
}

#[test]
fn steps_multiply_consecutive_matrices_and_drop_what_changes_nothing() {
    let s = |v: f64| Animated::Static(v);
    let ops = vec![
        ColorOp::Saturate { amount: s(1.0) },
        ColorOp::HueRotate { degrees: s(30.0) },
        ColorOp::Brightness { amount: s(0.5) },
        ColorOp::Transfer {
            red: Transfer::Discrete {
                values: vec![0.0, 1.0],
            },
            green: Transfer::Identity,
            blue: Transfer::Identity,
            alpha: Transfer::Identity,
        },
        ColorOp::Invert { amount: s(0.0) },
    ];
    let steps = steps_at(&ops, 0.0);
    assert_eq!(steps.len(), 2, "{steps:?}");
    assert!(matches!(steps[0], ColorStep::Matrix(_)));
    assert!(matches!(steps[1], ColorStep::Transfer(_)));
    // A gradient map: black to blue, white to yellow.
    let map = ColorStep::GradientMap {
        stops: vec![
            GradientStop {
                offset: 0.0,
                color: Color::rgba(0.0, 0.0, 1.0, 1.0),
            },
            GradientStop {
                offset: 1.0,
                color: Color::rgba(1.0, 1.0, 0.0, 1.0),
            },
        ],
        amount: 1.0,
    };
    assert!(close(
        apply_steps(std::slice::from_ref(&map), false, [1.0, 1.0, 1.0, 0.5]),
        [1.0, 1.0, 0.0, 0.5]
    ));
}

fn scene(effects: Vec<Effect>) -> Scene {
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_mode: LoopMode::Once,
            markers: vec![],
        },
        root: Group {
            children: vec![Node::Group(Group {
                transform: vec![TransformOp::scale(2.0, 2.0), TransformOp::rotate(90.0)],
                effects,
                ..Group::new()
            })],
            ..Group::new()
        },
    }
}

#[test]
fn frames_hold_inner_shadows_in_the_frames_lengths_and_color_steps() {
    let hue = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, 0.0, Easing::Linear),
        Keyframe::new(2.0, 180.0, Easing::Linear),
    ]));
    let s = scene(vec![
        Effect::InnerShadow(dxn_vector_animation::InnerShadow {
            color: Animated::Static(Color::BLACK),
            offset: AnimatedPoint::fixed(0.0, 3.0),
            blur: Animated::Static(2.0),
            hard: true,
        }),
        Effect::Color(ColorEffect {
            ops: vec![ColorOp::HueRotate { degrees: hue }],
            linear: false,
        }),
    ]);
    let frame = s.frame_at(1.0);
    let Item::PushLayer(layer) = &frame.items[0] else {
        panic!("{:?}", frame.items);
    };
    let FrameEffect::InnerShadow { offset, sigma, .. } = &layer.effects[0] else {
        panic!("{:?}", layer.effects);
    };
    // Turned a quarter and doubled.
    assert!(
        (offset.x + 6.0).abs() < 1e-9 && offset.y.abs() < 1e-9,
        "{offset:?}"
    );
    assert!((sigma - 4.0).abs() < 1e-9);
    let FrameEffect::Color { steps, linear } = &layer.effects[1] else {
        panic!("{:?}", layer.effects);
    };
    assert!(!linear);
    assert_eq!(
        steps,
        &vec![ColorStep::Matrix(ColorMatrix::hue_rotate(90.0))]
    );
    // At the start the hue turns by nothing: no color step at all.
    let frame = s.frame_at(0.0);
    let Item::PushLayer(layer) = &frame.items[0] else {
        panic!();
    };
    assert_eq!(layer.effects.len(), 1);
}
