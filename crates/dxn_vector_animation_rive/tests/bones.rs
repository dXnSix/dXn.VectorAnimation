//! Bones: shapes that ride on them, outlines and image meshes their skins
//! deform.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use std::f32::consts::FRAC_PI_2;

use common::{Riv, V, color_at, key, near, scene, ty};
use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Fill, Geometry, Group, LoopMode, Node, Paint, Scene, Shape,
    ShapePath, Timeline,
};
use dxn_vector_animation_render::{RenderOptions, render_png};

const RED: u32 = 0xFFFF_0000;
const WHITE: [u8; 3] = [255, 255, 255];
const BLUE: [u8; 3] = [0, 0, 255];

/// A second-long animation (10 frames at 10 fps) keying property `prop` of
/// `object` from `from` to `to`, linearly.
fn animate(riv: Riv, object: u64, prop: u16, from: f32, to: f32) -> Riv {
    let riv = riv
        .obj(
            ty::LINEAR_ANIMATION,
            &[
                (key::ANIMATION_NAME, V::S("move")),
                (key::FPS, V::U(10)),
                (key::DURATION, V::U(10)),
            ],
        )
        .obj(ty::KEYED_OBJECT, &[(key::OBJECT_ID, V::U(object))])
        .obj(
            ty::KEYED_PROPERTY,
            &[(key::PROPERTY_KEY, V::U(u64::from(prop)))],
        );
    [(0, from), (10, to)]
        .iter()
        .fold(riv, |riv, (frame, value)| {
            riv.obj(
                ty::KEY_FRAME_DOUBLE,
                &[
                    (key::FRAME, V::U(*frame)),
                    (key::VALUE, V::D(*value)),
                    (key::INTERPOLATION, V::U(1)),
                ],
            )
        })
}

/// A root bone under the artboard at (`x`, `y`), `length` long.
fn root_bone(riv: Riv, x: f32, y: f32, length: f32) -> Riv {
    riv.obj(
        ty::ROOT_BONE,
        &[
            (key::PARENT_ID, V::U(0)),
            (key::ROOT_BONE_X, V::D(x)),
            (key::ROOT_BONE_Y, V::D(y)),
            (key::BONE_LENGTH, V::D(length)),
        ],
    )
}

/// A weight binding its vertex `parent` wholly to the skin's bone `bone`
/// (1 the first tendon's).
fn weight(riv: Riv, parent: u64, bone: u64) -> Riv {
    riv.obj(
        ty::WEIGHT,
        &[
            (key::PARENT_ID, V::U(parent)),
            (key::WEIGHT_VALUES, V::U(255)),
            (key::WEIGHT_INDICES, V::U(bone)),
        ],
    )
}

#[test]
fn shapes_ride_on_bones() {
    // A root bone at (20, 25), 30 long, and a bone at its tip holding a red
    // square; the root turns a quarter.
    let riv = root_bone(Riv::new().artboard(100.0, 100.0), 20.0, 25.0, 30.0)
        .obj(
            ty::BONE,
            &[(key::PARENT_ID, V::U(1)), (key::BONE_LENGTH, V::D(20.0))],
        )
        .rect_shape(2, 0.0, 0.0, 10.0, 10.0, RED);
    let s = scene(&animate(riv, 1, key::ROTATION, 0.0, FRAC_PI_2));
    let c = color_at(&s, 0.0, 50, 25);
    assert!(near(c, [255, 0, 0], 2), "{c:?}");
    assert_eq!(color_at(&s, 1.0, 50, 25), WHITE);
    let c = color_at(&s, 1.0, 20, 55);
    assert!(near(c, [255, 0, 0], 2), "{c:?}");
}

