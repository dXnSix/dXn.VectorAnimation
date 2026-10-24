//! Rendered pixels of the fixtures.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::Color;
use dxn_vector_animation_render::{RenderOptions, render, size};

fn rgba(p: &tiny_skia::Pixmap, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let c = p.pixel(x, y).unwrap().demultiply();
    (c.red(), c.green(), c.blue(), c.alpha())
}

#[test]
fn figma_svg_frames() {
    let scene =
        dxn_vector_animation_svg::read(include_bytes!("../../../fixtures/figma-motion.svg"))
            .unwrap()
            .scene;
    let at_one = render(&scene.frame_at(1.0), &RenderOptions::default()).unwrap();
    assert_eq!((at_one.width(), at_one.height()), (200, 100));
    // The scaled-up button is opaque red at t = 1.
    assert_eq!(rgba(&at_one, 30, 35), (255, 0, 0, 255));
    // The gradient square shows only inside its circular mask.
    let inside = rgba(&at_one, 170, 30).3;
    assert!(
        (185..=197).contains(&inside),
        "{inside}: the gradient is 0.75 opaque halfway"
    );
    assert_eq!(rgba(&at_one, 151, 11).3, 0);
    // At t = 0 the button is scaled to nothing and transparent.
    let at_zero = render(&scene.frame_at(0.0), &RenderOptions::default()).unwrap();
    assert_eq!(rgba(&at_zero, 30, 35).3, 0);
}

#[test]
fn lottie_frames() {
    let scene =
        dxn_vector_animation_lottie::read(include_bytes!("../../../fixtures/lottie-motion.json"))
            .unwrap()
            .scene;
    let half = render(
        &scene.frame_at(0.5),
        &RenderOptions {
            scale: 2.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!((half.width(), half.height()), (200, 200));
    assert_eq!(rgba(&half, 100, 100), (255, 0, 0, 255));
    assert_eq!(rgba(&half, 20, 100).3, 0, "the box left its start");
}

#[test]
fn backgrounds_and_sizes() {
    let scene =
        dxn_vector_animation_lottie::read(include_bytes!("../../../fixtures/lottie-motion.json"))
            .unwrap()
            .scene;
    let frame = scene.frame_at(0.0);
    let options = RenderOptions {
        width: Some(50),
        background: Some(Color::WHITE),
        ..Default::default()
    };
    assert_eq!(size(&frame, &options), (50, 50));
    let p = render(&frame, &options).unwrap();
    assert_eq!(rgba(&p, 49, 0), (255, 255, 255, 255));
    let png = dxn_vector_animation_render::render_png(&frame, &options).unwrap();
    assert!(png.starts_with(b"\x89PNG"));
}
