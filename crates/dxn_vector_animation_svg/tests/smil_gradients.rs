//! SMIL animation of gradients, which LottieFiles' SVGs write on the
//! `<stop>`s (`stop-color`) and the gradient's line (`x1`, `y1`, `x2`,
//! `y2`): the elements are in `<defs>`, drawn only through a `fill`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{FramePaint, Item, Point, Scene};

fn read(svg: &str) -> (Scene, Vec<String>) {
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    (import.scene, import.warnings)
}

fn paint(scene: &Scene, t: f64) -> FramePaint {
    scene
        .frame_at(t)
        .items
        .into_iter()
        .find_map(|i| match i {
            Item::Fill { paint, .. } => Some(paint),
            _ => None,
        })
        .unwrap()
}

const STOPS: &str = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="100" y2="0">
      <stop offset="0%" stop-color="#ff0000"><animate attributeName="stop-color" values="#ff0000; #0000ff; #ff0000" keyTimes="0; 0.5; 1" dur="2s" repeatCount="indefinite"/></stop>
      <stop offset="100%" stop-color="#ffffff"/>
    </linearGradient>
  </defs>
  <rect width="100" height="100" fill="url(#g)"/>
</svg>"##;

#[test]
fn stop_color_animates() {
    let (scene, warnings) = read(STOPS);
    assert!(warnings.is_empty(), "{warnings:?}");
    let FramePaint::Linear { stops, .. } = paint(&scene, 1.0) else {
        panic!("a linear gradient");
    };
    assert_eq!(stops.len(), 2);
    assert!(
        stops[0].color.b > 0.99 && stops[0].color.r < 0.01,
        "blue at 1 s"
    );
    assert!(stops[1].color.r > 0.99, "the other stop stays white");
    let FramePaint::Linear { stops, .. } = paint(&scene, 0.5) else {
        panic!("a linear gradient");
    };
    assert!(
        stops[0].color.r > 0.4 && stops[0].color.b > 0.4,
        "halfway to blue"
    );
}

#[test]
fn gradient_line_animates() {
    let (scene, warnings) = read(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="100" y2="0">
      <stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/>
      <animate attributeName="x2" values="100; 50; 100" keyTimes="0; 0.5; 1" dur="2s" repeatCount="indefinite"/>
      <animate attributeName="y1" values="0; 20; 0" keyTimes="0; 0.5; 1" dur="2s" repeatCount="indefinite"/>
    </linearGradient>
  </defs>
  <rect width="100" height="100" fill="url(#g)"/>
</svg>"#,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let FramePaint::Linear { start, end, .. } = paint(&scene, 1.0) else {
        panic!("a linear gradient");
    };
    assert_eq!((start, end), (Point::new(0.0, 20.0), Point::new(50.0, 0.0)));
}

#[test]
fn an_unused_gradient_is_not_a_loss() {
    // Nothing fills with it, so nothing is lost; a drawn one is read.
    let (_, warnings) = read(
        r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg">
  <defs><g id="a"><rect width="5" height="5"><animate attributeName="width" values="5;9;5" dur="1s" repeatCount="indefinite"/></rect></g></defs>
</svg>"#,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}
