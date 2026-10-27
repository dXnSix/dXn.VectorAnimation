//! Blend states as Rive's runtime plays them: a 1D blend mixing the two
//! animations around its number input's value, a direct blend mixing each
//! by its own input or value; each applied in turn over what the property
//! had (a frame before, or its reset).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, key, scene, ty};
use dxn_vector_animation::{Limits, Scene};
use dxn_vector_animation_rive::{InputChange, InputValue, Options, read_options};

const RED: [u8; 3] = [255, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];

/// A 100 × 50 artboard with a red 10 × 10 square (shape 1) at (10, 25),
/// animations holding its x: "left" (0) at 10 and "right" (1) at 90, a
/// number input "x" at `x`, and a state machine whose layer enters the
/// blend state `blend` (its type and properties, then its animations').
fn blended(x: f64, blend: (u16, Vec<(u16, V)>), parts: Vec<(u16, Vec<(u16, V)>)>) -> Riv {
    let mut riv =
        Riv::new()
            .artboard(100.0, 50.0)
            .rect_shape(0, 10.0, 25.0, 10.0, 10.0, 0xFFFF_0000);
    for (name, value) in [("left", 10.0), ("right", 90.0)] {
        riv = riv
            .obj(
                ty::LINEAR_ANIMATION,
                &[
                    (key::ANIMATION_NAME, V::S(name)),
                    (key::FPS, V::U(10)),
                    (key::DURATION, V::U(10)),
                ],
            )
            .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(1))])
            .obj(
                ty::KEYED_PROPERTY,
                &[(key::PROPERTY_KEY, V::U(u64::from(key::X)))],
            )
            .obj(
                ty::KEY_FRAME_DOUBLE,
                &[(key::FRAME, V::U(0)), (key::VALUE, V::D(value))],
            );
    }
    riv = riv
        .obj(
            ty::STATE_MACHINE,
            &[(key::ANIMATION_NAME, V::S("State Machine 1"))],
        )
        .obj(
            ty::STATE_MACHINE_NUMBER,
            &[
                (key::INPUT_NAME, V::S("x")),
                (key::NUMBER_VALUE, V::D(x as f32)),
            ],
        )
        .obj(ty::STATE_MACHINE_LAYER, &[])
        .obj(ty::ENTRY_STATE, &[])
        .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(3))])
        .obj(ty::ANY_STATE, &[])
        .obj(ty::EXIT_STATE, &[])
        .obj(blend.0, &blend.1);
    for (t, props) in parts {
        riv = riv.obj(t, &props);
    }
    riv
}

/// A 1D blend by input "x" of "left" at 0 and "right" at 100, resetting
/// when `reset` says.
fn one_d(x: f64, reset: bool) -> Riv {
    let part = |anim, value| {
        (
            ty::BLEND_ANIMATION_1D,
            vec![
                (key::BLEND_ANIMATION_ID, V::U(anim)),
                (key::BLEND_VALUE, V::D(value)),
            ],
        )
    };
    blended(
        x,
        (
            ty::BLEND_STATE_1D_INPUT,
            vec![
                (key::BLEND_INPUT, V::U(0)),
                (key::STATE_FLAGS, V::U(if reset { 2 } else { 0 })),
            ],
        ),
        vec![part(0, 0.0), part(1, 100.0)],
    )
}

/// The red square's left edge at `t`, or `None` where it is not on the
/// row (scanned a unit at a time).
fn left_edge(s: &Scene, t: f64) -> Option<u32> {
    (0..100).find(|&x| color_at(s, t, x, 25) == RED)
}

fn with_x(riv: &Riv, changes: &[(f64, f64)]) -> Scene {
    let options = Options {
        inputs: changes
            .iter()
            .map(|&(time, x)| InputChange {
                time,
                input: "x".into(),
                value: InputValue::Number(x),
            })
            .collect(),
        ..Options::default()
    };
    read_options(&riv.bytes(), &Limits::default(), &options)
        .unwrap()
        .scene
}

