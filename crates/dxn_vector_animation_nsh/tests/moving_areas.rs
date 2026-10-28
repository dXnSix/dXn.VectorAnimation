//! Canvases that draw again each frame are as large as where they draw over
//! the playback (at most half the scene): a canvas over that area, fewer
//! pixels drawn each frame. Ones drawing over more, or drawing what is not
//! measured (text, shadows), stay the size of the view. All draw what the
//! reference draws.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{Matrix, Scene};
use dxn_vector_animation_nsh::plan::{self, Op, Unit};

/// `body` alone: what draws on one canvas is all it holds.
fn read(body: &str) -> Scene {
    let svg = format!(
        r##"<svg width="100" height="100" viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">{body}</svg>"##
    );
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    import.scene
}

/// The canvases of `units` that draw again each frame: over an area, or
/// the size of the view.
fn moving(units: &[Unit]) -> (Vec<dxn_vector_animation::Rect>, usize) {
    units
        .iter()
        .fold((Vec::new(), 0), |(mut areas, whole), u| match u {
            Unit::Area { area, ops, .. } if ops.iter().any(Op::moves) => {
                areas.push(*area);
                (areas, whole)
            }
            Unit::Canvas(ops) if ops.iter().any(Op::moves) => (areas, whole + 1),
            Unit::Layer { units, .. } | Unit::Clip { units, .. } => {
                let (a, w) = moving(units);
                areas.extend(a);
                (areas, whole + w)
            }
            _ => (areas, whole),
        })
}

fn assert_matches(scene: &Scene, within: f64) {
    for t in [0.0, 0.25, 0.5, 0.8, 1.0] {
        let share = common::differing_by(
            &common::plan_pixels(scene, t, 100, 100),
            &common::reference_pixels(scene, t, 100, 100),
            24,
        );
        assert!(share <= within, "at {t}: {:.2}% differ", share * 100.0);
    }
}

#[test]
fn a_small_drawing_that_changes_draws_over_where_it_goes() {
    // A bar growing as it moves right: no moving layer (its content
    // changes), a canvas over the band it sweeps.
    let scene = read(
        r##"<rect x="10" y="40" width="10" height="12" rx="3" fill="#3070e0" stroke="#202020" stroke-width="2">
<animate attributeName="width" values="10; 30; 20" dur="1s" fill="freeze"/>
<animate attributeName="x" values="10; 50; 60" dur="1s" fill="freeze"/></rect>"##,
    );
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    let (areas, whole) = moving(&plan.units);
    assert_eq!((areas.len(), whole), (1, 0), "{:?}", plan.units);
    let a = areas[0];
    assert!(
        a.x <= 9.0 && a.x + a.width >= 81.0,
        "the band it sweeps: {a:?}"
    );
    assert!(
        a.y <= 39.0 && a.y + a.height >= 53.0 && a.height < 30.0,
        "{a:?}"
    );
    assert_matches(&scene, 0.002);
}

#[test]
fn a_drawing_over_most_of_the_view_stays_the_size_of_the_view() {
    let scene = read(
        r##"<circle cx="50" cy="50" r="30" fill="#3070e0"><animate attributeName="r" values="30; 48; 30" dur="1s"/></circle>"##,
    );
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert_eq!(moving(&plan.units), (Vec::new(), 1));
}

#[test]
fn text_that_changes_stays_the_size_of_the_view() {
    let scene = read(
        r##"<text x="10" y="50" font-size="10" fill="#202020">Hi<animate attributeName="fill" values="#202020; #e04020" dur="1s"/></text>"##,
    );
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert!(moving(&plan.units).0.is_empty());
}
