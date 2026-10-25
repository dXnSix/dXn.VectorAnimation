//! Shadows of what the canvas cannot cast from operations (content that
//! casts shadows itself, clipped or masked content, content on several
//! canvases) are cast from an image of the content, where it holds still
//! within its group: the group may move, turn and fade, and the shadow may
//! move, on the graph. They draw what the reference draws.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{Matrix, Scene};
use dxn_vector_animation_nsh::plan::{self, Op, Unit};

fn read(body: &str) -> Scene {
    let svg = format!(
        r##"<svg width="100" height="100" viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
<rect width="100" height="100" fill="#f4f0e8"/>{body}</svg>"##
    );
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    import.scene
}

/// Whether a plan casts from an image (a shadow's layer holding one).
fn images(units: &[Unit]) -> usize {
    units
        .iter()
        .map(|u| match u {
            Unit::Canvas(ops) | Unit::Wide { ops, .. } | Unit::Area { ops, .. } => {
                let shadowed = ops.iter().any(|op| matches!(op, Op::Shadow { .. }));
                usize::from(shadowed && ops.iter().any(|op| matches!(op, Op::Image(..))))
            }
            Unit::Layer { units, .. } => images(units),
            _ => 0,
        })
        .sum()
}

fn assert_native(scene: &Scene, within: f64) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    assert!(images(&plan.units) >= 1, "cast from an image");
    for t in [0.0, 0.5, 1.0] {
        let share = common::differing_by(
            &common::plan_pixels(scene, t, 100, 100),
            &common::reference_pixels(scene, t, 100, 100),
            24,
        );
        assert!(share <= within, "at {t}: {:.2}% differ", share * 100.0);
    }
}

const SHADOW: &str = r##"<filter id="s" x="-50" y="-50" width="200" height="200" filterUnits="userSpaceOnUse"><feDropShadow dx="4" dy="6" stdDeviation="3" flood-color="#000" flood-opacity="0.45"/></filter>"##;
const INNER: &str = r##"<filter id="i" x="-50" y="-50" width="200" height="200" filterUnits="userSpaceOnUse"><feDropShadow dx="-3" dy="2" stdDeviation="1.5" flood-color="#c03020" flood-opacity="0.6"/></filter>"##;

#[test]
fn a_shadow_of_content_casting_shadows() {
    let scene = read(&format!(
        r##"<defs>{SHADOW}{INNER}</defs>
<g filter="url(#s)"><rect x="15" y="15" width="40" height="40" fill="#4080e0"/>
<g filter="url(#i)"><circle cx="62" cy="58" r="20" fill="#f0c040"/></g></g>"##
    ));
    assert_native(&scene, 0.006);
}

#[test]
fn a_shadow_of_clipped_and_masked_content() {
    let scene = read(&format!(
        r##"<defs>{SHADOW}<clipPath id="c"><circle cx="45" cy="45" r="28"/></clipPath>
<mask id="m"><rect x="10" y="10" width="80" height="40" fill="#fff"/></mask></defs>
<g filter="url(#s)"><g clip-path="url(#c)"><rect x="10" y="10" width="60" height="60" fill="#4080e0"/></g>
<g mask="url(#m)"><rect x="55" y="30" width="30" height="50" fill="#20a060"/></g></g>"##
    ));
    assert_native(&scene, 0.006);
}

#[test]
fn a_moving_turning_fading_group_of_still_content() {
    let scene = read(&format!(
        r##"<defs>{SHADOW}<clipPath id="c"><circle cx="45" cy="45" r="25"/></clipPath></defs>
<g><animateTransform attributeName="transform" type="rotate" from="0 50 50" to="30 50 50" dur="1s" fill="freeze"/>
<animate attributeName="opacity" from="1" to="0.6" dur="1s" fill="freeze"/>
<g filter="url(#s)"><g clip-path="url(#c)"><rect x="15" y="15" width="60" height="60" fill="#4080e0"/></g>
<g opacity="0.5"><rect x="40" y="40" width="40" height="30" fill="#e04020"/><rect x="50" y="50" width="30" height="30" fill="#2040a0"/></g></g></g>"##
    ));
    assert_native(&scene, 0.01);
}

#[test]
fn a_shadow_of_moving_content_the_canvas_cannot_cast_is_drawn_as_the_reference_draws_it() {
    let scene = read(&format!(
        r##"<defs>{SHADOW}<clipPath id="c"><circle cx="45" cy="45" r="28"/></clipPath></defs>
<g filter="url(#s)"><g clip-path="url(#c)"><rect x="10" y="10" width="60" height="60" fill="#4080e0">
<animate attributeName="x" from="10" to="30" dur="1s"/></rect></g></g>"##
    ));
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert_eq!(plan.sampled, vec!["a shadow of clipped content".to_owned()]);
}
