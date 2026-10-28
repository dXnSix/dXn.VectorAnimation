//! Shadows cast by a group's silhouette on the graph: offsets and hues that
//! move, shadows seen through see-through content (cast from past the view,
//! on a canvas reaching past it), Figma's all-or-nothing silhouettes, cards
//! clipping their content (the clip's outline casting where a shape fills
//! it whole), groups that turn; what the canvas casts no silhouette of is
//! drawn as the reference draws it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{
    Animated, AnimatedPoint, Clip, ClipShape, Color, Direction, DropShadow, Easing, Effect,
    EllipseStart, Fill, FillRule, Geometry, GradientStop, Group, Keyframe, LinearGradient,
    LoopMode, Matrix, Node, Paint, RectStart, Scene, Shape, ShapePath, Timeline, Track,
    TransformOp,
};
use dxn_vector_animation_nsh::plan::{self, Op, Unit};

const TIMES: &[f64] = &[0.0, 0.3, 0.5, 1.0, 1.4, 2.0];

/// The share of pixels that differ from the reference by more than a
/// tenth, at its worst over `TIMES`.
fn worst(scene: &Scene) -> f64 {
    TIMES
        .iter()
        .map(|&t| {
            common::differing_by(
                &common::plan_pixels(scene, t, 100, 100),
                &common::reference_pixels(scene, t, 100, 100),
                24,
            )
        })
        .fold(0.0, f64::max)
}

fn assert_matches(scene: &Scene) {
    assert_within(scene, 0.004);
}

/// As [`assert_matches`], `share` of the pixels apart at most: where the
/// shadow shows under the content's edges, which a knock-out takes away
/// from every pixel they cover at all and a canvas by how much they cover.
fn assert_within(scene: &Scene, share_at_most: f64) {
    for &t in TIMES {
        let ours = common::plan_pixels(scene, t, 100, 100);
        let reference = common::reference_pixels(scene, t, 100, 100);
        let share = common::differing_by(&ours, &reference, 24);
        if share > share_at_most {
            let dir = std::env::temp_dir();
            let _ = ours.save_png(dir.join(format!("cast-plan-{t}.png")));
            let _ = reference.save_png(dir.join(format!("cast-reference-{t}.png")));
            panic!(
                "at t = {t}: {:.2}% of the pixels differ (see {})",
                share * 100.0,
                dir.display()
            );
        }
    }
}

/// Units, all levels: (shadows cast in place, shadows cast from past the
/// view, rasters).
fn kinds(units: &[Unit]) -> (usize, usize, usize) {
    let casts = |ops: &[Op]| {
        usize::from(
            ops.iter()
                .any(|op| matches!(op, Op::Shadow { color, .. } if color & 0xff > 0)),
        )
    };
    units.iter().fold((0, 0, 0), |(a, b, c), u| match u {
        Unit::Canvas(ops) => (a + casts(ops), b, c),
        Unit::Wide { ops, .. }
        | Unit::Area {
            ops,
            shown: Some(_),
            ..
        } => (a, b + casts(ops), c),
        Unit::Area {
            ops, shown: None, ..
        } => (a + casts(ops), b, c),
        Unit::Layer { units, .. } | Unit::Clip { units, .. } => {
            let (x, y, z) = kinds(units);
            (a + x, b + y, c + z)
        }
        Unit::Raster(_) => (a, b, c + 1),
        Unit::Sampled(_) => (a, b, c),
    })
}

