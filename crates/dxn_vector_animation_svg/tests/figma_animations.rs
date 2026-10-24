//! CSS animations as Figma Motion writes them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{FramePaint, Group, Item, LoopMode, Node, Point};

const FIGMA: &str = include_str!("../../../fixtures/figma-motion.svg");

fn group<'a>(root: &'a Group, id: &str) -> &'a Group {
    root.children
        .iter()
        .find_map(|n| match n {
            Node::Group(g) if g.id.as_deref() == Some(id) => Some(g),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no group {id}"))
}

#[test]
fn the_timeline_loops_over_the_animations() {
    let import = dxn_vector_animation_svg::read(FIGMA.as_bytes()).unwrap();
    assert_eq!(import.scene.timeline.duration, 2.0);
    assert_eq!(import.scene.timeline.loop_mode, LoopMode::Loop);
    assert_eq!((import.scene.width, import.scene.height), (200.0, 100.0));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
}

#[test]
fn opacity_follows_each_stops_timing_function() {
    let scene = dxn_vector_animation_svg::read(FIGMA.as_bytes())
        .unwrap()
        .scene;
    let button = group(&scene.root, "Button");
    // cubic-bezier(0.5, 0, 0.5, 1) is symmetric: halfway in time, halfway in value.
    assert!((button.opacity.sample(0.5) - 0.5).abs() < 1e-6);
    assert_eq!(button.opacity.sample(1.5), 1.0);
}

#[test]
fn the_animated_transform_replaces_the_attribute() {
    let scene = dxn_vector_animation_svg::read(FIGMA.as_bytes())
        .unwrap()
        .scene;
    let frame = scene.frame_at(1.0);
    let transform = frame
        .items
        .iter()
        .find_map(|i| match i {
            Item::Fill {
                transform,
                paint: FramePaint::Solid(c),
                ..
            } if c.r == 1.0 => Some(*transform),
            _ => None,
        })
        .unwrap();
    // translate(20, 30) translate(10, 5) scale(1.07) translate(-10, -5).
    let origin = transform.apply(Point::ZERO);
    assert!(
        (origin.x - 19.3).abs() < 1e-6 && (origin.y - 29.65).abs() < 1e-6,
        "{origin:?}"
    );
    assert!((transform.a - 1.07).abs() < 1e-9);
}

#[test]
fn only_the_scales_animate() {
    let scene = dxn_vector_animation_svg::read(FIGMA.as_bytes())
        .unwrap()
        .scene;
    let button = group(&scene.root, "Button");
    let animated = button
        .transform
        .iter()
        .filter(|op| op.is_animated())
        .count();
    assert_eq!(animated, 2, "{:?}", button.transform);
}

#[test]
fn motion_follows_the_offset_path() {
    let scene = dxn_vector_animation_svg::read(FIGMA.as_bytes())
        .unwrap()
        .scene;
    let frame = scene.frame_at(1.0);
    let transform = frame
        .items
        .iter()
        .find_map(|i| match i {
            Item::Fill {
                transform,
                paint: FramePaint::Solid(c),
                ..
            } if c.b == 1.0 && c.r == 0.0 => Some(*transform),
            _ => None,
        })
        .unwrap();
    // The curve is symmetric about its middle, (100, 50).
    let p = transform.apply(Point::ZERO);
    assert!(
        (p.x - 100.0).abs() < 0.01 && (p.y - 50.0).abs() < 0.01,
        "{p:?}"
    );
    assert_eq!(transform.a, 1.0, "offset-rotate: 0deg keeps it upright");
}

#[test]
fn css_positions_replace_the_transform_attribute() {
    // As Figma Motion writes a moving element: its position both as the
    // attribute and as CSS `translate` with a motion path.
    let svg = r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
      <style>#t { translate: 10px 20px; offset-path: path('M0 0 L50 0'); offset-rotate: 0deg; animation: kf 1s linear infinite; }
      @keyframes kf { 0% { offset-distance: 0%; } 100% { offset-distance: 100%; } }</style>
      <rect id="t" transform="translate(10 20)" width="5" height="5" fill="red"/>
    </svg>"#;
    let scene = dxn_vector_animation_svg::read(svg.as_bytes())
        .unwrap()
        .scene;
    let frame = scene.frame_at(0.5);
    let Item::Fill { transform, .. } = &frame.items[0] else {
        panic!()
    };
    assert_eq!(transform.apply(Point::ZERO), Point::new(35.0, 20.0));
}

#[test]
fn visibility_steps_as_css_interpolates_it() {
    let svg = r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg">
      <style>#v { animation: kf 1s linear infinite; }
      @keyframes kf { 0% { visibility: hidden; } 50% { visibility: visible; } 75% { visibility: hidden; } 100% { visibility: hidden; } }</style>
      <rect id="v" width="5" height="5" fill="red"/>
    </svg>"#;
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let shown = |t: f64| !import.scene.frame_at(t).items.is_empty();
    assert!(!shown(0.0));
    assert!(
        shown(0.1),
        "visible from the start of a segment ending visible"
    );
    assert!(shown(0.7), "and until the end of one starting visible");
    assert!(!shown(0.8));
}
