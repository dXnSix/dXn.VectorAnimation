//! Nested artboards: drawn at their transform about their origin, playing
//! their nested animations on the parent's clock, in layers.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, key, near, scene, ty};
use dxn_vector_animation::LoopMode;

const RED: u32 = 0xFFFF_0000;
const WHITE: [u8; 3] = [255, 255, 255];

/// An animation of the nested artboard: its name, frames (at 10 a second),
/// loop, and the keyframes of its square's x (frame, value), straight.
type Animation = (&'static str, u64, u64, Vec<(u64, f32)>);

/// A 100 × 100 artboard (named "Main", playing a state machine "sm" of no
/// states) showing artboard 1 at (`x`, 50), with the nested animations
/// `nested` (type and properties, parent ids given), then artboard 1: 20 × 20, its origin at its center, a red 10 × 10
/// square (shape 1) at the origin, and its animations `animations`.
fn parent(
    x: f32,
    parent_animation: Option<Vec<(u64, f32)>>,
    nested: &[(u16, Vec<(u16, V)>)],
    animations: &[Animation],
) -> Riv {
    let mut riv = Riv::new().artboard(100.0, 100.0).obj(
        ty::NESTED_ARTBOARD,
        &[
            (key::PARENT_ID, V::U(0)),
            (key::X, V::D(x)),
            (key::Y, V::D(50.0)),
            (key::ARTBOARD_ID, V::U(1)),
        ],
    );
    for (t, props) in nested {
        let mut props = props.clone();
        props.insert(0, (key::PARENT_ID, V::U(1)));
        riv = riv.obj(*t, &props);
    }
    // A state machine the parent plays (nothing advances without one).
    riv = riv
        .obj(ty::STATE_MACHINE, &[(key::ANIMATION_NAME, V::S("sm"))])
        .obj(ty::STATE_MACHINE_LAYER, &[])
        .obj(ty::ENTRY_STATE, &[])
        .obj(ty::ANY_STATE, &[])
        .obj(ty::EXIT_STATE, &[]);
    if let Some(frames) = parent_animation {
        // The parent's animation keys whether the first nested one (object
        // 2) plays.
        riv = riv
            .obj(
                ty::LINEAR_ANIMATION,
                &[
                    (key::ANIMATION_NAME, V::S("parent")),
                    (key::FPS, V::U(10)),
                    (key::DURATION, V::U(40)),
                ],
            )
            .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(2))])
            .obj(
                ty::KEYED_PROPERTY,
                &[(key::PROPERTY_KEY, V::U(u64::from(key::IS_PLAYING)))],
            );
        for (frame, value) in frames {
            riv = riv.obj(84, &[(key::FRAME, V::U(frame)), (181, V::B(value != 0.0))]);
        }
    }
    let mut riv = riv
        .obj(
            ty::ARTBOARD,
            &[
                (key::NAME, V::S("Nested")),
                (key::WIDTH, V::D(20.0)),
                (key::HEIGHT, V::D(20.0)),
                (key::ORIGIN_X, V::D(0.5)),
                (key::ORIGIN_Y, V::D(0.5)),
            ],
        )
        .rect_shape(0, 0.0, 0.0, 10.0, 10.0, RED);
    for (name, frames, loop_value, keys) in animations {
        riv = riv
            .obj(
                ty::LINEAR_ANIMATION,
                &[
                    (key::ANIMATION_NAME, V::S(name)),
                    (key::FPS, V::U(10)),
                    (key::DURATION, V::U(*frames)),
                    (key::LOOP, V::U(*loop_value)),
                ],
            )
            .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(1))])
            .obj(
                ty::KEYED_PROPERTY,
                &[(key::PROPERTY_KEY, V::U(u64::from(key::X)))],
            );
        for (frame, value) in keys {
            riv = riv.obj(
                ty::KEY_FRAME_DOUBLE,
                &[
                    (key::FRAME, V::U(*frame)),
                    (key::VALUE, V::D(*value)),
                    (key::INTERPOLATION, V::U(1)),
                ],
            );
        }
    }
    riv
}

fn red(c: [u8; 3]) -> bool {
    near(c, [255, 0, 0], 2)
}

#[test]
fn a_nested_artboard_is_drawn_about_its_origin_and_clipped() {
    // Its origin at (30, 50): the square from 25 to 35; the artboard clips
    // what lies past 20 × 20 around it.
    let s = scene(&parent(30.0, None, &[], &[]).obj(ty::FILL, &[]));
    assert!(red(color_at(&s, 0.0, 30, 50)));
    assert_eq!(color_at(&s, 0.0, 37, 50), WHITE);
}

