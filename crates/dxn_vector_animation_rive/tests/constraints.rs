//! Constraints, applied as Rive's runtime applies them: translation,
//! rotation, scale, transform, distance, IK and follow-path.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use std::f32::consts::FRAC_PI_2;

use common::{Riv, V, color_at, key, near, scene, ty};

const RED: u32 = 0xFFFF_0000;
const WHITE: [u8; 3] = [255, 255, 255];
const RED_RGB: [u8; 3] = [255, 0, 0];

/// A second-long animation (10 frames at 10 fps) moving each (object,
/// property) from one value to another, linearly.
fn animation(riv: Riv, keys: &[(u64, u16, f32, f32)]) -> Riv {
    let mut riv = riv.obj(
        ty::LINEAR_ANIMATION,
        &[
            (key::ANIMATION_NAME, V::S("move")),
            (key::FPS, V::U(10)),
            (key::DURATION, V::U(10)),
        ],
    );
    for &(object, prop, from, to) in keys {
        riv = riv
            .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(object))])
            .obj(
                ty::KEYED_PROPERTY,
                &[(key::PROPERTY_KEY, V::U(u64::from(prop)))],
            );
        for (frame, value) in [(0, from), (10, to)] {
            riv = riv.obj(
                ty::KEY_FRAME_DOUBLE,
                &[
                    (key::FRAME, V::U(frame)),
                    (key::VALUE, V::D(value)),
                    (key::INTERPOLATION, V::U(1)),
                ],
            );
        }
    }
    riv
}

/// A node under `parent` at (`x`, `y`).
fn node(riv: Riv, parent: u64, x: f32, y: f32) -> Riv {
    riv.obj(
        ty::NODE,
        &[
            (key::PARENT_ID, V::U(parent)),
            (key::X, V::D(x)),
            (key::Y, V::D(y)),
        ],
    )
}

/// A 100 × 100 artboard with a target node (object 1) at (`x`, `y`) and a
/// red `w` × `h` rectangle in shape 2 at (`sx`, `sy`) (objects 2 to 5).
fn target_and_shape(x: f32, y: f32, sx: f32, sy: f32, w: f32, h: f32) -> Riv {
    node(Riv::new().artboard(100.0, 100.0), 0, x, y).rect_shape(0, sx, sy, w, h, RED)
}

/// Asserts the frame at `t` is red at each of `red` and white at each of
/// `white`.
fn check(riv: &Riv, t: f64, red: &[(u32, u32)], white: &[(u32, u32)]) {
    let s = scene(riv);
    for &(x, y) in red {
        let c = color_at(&s, t, x, y);
        assert!(near(c, RED_RGB, 2), "({x}, {y}) at {t}: {c:?}");
    }
    for &(x, y) in white {
        assert_eq!(color_at(&s, t, x, y), WHITE, "({x}, {y}) at {t}");
    }
}

#[test]
fn translation_constraints_pull_toward_their_target() {
    // Half way from (50, 70) to the target's x; y not copied.
    let riv = target_and_shape(10.0, 20.0, 50.0, 70.0, 10.0, 10.0).obj(
        ty::TRANSLATION_CONSTRAINT,
        &[
            (key::PARENT_ID, V::U(2)),
            (key::TARGET_ID, V::U(1)),
            (key::STRENGTH, V::D(0.5)),
            (key::DOES_COPY_Y, V::B(false)),
        ],
    );
    let riv = animation(riv, &[(1, key::X, 10.0, 90.0)]);
    check(&riv, 0.0, &[(30, 70)], &[(50, 70)]);
    check(&riv, 1.0, &[(70, 70)], &[(30, 70)]);
}

#[test]
fn rotation_constraints_turn_with_their_target() {
    let riv = target_and_shape(10.0, 10.0, 50.0, 50.0, 40.0, 10.0).obj(
        ty::ROTATION_CONSTRAINT,
        &[(key::PARENT_ID, V::U(2)), (key::TARGET_ID, V::U(1))],
    );
    let riv = animation(riv, &[(1, key::ROTATION, 0.0, FRAC_PI_2)]);
    check(&riv, 0.0, &[(35, 50)], &[(50, 35)]);
    check(&riv, 1.0, &[(50, 35)], &[(35, 50)]);
}

#[test]
fn scale_constraints_scale_with_their_target() {
    let riv = target_and_shape(10.0, 10.0, 50.0, 50.0, 10.0, 10.0).obj(
        ty::SCALE_CONSTRAINT,
        &[(key::PARENT_ID, V::U(2)), (key::TARGET_ID, V::U(1))],
    );
    let riv = animation(riv, &[(1, key::SCALE_X, 1.0, 3.0)]);
    check(&riv, 0.0, &[(50, 50)], &[(62, 50)]);
    check(&riv, 1.0, &[(62, 50)], &[(50, 57)]);
}

#[test]
fn transform_constraints_copy_the_whole_transform() {
    let riv = target_and_shape(20.0, 20.0, 80.0, 80.0, 20.0, 10.0).obj(
        ty::TRANSFORM_CONSTRAINT,
        &[(key::PARENT_ID, V::U(2)), (key::TARGET_ID, V::U(1))],
    );
    let riv = animation(
        riv,
        &[(1, key::X, 20.0, 60.0), (1, key::ROTATION, 0.0, FRAC_PI_2)],
    );
    check(&riv, 0.0, &[(27, 20)], &[(80, 80)]);
    check(&riv, 1.0, &[(60, 27)], &[(67, 20)]);
}

