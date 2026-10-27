//! Data binding: the artboard's view model's first instance bound, as
//! Rive's editor and players binding automatically have it; its values set
//! what they are bound to (through converters and nested view models), and
//! the state machine's conditions on them hold as they compare.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, inked, key, scene, ty};
use dxn_vector_animation::{Limits, Point};
use dxn_vector_animation_rive::{
    Action, Interaction, Options, Pointer, PropertyValue, ValueChange, read_options,
};

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];
const FONT: &[u8] = include_bytes!("../../../fixtures/fonts/KaTeX_SansSerif-Regular.ttf");

/// Types and properties of view models and converters these tests write.
mod dt {
    pub const VM_BOOL: u16 = 448;
    pub const VM_INSTANCE_BOOL: u16 = 449;
    pub const VM_BOOL_VALUE: u16 = 593;
    pub const VM_VIEW_MODEL: u16 = 436;
    pub const VM_VIEW_MODEL_REFERENCE: u16 = 565;
    pub const VM_INSTANCE_VIEW_MODEL: u16 = 444;
    pub const VM_INSTANCE_VIEW_MODEL_VALUE: u16 = 577;
    pub const BIND_CONVERTER: u16 = 660;
    pub const OPERATION_VALUE: u16 = 500;
    pub const OPERATION_TYPE: u16 = 682;
    pub const OPERAND: u16 = 681;
    pub const RANGE_MAPPER: u16 = 519;
    pub const RANGE_MIN_INPUT: u16 = 716;
    pub const RANGE_MAX_INPUT: u16 = 717;
    pub const RANGE_MIN_OUTPUT: u16 = 718;
    pub const RANGE_MAX_OUTPUT: u16 = 719;
    pub const GROUP: u16 = 499;
    pub const GROUP_ITEM: u16 = 498;
    pub const GROUP_ITEM_CONVERTER: u16 = 679;
    pub const ROUNDER: u16 = 489;
    pub const BOOLEAN_NEGATE: u16 = 535;
    pub const TO_STRING: u16 = 490;
    pub const TO_STRING_FLAGS: u16 = 764;
    pub const TO_STRING_DECIMALS: u16 = 765;
    pub const IS_VISIBLE: u16 = 41;
    pub const VM_TRIGGER: u16 = 502;
    pub const VM_INSTANCE_TRIGGER: u16 = 501;
    pub const BINDABLE_TRIGGER: u16 = 503;
    pub const VALUE_TRIGGER_COMPARATOR: u16 = 505;
    pub const SELF_COMPARATOR: u16 = 593;
    pub const BINDABLE_TRIGGER_VALUE: u16 = 686;
    pub const BINDABLE_NUMBER_VALUE: u16 = 636;
    pub const TRIGGER_CONVERTER: u16 = 504;
    pub const BIND_FLAGS: u16 = 587;
    pub const LISTENER_VIEW_MODEL_CHANGE: u16 = 487;
    pub const NESTED_DATA_BIND_PATH: u16 = 582;
    pub const MANIFEST_ASSET: u16 = 642;
}

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

/// `riv` with a binding of property `property` of what it read last to the
/// view model property at `path`, through converter `converter`.
fn bind(riv: Riv, property: u16, path: &[u8], converter: Option<u64>) -> Riv {
    let mut props = vec![
        (key::BIND_PROPERTY_KEY, V::U(u64::from(property))),
        (key::BIND_PATH, V::Bytes(path.to_vec())),
    ];
    if let Some(c) = converter {
        props.push((dt::BIND_CONVERTER, V::U(c)));
    }
    riv.obj(ty::DATA_BIND_CONTEXT, &props)
}

