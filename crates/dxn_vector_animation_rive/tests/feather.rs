//! Feathered fills and strokes: blurred by half their strength, offset in
//! the artboard's or the shape's space, inner feathers inside the outline,
//! and feathered fills drawn only under the clockwise rule.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, key, near, scene, ty};

const BLACK: u32 = 0xFF00_0000;
const WHITE: [u8; 3] = [255, 255, 255];

/// A 100 × 100 artboard with a square of `size` at (`x`, `y`), turned by
/// `rotation`, filled black (fill rule `rule`) and feathered as `feather`
/// says.
fn feathered(size: f32, x: f32, y: f32, rotation: f32, rule: u64, feather: &[(u16, V)]) -> Riv {
    let riv = Riv::new().artboard(100.0, 100.0).obj(
        ty::SHAPE,
        &[
            (key::PARENT_ID, V::U(0)),
            (key::X, V::D(x)),
            (key::Y, V::D(y)),
            (key::ROTATION, V::D(rotation)),
        ],
    );
    let riv = riv.obj(
        ty::RECTANGLE,
        &[
            (key::PARENT_ID, V::U(1)),
            (key::PATH_WIDTH, V::D(size)),
            (key::PATH_HEIGHT, V::D(size)),
        ],
    );
    let mut props = vec![(key::PARENT_ID, V::U(3))];
    props.extend(feather.iter().cloned());
    riv.obj(
        ty::FILL,
        &[(key::PARENT_ID, V::U(1)), (key::FILL_RULE, V::U(rule))],
    )
    .obj(
        ty::SOLID_COLOR,
        &[(key::PARENT_ID, V::U(3)), (key::COLOR, V::C(BLACK))],
    )
    .obj(ty::FEATHER, &props)
}

fn gray(c: [u8; 3]) -> f64 {
    f64::from(c[0]) / 255.0
}

#[test]
fn a_feather_blurs_the_edge_by_half_its_strength() {
    // Strength 20: a deviation of 10. The edge (x 30) is half covered,
    // one deviation in a sixth, one out five sixths.
    let s = scene(&feathered(
        40.0,
        50.0,
        50.0,
        0.0,
        2,
        &[(key::FEATHER_STRENGTH, V::D(20.0))],
    ));
    let at = |x| gray(color_at(&s, 0.0, x, 50));
    assert!((at(30) - 0.5).abs() < 0.06, "{}", at(30));
    assert!((at(40) - 0.16).abs() < 0.06, "{}", at(40));
    assert!((at(20) - 0.84).abs() < 0.06, "{}", at(20));
    assert!(at(50) < 0.1, "the middle stays dark: {}", at(50));
}

#[test]
fn an_offset_in_the_artboards_space_ignores_the_shapes_turn() {
    // A square turned a quarter, its feather offset 30 right in the
    // artboard's space (the default): the blur lies right of it.
    let s = scene(&feathered(
        20.0,
        30.0,
        50.0,
        std::f32::consts::FRAC_PI_2,
        2,
        &[
            (key::FEATHER_STRENGTH, V::D(2.0)),
            (key::FEATHER_OFFSET_X, V::D(30.0)),
        ],
    ));
    assert!(gray(color_at(&s, 0.0, 60, 50)) < 0.2);
    assert_eq!(color_at(&s, 0.0, 30, 50), WHITE);
}

#[test]
fn an_offset_in_the_shapes_space_turns_with_it() {
    // The same in the shape's space (1): turned a quarter clockwise, its x
    // points down.
    let s = scene(&feathered(
        20.0,
        30.0,
        30.0,
        std::f32::consts::FRAC_PI_2,
        2,
        &[
            (key::FEATHER_STRENGTH, V::D(2.0)),
            (key::FEATHER_OFFSET_X, V::D(30.0)),
            (key::FEATHER_SPACE, V::U(1)),
        ],
    ));
    assert!(gray(color_at(&s, 0.0, 30, 60)) < 0.2);
    assert_eq!(color_at(&s, 0.0, 60, 30), WHITE);
}

