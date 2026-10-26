//! Embedded images: their colors as a browser draws them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use dxn_vector_animation::{Group, Image, ImageData, LoopMode, Node, Rect, Scene, Timeline};
use dxn_vector_animation_render::{RenderOptions, render};
use image::{ImageEncoder, codecs::png::PngEncoder};

/// A 1 × 1 PNG of `rgb`, tagged with ICC `profile` when given.
fn png(rgb: [u8; 3], profile: Option<Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = PngEncoder::new(&mut out);
    if let Some(profile) = profile {
        encoder.set_icc_profile(profile).unwrap();
    }
    encoder
        .write_image(&rgb, 1, 1, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// The color the renderer draws image `bytes` in.
fn drawn(bytes: Vec<u8>) -> [u8; 3] {
    let scene = Scene {
        width: 1.0,
        height: 1.0,
        timeline: Timeline {
            duration: 0.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Once,
            markers: Vec::new(),
        },
        root: Group {
            children: vec![Node::Image(Image {
                id: None,
                name: None,
                data: Arc::new(ImageData {
                    mime: "image/png".to_owned(),
                    bytes,
                }),
                rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            })],
            ..Group::new()
        },
    };
    let p = render(&scene.frame_at(0.0), &RenderOptions::default()).unwrap();
    let c = p.pixel(0, 0).unwrap().demultiply();
    [c.red(), c.green(), c.blue()]
}

#[test]
fn untagged_images_are_drawn_as_they_are() {
    assert_eq!(drawn(png([241, 7, 77], None)), [241, 7, 77]);
}

#[test]
fn images_in_another_color_space_are_drawn_in_srgb() {
    // A Display P3 red past sRGB's reach, clipped as a browser clips it.
    let p3 = moxcms::ColorProfile::new_display_p3().encode().unwrap();
    let [r, g, b] = drawn(png([241, 7, 77], Some(p3)));
    assert!(r == 255 && g <= 3 && (68..=76).contains(&b), "{r} {g} {b}");
}
