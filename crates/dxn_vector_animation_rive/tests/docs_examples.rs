//! The Rive files the documentation's examples play (`examples/docs`): a
//! switch its listener toggles, a ball firing events as it travels, and a
//! progress bar a view model drives through converters. Built here, checked
//! to draw what they show, and written to `fixtures/docs/` with
//! `DXN_RIVE_DOCS` set (the check tool's `scenes --rive` takes them from
//! there into the examples app).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, inked, key, near, scene, ty};
use dxn_vector_animation::{Limits, Point};
use dxn_vector_animation_rive::{
    Interaction, Options, Pointer, PropertyValue, ValueChange, read_options,
};

/// Types and properties these files write that the shared ones do not
/// name.
mod dt {
    pub const TRANSITION_INTERPOLATION: u16 = 349;
    pub const TRANSITION_INTERPOLATOR: u16 = 350;
    pub const VM_TRIGGER: u16 = 502;
    pub const VM_INSTANCE_TRIGGER: u16 = 501;
    pub const BINDABLE_TRIGGER: u16 = 503;
    pub const BINDABLE_TRIGGER_VALUE: u16 = 686;
    pub const VALUE_TRIGGER_COMPARATOR: u16 = 505;
    pub const BIND_CONVERTER: u16 = 660;
    pub const GROUP: u16 = 499;
    pub const GROUP_ITEM: u16 = 498;
    pub const GROUP_ITEM_CONVERTER: u16 = 679;
    pub const INTERPOLATOR: u16 = 534;
    pub const INTERPOLATOR_DURATION: u16 = 756;
    pub const INTERPOLATOR_ID: u16 = 758;
    pub const RANGE_MAPPER: u16 = 519;
    pub const RANGE_FLAGS: u16 = 715;
    pub const RANGE_MIN_INPUT: u16 = 716;
    pub const RANGE_MAX_INPUT: u16 = 717;
    pub const RANGE_MIN_OUTPUT: u16 = 718;
    pub const RANGE_MAX_OUTPUT: u16 = 719;
    pub const TO_STRING: u16 = 490;
    pub const TO_STRING_FLAGS: u16 = 764;
}

const FONT: &[u8] = include_bytes!("../../../fixtures/fonts/KaTeX_SansSerif-Regular.ttf");

/// Writes `riv` to `fixtures/docs/<name>.riv` when `DXN_RIVE_DOCS` is set.
fn publish(riv: &Riv, name: &str) {
    if std::env::var_os("DXN_RIVE_DOCS").is_some() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/docs");
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(format!("{dir}/{name}.riv"), riv.bytes()).unwrap();
    }
}

/// `riv` with a cubic ease interpolator (`x1`, `y1`, `x2`, `y2`).
fn ease(riv: Riv, [x1, y1, x2, y2]: [f32; 4]) -> Riv {
    riv.obj(
        ty::CUBIC_EASE_INTERPOLATOR,
        &[
            (key::X1, V::D(x1)),
            (key::Y1, V::D(y1)),
            (key::X2, V::D(x2)),
            (key::Y2, V::D(y2)),
        ],
    )
}

/// `riv` with a shape under `parent` at (`x`, `y`) holding a `w` × `h`
/// rectangle of corner radius `r` (left-anchored when `left`) filled with
/// `argb`: four objects, the shape's index first.
fn rounded(
    riv: Riv,
    parent: u64,
    (x, y): (f32, f32),
    (w, h, r): (f32, f32, f32),
    argb: u32,
    left: bool,
) -> Riv {
    let shape = riv.next_index();
    let mut rect = vec![
        (key::PARENT_ID, V::U(shape)),
        (key::PATH_WIDTH, V::D(w)),
        (key::PATH_HEIGHT, V::D(h)),
        (key::CORNER_RADIUS_TL, V::D(r)),
    ];
    if left {
        rect.push((key::PATH_ORIGIN_X, V::D(0.0)));
    }
    riv.shape(parent, x, y)
        .obj(ty::RECTANGLE, &rect)
        .fill(shape, argb)
}

