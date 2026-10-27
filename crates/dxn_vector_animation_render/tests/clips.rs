//! Clips: their shapes' union or one path of their outlines, and what is
//! drawn inside them blending with what is under them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, AnimatedPoint, BlendMode, Clip, ClipShape, Color, Fill, FillRule, Geometry, Group,
    LoopMode, Node, Paint, Scene, Shape, ShapePath, Timeline,
};
use dxn_vector_animation_render::{RenderOptions, render};

/// A `w` × `h` rectangle about (`x`, `y`).
fn rect(x: f64, y: f64, w: f64, h: f64) -> Geometry {
    Geometry::Rect {
        center: AnimatedPoint::fixed(x, y),
        size: AnimatedPoint::fixed(w, h),
        radius: AnimatedPoint::fixed(0.0, 0.0),
        direction: Default::default(),
        start: Default::default(),
    }
}

/// `geometry` filled with `color`.
fn filled(geometry: Geometry, color: Color) -> Node {
    Node::shape(Shape {
        paths: vec![ShapePath::new(geometry)],
        fill: Some(Fill {
            paint: Paint::solid(color),
            opacity: Animated::Static(1.0),
            rule: Default::default(),
        }),
        ..Default::default()
    })
}

/// The color at (`x`, `y`) of a 40 × 40 scene of `children` on white.
fn color_at(children: Vec<Node>, x: u32, y: u32) -> [u8; 3] {
    let scene = Scene {
        width: 40.0,
        height: 40.0,
        timeline: Timeline {
            duration: 0.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Once,
            markers: Vec::new(),
            cues: Vec::new(),
        },
        root: Group {
            children,
            ..Group::new()
        },
    };
    let p = render(
        &scene.frame_at(0.0),
        &RenderOptions {
            background: Some(Color::WHITE),
            ..RenderOptions::default()
        },
    )
    .unwrap();
    let c = p.pixel(x, y).unwrap().demultiply();
    [c.red(), c.green(), c.blue()]
}

/// A black square clipped by two overlapping rectangles, even-odd.
fn clipped(one_path: bool) -> Vec<Node> {
    let shape = |g| ClipShape {
        geometry: g,
        transform: Vec::new(),
        rule: FillRule::EvenOdd,
    };
    vec![Node::Group(Group {
        clip: Some(Clip {
            shapes: vec![
                shape(rect(15.0, 20.0, 20.0, 20.0)),
                shape(rect(25.0, 20.0, 20.0, 20.0)),
            ],
            one_path,
        }),
        children: vec![filled(rect(20.0, 20.0, 40.0, 40.0), Color::BLACK)],
        ..Group::new()
    })]
}

#[test]
fn a_clips_shapes_clip_together() {
    // The union: where the rectangles overlap, it shows.
    assert_eq!(color_at(clipped(false), 20, 20), [0, 0, 0]);
    assert_eq!(color_at(clipped(false), 8, 20), [0, 0, 0]);
}

#[test]
fn a_clip_of_one_path_cuts_out_where_its_outlines_overlap_even_odd() {
    assert_eq!(color_at(clipped(true), 20, 20), [255, 255, 255]);
    assert_eq!(color_at(clipped(true), 8, 20), [0, 0, 0]);
    assert_eq!(color_at(clipped(true), 32, 20), [0, 0, 0]);
}

#[test]
fn what_a_clip_draws_blends_with_what_is_under_it() {
    // Red under a clipped group drawing blue multiplied: black inside the
    // clip, red outside it.
    let children = vec![
        filled(
            rect(20.0, 20.0, 40.0, 40.0),
            Color::rgba(1.0, 0.0, 0.0, 1.0),
        ),
        Node::Group(Group {
            clip: Some(Clip {
                shapes: vec![ClipShape {
                    geometry: rect(10.0, 20.0, 20.0, 40.0),
                    transform: Vec::new(),
                    rule: FillRule::NonZero,
                }],
                one_path: false,
            }),
            children: vec![Node::Group(Group {
                blend: BlendMode::Multiply,
                children: vec![filled(
                    rect(20.0, 20.0, 40.0, 40.0),
                    Color::rgba(0.0, 0.0, 1.0, 1.0),
                )],
                ..Group::new()
            })],
            ..Group::new()
        }),
    ];
    assert_eq!(color_at(children.clone(), 10, 20), [0, 0, 0]);
    assert_eq!(color_at(children, 30, 20), [255, 0, 0]);
}
