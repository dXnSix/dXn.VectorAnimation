//! An artboard's content: shapes and their outlines, paints, transforms,
//! opacity, draw order, clips, solos and images.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, key, near, scene, ty};
use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Fill, Geometry, Group, LoopMode, Node, Paint, Scene, Shape,
    ShapePath, Timeline,
};
use dxn_vector_animation_render::{RenderOptions, render_png};

const RED: u32 = 0xFFFF_0000;
const BLUE: u32 = 0xFF00_00FF;
const WHITE: [u8; 3] = [255, 255, 255];

#[test]
fn a_rectangle_is_filled_about_its_origin() {
    // 20 × 10 about (30, 20): from (20, 15) to (40, 25).
    let s = scene(
        &Riv::new()
            .artboard(50.0, 50.0)
            .rect_shape(0, 30.0, 20.0, 20.0, 10.0, RED),
    );
    assert!(near(color_at(&s, 0.0, 30, 20), [255, 0, 0], 2));
    assert!(near(color_at(&s, 0.0, 21, 16), [255, 0, 0], 2));
    assert_eq!(color_at(&s, 0.0, 18, 20), WHITE);
    assert_eq!(color_at(&s, 0.0, 30, 27), WHITE);
}

#[test]
fn the_first_drawable_in_the_file_is_drawn_on_top() {
    let s = scene(
        &Riv::new()
            .artboard(50.0, 50.0)
            .rect_shape(0, 20.0, 25.0, 20.0, 20.0, RED)
            .rect_shape(0, 30.0, 25.0, 20.0, 20.0, BLUE),
    );
    assert!(
        near(color_at(&s, 0.0, 25, 25), [255, 0, 0], 2),
        "the overlap"
    );
    assert!(near(color_at(&s, 0.0, 37, 25), [0, 0, 255], 2));
}

#[test]
fn nodes_move_rotate_and_scale_what_is_under_them() {
    // A node at (25, 25), turned a quarter and twice as wide: its child,
    // 10 × 4 at (5, 0), lies along y from (25, 25) down, 20 long and 4 wide
    // once scaled before turning.
    let riv = Riv::new().artboard(50.0, 50.0).obj(
        ty::NODE,
        &[
            (key::PARENT_ID, V::U(0)),
            (key::X, V::D(25.0)),
            (key::Y, V::D(25.0)),
            (key::ROTATION, V::D(std::f32::consts::FRAC_PI_2)),
            (key::SCALE_X, V::D(2.0)),
        ],
    );
    let s = scene(&riv.rect_shape(1, 5.0, 0.0, 10.0, 4.0, RED));
    assert!(near(color_at(&s, 0.0, 25, 40), [255, 0, 0], 2));
    assert_eq!(color_at(&s, 0.0, 35, 30), WHITE);
    assert_eq!(color_at(&s, 0.0, 25, 20), WHITE);
}

#[test]
fn opacity_multiplies_down_the_tree_into_each_paint() {
    let riv = Riv::new()
        .artboard(50.0, 50.0)
        .obj(
            ty::NODE,
            &[(key::PARENT_ID, V::U(0)), (key::OPACITY, V::D(0.5))],
        )
        .obj(
            ty::SHAPE,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::X, V::D(25.0)),
                (key::Y, V::D(25.0)),
                (key::OPACITY, V::D(0.5)),
            ],
        )
        .obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(2)),
                (key::PATH_WIDTH, V::D(20.0)),
                (key::PATH_HEIGHT, V::D(20.0)),
            ],
        )
        .fill(2, RED);
    let s = scene(&riv);
    // A quarter of red over white.
    assert!(
        near(color_at(&s, 0.0, 25, 25), [255, 191, 191], 3),
        "{:?}",
        color_at(&s, 0.0, 25, 25)
    );
}

#[test]
fn hidden_drawables_are_not_drawn() {
    let riv = Riv::new().artboard(50.0, 50.0).obj(
        ty::SHAPE,
        &[
            (key::PARENT_ID, V::U(0)),
            (key::X, V::D(25.0)),
            (key::Y, V::D(25.0)),
            (key::DRAWABLE_FLAGS, V::U(1)),
        ],
    );
    let riv = riv
        .obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::PATH_WIDTH, V::D(20.0)),
                (key::PATH_HEIGHT, V::D(20.0)),
            ],
        )
        .fill(1, RED);
    assert_eq!(color_at(&scene(&riv), 0.0, 25, 25), WHITE);
}

#[test]
fn the_artboard_origin_moves_its_content() {
    let riv = Riv::new()
        .obj(
            ty::ARTBOARD,
            &[
                (key::WIDTH, V::D(50.0)),
                (key::HEIGHT, V::D(50.0)),
                (key::ORIGIN_X, V::D(0.5)),
                (key::ORIGIN_Y, V::D(0.5)),
            ],
        )
        .rect_shape(0, 0.0, 0.0, 10.0, 10.0, RED);
    let s = scene(&riv);
    assert!(near(color_at(&s, 0.0, 25, 25), [255, 0, 0], 2));
    assert_eq!(color_at(&s, 0.0, 2, 2), WHITE);
}

