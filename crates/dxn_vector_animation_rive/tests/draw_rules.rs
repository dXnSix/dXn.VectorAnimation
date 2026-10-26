//! Draw rules: drawables moved before or after another, as Rive orders
//! them, the target keyed over time too.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, key, near, scene, ty};

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];

/// Three overlapping squares: red (shape 1) on top, green (shape 5) and
/// blue (shape 10, in node 9) at the bottom; rules 14 on the node with
/// targets 15 (red, `placement`) and 16 (green, in front of it), the first
/// active.
fn squares(placement: u64) -> Riv {
    Riv::new()
        .artboard(100.0, 100.0)
        .rect_shape(0, 40.0, 50.0, 30.0, 30.0, 0xFFFF_0000)
        .rect_shape(0, 50.0, 50.0, 30.0, 30.0, 0xFF00_FF00)
        .obj(ty::NODE, &[(key::PARENT_ID, V::U(0))])
        .rect_shape(9, 60.0, 50.0, 30.0, 30.0, 0xFF00_00FF)
        .obj(
            ty::DRAW_RULES,
            &[(key::PARENT_ID, V::U(9)), (key::DRAW_TARGET_ID, V::U(15))],
        )
        .obj(
            ty::DRAW_TARGET,
            &[
                (key::PARENT_ID, V::U(14)),
                (key::DRAWABLE_ID, V::U(1)),
                (key::PLACEMENT, V::U(placement)),
            ],
        )
        .obj(
            ty::DRAW_TARGET,
            &[(key::PARENT_ID, V::U(14)), (key::DRAWABLE_ID, V::U(5))],
        )
}

#[test]
fn a_rule_draws_its_drawables_in_front_of_its_target() {
    let s = scene(&squares(0));
    let c = color_at(&s, 0.0, 50, 50);
    assert!(near(c, BLUE, 2), "{c:?}");
}

#[test]
fn a_rule_draws_its_drawables_behind_its_target_after() {
    // Behind red, still in front of green.
    let s = scene(&squares(1));
    let c = color_at(&s, 0.0, 50, 50);
    assert!(near(c, RED, 2), "{c:?}");
    let c = color_at(&s, 0.0, 62, 50);
    assert!(near(c, BLUE, 2), "{c:?}");
}

#[test]
fn keyed_targets_reorder_over_time() {
    // In front of red, then from half a second in front of green only.
    let riv = squares(0)
        .obj(
            ty::LINEAR_ANIMATION,
            &[
                (key::ANIMATION_NAME, V::S("move")),
                (key::FPS, V::U(10)),
                (key::DURATION, V::U(10)),
            ],
        )
        .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(14))])
        .obj(
            ty::KEYED_PROPERTY,
            &[(key::PROPERTY_KEY, V::U(u64::from(key::DRAW_TARGET_ID)))],
        )
        .obj(
            ty::KEY_FRAME_ID,
            &[(key::FRAME, V::U(0)), (key::ID_VALUE, V::U(15))],
        )
        .obj(
            ty::KEY_FRAME_ID,
            &[(key::FRAME, V::U(5)), (key::ID_VALUE, V::U(16))],
        );
    let s = scene(&riv);
    for (t, x, want) in [
        (0.0, 50, BLUE),
        (0.0, 62, BLUE),
        (0.7, 50, RED),
        (0.7, 62, BLUE),
        (0.7, 38, RED),
    ] {
        let c = color_at(&s, t, x, 50);
        assert!(near(c, want, 2), "({x}, 50) at {t}: {c:?}");
    }
}
