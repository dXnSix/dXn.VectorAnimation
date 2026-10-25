//! Color effects play on the graph: each paint inside recolored (colors,
//! gradients' stops, text, PNG images, shadows), moving effects sampled
//! into color tracks, alpha scales fading the group; what changes alpha
//! otherwise, or moves over gradients and images, is drawn as the
//! reference draws it. They draw what the reference draws.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{Matrix, Scene};
use dxn_vector_animation_nsh::plan::{self, Unit};

fn read(style: &str, body: &str) -> Scene {
    let svg = format!(
        r##"<svg width="100" height="100" viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg"><style>{style}</style>
<rect width="100" height="100" fill="#f4f0e8"/>{body}</svg>"##
    );
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    import.scene
}

const SHAPES: &str = r##"<g id="g">
<rect x="10" y="10" width="50" height="50" fill="#e04020"/>
<circle cx="60" cy="60" r="28" fill="#2050d0" fill-opacity="0.7"/>
<rect x="20" y="70" width="60" height="18" fill="url(#lg)"/>
</g>
<defs><linearGradient id="lg"><stop offset="0" stop-color="#10a040"/><stop offset="1" stop-color="#f0c020"/></linearGradient></defs>"##;

fn rasters(units: &[Unit]) -> usize {
    units
        .iter()
        .map(|u| match u {
            Unit::Raster(_) => 1,
            Unit::Layer { units, .. } => rasters(units),
            _ => 0,
        })
        .sum()
}

fn assert_matches(scene: &Scene, within: f64) {
    for t in [0.0, 0.4, 1.0] {
        let share = common::differing_by(
            &common::plan_pixels(scene, t, 100, 100),
            &common::reference_pixels(scene, t, 100, 100),
            24,
        );
        assert!(share <= within, "at {t}: {:.2}% differ", share * 100.0);
    }
}

#[test]
fn still_color_functions_recolor_each_paint() {
    for f in [
        "grayscale(1)",
        "sepia(0.8) saturate(1.5)",
        "hue-rotate(120deg)",
        "invert(0.9) contrast(1.2) brightness(0.8)",
    ] {
        let scene = read(&format!("#g {{ filter: {f}; }}"), SHAPES);
        let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
        assert!(plan.sampled.is_empty(), "{f}: {:?}", plan.sampled);
        // Where the see-through disc overlaps, the blend is changed by
        // the reference, each paint here.
        assert_matches(&scene, 0.03);
    }
}

#[test]
fn a_hue_turning_samples_the_colors() {
    let scene = read(
        "#g { animation: k 1s linear both; } @keyframes k { from { filter: hue-rotate(0deg); } to { filter: hue-rotate(300deg); } }",
        r##"<g id="g"><rect x="10" y="10" width="80" height="80" fill="#e04020"/></g>"##,
    );
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    assert_matches(&scene, 0.002);
}

#[test]
fn an_opacity_function_fades_the_group_as_one() {
    let scene = read("#g { filter: opacity(0.5); }", SHAPES);
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    assert!(plan.units.iter().any(|u| matches!(u, Unit::Layer { .. })));
    assert_matches(&scene, 0.004);
}

#[test]
fn what_changes_alpha_or_moves_over_a_gradient_is_drawn_as_the_reference_draws_it() {
    let scene = read(
        "",
        &format!(
            r#"<filter id="l"><feColorMatrix type="luminanceToAlpha"/></filter>{}"#,
            SHAPES.replace(r#"<g id="g">"#, r#"<g id="g" filter="url(#l)">"#)
        ),
    );
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert_eq!(plan.sampled, vec!["a color change of alpha".to_owned()]);
    assert_eq!(rasters(&plan.units), 1);
    let scene = read(
        "#g { animation: k 1s linear both; } @keyframes k { from { filter: sepia(0); } to { filter: sepia(1); } }",
        SHAPES,
    );
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert_eq!(
        plan.sampled,
        vec!["a color change moving over a gradient".to_owned()]
    );
    assert_matches(&scene, 0.004);
}

#[test]
fn lotties_tint_recolors_on_the_graph() {
    let json = r#"{"v":"5.7.0","fr":30,"ip":0,"op":30,"w":100,"h":100,"layers":[
{"ty":4,"ind":1,"ip":0,"op":30,"st":0,
 "ks":{"o":{"a":0,"k":100},"p":{"a":0,"k":[0,0]},"a":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0}},
 "ef":[{"ty":20,"nm":"Tint","ef":[{"ty":2,"v":{"a":0,"k":[0.1,0,0.3,1]}},{"ty":2,"v":{"a":0,"k":[1,0.9,0.6,1]}},{"ty":0,"v":{"a":1,"k":[{"t":0,"s":[0],"o":{"x":[0],"y":[0]},"i":{"x":[1],"y":[1]}},{"t":30,"s":[100]}]}}]}],
 "shapes":[{"ty":"gr","it":[{"ty":"rc","p":{"a":0,"k":[50,50]},"s":{"a":0,"k":[70,70]},"r":{"a":0,"k":8}},
 {"ty":"fl","c":{"a":0,"k":[0.2,0.6,0.9,1]},"o":{"a":0,"k":100}},
 {"ty":"tr","p":{"a":0,"k":[0,0]},"a":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}]}]}]}"#;
    let scene = dxn_vector_animation_lottie::read(json.as_bytes())
        .unwrap()
        .scene;
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    assert_matches(&scene, 0.003);
}
