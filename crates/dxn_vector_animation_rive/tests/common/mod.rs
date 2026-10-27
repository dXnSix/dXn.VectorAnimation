//! Rive files written byte by byte, and what the tests read of them.

#![allow(dead_code)]

use dxn_vector_animation::{Color, Scene};
use dxn_vector_animation_render::{RenderOptions, render};

/// Type keys (rive-runtime's `dev/defs`).
pub mod ty {
    pub const ARTBOARD: u16 = 1;
    pub const NODE: u16 = 2;
    pub const SHAPE: u16 = 3;
    pub const ELLIPSE: u16 = 4;
    pub const STRAIGHT_VERTEX: u16 = 5;
    pub const CUBIC_DETACHED_VERTEX: u16 = 6;
    pub const RECTANGLE: u16 = 7;
    pub const POINTS_PATH: u16 = 16;
    pub const RADIAL_GRADIENT: u16 = 17;
    pub const SOLID_COLOR: u16 = 18;
    pub const GRADIENT_STOP: u16 = 19;
    pub const FILL: u16 = 20;
    pub const LINEAR_GRADIENT: u16 = 22;
    pub const BACKBOARD: u16 = 23;
    pub const STROKE: u16 = 24;
    pub const KEYED_OBJECT: u16 = 25;
    pub const KEYED_PROPERTY: u16 = 26;
    pub const CUBIC_EASE_INTERPOLATOR: u16 = 28;
    pub const KEY_FRAME_DOUBLE: u16 = 30;
    pub const LINEAR_ANIMATION: u16 = 31;
    pub const KEY_FRAME_COLOR: u16 = 37;
    pub const CLIPPING_SHAPE: u16 = 42;
    pub const TRIM_PATH: u16 = 47;
    pub const KEY_FRAME_ID: u16 = 50;
    pub const STATE_MACHINE: u16 = 53;
    pub const STATE_MACHINE_LAYER: u16 = 57;
    pub const ANIMATION_STATE: u16 = 61;
    pub const ANY_STATE: u16 = 62;
    pub const ENTRY_STATE: u16 = 63;
    pub const EXIT_STATE: u16 = 64;
    pub const STATE_TRANSITION: u16 = 65;
    pub const IMAGE: u16 = 100;
    pub const IMAGE_ASSET: u16 = 105;
    pub const FILE_ASSET_CONTENTS: u16 = 106;
    pub const FORCED_EDGE: u16 = 112;
    pub const CUBIC_VALUE_INTERPOLATOR: u16 = 138;
    pub const SOLO: u16 = 147;
    pub const ELASTIC_INTERPOLATOR: u16 = 174;
    pub const FEATHER: u16 = 533;
    pub const NESTED_ARTBOARD: u16 = 92;
    pub const NESTED_STATE_MACHINE: u16 = 95;
    pub const NESTED_SIMPLE_ANIMATION: u16 = 96;
    pub const NESTED_REMAP_ANIMATION: u16 = 98;
    pub const TEXT: u16 = 134;
    pub const TEXT_VALUE_RUN: u16 = 135;
    pub const TEXT_STYLE_PAINT: u16 = 137;
    pub const FONT_ASSET: u16 = 141;
    pub const KEY_FRAME_STRING: u16 = 142;
    pub const TEXT_STYLE_AXIS: u16 = 144;
    pub const BONE: u16 = 40;
    pub const ROOT_BONE: u16 = 41;
    pub const SKIN: u16 = 43;
    pub const TENDON: u16 = 44;
    pub const WEIGHT: u16 = 45;
    pub const CUBIC_WEIGHT: u16 = 46;
    pub const MESH_VERTEX: u16 = 108;
    pub const MESH: u16 = 109;
    pub const IK_CONSTRAINT: u16 = 81;
    pub const DISTANCE_CONSTRAINT: u16 = 82;
    pub const TRANSFORM_CONSTRAINT: u16 = 83;
    pub const TRANSLATION_CONSTRAINT: u16 = 87;
    pub const SCALE_CONSTRAINT: u16 = 88;
    pub const ROTATION_CONSTRAINT: u16 = 89;
    pub const FOLLOW_PATH_CONSTRAINT: u16 = 165;
    pub const DRAW_TARGET: u16 = 48;
    pub const DRAW_RULES: u16 = 49;
    pub const TEXT_MODIFIER_RANGE: u16 = 158;
    pub const TEXT_MODIFIER_GROUP: u16 = 159;
    pub const CUBIC_INTERPOLATOR_COMPONENT: u16 = 163;
    pub const STATE_MACHINE_NUMBER: u16 = 56;
    pub const STATE_MACHINE_BOOL: u16 = 59;
    pub const TRANSITION_NUMBER_CONDITION: u16 = 70;
    pub const TRANSITION_BOOL_CONDITION: u16 = 71;
    pub const NESTED_BOOL: u16 = 123;
    pub const STATE_MACHINE_TRIGGER: u16 = 58;
    pub const LISTENER_SINGLE: u16 = 114;
    pub const LISTENER_BOOL_CHANGE: u16 = 117;
    pub const LISTENER_ALIGN_TARGET: u16 = 126;
    pub const JOYSTICK: u16 = 148;
    pub const BLEND_STATE_DIRECT: u16 = 73;
    pub const CUSTOM_NUMBER: u16 = 127;
    pub const VIEW_MODEL: u16 = 435;
    pub const VM_NUMBER: u16 = 431;
    pub const VM_COLOR: u16 = 440;
    pub const VM_INSTANCE: u16 = 437;
    pub const VM_INSTANCE_NUMBER: u16 = 442;
    pub const VM_INSTANCE_COLOR: u16 = 426;
    pub const DATA_BIND_CONTEXT: u16 = 447;
    pub const BINDABLE_NUMBER: u16 = 473;
    pub const VM_CONDITION: u16 = 482;
    pub const VM_PROPERTY_COMPARATOR: u16 = 479;
    pub const VALUE_NUMBER_COMPARATOR: u16 = 484;
    pub const EVENT: u16 = 128;
    pub const CUSTOM_BOOL: u16 = 129;
    pub const CUSTOM_STRING: u16 = 130;
    pub const OPEN_URL_EVENT: u16 = 131;
    pub const LISTENER_FIRE_EVENT: u16 = 168;
    pub const LISTENER_TRIGGER_CHANGE: u16 = 115;
    pub const FIRE_EVENT: u16 = 169;
    pub const KEY_FRAME_CALLBACK: u16 = 171;
    pub const BLEND_ANIMATION_1D: u16 = 75;
    pub const BLEND_STATE_1D_INPUT: u16 = 76;
    pub const BLEND_ANIMATION_DIRECT: u16 = 77;
    pub const TRANSITION_TRIGGER_CONDITION: u16 = 68;
    pub const KEY_FRAME_BOOL: u16 = 84;
}