/// A view model with a number "n" (property 0) of 12.34, a bool "off"
/// (property 1) of false and a number "pick" (property 2) of 1, and
/// converters: 0 multiplies by 2, 1 maps 0 to 20 onto 0 to 100, 2 groups 3
/// (rounding to a whole number) and 4 (adding 40), 5 negates.
fn with_converters() -> Riv {
    Riv::new()
        .obj(
            dt::OPERATION_VALUE,
            &[(dt::OPERATION_TYPE, V::U(2)), (dt::OPERAND, V::D(2.0))],
        )
        .obj(
            dt::RANGE_MAPPER,
            &[
                (dt::RANGE_MIN_INPUT, V::D(0.0)),
                (dt::RANGE_MAX_INPUT, V::D(20.0)),
                (dt::RANGE_MIN_OUTPUT, V::D(0.0)),
                (dt::RANGE_MAX_OUTPUT, V::D(100.0)),
            ],
        )
        .obj(dt::GROUP, &[])
        .obj(dt::GROUP_ITEM, &[(dt::GROUP_ITEM_CONVERTER, V::U(3))])
        .obj(dt::GROUP_ITEM, &[(dt::GROUP_ITEM_CONVERTER, V::U(4))])
        .obj(dt::ROUNDER, &[])
        .obj(
            dt::OPERATION_VALUE,
            &[(dt::OPERATION_TYPE, V::U(0)), (dt::OPERAND, V::D(40.0))],
        )
        .obj(dt::BOOLEAN_NEGATE, &[])
        .obj(ty::VIEW_MODEL, &[(key::VM_NAME, V::S("Game"))])
        .obj(ty::VM_NUMBER, &[(key::VM_NAME, V::S("n"))])
        .obj(dt::VM_BOOL, &[(key::VM_NAME, V::S("off"))])
        .obj(ty::VM_NUMBER, &[(key::VM_NAME, V::S("pick"))])
        .obj(ty::VM_INSTANCE, &[(key::VM_INSTANCE_VM, V::U(0))])
        .obj(
            ty::VM_INSTANCE_NUMBER,
            &[
                (key::VM_VALUE_PROPERTY, V::U(0)),
                (key::VM_NUMBER_VALUE, V::D(12.34)),
            ],
        )
        .obj(
            dt::VM_INSTANCE_BOOL,
            &[
                (key::VM_VALUE_PROPERTY, V::U(1)),
                (dt::VM_BOOL_VALUE, V::B(false)),
            ],
        )
        .obj(
            ty::VM_INSTANCE_NUMBER,
            &[
                (key::VM_VALUE_PROPERTY, V::U(2)),
                (key::VM_NUMBER_VALUE, V::D(1.0)),
            ],
        )
}

/// The artboard of a file `riv` starts, 100 × 60 and bound to view model 0.
fn bound_artboard(riv: Riv) -> Riv {
    riv.obj(
        ty::ARTBOARD,
        &[
            (key::NAME, V::S("Main")),
            (key::WIDTH, V::D(100.0)),
            (key::HEIGHT, V::D(60.0)),
            (key::ORIGIN_X, V::D(0.0)),
            (key::ORIGIN_Y, V::D(0.0)),
            (key::ARTBOARD_VIEW_MODEL, V::U(0)),
        ],
    )
}

/// A 6 × 6 square filled with `argb` under `parent` at (`x`, `y`), its x
/// bound to `x_from` (a path and converter) and its fill's visibility to
/// `shown_from`.
fn square(
    riv: Riv,
    parent: u64,
    (x, y): (f32, f32),
    argb: u32,
    x_from: Option<(&[u8], Option<u64>)>,
    shown_from: Option<(&[u8], Option<u64>)>,
) -> Riv {
    let shape = riv.next_index();
    let mut riv = riv.shape(parent, x, y);
    if let Some((path, c)) = x_from {
        riv = bind(riv, key::X, path, c);
    }
    let fill = shape + 2;
    riv = riv
        .obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(shape)),
                (key::PATH_WIDTH, V::D(6.0)),
                (key::PATH_HEIGHT, V::D(6.0)),
            ],
        )
        .obj(ty::FILL, &[(key::PARENT_ID, V::U(shape))]);
    if let Some((path, c)) = shown_from {
        riv = bind(riv, dt::IS_VISIBLE, path, c);
    }
    riv.obj(
        ty::SOLID_COLOR,
        &[(key::PARENT_ID, V::U(fill)), (key::COLOR, V::C(argb))],
    )
}

