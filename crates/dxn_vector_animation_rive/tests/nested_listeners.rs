//! Listeners in nested artboards and drags: a pointer reaches the
//! listeners of the state machines nested artboards play, the events they
//! fire reach their parent's listeners, and a pointer pressed on a shape
//! drags it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, key, scene, ty};
use dxn_vector_animation::{Limits, Point, Scene};
use dxn_vector_animation_rive::{Interaction, Options, Pointer};

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

/// An animation `name` keying object `object`'s x from `from` to `to` over
/// `frames` frames at 10 a second, straight (held when 0 frames).
fn slide(riv: Riv, name: &'static str, object: u64, from: f32, to: f32, frames: u64) -> Riv {
    let mut riv = riv
        .obj(
            ty::LINEAR_ANIMATION,
            &[
                (key::ANIMATION_NAME, V::S(name)),
                (key::FPS, V::U(10)),
                (key::DURATION, V::U(frames.max(1))),
            ],
        )
        .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(object))])
        .obj(
            ty::KEYED_PROPERTY,
            &[(key::PROPERTY_KEY, V::U(u64::from(key::X)))],
        );
    for (frame, x) in [(0, from), (frames, to)] {
        riv = riv.obj(
            ty::KEY_FRAME_DOUBLE,
            &[
                (key::FRAME, V::U(frame)),
                (key::VALUE, V::D(x)),
                (key::INTERPOLATION, V::U(1)),
            ],
        );
    }
    riv
}

/// A layer entering animation state 3 (animation 0), going to state 4
/// (animation 1) on `condition` from any state.
fn layer(riv: Riv, condition: (u16, Vec<(u16, V)>)) -> Riv {
    riv.obj(ty::STATE_MACHINE_LAYER, &[])
        .obj(ty::ENTRY_STATE, &[])
        .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(3))])
        .obj(ty::ANY_STATE, &[])
        .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(4))])
        .obj(condition.0, &condition.1)
        .obj(ty::EXIT_STATE, &[])
        .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(0))])
        .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(1))])
}

/// A 100 × 50 artboard nesting artboard "Button" (object 1, its state
/// machine object 2) at (30, 25), with a blue square (shape 3) at (80, 25)
/// that its trigger "go" sends to 20 in a second; listening for the
/// button's event "poke". The button: 20 × 20 about its center, a red
/// square (shape 1) at its center; pressed, its bool "on" sends the square
/// 10 right and it fires "poke" (object 5).
fn button() -> Riv {
    let riv = Riv::new()
        .artboard(100.0, 50.0)
        .obj(
            ty::NESTED_ARTBOARD,
            &[
                (key::PARENT_ID, V::U(0)),
                (key::X, V::D(30.0)),
                (key::Y, V::D(25.0)),
                (key::ARTBOARD_ID, V::U(1)),
            ],
        )
        .obj(
            ty::NESTED_STATE_MACHINE,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::NESTED_ANIMATION_ID, V::U(0)),
            ],
        )
        .rect_shape(0, 80.0, 25.0, 10.0, 10.0, 0xFF00_00FF);
    let riv = slide(riv, "stay", 3, 80.0, 80.0, 0);
    let riv = slide(riv, "move", 3, 80.0, 20.0, 10);
    let riv = riv
        .obj(
            ty::STATE_MACHINE,
            &[(key::ANIMATION_NAME, V::S("State Machine 1"))],
        )
        .obj(ty::STATE_MACHINE_TRIGGER, &[(key::INPUT_NAME, V::S("go"))]);
    let riv = layer(
        riv,
        (
            ty::TRANSITION_TRIGGER_CONDITION,
            vec![(key::CONDITION_INPUT, V::U(0))],
        ),
    )
    .obj(
        ty::LISTENER_SINGLE,
        &[
            (key::LISTENER_TARGET, V::U(1)),
            (key::LISTENER_TYPE, V::U(5)),
            (key::LISTENER_EVENT, V::U(5)),
        ],
    )
    .obj(ty::LISTENER_TRIGGER_CHANGE, &[(key::ACTION_INPUT, V::U(0))]);
    // The button.
    let riv = riv
        .obj(
            ty::ARTBOARD,
            &[
                (key::NAME, V::S("Button")),
                (key::WIDTH, V::D(20.0)),
                (key::HEIGHT, V::D(20.0)),
                (key::ORIGIN_X, V::D(0.5)),
                (key::ORIGIN_Y, V::D(0.5)),
            ],
        )
        .rect_shape(0, 0.0, 0.0, 10.0, 10.0, 0xFFFF_0000)
        .obj(
            ty::EVENT,
            &[(key::PARENT_ID, V::U(0)), (key::NAME, V::S("poke"))],
        );
    let riv = slide(riv, "rest", 1, 0.0, 0.0, 0);
    let riv = slide(riv, "away", 1, 10.0, 10.0, 0);
    let riv = riv
        .obj(
            ty::STATE_MACHINE,
            &[(key::ANIMATION_NAME, V::S("Button machine"))],
        )
        .obj(
            ty::STATE_MACHINE_BOOL,
            &[
                (key::INPUT_NAME, V::S("on")),
                (key::BOOL_VALUE, V::B(false)),
            ],
        );
    layer(
        riv,
        (
            ty::TRANSITION_BOOL_CONDITION,
            vec![
                (key::CONDITION_INPUT, V::U(0)),
                (key::CONDITION_OP, V::U(0)),
            ],
        ),
    )
    .obj(
        ty::LISTENER_SINGLE,
        &[
            (key::LISTENER_TARGET, V::U(1)),
            (key::LISTENER_TYPE, V::U(2)),
        ],
    )
    .obj(
        ty::LISTENER_BOOL_CHANGE,
        &[
            (key::ACTION_INPUT, V::U(0)),
            (key::ACTION_BOOL_VALUE, V::U(1)),
        ],
    )
    .obj(
        ty::LISTENER_FIRE_EVENT,
        &[(key::FIRE_LISTENER_EVENT, V::U(5))],
    )
}

