//! State machines played as Rive's runtime plays them with their inputs as
//! they are: layers together, exit times, mixes and conditions.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, key, near, scene, ty};
use dxn_vector_animation::Point;
use dxn_vector_animation::{Limits, LoopMode, Scene};
use dxn_vector_animation_rive::{
    Action, Change, InputChange, InputValue, Interaction, Options, Pointer, PointerEvent,
    read_options,
};

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

/// A 100 × 50 artboard with a red square (shape 1) at (10, 15) and a blue
/// one (shape 5) at (10, 35), and three animations: "moveA" (0) moving the
/// red one from x 10 to 90 in a second, looping; "moveB" (1) the blue one
/// in two, looping; "backA" (2) the red one from 90 to 10 in a second, once.
fn squares() -> Riv {
    squares_after(Riv::new().artboard(100.0, 50.0))
}

/// The squares and their animations in the artboard `riv` has just opened.
fn squares_after(riv: Riv) -> Riv {
    let mut riv = riv
        .rect_shape(0, 10.0, 15.0, 10.0, 10.0, 0xFFFF_0000)
        .rect_shape(0, 10.0, 35.0, 10.0, 10.0, 0xFF00_00FF);
    for (name, object, from, to, frames, loop_value) in [
        ("moveA", 1, 10.0, 90.0, 10, 1),
        ("moveB", 5, 10.0, 90.0, 20, 1),
        ("backA", 1, 90.0, 10.0, 10, 0),
    ] {
        riv = riv
            .obj(
                ty::LINEAR_ANIMATION,
                &[
                    (key::ANIMATION_NAME, V::S(name)),
                    (key::FPS, V::U(10)),
                    (key::DURATION, V::U(frames)),
                    (key::LOOP, V::U(loop_value)),
                ],
            )
            .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(object))])
            .obj(
                ty::KEYED_PROPERTY,
                &[(key::PROPERTY_KEY, V::U(u64::from(key::X)))],
            );
        for (frame, value) in [(0, from), (frames, to)] {
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

/// A transition to state `to` (states count from the layer's first: entry
/// 0, any 1, exit 2, then the animation states), with `props` and
/// `conditions` (each a type and its properties).
type Transition<'a> = (u64, &'a [(u16, V)], &'a [(u16, &'a [(u16, V)])]);

fn transition(mut riv: Riv, (to, props, conditions): &Transition<'_>) -> Riv {
    let mut all = vec![(key::STATE_TO_ID, V::U(*to))];
    all.extend(props.iter().cloned());
    riv = riv.obj(ty::STATE_TRANSITION, &all);
    for (ty, props) in *conditions {
        riv = riv.obj(*ty, props);
    }
    riv
}