/// A one-frame animation named `name` keying `keys` (object, property and
/// value; colors as `V::C`).
fn pose(mut riv: Riv, name: &'static str, keys: &[(u64, u16, V)]) -> Riv {
    riv = riv.obj(
        ty::LINEAR_ANIMATION,
        &[
            (key::ANIMATION_NAME, V::S(name)),
            (key::FPS, V::U(60)),
            (key::DURATION, V::U(1)),
        ],
    );
    for (object, property, value) in keys {
        riv = riv
            .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(*object))])
            .obj(
                ty::KEYED_PROPERTY,
                &[(key::PROPERTY_KEY, V::U(u64::from(*property)))],
            );
        riv = match value {
            V::C(c) => riv.obj(
                ty::KEY_FRAME_COLOR,
                &[(key::FRAME, V::U(0)), (key::COLOR_VALUE, V::C(*c))],
            ),
            v => riv.obj(
                ty::KEY_FRAME_DOUBLE,
                &[(key::FRAME, V::U(0)), (key::VALUE, v.clone())],
            ),
        };
    }
    riv
}

const GRAY: u32 = 0xFFD1_D5DB;
const GREEN: u32 = 0xFF22_C55E;
const WHITE: u32 = 0xFFFF_FFFF;

/// A switch, 160 × 90: a white knob (shape 1, drawn on top as the first)
/// over a gray track (shape 5) that slides right as the track turns green
/// when its bool input "on" is set, eased over 0.3 s; a click on the track
/// toggles it.
fn switch() -> Riv {
    let riv = Riv::new().artboard(160.0, 90.0);
    let knob = riv.next_index();
    let riv = riv.shape(0, 55.0, 45.0).obj(
        ty::ELLIPSE,
        &[
            (key::PARENT_ID, V::U(knob)),
            (key::PATH_WIDTH, V::D(40.0)),
            (key::PATH_HEIGHT, V::D(40.0)),
        ],
    );
    let riv = riv.fill(knob, WHITE);
    let track = riv.next_index();
    let riv = rounded(riv, 0, (80.0, 45.0), (100.0, 50.0, 25.0), GRAY, false);
    let color = track + 3;
    let curve = riv.next_index();
    let riv = ease(riv, [0.4, 0.0, 0.2, 1.0]);
    let riv = pose(
        riv,
        "off",
        &[(knob, key::X, V::D(55.0)), (color, key::COLOR, V::C(GRAY))],
    );
    let riv = pose(
        riv,
        "on",
        &[
            (knob, key::X, V::D(105.0)),
            (color, key::COLOR, V::C(GREEN)),
        ],
    );
    let to = |state: u64, on: bool| {
        (
            vec![
                (key::STATE_TO_ID, V::U(state)),
                (key::TRANSITION_DURATION, V::U(300)),
                (dt::TRANSITION_INTERPOLATION, V::U(2)),
                (dt::TRANSITION_INTERPOLATOR, V::U(curve)),
            ],
            vec![
                (key::CONDITION_INPUT, V::U(0)),
                (key::CONDITION_OP, V::U(u64::from(!on))),
            ],
        )
    };
    let (to_on, if_on) = to(4, true);
    let (to_off, if_off) = to(3, false);
    riv.obj(
        ty::STATE_MACHINE,
        &[(key::ANIMATION_NAME, V::S("State Machine 1"))],
    )
    .obj(
        ty::STATE_MACHINE_BOOL,
        &[
            (key::INPUT_NAME, V::S("on")),
            (key::BOOL_VALUE, V::B(false)),
        ],
    )
    .obj(ty::STATE_MACHINE_LAYER, &[])
    .obj(ty::ENTRY_STATE, &[])
    .obj(ty::STATE_TRANSITION, &[(key::STATE_TO_ID, V::U(3))])
    .obj(ty::ANY_STATE, &[])
    .obj(ty::EXIT_STATE, &[])
    .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(0))])
    .obj(ty::STATE_TRANSITION, &to_on)
    .obj(ty::TRANSITION_BOOL_CONDITION, &if_on)
    .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(1))])
    .obj(ty::STATE_TRANSITION, &to_off)
    .obj(ty::TRANSITION_BOOL_CONDITION, &if_off)
    // A click on the track toggles "on".
    .obj(
        ty::LISTENER_SINGLE,
        &[
            (key::LISTENER_TARGET, V::U(track)),
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
fn the_switch_turns_on_and_off_as_it_is_clicked() {
    let riv = switch();
    let s = scene(&riv);
    assert!(near(color_at(&s, 0.0, 115, 45), [209, 213, 219], 2), "gray");
    assert!(
        near(color_at(&s, 0.0, 55, 45), [255, 255, 255], 2),
        "knob left"
    );
    let mut play = Interaction::new(riv.bytes(), Limits::default(), Options::default()).unwrap();
    let at = Point::new(80.0, 45.0);
    play.pointer(0.5, Pointer::Down, at).unwrap();
    assert!(play.pointer(0.55, Pointer::Up, at).unwrap());
    let s = &play.import().scene;
    assert!(
        near(color_at(s, 1.2, 105, 45), [255, 255, 255], 2),
        "knob right"
    );
    assert!(near(color_at(s, 1.2, 45, 45), [34, 197, 94], 2), "green");
    publish(&riv, "switch");
}

/// A ball crossing a 300 × 80 artboard and back every 2 s, eased, firing
/// "left" (object 5) and "right" (object 6) as it reaches each side, and
/// "started" as its state is entered.
fn travel() -> Riv {
    let riv = Riv::new().artboard(300.0, 80.0);
    let ball = riv.next_index();
    let riv = riv.shape(0, 30.0, 40.0).obj(
        ty::ELLIPSE,
        &[
            (key::PARENT_ID, V::U(ball)),
            (key::PATH_WIDTH, V::D(36.0)),
            (key::PATH_HEIGHT, V::D(36.0)),
        ],
    );
    let riv = riv.fill(ball, 0xFF63_66F1);
    let mut riv = riv;
    for name in ["left", "right", "started"] {
        riv = riv.obj(
            ty::EVENT,
            &[(key::PARENT_ID, V::U(0)), (key::NAME, V::S(name))],
        );
    }
    let curve = riv.next_index();
    let riv = ease(riv, [0.45, 0.0, 0.55, 1.0]);
    let mut riv = riv
        .obj(
            ty::LINEAR_ANIMATION,
            &[
                (key::ANIMATION_NAME, V::S("travel")),
                (key::FPS, V::U(10)),
                (key::DURATION, V::U(20)),
                (key::LOOP, V::U(1)),
            ],
        )
        .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(ball))])
        .obj(
            ty::KEYED_PROPERTY,
            &[(key::PROPERTY_KEY, V::U(u64::from(key::X)))],
        );
    for (frame, x) in [(0, 30.0), (10, 270.0), (20, 30.0)] {
        riv = riv.obj(
            ty::KEY_FRAME_DOUBLE,
            &[
                (key::FRAME, V::U(frame)),
                (key::VALUE, V::D(x)),
                (key::INTERPOLATION, V::U(2)),
                (key::INTERPOLATOR_ID, V::U(curve)),
            ],
        );
    }
    // "left" at the start alone: keyed at the end too, Rive fires it twice
    // as the animation loops.
    for (event, frame) in [(5u64, 0u64), (6, 10)] {
        riv = riv
            .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(event))])
            .obj(
                ty::KEYED_PROPERTY,
                &[(key::PROPERTY_KEY, V::U(u64::from(key::EVENT_TRIGGER)))],
            )
            .obj(ty::KEY_FRAME_CALLBACK, &[(key::FRAME, V::U(frame))]);
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
    .obj(
        ty::FIRE_EVENT,
        &[(key::FIRE_EVENT_ID, V::U(7)), (key::FIRE_OCCURS, V::U(0))],
    )
}