const RED_ARGB: u32 = 0xFFFF_0000;

#[test]
fn converters_change_values_on_their_way_to_what_is_bound() {
    let mut riv = bound_artboard(with_converters());
    // 12.34 × 2, mapped to 61.7, rounded plus 40.
    riv = square(riv, 0, (0.0, 5.0), RED_ARGB, Some((&[0, 0], Some(0))), None);
    riv = square(
        riv,
        0,
        (0.0, 15.0),
        RED_ARGB,
        Some((&[0, 0], Some(1))),
        None,
    );
    riv = square(
        riv,
        0,
        (0.0, 25.0),
        RED_ARGB,
        Some((&[0, 0], Some(2))),
        None,
    );
    // Shown as "off" negated, hidden as "off".
    riv = square(
        riv,
        0,
        (80.0, 35.0),
        RED_ARGB,
        None,
        Some((&[0, 1], Some(5))),
    );
    riv = square(riv, 0, (90.0, 35.0), RED_ARGB, None, Some((&[0, 1], None)));
    let s = scene(&riv);
    assert_eq!(color_at(&s, 0.0, 24, 5), RED);
    assert_eq!(color_at(&s, 0.0, 61, 15), RED);
    assert_eq!(color_at(&s, 0.0, 52, 25), RED);
    assert_eq!(color_at(&s, 0.0, 2, 25), WHITE);
    assert_eq!(color_at(&s, 0.0, 80, 35), RED);
    assert_eq!(color_at(&s, 0.0, 90, 35), WHITE);
}

#[test]
fn a_number_bound_to_a_solo_picks_its_child_by_index() {
    let riv = bound_artboard(with_converters());
    let solo = riv.next_index();
    let mut riv = riv.obj(ty::SOLO, &[(key::PARENT_ID, V::U(0))]);
    riv = bind(riv, key::ACTIVE_COMPONENT_ID, &[0, 2], None);
    for x in [20.0, 50.0, 80.0] {
        riv = square(riv, solo, (x, 30.0), RED_ARGB, None, None);
    }
    let s = scene(&riv);
    assert_eq!(color_at(&s, 0.0, 50, 30), RED);
    assert_eq!(color_at(&s, 0.0, 20, 30), WHITE);
    assert_eq!(color_at(&s, 0.0, 80, 30), WHITE);
}

/// `riv` with the font embedded, then a 100 × 60 artboard bound to view
/// model 0 and black 30-unit text at (5, 5) of one run of `text`, bound
/// through converter `converter` to "n" when there is one.
fn bound_text(riv: Riv, text: &'static str, converter: Option<u64>) -> Riv {
    let riv = riv.obj(ty::FONT_ASSET, &[(key::NAME, V::S("KaTeX"))]).obj(
        ty::FILE_ASSET_CONTENTS,
        &[(key::BYTES, V::Bytes(FONT.to_vec()))],
    );
    let riv = bound_artboard(riv)
        .obj(
            ty::TEXT,
            &[
                (key::PARENT_ID, V::U(0)),
                (key::X, V::D(5.0)),
                (key::Y, V::D(5.0)),
            ],
        )
        .obj(
            ty::TEXT_STYLE_PAINT,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::FONT_SIZE, V::D(30.0)),
                (key::FONT_ASSET_ID, V::U(0)),
            ],
        )
        .fill(2, 0xFF00_0000)
        .obj(
            ty::TEXT_VALUE_RUN,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::RUN_STYLE_ID, V::U(2)),
                (key::RUN_TEXT, V::S(text)),
            ],
        );
    match converter {
        Some(c) => bind(riv, key::RUN_TEXT, &[0, 0], Some(c)),
        None => riv,
    }
}

