//! Layers: transforms, easing, parenting, holds, time, markers.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{FramePaint, Item, LoopMode, Point, outline::path_length};

const MOTION: &[u8] = include_bytes!("../../../fixtures/lottie-motion.json");

fn fill_origin(items: &[Item], red: bool) -> Point {
    items
        .iter()
        .find_map(|i| match i {
            Item::Fill {
                transform,
                paint: FramePaint::Solid(c),
                ..
            } if (c.r == 1.0) == red => Some(transform.apply(Point::ZERO)),
            _ => None,
        })
        .unwrap()
}

#[test]
fn reads_the_composition() {
    let import = dxn_vector_animation_lottie::read(MOTION).unwrap();
    let t = &import.scene.timeline;
    assert_eq!(
        (t.duration, t.frame_rate, t.loop_mode),
        (2.0, Some(30.0), LoopMode::Loop)
    );
    assert_eq!(t.markers[0].name, "half");
    assert_eq!((t.markers[0].start, t.markers[0].duration), (1.0, 0.5));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
}

#[test]
fn position_eases_between_keyframes() {
    let scene = dxn_vector_animation_lottie::read(MOTION).unwrap().scene;
    // cubic-bezier(0.33, 0, 0.67, 1) is symmetric: halfway at half time.
    let p = fill_origin(&scene.frame_at(0.5).items, true);
    assert!(
        (p.x - 50.0).abs() < 1e-6 && (p.y - 50.0).abs() < 1e-6,
        "{p:?}"
    );
    let early = fill_origin(&scene.frame_at(0.1).items, true);
    assert!(
        early.x < 20.0 + 60.0 * (0.1 / 1.0),
        "eased in: slower than linear"
    );
}

#[test]
fn styles_listed_first_draw_on_top() {
    let scene = dxn_vector_animation_lottie::read(MOTION).unwrap().scene;
    let items = scene.frame_at(0.0).items;
    let fill = items
        .iter()
        .position(|i| matches!(i, Item::Fill { .. }))
        .unwrap();
    let stroke = items
        .iter()
        .position(|i| matches!(i, Item::Stroke { paint: FramePaint::Solid(c), .. } if c.b == 0.0))
        .unwrap();
    assert!(
        fill < stroke,
        "the stroke is listed before the fill, so it is on top"
    );
}

#[test]
fn parents_move_their_children_and_holds_jump() {
    let scene = dxn_vector_animation_lottie::read(MOTION).unwrap().scene;
    let items = scene.frame_at(1.5).items;
    let (transform, path) = items
        .iter()
        .find_map(|i| match i {
            Item::Stroke {
                transform,
                path,
                paint: FramePaint::Solid(c),
                ..
            } if c.b == 1.0 => Some((*transform, path.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(transform.apply(Point::ZERO), Point::new(10.0, 10.0));
    // Trimmed to three quarters (linear easing from 0 to 100 over 2 s).
    let full = 2.0 * std::f64::consts::PI * 10.0;
    assert!((path_length(&path) - 0.75 * full).abs() < 0.1);
    let layer = |t: f64| {
        scene.frame_at(t).items.iter().find_map(|i| match i {
            Item::PushLayer(l) => Some(l.opacity),
            _ => None,
        })
    };
    assert_eq!(layer(0.9), None, "held at 100 until the next keyframe");
    assert_eq!(layer(1.0), Some(0.4));
}
