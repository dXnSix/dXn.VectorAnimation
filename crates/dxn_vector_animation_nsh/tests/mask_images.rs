//! Masks whose content does not move (in the masked group's lengths) are
//! images of what they hide, as the reference renders their content,
//! taking away from the masked content under the group's transform: masks
//! of gradients, of many overlapping shapes (no seams where they meet), of
//! groups that move or turn. Masked content on several canvases is masked
//! on each (where they overlap and the mask shows part, what is under
//! shows through a little).

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

/// Whether a canvas of `units` draws an image taking away
/// (`destination-out`), and how many canvases do.
fn images_taking_away(units: &[Unit]) -> usize {
    units
        .iter()
        .map(|u| match u {
            Unit::Canvas(ops) => {
                let mut away = false;
                let mut found = 0;
                for op in ops {
                    match op {
                        Op::Blend(b) => away = *b == BlendMode::DestinationOut,
                        Op::Restore => away = false,
                        Op::Image(..) if away => found = 1,
                        _ => {}
                    }
                }
                found
            }
            Unit::Layer { units, .. } => images_taking_away(units),
            _ => 0,
        })
        .sum()
}

/// Asserts the plan masks `scene` with `images` images and draws what the
/// reference draws.
fn assert_masked(scene: &Scene, images: usize) {
    assert_masked_within(scene, images, 0.004);
}

/// As [`assert_masked`], `share` of the pixels apart at most.
fn assert_masked_within(scene: &Scene, images: usize, share_at_most: f64) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    assert_eq!(images_taking_away(&plan.units), images);
    for &t in TIMES {
        let share = common::differing_by(
            &common::plan_pixels(scene, t, 100, 100),
            &common::reference_pixels(scene, t, 100, 100),
            24,
        );
        assert!(
            share <= share_at_most,
            "at t = {t}: {:.2}% of the pixels differ",
            share * 100.0
        );
    }
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