#[test]
fn numbers_converted_to_text_set_runs() {
    // "n" with one decimal: as the run "12.3" draws.
    let riv = with_converters().obj(
        dt::TO_STRING,
        &[
            (dt::TO_STRING_FLAGS, V::U(1)),
            (dt::TO_STRING_DECIMALS, V::U(1)),
        ],
    );
    let bound = scene(&bound_text(riv, "x", Some(6)));
    let written = scene(&bound_text(with_converters(), "12.3", None));
    assert!(inked(&bound, 0.0, 5, 5, 95, 55));
    for (x, y) in [(10, 20), (20, 20), (30, 25), (40, 20), (55, 20), (70, 20)] {
        assert_eq!(
            color_at(&bound, 0.0, x, y),
            color_at(&written, 0.0, x, y),
            "({x}, {y})"
        );
    }
}

#[test]
fn nested_view_models_bind_their_instances_values() {
    // "Game" holds a "Player" (view model 1, its instance 1 of x 70) as
    // property 0; the square's x is bound to the player's x.
    let riv = Riv::new()
        .obj(ty::VIEW_MODEL, &[(key::VM_NAME, V::S("Game"))])
        .obj(
            dt::VM_VIEW_MODEL,
            &[
                (key::VM_NAME, V::S("player")),
                (dt::VM_VIEW_MODEL_REFERENCE, V::U(1)),
            ],
        )
        .obj(ty::VM_INSTANCE, &[(key::VM_INSTANCE_VM, V::U(0))])
        .obj(
            dt::VM_INSTANCE_VIEW_MODEL,
            &[
                (key::VM_VALUE_PROPERTY, V::U(0)),
                (dt::VM_INSTANCE_VIEW_MODEL_VALUE, V::U(1)),
            ],
        )
        .obj(ty::VIEW_MODEL, &[(key::VM_NAME, V::S("Player"))])
        .obj(ty::VM_NUMBER, &[(key::VM_NAME, V::S("x"))])
        .obj(ty::VM_INSTANCE, &[(key::VM_INSTANCE_VM, V::U(1))])
        .obj(
            ty::VM_INSTANCE_NUMBER,
            &[
                (key::VM_VALUE_PROPERTY, V::U(0)),
                (key::VM_NUMBER_VALUE, V::D(30.0)),
            ],
        )
        .obj(ty::VM_INSTANCE, &[(key::VM_INSTANCE_VM, V::U(1))])
        .obj(
            ty::VM_INSTANCE_NUMBER,
            &[
                (key::VM_VALUE_PROPERTY, V::U(0)),
                (key::VM_NUMBER_VALUE, V::D(70.0)),
            ],
        );
    let riv = square(
        bound_artboard(riv),
        0,
        (10.0, 30.0),
        RED_ARGB,
        Some((&[0, 0, 0], None)),
        None,
    );
    let s = scene(&riv);
    assert_eq!(color_at(&s, 0.0, 70, 30), RED);
    assert_eq!(color_at(&s, 0.0, 10, 30), WHITE);
}

/// `riv` read with the view model changes `values` (path, value) made at
/// their times.
fn changed(riv: &Riv, values: &[(f64, &str, PropertyValue)]) -> dxn_vector_animation::Scene {
    let options = Options {
        values: values
            .iter()
            .map(|(time, path, value)| ValueChange {
                time: *time,
                path: (*path).into(),
                value: value.clone(),
            })
            .collect(),
        ..Options::default()
    };
    // Written for Rive's runtime to replay too.
    let _ = scene(riv);
    read_options(&riv.bytes(), &Limits::default(), &options)
        .unwrap()
        .scene
}

#[test]
fn values_changed_over_time_change_what_they_are_bound_to() {
    // Green from 0.5 s; "level" 5 from 1 s sends the square right.
    let s = changed(
        &bound(1.0),
        &[
            (0.5, "tint", PropertyValue::Color(0xFF00_FF00)),
            (1.0, "level", PropertyValue::Number(5.0)),
        ],
    );
    assert_eq!(color_at(&s, 0.25, 10, 25), BLUE);
    assert_eq!(color_at(&s, 0.75, 10, 25), [0, 255, 0]);
    assert_eq!(color_at(&s, 1.25, 10, 25), WHITE);
    assert_eq!(color_at(&s, 1.25, 70, 25), [0, 255, 0]);
}

