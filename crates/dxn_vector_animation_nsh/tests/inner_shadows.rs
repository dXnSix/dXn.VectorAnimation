//! Inner shadows play on the graph: the content's silhouette in the
//! shadow's color, less the silhouette moved and blurred, which casts from
//! past the view and takes away as it falls (`destination-out`); moving
//! offsets and colors on the graph. They draw what the reference draws.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{Matrix, Scene};
use dxn_vector_animation_nsh::plan::{self, Unit};

fn read(defs: &str, body: &str) -> Scene {
    let svg = format!(
        r##"<svg width="100" height="100" viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
<rect width="100" height="100" fill="#f4f0e8"/>{body}<defs>{defs}</defs></svg>"##
    );
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    import.scene
}

/// Figma's inner shadow: `dy` down, blurred `sigma`, of `alpha` black.
fn figma_inner(dx: f64, dy: f64, sigma: f64, alpha: f64) -> String {
    format!(
        r#"<filter id="f" x="0" y="0" width="100" height="100" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dx="{dx}" dy="{dy}"/>
<feGaussianBlur stdDeviation="{sigma}"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 {alpha} 0"/>
<feBlend mode="normal" in2="shape" result="effect1_innerShadow"/>
</filter>"#
    )
}

const CARD: &str = r##"<g filter="url(#f)"><rect x="15" y="20" width="70" height="55" rx="10" fill="#4080e0"/><circle cx="70" cy="30" r="14" fill="#f0a020"/></g>"##;

fn wides(units: &[Unit]) -> usize {
    units
        .iter()
        .map(|u| match u {
            Unit::Wide { .. } => 1,
            Unit::Layer { units, .. } => wides(units),
            _ => 0,
        })
        .sum()
}

fn assert_native(scene: &Scene) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    assert!(wides(&plan.units) >= 1);
    for t in [0.0, 0.5, 1.0] {
        let share = common::differing_by(
            &common::plan_pixels(scene, t, 100, 100),
            &common::reference_pixels(scene, t, 100, 100),
            24,
        );
        // Figma's silhouettes are hard: at anti-aliased edges any
        // coverage casts in the reference, the coverage here (a ring of a
        // pixel, a white inner shadow over a dark shadow showing most).
        assert!(share <= 0.008, "at {t}: {:.2}% differ", share * 100.0);
    }
}

#[test]
fn figmas_inner_shadow_falls_inside() {
    assert_native(&read(&figma_inner(0.0, 6.0, 3.0, 0.4), CARD));
    assert_native(&read(&figma_inner(-4.0, -2.0, 0.0, 0.6), CARD));
}

#[test]
fn an_inner_shadow_with_a_drop_shadow_and_moving() {
    // A drop shadow under, an inner shadow over; then the group moving.
    let defs = r#"<filter id="f" x="0" y="0" width="100" height="100" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="4"/><feGaussianBlur stdDeviation="2"/><feComposite in2="hardAlpha" operator="out"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.3 0"/>
<feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow"/>
<feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow" result="shape"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dx="3" dy="3"/><feGaussianBlur stdDeviation="2"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.5 0"/>
<feBlend mode="normal" in2="shape" result="effect2_innerShadow"/>
</filter>"#;
    assert_native(&read(defs, CARD));
    let moving = format!(
        r#"<g><animateTransform attributeName="transform" type="translate" from="-10 0" to="10 5" dur="1s" fill="freeze"/>{CARD}</g>"#
    );
    assert_native(&read(&figma_inner(2.0, 5.0, 2.0, 0.5), &moving));
}

#[test]
fn an_inner_shadow_of_a_turning_group_is_drawn_as_the_reference_draws_it() {
    let turning = format!(
        r#"<g><animateTransform attributeName="transform" type="rotate" from="0 50 50" to="40 50 50" dur="1s" fill="freeze"/>{CARD}</g>"#
    );
    let scene = read(&figma_inner(0.0, 6.0, 3.0, 0.4), &turning);
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert_eq!(
        plan.sampled,
        vec!["an inner shadow of a group that turns or scales".to_owned()]
    );
}
