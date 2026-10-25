//! Parts drawn as images the reference renderer makes, where what they
//! draw is blurred throughout, are made at as few pixels as keep the blur
//! three of them, and scaled up where they show; parts with anything
//! sharp (a clip after the blur, a shadow of sharp content) keep the
//! display's pixels.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{
    Animated, AnimatedPoint, Blur, Clip, ClipShape, Color, Direction, DropShadow, Effect,
    EllipseStart, Fill, FillRule, Geometry, Group, LoopMode, Matrix, Node, Paint, RectStart, Scene,
    Shape, ShapePath, Timeline,
};
use dxn_vector_animation_nsh::plan::{self, Unit};

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

fn rect(x: f64, y: f64, w: f64, h: f64) -> Geometry {
    Geometry::Rect {
        center: AnimatedPoint::fixed(x + w / 2.0, y + h / 2.0),
        size: AnimatedPoint::fixed(w, h),
        radius: AnimatedPoint::fixed(0.0, 0.0),
        direction: Direction::Clockwise,
        start: RectStart::TopLeft,
    }
}

/// A disc and a bar in a group with `effects` and `clip`, over a ground.
fn scene(effects: Vec<Effect>, clip: Option<Clip>) -> Scene {
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Loop,
            markers: vec![],
        },
        root: Group {
            children: vec![
                filled(
                    rect(0.0, 0.0, 100.0, 100.0),
                    Color::rgba(0.95, 0.95, 0.9, 1.0),
                ),
                Node::Group(Group {
                    effects,
                    clip,
                    children: vec![
                        filled(
                            Geometry::Ellipse {
                                center: AnimatedPoint::fixed(45.0, 45.0),
                                radius: AnimatedPoint::fixed(22.0, 22.0),
                                direction: Direction::Clockwise,
                                start: EllipseStart::Top,
                            },
                            Color::rgba(0.9, 0.2, 0.1, 1.0),
                        ),
                        filled(
                            rect(30.0, 62.0, 50.0, 14.0),
                            Color::rgba(0.1, 0.3, 0.9, 1.0),
                        ),
                    ],
                    ..Group::new()
                }),
            ],
            ..Group::new()
        },
    }
}

fn blur(sigma: f64) -> Effect {
    Effect::Blur(Blur {
        radius: Animated::Static(sigma),
    })
}

/// The part rasterized, and the image made of it at three pixels a point:
/// its pixels a point.
fn pixels_a_point(scene: &Scene) -> f64 {
    let plan = plan::compile(scene, Matrix::IDENTITY).unwrap();
    let part = plan
        .units
        .iter()
        .find_map(|u| match u {
            Unit::Raster(p) => Some(p.clone()),
            _ => None,
        })
        .expect("a part rasterized");
    let view = plan::in_view(&part, Matrix::IDENTITY, 100.0, 100.0);
    let (image, at) = plan::raster(&view, 0.0, 3.0).unwrap();
    let image = tiny_skia::Pixmap::decode_png(&image.bytes).unwrap();
    f64::from(image.width()) / at.width
}

fn assert_matches(scene: &Scene) {
    for t in [0.0, 1.0] {
        let share = common::differing_by(
            &common::plan_pixels(scene, t, 100, 100),
            &common::reference_pixels(scene, t, 100, 100),
            24,
        );
        assert!(share <= 0.004, "at {t}: {:.2}% differ", share * 100.0);
    }
}

#[test]
fn a_part_blurred_throughout_is_made_at_fewer_pixels() {
    // A deviation of 6 points: half a pixel a point keeps it 3 pixels.
    let s = scene(vec![blur(6.0)], None);
    let k = pixels_a_point(&s);
    assert!((0.45..=0.6).contains(&k), "{k} pixels a point");
    assert_matches(&s);
}

#[test]
fn a_slight_blur_keeps_the_displays_pixels() {
    let s = scene(vec![blur(0.5)], None);
    assert!(pixels_a_point(&s) > 2.9);
    assert_matches(&s);
}

#[test]
fn a_blur_clipped_after_keeps_the_displays_pixels() {
    // The clip's edges come after the blur, sharp.
    let clip = Clip {
        shapes: vec![ClipShape {
            geometry: rect(25.0, 25.0, 50.0, 50.0),
            transform: Vec::new(),
            rule: FillRule::NonZero,
        }],
    };
    let s = scene(vec![blur(6.0)], Some(clip));
    assert!(pixels_a_point(&s) > 2.9);
    assert_matches(&s);
}

#[test]
fn a_blurred_shadow_of_sharp_content_keeps_the_displays_pixels() {
    // A shadow under the sharp content that casts it: not blurred
    // throughout.
    let mut s = scene(
        vec![Effect::DropShadow(DropShadow {
            color: Animated::Static(Color::rgba(0.0, 0.0, 0.0, 0.5)),
            offset: AnimatedPoint::fixed(3.0, 3.0),
            blur: Animated::Static(6.0),
            knockout: false,
            hard: false,
        })],
        None,
    );
    // Content the canvas cannot cast, moving: clipped inside, sliding.
    if let Node::Group(g) = &mut s.root.children[1] {
        let inner = std::mem::take(&mut g.children);
        g.children = vec![Node::Group(Group {
            clip: Some(Clip {
                shapes: vec![ClipShape {
                    geometry: rect(20.0, 20.0, 50.0, 50.0),
                    transform: Vec::new(),
                    rule: FillRule::NonZero,
                }],
            }),
            children: vec![Node::Group(Group {
                transform: vec![dxn_vector_animation::TransformOp::Translate {
                    x: Animated::Track(dxn_vector_animation::Track::new(vec![
                        dxn_vector_animation::Keyframe::new(
                            0.0,
                            0.0,
                            dxn_vector_animation::Easing::Linear,
                        ),
                        dxn_vector_animation::Keyframe::new(
                            2.0,
                            10.0,
                            dxn_vector_animation::Easing::Linear,
                        ),
                    ])),
                    y: Animated::Static(0.0),
                }],
                children: inner,
                ..Group::new()
            })],
            ..Group::new()
        })];
    }
    assert!(pixels_a_point(&s) > 2.9);
}
