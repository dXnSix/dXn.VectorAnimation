//! SMIL animation inside `<clipPath>`s and `<mask>`s, which LottieFiles'
//! SVGs use to reveal a layer (`d` of a clip path's shape, `transform` and
//! `width` of what a mask draws).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Item, Scene};

fn read(svg: &str) -> (Scene, Vec<String>) {
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    (import.scene, import.warnings)
}

/// The bounds' width of the first clip shape of the first layer at `t`.
fn clip_width(scene: &Scene, t: f64) -> f64 {
    scene
        .frame_at(t)
        .items
        .iter()
        .find_map(|i| match i {
            Item::PushLayer(l) => l.clip.as_ref().and_then(|c| {
                let b = c[0].path.transformed(c[0].transform).bounds()?;
                Some(b.x + b.width)
            }),
            _ => None,
        })
        .unwrap()
}

fn mask_right_edge(scene: &Scene, t: f64) -> f64 {
    scene
        .frame_at(t)
        .items
        .iter()
        .find_map(|i| match i {
            Item::PushLayer(l) => l.mask.as_ref().and_then(|m| {
                m.items.iter().find_map(|i| match i {
                    Item::Fill {
                        path, transform, ..
                    } => {
                        let b = path.transformed(*transform).bounds()?;
                        Some(b.x + b.width)
                    }
                    _ => None,
                })
            }),
            _ => None,
        })
        .unwrap()
}

#[test]
fn clip_path_morphs() {
    let (scene, warnings) = read(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
  <defs><clipPath id="c"><path d="M0 0L10 0L10 10Z"><animate attributeName="d" values="M0 0L10 0L10 10Z; M0 0L50 0L50 50Z; M0 0L10 0L10 10Z" keyTimes="0; 0.5; 1" dur="2s" repeatCount="indefinite"/></path></clipPath></defs>
  <rect width="100" height="100" fill="red" clip-path="url(#c)"/>
</svg>"#,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!((clip_width(&scene, 0.0) - 10.0).abs() < 1e-3);
    assert!((clip_width(&scene, 1.0) - 50.0).abs() < 1e-3);
}

#[test]
fn clip_path_shape_moves() {
    let (scene, warnings) = read(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
  <defs><clipPath id="c"><rect width="10" height="10"><animateTransform attributeName="transform" type="translate" values="0 0; 40 0; 0 0" keyTimes="0; 0.5; 1" dur="2s" repeatCount="indefinite"/></rect></clipPath></defs>
  <rect width="100" height="100" fill="red" clip-path="url(#c)"/>
</svg>"#,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!((clip_width(&scene, 0.0) - 10.0).abs() < 1e-3);
    assert!((clip_width(&scene, 1.0) - 50.0).abs() < 1e-3);
}

#[test]
fn mask_content_animates() {
    let (scene, warnings) = read(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
  <defs><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100"><rect x="0" width="10" height="100" fill="white"><animate attributeName="width" values="10; 60; 10" keyTimes="0; 0.5; 1" dur="2s" repeatCount="indefinite"/></rect></mask></defs>
  <rect width="100" height="100" fill="red" mask="url(#m)"/>
</svg>"#,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!((mask_right_edge(&scene, 0.0) - 10.0).abs() < 1e-3);
    assert!((mask_right_edge(&scene, 1.0) - 60.0).abs() < 1e-3);
}

#[test]
fn a_hidden_subtree_loses_nothing() {
    // `display="none"` for good: what animates inside it is never seen.
    let (_, warnings) = read(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
  <g display="none"><rect width="10" height="10"><animate attributeName="width" values="10; 60; 10" dur="2s" repeatCount="indefinite"/></rect></g>
</svg>"#,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}