/// A layer: its entry's transitions, its any state's, then a state for
/// each of `states` (an animation and its transitions).
fn layer(
    mut riv: Riv,
    entry: &[Transition<'_>],
    any: &[Transition<'_>],
    states: &[(u64, &[Transition<'_>])],
) -> Riv {
    riv = riv
        .obj(ty::STATE_MACHINE_LAYER, &[])
        .obj(ty::ENTRY_STATE, &[]);
    for t in entry {
        riv = transition(riv, t);
    }
    riv = riv.obj(ty::ANY_STATE, &[]);
    for t in any {
        riv = transition(riv, t);
    }
    riv = riv.obj(ty::EXIT_STATE, &[]);
    for (animation, transitions) in states {
        riv = riv.obj(
            ty::ANIMATION_STATE,
            &[(key::ANIMATION_ID, V::U(*animation))],
        );
        for t in *transitions {
            riv = transition(riv, t);
        }
    }
    riv
}

fn machine(riv: Riv) -> Riv {
    riv.obj(
        ty::STATE_MACHINE,
        &[(key::ANIMATION_NAME, V::S("State Machine 1"))],
    )
}

fn at(s: &Scene, t: f64, x: u32, y: u32) -> [u8; 3] {
    color_at(s, t, x, y)
}

#[test]
fn layers_play_together() {
    // Layer one plays "moveA", layer two "moveB": half a second in, the red
    // square is at 50, the blue one at 30.
    let riv = layer(machine(squares()), &[(3, &[], &[])], &[], &[(0, &[])]);
    let riv = layer(riv, &[(3, &[], &[])], &[], &[(1, &[])]);
    let s = scene(&riv);
    let m = &s.timeline.markers[0];
    assert_eq!(
        (m.name.as_str(), m.loop_mode),
        ("State Machine 1", Some(LoopMode::Loop))
    );
    // Both loops repeat every 2 s.
    assert!(
        (s.timeline.duration - 2.0).abs() < 0.02,
        "{}",
        s.timeline.duration
    );
    assert!(near(at(&s, 0.5, 50, 15), RED, 2));
    assert!(near(at(&s, 0.5, 30, 35), BLUE, 2));
    assert_eq!(at(&s, 0.5, 10, 35), WHITE);
}

#[test]
fn exit_times_hand_over_with_a_mix() {
    // "moveA", then at its end "backA", mixing over half a second.
    const EXIT_TIME_PERCENT: u64 = (1 << 2) | (1 << 3);
    let riv = layer(
        machine(squares()),
        &[(3, &[], &[])],
        &[],
        &[
            (
                0,
                &[(
                    4,
                    &[
                        (key::TRANSITION_FLAGS, V::U(EXIT_TIME_PERCENT)),
                        (key::EXIT_TIME, V::U(100)),
                        (key::TRANSITION_DURATION, V::U(500)),
                    ],
                    &[],
                )],
            ),
            (2, &[]),
        ],
    );
    let s = scene(&riv);
    // A quarter into the mix: "moveA" at 30 (looped), "backA" at 70, half
    // way between them.
    assert!(near(at(&s, 1.25, 50, 15), RED, 2));
    // "backA" played out: back at 10, held.
    assert!(near(at(&s, 2.5, 10, 15), RED, 2));
    assert_eq!(s.timeline.loop_mode, LoopMode::Once);
}

#[test]
fn conditions_hold_for_the_inputs_values() {
    // From any state to "backA" when "go" is on.
    let with = |go: bool| {
        let riv = machine(squares()).obj(
            ty::STATE_MACHINE_BOOL,
            &[(key::INPUT_NAME, V::S("go")), (key::BOOL_VALUE, V::B(go))],
        );
        layer(
            riv,
            &[(3, &[], &[])],
            &[(
                4,
                &[],
                &[(
                    ty::TRANSITION_BOOL_CONDITION,
                    &[
                        (key::CONDITION_INPUT, V::U(0)),
                        (key::CONDITION_OP, V::U(0)),
                    ],
                )],
            )],
            &[(0, &[]), (2, &[])],
        )
    };
    let on = scene(&with(true));
    assert!(near(at(&on, 0.25, 70, 15), RED, 2));
    let off = scene(&with(false));
    assert!(near(at(&off, 0.25, 30, 15), RED, 2));
}

#[test]
fn nested_state_machines_follow_the_inputs_their_parent_keys() {
    // The parent shows the squares' artboard (its state machine going from
    // "moveA" to "backA" when "go" is on) and turns "go" on at 0.5 s.
    let parent = Riv::new()
        .artboard(100.0, 50.0)
        .obj(
            ty::NESTED_ARTBOARD,
            &[(key::PARENT_ID, V::U(0)), (key::ARTBOARD_ID, V::U(1))],
        )
        .obj(
            ty::NESTED_STATE_MACHINE,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::NESTED_ANIMATION_ID, V::U(0)),
            ],
        )
        .obj(
            ty::NESTED_BOOL,
            &[(key::PARENT_ID, V::U(2)), (key::NESTED_INPUT, V::U(0))],
        )
        .obj(
            ty::LINEAR_ANIMATION,
            &[
                (key::ANIMATION_NAME, V::S("go")),
                (key::FPS, V::U(10)),
                (key::DURATION, V::U(20)),
            ],
        )
        .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(3))])
        .obj(
            ty::KEYED_PROPERTY,
            &[(key::PROPERTY_KEY, V::U(u64::from(key::NESTED_BOOL_VALUE)))],
        )
        .obj(
            ty::KEY_FRAME_BOOL,
            &[(key::FRAME, V::U(0)), (key::BOOL_KEY_VALUE, V::B(false))],
        )
        .obj(
            ty::KEY_FRAME_BOOL,
            &[(key::FRAME, V::U(5)), (key::BOOL_KEY_VALUE, V::B(true))],
        );
    let parent = layer(machine(parent), &[(3, &[], &[])], &[], &[(0, &[])]);
    let child = machine(squares_after(parent.obj(
        ty::ARTBOARD,
        &[
            (key::NAME, V::S("Squares")),
            (key::WIDTH, V::D(100.0)),
            (key::HEIGHT, V::D(50.0)),
            (key::ORIGIN_X, V::D(0.0)),
            (key::ORIGIN_Y, V::D(0.0)),
        ],
    )))
    .obj(
        ty::STATE_MACHINE_BOOL,
        &[
            (key::INPUT_NAME, V::S("go")),
            (key::BOOL_VALUE, V::B(false)),
        ],
    );
    let riv = layer(
        child,
        &[(3, &[], &[])],
        &[(
            4,
            &[],
            &[(
                ty::TRANSITION_BOOL_CONDITION,
                &[
                    (key::CONDITION_INPUT, V::U(0)),
                    (key::CONDITION_OP, V::U(0)),
                ],
            )],
        )],
        &[(0, &[]), (2, &[])],
    );
    let s = scene(&riv);
    // "moveA" until 0.5 s (the red square at 30 a quarter second in), then
    // "backA" from 90.
    assert!(near(at(&s, 0.25, 30, 15), RED, 2));
    assert!(near(at(&s, 0.75, 70, 15), RED, 2));
}

