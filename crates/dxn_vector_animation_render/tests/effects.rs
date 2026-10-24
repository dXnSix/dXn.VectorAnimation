//! Drop shadows and blurs as the reference draws them: a Gaussian blur
//! (three boxes along each axis), a shadow of the silhouette under the
//! drawing, moved and colored.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    BlendMode, Color, Frame, FrameEffect, FramePaint, Item, Layer, Matrix, Path, Point,
};
use dxn_vector_animation_render::{RenderOptions, render};

/// A frame of a layer with `effects` holding a white-on-red square from
/// 40 to 60.
fn frame(effects: Vec<FrameEffect>, color: Color) -> Frame {
    let mut p = Path::new();
    p.move_to(Point::new(40.0, 40.0));
    p.line_to(Point::new(60.0, 40.0));
    p.line_to(Point::new(60.0, 60.0));
    p.line_to(Point::new(40.0, 60.0));
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

fn alpha(p: &tiny_skia::Pixmap, x: u32, y: u32) -> f64 {
    f64::from(p.pixel(x, y).unwrap().alpha()) / 255.0
}

/// The normal distribution's CDF, for edges blurred by a Gaussian.
fn cdf(x: f64, sigma: f64) -> f64 {
    // Abramowitz and Stegun 7.1.26.
    let z = x / (sigma * std::f64::consts::SQRT_2);
    let t = 1.0 / (1.0 + 0.3275911 * z.abs());
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
            + 0.254829592)
            * t
            * (-z * z).exp();
    0.5 * (1.0 + y.copysign(z))
}

#[test]
fn a_blur_spreads_an_edge_as_a_gaussian_does() {
    let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
    for sigma in [1.0, 4.0] {
        let p = render(
            &frame(vec![FrameEffect::Blur { sigma }], red),
            &RenderOptions::default(),
        )
        .unwrap();
        // Along the middle row, across the left edge at x = 40.
        for x in 30..50 {
            let want = cdf(f64::from(x) + 0.5 - 40.0, sigma);
            let got = alpha(&p, x, 50);
            assert!(
                (got - want).abs() < 0.04,
                "sigma {sigma}, x {x}: {got} against {want}"
            );
        }
    }
}

#[test]
fn a_blur_keeps_what_is_drawn() {
    let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
    let p = render(
        &frame(vec![FrameEffect::Blur { sigma: 3.0 }], red),
        &RenderOptions::default(),
    )
    .unwrap();
    let total: f64 = p
        .pixels()
        .iter()
        .map(|c| f64::from(c.alpha()) / 255.0)
        .sum();
    assert!((total - 400.0).abs() < 4.0, "{total}");
    // Still red throughout.
    let c = p.pixel(38, 50).unwrap().demultiply();
    assert!(c.red() > 250 && c.green() < 5, "{c:?}");
}

#[test]
fn a_shadow_falls_under_the_drawing() {
    let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
    let shadow = FrameEffect::DropShadow {
        color: Color::rgba(0.0, 0.0, 1.0, 0.5),
        offset: Point::new(10.0, 20.0),
        sigma: 0.0,
        knockout: false,
        hard: false,
    };
    let p = render(&frame(vec![shadow], red), &RenderOptions::default()).unwrap();
    // The drawing on top, its shadow beside it, half opaque blue.
    let top = p.pixel(45, 45).unwrap().demultiply();
    assert_eq!((top.red(), top.blue(), top.alpha()), (255, 0, 255));
    let s = p.pixel(65, 75).unwrap().demultiply();
    assert!(s.blue() > 250 && (126..=129).contains(&s.alpha()), "{s:?}");
    assert_eq!(p.pixel(35, 35).unwrap().alpha(), 0);
}

#[test]
fn a_knocked_out_shadow_hides_under_a_see_through_drawing() {
    let clear_red = Color::rgba(1.0, 0.0, 0.0, 0.5);
    let shadow = |knockout| FrameEffect::DropShadow {
        color: Color::rgba(0.0, 0.0, 0.0, 1.0),
        offset: Point::new(5.0, 5.0),
        sigma: 0.0,
        knockout,
        hard: false,
    };
    let behind = render(
        &frame(vec![shadow(false)], clear_red),
        &RenderOptions::default(),
    )
    .unwrap();
    let hidden = render(
        &frame(vec![shadow(true)], clear_red),
        &RenderOptions::default(),
    )
    .unwrap();
    // Where the drawing and its shadow (as see-through as it) overlap.
    assert!((alpha(&behind, 55, 55) - 0.75).abs() < 0.01);
    assert!((alpha(&hidden, 55, 55) - 0.5).abs() < 0.01);
    // Beyond the drawing both shadows show.
    assert!((alpha(&hidden, 62, 62) - 0.5).abs() < 0.01);
}

#[test]
fn shadows_one_after_another_are_cast_by_the_drawing() {
    let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
    let shadow = |dx: f64| FrameEffect::DropShadow {
        color: Color::rgba(0.0, 0.0, 0.0, 0.5),
        offset: Point::new(dx, 0.0),
        sigma: 0.0,
        knockout: false,
        hard: false,
    };
    let p = render(
        &frame(vec![shadow(10.0), shadow(20.0)], red),
        &RenderOptions::default(),
    )
    .unwrap();
    // The second shadow is the drawing's, not the first shadow's: nothing
    // falls 30 to the right.
    assert!((alpha(&p, 65, 50) - 0.75).abs() < 0.01);
    assert!((alpha(&p, 75, 50) - 0.5).abs() < 0.01);
    assert_eq!(alpha(&p, 85, 50), 0.0);
}

#[test]
fn a_hard_shadow_is_cast_in_full_by_a_see_through_drawing() {
    // A drawing a tenth opaque: its silhouette all or nothing, as Figma's.
    let faint = Color::rgba(1.0, 0.0, 0.0, 0.1);
    let shadow = |hard| FrameEffect::DropShadow {
        color: Color::rgba(0.0, 0.0, 0.0, 0.5),
        offset: Point::new(10.0, 0.0),
        sigma: 0.0,
        knockout: true,
        hard,
    };
    let soft = render(
        &frame(vec![shadow(false)], faint),
        &RenderOptions::default(),
    )
    .unwrap();
    let hard = render(&frame(vec![shadow(true)], faint), &RenderOptions::default()).unwrap();
    // Beside the drawing: a twentieth, or the shadow's full half.
    assert!((alpha(&soft, 65, 50) - 0.05).abs() < 0.01);
    assert!((alpha(&hard, 65, 50) - 0.5).abs() < 0.01);
    // Under it, knocked out either way.
    assert!((alpha(&hard, 55, 50) - 0.1).abs() < 0.01);
}