#[test]
fn an_inner_feather_shades_the_inside_from_its_edge() {
    let s = scene(&feathered(
        60.0,
        50.0,
        50.0,
        0.0,
        2,
        &[
            (key::FEATHER_STRENGTH, V::D(10.0)),
            (key::FEATHER_INNER, V::B(true)),
        ],
    ));
    // A deviation of 5: 2 inside the edge, a third of the way dark (what
    // lies outside the outline, blurred in), light in the middle, nothing
    // outside. Rive's runtime draws the same.
    let edge = gray(color_at(&s, 0.0, 22, 50));
    assert!((edge - 0.66).abs() < 0.06, "{edge}");
    assert!(gray(color_at(&s, 0.0, 50, 50)) > 0.95);
    assert_eq!(color_at(&s, 0.0, 15, 50), WHITE);
}

#[test]
fn an_inner_feather_offset_in_the_artboards_space_ignores_the_shapes_turn() {
    // A square turned a quarter, its inner shade offset 20 right in the
    // artboard's space: the shade lies along its left side, not its top.
    let s = scene(&feathered(
        60.0,
        50.0,
        50.0,
        std::f32::consts::FRAC_PI_2,
        2,
        &[
            (key::FEATHER_STRENGTH, V::D(4.0)),
            (key::FEATHER_INNER, V::B(true)),
            (key::FEATHER_OFFSET_X, V::D(20.0)),
        ],
    ));
    assert!(gray(color_at(&s, 0.0, 28, 50)) < 0.3);
    assert!(gray(color_at(&s, 0.0, 50, 28)) > 0.8);
}

#[test]
fn an_inner_feather_shades_nothing_where_outlines_overlap() {
    // A 20 × 20 rectangle inside the 60 × 60 one, turning the same way: Rive
    // leaves the inside of both unshaded.
    let riv = feathered(
        60.0,
        50.0,
        50.0,
        0.0,
        2,
        &[
            (key::FEATHER_STRENGTH, V::D(4.0)),
            (key::FEATHER_INNER, V::B(true)),
        ],
    )
    .obj(
        ty::RECTANGLE,
        &[
            (key::PARENT_ID, V::U(1)),
            (key::PATH_WIDTH, V::D(20.0)),
            (key::PATH_HEIGHT, V::D(20.0)),
        ],
    );
    let s = scene(&riv);
    assert!(gray(color_at(&s, 0.0, 50, 50)) > 0.95);
    assert!(gray(color_at(&s, 0.0, 21, 50)) < 0.9);
}

#[test]
fn feathered_fills_under_other_rules_are_not_drawn() {
    let s = scene(&feathered(
        40.0,
        50.0,
        50.0,
        0.0,
        0,
        &[(key::FEATHER_STRENGTH, V::D(4.0))],
    ));
    assert_eq!(color_at(&s, 0.0, 50, 50), WHITE);
}

#[test]
fn feathered_strokes_are_blurred() {
    let riv = Riv::new()
        .artboard(100.0, 100.0)
        .shape(0, 50.0, 50.0)
        .obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::PATH_WIDTH, V::D(60.0)),
                (key::PATH_HEIGHT, V::D(60.0)),
            ],
        )
        .obj(
            ty::STROKE,
            &[(key::PARENT_ID, V::U(1)), (key::THICKNESS, V::D(4.0))],
        )
        .obj(
            ty::SOLID_COLOR,
            &[(key::PARENT_ID, V::U(3)), (key::COLOR, V::C(BLACK))],
        )
        .obj(
            ty::FEATHER,
            &[
                (key::PARENT_ID, V::U(3)),
                (key::FEATHER_STRENGTH, V::D(8.0)),
            ],
        );
    let s = scene(&riv);
    // The line (x 20) is gray, not black, and its blur reaches 6 away.
    let line = gray(color_at(&s, 0.0, 20, 50));
    assert!(line > 0.3 && line < 0.8, "{line}");
    assert!(gray(color_at(&s, 0.0, 26, 50)) < 0.97);
    assert!(near(color_at(&s, 0.0, 50, 50), WHITE, 1));
}
