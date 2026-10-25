//! Inner shadows as the reference draws them: the silhouette less itself
//! moved and blurred, colored and laid over the drawing, inside it only.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    BlendMode, Color, Frame, FrameEffect, FramePaint, Item, Layer, Matrix, Path, Point,
};
use dxn_vector_animation_render::{RenderOptions, render};

/// A layer with `effects` holding a red square from 30 to 70.
fn frame(effects: Vec<FrameEffect>) -> Frame {
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
                paint: FramePaint::Solid(Color::rgba(1.0, 0.0, 0.0, 1.0)),
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

fn shadow(dx: f64, dy: f64, sigma: f64) -> FrameEffect {
    FrameEffect::InnerShadow {
        color: Color::rgba(0.0, 0.0, 1.0, 1.0),
        offset: Point::new(dx, dy),
        sigma,
        hard: true,
    }
}

#[test]
fn an_inner_shadow_falls_inside_from_the_edge_it_moves_away_from() {
    // Moved down by 6: the band along the top edge is in shadow.
    let p = render(
        &frame(vec![shadow(0.0, 6.0, 0.0)]),
        &RenderOptions::default(),
    )
    .unwrap();
    assert_eq!(rgba(&p, 50, 32), [0, 0, 255, 255], "in the top band");
    assert_eq!(rgba(&p, 50, 40), [255, 0, 0, 255], "below the band");
    assert_eq!(rgba(&p, 50, 68), [255, 0, 0, 255], "at the bottom");
    // Nothing outside the drawing.
    assert_eq!(rgba(&p, 50, 25)[3], 0);
    assert_eq!(rgba(&p, 50, 75)[3], 0);
}

#[test]
fn a_blurred_inner_shadow_fades_away_from_the_edge() {
    let p = render(
        &frame(vec![shadow(0.0, 0.0, 4.0)]),
        &RenderOptions::default(),
    )
    .unwrap();
    // Unmoved, the shadow is half at the edge and fades inwards.
    let blue = |y: u32| f64::from(rgba(&p, 50, y)[2]) / 255.0;
    assert!(blue(30) > 0.4 && blue(30) < 0.6, "{}", blue(30));
    assert!(blue(33) < blue(31));
    assert!(blue(42) < 0.01, "{}", blue(42));
    assert_eq!(rgba(&p, 50, 26)[3], 0);
}

#[test]
fn inner_and_drop_shadows_are_cast_by_the_drawing_before_them() {
    // A drop shadow first: the inner shadow is still cast by the square
    // alone, inside it.
    let drop = FrameEffect::DropShadow {
        color: Color::rgba(0.0, 0.0, 0.0, 1.0),
        offset: Point::new(0.0, 10.0),
        sigma: 0.0,
        knockout: false,
        hard: false,
    };
    let p = render(
        &frame(vec![drop, shadow(0.0, 6.0, 0.0)]),
        &RenderOptions::default(),
    )
    .unwrap();
    assert_eq!(rgba(&p, 50, 32), [0, 0, 255, 255]);
    assert_eq!(rgba(&p, 50, 75), [0, 0, 0, 255], "the drop shadow below");
    assert_eq!(rgba(&p, 50, 68), [255, 0, 0, 255]);
}
