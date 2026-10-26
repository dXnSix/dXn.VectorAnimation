//! SMIL animation of `display`, which LottieFiles' SVGs write (discrete,
//! `inline` and `none`) to show a layer for the frames it is in.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Item, Scene};

fn read(svg: &str) -> (Scene, Vec<String>) {
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    (import.scene, import.warnings)
}

/// The fills drawn at `t` that no layer of opacity 0 holds.
fn fills(scene: &Scene, t: f64) -> usize {
    let mut hidden = Vec::new();
    let mut shown = 0;
    for item in scene.frame_at(t).items {
        match item {
            Item::PushLayer(l) => hidden.push(l.opacity <= 0.0),
            Item::PopLayer => {
                hidden.pop();
            }
            Item::Fill { .. } if !hidden.contains(&true) => shown += 1,
            _ => {}
        }
    }
    shown
}

#[test]
fn a_layer_shown_then_hidden() {
    let (scene, warnings) = read(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
  <g><animate attributeName="display" calcMode="discrete" values="inline; none; none; inline" keyTimes="0; 0.5; 0.75; 1" dur="4s" repeatCount="indefinite"/>
    <rect width="10" height="10"/></g>
</svg>"#,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(fills(&scene, 1.0), 1, "shown");
    assert_eq!(fills(&scene, 2.5), 0, "hidden from half time");
    assert_eq!(fills(&scene, 3.9), 0, "still hidden");
}

#[test]
fn shown_by_a_display_animation_from_hidden() {
    let (scene, warnings) = read(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
  <g display="none"><animate attributeName="display" calcMode="discrete" values="none; inline" keyTimes="0; 0.5" dur="2s" repeatCount="indefinite"/>
    <rect width="10" height="10"/></g>
</svg>"#,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(fills(&scene, 0.5), 0);
    assert_eq!(fills(&scene, 1.5), 1);
}
