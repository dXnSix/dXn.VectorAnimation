//! Soft masks (partly transparent, or by luminance) play on the graph: the
//! content on a canvas of its own, then taken away region by region of the
//! plane the mask's shapes cut it into, as much as the mask does not show
//! there. They draw what the reference draws, colors within a tenth.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{
    Animated, AnimatedPoint, BlendMode, Color, Direction, Easing, EllipseStart, Fill, Geometry,
    GradientStop, GradientUnits, Group, Keyframe, LinearGradient, LoopMode, Mask, MaskKind, Matrix,
    Node, Paint, RectStart, Scene, Shape, ShapePath, Spread, Timeline, Track, TransformOp,
};
use dxn_vector_animation_nsh::plan::{self, Op, Unit};

const RED: Color = Color::rgba(0.9, 0.1, 0.1, 1.0);
const BLUE: Color = Color::rgba(0.1, 0.3, 0.9, 1.0);
const WHITE: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);
const TIMES: &[f64] = &[0.0, 0.3, 0.7, 1.0, 1.4, 2.0];

/// Asserts the plan masks softly (with `destination-out`) and draws what
/// the reference does, colors within a tenth.
fn assert_soft(scene: &Scene) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    assert!(takes_away(&plan.units), "the mask is soft");
    for &t in TIMES {
        let share = common::differing_by(
            &common::plan_pixels(scene, t, 100, 100),
            &common::reference_pixels(scene, t, 100, 100),
            24,
        );
        assert!(
            share <= 0.004,
            "at t = {t}: {:.2}% of the pixels differ",
            share * 100.0
        );
    }
}

/// Whether a canvas of `units` takes away (`destination-out`).
fn takes_away(units: &[Unit]) -> bool {
    units.iter().any(|u| match u {
        Unit::Canvas(ops) => ops
            .iter()
            .any(|op| matches!(op, Op::Blend(BlendMode::DestinationOut))),
        Unit::Layer { units, .. } => takes_away(units),
        Unit::Sampled(_) => false,
    })
}

fn track(keys: &[(f64, f64)]) -> Animated<f64> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|(t, v)| Keyframe::new(*t, *v, Easing::EASE_IN_OUT))
            .collect(),
    ))
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> ShapePath {
    ShapePath::new(Geometry::Rect {
        center: AnimatedPoint::fixed(x + w / 2.0, y + h / 2.0),
        size: AnimatedPoint::fixed(w, h),
        radius: AnimatedPoint::fixed(0.0, 0.0),
        direction: Direction::Clockwise,
        start: RectStart::TopLeft,
    })
}

fn circle(x: f64, y: f64, r: f64) -> ShapePath {
    ShapePath::new(Geometry::Ellipse {
        center: AnimatedPoint::fixed(x, y),
        radius: AnimatedPoint::fixed(r, r),
        direction: Direction::Clockwise,
        start: EllipseStart::Top,
    })
}

fn filled(path: ShapePath, color: Color, opacity: Animated<f64>) -> Node {
    Node::shape(Shape {
        paths: vec![path],
        fill: Some(Fill {
            paint: Paint::solid(color),
            opacity,
            rule: Default::default(),
        }),
        ..Default::default()
    })
}

fn group(children: Vec<Node>) -> Group {
    Group {
        children,
        ..Group::new()
    }
}

fn blended(blend: BlendMode, children: Vec<Node>) -> Node {
    Node::Group(Group {
        blend,
        ..group(children)
    })
}

/// Opaque content: a red square and a blue disc over it.
fn content() -> Vec<Node> {
    vec![
        filled(rect(10.0, 10.0, 60.0, 60.0), RED, Animated::Static(1.0)),
        filled(circle(60.0, 60.0, 30.0), BLUE, Animated::Static(1.0)),
    ]
}

/// `content` masked by `mask`, over a white ground.
fn masked(kind: MaskKind, inverted: bool, mask: Vec<Node>, content: Vec<Node>) -> Scene {
    let mut masked = group(content);
    masked.mask = Some(Box::new(Mask {
        kind,
        inverted,
        content: group(mask),
    }));
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_mode: LoopMode::Loop,
            markers: vec![],
        },
        root: group(vec![
            filled(rect(0.0, 0.0, 100.0, 100.0), WHITE, Animated::Static(1.0)),
            Node::Group(masked),
        ]),
    }
}

fn half(path: ShapePath) -> Node {
    filled(path, WHITE, Animated::Static(0.5))
}

#[test]
fn a_shape_half_opaque_shows_half() {
    let s = masked(
        MaskKind::Alpha,
        false,
        vec![half(circle(45.0, 45.0, 35.0))],
        content(),
    );
    assert_soft(&s);
}

#[test]
fn shapes_added_show_more_where_they_overlap() {
    let s = masked(
        MaskKind::Alpha,
        false,
        vec![
            half(circle(35.0, 40.0, 25.0)),
            half(circle(60.0, 40.0, 25.0)),
            half(rect(30.0, 50.0, 40.0, 40.0)),
        ],
        content(),
    );
    assert_soft(&s);
}

#[test]
fn shapes_intersected_and_subtracted() {
    let s = masked(
        MaskKind::Alpha,
        false,
        vec![
            filled(rect(0.0, 0.0, 100.0, 100.0), WHITE, Animated::Static(1.0)),
            blended(
                BlendMode::DestinationOut,
                vec![half(circle(40.0, 40.0, 25.0))],
            ),
            blended(
                BlendMode::DestinationIn,
                vec![filled(
                    rect(15.0, 15.0, 70.0, 70.0),
                    WHITE,
                    Animated::Static(0.8),
                )],
            ),
        ],
        content(),
    );
    assert_soft(&s);
}