/// Property keys.
pub mod key {
    pub const NAME: u16 = 4;
    pub const PARENT_ID: u16 = 5;
    pub const WIDTH: u16 = 7;
    pub const HEIGHT: u16 = 8;
    pub const ORIGIN_X: u16 = 11;
    pub const ORIGIN_Y: u16 = 12;
    pub const X: u16 = 13;
    pub const Y: u16 = 14;
    pub const ROTATION: u16 = 15;
    pub const SCALE_X: u16 = 16;
    pub const OPACITY: u16 = 18;
    pub const PATH_WIDTH: u16 = 20;
    pub const PATH_HEIGHT: u16 = 21;
    pub const BLEND_MODE: u16 = 23;
    pub const VERTEX_X: u16 = 24;
    pub const VERTEX_Y: u16 = 25;
    pub const RADIUS: u16 = 26;
    pub const CORNER_RADIUS_TL: u16 = 31;
    pub const IS_CLOSED: u16 = 32;
    pub const GRADIENT_END_X: u16 = 34;
    pub const COLOR: u16 = 37;
    pub const STOP_COLOR: u16 = 38;
    pub const STOP_POSITION: u16 = 39;
    pub const FILL_RULE: u16 = 40;
    pub const GRADIENT_START_X: u16 = 42;
    pub const THICKNESS: u16 = 47;
    pub const OBJECT_ID: u16 = 51;
    pub const PROPERTY_KEY: u16 = 53;
    pub const ANIMATION_NAME: u16 = 55;
    pub const FPS: u16 = 56;
    pub const DURATION: u16 = 57;
    pub const SPEED: u16 = 58;
    pub const LOOP: u16 = 59;
    pub const WORK_START: u16 = 60;
    pub const WORK_END: u16 = 61;
    pub const ENABLE_WORK_AREA: u16 = 62;
    pub const X1: u16 = 63;
    pub const Y1: u16 = 64;
    pub const X2: u16 = 65;
    pub const Y2: u16 = 66;
    pub const FRAME: u16 = 67;
    pub const INTERPOLATION: u16 = 68;
    pub const INTERPOLATOR_ID: u16 = 69;
    pub const VALUE: u16 = 70;
    pub const COLOR_VALUE: u16 = 88;
    pub const SOURCE_ID: u16 = 92;
    pub const TRIM_START: u16 = 114;
    pub const TRIM_END: u16 = 115;
    pub const TRIM_MODE: u16 = 117;
    pub const ID_VALUE: u16 = 122;
    pub const PATH_ORIGIN_X: u16 = 123;
    pub const PATH_ORIGIN_Y: u16 = 124;
    pub const PATH_FLAGS: u16 = 128;
    pub const DRAWABLE_FLAGS: u16 = 129;
    pub const ANIMATION_ID: u16 = 149;
    pub const STATE_SPEED: u16 = 292;
    pub const STATE_TO_ID: u16 = 151;
    pub const LINK_CORNER_RADIUS: u16 = 164;
    pub const CLIP: u16 = 196;
    pub const ASSET_ID: u16 = 206;
    pub const BYTES: u16 = 212;
    pub const ACTIVE_COMPONENT_ID: u16 = 296;
    pub const ELASTIC_EASING: u16 = 405;
    pub const FEATHER_SPACE: u16 = 748;
    pub const FEATHER_STRENGTH: u16 = 749;
    pub const FEATHER_OFFSET_X: u16 = 750;
    pub const FEATHER_OFFSET_Y: u16 = 751;
    pub const FEATHER_INNER: u16 = 752;
    pub const ARTBOARD_ID: u16 = 197;
    pub const NESTED_ANIMATION_ID: u16 = 198;
    pub const NESTED_SPEED: u16 = 199;
    pub const IS_PLAYING: u16 = 201;
    pub const REMAP_TIME: u16 = 202;
    pub const RUN_TEXT: u16 = 268;
    pub const RUN_STYLE_ID: u16 = 272;
    pub const FONT_SIZE: u16 = 274;
    pub const FONT_ASSET_ID: u16 = 279;
    pub const STRING_VALUE: u16 = 280;
    pub const TEXT_ALIGN: u16 = 281;
    pub const TEXT_SIZING: u16 = 284;
    pub const TEXT_WIDTH: u16 = 285;
    pub const TEXT_HEIGHT: u16 = 286;
    pub const TEXT_OVERFLOW: u16 = 287;
    pub const TEXT_ORIGIN_X: u16 = 366;
    pub const TEXT_ORIGIN_Y: u16 = 367;
    pub const LINE_HEIGHT: u16 = 370;
    pub const PARAGRAPH_SPACING: u16 = 371;
    pub const TEXT_ORIGIN: u16 = 377;
    pub const LETTER_SPACING: u16 = 390;
    pub const VERTICAL_ALIGN: u16 = 685;
    pub const BONE_LENGTH: u16 = 89;
    pub const ROOT_BONE_X: u16 = 90;
    pub const ROOT_BONE_Y: u16 = 91;
    pub const TENDON_BONE_ID: u16 = 95;
    pub const TENDON_TX: u16 = 100;
    pub const TENDON_TY: u16 = 101;
    pub const WEIGHT_VALUES: u16 = 102;
    pub const WEIGHT_INDICES: u16 = 103;
    pub const SKIN_TX: u16 = 108;
    pub const SKIN_TY: u16 = 109;
    pub const MESH_U: u16 = 215;
    pub const MESH_V: u16 = 216;
    pub const TRIANGLE_INDEX_BYTES: u16 = 223;
    pub const SCALE_Y: u16 = 17;
    pub const STRENGTH: u16 = 172;
    pub const TARGET_ID: u16 = 173;
    pub const INVERT_DIRECTION: u16 = 174;
    pub const PARENT_BONE_COUNT: u16 = 175;
    pub const DISTANCE: u16 = 177;
    pub const DISTANCE_MODE: u16 = 178;
    pub const SOURCE_SPACE: u16 = 179;
    pub const DEST_SPACE: u16 = 180;
    pub const COPY_FACTOR: u16 = 182;
    pub const CONSTRAINT_OFFSET: u16 = 188;
    pub const DOES_COPY: u16 = 189;
    pub const DOES_COPY_Y: u16 = 192;
    pub const PATH_DISTANCE: u16 = 363;
    pub const ORIENT: u16 = 364;
    pub const DRAWABLE_ID: u16 = 119;
    pub const PLACEMENT: u16 = 120;
    pub const DRAW_TARGET_ID: u16 = 121;
    pub const RANGE_UNITS: u16 = 316;
    pub const FALLOFF_FROM: u16 = 317;
    pub const FALLOFF_TO: u16 = 318;
    pub const RANGE_OFFSET: u16 = 319;
    pub const MODIFIER_X: u16 = 322;
    pub const MODIFIER_Y: u16 = 323;
    pub const MODIFIER_OPACITY: u16 = 324;
    pub const RANGE_TYPE: u16 = 325;
    pub const RANGE_MODE: u16 = 326;
    pub const MODIFY_FROM: u16 = 327;
    pub const MODIFIER_ORIGIN_X: u16 = 328;
    pub const MODIFIER_ORIGIN_Y: u16 = 329;
    pub const MODIFIER_SCALE_X: u16 = 330;
    pub const MODIFIER_SCALE_Y: u16 = 331;
    pub const MODIFIER_ROTATION: u16 = 332;
    pub const RANGE_STRENGTH: u16 = 334;
    pub const MODIFIER_FLAGS: u16 = 335;
    pub const MODIFY_TO: u16 = 336;
    pub const INTERPOLATOR_X1: u16 = 337;
    pub const INTERPOLATOR_Y1: u16 = 338;
    pub const INTERPOLATOR_X2: u16 = 339;
    pub const INTERPOLATOR_Y2: u16 = 340;
    pub const INPUT_NAME: u16 = 138;
    pub const NUMBER_VALUE: u16 = 140;
    pub const BOOL_VALUE: u16 = 141;
    pub const TRANSITION_FLAGS: u16 = 152;
    pub const CONDITION_INPUT: u16 = 155;
    pub const CONDITION_OP: u16 = 156;
    pub const CONDITION_VALUE: u16 = 157;
    pub const TRANSITION_DURATION: u16 = 158;
    pub const EXIT_TIME: u16 = 160;
    pub const NESTED_INPUT: u16 = 237;
    pub const NESTED_BOOL_VALUE: u16 = 238;
    pub const BOOL_KEY_VALUE: u16 = 181;
    pub const LISTENER_TARGET: u16 = 224;
    pub const LISTENER_TYPE: u16 = 225;
    pub const ALIGN_TARGET: u16 = 240;
    pub const ALIGN_PRESERVE_OFFSET: u16 = 541;
    pub const JOYSTICK_X_ID: u16 = 301;
    pub const JOYSTICK_POS_X: u16 = 303;
    pub const JOYSTICK_POS_Y: u16 = 304;
    pub const JOYSTICK_WIDTH: u16 = 305;
    pub const JOYSTICK_HEIGHT: u16 = 306;
    pub const JOYSTICK_HANDLE: u16 = 313;
    pub const BLEND_ANIMATION_ID: u16 = 165;
    pub const BLEND_VALUE: u16 = 166;
    pub const BLEND_INPUT: u16 = 167;
    pub const DIRECT_INPUT: u16 = 168;
    pub const MIX_VALUE: u16 = 297;
    pub const BLEND_SOURCE: u16 = 298;
    pub const STATE_FLAGS: u16 = 536;
    pub const VM_NAME: u16 = 557;
    pub const VM_INSTANCE_VM: u16 = 566;
    pub const VM_VALUE_PROPERTY: u16 = 554;
    pub const VM_COLOR_VALUE: u16 = 555;
    pub const VM_NUMBER_VALUE: u16 = 575;
    pub const ARTBOARD_VIEW_MODEL: u16 = 583;
    pub const BIND_PROPERTY_KEY: u16 = 586;
    pub const BIND_PATH: u16 = 588;
    pub const VM_CONDITION_OP: u16 = 650;
    pub const COMPARATOR_NUMBER: u16 = 652;
    pub const CUSTOM_NUMBER_VALUE: u16 = 243;
    pub const CUSTOM_BOOL_VALUE: u16 = 245;
    pub const CUSTOM_STRING_VALUE: u16 = 246;
    pub const URL: u16 = 248;
    pub const URL_TARGET: u16 = 249;
    pub const FIRE_LISTENER_EVENT: u16 = 389;
    pub const FIRE_EVENT_ID: u16 = 392;
    pub const FIRE_OCCURS: u16 = 393;
    pub const EVENT_TRIGGER: u16 = 395;
    pub const LISTENER_EVENT: u16 = 399;
    pub const ACTION_INPUT: u16 = 227;
    pub const ACTION_BOOL_VALUE: u16 = 228;
}