/// `riv` read with `inputs` changed (name, value, seconds).
fn with_inputs(riv: &Riv, inputs: &[(&str, InputValue, f64)]) -> Scene {
    let options = Options {
        inputs: inputs
            .iter()
            .map(|(name, value, time)| InputChange {
                time: *time,
                input: (*name).to_owned(),
                value: *value,
            })
            .collect(),
        ..Options::default()
    };
    // Written for the comparison with Rive's runtime too.
    let _ = scene(riv);
    read_options(&riv.bytes(), &Limits::default(), &options)
        .unwrap()
        .scene
}

#[test]
fn inputs_changed_while_playing_take_their_transitions() {
    let riv = machine(squares()).obj(
        ty::STATE_MACHINE_BOOL,
        &[
            (key::INPUT_NAME, V::S("go")),
            (key::BOOL_VALUE, V::B(false)),
        ],
    );
    let riv = layer(
        riv,
        &[(3, &[], &[])],
        &[(
            4,
            &[],
            &[(
                ty::TRANSITION_BOOL_CONDITION,
                &[
                    (key::CONDITION_INPUT, V::U(0)),
                    (key::CONDITION_OP, V::U(0)),
                ],
            )],
        )],
        &[(0, &[]), (2, &[])],
    );
    // "go" on at 0.5 s: "moveA" (the red square at 30 a quarter second in),
    // then "backA" from 90.
    let s = with_inputs(&riv, &[("go", InputValue::Bool(true), 0.5)]);
    assert!(near(at(&s, 0.25, 30, 15), RED, 2));
    assert!(near(at(&s, 0.75, 70, 15), RED, 2));
}

#[test]
fn triggers_fire_once() {
    // "moveA"; "jump" mixes into "backA" over 200 ms; at its end, back to
    // "moveA".
    const EXIT_TIME_PERCENT: u64 = (1 << 2) | (1 << 3);
    let riv = machine(squares()).obj(
        ty::STATE_MACHINE_TRIGGER,
        &[(key::INPUT_NAME, V::S("jump"))],
    );
    let riv = layer(
        riv,
        &[(3, &[], &[])],
        &[],
        &[
            (
                0,
                &[(
                    4,
                    &[(key::TRANSITION_DURATION, V::U(200))],
                    &[(
                        ty::TRANSITION_TRIGGER_CONDITION,
                        &[(key::CONDITION_INPUT, V::U(0))],
                    )],
                )],
            ),
            (
                2,
                &[(
                    3,
                    &[
                        (key::TRANSITION_FLAGS, V::U(EXIT_TIME_PERCENT)),
                        (key::EXIT_TIME, V::U(100)),
                    ],
                    &[],
                )],
            ),
        ],
    );
    let s = with_inputs(
        &riv,
        &[
            ("jump", InputValue::Fire, 0.3),
            ("jump", InputValue::Fire, 2.0),
        ],
    );
    // Before: "moveA" at 26 at 0.2 s. After the mix: "backA" at 50 at
    // 0.8 s. Back in "moveA" from 1.3 s, a second in it at 1.5 s.
    assert!(near(at(&s, 0.2, 26, 15), RED, 2));
    assert!(near(at(&s, 0.8, 50, 15), RED, 2));
    assert!(near(at(&s, 1.5, 26, 15), RED, 2));
    // The machine settles after the last trigger: the timeline repeats from
    // there.
    assert!(s.timeline.loop_start >= 2.0, "{}", s.timeline.loop_start);
}