#[test]
fn a_1d_blend_mixes_the_animations_around_its_input() {
    // At 25 a quarter of the way from 10 to 90: 30; at 75 (from 0.5 s), 70.
    let riv = one_d(25.0, true);
    let _ = scene(&riv);
    let s = with_x(&riv, &[(0.5, 75.0)]);
    assert_eq!(left_edge(&s, 0.2), Some(25));
    assert_eq!(left_edge(&s, 0.8), Some(65));
    // On an animation's value, that animation alone.
    let s = with_x(&riv, &[(0.5, 100.0)]);
    assert_eq!(left_edge(&s, 0.8), Some(85));
    assert_eq!(color_at(&s, 0.8, 50, 25), WHITE);
}

#[test]
fn a_1d_blend_that_does_not_reset_mixes_over_what_the_property_had() {
    // Each frame from the last one's value: 10, then 30, then towards the
    // value where mixing it in again leaves it (34.6 for 25).
    let riv = one_d(25.0, false);
    let s = scene(&riv);
    assert_eq!(left_edge(&s, 0.0), Some(25));
    assert_eq!(left_edge(&s, 0.5), Some(30));
}

/// A direct blend of "right" mixed by input "x" (`source` 0) or by the fixed
/// `mix` (`source` 1).
fn direct(source: u64, mix: f32) -> Riv {
    let extra = if source == 0 {
        (key::DIRECT_INPUT, V::U(0))
    } else {
        (key::MIX_VALUE, V::D(mix))
    };
    blended(
        50.0,
        (ty::BLEND_STATE_DIRECT, vec![]),
        vec![(
            ty::BLEND_ANIMATION_DIRECT,
            vec![
                (key::BLEND_ANIMATION_ID, V::U(1)),
                (key::BLEND_SOURCE, V::U(source)),
                extra,
            ],
        )],
    )
}

#[test]
fn a_direct_blend_mixes_an_animation_by_its_input() {
    // "right" by input "x" (50 of 100): each frame halfway from what the
    // square had towards 90, so soon at 90.
    let s = scene(&direct(0, 0.0));
    assert_eq!(left_edge(&s, 0.0), Some(45));
    assert_eq!(left_edge(&s, 0.5), Some(85));
}

#[test]
fn a_direct_blend_mixes_an_animation_by_its_value() {
    // A fixed mix of 0 leaves the square where it is; of 100, at 90.
    assert_eq!(left_edge(&scene(&direct(1, 100.0)), 0.5), Some(85));
    assert_eq!(left_edge(&scene(&direct(1, 0.0)), 0.5), Some(5));
}

#[test]
fn a_blend_state_mixes_into_the_next_state() {
    // The 1D blend at 25 (x 30); past 50 (100 from 0.3 s) the layer goes
    // to "left" over half a second, mixing the blend (now "right" alone,
    // at 90) out.
    let riv = one_d(25.0, true)
        .obj(
            ty::STATE_TRANSITION,
            &[
                (key::STATE_TO_ID, V::U(4)),
                (key::TRANSITION_DURATION, V::U(500)),
            ],
        )
        .obj(
            ty::TRANSITION_NUMBER_CONDITION,
            &[
                (key::CONDITION_INPUT, V::U(0)),
                (key::CONDITION_OP, V::U(5)),
                (key::CONDITION_VALUE, V::D(50.0)),
            ],
        )
        .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(0))]);
    let _ = scene(&riv);
    let s = with_x(&riv, &[(0.3, 100.0)]);
    assert_eq!(left_edge(&s, 0.2), Some(25));
    // Halfway through the mix, between 90 and 10.
    let mid = left_edge(&s, 0.55).unwrap();
    assert!((20..80).contains(&mid), "{mid}");
    assert_eq!(left_edge(&s, 1.0), Some(5));
}
