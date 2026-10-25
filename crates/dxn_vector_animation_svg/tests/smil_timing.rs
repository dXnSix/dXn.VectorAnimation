//! SMIL timing beyond begins: `end` times (offsets and other animations'
//! begins and ends) cutting instances short, begins on another's repeats,
//! and `accumulate="sum"` building each repeat on the last; events never
//! happen here.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Item, Scene};

fn read(svg: &str) -> (Scene, Vec<String>) {
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    (import.scene, import.warnings)
}

/// The first fill's path bounds at `t` (x, y, width).
fn rect_at(scene: &Scene, t: f64) -> (f64, f64, f64) {
    scene
        .frame_at(t)
        .items
        .iter()
        .find_map(|i| match i {
            Item::Fill {
                path, transform, ..
            } => {
                let b = path.transformed(*transform).bounds()?;
                Some((b.x, b.y, b.width))
            }
            _ => None,
        })
        .unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

fn svg(body: &str) -> String {
    format!(r#"<svg width="400" height="100" xmlns="http://www.w3.org/2000/svg">{body}</svg>"#)
}

#[test]
fn an_end_offset_cuts_the_animation_and_freezes_there() {
    let (scene, warnings) = read(&svg(
        r#"<rect width="10" height="10"><animate attributeName="x" from="0" to="100" dur="4s" end="2s" fill="freeze"/></rect>"#,
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(close(rect_at(&scene, 1.0).0, 25.0));
    // Cut at 2 s halfway, and held there.
    assert!(
        close(rect_at(&scene, 3.0).0, 50.0),
        "{:?}",
        rect_at(&scene, 3.0)
    );
    assert!(
        close(scene.timeline.duration, 2.0),
        "{}",
        scene.timeline.duration
    );
}

#[test]
fn an_end_at_another_animations_begin() {
    let (scene, warnings) = read(&svg(r#"<rect width="10" height="10">
  <animate id="a" attributeName="x" from="0" to="100" dur="4s" end="b.begin"/>
  <animate id="b" attributeName="y" from="0" to="10" dur="1s" begin="1s"/>
</rect>"#));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(close(rect_at(&scene, 0.5).0, 12.5));
    // `a` ends when `b` begins: x is back at its base.
    assert!(
        close(rect_at(&scene, 1.5).0, 0.0),
        "{:?}",
        rect_at(&scene, 1.5)
    );
}

#[test]
fn a_begin_on_another_animations_repeat() {
    let (scene, warnings) = read(&svg(r#"<rect width="10" height="10">
  <animate id="a" attributeName="y" from="0" to="10" dur="1s" repeatCount="3"/>
  <animate attributeName="x" from="0" to="100" dur="1s" begin="a.repeat(2)" fill="freeze"/>
</rect>"#));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(close(rect_at(&scene, 1.5).0, 0.0));
    assert!(
        close(rect_at(&scene, 2.5).0, 50.0),
        "{:?}",
        rect_at(&scene, 2.5)
    );
}

#[test]
fn accumulate_builds_each_repeat_on_the_last() {
    let (scene, warnings) = read(&svg(
        r#"<rect width="10" height="10"><animate attributeName="x" from="0" to="20" dur="1s" repeatCount="3" accumulate="sum" fill="freeze"/></rect>"#,
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(close(rect_at(&scene, 0.5).0, 10.0));
    assert!(
        close(rect_at(&scene, 1.5).0, 30.0),
        "{:?}",
        rect_at(&scene, 1.5)
    );
    assert!(close(rect_at(&scene, 2.5).0, 50.0));
    // Frozen at the end of the third.
    assert!(
        close(rect_at(&scene, 5.0).0, 60.0),
        "{:?}",
        rect_at(&scene, 5.0)
    );
}

#[test]
fn accumulated_by_and_transforms() {
    // `by` adds to what is there: each repeat moves by it again.
    let (scene, _) = read(&svg(
        r#"<rect x="5" width="10" height="10"><animate attributeName="x" by="10" dur="1s" repeatCount="3" accumulate="sum"/></rect>"#,
    ));
    assert!(
        close(rect_at(&scene, 1.5).0, 20.0),
        "{:?}",
        rect_at(&scene, 1.5)
    );
    let (scene, _) = read(&svg(
        r#"<rect width="10" height="10"><animateTransform attributeName="transform" type="translate" from="0 0" to="30 5" dur="1s" repeatCount="2" accumulate="sum"/></rect>"#,
    ));
    let (x, y, _) = rect_at(&scene, 1.5);
    assert!(close(x, 45.0) && close(y, 7.5), "{x} {y}");
}

#[test]
fn events_never_happen_here() {
    let (scene, warnings) = read(&svg(
        r#"<rect width="10" height="10"><animate attributeName="x" from="0" to="100" dur="1s" begin="click"/></rect>"#,
    ));
    assert!(close(rect_at(&scene, 0.5).0, 0.0));
    assert!(warnings.iter().any(|w| w.contains("click")), "{warnings:?}");
}