/// The scene of `bound` with a trigger "go" (property 2) and a number
/// "count" (property 3), its machine going from "left" to "right" when
/// "go" is fired and back to "left" when "count" changes.
fn triggered() -> Riv {
    let riv = Riv::new()
        .obj(ty::VIEW_MODEL, &[(key::VM_NAME, V::S("Game"))])
        .obj(ty::VM_NUMBER, &[(key::VM_NAME, V::S("level"))])
        .obj(ty::VM_COLOR, &[(key::VM_NAME, V::S("tint"))])
        .obj(dt::VM_TRIGGER, &[(key::VM_NAME, V::S("go"))])
        .obj(ty::VM_NUMBER, &[(key::VM_NAME, V::S("count"))])
        .obj(ty::VM_INSTANCE, &[(key::VM_INSTANCE_VM, V::U(0))])
        .obj(
            dt::VM_INSTANCE_TRIGGER,
            &[(key::VM_VALUE_PROPERTY, V::U(2))],
        )
        .obj(ty::VM_INSTANCE_NUMBER, &[(key::VM_VALUE_PROPERTY, V::U(3))]);
    let mut riv = bound_artboard(riv).rect_shape(0, 10.0, 25.0, 10.0, 10.0, 0xFFFF_0000);
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
    .obj(ty::VM_CONDITION, &[])
    .obj(dt::BINDABLE_TRIGGER, &[])
    .obj(
        ty::DATA_BIND_CONTEXT,
        &[
            (
                key::BIND_PROPERTY_KEY,
                V::U(u64::from(dt::BINDABLE_TRIGGER_VALUE)),
            ),
            (key::BIND_PATH, V::Bytes(vec![0, 2])),
        ],
    )
    .obj(ty::VM_PROPERTY_COMPARATOR, &[])
    .obj(dt::VALUE_TRIGGER_COMPARATOR, &[])
    .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(1))])
    .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(3))])
    .obj(ty::VM_CONDITION, &[])
    .obj(ty::BINDABLE_NUMBER, &[])
    .obj(
        ty::DATA_BIND_CONTEXT,
        &[
            (key::BIND_PROPERTY_KEY, V::U(636)),
            (key::BIND_PATH, V::Bytes(vec![0, 3])),
        ],
    )
    .obj(ty::VM_PROPERTY_COMPARATOR, &[])
    .obj(dt::SELF_COMPARATOR, &[])
}

#[test]
fn triggers_and_changing_values_hold_in_the_frame_they_change() {
    let riv = triggered();
    // Nothing fired: it stays left.
    let s = changed(&riv, &[]);
    assert_eq!(color_at(&s, 1.0, 10, 25), RED);
    // Fired at 0.5 s: right; "count" changed at 1 s: left again; set to
    // what it holds at 1.5 s: it stays.
    let s = changed(
        &riv,
        &[
            (0.5, "go", PropertyValue::Trigger),
            (1.0, "count", PropertyValue::Number(2.0)),
            (1.5, "go", PropertyValue::Trigger),
            (2.0, "count", PropertyValue::Number(2.0)),
        ],
    );
    assert_eq!(color_at(&s, 0.25, 10, 25), RED);
    assert_eq!(color_at(&s, 0.75, 70, 25), RED);
    assert_eq!(color_at(&s, 1.25, 10, 25), RED);
    assert_eq!(color_at(&s, 1.75, 70, 25), RED);
    assert_eq!(color_at(&s, 2.25, 70, 25), RED);
}

