//! Linear animations on the timeline: one marker each, the entry's first,
//! and their keyframes with Rive's interpolation.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, key, near, scene, ty};
use dxn_vector_animation::{Animated, Easing, Group, LoopMode, Node, Scene, TransformOp};

const RED: u32 = 0xFFFF_0000;

/// A 10 × 10 red square in shape 1 at (10, 25) of a 100 × 50 artboard.
fn square() -> Riv {
    Riv::new()
        .artboard(100.0, 50.0)
        .rect_shape(0, 10.0, 25.0, 10.0, 10.0, RED)
}

/// An animation of `frames` at 10 fps named `name`, looping as `loop_value`
/// says.
fn animation(riv: Riv, name: &'static str, frames: u64, loop_value: u64) -> Riv {
    riv.obj(
        ty::LINEAR_ANIMATION,
        &[
            (key::ANIMATION_NAME, V::S(name)),
            (key::FPS, V::U(10)),
            (key::DURATION, V::U(frames)),
            (key::LOOP, V::U(loop_value)),
        ],
    )
}

/// Keyframes of property `prop` of object `object`: (frame, value,
/// interpolation type, interpolator).
fn keys(riv: Riv, object: u64, prop: u16, frames: &[(u64, f32, u64, Option<u64>)]) -> Riv {
    let mut riv = riv
        .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(object))])
        .obj(
            ty::KEYED_PROPERTY,
            &[(key::PROPERTY_KEY, V::U(u64::from(prop)))],
        );
    for (frame, value, interpolation, interpolator) in frames {
        let mut props = vec![
            (key::FRAME, V::U(*frame)),
            (key::VALUE, V::D(*value)),
            (key::INTERPOLATION, V::U(*interpolation)),
        ];
        if let Some(i) = interpolator {
            props.push((key::INTERPOLATOR_ID, V::U(*i)));
        }
        riv = riv.obj(ty::KEY_FRAME_DOUBLE, &props);
    }
    riv
}

/// The translation of drawable `id`.
fn translate_x(scene: &Scene, id: &str) -> Animated<f64> {
    fn find<'a>(g: &'a Group, id: &str) -> Option<&'a Group> {
        if g.id.as_deref() == Some(id) {
            return Some(g);
        }
        g.children.iter().find_map(|c| match c {
            Node::Group(g) => find(g, id),
            _ => None,
        })
    }
    let g = find(&scene.root, id).expect("the drawable");
    match &g.transform[0] {
        TransformOp::Translate { x, .. } => x.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn each_animation_is_a_marker_with_its_loop() {
    let riv = animation(square(), "idle", 20, 1);
    let riv = animation(riv, "tap", 5, 0);
    let s = scene(&riv);
    let m = &s.timeline.markers;
    assert_eq!(m.len(), 2);
    assert_eq!(
        (m[0].name.as_str(), m[0].start, m[0].duration),
        ("idle", 0.0, 2.0)
    );
    assert_eq!(m[0].loop_mode, Some(LoopMode::Loop));
    assert_eq!((m[1].name.as_str(), m[1].duration), ("tap", 0.5));
    assert_eq!(m[1].loop_mode, Some(LoopMode::Once));
    assert!(m[1].start > 2.0, "after the first, a gap between them");
    // The timeline is the first animation's.
    assert_eq!(s.timeline.duration, 2.0);
    assert_eq!(s.timeline.loop_mode, LoopMode::Loop);
}

#[test]
fn the_default_state_machine_plays_first() {
    let riv = animation(square(), "idle", 20, 1);
    let riv = animation(riv, "intro", 10, 0)
        .obj(
            ty::STATE_MACHINE,
            &[(key::ANIMATION_NAME, V::S("State Machine 1"))],
        )
        .obj(ty::STATE_MACHINE_LAYER, &[])
        .obj(ty::ENTRY_STATE, &[])
        .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(3))])
        .obj(ty::ANY_STATE, &[])
        .obj(ty::EXIT_STATE, &[])
        .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(1))]);
    let s = scene(&riv);
    // The machine plays "intro" once, then holds: its span, then the
    // animations one after another.
    let m = &s.timeline.markers;
    assert_eq!(
        (m[0].name.as_str(), m[0].start, m[0].loop_mode),
        ("State Machine 1", 0.0, Some(LoopMode::Once))
    );
    assert!(
        (s.timeline.duration - 1.0).abs() < 0.02,
        "{}",
        s.timeline.duration
    );
    assert_eq!((m[1].name.as_str(), m[2].name.as_str()), ("idle", "intro"));
    assert!(m[1].start > s.timeline.duration);
    // The square moves only in "idle": still while the machine plays.
    assert!(near(color_at(&s, 0.5, 10, 25), [255, 0, 0], 2));
}

