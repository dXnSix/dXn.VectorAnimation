//! Color effects as the reference draws them: each pixel's straight color
//! through the steps, in sRGB or in linear light; transparent pixels stay
//! so.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    BlendMode, Color, ColorMatrix, Frame, FrameEffect, FramePaint, Item, Layer, Matrix, Path,
    Point, filter::ColorStep,
};
use dxn_vector_animation_render::{RenderOptions, render};

/// A layer with `effects` holding a square of `color` from 30 to 70.
fn frame(effects: Vec<FrameEffect>, color: Color) -> Frame {
    let mut p = Path::new();
    p.move_to(Point::new(30.0, 30.0));
    p.line_to(Point::new(70.0, 30.0));
    p.line_to(Point::new(70.0, 70.0));
    p.line_to(Point::new(30.0, 70.0));
    p.close();
    Frame {
        width: 100.0,
        height: 100.0,
        items: vec![
            Item::PushLayer(Layer {
                opacity: 1.0,
                blend: BlendMode::Normal,
                clip: None,
                mask: None,
                effects,
            }),
            Item::Fill {
                path: p,
                transform: Matrix::IDENTITY,
                paint: FramePaint::Solid(color),
                rule: Default::default(),
            },
            Item::PopLayer,
        ],
    }
}

fn rgba(p: &tiny_skia::Pixmap, x: u32, y: u32) -> [u8; 4] {
    let c = p.pixel(x, y).unwrap().demultiply();
    [c.red(), c.green(), c.blue(), c.alpha()]
}

fn color(steps: Vec<ColorStep>, linear: bool) -> FrameEffect {
    FrameEffect::Color { steps, linear }
}

#[test]
fn grayscale_turns_a_color_into_its_luminance() {
    let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
    let p = render(
        &frame(
            vec![color(
                vec![ColorStep::Matrix(ColorMatrix::grayscale(1.0))],
                false,
            )],
            red,
        ),
        &RenderOptions::default(),
    )
    .unwrap();
    // 0.2126 of white.
    assert_eq!(rgba(&p, 50, 50), [54, 54, 54, 255]);
    // Transparent pixels stay so.
    assert_eq!(rgba(&p, 10, 10)[3], 0);
}

#[test]
fn half_transparent_colors_change_as_straight_colors() {
    let p = render(
        &frame(
            vec![color(
                vec![ColorStep::Matrix(ColorMatrix::invert(1.0))],
                false,
            )],
            Color::rgba(1.0, 0.25, 0.0, 0.5),
        ),
        &RenderOptions::default(),
    )
    .unwrap();
    let [r, g, b, a] = rgba(&p, 50, 50);
    assert_eq!(a, 128);
    assert!(
        r <= 1 && (190..=193).contains(&g) && b >= 254,
        "{r} {g} {b}"
    );
}

#[test]
fn luminance_to_alpha_makes_black_of_the_luminance() {
    let p = render(
        &frame(
            vec![color(
                vec![ColorStep::Matrix(ColorMatrix::luminance_to_alpha())],
                false,
            )],
            Color::WHITE,
        ),
        &RenderOptions::default(),
    )
    .unwrap();
    let [r, g, b, a] = rgba(&p, 50, 50);
    assert_eq!((r, g, b), (0, 0, 0));
    assert!(a >= 254, "{a}");
}

#[test]
fn in_linear_light_a_half_brightness_is_lighter_in_srgb() {
    let gray = Color::rgba(0.8, 0.8, 0.8, 1.0);
    let half = || vec![ColorStep::Matrix(ColorMatrix::brightness(0.5))];
    let srgb = render(
        &frame(vec![color(half(), false)], gray),
        &RenderOptions::default(),
    )
    .unwrap();
    let linear = render(
        &frame(vec![color(half(), true)], gray),
        &RenderOptions::default(),
    )
    .unwrap();
    assert_eq!(rgba(&srgb, 50, 50)[0], 102);
    // 0.8 is 0.604 in linear light, half of it 0.302, which is 0.59 in sRGB.
    assert!(
        (149..=152).contains(&rgba(&linear, 50, 50)[0]),
        "{:?}",
        rgba(&linear, 50, 50)
    );
}