fn at(s: &Scene, t: f64, x: u32, y: u32) -> [u8; 3] {
    color_at(s, t, x, y)
}

#[test]
fn a_pointer_reaches_a_nested_artboards_listeners() {
    let riv = button();
    let s = scene(&riv);
    assert_eq!(at(&s, 0.05, 30, 25), RED);
    let mut play = Interaction::new(riv.bytes(), Limits::default(), Options::default()).unwrap();
    let m = play.machine().unwrap();
    assert_eq!(m.nested.len(), 1);
    assert_eq!(m.nested[0].path, "#1");
    // Pressed beside the button, nothing; on it, its square goes right.
    assert!(
        !play
            .pointer(0.2, Pointer::Down, Point::new(50.0, 10.0))
            .unwrap()
    );
    assert!(
        play.pointer(0.3, Pointer::Down, Point::new(30.0, 25.0))
            .unwrap()
    );
    let s = &play.import().scene;
    assert_eq!(at(s, 0.2, 30, 25), RED);
    assert_eq!(at(s, 0.5, 30, 25), WHITE);
    // Shown to 40, where the nested artboard clips it.
    assert_eq!(at(s, 0.5, 37, 25), RED);
}

#[test]
fn a_nested_listeners_event_reaches_its_parents_listeners() {
    let riv = button();
    let _ = scene(&riv);
    let mut play = Interaction::new(riv.bytes(), Limits::default(), Options::default()).unwrap();
    play.pointer(0.3, Pointer::Down, Point::new(30.0, 25.0))
        .unwrap();
    let changes: Vec<(String, String)> = play
        .changes()
        .iter()
        .map(|c| (c.path.clone(), c.input.clone()))
        .collect();
    assert_eq!(
        changes,
        [("#1".into(), "on".into()), (String::new(), "go".into())]
    );
    // The nested artboard's event is its parent's alone.
    assert!(play.take_fired().is_empty());
    let s = &play.import().scene;
    assert_eq!(at(s, 0.25, 80, 25), BLUE);
    assert_eq!(at(s, 1.4, 20, 25), BLUE);
}

/// A 100 × 50 artboard with a red square (shape 1) at (10, 25) that a drag
/// moves along with the pointer, and a state machine holding it.
fn draggable() -> Riv {
    let riv = Riv::new()
        .artboard(100.0, 50.0)
        .rect_shape(0, 10.0, 25.0, 10.0, 10.0, 0xFFFF_0000);
    let riv = slide(riv, "a", 0, 0.0, 0.0, 0);
    let riv = slide(riv, "b", 0, 0.0, 0.0, 0)
        .obj(
            ty::STATE_MACHINE,
            &[(key::ANIMATION_NAME, V::S("State Machine 1"))],
        )
        .obj(ty::STATE_MACHINE_TRIGGER, &[(key::INPUT_NAME, V::S("go"))]);
    layer(
        riv,
        (
            ty::TRANSITION_TRIGGER_CONDITION,
            vec![(key::CONDITION_INPUT, V::U(0))],
        ),
    )
    .obj(
        ty::LISTENER_SINGLE,
        &[
            (key::LISTENER_TARGET, V::U(1)),
            (key::LISTENER_TYPE, V::U(12)),
        ],
    )
    .obj(
        ty::LISTENER_ALIGN_TARGET,
        &[
            (key::ALIGN_TARGET, V::U(1)),
            (key::ALIGN_PRESERVE_OFFSET, V::B(true)),
        ],
    )
}

#[test]
fn a_pointer_pressed_on_a_shape_drags_it() {
    let riv = draggable();
    let _ = scene(&riv);
    let mut play = Interaction::new(riv.bytes(), Limits::default(), Options::default()).unwrap();
    // Moving without pressing drags nothing; pressed, the first move starts
    // the drag (as Rive's web runtime has it), and each move after drags
    // it as far, though the pointer runs ahead of it.
    play.pointer(0.1, Pointer::Move, Point::new(30.0, 25.0))
        .unwrap();
    play.pointer(0.2, Pointer::Down, Point::new(10.0, 25.0))
        .unwrap();
    play.pointer(0.3, Pointer::Move, Point::new(30.0, 25.0))
        .unwrap();
    play.pointer(0.4, Pointer::Move, Point::new(60.0, 30.0))
        .unwrap();
    play.pointer(0.5, Pointer::Up, Point::new(60.0, 30.0))
        .unwrap();
    play.pointer(0.6, Pointer::Move, Point::new(90.0, 30.0))
        .unwrap();
    let s = &play.import().scene;
    assert_eq!(at(s, 0.25, 10, 25), RED);
    assert_eq!(at(s, 0.35, 10, 25), RED);
    assert_eq!(at(s, 0.45, 40, 30), RED);
    assert_eq!(at(s, 0.7, 40, 30), RED);
    assert_eq!(at(s, 0.7, 70, 30), WHITE);
}