/// A property's value.
#[derive(Debug, Clone)]
pub enum V {
    U(u64),
    D(f32),
    C(u32),
    B(bool),
    S(&'static str),
    Bytes(Vec<u8>),
}

/// The types the tests write that are an artboard's objects (components,
/// interpolators, and `ForcedEdge`, which the runtime does not know but
/// gives a slot).
const COMPONENTS: &[u16] = &[
    ty::NODE,
    ty::JOYSTICK,
    ty::EVENT,
    ty::OPEN_URL_EVENT,
    ty::CUSTOM_NUMBER,
    ty::CUSTOM_BOOL,
    ty::CUSTOM_STRING,
    ty::SHAPE,
    ty::ELLIPSE,
    ty::STRAIGHT_VERTEX,
    ty::CUBIC_DETACHED_VERTEX,
    ty::RECTANGLE,
    ty::POINTS_PATH,
    ty::RADIAL_GRADIENT,
    ty::SOLID_COLOR,
    ty::GRADIENT_STOP,
    ty::FILL,
    ty::LINEAR_GRADIENT,
    ty::STROKE,
    ty::CLIPPING_SHAPE,
    ty::TRIM_PATH,
    ty::IMAGE,
    ty::SOLO,
    ty::CUBIC_EASE_INTERPOLATOR,
    ty::CUBIC_VALUE_INTERPOLATOR,
    ty::ELASTIC_INTERPOLATOR,
    ty::FORCED_EDGE,
    ty::FEATHER,
    ty::NESTED_ARTBOARD,
    ty::NESTED_STATE_MACHINE,
    ty::NESTED_SIMPLE_ANIMATION,
    ty::NESTED_REMAP_ANIMATION,
    ty::TEXT,
    ty::TEXT_VALUE_RUN,
    ty::TEXT_STYLE_PAINT,
    ty::TEXT_STYLE_AXIS,
    ty::BONE,
    ty::ROOT_BONE,
    ty::SKIN,
    ty::TENDON,
    ty::WEIGHT,
    ty::CUBIC_WEIGHT,
    ty::MESH_VERTEX,
    ty::MESH,
    ty::IK_CONSTRAINT,
    ty::DISTANCE_CONSTRAINT,
    ty::TRANSFORM_CONSTRAINT,
    ty::TRANSLATION_CONSTRAINT,
    ty::SCALE_CONSTRAINT,
    ty::ROTATION_CONSTRAINT,
    ty::FOLLOW_PATH_CONSTRAINT,
    ty::DRAW_TARGET,
    ty::DRAW_RULES,
    ty::TEXT_MODIFIER_RANGE,
    ty::TEXT_MODIFIER_GROUP,
    ty::CUBIC_INTERPOLATOR_COMPONENT,
    ty::NESTED_BOOL,
];

/// A Rive file being written.
#[derive(Debug, Clone)]
pub struct Riv {
    bytes: Vec<u8>,
    /// The objects of the last artboard so far.
    components: u64,
}

fn varuint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

impl Riv {
    /// A file of format 7.0 with an empty table of contents and a
    /// backboard.
    pub fn new() -> Self {
        Riv::with_toc(&[])
    }