#[test]
fn skinned_outlines_follow_their_bones() {
    // A red square from (40, 40) to (60, 60): its left side on a root bone
    // at (30, 50), its right side on the bone at its tip (50, 50), which
    // turns a quarter and folds the right side down onto the bottom.
    let mut riv = root_bone(Riv::new().artboard(100.0, 100.0), 30.0, 50.0, 20.0)
        .obj(ty::BONE, &[(key::PARENT_ID, V::U(1))])
        .shape(0, 0.0, 0.0)
        .obj(
            ty::POINTS_PATH,
            &[(key::PARENT_ID, V::U(3)), (key::IS_CLOSED, V::B(true))],
        );
    for (x, y, bone) in [
        (40.0, 40.0, 1),
        (60.0, 40.0, 2),
        (60.0, 60.0, 2),
        (40.0, 60.0, 1),
    ] {
        let vertex = riv.next_index();
        riv = riv.obj(
            ty::STRAIGHT_VERTEX,
            &[
                (key::PARENT_ID, V::U(4)),
                (key::VERTEX_X, V::D(x)),
                (key::VERTEX_Y, V::D(y)),
            ],
        );
        riv = weight(riv, vertex, bone);
    }
    let skin = riv.next_index();
    let riv = riv
        .obj(ty::SKIN, &[(key::PARENT_ID, V::U(4))])
        .obj(
            ty::TENDON,
            &[
                (key::PARENT_ID, V::U(skin)),
                (key::TENDON_BONE_ID, V::U(1)),
                (key::TENDON_TX, V::D(30.0)),
                (key::TENDON_TY, V::D(50.0)),
            ],
        )
        .obj(
            ty::TENDON,
            &[
                (key::PARENT_ID, V::U(skin)),
                (key::TENDON_BONE_ID, V::U(2)),
                (key::TENDON_TX, V::D(50.0)),
                (key::TENDON_TY, V::D(50.0)),
            ],
        )
        .fill(3, RED);
    let s = scene(&animate(riv, 2, key::ROTATION, 0.0, FRAC_PI_2));
    let c = color_at(&s, 0.0, 55, 44);
    assert!(near(c, [255, 0, 0], 2), "{c:?}");
    // At the end, the triangle (40, 40), (60, 60), (40, 60).
    assert_eq!(color_at(&s, 1.0, 55, 44), WHITE);
    let c = color_at(&s, 1.0, 44, 55);
    assert!(near(c, [255, 0, 0], 2), "{c:?}");
}

/// A 10 × 10 blue PNG.
fn blue_png() -> Vec<u8> {
    render_png(
        &Scene {
            width: 10.0,
            height: 10.0,
            timeline: Timeline {
                duration: 0.0,
                frame_rate: None,
                loop_start: 0.0,
                loop_mode: LoopMode::Once,
                markers: Vec::new(),
                cues: Vec::new(),
            },
            root: Group {
                children: vec![Node::shape(Shape {
                    paths: vec![ShapePath::new(Geometry::Rect {
                        center: AnimatedPoint::fixed(5.0, 5.0),
                        size: AnimatedPoint::fixed(10.0, 10.0),
                        radius: AnimatedPoint::fixed(0.0, 0.0),
                        direction: Default::default(),
                        start: Default::default(),
                    })],
                    fill: Some(Fill {
                        paint: Paint::solid(Color::rgba(0.0, 0.0, 1.0, 1.0)),
                        opacity: Animated::Static(1.0),
                        rule: Default::default(),
                    }),
                    ..Default::default()
                })],
                ..Group::new()
            },
        }
        .frame_at(0.0),
        &RenderOptions::default(),
    )
    .unwrap()
}

/// The image's mesh under `image`: its four corners, weighted to `bone`
/// when given; returns the file and the index of the third corner.
fn mesh(riv: Riv, image: u64, bone: Option<u64>) -> (Riv, u64) {
    let m = riv.next_index();
    let mut riv = riv.obj(
        ty::MESH,
        &[
            (key::PARENT_ID, V::U(image)),
            (key::TRIANGLE_INDEX_BYTES, V::Bytes(vec![0, 1, 2, 0, 2, 3])),
        ],
    );
    let mut third = 0;
    for (k, (x, y)) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
        .into_iter()
        .enumerate()
    {
        let vertex = riv.next_index();
        if k == 2 {
            third = vertex;
        }
        riv = riv.obj(
            ty::MESH_VERTEX,
            &[
                (key::PARENT_ID, V::U(m)),
                (key::VERTEX_X, V::D(x)),
                (key::VERTEX_Y, V::D(y)),
                (key::MESH_U, V::D(x / 10.0)),
                (key::MESH_V, V::D(y / 10.0)),
            ],
        );
        if let Some(bone) = bone {
            riv = weight(riv, vertex, bone);
        }
    }
    (riv, third)
}