#[test]
fn distance_constraints_pull_within_their_distance() {
    let riv = target_and_shape(50.0, 50.0, 90.0, 50.0, 10.0, 10.0).obj(
        ty::DISTANCE_CONSTRAINT,
        &[
            (key::PARENT_ID, V::U(2)),
            (key::TARGET_ID, V::U(1)),
            (key::DISTANCE, V::D(20.0)),
        ],
    );
    let riv = animation(riv, &[(6, key::DISTANCE, 20.0, 30.0)]);
    check(&riv, 0.0, &[(70, 50)], &[(90, 50)]);
    check(&riv, 1.0, &[(80, 50)], &[(70, 50)]);
}

/// A red bar along each of `bones` (each `length` long) and a square at the
/// last one's tip.
fn bars(mut riv: Riv, bones: &[u64], length: f32) -> Riv {
    for &bone in bones {
        riv = riv.rect_shape(bone, length / 2.0, 0.0, length, 4.0, RED);
    }
    riv.rect_shape(*bones.last().unwrap(), length, 0.0, 8.0, 8.0, RED)
}

#[test]
fn ik_bends_two_bones_toward_their_target() {
    // Two 30-long bones from (20, 60); the target at (60, 30), then (35, 30).
    let riv = Riv::new()
        .artboard(100.0, 100.0)
        .obj(
            ty::ROOT_BONE,
            &[
                (key::PARENT_ID, V::U(0)),
                (key::ROOT_BONE_X, V::D(20.0)),
                (key::ROOT_BONE_Y, V::D(60.0)),
                (key::BONE_LENGTH, V::D(30.0)),
            ],
        )
        .obj(
            ty::BONE,
            &[(key::PARENT_ID, V::U(1)), (key::BONE_LENGTH, V::D(30.0))],
        );
    let riv = node(riv, 0, 60.0, 30.0).obj(
        ty::IK_CONSTRAINT,
        &[
            (key::PARENT_ID, V::U(2)),
            (key::TARGET_ID, V::U(3)),
            (key::PARENT_BONE_COUNT, V::U(1)),
        ],
    );
    let riv = animation(bars(riv, &[1, 2], 30.0), &[(3, key::X, 60.0, 35.0)]);
    check(&riv, 0.0, &[(60, 30)], &[(80, 60)]);
    check(&riv, 1.0, &[(35, 30)], &[(60, 30)]);
}

#[test]
fn ik_bends_longer_chains_the_other_way_when_inverted() {
    // Three 25-long bones from (20, 80), the target at (60, 40).
    let mut riv = Riv::new().artboard(100.0, 100.0).obj(
        ty::ROOT_BONE,
        &[
            (key::PARENT_ID, V::U(0)),
            (key::ROOT_BONE_X, V::D(20.0)),
            (key::ROOT_BONE_Y, V::D(80.0)),
            (key::BONE_LENGTH, V::D(25.0)),
        ],
    );
    for parent in [1, 2] {
        riv = riv.obj(
            ty::BONE,
            &[
                (key::PARENT_ID, V::U(parent)),
                (key::BONE_LENGTH, V::D(25.0)),
            ],
        );
    }
    let riv = node(riv, 0, 60.0, 40.0).obj(
        ty::IK_CONSTRAINT,
        &[
            (key::PARENT_ID, V::U(3)),
            (key::TARGET_ID, V::U(4)),
            (key::PARENT_BONE_COUNT, V::U(2)),
            (key::INVERT_DIRECTION, V::B(true)),
        ],
    );
    let riv = animation(bars(riv, &[1, 2, 3], 25.0), &[(4, key::Y, 40.0, 60.0)]);
    check(&riv, 0.0, &[(60, 40)], &[]);
    check(&riv, 1.0, &[(60, 60)], &[]);
}

#[test]
fn follow_path_constraints_ride_their_targets_outline() {
    // A ring of radius 30 about (50, 50), stroked; a square riding it from
    // its top, turned along it (off the ring's line, which is drawn over
    // it).
    let riv = Riv::new()
        .artboard(100.0, 100.0)
        .shape(0, 50.0, 50.0)
        .obj(
            ty::ELLIPSE,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::PATH_WIDTH, V::D(60.0)),
                (key::PATH_HEIGHT, V::D(60.0)),
            ],
        )
        .obj(
            ty::STROKE,
            &[(key::PARENT_ID, V::U(1)), (key::THICKNESS, V::D(1.0))],
        )
        .obj(
            ty::SOLID_COLOR,
            &[(key::PARENT_ID, V::U(3)), (key::COLOR, V::C(0xFF00_00FF))],
        )
        .rect_shape(0, 0.0, 0.0, 12.0, 6.0, RED)
        .obj(
            ty::FOLLOW_PATH_CONSTRAINT,
            &[(key::PARENT_ID, V::U(5)), (key::TARGET_ID, V::U(1))],
        );
    let riv = animation(riv, &[(9, key::PATH_DISTANCE, 0.0, 1.0)]);
    check(&riv, 0.0, &[(54, 22), (46, 18)], &[(50, 25)]);
    // A quarter round: on the right, upright.
    check(&riv, 0.25, &[(82, 54), (78, 46)], &[(85, 50)]);
    check(&riv, 0.5, &[(54, 78)], &[]);
}
