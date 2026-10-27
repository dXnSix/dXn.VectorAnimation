//! Data binding: the artboard's view model's first instance bound, as
//! Rive's editor and players binding automatically have it; its values set
//! what they are bound to, and the state machine's conditions on them hold
//! as they compare.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, key, scene, ty};
use dxn_vector_animation::Limits;
use dxn_vector_animation_rive::{Options, read_options};

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

/// A view model with a number "level" (property 0) and a color "tint"
/// (property 1), its instance holding `level` and blue; a 100 × 50
/// artboard bound to it, with a red square (shape 1, its color object 4
/// bound to "tint") at (10, 25) and animations holding it at 10 ("left")
/// or at 70 ("right"); its state machine goes from "left" to "right" when
/// "level" is over 3.
fn bound(level: f32) -> Riv {
    let riv = Riv::new()
        .obj(ty::VIEW_MODEL, &[(key::VM_NAME, V::S("Game"))])
        .obj(ty::VM_NUMBER, &[(key::VM_NAME, V::S("level"))])
        .obj(ty::VM_COLOR, &[(key::VM_NAME, V::S("tint"))])
        .obj(ty::VM_INSTANCE, &[(key::VM_INSTANCE_VM, V::U(0))])
        .obj(
            ty::VM_INSTANCE_NUMBER,
            &[
                (key::VM_VALUE_PROPERTY, V::U(0)),
                (key::VM_NUMBER_VALUE, V::D(level)),
            ],
        )
        .obj(
            ty::VM_INSTANCE_COLOR,
            &[
                (key::VM_VALUE_PROPERTY, V::U(1)),
                (key::VM_COLOR_VALUE, V::C(0xFF00_00FF)),
            ],
        )
        .obj(
            ty::ARTBOARD,
            &[
                (key::NAME, V::S("Main")),
                (key::WIDTH, V::D(100.0)),
                (key::HEIGHT, V::D(50.0)),
                (key::ORIGIN_X, V::D(0.0)),
                (key::ORIGIN_Y, V::D(0.0)),
                (key::ARTBOARD_VIEW_MODEL, V::U(0)),
            ],
        )
        .rect_shape(0, 10.0, 25.0, 10.0, 10.0, 0xFFFF_0000)
        .obj(
            ty::DATA_BIND_CONTEXT,
            &[
                (key::BIND_PROPERTY_KEY, V::U(u64::from(key::COLOR))),
                (key::BIND_PATH, V::Bytes(vec![0, 1])),
            ],
        );
    let mut riv = riv;
    for (name, x) in [("left", 10.0), ("right", 70.0)] {
        riv = riv
            .obj(
                ty::LINEAR_ANIMATION,
                &[
                    (key::ANIMATION_NAME, V::S(name)),
                    (key::FPS, V::U(10)),
                    (key::DURATION, V::U(1)),
                ],
            )
            .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(1))])
            .obj(
                ty::KEYED_PROPERTY,
                &[(key::PROPERTY_KEY, V::U(u64::from(key::X)))],
            )
            .obj(
                ty::KEY_FRAME_DOUBLE,
                &[(key::FRAME, V::U(0)), (key::VALUE, V::D(x))],
            );
    }
    riv.obj(
        ty::STATE_MACHINE,
        &[(key::ANIMATION_NAME, V::S("State Machine 1"))],
    )
    .obj(ty::STATE_MACHINE_LAYER, &[])
    .obj(ty::ENTRY_STATE, &[])
    .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(3))])
    .obj(ty::ANY_STATE, &[])
    .obj(ty::EXIT_STATE, &[])
    .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(0))])
    .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(4))])
    .obj(ty::VM_CONDITION, &[(key::VM_CONDITION_OP, V::U(5))])
    .obj(ty::BINDABLE_NUMBER, &[])
    .obj(
        ty::DATA_BIND_CONTEXT,
        &[
            (key::BIND_PROPERTY_KEY, V::U(636)),
            (key::BIND_PATH, V::Bytes(vec![0, 0])),
        ],
    )
    .obj(ty::VM_PROPERTY_COMPARATOR, &[])
    .obj(
        ty::VALUE_NUMBER_COMPARATOR,
        &[(key::COMPARATOR_NUMBER, V::D(3.0))],
    )
    .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(1))])
}

#[test]
fn bound_values_set_what_they_are_bound_to() {
    let s = scene(&bound(1.0));
    assert_eq!(color_at(&s, 0.0, 10, 25), BLUE);
    // Read unbound, the file's own color.
    let unbound = Options {
        unbound: true,
        ..Options::default()
    };
    let s = read_options(&bound(1.0).bytes(), &Limits::default(), &unbound)
        .unwrap()
        .scene;
    assert_eq!(color_at(&s, 0.0, 10, 25), RED);
}

#[test]
fn conditions_on_bound_values_hold_as_they_compare() {
    // A level of 1 keeps the square left; 5 sends it right.
    let s = scene(&bound(1.0));
    assert_eq!(color_at(&s, 0.0, 10, 25), BLUE);
    let riv = bound(5.0);
    let s = scene(&riv);
    assert_eq!(color_at(&s, 0.05, 10, 25), WHITE);
    assert_eq!(color_at(&s, 0.05, 70, 25), BLUE);
}