fn native(scene: &Scene) -> (usize, usize, usize) {
    let plan = plan::compile(scene, Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    kinds(&plan.units)
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

fn rect_geometry(x: f64, y: f64, w: f64, h: f64, r: f64) -> Geometry {
    Geometry::Rect {
        center: AnimatedPoint::fixed(x + w / 2.0, y + h / 2.0),
        size: AnimatedPoint::fixed(w, h),
        radius: AnimatedPoint::fixed(r, r),
        direction: Direction::Clockwise,
        start: RectStart::TopLeft,
    }
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> ShapePath {
    ShapePath::new(rect_geometry(x, y, w, h, 4.0))
}

fn circle(x: f64, y: f64, r: f64) -> ShapePath {
    ShapePath::new(Geometry::Ellipse {
        center: AnimatedPoint::fixed(x, y),
        radius: AnimatedPoint::fixed(r, r),
        direction: Direction::Clockwise,
        start: EllipseStart::Top,
    })
}

const RED: Color = Color::rgba(0.9, 0.2, 0.1, 1.0);
const BLUE: Color = Color::rgba(0.1, 0.3, 0.9, 1.0);
const GROUND: Color = Color::rgba(0.95, 0.95, 0.9, 1.0);

fn timeline() -> Timeline {
    Timeline {
        duration: 2.0,
        frame_rate: None,
        loop_start: 0.0,
        loop_mode: LoopMode::Loop,
        markers: vec![],
    }
}

/// `group` over a pale ground.
fn on_ground(group: Group) -> Scene {
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: timeline(),
        root: Group {
            children: vec![
                filled(rect(0.0, 0.0, 100.0, 100.0), Paint::solid(GROUND), 1.0),
                Node::Group(group),
            ],
            ..Group::new()
        },
    }
}

/// A card and a disc, `opacity` see-through, with `effects`.
fn scene(effects: Vec<Effect>, opacity: f64) -> Scene {
    on_ground(Group {
        effects,
        children: vec![
            filled(rect(20.0, 15.0, 50.0, 40.0), Paint::solid(RED), opacity),
            filled(circle(55.0, 60.0, 20.0), Paint::solid(BLUE), opacity),
        ],
        ..Group::new()
    })
}

fn shadow(color: Animated<Color>, offset: AnimatedPoint, blur: f64, knockout: bool) -> DropShadow {
    DropShadow {
        color,
        offset,
        blur: Animated::Static(blur),
        knockout,
        hard: false,
    }
}

fn dark(a: f64) -> Animated<Color> {
    Animated::Static(Color::rgba(0.1, 0.0, 0.2, a))
}

fn track<T: Clone>(keys: &[(f64, T)]) -> Animated<T> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|(t, v)| Keyframe::new(*t, v.clone(), Easing::EASE_IN_OUT))
            .collect(),
    ))
}

#[test]
fn a_shadow_that_moves_plays_on_the_graph() {
    let s = scene(
        vec![Effect::DropShadow(shadow(
            dark(0.5),
            AnimatedPoint {
                x: track(&[(0.0, -6.0), (2.0, 10.0)]),
                y: track(&[(0.0, 2.0), (1.0, 8.0), (2.0, 4.0)]),
            },
            3.0,
            true,
        ))],
        1.0,
    );
    assert_eq!(native(&s), (0, 1, 0));
    assert_within(&s, 0.01);
}

#[test]
fn a_shadow_seen_through_content_plays_on_the_graph() {
    // Not knocked out: under the half see-through card, the shadow shows.
    let s = scene(
        vec![Effect::DropShadow(shadow(
            dark(0.8),
            AnimatedPoint::fixed(6.0, 6.0),
            2.0,
            false,
        ))],
        0.5,
    );
    assert_eq!(native(&s), (0, 1, 0));
    assert_matches(&s);
}

#[test]
fn a_shadow_under_opaque_content_is_cast_in_place() {
    let s = scene(
        vec![Effect::DropShadow(shadow(
            dark(0.8),
            AnimatedPoint::fixed(6.0, 6.0),
            2.0,
            false,
        ))],
        1.0,
    );
    assert_eq!(native(&s), (1, 0, 0));
    assert_within(&s, 0.008);
}

#[test]
fn a_hard_shadow_is_cast_in_full_by_see_through_content() {
    // A fifth opaque, as Figma casts it: the shadow at its full opacity
    // beside the content, knocked out under it.
    let mut hard = shadow(dark(0.6), AnimatedPoint::fixed(5.0, 7.0), 2.0, true);
    hard.hard = true;
    let s = scene(vec![Effect::DropShadow(hard)], 0.2);
    assert_eq!(native(&s), (1, 0, 0));
    assert_matches(&s);
}

