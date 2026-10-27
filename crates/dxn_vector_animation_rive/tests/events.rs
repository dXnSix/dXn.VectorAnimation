//! Rive events as the timeline's cues: what the default state machine's
//! states and transitions fire, what the animations its states play key,
//! with an event's custom properties or link; and what listeners fire.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, key, scene, ty};
use dxn_vector_animation::{CueValue, Limits, Point};
use dxn_vector_animation_rive::{Interaction, Options, Pointer};

/// A 100 × 50 artboard with a red square (shape 1) at (10, 25) and the
/// events `events` after it (objects 5 on: each a type, a name and its
/// children's types and properties), then two animations moving the square:
/// "go" (0) from x 10 to 90 in a second, once, keying `keyed` (an event
/// and the frames at 10 a second its trigger is keyed at), and "loop" (1),
/// a second long, looping, keying `looped` the same way.
/// Objects of a type with properties.
type Objects = Vec<(u16, Vec<(u16, V)>)>;

fn file(
    events: &[(u16, &'static str, Objects)],
    keyed: Option<(u64, &[u64])>,
    looped: Option<(u64, &[u64])>,
) -> Riv {
    let mut riv =
        Riv::new()
            .artboard(100.0, 50.0)
            .rect_shape(0, 10.0, 25.0, 10.0, 10.0, 0xFFFF_0000);
    let mut index = 5;
    for (t, name, children) in events {
        riv = riv.obj(*t, &[(key::PARENT_ID, V::U(0)), (key::NAME, V::S(name))]);
        let event = index;
        index += 1;
        for (ct, props) in children {
            let mut props = props.clone();
            props.insert(0, (key::PARENT_ID, V::U(event)));
            riv = riv.obj(*ct, &props);
            index += 1;
        }
    }
    for (name, loop_value, moves, keys) in [("go", 0, true, keyed), ("loop", 1, false, looped)] {
        riv = riv.obj(
            ty::LINEAR_ANIMATION,
            &[
                (key::ANIMATION_NAME, V::S(name)),
                (key::FPS, V::U(10)),
                (key::DURATION, V::U(10)),
                (key::LOOP, V::U(loop_value)),
            ],
        );
        if moves {
            riv = riv.obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(1))]).obj(
                ty::KEYED_PROPERTY,
                &[(key::PROPERTY_KEY, V::U(u64::from(key::X)))],
            );
            for (frame, x) in [(0, 10.0), (10, 90.0)] {
                riv = riv.obj(
                    ty::KEY_FRAME_DOUBLE,
                    &[
                        (key::FRAME, V::U(frame)),
                        (key::VALUE, V::D(x)),
                        (key::INTERPOLATION, V::U(1)),
                    ],
                );
            }
        }
        if let Some((event, frames)) = keys {
            riv = riv
                .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(event))])
                .obj(
                    ty::KEYED_PROPERTY,
                    &[(key::PROPERTY_KEY, V::U(u64::from(key::EVENT_TRIGGER)))],
                );
            for &frame in frames {
                riv = riv.obj(ty::KEY_FRAME_CALLBACK, &[(key::FRAME, V::U(frame))]);
            }
        }
    }
    riv.obj(
        ty::STATE_MACHINE,
        &[(key::ANIMATION_NAME, V::S("State Machine 1"))],
    )
}

fn fire(event: u64, at_end: bool) -> (u16, Vec<(u16, V)>) {
    (
        ty::FIRE_EVENT,
        vec![
            (key::FIRE_EVENT_ID, V::U(event)),
            (key::FIRE_OCCURS, V::U(u64::from(at_end))),
        ],
    )
}

fn objs(mut riv: Riv, objects: Vec<(u16, Vec<(u16, V)>)>) -> Riv {
    for (t, props) in objects {
        riv = riv.obj(t, &props);
    }
    riv
}

/// The cues of `riv` (read as a scene), each its name and time.
fn cues(riv: &Riv) -> Vec<(String, f64)> {
    scene(riv)
        .timeline
        .cues
        .iter()
        .map(|c| (c.name.clone(), (c.time * 1000.0).round() / 1000.0))
        .collect()
}