#[test]
fn luminance_of_colored_shapes_turning() {
    let mut turning = group(vec![
        filled(rect(20.0, 20.0, 40.0, 40.0), RED, Animated::Static(1.0)),
        filled(
            circle(55.0, 55.0, 25.0),
            Color::rgba(0.0, 1.0, 0.0, 1.0),
            Animated::Static(1.0),
        ),
    ]);
    turning.opacity = Animated::Static(0.75);
    turning.transform = vec![
        TransformOp::Translate {
            x: Animated::Static(50.0),
            y: Animated::Static(50.0),
        },
        TransformOp::Rotate {
            degrees: track(&[(0.0, 0.0), (2.0, 90.0)]),
        },
        TransformOp::Translate {
            x: Animated::Static(-50.0),
            y: Animated::Static(-50.0),
        },
    ];
    let s = masked(
        MaskKind::Luminance,
        false,
        vec![Node::Group(turning)],
        content(),
    );
    assert_soft(&s);
}

#[test]
fn an_inverted_mask_shows_where_it_does_not() {
    let s = masked(
        MaskKind::Alpha,
        true,
        vec![half(circle(45.0, 45.0, 35.0))],
        content(),
    );
    assert_soft(&s);
}

#[test]
fn a_mask_fading_in() {
    let s = masked(
        MaskKind::Alpha,
        false,
        vec![filled(
            circle(45.0, 45.0, 35.0),
            WHITE,
            track(&[(0.0, 0.1), (2.0, 0.9)]),
        )],
        content(),
    );
    assert_soft(&s);
}

#[test]
fn translucent_content_is_masked_pixel_by_pixel() {
    let translucent = vec![
        filled(rect(10.0, 10.0, 60.0, 60.0), RED, Animated::Static(0.6)),
        filled(circle(60.0, 60.0, 30.0), BLUE, Animated::Static(0.5)),
    ];
    let s = masked(
        MaskKind::Alpha,
        false,
        vec![
            half(circle(35.0, 40.0, 25.0)),
            half(circle(60.0, 40.0, 25.0)),
        ],
        translucent,
    );
    assert_soft(&s);
}

#[test]
fn content_on_several_canvases_is_clipped_hard() {
    // A layer in the content: the mask cannot take away from one canvas.
    let mut layer = group(content());
    layer.opacity = Animated::Static(0.5);
    let s = masked(
        MaskKind::Alpha,
        false,
        vec![half(circle(45.0, 45.0, 35.0))],
        vec![Node::Group(layer)],
    );
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert!(!takes_away(&plan.units));
}

#[test]
fn a_gradient_mask_is_clipped_hard() {
    let gradient = Paint::LinearGradient(LinearGradient {
        start: AnimatedPoint::fixed(0.0, 0.0),
        end: AnimatedPoint::fixed(100.0, 0.0),
        stops: Animated::Static(vec![
            GradientStop {
                offset: 0.0,
                color: WHITE,
            },
            GradientStop {
                offset: 1.0,
                color: Color::rgba(1.0, 1.0, 1.0, 0.0),
            },
        ]),
        units: GradientUnits::UserSpace,
        transform: Matrix::IDENTITY,
        spread: Spread::Pad,
    });
    let s = masked(
        MaskKind::Alpha,
        false,
        vec![Node::shape(Shape {
            paths: vec![circle(45.0, 45.0, 35.0)],
            fill: Some(Fill {
                paint: gradient,
                opacity: Animated::Static(1.0),
                rule: Default::default(),
            }),
            ..Default::default()
        })],
        content(),
    );
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert!(!takes_away(&plan.units));
}

#[test]
fn a_chained_matte_masks_its_mask() {
    // The mask's shapes are themselves masked, inverted, by a disc.
    let mut matte = group(vec![
        half(rect(10.0, 10.0, 80.0, 40.0)),
        filled(circle(50.0, 60.0, 30.0), WHITE, Animated::Static(0.8)),
    ]);
    matte.mask = Some(Box::new(Mask {
        kind: MaskKind::Alpha,
        inverted: true,
        content: group(vec![filled(
            circle(40.0, 40.0, 18.0),
            WHITE,
            Animated::Static(1.0),
        )]),
    }));
    let s = masked(
        MaskKind::Luminance,
        false,
        vec![Node::Group(matte)],
        content(),
    );
    assert_soft(&s);
}

#[test]
fn a_stroked_mask_shape_shows_its_stroke() {
    let ring = Node::shape(Shape {
        paths: vec![circle(50.0, 50.0, 30.0)],
        fill: Some(Fill {
            paint: Paint::solid(WHITE),
            opacity: Animated::Static(0.3),
            rule: Default::default(),
        }),
        stroke: Some(dxn_vector_animation::Stroke {
            paint: Paint::solid(WHITE),
            opacity: Animated::Static(0.9),
            width: Animated::Static(10.0),
            cap: dxn_vector_animation::LineCap::Butt,
            join: dxn_vector_animation::LineJoin::Miter,
            miter_limit: 4.0,
            dash: None,
        }),
        ..Default::default()
    });
    let s = masked(MaskKind::Alpha, false, vec![ring], content());
    assert_soft(&s);
}