/// A view model with a trigger "go" and a number "level" of 80; a red
/// square (shape 1) at (10, 15) its state machine sends right (to 70) when
/// "go" is fired, and a 6 × 6 one (shape 5) at "level" across, 40 down. A
/// press on the red square fires "go" (through a trigger converter, as
/// Rive's editor writes it); one on the other sets "level" to 50.
fn listened() -> Riv {
    let riv = Riv::new()
        .obj(dt::TRIGGER_CONVERTER, &[])
        .obj(ty::VIEW_MODEL, &[(key::VM_NAME, V::S("Game"))])
        .obj(dt::VM_TRIGGER, &[(key::VM_NAME, V::S("go"))])
        .obj(ty::VM_NUMBER, &[(key::VM_NAME, V::S("level"))])
        .obj(ty::VM_INSTANCE, &[(key::VM_INSTANCE_VM, V::U(0))])
        .obj(
            dt::VM_INSTANCE_TRIGGER,
            &[(key::VM_VALUE_PROPERTY, V::U(0))],
        )
        .obj(
            ty::VM_INSTANCE_NUMBER,
            &[
                (key::VM_VALUE_PROPERTY, V::U(1)),
                (key::VM_NUMBER_VALUE, V::D(80.0)),
            ],
        );
    let riv = bound_artboard(riv).rect_shape(0, 10.0, 15.0, 10.0, 10.0, 0xFFFF_0000);
    let mut riv = square(
        riv,
        0,
        (0.0, 40.0),
        0xFF00_00FF,
        Some((&[0, 1], None)),
        None,
    );
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
    let to_view_model = |riv: Riv, property: u16, path: &[u8], converter: Option<u64>| {
        let mut props = vec![
            (key::BIND_PROPERTY_KEY, V::U(u64::from(property))),
            (dt::BIND_FLAGS, V::U(1)),
            (key::BIND_PATH, V::Bytes(path.to_vec())),
        ];
        if let Some(c) = converter {
            props.push((dt::BIND_CONVERTER, V::U(c)));
        }
        riv.obj(ty::DATA_BIND_CONTEXT, &props)
    };
    let riv = riv
        .obj(
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
        .obj(ty::VM_CONDITION, &[])
        .obj(dt::BINDABLE_TRIGGER, &[]);
    let riv = bind(riv, dt::BINDABLE_TRIGGER_VALUE, &[0, 0], None)
        .obj(ty::VM_PROPERTY_COMPARATOR, &[])
        .obj(dt::VALUE_TRIGGER_COMPARATOR, &[])
        .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(1))])
        .obj(
            ty::LISTENER_SINGLE,
            &[
                (key::LISTENER_TARGET, V::U(1)),
                (key::LISTENER_TYPE, V::U(2)),
            ],
        )
        .obj(dt::BINDABLE_TRIGGER, &[]);
    let riv = to_view_model(riv, dt::BINDABLE_TRIGGER_VALUE, &[0, 0], Some(0));
    let riv = bind(riv, dt::BINDABLE_TRIGGER_VALUE, &[0, 0], None)
        .obj(dt::LISTENER_VIEW_MODEL_CHANGE, &[])
        .obj(
            ty::LISTENER_SINGLE,
            &[
                (key::LISTENER_TARGET, V::U(5)),
                (key::LISTENER_TYPE, V::U(2)),
            ],
        )
        .obj(
            ty::BINDABLE_NUMBER,
            &[(dt::BINDABLE_NUMBER_VALUE, V::D(50.0))],
        );
    to_view_model(riv, dt::BINDABLE_NUMBER_VALUE, &[0, 1], None)
        .obj(dt::LISTENER_VIEW_MODEL_CHANGE, &[])
}