#[test]
fn a_hard_shadow_of_a_fading_gradient() {
    // Stops from opaque to clear: all or nothing but at the faintest end.
    let gradient = Paint::LinearGradient(LinearGradient {
        start: AnimatedPoint::fixed(20.0, 0.0),
        end: AnimatedPoint::fixed(80.0, 0.0),
        stops: Animated::Static(vec![
            GradientStop {
                offset: 0.0,
                color: RED,
            },
            GradientStop {
                offset: 1.0,
                color: Color::rgba(0.9, 0.2, 0.1, 0.0),
            },
        ]),
        units: Default::default(),
        transform: Matrix::IDENTITY,
        spread: Default::default(),
    });
    for hard in [false, true] {
        let mut cast = shadow(dark(0.7), AnimatedPoint::fixed(0.0, 8.0), 1.0, false);
        cast.hard = hard;
        let s = on_ground(Group {
            effects: vec![Effect::DropShadow(cast)],
            children: vec![filled(rect(20.0, 20.0, 60.0, 30.0), gradient.clone(), 1.0)],
            ..Group::new()
        });
        assert_matches(&s);
    }
}

#[test]
fn a_shadow_changing_hue_plays_on_the_graph() {
    let color = track(&[
        (0.0, Color::rgba(0.8, 0.1, 0.1, 0.6)),
        (1.0, Color::rgba(0.1, 0.1, 0.8, 0.3)),
        (2.0, Color::rgba(0.1, 0.7, 0.2, 0.7)),
    ]);
    let s = scene(
        vec![Effect::DropShadow(shadow(
            color,
            AnimatedPoint::fixed(4.0, 6.0),
            2.0,
            true,
        ))],
        1.0,
    );
    assert_eq!(native(&s), (1, 0, 0));
    assert_matches(&s);
}

/// A card clipping its content (as Figma exports a frame that clips): a
/// group clipped to its rounded outline, its background filling it, a disc
/// running past it.
fn card(background: Paint, clip_by_transform: bool) -> Group {
    let clip = if clip_by_transform {
        ClipShape {
            geometry: rect_geometry(0.0, 0.0, 50.0, 40.0, 6.0),
            transform: vec![TransformOp::translate(20.0, 15.0)],
            rule: FillRule::NonZero,
        }
    } else {
        ClipShape {
            geometry: rect_geometry(20.0, 15.0, 50.0, 40.0, 6.0),
            transform: Vec::new(),
            rule: FillRule::NonZero,
        }
    };
    Group {
        clip: Some(Clip {
            shapes: vec![clip],
            one_path: false,
        }),
        children: vec![
            filled(
                ShapePath::new(rect_geometry(20.0, 15.0, 50.0, 40.0, 6.0)),
                background,
                1.0,
            ),
            filled(circle(62.0, 50.0, 18.0), Paint::solid(BLUE), 1.0),
        ],
        ..Group::new()
    }
}

#[test]
fn a_shadow_of_a_card_clipping_its_content_plays_on_the_graph() {
    for by_transform in [false, true] {
        let s = on_ground(Group {
            effects: vec![Effect::DropShadow(DropShadow {
                hard: true,
                ..shadow(dark(0.4), AnimatedPoint::fixed(0.0, 6.0), 4.0, true)
            })],
            children: vec![Node::Group(card(Paint::solid(RED), by_transform))],
            ..Group::new()
        });
        assert_eq!(native(&s), (1, 0, 0));
        assert_matches(&s);
    }
}

#[test]
fn a_shadow_of_content_clipped_otherwise_casts_from_an_image_of_it() {
    // The background half see-through: the clip is not the silhouette;
    // the content holds still, so an image of it casts.
    let s = on_ground(Group {
        effects: vec![Effect::DropShadow(shadow(
            dark(0.4),
            AnimatedPoint::fixed(0.0, 6.0),
            4.0,
            true,
        ))],
        children: vec![Node::Group(card(
            Paint::solid(Color::rgba(0.9, 0.2, 0.1, 0.5)),
            false,
        ))],
        ..Group::new()
    });
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    assert_eq!(kinds(&plan.units), (1, 0, 0));
    assert!(worst(&s) <= 0.004);
}