#[test]
fn a_simple_animation_plays_on_the_parents_clock() {
    // Its square moves right 10 a second (x 0 to 20 in 2 s), from the
    // parent's 0; the parent has no animation of its own.
    let s = scene(&parent(
        30.0,
        None,
        &[(
            ty::NESTED_SIMPLE_ANIMATION,
            vec![
                (key::NESTED_ANIMATION_ID, V::U(0)),
                (key::IS_PLAYING, V::B(true)),
            ],
        )],
        &[("move", 20, 0, vec![(0, 0.0), (20, 20.0)])],
    ));
    assert!(red(color_at(&s, 0.0, 30, 50)));
    // At 1 s, it is at 40 (clipped to 40 by the artboard's edge).
    assert!(red(color_at(&s, 1.0, 38, 50)));
    assert_eq!(color_at(&s, 1.0, 30, 50), WHITE);
}

#[test]
fn a_simple_animation_that_does_not_play_shows_its_start() {
    let s = scene(&parent(
        30.0,
        None,
        &[(
            ty::NESTED_SIMPLE_ANIMATION,
            vec![(key::NESTED_ANIMATION_ID, V::U(0))],
        )],
        &[("move", 20, 0, vec![(0, 5.0), (20, 20.0)])],
    ));
    // x 5 at its start: from 30 to 40 (clipped at 40).
    assert!(red(color_at(&s, 1.0, 37, 50)));
    assert_eq!(color_at(&s, 1.0, 27, 50), WHITE);
}

#[test]
fn the_parent_starts_and_stops_its_nested_animations() {
    // The parent plays it from 1 s to 2 s only: at 3 s it has played 1 s.
    let s = scene(&parent(
        30.0,
        Some(vec![(0, 0.0), (10, 1.0), (20, 0.0)]),
        &[(
            ty::NESTED_SIMPLE_ANIMATION,
            vec![
                (key::NESTED_ANIMATION_ID, V::U(0)),
                (key::NESTED_SPEED, V::D(0.5)),
            ],
        )],
        &[("move", 20, 0, vec![(0, 0.0), (20, 20.0)])],
    ));
    // Half speed for 1 s: x 5, the square from 30 to 40.
    assert!(red(color_at(&s, 0.5, 30, 50)));
    assert!(red(color_at(&s, 3.0, 38, 50)));
    assert_eq!(color_at(&s, 3.0, 28, 50), WHITE);
}

#[test]
fn looping_nested_animations_start_over() {
    let s = scene(&parent(
        30.0,
        None,
        &[(
            ty::NESTED_SIMPLE_ANIMATION,
            vec![
                (key::NESTED_ANIMATION_ID, V::U(0)),
                (key::IS_PLAYING, V::B(true)),
            ],
        )],
        &[("loop", 10, 1, vec![(0, -10.0), (10, 10.0)])],
    ));
    // Without animations of its own, the parent's timeline is the nested
    // one's, looping: at 1.5 s it is half way round its second run, x 0.
    assert_eq!(
        (s.timeline.duration, s.timeline.loop_mode),
        (1.0, LoopMode::Loop)
    );
    let t = s.timeline.time_at(1.5);
    assert!(red(color_at(&s, t, 30, 50)));
    assert_eq!(color_at(&s, t, 22, 50), WHITE);
}

#[test]
fn later_nested_animations_override_earlier_ones() {
    // Two animations key x: the later (held at 8) wins over the earlier.
    let s = scene(&parent(
        30.0,
        None,
        &[
            (
                ty::NESTED_SIMPLE_ANIMATION,
                vec![
                    (key::NESTED_ANIMATION_ID, V::U(0)),
                    (key::IS_PLAYING, V::B(true)),
                ],
            ),
            (
                ty::NESTED_SIMPLE_ANIMATION,
                vec![
                    (key::NESTED_ANIMATION_ID, V::U(1)),
                    (key::IS_PLAYING, V::B(true)),
                ],
            ),
        ],
        &[
            ("first", 10, 0, vec![(0, -8.0), (10, -8.0)]),
            ("second", 10, 0, vec![(0, 8.0), (10, 8.0)]),
        ],
    ));
    assert!(red(color_at(&s, 0.5, 37, 50)));
    assert_eq!(color_at(&s, 0.5, 23, 50), WHITE);
}

#[test]
fn a_remap_animation_shows_the_time_keyed() {
    // The remap's time (a fraction of the animation) is 0.5: x 10.
    let s = scene(&parent(
        30.0,
        None,
        &[(
            ty::NESTED_REMAP_ANIMATION,
            vec![
                (key::NESTED_ANIMATION_ID, V::U(0)),
                (key::REMAP_TIME, V::D(0.5)),
            ],
        )],
        &[("move", 20, 0, vec![(0, -10.0), (20, 10.0)])],
    ));
    assert!(red(color_at(&s, 0.0, 30, 50)));
    assert_eq!(color_at(&s, 0.0, 22, 50), WHITE);
}
