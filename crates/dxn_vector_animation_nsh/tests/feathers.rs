//! Blurs of one shape of one color (Rive's feathers): the canvas draws them
//! as the shadow the shape casts from past the view, the shape not drawn,
//! rather than as an image the reference renderer makes each frame.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{
    Animated, AnimatedPoint, Blur, Clip, ClipShape, Color, Direction, Easing, Effect,
    EllipseStart, Fill, FillRule, Geometry, Group, Keyframe, LineCap, LineJoin, LoopMode, Node,
    Paint, RectStart, Scene, Shape, ShapePath, Stroke, Timeline, Track, TransformOp,
};
use dxn_vector_animation_nsh::plan::{self, Unit};

const TIMES: &[f64] = &[0.0, 0.5, 1.0];

fn track(from: f64, to: f64) -> Animated<f64> {
    Animated::Track(Track::new(vec![
        Keyframe::new(0.0, from, Easing::Linear),
        Keyframe::new(1.0, to, Easing::Linear),
    ]))
}

fn scene(root: Group) -> Scene {
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 1.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Loop,
            markers: vec![],
            cues: Vec::new(),
        },
        root,
    }
}

fn disc() -> ShapePath {
    ShapePath::new(Geometry::Ellipse {
        center: AnimatedPoint::fixed(50.0, 50.0),
        radius: AnimatedPoint::fixed(22.0, 22.0),
        direction: Direction::Clockwise,
        start: EllipseStart::Top,
    })
}

fn filled(color: Animated<Color>, opacity: Animated<f64>) -> Node {
    Node::shape(Shape {
        paths: vec![disc()],
        fill: Some(Fill {
            paint: Paint::Solid { color },
            opacity,
            rule: FillRule::NonZero,
        }),
        ..Default::default()
    })
}

fn blurred(radius: Animated<f64>, child: Node) -> Group {
    Group {
        effects: vec![Effect::Blur(Blur { radius })],
        children: vec![child],
        ..Group::new()
    }
}

fn root(children: Vec<Node>) -> Group {
    Group {
        children,
        ..Group::new()
    }
}

/// How many parts the plan draws as images.
fn rasters(units: &[Unit]) -> usize {
    units
        .iter()
        .map(|u| match u {
            Unit::Raster(_) => 1,
            Unit::Layer { units, .. } => rasters(units),
            _ => 0,
        })
        .sum()
}

/// Draws as the reference does, and without an image.
fn native(scene: &Scene) {
    let base = dxn_vector_animation_nsh::fit(scene, 100.0, 100.0, Default::default());
    let plan = plan::compile(scene, base).expect("the scene compiles");
    assert_eq!(rasters(&plan.units), 0, "{:#?}", plan.units);
    common::assert_plan_matches(scene, TIMES, 100, 100);
}

#[test]
fn a_feathered_fill_is_the_shadow_it_casts() {
    let red = Animated::Static(Color::rgba(0.9, 0.2, 0.1, 1.0));
    native(&scene(root(vec![Node::Group(blurred(
        Animated::Static(6.0),
        filled(red, Animated::Static(1.0)),
    ))])));
}

#[test]
fn a_feather_that_grows_fades_between_still_ones() {
    let red = Animated::Static(Color::rgba(0.9, 0.2, 0.1, 1.0));
    native(&scene(root(vec![Node::Group(blurred(
        track(2.0, 9.0),
        filled(red, Animated::Static(1.0)),
    ))])));
}

#[test]
fn a_feathers_color_and_opacity_move_with_it() {
    let color = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, Color::rgba(0.9, 0.2, 0.1, 1.0), Easing::Linear),
        Keyframe::new(1.0, Color::rgba(0.1, 0.3, 0.9, 0.6), Easing::Linear),
    ]));
    native(&scene(root(vec![Node::Group(blurred(
        Animated::Static(5.0),
        filled(color, Animated::Static(0.8)),
    ))])));
    let red = Animated::Static(Color::rgba(0.9, 0.2, 0.1, 1.0));
    native(&scene(root(vec![Node::Group(blurred(
        Animated::Static(5.0),
        filled(red, track(1.0, 0.2)),
    ))])));
}