#[test]
fn listeners_turn_pointer_events_into_input_changes() {
    // Pressing the red square sets "go": from any state to "backA".
    let riv = machine(squares()).obj(
        ty::STATE_MACHINE_BOOL,
        &[
            (key::INPUT_NAME, V::S("go")),
            (key::BOOL_VALUE, V::B(false)),
        ],
    );
    let riv = layer(
        riv,
        &[(3, &[], &[])],
        &[(
            4,
            &[],
            &[(
                ty::TRANSITION_BOOL_CONDITION,
                &[
                    (key::CONDITION_INPUT, V::U(0)),
                    (key::CONDITION_OP, V::U(0)),
                ],
            )],
        )],
        &[(0, &[]), (2, &[])],
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
    );
    let _ = scene(&riv);
    let m = dxn_vector_animation_rive::machine(&riv.bytes(), &Options::default())
        .unwrap()
        .unwrap();
    assert_eq!(m.listeners.len(), 1);
    assert_eq!(m.listeners[0].on, [PointerEvent::Down]);
    assert_eq!(
        m.listeners[0].actions,
        [Action::Input {
            input: "go".into(),
            change: Change::Set(InputValue::Bool(true))
        }]
    );
    let mut play = Interaction::new(riv.bytes(), Limits::default(), Options::default()).unwrap();
    // A press beside the square at 0.3 s does nothing; on it (at 34), it
    // sets "go".
    assert!(
        !play
            .pointer(0.3, Pointer::Down, Point::new(60.0, 15.0))
            .unwrap()
    );
    assert!(
        play.pointer(0.3, Pointer::Down, Point::new(34.0, 15.0))
            .unwrap()
    );
    assert_eq!(play.changes().len(), 1);
    let s = &play.import().scene;
    assert!(near(at(s, 0.25, 30, 15), RED, 2));
    assert!(near(at(s, 0.55, 70, 15), RED, 2));
}

#[test]
fn a_state_of_negative_speed_plays_its_animation_backwards_from_its_end() {
    // "backA" (red from 90 to 10, once) at speed -1: from 10 to 90.
    let riv = layer(machine(squares()), &[(3, &[], &[])], &[], &[]).obj(
        ty::ANIMATION_STATE,
        &[(key::ANIMATION_ID, V::U(2)), (key::STATE_SPEED, V::D(-1.0))],
    );
    let s = scene(&riv);
    assert_eq!(at(&s, 0.25, 30, 15), RED);
    assert_eq!(at(&s, 0.25, 70, 15), WHITE);
    assert_eq!(at(&s, 1.5, 90, 15), RED);
    assert_eq!(at(&s, 1.5, 10, 15), WHITE);
}

/// The red and blue squares, "moveB" moving the blue one and playing, and
/// a listener moving the red one (node 1) to the pointer as it moves over
/// the blue one, keeping its offset as `keep_offset` says.
fn follower(keep_offset: bool) -> Riv {
    let mut riv = Riv::new()
        .artboard(100.0, 50.0)
        .rect_shape(0, 10.0, 15.0, 10.0, 10.0, 0xFFFF_0000)
        .rect_shape(0, 10.0, 35.0, 10.0, 10.0, 0xFF00_00FF)
        .obj(
            ty::LINEAR_ANIMATION,
            &[
                (key::ANIMATION_NAME, V::S("moveB")),
                (key::FPS, V::U(10)),
                (key::DURATION, V::U(20)),
                (key::LOOP, V::U(1)),
            ],
        )
        .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(5))])
        .obj(
            ty::KEYED_PROPERTY,
            &[(key::PROPERTY_KEY, V::U(u64::from(key::X)))],
        );
    for (frame, value) in [(0, 10.0), (20, 90.0)] {
        riv = riv.obj(
            ty::KEY_FRAME_DOUBLE,
            &[
                (key::FRAME, V::U(frame)),
                (key::VALUE, V::D(value)),
                (key::INTERPOLATION, V::U(1)),
            ],
        );
    }
    layer(machine(riv), &[(3, &[], &[])], &[], &[(0, &[])])
        .obj(
            ty::LISTENER_SINGLE,
            &[
                (key::LISTENER_TARGET, V::U(5)),
                (key::LISTENER_TYPE, V::U(4)),
            ],
        )
        .obj(
            ty::LISTENER_ALIGN_TARGET,
            &[
                (key::ALIGN_TARGET, V::U(1)),
                (key::ALIGN_PRESERVE_OFFSET, V::B(keep_offset)),
            ],
        )
}