#[test]
fn states_and_transitions_fire_their_events() {
    // Entered "go" fires "entered"; at its end, the transition to "loop"
    // fires "leaving" (as it starts) and "arrived" (as its half second of
    // mixing ends), and "go" fires "left" as it is left. Its end comes in
    // the frame after 1 s (61 frames of 1/60 s in 32-bit floats), as in
    // Rive's runtime.
    let events =
        ["entered", "leaving", "arrived", "left"].map(|name| (ty::EVENT, name, Vec::new()));
    let riv = file(&events, None, None)
        .obj(ty::STATE_MACHINE_LAYER, &[])
        .obj(ty::ENTRY_STATE, &[])
        .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(3))])
        .obj(ty::ANY_STATE, &[])
        .obj(ty::EXIT_STATE, &[])
        .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(0))]);
    let riv = objs(riv, vec![fire(5, false), fire(8, true)]).obj(
        ty::STATE_TRANSITION,
        &[
            (key::STATE_TO_ID, V::U(4)),
            (key::TRANSITION_FLAGS, V::U(4 | 8)),
            (key::EXIT_TIME, V::U(100)),
            (key::TRANSITION_DURATION, V::U(500)),
        ],
    );
    let riv = objs(riv, vec![fire(6, false), fire(7, true)])
        .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(1))]);
    assert_eq!(
        cues(&riv),
        [
            ("entered".into(), 0.0),
            ("left".into(), 1.017),
            ("leaving".into(), 1.017),
            ("arrived".into(), 1.517),
        ]
    );
}

#[test]
fn animations_fire_the_events_they_key_as_they_pass_them() {
    // "loop" keys "tick" at 0 and 0.5 s: each time it passes them, a
    // frame's fraction late at most.
    let riv = file(&[(ty::EVENT, "tick", Vec::new())], None, Some((5, &[0, 5])))
        .obj(ty::STATE_MACHINE_LAYER, &[])
        .obj(ty::ENTRY_STATE, &[])
        .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(3))])
        .obj(ty::ANY_STATE, &[])
        .obj(ty::EXIT_STATE, &[])
        .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(1))]);
    let s = scene(&riv);
    let times: Vec<f64> = s.timeline.cues.iter().map(|c| c.time).collect();
    assert!(times.len() >= 2, "{times:?}");
    assert!(times[0].abs() < 1e-3, "{times:?}");
    assert!((times[1] - 0.5).abs() < 1e-3, "{times:?}");
    assert!(times.iter().all(|t| *t <= s.timeline.duration + 1e-9));
}

#[test]
fn a_cue_carries_its_events_properties_and_link() {
    let events = [
        (
            ty::EVENT,
            "score",
            vec![
                (
                    ty::CUSTOM_NUMBER,
                    vec![
                        (key::NAME, V::S("points")),
                        (key::CUSTOM_NUMBER_VALUE, V::D(3.0)),
                    ],
                ),
                (
                    ty::CUSTOM_STRING,
                    vec![
                        (key::NAME, V::S("who")),
                        (key::CUSTOM_STRING_VALUE, V::S("me")),
                    ],
                ),
                (
                    ty::CUSTOM_BOOL,
                    vec![
                        (key::NAME, V::S("won")),
                        (key::CUSTOM_BOOL_VALUE, V::B(true)),
                    ],
                ),
            ],
        ),
        (ty::OPEN_URL_EVENT, "help", Vec::new()),
    ];
    // The link: on the open-URL event (object 9).
    let riv = file(&events, None, None)
        .obj(ty::STATE_MACHINE_LAYER, &[])
        .obj(ty::ENTRY_STATE, &[])
        .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(3))])
        .obj(ty::ANY_STATE, &[])
        .obj(ty::EXIT_STATE, &[])
        .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(0))]);
    let riv = objs(riv, vec![fire(5, false), fire(9, false)]);
    let s = scene(&riv);
    let c = &s.timeline.cues;
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].name, "score");
    assert_eq!(
        c[0].properties,
        [
            ("points".into(), CueValue::Number(3.0)),
            ("who".into(), CueValue::Text("me".into())),
            ("won".into(), CueValue::Bool(true)),
        ]
    );
    assert_eq!(c[1].name, "help");
    assert_eq!(c[1].url, Some((String::new(), "_blank".into())));
}

#[test]
fn listeners_fire_events() {
    let riv = file(&[(ty::EVENT, "poked", Vec::new())], None, None)
        .obj(ty::STATE_MACHINE_LAYER, &[])
        .obj(ty::ENTRY_STATE, &[])
        .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(3))])
        .obj(ty::ANY_STATE, &[])
        .obj(ty::EXIT_STATE, &[])
        .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(1))])
        .obj(
            ty::LISTENER_SINGLE,
            &[
                (key::LISTENER_TARGET, V::U(1)),
                (key::LISTENER_TYPE, V::U(2)),
            ],
        )
        .obj(
            ty::LISTENER_FIRE_EVENT,
            &[(key::FIRE_LISTENER_EVENT, V::U(5))],
        );
    let _ = scene(&riv);
    let mut play = Interaction::new(riv.bytes(), Limits::default(), Options::default()).unwrap();
    play.pointer(0.2, Pointer::Down, Point::new(10.0, 25.0))
        .unwrap();
    let fired = play.take_fired();
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].name, "poked");
    assert_eq!(fired[0].time, 0.2);
    assert!(play.take_fired().is_empty());
}