#[test]
fn clipping_shapes_clip_the_drawables_under_their_parent() {
    // A node holding a red square 40 wide and a clip by a 10-wide square
    // (its source, under the artboard, not drawn: no fill).
    let riv = Riv::new()
        .artboard(50.0, 50.0)
        .obj(ty::NODE, &[(key::PARENT_ID, V::U(0))])
        .rect_shape(1, 25.0, 25.0, 40.0, 40.0, RED);
    let source = riv.next_index();
    let riv = riv
        .shape(0, 25.0, 25.0)
        .obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(source)),
                (key::PATH_WIDTH, V::D(10.0)),
                (key::PATH_HEIGHT, V::D(10.0)),
            ],
        )
        .obj(
            ty::CLIPPING_SHAPE,
            &[(key::PARENT_ID, V::U(1)), (key::SOURCE_ID, V::U(source))],
        );
    let s = scene(&riv);
    assert!(near(color_at(&s, 0.0, 25, 25), [255, 0, 0], 2));
    assert_eq!(color_at(&s, 0.0, 10, 10), WHITE);
}

#[test]
fn a_clip_of_several_outlines_is_one_path_under_its_fill_rule() {
    // A red square 40 wide clipped by a source of two concentric squares
    // (30 and 10 wide) even-odd: a ring, empty in the middle.
    let riv = Riv::new()
        .artboard(50.0, 50.0)
        .obj(ty::NODE, &[(key::PARENT_ID, V::U(0))])
        .rect_shape(1, 25.0, 25.0, 40.0, 40.0, RED);
    let source = riv.next_index();
    let mut riv = riv.shape(0, 25.0, 25.0);
    for size in [30.0, 10.0] {
        riv = riv.obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(source)),
                (key::PATH_WIDTH, V::D(size)),
                (key::PATH_HEIGHT, V::D(size)),
            ],
        );
    }
    let riv = riv.obj(
        ty::CLIPPING_SHAPE,
        &[
            (key::PARENT_ID, V::U(1)),
            (key::SOURCE_ID, V::U(source)),
            (93, V::U(1)),
        ],
    );
    let s = scene(&riv);
    assert_eq!(color_at(&s, 0.0, 25, 25), WHITE, "the middle");
    assert!(near(color_at(&s, 0.0, 14, 25), [255, 0, 0], 2), "the ring");
    assert_eq!(color_at(&s, 0.0, 7, 25), WHITE, "outside");
}

#[test]
fn a_solo_shows_only_its_active_child() {
    let riv = Riv::new().artboard(50.0, 50.0).obj(
        ty::SOLO,
        &[
            (key::PARENT_ID, V::U(0)),
            (key::ACTIVE_COMPONENT_ID, V::U(6)),
        ],
    );
    // Shape 2 (red) and shape 6 (blue), each four objects.
    let riv = riv
        .rect_shape(1, 25.0, 25.0, 20.0, 20.0, RED)
        .rect_shape(1, 25.0, 25.0, 10.0, 10.0, BLUE);
    let s = scene(&riv);
    assert!(near(color_at(&s, 0.0, 25, 25), [0, 0, 255], 2));
    assert_eq!(color_at(&s, 0.0, 18, 18), WHITE);
}

#[test]
fn points_paths_draw_straight_and_cubic_vertices() {
    // A closed triangle of straight vertices.
    let riv = Riv::new().artboard(50.0, 50.0).shape(0, 0.0, 0.0).obj(
        ty::POINTS_PATH,
        &[(key::PARENT_ID, V::U(1)), (key::IS_CLOSED, V::B(true))],
    );
    let mut riv = riv;
    for (x, y) in [(10.0, 10.0), (40.0, 10.0), (10.0, 40.0)] {
        riv = riv.obj(
            ty::STRAIGHT_VERTEX,
            &[
                (key::PARENT_ID, V::U(2)),
                (key::VERTEX_X, V::D(x)),
                (key::VERTEX_Y, V::D(y)),
            ],
        );
    }
    let s = scene(&riv.fill(1, RED));
    assert!(near(color_at(&s, 0.0, 15, 15), [255, 0, 0], 2));
    assert_eq!(color_at(&s, 0.0, 35, 35), WHITE);
}

#[test]
fn holes_under_the_clockwise_fill_rule_are_cut_out() {
    // An outer square and an inner one flagged a hole, both drawn
    // clockwise: the clockwise rule turns the hole round.
    let riv = Riv::new().artboard(50.0, 50.0).shape(0, 25.0, 25.0);
    let riv = riv
        .obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::PATH_WIDTH, V::D(40.0)),
                (key::PATH_HEIGHT, V::D(40.0)),
            ],
        )
        .obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::PATH_WIDTH, V::D(10.0)),
                (key::PATH_HEIGHT, V::D(10.0)),
                (770, V::B(true)),
            ],
        );
    let fill = riv.next_index();
    let riv = riv
        .obj(
            ty::FILL,
            &[(key::PARENT_ID, V::U(1)), (key::FILL_RULE, V::U(2))],
        )
        .obj(
            ty::SOLID_COLOR,
            &[(key::PARENT_ID, V::U(fill)), (key::COLOR, V::C(RED))],
        );
    let s = scene(&riv);
    assert_eq!(color_at(&s, 0.0, 25, 25), WHITE);
    assert!(near(color_at(&s, 0.0, 10, 10), [255, 0, 0], 2));
}