#[test]
fn keyframes_keep_their_cubic_easing() {
    // A cubic ease (object 5) moving the square from x 10 to 90 in a second.
    let riv = square().obj(
        ty::CUBIC_EASE_INTERPOLATOR,
        &[
            (key::X1, V::D(0.42)),
            (key::Y1, V::D(0.0)),
            (key::X2, V::D(0.58)),
            (key::Y2, V::D(1.0)),
        ],
    );
    let riv = animation(riv, "move", 10, 0);
    let riv = keys(
        riv,
        1,
        key::X,
        &[(0, 10.0, 2, Some(5)), (10, 90.0, 1, None)],
    );
    let s = scene(&riv);
    let x = translate_x(&s, "1");
    let k = x.keyframes();
    assert_eq!(k.len(), 2, "{k:?}");
    assert_eq!(k[0].easing, Easing::EASE_IN_OUT);
    assert_eq!(
        (k[0].time, k[0].value, k[1].time, k[1].value),
        (0.0, 10.0, 1.0, 90.0)
    );
    // Half way, the square is half way (an ease in and out is symmetric).
    assert!(near(color_at(&s, 0.5, 50, 25), [255, 0, 0], 2));
}

#[test]
fn what_an_animation_does_not_key_holds_its_setup_value() {
    let riv = animation(square(), "still", 10, 0);
    let riv = animation(riv, "move", 10, 0);
    let riv = keys(riv, 1, key::X, &[(0, 50.0, 1, None), (10, 90.0, 1, None)]);
    let s = scene(&riv);
    let x = translate_x(&s, "1");
    let tap = &s.timeline.markers[1];
    assert_eq!(x.sample(0.5), 10.0);
    assert_eq!(x.sample(tap.start), 50.0);
    assert_eq!(x.sample(tap.start + 0.5), 70.0);
    // Held to the end of its span and past it.
    assert_eq!(x.sample(tap.start + tap.duration + 0.5), 90.0);
}

#[test]
fn value_curves_become_easings_of_the_change() {
    let riv = square().obj(
        ty::CUBIC_VALUE_INTERPOLATOR,
        &[
            (key::X1, V::D(0.5)),
            (key::Y1, V::D(10.0)),
            (key::X2, V::D(0.5)),
            (key::Y2, V::D(90.0)),
        ],
    );
    let riv = animation(riv, "move", 10, 0);
    let riv = keys(
        riv,
        1,
        key::X,
        &[(0, 10.0, 2, Some(5)), (10, 90.0, 1, None)],
    );
    let x = translate_x(&scene(&riv), "1");
    let k = x.keyframes();
    assert_eq!(k.len(), 2);
    assert!(matches!(k[0].easing, Easing::CubicBezier { y1, y2, .. } if y1 == 0.0 && y2 == 1.0));
}

