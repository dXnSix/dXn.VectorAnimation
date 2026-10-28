//! Groups that only move (and fade) are layers the platform moves: their
//! still content drawn once, in their own lengths scaled up to stay sharp,
//! on canvases whose operations hold no animated value; groups that only
//! fade are layers it fades. A still group inside one whose content
//! moves is a moving layer of its own, moved as the groups it is in move.
//! Groups whose content moves, or that need the view's coordinates (inside
//! a clip, casting a shadow), are drawn as before. All draw what the
//! reference draws.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{Matrix, Scene};
use dxn_vector_animation_nsh::{
    graph::Num,
    plan::{self, Op, Unit},
};

fn read(body: &str) -> Scene {
    let svg = format!(
        r##"<svg width="100" height="100" viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
<rect width="100" height="100" fill="#f4f0e8"/>{body}</svg>"##
    );
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    import.scene
}

/// The moving layers of a plan, and whether every canvas inside them is
/// still (its operations hold no animated value).
fn moving_layers(units: &[Unit]) -> (usize, bool) {
    let animated = |ops: &[Op]| {
        ops.iter().any(|op| {
            let n = |n: &Num| matches!(n, Num::Node(_));
            match op {
                Op::Translate(a, b) | Op::Scale(a, b) | Op::MoveTo(a, b) | Op::LineTo(a, b) => {
                    n(a) || n(b)
                }
                Op::Rotate(a) | Op::LineWidth(a) | Op::GlobalAlpha(a) => n(a),
                Op::Transform(m) => m.iter().any(n),
                Op::CubicTo(v) => v.iter().any(n),
                Op::QuadTo(v) => v.iter().any(n),
                Op::FillColor(dxn_vector_animation_nsh::graph::Paint::Node(_)) => true,
                _ => false,
            }
        })
    };
    fn still(units: &[Unit], animated: &dyn Fn(&[Op]) -> bool) -> bool {
        units.iter().all(|u| match u {
            Unit::Canvas(ops) | Unit::Area { ops, .. } | Unit::Wide { ops, .. } => !animated(ops),
            Unit::Layer { units, .. } => still(units, animated),
            _ => false,
        })
    }
    units.iter().fold((0, true), |(n, ok), u| match u {
        Unit::Layer {
            units,
            transform: Some(_),
            ..
        } => (n + 1, ok && still(units, &animated)),
        Unit::Layer { units, .. } => {
            let (m, inner) = moving_layers(units);
            (n + m, ok && inner)
        }
        _ => (n, ok),
    })
}

fn assert_matches(scene: &Scene, within: f64) {
    for t in [0.0, 0.3, 0.7, 1.0] {
        let share = common::differing_by(
            &common::plan_pixels(scene, t, 100, 100),
            &common::reference_pixels(scene, t, 100, 100),
            24,
        );
        assert!(share <= within, "at {t}: {:.2}% differ", share * 100.0);
    }
}

const CONTENT: &str = r##"<rect x="-20" y="-15" width="40" height="30" rx="6" fill="#3070e0"/>
<circle cx="12" cy="8" r="10" fill="url(#g)"/><path d="M-15 10 L0 -10 L15 10" fill="none" stroke="#202020" stroke-width="2"/>
<defs><linearGradient id="g"><stop offset="0" stop-color="#f0c020"/><stop offset="1" stop-color="#e04020"/></linearGradient></defs>"##;

#[test]
fn a_group_moving_turning_and_growing_is_a_moving_layer() {
    let scene = read(&format!(
        r#"<g><animateTransform attributeName="transform" type="translate" values="30 40; 70 60; 50 50" dur="1s" fill="freeze"/>
<g><animateTransform attributeName="transform" type="rotate" from="0" to="120" dur="1s" fill="freeze"/>
<g><animateTransform attributeName="transform" type="scale" values="0.5; 1.3; 1" dur="1s" fill="freeze"/>{CONTENT}</g></g></g>"#
    ));
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    let (layers, still) = moving_layers(&plan.units);
    assert_eq!(
        layers, 1,
        "one moving layer: the outermost group that moves"
    );
    assert!(still, "its canvases draw once");
    // Its canvas drawn once and turned as a whole (resampled, as the
    // platforms composite a layer): edges a little softer.
    assert_matches(&scene, 0.015);
}