#[test]
fn linear_gradients_run_from_start_to_end() {
    let riv = Riv::new()
        .artboard(50.0, 50.0)
        .shape(0, 25.0, 25.0)
        .obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::PATH_WIDTH, V::D(50.0)),
                (key::PATH_HEIGHT, V::D(50.0)),
            ],
        )
        .obj(ty::FILL, &[(key::PARENT_ID, V::U(1))])
        .obj(
            ty::LINEAR_GRADIENT,
            &[
                (key::PARENT_ID, V::U(3)),
                (key::GRADIENT_START_X, V::D(-25.0)),
                (key::GRADIENT_END_X, V::D(25.0)),
            ],
        )
        .obj(
            ty::GRADIENT_STOP,
            &[
                (key::PARENT_ID, V::U(4)),
                (key::STOP_COLOR, V::C(RED)),
                (key::STOP_POSITION, V::D(0.0)),
            ],
        )
        .obj(
            ty::GRADIENT_STOP,
            &[
                (key::PARENT_ID, V::U(4)),
                (key::STOP_COLOR, V::C(BLUE)),
                (key::STOP_POSITION, V::D(1.0)),
            ],
        );
    let s = scene(&riv);
    assert!(near(color_at(&s, 0.0, 1, 25), [250, 0, 5], 8));
    assert!(near(color_at(&s, 0.0, 48, 25), [5, 0, 250], 8));
}

/// A 20-wide square at (`x`, 50) scaled 3 times, stroked 2 wide in blue,
/// its stroke scaled with it or not.
fn scaled_stroke(riv: Riv, x: f32, scales: bool) -> Riv {
    let shape = riv.next_index();
    let riv = riv
        .obj(
            ty::SHAPE,
            &[
                (key::PARENT_ID, V::U(0)),
                (key::X, V::D(x)),
                (key::Y, V::D(50.0)),
                (key::SCALE_X, V::D(3.0)),
                (17, V::D(3.0)),
            ],
        )
        .obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(shape)),
                (key::PATH_WIDTH, V::D(20.0)),
                (key::PATH_HEIGHT, V::D(20.0)),
            ],
        );
    let stroke = riv.next_index();
    riv.obj(
        ty::STROKE,
        &[
            (key::PARENT_ID, V::U(shape)),
            (key::THICKNESS, V::D(2.0)),
            (50, V::B(scales)),
        ],
    )
    .obj(
        ty::SOLID_COLOR,
        &[(key::PARENT_ID, V::U(stroke)), (key::COLOR, V::C(BLUE))],
    )
}

#[test]
fn strokes_the_transform_does_not_scale_keep_their_width() {
    // Edges at x 20 and 110: 6 wide scaled (17 to 23), 2 wide not (109 to
    // 111).
    let riv = scaled_stroke(Riv::new().artboard(140.0, 100.0), 50.0, true);
    let s = scene(&scaled_stroke(riv, 140.0, false));
    assert!(near(color_at(&s, 0.0, 22, 50), [0, 0, 255], 2));
    assert!(near(color_at(&s, 0.0, 110, 50), [0, 0, 255], 40));
    assert_eq!(color_at(&s, 0.0, 112, 50), WHITE);
}

#[test]
fn embedded_images_are_drawn_about_their_origin() {
    // A 2 × 2 blue PNG, drawn at (25, 25) about its center.
    let png = render_png(
        &Scene {
            width: 2.0,
            height: 2.0,
            timeline: Timeline {
                duration: 0.0,
                frame_rate: None,
                loop_start: 0.0,
                loop_mode: LoopMode::Once,
                markers: Vec::new(),
            },
            root: Group {
                children: vec![Node::shape(Shape {
                    paths: vec![ShapePath::new(Geometry::Rect {
                        center: AnimatedPoint::fixed(1.0, 1.0),
                        size: AnimatedPoint::fixed(2.0, 2.0),
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
    .unwrap();
    let riv = Riv::new()
        .obj(ty::IMAGE_ASSET, &[(key::NAME, V::S("dot"))])
        .obj(ty::FILE_ASSET_CONTENTS, &[(key::BYTES, V::Bytes(png))])
        .artboard(50.0, 50.0)
        .obj(
            ty::IMAGE,
            &[
                (key::PARENT_ID, V::U(0)),
                (key::X, V::D(25.0)),
                (key::Y, V::D(25.0)),
                (key::SCALE_X, V::D(10.0)),
                (key::ASSET_ID, V::U(0)),
            ],
        );
    let s = scene(&riv);
    assert!(near(color_at(&s, 0.0, 22, 25), [0, 0, 255], 2));
    assert_eq!(color_at(&s, 0.0, 25, 28), WHITE);
}