#[test]
fn elastic_easing_is_sampled() {
    let riv = square().obj(ty::ELASTIC_INTERPOLATOR, &[(key::ELASTIC_EASING, V::U(1))]);
    let riv = animation(riv, "bounce", 10, 0);
    let riv = keys(
        riv,
        1,
        key::X,
        &[(0, 10.0, 2, Some(5)), (10, 90.0, 1, None)],
    );
    let x = translate_x(&scene(&riv), "1");
    assert!(x.keyframes().len() > 30);
    // An elastic ease out overshoots its end.
    let top = (0..100)
        .map(|i| x.sample(f64::from(i) / 100.0))
        .fold(0.0, f64::max);
    assert!(top > 91.0, "{top}");
    assert_eq!(x.sample(1.0), 90.0);
}

#[test]
fn holds_hold() {
    let riv = animation(square(), "jump", 10, 0);
    let riv = keys(riv, 1, key::X, &[(0, 10.0, 0, None), (5, 90.0, 0, None)]);
    let x = translate_x(&scene(&riv), "1");
    assert_eq!(x.sample(0.49), 10.0);
    assert_eq!(x.sample(0.5), 90.0);
}

#[test]
fn work_areas_and_speeds_set_what_plays_and_how_fast() {
    // Frames 5 to 10 of 20, at twice the speed: a quarter of a second.
    let riv = square().obj(
        ty::LINEAR_ANIMATION,
        &[
            (key::ANIMATION_NAME, V::S("part")),
            (key::FPS, V::U(10)),
            (key::DURATION, V::U(20)),
            (key::SPEED, V::D(2.0)),
            (key::WORK_START, V::U(5)),
            (key::WORK_END, V::U(10)),
            (key::ENABLE_WORK_AREA, V::B(true)),
        ],
    );
    let riv = keys(riv, 1, key::X, &[(0, 0.0, 1, None), (20, 100.0, 1, None)]);
    let s = scene(&riv);
    assert_eq!(s.timeline.duration, 0.25);
    let x = translate_x(&s, "1");
    assert!((x.sample(0.0) - 25.0).abs() < 1e-6);
    assert!((x.sample(0.25) - 50.0).abs() < 1e-6);
}

#[test]
fn animations_played_backwards_run_from_their_end() {
    let riv = square().obj(
        ty::LINEAR_ANIMATION,
        &[
            (key::ANIMATION_NAME, V::S("back")),
            (key::FPS, V::U(10)),
            (key::DURATION, V::U(10)),
            (key::SPEED, V::D(-1.0)),
        ],
    );
    let riv = keys(riv, 1, key::X, &[(0, 10.0, 1, None), (10, 90.0, 1, None)]);
    let x = translate_x(&scene(&riv), "1");
    assert!((x.sample(0.0) - 90.0).abs() < 1e-6);
    assert!((x.sample(0.75) - 30.0).abs() < 1e-6);
}

#[test]
fn colors_mix_channel_by_channel_as_rive_mixes_them() {
    // The fill's color (object 4) from transparent black to opaque red.
    let riv = animation(square(), "fade", 10, 0)
        .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(4))])
        .obj(
            ty::KEYED_PROPERTY,
            &[(key::PROPERTY_KEY, V::U(u64::from(key::COLOR)))],
        )
        .obj(
            ty::KEY_FRAME_COLOR,
            &[
                (key::FRAME, V::U(0)),
                (key::COLOR_VALUE, V::C(0x0000_0000)),
                (key::INTERPOLATION, V::U(1)),
            ],
        )
        .obj(
            ty::KEY_FRAME_COLOR,
            &[
                (key::FRAME, V::U(10)),
                (key::COLOR_VALUE, V::C(RED)),
                (key::INTERPOLATION, V::U(1)),
            ],
        );
    let s = scene(&riv);
    // Half way: (127, 0, 0) at half opacity over white, as Rive mixes it
    // (straight), not red at half opacity (premultiplied).
    let c = color_at(&s, 0.5, 10, 25);
    assert!(near(c, [191, 128, 128], 3), "{c:?}");
}
