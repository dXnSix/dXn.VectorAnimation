//! Text modifiers: groups moving, turning, scaling and fading glyphs as
//! their ranges cover them, as Rive's runtime applies them.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, inked, key, scene, ty};

const FONT: &[u8] = include_bytes!("../../../fixtures/fonts/KaTeX_SansSerif-Regular.ttf");

/// A 200 × 100 artboard with "HHHH" at 40 units in black at (10, 20)
/// (objects 1 to 5), a modifier group (6) with properties `group` and a
/// range (7) with `range` (and a cubic `interpolator`, object 8); animated
/// as `keys` say over a second (object, property, from, to).
fn modified(
    group: &[(u16, V)],
    range: &[(u16, V)],
    interpolator: Option<[f32; 4]>,
    keys: &[(u64, u16, f32, f32)],
) -> Riv {
    let mut group_props = vec![(key::PARENT_ID, V::U(1))];
    group_props.extend(group.iter().cloned());
    let mut range_props = vec![(key::PARENT_ID, V::U(6))];
    range_props.extend(range.iter().cloned());
    let mut riv = Riv::new()
        .obj(ty::FONT_ASSET, &[(key::NAME, V::S("KaTeX"))])
        .obj(
            ty::FILE_ASSET_CONTENTS,
            &[(key::BYTES, V::Bytes(FONT.to_vec()))],
        )
        .artboard(200.0, 100.0)
        .obj(
            ty::TEXT,
            &[
                (key::PARENT_ID, V::U(0)),
                (key::X, V::D(10.0)),
                (key::Y, V::D(20.0)),
            ],
        )
        .obj(
            ty::TEXT_STYLE_PAINT,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::FONT_SIZE, V::D(40.0)),
                (key::FONT_ASSET_ID, V::U(0)),
            ],
        )
        .fill(2, 0xFF00_0000)
        .obj(
            ty::TEXT_VALUE_RUN,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::RUN_STYLE_ID, V::U(2)),
                (key::RUN_TEXT, V::S("HHHH")),
            ],
        )
        .obj(ty::TEXT_MODIFIER_GROUP, &group_props)
        .obj(ty::TEXT_MODIFIER_RANGE, &range_props);
    if let Some([x1, y1, x2, y2]) = interpolator {
        riv = riv.obj(
            ty::CUBIC_INTERPOLATOR_COMPONENT,
            &[
                (key::PARENT_ID, V::U(7)),
                (key::INTERPOLATOR_X1, V::D(x1)),
                (key::INTERPOLATOR_Y1, V::D(y1)),
                (key::INTERPOLATOR_X2, V::D(x2)),
                (key::INTERPOLATOR_Y2, V::D(y2)),
            ],
        );
    }
    if keys.is_empty() {
        return riv;
    }
    riv = riv.obj(
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

const OPACITY: u64 = 1 << 5;
const TRANSLATION: u64 = 1 << 2;
const ROTATION: u64 = 1 << 3;
const SCALE: u64 = 1 << 4;
const ORIGIN: u64 = 1;

#[test]
fn an_opacity_modifier_reveals_letters_as_its_range_grows() {
    // Covered letters show; the range grows from none to all four.
    let riv = modified(
        &[(key::MODIFIER_FLAGS, V::U(OPACITY))],
        &[(key::MODIFY_TO, V::D(0.0))],
        None,
        &[(7, key::MODIFY_TO, 0.0, 1.0)],
    );
    let s = scene(&riv);
    assert!(!inked(&s, 0.0, 0, 0, 200, 100));
    // Half way, the first two.
    assert!(inked(&s, 0.5, 10, 20, 50, 60));
    assert!(!inked(&s, 0.5, 70, 20, 200, 60));
    assert!(inked(&s, 1.0, 100, 20, 200, 60));
}

#[test]
fn a_translation_modifier_moves_what_it_covers() {
    // The second half moved down 40.
    let riv = modified(
        &[
            (key::MODIFIER_FLAGS, V::U(TRANSLATION)),
            (key::MODIFIER_Y, V::D(40.0)),
        ],
        &[(key::MODIFY_FROM, V::D(0.5))],
        None,
        &[],
    );
    let s = scene(&riv);
    assert!(inked(&s, 0.0, 10, 25, 40, 55));
    assert!(!inked(&s, 0.0, 70, 25, 200, 55));
    assert!(inked(&s, 0.0, 70, 65, 200, 95));
}

#[test]
fn falloffs_ease_the_change_across_the_range() {
    // Turned and scaled about an origin, falling off over the whole word
    // with a cubic interpolator, the turn keyed.
    let riv = modified(
        &[
            (key::MODIFIER_FLAGS, V::U(ROTATION | SCALE | ORIGIN)),
            (key::MODIFIER_ROTATION, V::D(0.8)),
            (key::MODIFIER_SCALE_X, V::D(0.5)),
            (key::MODIFIER_SCALE_Y, V::D(1.5)),
            (key::MODIFIER_ORIGIN_Y, V::D(-10.0)),
        ],
        &[(key::FALLOFF_FROM, V::D(0.5)), (key::FALLOFF_TO, V::D(0.5))],
        Some([0.42, 0.0, 0.58, 1.0]),
        &[(6, key::MODIFIER_ROTATION, 0.0, 0.8)],
    );
    let s = scene(&riv);
    assert!(inked(&s, 1.0, 10, 0, 200, 100));
}