/// The blue image with an asset, in a 60 × 60 artboard.
fn with_image() -> Riv {
    Riv::new()
        .obj(ty::IMAGE_ASSET, &[(key::NAME, V::S("blue"))])
        .obj(
            ty::FILE_ASSET_CONTENTS,
            &[(key::BYTES, V::Bytes(blue_png()))],
        )
        .artboard(60.0, 60.0)
}

/// An image at (20, 20) under `parent`.
fn image(riv: Riv, parent: u64) -> Riv {
    riv.obj(
        ty::IMAGE,
        &[
            (key::PARENT_ID, V::U(parent)),
            (key::X, V::D(20.0)),
            (key::Y, V::D(20.0)),
            (key::ASSET_ID, V::U(0)),
        ],
    )
}

#[test]
fn image_meshes_warp_the_image() {
    // The mesh's third corner moves from (10, 10) to (30, 10).
    let (riv, third) = mesh(image(with_image(), 0), 1, None);
    let s = scene(&animate(riv, third, key::VERTEX_X, 10.0, 30.0));
    // No seam where the triangles meet.
    let c = color_at(&s, 0.0, 25, 25);
    assert!(near(c, BLUE, 2), "{c:?}");
    let c = color_at(&s, 0.0, 21, 28);
    assert!(near(c, BLUE, 2), "{c:?}");
    assert_eq!(color_at(&s, 0.0, 40, 28), WHITE);
    let c = color_at(&s, 1.0, 40, 28);
    assert!(near(c, BLUE, 2), "{c:?}");
    assert_eq!(color_at(&s, 1.0, 40, 21), WHITE);
}

#[test]
fn skinned_meshes_follow_their_bones() {
    // Every corner on a root bone that moves 20 to the right.
    let riv = image(root_bone(with_image(), 0.0, 0.0, 10.0), 0);
    let (riv, _) = mesh(riv, 2, Some(1));
    let skin = riv.next_index();
    let riv = riv
        .obj(
            ty::SKIN,
            &[
                (key::PARENT_ID, V::U(3)),
                (key::SKIN_TX, V::D(20.0)),
                (key::SKIN_TY, V::D(20.0)),
            ],
        )
        .obj(
            ty::TENDON,
            &[(key::PARENT_ID, V::U(skin)), (key::TENDON_BONE_ID, V::U(1))],
        );
    let s = scene(&animate(riv, 1, key::ROOT_BONE_X, 0.0, 20.0));
    let c = color_at(&s, 0.0, 25, 25);
    assert!(near(c, BLUE, 2), "{c:?}");
    assert_eq!(color_at(&s, 1.0, 25, 25), WHITE);
    let c = color_at(&s, 1.0, 45, 25);
    assert!(near(c, BLUE, 2), "{c:?}");
}

#[test]
fn a_skinned_mesh_read_from_a_time_plays_on_from_it_as_read_whole() {
    let riv = image(root_bone(with_image(), 0.0, 0.0, 10.0), 0);
    let (riv, _) = mesh(riv, 2, Some(1));
    let skin = riv.next_index();
    let riv = riv
        .obj(
            ty::SKIN,
            &[
                (key::PARENT_ID, V::U(3)),
                (key::SKIN_TX, V::D(20.0)),
                (key::SKIN_TY, V::D(20.0)),
            ],
        )
        .obj(
            ty::TENDON,
            &[(key::PARENT_ID, V::U(skin)), (key::TENDON_BONE_ID, V::U(1))],
        );
    let bytes = animate(riv, 1, key::ROOT_BONE_X, 0.0, 20.0).bytes();
    let limits = dxn_vector_animation::Limits::default();
    let whole = dxn_vector_animation_rive::read_options(&bytes, &limits, &Default::default())
        .unwrap()
        .scene;
    let from = dxn_vector_animation_rive::read_options(
        &bytes,
        &limits,
        &dxn_vector_animation_rive::Options {
            from: 0.55,
            ..Default::default()
        },
    )
    .unwrap()
    .scene;
    let png = |s: &Scene, t: f64| render_png(&s.frame_at(t), &RenderOptions::default()).unwrap();
    for t in [0.55, 0.6, 0.75, 1.0] {
        assert!(png(&whole, t) == png(&from, t), "at {t}");
    }
}