#[test]
fn listeners_move_nodes_to_the_pointer() {
    let riv = follower(false);
    let _ = scene(&riv);
    let m = dxn_vector_animation_rive::machine(&riv.bytes(), &Options::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        m.listeners[0].actions,
        [Action::Align {
            node: 1,
            keep_offset: false
        }]
    );
    let mut play = Interaction::new(riv.bytes(), Limits::default(), Options::default()).unwrap();
    // Moving off the blue square moves nothing; on it, the red one comes.
    assert!(
        !play
            .pointer(0.05, Pointer::Move, Point::new(60.0, 35.0))
            .unwrap()
    );
    assert!(
        play.pointer(0.05, Pointer::Move, Point::new(12.0, 35.0))
            .unwrap()
    );
    assert_eq!(play.aligned().len(), 1);
    let s = &play.import().scene;
    assert_eq!(at(s, 0.0, 10, 15), RED);
    assert_eq!(at(s, 0.1, 10, 15), WHITE);
    assert_eq!(at(s, 0.1, 12, 35), RED);
}

#[test]
fn listeners_keeping_the_offset_move_nodes_as_far_as_the_pointer() {
    let riv = follower(true);
    let _ = scene(&riv);
    let mut play = Interaction::new(riv.bytes(), Limits::default(), Options::default()).unwrap();
    // Entering the blue square starts afresh: the red one stays; moving on
    // by 3, 3 moves it as far.
    play.pointer(0.0, Pointer::Move, Point::new(10.0, 35.0))
        .unwrap();
    play.pointer(0.05, Pointer::Move, Point::new(13.0, 38.0))
        .unwrap();
    let s = &play.import().scene;
    assert_eq!(at(s, 0.02, 10, 15), RED);
    assert_eq!(at(s, 0.02, 17, 22), WHITE);
    assert_eq!(at(s, 0.1, 17, 22), RED);
    assert_eq!(at(s, 0.1, 6, 11), WHITE);
}