#[test]
fn listeners_change_view_model_values() {
    let riv = listened();
    let _ = scene(&riv);
    let m = dxn_vector_animation_rive::machine(&riv.bytes(), &Options::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        m.listeners[0].actions,
        [Action::Value {
            property: "go".into(),
            bindable: 1
        }]
    );
    let mut play = Interaction::new(riv.bytes(), Limits::default(), Options::default()).unwrap();
    // A press on the red square fires "go": it goes right.
    assert!(
        play.pointer(0.3, Pointer::Down, Point::new(15.0, 15.0))
            .unwrap()
    );
    assert_eq!(play.values().len(), 1);
    // One on the other sets "level" to 50.
    assert!(
        play.pointer(0.6, Pointer::Down, Point::new(80.0, 40.0))
            .unwrap()
    );
    let s = &play.import().scene;
    assert_eq!(color_at(s, 0.2, 10, 15), RED);
    assert_eq!(color_at(s, 0.4, 70, 15), RED);
    assert_eq!(color_at(s, 0.5, 80, 40), BLUE);
    assert_eq!(color_at(s, 0.7, 50, 40), BLUE);
    assert_eq!(color_at(s, 0.7, 80, 40), WHITE);
}

/// "Game" (view model 0) with a number "level" of 20 and a "player"
/// (view model 1, "Player", its instance 1 of x 60); the main artboard
/// shows "Child" (a red square at "level" across, 15 down) as its host's
/// instance has it, and "Badge" (a blue one at x across, 40 down) bound to
/// the player.
fn nested_contexts() -> Riv {
    let riv = Riv::new()
        .obj(ty::VIEW_MODEL, &[(key::VM_NAME, V::S("Game"))])
        .obj(ty::VM_NUMBER, &[(key::VM_NAME, V::S("level"))])
        .obj(
            dt::VM_VIEW_MODEL,
            &[
                (key::VM_NAME, V::S("player")),
                (dt::VM_VIEW_MODEL_REFERENCE, V::U(1)),
            ],
        )
        .obj(ty::VM_INSTANCE, &[(key::VM_INSTANCE_VM, V::U(0))])
        .obj(
            ty::VM_INSTANCE_NUMBER,
            &[
                (key::VM_VALUE_PROPERTY, V::U(0)),
                (key::VM_NUMBER_VALUE, V::D(20.0)),
            ],
        )
        .obj(
            dt::VM_INSTANCE_VIEW_MODEL,
            &[
                (key::VM_VALUE_PROPERTY, V::U(1)),
                (dt::VM_INSTANCE_VIEW_MODEL_VALUE, V::U(1)),
            ],
        )
        .obj(ty::VIEW_MODEL, &[(key::VM_NAME, V::S("Player"))])
        .obj(ty::VM_NUMBER, &[(key::VM_NAME, V::S("x"))]);
    let mut riv = riv;
    for x in [30.0, 60.0] {
        riv = riv
            .obj(ty::VM_INSTANCE, &[(key::VM_INSTANCE_VM, V::U(1))])
            .obj(
                ty::VM_INSTANCE_NUMBER,
                &[
                    (key::VM_VALUE_PROPERTY, V::U(0)),
                    (key::VM_NUMBER_VALUE, V::D(x)),
                ],
            );
    }
    let riv = bound_artboard(riv)
        .obj(
            ty::NESTED_ARTBOARD,
            &[(key::PARENT_ID, V::U(0)), (key::ARTBOARD_ID, V::U(1))],
        )
        .obj(
            ty::NESTED_ARTBOARD,
            &[
                (key::PARENT_ID, V::U(0)),
                (key::ARTBOARD_ID, V::U(2)),
                (dt::NESTED_DATA_BIND_PATH, V::Bytes(vec![0, 1])),
            ],
        )
        .obj(
            ty::STATE_MACHINE,
            &[(key::ANIMATION_NAME, V::S("State Machine 1"))],
        )
        .obj(ty::STATE_MACHINE_LAYER, &[])
        .obj(ty::ENTRY_STATE, &[])
        .obj(ty::ANY_STATE, &[])
        .obj(ty::EXIT_STATE, &[]);
    let riv = riv.artboard(100.0, 60.0);
    let riv = square(
        riv,
        0,
        (0.0, 15.0),
        0xFFFF_0000,
        Some((&[0, 0], None)),
        None,
    );
    let riv = riv.artboard(100.0, 60.0);
    square(
        riv,
        0,
        (0.0, 40.0),
        0xFF00_00FF,
        Some((&[1, 0], None)),
        None,
    )
}

