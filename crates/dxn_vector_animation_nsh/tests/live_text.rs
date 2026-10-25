//! Live text plays on the graph: each line one `fillText` (and
//! `strokeText`) the platform sets, placed, sized and colored by the graph;
//! lines of several styles and spaced letters are sampled. They draw what
//! the reference draws (both set in the system's fonts here).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{Matrix, Scene};
use dxn_vector_animation_nsh::plan::{self, Op, Unit};

fn read(body: &str) -> Scene {
    let svg = format!(
        r#"<svg width="200" height="100" viewBox="0 0 200 100" xmlns="http://www.w3.org/2000/svg">{body}</svg>"#
    );
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    import.scene
}

/// The text operations of a plan's canvases.
fn texts(units: &[Unit]) -> Vec<Op> {
    units
        .iter()
        .flat_map(|u| match u {
            Unit::Canvas(ops) => ops
                .iter()
                .filter(|op| matches!(op, Op::Text { .. }))
                .cloned()
                .collect(),
            Unit::Layer { units, .. } => texts(units),
            _ => Vec::new(),
        })
        .collect()
}

fn assert_native(scene: &Scene, ops: usize) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    assert_eq!(texts(&plan.units).len(), ops);
    if !dxn_vector_animation_render::fonts_available() {
        return;
    }
    for t in [0.0, 0.5, 1.0] {
        let share = common::differing_by(
            &common::plan_pixels(scene, t, 200, 100),
            &common::reference_pixels(scene, t, 200, 100),
            48,
        );
        assert!(share <= 0.002, "at {t}: {:.2}% differ", share * 100.0);
    }
}

#[test]
fn a_line_of_text_is_set_by_the_platform() {
    let scene = read(
        r##"<text x="10" y="50" font-size="24" font-family="sans-serif" fill="#c02020">Hello, text</text>"##,
    );
    assert_native(&scene, 1);
}

#[test]
fn text_moves_grows_and_changes_color_on_the_graph() {
    let scene = read(
        r##"<style>
  #t { animation: k 1s linear both; transform-origin: 100px 50px; }
  @keyframes k { from { transform: translateX(-20px); fill: #2040c0; } to { transform: translateX(20px); fill: #c04020; } }
</style>
<text id="t" x="100" y="60" font-size="20" text-anchor="middle">Centered<animate attributeName="font-size" from="12" to="28" dur="1s" fill="freeze"/></text>"##,
    );
    assert_native(&scene, 1);
}

#[test]
fn stroked_and_filled_lines_in_their_order() {
    let scene = read(
        r##"<text x="10" y="40" font-size="28" font-weight="bold" fill="#ffd040" stroke="#202020" stroke-width="2" paint-order="stroke">Outline</text>
<text x="190" y="85" font-size="16" text-anchor="end" fill="#208040">Second line</text>"##,
    );
    assert_native(&scene, 3);
}

#[test]
fn lines_in_several_styles_or_spaced_are_sampled() {
    let scene = read(
        r#"<text x="10" y="50" font-size="20">Mixed <tspan font-weight="bold">styles</tspan></text>"#,
    );
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert_eq!(
        plan.sampled,
        vec!["a line of text in several styles".to_owned()]
    );
    let scene = read(r#"<text x="10" y="50" letter-spacing="3">spaced</text>"#);
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert_eq!(plan.sampled, vec!["spaced text".to_owned()]);
}
