//! A region of a frame renders as that part of the whole frame does.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::Rect;
use dxn_vector_animation_render::{RenderOptions, render, render_region};

#[test]
fn a_region_is_the_frames_part() {
    let scene =
        dxn_vector_animation_svg::read(include_bytes!("../../../fixtures/figma-motion.svg"))
            .unwrap()
            .scene;
    let frame = scene.frame_at(1.0);
    let whole = render(
        &frame,
        &RenderOptions {
            scale: 2.0,
            ..Default::default()
        },
    )
    .unwrap();
    let part = render_region(&frame, Rect::new(120.0, 10.0, 60.0, 50.0), 2.0).unwrap();
    assert_eq!((part.width(), part.height()), (120, 100));
    let mut differ = 0;
    for y in 0..100 {
        for x in 0..120 {
            let a = part.pixel(x, y).unwrap();
            let b = whole.pixel(240 + x, 20 + y).unwrap();
            if a.alpha().abs_diff(b.alpha()) > 2 || a.red().abs_diff(b.red()) > 2 {
                differ += 1;
            }
        }
    }
    assert!(differ < 20, "{differ} pixels differ");
}