#[test]
fn nested_artboards_bind_their_hosts_instance_or_the_one_their_path_names() {
    let riv = nested_contexts();
    // "level" 50 from 1 s moves the red square.
    let s = changed(&riv, &[(1.0, "level", PropertyValue::Number(50.0))]);
    assert_eq!(color_at(&s, 0.5, 20, 15), RED);
    assert_eq!(color_at(&s, 0.5, 60, 40), BLUE);
    assert_eq!(color_at(&s, 0.5, 30, 40), WHITE);
    assert_eq!(color_at(&s, 1.5, 50, 15), RED);
    assert_eq!(color_at(&s, 1.5, 20, 15), WHITE);
}

/// Rive's manifest of `names` and `paths` (each an id and its value).
fn manifest(names: &[(u8, &str)], paths: &[(u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut section = vec![names.len() as u8];
    for (id, name) in names {
        section.extend([*id, name.len() as u8]);
        section.extend(name.as_bytes());
    }
    out.extend([0, section.len() as u8]);
    out.extend(section);
    let mut section = vec![paths.len() as u8];
    for (id, path) in paths {
        section.extend([*id, path.len() as u8]);
        section.extend(*path);
    }
    out.extend([1, section.len() as u8]);
    out.extend(section);
    out
}

#[test]
fn bindings_by_name_read_the_properties_their_names_name() {
    // "level" 40 and "tint" blue, bound by the manifest's names: the
    // square's x through path 5 ("level"), its color through name 2
    // ("tint") itself.
    let riv = Riv::new()
        .obj(dt::MANIFEST_ASSET, &[(key::NAME, V::S("manifest"))])
        .obj(
            ty::FILE_ASSET_CONTENTS,
            &[(
                key::BYTES,
                V::Bytes(manifest(
                    &[(1, "level"), (2, "tint")],
                    &[(5, &[1]), (6, &[2])],
                )),
            )],
        )
        .obj(ty::VIEW_MODEL, &[(key::VM_NAME, V::S("Game"))])
        .obj(ty::VM_COLOR, &[(key::VM_NAME, V::S("tint"))])
        .obj(ty::VM_NUMBER, &[(key::VM_NAME, V::S("level"))])
        .obj(ty::VM_INSTANCE, &[(key::VM_INSTANCE_VM, V::U(0))])
        .obj(
            ty::VM_INSTANCE_COLOR,
            &[
                (key::VM_VALUE_PROPERTY, V::U(0)),
                (key::VM_COLOR_VALUE, V::C(0xFF00_00FF)),
            ],
        )
        .obj(
            ty::VM_INSTANCE_NUMBER,
            &[
                (key::VM_VALUE_PROPERTY, V::U(1)),
                (key::VM_NUMBER_VALUE, V::D(40.0)),
            ],
        );
    let by_name = |riv: Riv, property: u16, path: &[u8]| {
        riv.obj(
            ty::DATA_BIND_CONTEXT,
            &[
                (key::BIND_PROPERTY_KEY, V::U(u64::from(property))),
                (dt::BIND_FLAGS, V::U(16)),
                (key::BIND_PATH, V::Bytes(path.to_vec())),
            ],
        )
    };
    let riv = by_name(bound_artboard(riv).shape(0, 10.0, 30.0), key::X, &[5])
        .obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::PATH_WIDTH, V::D(10.0)),
                (key::PATH_HEIGHT, V::D(10.0)),
            ],
        )
        .obj(ty::FILL, &[(key::PARENT_ID, V::U(1))])
        .obj(
            ty::SOLID_COLOR,
            &[(key::PARENT_ID, V::U(3)), (key::COLOR, V::C(0xFFFF_0000))],
        );
    let riv = by_name(riv, key::COLOR, &[2]);
    let s = scene(&riv);
    assert_eq!(color_at(&s, 0.0, 40, 30), BLUE);
    assert_eq!(color_at(&s, 0.0, 10, 30), WHITE);
}