#[test]
fn the_ball_fires_its_events_as_it_travels() {
    let riv = travel();
    let s = scene(&riv);
    let names: Vec<&str> = s.timeline.cues.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.starts_with(&["started", "left", "right"]),
        "{names:?}"
    );
    assert!(inked(&s, 1.0, 250, 30, 290, 50), "right at 1 s");
    publish(&riv, "travel");
}

/// A progress bar, 300 × 100, bound to view model "Progress": "value" (0
/// to 100, 30) sets its width through an interpolating converter (0.6 s,
/// eased) and a range mapper (0 to 260), and its label through the same
/// interpolator and a whole number's text; "tint" its color, eased too;
/// "celebrate", a trigger, makes it pulse once.
fn progress() -> Riv {
    let riv = Riv::new()
        .obj(ty::FONT_ASSET, &[(key::NAME, V::S("KaTeX"))])
        .obj(
            ty::FILE_ASSET_CONTENTS,
            &[(key::BYTES, V::Bytes(FONT.to_vec()))],
        );
    // The converters' easing, the file's own.
    let riv = ease(riv, [0.4, 0.0, 0.2, 1.0])
        // 0: the width (1, then 2); 3: the label (1, then 4).
        .obj(dt::GROUP, &[])
        .obj(dt::GROUP_ITEM, &[(dt::GROUP_ITEM_CONVERTER, V::U(1))])
        .obj(dt::GROUP_ITEM, &[(dt::GROUP_ITEM_CONVERTER, V::U(2))])
        .obj(
            dt::INTERPOLATOR,
            &[
                (dt::INTERPOLATOR_DURATION, V::D(0.6)),
                (dt::INTERPOLATOR_ID, V::U(0)),
            ],
        )
        .obj(
            dt::RANGE_MAPPER,
            &[
                (dt::RANGE_FLAGS, V::U(3)),
                (dt::RANGE_MIN_INPUT, V::D(0.0)),
                (dt::RANGE_MAX_INPUT, V::D(100.0)),
                (dt::RANGE_MIN_OUTPUT, V::D(0.0)),
                (dt::RANGE_MAX_OUTPUT, V::D(260.0)),
            ],
        )
        .obj(dt::GROUP, &[])
        .obj(dt::GROUP_ITEM, &[(dt::GROUP_ITEM_CONVERTER, V::U(1))])
        .obj(dt::GROUP_ITEM, &[(dt::GROUP_ITEM_CONVERTER, V::U(4))])
        .obj(dt::TO_STRING, &[(dt::TO_STRING_FLAGS, V::U(1))])
        .obj(ty::VIEW_MODEL, &[(key::VM_NAME, V::S("Progress"))])
        .obj(ty::VM_NUMBER, &[(key::VM_NAME, V::S("value"))])
        .obj(ty::VM_COLOR, &[(key::VM_NAME, V::S("tint"))])
        .obj(dt::VM_TRIGGER, &[(key::VM_NAME, V::S("celebrate"))])
        .obj(ty::VM_INSTANCE, &[(key::VM_INSTANCE_VM, V::U(0))])
        .obj(
            ty::VM_INSTANCE_NUMBER,
            &[
                (key::VM_VALUE_PROPERTY, V::U(0)),
                (key::VM_NUMBER_VALUE, V::D(30.0)),
            ],
        )
        .obj(
            ty::VM_INSTANCE_COLOR,
            &[
                (key::VM_VALUE_PROPERTY, V::U(1)),
                (key::VM_COLOR_VALUE, V::C(0xFF3B_82F6)),
            ],
        )
        .obj(
            dt::VM_INSTANCE_TRIGGER,
            &[(key::VM_VALUE_PROPERTY, V::U(2))],
        )
        .obj(
            ty::ARTBOARD,
            &[
                (key::NAME, V::S("Progress")),
                (key::WIDTH, V::D(300.0)),
                (key::HEIGHT, V::D(100.0)),
                (key::ORIGIN_X, V::D(0.0)),
                (key::ORIGIN_Y, V::D(0.0)),
                (key::ARTBOARD_VIEW_MODEL, V::U(0)),
            ],
        );
    let bind = |riv: Riv, property: u16, path: &[u8], converter: Option<u64>| {
        let mut props = vec![
            (key::BIND_PROPERTY_KEY, V::U(u64::from(property))),
            (key::BIND_PATH, V::Bytes(path.to_vec())),
        ];
        if let Some(c) = converter {
            props.push((dt::BIND_CONVERTER, V::U(c)));
        }
        riv.obj(ty::DATA_BIND_CONTEXT, &props)
    };
    // The bar (1 to 4, on top as the first): its width and color bound;
    // then the track.
    let bar = riv.next_index();
    let riv = riv.shape(0, 20.0, 65.0).obj(
        ty::RECTANGLE,
        &[
            (key::PARENT_ID, V::U(bar)),
            (key::PATH_WIDTH, V::D(78.0)),
            (key::PATH_HEIGHT, V::D(16.0)),
            (key::CORNER_RADIUS_TL, V::D(8.0)),
            (key::PATH_ORIGIN_X, V::D(0.0)),
        ],
    );
    let riv = bind(riv, key::PATH_WIDTH, &[0, 0], Some(0));
    let riv = riv.obj(ty::FILL, &[(key::PARENT_ID, V::U(bar))]).obj(
        ty::SOLID_COLOR,
        &[
            (key::PARENT_ID, V::U(bar + 2)),
            (key::COLOR, V::C(0xFF3B_82F6)),
        ],
    );
    let riv = bind(riv, key::COLOR, &[0, 1], Some(1));
    let riv = rounded(
        riv,
        0,
        (150.0, 65.0),
        (260.0, 16.0, 8.0),
        0xFFE5_E7EB,
        false,
    );
    // The label: the value's whole number, then "%".
    let text = riv.next_index();
    let riv = riv.obj(
        ty::TEXT,
        &[
            (key::PARENT_ID, V::U(0)),
            (key::X, V::D(20.0)),
            (key::Y, V::D(12.0)),
        ],
    );
    let style = riv.next_index();
    let riv = riv
        .obj(
            ty::TEXT_STYLE_PAINT,
            &[
                (key::PARENT_ID, V::U(text)),
                (key::FONT_SIZE, V::D(30.0)),
                (key::FONT_ASSET_ID, V::U(0)),
            ],
        )
        .fill(style, 0xFF11_1827)
        .obj(
            ty::TEXT_VALUE_RUN,
            &[
                (key::PARENT_ID, V::U(text)),
                (key::RUN_STYLE_ID, V::U(style)),
                (key::RUN_TEXT, V::S("30")),
            ],
        );
    let riv = bind(riv, key::RUN_TEXT, &[0, 0], Some(3)).obj(
        ty::TEXT_VALUE_RUN,
        &[
            (key::PARENT_ID, V::U(text)),
            (key::RUN_STYLE_ID, V::U(style)),
            (key::RUN_TEXT, V::S("%")),
        ],
    );
    // "idle" holds the bar; "pulse" swells it and back in 0.4 s.
    let curve = riv.next_index();
    let riv = ease(riv, [0.4, 0.0, 0.2, 1.0]);
    let riv = pose(riv, "idle", &[(bar, key::SCALE_Y, V::D(1.0))]);
    let mut riv = riv
        .obj(
            ty::LINEAR_ANIMATION,
            &[
                (key::ANIMATION_NAME, V::S("pulse")),
                (key::FPS, V::U(10)),
                (key::DURATION, V::U(4)),
            ],
        )
        .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(bar))])
        .obj(
            ty::KEYED_PROPERTY,
            &[(key::PROPERTY_KEY, V::U(u64::from(key::SCALE_Y)))],
        );
    for (frame, s) in [(0, 1.0), (2, 1.8), (4, 1.0)] {
        riv = riv.obj(
            ty::KEY_FRAME_DOUBLE,
            &[
                (key::FRAME, V::U(frame)),
                (key::VALUE, V::D(s)),
                (key::INTERPOLATION, V::U(2)),
                (key::INTERPOLATOR_ID, V::U(curve)),
            ],
        );
    }
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
    bind(riv, dt::BINDABLE_TRIGGER_VALUE, &[0, 2], None)
        .obj(ty::VM_PROPERTY_COMPARATOR, &[])
        .obj(dt::VALUE_TRIGGER_COMPARATOR, &[])
        .obj(ty::ANIMATION_STATE, &[(key::ANIMATION_ID, V::U(1))])
        .obj(
            ty::STATE_TRANSITION,
            &[
                (key::STATE_TO_ID, V::U(3)),
                (key::TRANSITION_FLAGS, V::U(4 | 8)),
                (key::EXIT_TIME, V::U(100)),
            ],
        )
}