    /// A file whose table of contents lists `keys` with their field types
    /// (0 uint, 1 string, 2 double, 3 color).
    pub fn with_toc(keys: &[(u16, u8)]) -> Self {
        let mut bytes = b"RIVE".to_vec();
        varuint(&mut bytes, 7);
        varuint(&mut bytes, 0);
        varuint(&mut bytes, 1);
        for (k, _) in keys {
            varuint(&mut bytes, u64::from(*k));
        }
        varuint(&mut bytes, 0);
        for chunk in keys.chunks(4) {
            let mut word = 0u32;
            for (i, (_, f)) in chunk.iter().enumerate() {
                word |= u32::from(*f & 3) << (i * 2);
            }
            bytes.extend(word.to_le_bytes());
        }
        Riv {
            bytes,
            components: 0,
        }
        .obj(ty::BACKBOARD, &[])
    }

    /// The file with an object of type `ty` and properties `props`.
    pub fn obj(mut self, ty: u16, props: &[(u16, V)]) -> Self {
        if ty == ty::ARTBOARD {
            self.components = 1;
        } else if COMPONENTS.contains(&ty) {
            self.components += 1;
        }
        varuint(&mut self.bytes, u64::from(ty));
        for (k, v) in props {
            varuint(&mut self.bytes, u64::from(*k));
            match v {
                V::U(u) => varuint(&mut self.bytes, *u),
                V::D(d) => self.bytes.extend(d.to_le_bytes()),
                V::C(c) => self.bytes.extend(c.to_le_bytes()),
                V::B(b) => self.bytes.push(u8::from(*b)),
                V::S(s) => {
                    varuint(&mut self.bytes, s.len() as u64);
                    self.bytes.extend(s.as_bytes());
                }
                V::Bytes(b) => {
                    varuint(&mut self.bytes, b.len() as u64);
                    self.bytes.extend(b);
                }
            }
        }
        varuint(&mut self.bytes, 0);
        self
    }