#[test]
fn a_feathered_stroke_is_the_shadow_it_casts() {
    let ring = Node::shape(Shape {
        paths: vec![disc()],
        stroke: Some(Stroke {
            paint: Paint::solid(Color::rgba(0.1, 0.5, 0.2, 1.0)),
            opacity: Animated::Static(1.0),
            width: Animated::Static(6.0),
            cap: LineCap::Round,
            join: LineJoin::Round,
            miter_limit: 4.0,
            dash: None,
        }),
        ..Default::default()
    });
    native(&scene(root(vec![Node::Group(blurred(
        Animated::Static(4.0),
        ring,
    ))])));
}

#[test]
fn a_feather_over_other_drawing_shows_it_through() {
    let back = Node::shape(Shape {
        paths: vec![ShapePath::new(Geometry::Rect {
            center: AnimatedPoint::fixed(50.0, 50.0),
            size: AnimatedPoint::fixed(80.0, 30.0),
            radius: AnimatedPoint::fixed(0.0, 0.0),
            direction: Direction::Clockwise,
            start: RectStart::TopLeft,
        })],
        fill: Some(Fill {
            paint: Paint::solid(Color::rgba(0.1, 0.1, 0.1, 1.0)),
            opacity: Animated::Static(1.0),
            rule: FillRule::NonZero,
        }),
        ..Default::default()
    });
    let glow = Animated::Static(Color::rgba(1.0, 0.9, 0.2, 0.8));
    native(&scene(root(vec![
        back,
        Node::Group(blurred(
            Animated::Static(7.0),
            filled(glow, Animated::Static(1.0)),
        )),
    ])));
}

#[test]
fn a_feather_inside_a_clip_is_clipped() {
    // As a Rive inner feather: the blur within the outline.
    let clip = Clip {
        shapes: vec![ClipShape {
            geometry: disc().geometry,
            transform: Vec::new(),
            rule: FillRule::NonZero,
        }],
        one_path: false,
    };
    let red = Animated::Static(Color::rgba(0.9, 0.2, 0.1, 1.0));
    let mut moved = blurred(Animated::Static(5.0), filled(red, Animated::Static(1.0)));
    moved.transform = vec![TransformOp::translate(8.0, 6.0)];
    native(&scene(Group {
        clip: Some(clip),
        children: vec![Node::Group(moved)],
        ..Group::new()
    }));
}

#[test]
fn a_feather_that_turns_and_scales_is_cast_in_its_lengths() {
    let red = Animated::Static(Color::rgba(0.9, 0.2, 0.1, 1.0));
    let mut g = blurred(Animated::Static(4.0), filled(red, Animated::Static(1.0)));
    g.transform = vec![
        TransformOp::translate(50.0, 50.0),
        TransformOp::Rotate {
            degrees: track(0.0, 90.0),
        },
        TransformOp::Scale {
            x: track(1.0, 0.6),
            y: Animated::Static(0.8),
        },
        TransformOp::translate(-50.0, -50.0),
    ];
    native(&scene(root(vec![Node::Group(g)])));
}

#[test]
fn a_blur_of_a_gradient_is_still_an_image() {
    let gradient = Node::shape(Shape {
        paths: vec![disc()],
        fill: Some(Fill {
            paint: Paint::LinearGradient(dxn_vector_animation::LinearGradient {
                start: AnimatedPoint::fixed(28.0, 50.0),
                end: AnimatedPoint::fixed(72.0, 50.0),
                stops: Animated::Static(vec![
                    dxn_vector_animation::GradientStop {
                        offset: 0.0,
                        color: Color::rgba(1.0, 0.0, 0.0, 1.0),
                    },
                    dxn_vector_animation::GradientStop {
                        offset: 1.0,
                        color: Color::rgba(0.0, 0.0, 1.0, 1.0),
                    },
                ]),
                units: Default::default(),
                transform: Default::default(),
                spread: Default::default(),
            }),
            opacity: Animated::Static(1.0),
            rule: FillRule::NonZero,
        }),
        ..Default::default()
    });
    let s = scene(root(vec![Node::Group(blurred(
        Animated::Static(5.0),
        gradient,
    ))]));
    let base = dxn_vector_animation_nsh::fit(&s, 100.0, 100.0, Default::default());
    let plan = plan::compile(&s, base).expect("the scene compiles");
    assert_eq!(rasters(&plan.units), 1);
}