/// A 100 × 50 artboard with a red 40 × 30 square in its middle (shape 1)
/// and a bool input "on" that clicks on it toggle: off, the square holds
/// at x 50 ("left"); on, at 70 ("right").
fn toggled() -> Riv {
    let mut riv =
        Riv::new()
            .artboard(100.0, 50.0)
            .rect_shape(0, 50.0, 25.0, 40.0, 30.0, 0xFFFF_0000);
    for (name, x) in [("left", 50.0), ("right", 70.0)] {
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
    let riv = machine(riv).obj(
        ty::STATE_MACHINE_BOOL,
        &[
            (key::INPUT_NAME, V::S("on")),
            (key::BOOL_VALUE, V::B(false)),
        ],
    );
    let on = |op| -> [(u16, &[(u16, V)]); 1] {
        [(
            ty::TRANSITION_BOOL_CONDITION,
            if op == 0 {
                &[
                    (key::CONDITION_INPUT, V::U(0)),
                    (key::CONDITION_OP, V::U(0)),
                ]
            } else {
                &[
                    (key::CONDITION_INPUT, V::U(0)),
                    (key::CONDITION_OP, V::U(1)),
                ]
            },
        )]
    };
    let (set, unset) = (on(0), on(1));
    layer(
        riv,
        &[(3, &[], &[])],
        &[(4, &[], &set), (3, &[], &unset)],
        &[(0, &[]), (1, &[])],
    )
    .obj(
        ty::LISTENER_SINGLE,
        &[
            (key::LISTENER_TARGET, V::U(1)),
            (key::LISTENER_TYPE, V::U(6)),
        ],
    )
    .obj(
        ty::LISTENER_BOOL_CHANGE,
        &[
            (key::ACTION_INPUT, V::U(0)),
            (key::ACTION_BOOL_VALUE, V::U(2)),
        ],
    )
}

#[test]
fn click_listeners_toggle_bool_inputs() {
    let riv = toggled();
    let _ = scene(&riv);
    let mut play = Interaction::new(riv.bytes(), Limits::default(), Options::default()).unwrap();
    let middle = Point::new(50.0, 25.0);
    // A click (a press, then a finger lifted) turns it on, another off.
    for (t, kind) in [
        (0.2, Pointer::Down),
        (0.25, Pointer::Lift),
        (0.5, Pointer::Down),
        (0.55, Pointer::Lift),
    ] {
        play.pointer(t, kind, middle).unwrap();
    }
    assert_eq!(
        play.changes().iter().map(|c| c.value).collect::<Vec<_>>(),
        [InputValue::Bool(true), InputValue::Bool(false)]
    );
    let s = &play.import().scene;
    assert_eq!(at(s, 0.1, 85, 25), WHITE);
    assert_eq!(at(s, 0.4, 85, 25), RED);
    assert_eq!(at(s, 0.8, 85, 25), WHITE);
    assert_eq!(at(s, 0.8, 35, 25), RED);
}

/// A red square (shape 1) at x 10 that a joystick's x animation ("jx", 10
/// to 90) moves as its handle (node 9, in the middle) goes across the
/// artboard, a blue hit area over all of it (shape 5) whose listener
/// moves the handle to the pointer, and a state machine looping "idle".
fn joystick() -> Riv {
    let mut riv = Riv::new()
        .artboard(100.0, 50.0)
        .rect_shape(0, 10.0, 15.0, 10.0, 10.0, 0xFFFF_0000)
        .rect_shape(0, 50.0, 25.0, 100.0, 50.0, 0xFF00_00FF)
        .obj(
            ty::NODE,
            &[
                (key::PARENT_ID, V::U(0)),
                (key::X, V::D(50.0)),
                (key::Y, V::D(25.0)),
            ],
        )
        .obj(
            ty::JOYSTICK,
            &[
                (key::PARENT_ID, V::U(0)),
                (key::JOYSTICK_X_ID, V::U(0)),
                (key::JOYSTICK_POS_X, V::D(50.0)),
                (key::JOYSTICK_POS_Y, V::D(25.0)),
                (key::JOYSTICK_WIDTH, V::D(100.0)),
                (key::JOYSTICK_HEIGHT, V::D(50.0)),
                (key::JOYSTICK_HANDLE, V::U(9)),
            ],
        );
    for (name, object, keys, loop_value) in [
        ("jx", 1, [(0, 10.0), (10, 90.0)], 0),
        ("idle", 5, [(0, 50.0), (10, 50.0)], 1),
    ] {
        riv = riv
            .obj(
                ty::LINEAR_ANIMATION,
                &[
                    (key::ANIMATION_NAME, V::S(name)),
                    (key::FPS, V::U(10)),
                    (key::DURATION, V::U(10)),
                    (key::LOOP, V::U(loop_value)),
                ],
            )
            .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(object))])
            .obj(
                ty::KEYED_PROPERTY,
                &[(key::PROPERTY_KEY, V::U(u64::from(key::X)))],
            );
        for (frame, value) in keys {
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
    layer(machine(riv), &[(3, &[], &[])], &[], &[(1, &[])])
        .obj(
            ty::LISTENER_SINGLE,
            &[
                (key::LISTENER_TARGET, V::U(5)),
                (key::LISTENER_TYPE, V::U(4)),
            ],
        )
        .obj(ty::LISTENER_ALIGN_TARGET, &[(key::ALIGN_TARGET, V::U(9))])
}

#[test]
fn a_joystick_follows_its_handle_moved_to_the_pointer() {
    let riv = joystick();
    let _ = scene(&riv);
    let mut play = Interaction::new(riv.bytes(), Limits::default(), Options::default()).unwrap();
    // The handle in the middle: the square halfway, at 50; moved to the
    // right edge at 0.5 s (the pointer on the hit area's edge, which Rive
    // hits within 2 units), the square goes to 90 and stays there.
    play.pointer(0.5, Pointer::Move, Point::new(100.0, 25.0))
        .unwrap();
    let s = &play.import().scene;
    assert_eq!(at(s, 0.2, 50, 15), RED);
    assert_eq!(at(s, 0.6, 90, 15), RED);
    assert_eq!(at(s, 0.95, 90, 15), RED);
    assert_eq!(at(s, 0.95, 50, 15), BLUE);
}