fn filled(path: ShapePath, paint: Paint, opacity: f64) -> Node {
    Node::shape(Shape {
        paths: vec![path],
        fill: Some(Fill {
            paint,
            opacity: Animated::Static(opacity),
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

/// Opaque content: a red square and a blue disc over it.
fn content() -> Vec<Node> {
    vec![
        filled(rect(10.0, 10.0, 60.0, 60.0), Paint::solid(RED), 1.0),
        filled(circle(60.0, 60.0, 30.0), Paint::solid(BLUE), 1.0),
    ]
}

/// `content` in a group with `transform` masked by `mask`, over a white
/// ground.
fn masked(
    kind: MaskKind,
    inverted: bool,
    mask: Vec<Node>,
    content: Vec<Node>,
    transform: Vec<TransformOp>,
) -> Scene {
    let mut masked = group(content);
    masked.transform = transform;
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
            filled(rect(0.0, 0.0, 100.0, 100.0), Paint::solid(WHITE), 1.0),
            Node::Group(masked),
        ]),
    }
}

fn fade() -> Paint {
    Paint::LinearGradient(LinearGradient {
        start: AnimatedPoint::fixed(10.0, 0.0),
        end: AnimatedPoint::fixed(80.0, 0.0),
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
    })
}

#[test]
fn a_gradient_mask_fades_the_content() {
    for (kind, inverted) in [
        (MaskKind::Alpha, false),
        (MaskKind::Alpha, true),
        (MaskKind::Luminance, false),
    ] {
        let s = masked(
            kind,
            inverted,
            vec![filled(circle(45.0, 45.0, 35.0), fade(), 1.0)],
            content(),
            Vec::new(),
        );
        assert_masked(&s, 1);
    }
}

#[test]
fn many_overlapping_shapes_mask_without_seams() {
    // Six half-opaque discs over one another: more than regions take, and
    // where they overlap, edges no region draws twice.
    let discs: Vec<Node> = (0..6)
        .map(|k| {
            let a = f64::from(k) * std::f64::consts::TAU / 6.0;
            filled(
                circle(45.0 + 18.0 * a.cos(), 45.0 + 18.0 * a.sin(), 22.0),
                Paint::solid(WHITE),
                0.5,
            )
        })
        .collect();
    let s = masked(MaskKind::Alpha, false, discs, content(), Vec::new());
    assert_masked(&s, 1);
}

#[test]
fn a_still_mask_of_a_group_that_moves_and_turns_follows_it() {
    let s = masked(
        MaskKind::Alpha,
        false,
        vec![filled(circle(45.0, 45.0, 30.0), fade(), 1.0)],
        content(),
        vec![
            TransformOp::Translate {
                x: track(&[(0.0, 40.0), (2.0, 60.0)]),
                y: Animated::Static(50.0),
            },
            TransformOp::Rotate {
                degrees: track(&[(0.0, 0.0), (2.0, 90.0)]),
            },
            TransformOp::translate(-50.0, -50.0),
        ],
    );
    assert_masked(&s, 1);
}

#[test]
fn content_on_several_canvases_is_masked_on_each() {
    // A faded layer in the content, then a shape on a canvas after it.
    let mut layer = group(content());
    layer.opacity = Animated::Static(0.5);
    let s = masked(
        MaskKind::Alpha,
        false,
        vec![filled(circle(45.0, 45.0, 35.0), fade(), 1.0)],
        vec![
            Node::Group(layer),
            filled(rect(55.0, 5.0, 40.0, 20.0), Paint::solid(BLUE), 1.0),
        ],
        Vec::new(),
    );
    // Where the canvases overlap and the mask shows part, what is under
    // shows through the masked canvas above by as much as it hides of it
    // (a share of the mask times what it hides at most a quarter).
    assert_masked_within(&s, 2, 0.008);
    // Fitted into a view of another size: each canvas starts with the fit.
    for &t in TIMES {
        let share = common::differing_by(
            &common::plan_pixels(&s, t, 70, 70),
            &common::reference_pixels(&s, t, 70, 70),
            24,
        );
        assert!(share <= 0.012, "at t = {t}: {:.2}%", share * 100.0);
    }
}

#[test]
fn a_mask_moving_and_fading_as_a_whole_is_an_image_moving_so() {
    // The mask's root slides, turns and fades; its gradient disc and bar
    // do not move within it. Inverted too.
    for inverted in [false, true] {
        let mut s = masked(
            MaskKind::Alpha,
            inverted,
            vec![
                filled(circle(45.0, 45.0, 28.0), fade(), 1.0),
                filled(rect(20.0, 70.0, 60.0, 12.0), Paint::solid(WHITE), 0.6),
            ],
            content(),
            Vec::new(),
        );
        if let Node::Group(g) = &mut s.root.children[1]
            && let Some(mask) = &mut g.mask
        {
            mask.content.transform = vec![
                TransformOp::Translate {
                    x: track(&[(0.0, 35.0), (2.0, 60.0)]),
                    y: Animated::Static(50.0),
                },
                TransformOp::Rotate {
                    degrees: track(&[(0.0, -20.0), (2.0, 30.0)]),
                },
                TransformOp::translate(-50.0, -50.0),
            ];
            mask.content.opacity = track(&[(0.0, 1.0), (1.0, 0.4), (2.0, 0.9)]);
        }
        assert_masked(&s, 1);
    }
}

#[test]
fn a_soft_mask_changing_shape_is_drawn_as_the_reference_draws_it() {
    // A gradient disc growing: no image keeps it, no region draws it.
    let mut disc = circle(45.0, 45.0, 10.0);
    disc.geometry = Geometry::Ellipse {
        center: AnimatedPoint::fixed(45.0, 45.0),
        radius: AnimatedPoint {
            x: track(&[(0.0, 10.0), (2.0, 40.0)]),
            y: track(&[(0.0, 10.0), (2.0, 40.0)]),
        },
        direction: Direction::Clockwise,
        start: EllipseStart::Top,
    };
    let s = masked(
        MaskKind::Alpha,
        false,
        vec![filled(disc, fade(), 1.0)],
        content(),
        Vec::new(),
    );
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert_eq!(plan.sampled, vec!["a soft mask changing shape".to_owned()]);
    assert!(
        plan.units.iter().any(|u| matches!(u, Unit::Raster(_))),
        "the masked group is rasterized"
    );
    for &t in TIMES {
        let share = common::differing_by(
            &common::plan_pixels(&s, t, 100, 100),
            &common::reference_pixels(&s, t, 100, 100),
            24,
        );
        assert!(share <= 0.004, "at t = {t}: {:.2}%", share * 100.0);
    }
}