#[test]
fn the_progress_bar_follows_its_view_model() {
    let riv = progress();
    let s = scene(&riv);
    // 30: the bar to x 98.
    assert!(
        near(color_at(&s, 0.0, 90, 65), [59, 130, 246], 2),
        "blue to 98"
    );
    assert!(
        near(color_at(&s, 0.0, 110, 65), [229, 231, 235], 2),
        "track past it"
    );
    assert!(inked(&s, 0.0, 20, 12, 80, 40), "the label");
    // 80 and green at 0.5 s: eased there by 1.2 s; "celebrate" at 1.5 s
    // swells it at 1.7 s.
    let options = Options {
        values: vec![
            ValueChange {
                time: 0.5,
                path: "value".into(),
                value: PropertyValue::Number(80.0),
            },
            ValueChange {
                time: 0.5,
                path: "tint".into(),
                value: PropertyValue::Color(0xFF22_C55E),
            },
            ValueChange {
                time: 1.5,
                path: "celebrate".into(),
                value: PropertyValue::Trigger,
            },
        ],
        ..Options::default()
    };
    let s = read_options(&riv.bytes(), &Limits::default(), &options)
        .unwrap()
        .scene;
    assert!(
        near(color_at(&s, 1.2, 220, 65), [34, 197, 94], 2),
        "green to 228"
    );
    assert!(
        near(color_at(&s, 1.7, 150, 53), [34, 197, 94], 2),
        "swollen"
    );
    assert!(
        !near(color_at(&s, 1.4, 150, 53), [34, 197, 94], 30),
        "not yet"
    );
    publish(&riv, "progress");
}
