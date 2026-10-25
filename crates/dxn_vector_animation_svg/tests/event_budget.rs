//! What begins on an event resolves within a budget of its own: a cycle of
//! syncbase begins, or a timeline past the cap, cannot use up the instances
//! that clicks need.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Item, Scene};

fn read(body: &str) -> (Scene, Vec<String>) {
    let svg =
        format!(r#"<svg width="400" height="100" xmlns="http://www.w3.org/2000/svg">{body}</svg>"#);
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    (import.scene, import.warnings)
}

/// The left edge of the fill whose path is `width` wide, at `t`.
fn x_of(scene: &Scene, width: f64, t: f64) -> f64 {
    scene
        .frame_at(t)
        .items
        .iter()
        .find_map(|i| match i {
            Item::Fill {
                path, transform, ..
            } => {
                let b = path.transformed(*transform).bounds()?;
                ((b.width - width).abs() < 1e-6).then_some(b.x)
            }
            _ => None,
        })
        .unwrap()
}

/// A button that moves 100 to the right in 1 s when clicked.
const BUTTON: &str = r#"<rect id="btn" width="20" height="20"><animate attributeName="x" from="0" to="100" dur="1s" fill="freeze" begin="click"/></rect>"#;

#[test]
fn a_cycle_leaves_clicks_their_instances() {
    // Two animations starting each other every 0.1 s: up to the hour of the
    // timeline that is far more than 4,000 instances.
    let (scene, warnings) = read(&format!(
        r#"<rect width="10" height="10"><animate id="a" attributeName="y" from="0" to="1" dur="0.05s" begin="0s;b.end"/><animate id="b" attributeName="y" from="1" to="0" dur="0.05s" begin="a.end"/></rect>{BUTTON}"#
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(scene.events(), vec!["btn.click"]);
    let clicked = scene.with_events(&[("btn.click", 1.0)]);
    assert!((x_of(&clicked, 20.0, 1.5) - 50.0).abs() < 1e-3);
    assert!((x_of(&clicked, 20.0, 3.0) - 100.0).abs() < 1e-3);
}

#[test]
fn a_timeline_past_the_cap_warns_and_clicks_still_play() {
    let begins: Vec<String> = (0..4500)
        .map(|i| format!("{}s", f64::from(i) * 0.1))
        .collect();
    let (scene, warnings) = read(&format!(
        r#"<rect width="10" height="10"><animate attributeName="y" from="0" to="1" dur="0.05s" begin="{}"/></rect>{BUTTON}"#,
        begins.join(";")
    ));
    assert!(
        warnings
            .iter()
            .any(|w| w
                == "SMIL animations begin more than 4000 times: later beginnings are left out"),
        "{warnings:?}"
    );
    assert_eq!(scene.events(), vec!["btn.click"]);
    let clicked = scene.with_events(&[("btn.click", 1.0)]);
    assert!((x_of(&clicked, 20.0, 1.5) - 50.0).abs() < 1e-3);
}