    /// An artboard `w` × `h` with its origin at its top left.
    pub fn artboard(self, w: f32, h: f32) -> Self {
        self.obj(
            ty::ARTBOARD,
            &[
                (key::NAME, V::S("Main")),
                (key::WIDTH, V::D(w)),
                (key::HEIGHT, V::D(h)),
                (key::ORIGIN_X, V::D(0.0)),
                (key::ORIGIN_Y, V::D(0.0)),
            ],
        )
    }

    /// A shape (under `parent`) at (`x`, `y`) holding a `w` × `h`
    /// rectangle filled with `argb`: four objects from the shape's index.
    pub fn rect_shape(self, parent: u64, x: f32, y: f32, w: f32, h: f32, argb: u32) -> Self {
        let shape = self.shape(parent, x, y);
        let index = shape.next_index() - 1;
        shape
            .obj(
                ty::RECTANGLE,
                &[
                    (key::PARENT_ID, V::U(index)),
                    (key::PATH_WIDTH, V::D(w)),
                    (key::PATH_HEIGHT, V::D(h)),
                ],
            )
            .fill(index, argb)
    }

    /// A shape under `parent` at (`x`, `y`).
    pub fn shape(self, parent: u64, x: f32, y: f32) -> Self {
        self.obj(
            ty::SHAPE,
            &[
                (key::PARENT_ID, V::U(parent)),
                (key::X, V::D(x)),
                (key::Y, V::D(y)),
            ],
        )
    }