/// The card and disc turning about the middle and shrinking, with a shadow
/// falling right of them (seen through them, or knocked out).
fn turning(knockout: bool, opacity: f64) -> Scene {
    let mut s = scene(
        vec![Effect::DropShadow(shadow(
            dark(0.5),
            AnimatedPoint::fixed(6.0, 0.0),
            2.0,
            knockout,
        ))],
        opacity,
    );
    if let Node::Group(g) = &mut s.root.children[1] {
        g.transform = vec![
            TransformOp::translate(50.0, 50.0),
            TransformOp::Rotate {
                degrees: track(&[(0.0, 0.0), (2.0, 180.0)]),
            },
            TransformOp::Scale {
                x: track(&[(0.0, 1.0), (2.0, 0.6)]),
                y: track(&[(0.0, 1.0), (2.0, 0.6)]),
            },
            TransformOp::translate(-50.0, -50.0),
        ];
    }
    s
}

#[test]
fn a_shadow_turning_with_its_group_plays_on_the_graph() {
    // Cast in the group's lengths, in a layer turning and scaling as it.
    let s = turning(false, 0.6);
    assert_eq!(native(&s), (0, 1, 0));
    assert_matches(&s);
    // Knocked out: the content's edges, anti-aliased all along as it turns,
    // keep the shadow as they cover (see `assert_within`).
    let s = turning(true, 1.0);
    assert_eq!(native(&s), (1, 0, 0));
    assert_within(&s, 0.06);
}

#[test]
fn a_shadow_of_content_sliding_past_the_view() {
    // A card from above the view down past its bottom, its shadow seen
    // through it: the canvas reaches over where it goes.
    let s = on_ground(Group {
        transform: vec![TransformOp::Translate {
            x: Animated::Static(0.0),
            y: track(&[(0.0, -70.0), (2.0, 80.0)]),
        }],
        effects: vec![Effect::DropShadow(shadow(
            dark(0.7),
            AnimatedPoint::fixed(8.0, -10.0),
            3.0,
            false,
        ))],
        children: vec![filled(
            rect(25.0, 20.0, 50.0, 50.0),
            Paint::solid(Color::rgba(0.9, 0.2, 0.1, 0.6)),
            1.0,
        )],
        ..Group::new()
    });
    assert_eq!(native(&s), (0, 1, 0));
    assert_matches(&s);
}

#[test]
fn a_shadow_turning_inside_a_clip_is_clipped_where_it_falls() {
    // The turning card and disc in a frame clipping them to a band: the
    // band's clip undone into the turning group's lengths.
    let inner = turning(false, 0.6).root;
    let mut s = turning(false, 0.6);
    s.root = Group {
        children: vec![
            filled(rect(0.0, 0.0, 100.0, 100.0), Paint::solid(GROUND), 1.0),
            Node::Group(Group {
                transform: vec![TransformOp::translate(4.0, 2.0)],
                clip: Some(Clip {
                    shapes: vec![ClipShape {
                        geometry: rect_geometry(10.0, 25.0, 80.0, 45.0, 0.0),
                        transform: Vec::new(),
                        rule: FillRule::NonZero,
                    }],
                    one_path: false,
                }),
                children: vec![inner.children[1].clone()],
                ..Group::new()
            }),
        ],
        ..Group::new()
    };
    assert_eq!(native(&s), (0, 1, 0));
    assert_matches(&s);
}

#[test]
fn a_shadow_cast_from_past_the_view_inside_a_clip() {
    // Seen through the content, inside a frame clipping it to a band.
    let mut s = scene(
        vec![Effect::DropShadow(shadow(
            dark(0.8),
            AnimatedPoint {
                x: track(&[(0.0, -4.0), (2.0, 8.0)]),
                y: Animated::Static(6.0),
            },
            2.0,
            false,
        ))],
        0.5,
    );
    let shadowed = s.root.children.remove(1);
    s.root.children.push(Node::Group(Group {
        clip: Some(Clip {
            shapes: vec![ClipShape {
                geometry: rect_geometry(10.0, 25.0, 80.0, 45.0, 0.0),
                transform: Vec::new(),
                rule: FillRule::NonZero,
            }],
            one_path: false,
        }),
        children: vec![shadowed],
        ..Group::new()
    }));
    assert_eq!(native(&s), (0, 1, 0));
    assert_matches(&s);
}