#[test]
fn a_group_moving_and_fading_fades_its_layer() {
    let scene = read(&format!(
        r#"<g><animateTransform attributeName="transform" type="translate" from="20 50" to="80 50" dur="1s" fill="freeze"/>
<animate attributeName="opacity" values="1; 0.3; 1" dur="1s" fill="freeze"/>{CONTENT}</g>"#
    ));
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    let (layers, still) = moving_layers(&plan.units);
    assert_eq!((layers, still), (1, true));
    assert_matches(&scene, 0.01);
}

#[test]
fn groups_whose_content_moves_or_that_need_the_view_are_drawn_as_before() {
    // Its content moves.
    let scene = read(
        r##"<g><animateTransform attributeName="transform" type="translate" from="20 50" to="80 50" dur="1s"/>
<rect width="20" height="20" fill="#3070e0"><animate attributeName="width" from="10" to="30" dur="1s"/></rect></g>"##,
    );
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert_eq!(moving_layers(&plan.units).0, 0);
    // Inside a clip.
    let scene = read(&format!(
        r##"<defs><clipPath id="c"><rect x="10" y="10" width="60" height="60"/></clipPath></defs>
<g clip-path="url(#c)"><g><animateTransform attributeName="transform" type="translate" from="20 50" to="80 50" dur="1s"/>{CONTENT}</g></g>"##
    ));
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert_eq!(moving_layers(&plan.units).0, 0);
    assert_matches(&scene, 0.004);
}

#[test]
fn a_group_that_only_fades_is_a_layer_it_fades() {
    let scene = read(
        r##"<g transform="translate(50 50)"><animate attributeName="opacity" values="1; 0.2; 1" dur="1s" fill="freeze"/>
<circle r="20" fill="#3070e0"/></g>"##,
    );
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    let layer = plan.units.iter().find_map(|u| match u {
        Unit::Layer { opacity, units, .. } => Some((*opacity, units.clone())),
        _ => None,
    });
    let (opacity, units) = layer.expect("a layer");
    assert!(matches!(opacity, Num::Node(_)), "its opacity bound");
    assert!(moving_layers(&units).1, "its canvas still");
    assert_matches(&scene, 0.002);
}

/// A group turning about (50, 50) and holding a still group and one that
/// grows (its content moves, so it is no moving layer of its own).
fn turning_with_moving_content(dur: &str) -> Scene {
    read(&format!(
        r##"<g transform="translate(50 50)"><g><animateTransform attributeName="transform" type="rotate" from="0" to="90" dur="{dur}" repeatCount="indefinite"/>
<g transform="translate(0 -25)"><circle r="12" fill="#3070e0"/><rect x="-4" y="-4" width="8" height="8" fill="#f0c020"/></g>
<g transform="translate(0 25)"><rect x="-8" y="-8" width="16" height="16" fill="#e04020"><animate attributeName="width" from="8" to="20" dur="{dur}" repeatCount="indefinite"/></rect></g>
</g></g>"##
    ))
}

#[test]
fn a_still_group_in_a_moving_one_whose_content_moves_is_a_moving_layer() {
    let scene = turning_with_moving_content("1s");
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    let (layers, still) = moving_layers(&plan.units);
    assert_eq!(
        layers, 1,
        "the still group, moved as the group it is in turns"
    );
    assert!(still, "its canvas draws once");
    assert_matches(&scene, 0.015);
}

#[test]
fn over_a_playback_too_long_to_sample_whole_it_is_drawn_as_before() {
    // 20 s: more frames than the matrix is sampled at.
    let scene = turning_with_moving_content("20s");
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert_eq!(moving_layers(&plan.units).0, 0);
}