    /// A fill of `argb` on shape `shape`: two objects.
    pub fn fill(self, shape: u64, argb: u32) -> Self {
        let fill = self.next_index();
        self.obj(ty::FILL, &[(key::PARENT_ID, V::U(shape))]).obj(
            ty::SOLID_COLOR,
            &[(key::PARENT_ID, V::U(fill)), (key::COLOR, V::C(argb))],
        )
    }

    /// The index the next object of the last artboard takes (the
    /// artboard's own is 0).
    pub fn next_index(&self) -> u64 {
        self.components
    }

    /// The bytes.
    pub fn bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}

/// Reads `riv`, panicking on errors. With `DXN_RIVE_DUMP` set to a folder,
/// the file is written there too, named after the test, to be compared with
/// Rive's own runtime.
pub fn scene(riv: &Riv) -> Scene {
    if let Some(dir) = std::env::var_os("DXN_RIVE_DUMP") {
        let name = std::thread::current()
            .name()
            .unwrap_or("riv")
            .replace("::", "-");
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.riv")),
            riv.bytes(),
        )
        .expect("the dump folder is writable");
    }
    dxn_vector_animation_rive::read(&riv.bytes())
        .expect("the file reads")
        .scene
}

/// The reference renderer's color of `scene` at `time` at pixel (`x`, `y`)
/// of a frame of the scene's size on white.
pub fn color_at(scene: &Scene, time: f64, x: u32, y: u32) -> [u8; 3] {
    let pixmap = render(
        &scene.frame_at(time),
        &RenderOptions {
            background: Some(Color::WHITE),
            ..RenderOptions::default()
        },
    )
    .expect("renders");
    let p = pixmap.pixel(x, y).expect("in the frame").demultiply();
    [p.red(), p.green(), p.blue()]
}

/// Whether anything is drawn (not white) in the rectangle from (`x0`, `y0`)
/// to (`x1`, `y1`) of the frame at `time`, rendered once.
pub fn inked(scene: &Scene, time: f64, x0: u32, y0: u32, x1: u32, y1: u32) -> bool {
    let pixmap = render(
        &scene.frame_at(time),
        &RenderOptions {
            background: Some(Color::WHITE),
            ..RenderOptions::default()
        },
    )
    .expect("renders");
    (x0..x1).any(|x| {
        (y0..y1).any(|y| {
            pixmap.pixel(x, y).is_some_and(|p| {
                let p = p.demultiply();
                [p.red(), p.green(), p.blue()] != [255, 255, 255]
            })
        })
    })
}

/// Whether two colors are within `by` in every channel.
pub fn near(a: [u8; 3], b: [u8; 3], by: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= by)
}
