//! What the graph cannot express is sampled frame by frame, that part
//! alone: the rest stays on the graph, and the part draws within the groups
//! it is in, faded and blended once, as the reference draws it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{differing_by, plan_pixels, reference_pixels};
use dxn_vector_animation::{
    Animated, AnimatedPoint, BlendMode, Clip, ClipShape, Color, Direction, Easing, Fill, Geometry,
    Group, Keyframe, LoopMode, Matrix, Node, Paint, Path, PathEl, Point, RectStart, Scene, Shape,
    ShapePath, Star, StarKind, Timeline, Track, TransformOp, Trim, TrimMode,
};
use dxn_vector_animation_nsh::plan::{self, Unit};

const TIMES: &[f64] = &[0.0, 0.4, 0.9, 1.3, 2.0];

/// Asserts that the plan draws `scene` as the reference does, colors
/// within a tenth: a fade or blend applied twice shows.
fn assert_plan_matches(scene: &Scene, times: &[f64], w: u32, h: u32) {
    for &t in times {
        let share = differing_by(
            &plan_pixels(scene, t, w, h),
            &reference_pixels(scene, t, w, h),
            24,
        );
        assert!(
            share <= 0.004,
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

fn fill(color: Color) -> Option<Fill> {
    Some(Fill {
        paint: Paint::solid(color),
        opacity: Animated::Static(1.0),
        rule: Default::default(),
    })
}

fn rect(x: f64, y: f64, w: f64, h: f64, color: Color) -> Node {
    Node::shape(Shape {
        paths: vec![ShapePath::new(Geometry::Rect {
            center: AnimatedPoint::fixed(x + w / 2.0, y + h / 2.0),
            size: AnimatedPoint::fixed(w, h),
            radius: AnimatedPoint::fixed(0.0, 0.0),
            direction: Direction::Clockwise,
            start: RectStart::TopLeft,
        })],
        fill: fill(color),
        ..Default::default()
    })
}

/// A turning star.
fn star_geometry(x: f64, y: f64) -> Geometry {
    Geometry::Star(Star {
        kind: StarKind::Star,
        center: AnimatedPoint::fixed(x, y),
        points: Animated::Static(5.0),
        outer_radius: Animated::Static(22.0),
        inner_radius: track(&[(0.0, 8.0), (2.0, 14.0)]),
        outer_roundness: Animated::Static(0.0),
        inner_roundness: Animated::Static(0.0),
        rotation: track(&[(0.0, 0.0), (2.0, 72.0)]),
        direction: Direction::Clockwise,
    })
}

/// A turning star gaining points, trimmed: what the graph does not draw
/// (a trim of an outline whose segments change).
fn star(x: f64, y: f64, color: Color) -> Node {
    let mut geometry = star_geometry(x, y);
    if let Geometry::Star(s) = &mut geometry {
        s.points = track(&[(0.0, 5.0), (2.0, 7.0)]);
    }
    Node::shape(Shape {
        paths: vec![ShapePath::new(geometry)],
        fill: fill(color),
        trim: Some(Trim {
            start: Animated::Static(0.0),
            end: track(&[(0.0, 0.3), (2.0, 1.0)]),
            offset: Animated::Static(0.0),
            mode: TrimMode::Combined,
        }),
        ..Default::default()
    })
}

/// A triangle turning into a diamond of curves: in a clip, what the graph
/// does not draw (its commands change).
fn morphing(x: f64, y: f64) -> Geometry {
    let p = Point::new;
    let triangle = Path(vec![
        PathEl::MoveTo(p(x, y - 30.0)),
        PathEl::LineTo(p(x + 30.0, y + 25.0)),
        PathEl::LineTo(p(x - 30.0, y + 25.0)),
        PathEl::Close,
    ]);
    let curve = |a: Point, b: Point| PathEl::CubicTo(a, b, b);
    let diamond = Path(vec![
        PathEl::MoveTo(p(x, y - 35.0)),
        curve(p(x + 20.0, y - 20.0), p(x + 35.0, y)),
        curve(p(x + 20.0, y + 20.0), p(x, y + 35.0)),
        curve(p(x - 20.0, y + 20.0), p(x - 35.0, y)),
        curve(p(x - 20.0, y - 20.0), p(x, y - 35.0)),
        PathEl::Close,
    ]);
    Geometry::Path {
        path: Animated::Track(Track::new(vec![
            Keyframe::new(0.0, triangle, Easing::Linear),
            Keyframe::new(2.0, diamond, Easing::Linear),
        ])),
    }
}

fn group(children: Vec<Node>) -> Group {
    Group {
        children,
        ..Group::new()
    }
}

fn scene(children: Vec<Node>) -> Scene {
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
        root: group(children),
    }
}

const RED: Color = Color::rgba(0.86, 0.16, 0.16, 1.0);
const GREEN: Color = Color::rgba(0.12, 0.63, 0.31, 1.0);
const BLUE: Color = Color::rgba(0.16, 0.35, 0.86, 1.0);

/// How many units of each kind: canvases, sampled parts, layers.
fn kinds(units: &[Unit]) -> (usize, usize, usize) {
    units.iter().fold((0, 0, 0), |(c, s, l), unit| match unit {
        Unit::Canvas(_) | Unit::Wide { .. } | Unit::Area { .. } => (c + 1, s, l),
        Unit::Sampled(_) | Unit::Raster(_) => (c, s + 1, l),
        Unit::Layer { units, .. } => {
            let (ci, si, li) = kinds(units);
            (c + ci, s + si, l + li + 1)
        }
    })
}

#[test]
fn a_star_alone_is_sampled_between_canvases() {
    let s = scene(vec![
        rect(5.0, 5.0, 60.0, 60.0, RED),
        star(50.0, 50.0, GREEN),
        rect(40.0, 40.0, 55.0, 30.0, BLUE),
    ]);
    let plan = plan::compile(&s, Matrix::IDENTITY).expect("the rest plays on the graph");
    assert_eq!(
        plan.sampled,
        vec!["a trim of an outline whose runs change".to_owned()]
    );
    assert_eq!(kinds(&plan.units), (2, 1, 0));
    assert_plan_matches(&s, TIMES, 100, 100);
}

#[test]
fn sampled_siblings_share_one_part() {
    let s = scene(vec![
        star(30.0, 30.0, GREEN),
        star(70.0, 70.0, BLUE),
        rect(40.0, 40.0, 20.0, 20.0, RED),
        star(70.0, 30.0, RED),
    ]);
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert_eq!(plan.sampled.len(), 1, "each reason once");
    assert_eq!(kinds(&plan.units), (1, 2, 0));
    assert_plan_matches(&s, TIMES, 100, 100);
}

#[test]
fn a_part_in_a_fading_layer_fades_once() {
    // Two shapes in a group at half opacity: a layer, faded as one.
    let mut faded = group(vec![
        rect(10.0, 10.0, 60.0, 60.0, RED),
        star(55.0, 55.0, BLUE),
    ]);
    faded.opacity = track(&[(0.0, 0.3), (2.0, 0.8)]);
    let s = scene(vec![
        rect(0.0, 40.0, 100.0, 20.0, GREEN),
        Node::Group(faded),
    ]);
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert_eq!(kinds(&plan.units).2, 1, "the group is a layer");
    assert_plan_matches(&s, TIMES, 100, 100);
}

#[test]
fn a_part_in_a_faded_group_keeps_its_fade() {
    // The star alone in a group at half opacity: no layer fades it.
    let mut faded = group(vec![star(50.0, 50.0, BLUE)]);
    faded.opacity = Animated::Static(0.5);
    let s = scene(vec![
        rect(0.0, 40.0, 100.0, 20.0, GREEN),
        Node::Group(faded),
    ]);
    assert_plan_matches(&s, TIMES, 100, 100);
}

#[test]
fn a_part_in_a_blending_layer_blends_once() {
    // The star over a shape in the layer: the layer multiplies both onto
    // what is under it, the star's own canvas nothing.
    let mut multiplied = group(vec![
        rect(20.0, 20.0, 40.0, 40.0, RED),
        star(50.0, 50.0, BLUE),
    ]);
    multiplied.blend = BlendMode::Multiply;
    let s = scene(vec![
        rect(0.0, 30.0, 100.0, 40.0, GREEN),
        Node::Group(multiplied),
    ]);
    assert_plan_matches(&s, TIMES, 100, 100);
}

#[test]
fn drawing_resumes_within_the_groups_after_a_part() {
    // The canvas after the star starts inside the moved and clipped group.
    let mut moved = group(vec![
        rect(0.0, 0.0, 40.0, 40.0, RED),
        star(30.0, 30.0, GREEN),
        rect(25.0, 25.0, 40.0, 40.0, BLUE),
    ]);
    moved.transform = vec![TransformOp::Translate {
        x: track(&[(0.0, 5.0), (2.0, 25.0)]),
        y: Animated::Static(10.0),
    }];
    moved.clip = Some(Clip {
        shapes: vec![ClipShape {
            geometry: Geometry::Ellipse {
                center: AnimatedPoint::fixed(32.0, 32.0),
                radius: AnimatedPoint::fixed(30.0, 30.0),
                direction: Direction::Clockwise,
                start: Default::default(),
            },
            transform: Vec::new(),
            rule: Default::default(),
        }],
        one_path: false,
    });
    let s = scene(vec![Node::Group(moved)]);
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert_eq!(kinds(&plan.units), (2, 1, 0));
    assert_plan_matches(&s, TIMES, 100, 100);
}

#[test]
fn a_group_set_up_with_what_the_graph_cannot_express_is_sampled_whole() {
    // Clipped by a morph between different commands: the group's setup
    // fails, the group is sampled with all it holds, and nothing it began
    // stays behind.
    let mut clipped = group(vec![
        rect(0.0, 0.0, 100.0, 100.0, BLUE),
        rect(20.0, 20.0, 30.0, 30.0, RED),
    ]);
    clipped.transform = vec![TransformOp::Rotate {
        degrees: track(&[(0.0, 0.0), (2.0, 30.0)]),
    }];
    clipped.clip = Some(Clip {
        shapes: vec![ClipShape {
            geometry: morphing(50.0, 50.0),
            transform: Vec::new(),
            rule: Default::default(),
        }],
        one_path: false,
    });
    let s = scene(vec![
        rect(0.0, 0.0, 30.0, 30.0, GREEN),
        Node::Group(clipped),
        rect(70.0, 70.0, 30.0, 30.0, GREEN),
    ]);
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert_eq!(kinds(&plan.units), (2, 1, 0));
    assert_plan_matches(&s, TIMES, 100, 100);
}

/// The scene of [`a_star_alone_is_sampled_between_canvases`].
fn mixed_scene() -> Scene {
    scene(vec![
        rect(5.0, 5.0, 60.0, 60.0, RED),
        star(50.0, 50.0, GREEN),
        rect(40.0, 40.0, 55.0, 30.0, BLUE),
    ])
}

/// A scene with a sampled part in a player.
#[nsh_core::prelude::component]
fn Mixed() -> impl nsh_core::prelude::IntoElement {
    use dxn_vector_animation_nsh::{AnimationInfo, Source, vector_animation};
    use nsh_core::prelude::*;
    let status = signal(String::from("loading"));
    let json = dxn_vector_animation::compiled::to_json(&mixed_scene(), false).unwrap();
    view()
        .child(
            vector_animation()
                .src(Some(Source::Text(json)))
                .on_load(move |info: &AnimationInfo| {
                    status.set(format!("Loaded {:.1}s", info.duration))
                })
                .on_error(move |e: &String| status.set(format!("Error {e}"))),
        )
        .child(text(move || status.get()))
}

#[test]
fn the_player_plays_the_graph_and_draws_the_part_as_it_moves() {
    use std::sync::{Arc, Mutex};

    use nsh_core::{MountItem, Size, animation::AnimationOp, element::Root, prelude::px};

    let mut app = nsh_testing::Headless::start(nsh_engine::EngineConfig {
        viewport: Size::new(px(200.0), px(200.0)),
        ..nsh_engine::EngineConfig::new(Root::of::<Mixed>())
    })
    .expect("the engine starts");
    let seen = Arc::new(Mutex::new((0usize, false)));
    let log = seen.clone();
    app.observe(move |_, batch| {
        let mut s = log.lock().unwrap();
        for item in &batch.items {
            match item {
                MountItem::Canvas { .. } => s.0 += 1,
                MountItem::Animation(AnimationOp::StartComposite { .. }) => s.1 = true,
                _ => {}
            }
        }
    });
    app.wait_for("Loaded 2.0s").unwrap();
    app.wait(std::time::Duration::from_millis(100)).unwrap();
    let (before, started) = *seen.lock().unwrap();
    assert!(started, "the rest plays on the graph");
    // Drawn again and again as the clock moves: four times, however slowly
    // the machine draws them.
    let counted = seen.clone();
    app.wait_until(
        "the star's canvas drawn again four times",
        std::time::Duration::from_secs(10),
        move |_| counted.lock().unwrap().0 > before + 3,
    )
    .unwrap();
}
