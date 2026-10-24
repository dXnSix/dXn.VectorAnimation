//! Outlines whose points move as the graph does not compute (stars,
//! rectangles with elliptical corners) play on the graph from samples of
//! their points, and paths morphing between different commands from a copy
//! for each run of commands: both draw what the reference draws.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{differing, plan_pixels, reference_pixels};
use dxn_vector_animation::{
    Animated, AnimatedPoint, Clip, ClipShape, Color, Direction, Easing, Fill, Geometry, Group,
    Keyframe, LineCap, LineJoin, LoopMode, Matrix, Node, Paint, Path, PathEl, Point, RectStart,
    Scene, Shape, ShapePath, Star, StarKind, Stroke, Timeline, Track,
};
use dxn_vector_animation_nsh::plan;

/// Every tenth of a second, and either side of the switches of the morphs
/// below (halfway between keyframes).
fn times() -> Vec<f64> {
    let mut t: Vec<f64> = (0..=20).map(|k| f64::from(k) * 0.1).collect();
    t.extend([0.499, 0.501, 0.999, 1.001, 1.499, 1.501]);
    t
}

/// Asserts the plan plays `scene` on the graph alone and draws it as the
/// reference does.
fn assert_native(scene: &Scene) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    for t in times() {
        let share = differing(
            &plan_pixels(scene, t, 100, 100),
            &reference_pixels(scene, t, 100, 100),
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

fn stroke(color: Color) -> Option<Stroke> {
    Some(Stroke {
        paint: Paint::solid(color),
        opacity: Animated::Static(1.0),
        width: Animated::Static(4.0),
        cap: LineCap::Round,
        join: LineJoin::Round,
        miter_limit: 4.0,
        dash: None,
    })
}

fn scene(geometry: Geometry) -> Scene {
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_mode: LoopMode::Loop,
            markers: vec![],
        },
        root: Group {
            children: vec![Node::shape(Shape {
                paths: vec![ShapePath::new(geometry)],
                fill: fill(Color::rgba(0.9, 0.3, 0.1, 1.0)),
                stroke: stroke(Color::rgba(0.1, 0.2, 0.6, 1.0)),
                ..Default::default()
            })],
            ..Group::new()
        },
    }
}

fn star(kind: StarKind) -> Star {
    Star {
        kind,
        center: AnimatedPoint::fixed(50.0, 50.0),
        points: Animated::Static(5.0),
        outer_radius: Animated::Static(40.0),
        inner_radius: Animated::Static(18.0),
        outer_roundness: Animated::Static(0.0),
        inner_roundness: Animated::Static(0.0),
        rotation: Animated::Static(0.0),
        direction: Direction::Clockwise,
    }
}

#[test]
fn a_turning_star_plays_on_the_graph() {
    let mut s = star(StarKind::Star);
    s.rotation = track(&[(0.0, 0.0), (2.0, 144.0)]);
    s.inner_radius = track(&[(0.0, 10.0), (1.0, 25.0), (2.0, 15.0)]);
    assert_native(&scene(Geometry::Star(s)));
}

#[test]
fn a_star_moving_and_rounding_plays_on_the_graph() {
    let mut s = star(StarKind::Star);
    s.center = AnimatedPoint {
        x: track(&[(0.0, 40.0), (2.0, 60.0)]),
        y: Animated::Static(50.0),
    };
    s.outer_roundness = track(&[(0.0, 0.0), (2.0, 0.8)]);
    s.inner_roundness = track(&[(0.0, 0.3), (2.0, 0.0)]);
    assert_native(&scene(Geometry::Star(s)));
}

#[test]
fn a_growing_polygon_plays_on_the_graph() {
    let mut s = star(StarKind::Polygon);
    s.points = Animated::Static(6.0);
    s.outer_radius = track(&[(0.0, 15.0), (2.0, 45.0)]);
    s.rotation = track(&[(0.0, 0.0), (2.0, -60.0)]);
    assert_native(&scene(Geometry::Star(s)));
}

#[test]
fn a_star_held_between_keyframes_jumps() {
    let mut s = star(StarKind::Star);
    s.rotation = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, 0.0, Easing::Hold),
        Keyframe::new(1.0, 36.0, Easing::Linear),
        Keyframe::new(2.0, 72.0, Easing::Linear),
    ]));
    assert_native(&scene(Geometry::Star(s)));
}

#[test]
fn a_star_gaining_points_plays_on_the_graph() {
    // Each count of points a contour of its own, shown in its time.
    let mut s = star(StarKind::Star);
    s.points = track(&[(0.0, 5.0), (2.0, 8.0)]);
    s.rotation = track(&[(0.0, 0.0), (2.0, 90.0)]);
    assert_native(&scene(Geometry::Star(s)));
}

#[test]
fn a_clip_gaining_points_plays_on_the_graph() {
    let mut s = star(StarKind::Polygon);
    s.points = track(&[(0.0, 3.0), (2.0, 7.0)]);
    let mut clipped = Group {
        children: scene(Geometry::Rect {
            center: AnimatedPoint::fixed(50.0, 50.0),
            size: AnimatedPoint::fixed(90.0, 90.0),
            radius: AnimatedPoint::fixed(0.0, 0.0),
            direction: Direction::Clockwise,
            start: RectStart::TopLeft,
        })
        .root
        .children,
        ..Group::new()
    };
    clipped.clip = Some(Clip {
        shapes: vec![ClipShape {
            geometry: Geometry::Star(s),
            transform: Vec::new(),
            rule: Default::default(),
        }],
    });
    let mut sc = scene(Geometry::Star(star(StarKind::Star)));
    sc.root.children = vec![Node::Group(clipped)];
    assert_native(&sc);
}

#[test]
fn elliptical_corners_that_move_play_on_the_graph() {
    let geometry = Geometry::Rect {
        center: AnimatedPoint::fixed(50.0, 50.0),
        size: AnimatedPoint {
            x: track(&[(0.0, 60.0), (2.0, 80.0)]),
            y: Animated::Static(50.0),
        },
        radius: AnimatedPoint {
            x: track(&[(0.0, 5.0), (2.0, 25.0)]),
            y: Animated::Static(12.0),
        },
        direction: Direction::Clockwise,
        start: RectStart::TopLeft,
    };
    assert_native(&scene(geometry));
}

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// A triangle.
fn triangle(dx: f64) -> Path {
    Path(vec![
        PathEl::MoveTo(p(20.0 + dx, 80.0)),
        PathEl::LineTo(p(50.0 + dx, 20.0)),
        PathEl::LineTo(p(80.0 + dx, 80.0)),
        PathEl::Close,
    ])
}

/// A blob of curves.
fn blob(r: f64) -> Path {
    let k = r * 0.55;
    Path(vec![
        PathEl::MoveTo(p(50.0, 50.0 - r)),
        PathEl::CubicTo(
            p(50.0 + k, 50.0 - r),
            p(50.0 + r, 50.0 - k),
            p(50.0 + r, 50.0),
        ),
        PathEl::CubicTo(
            p(50.0 + r, 50.0 + k),
            p(50.0 + k, 50.0 + r),
            p(50.0, 50.0 + r),
        ),
        PathEl::CubicTo(
            p(50.0 - k, 50.0 + r),
            p(50.0 - r, 50.0 + k),
            p(50.0 - r, 50.0),
        ),
        PathEl::Close,
    ])
}

fn morph(keys: Vec<(f64, Path, Easing)>) -> Geometry {
    Geometry::Path {
        path: Animated::Track(Track::new(
            keys.into_iter()
                .map(|(t, path, easing)| Keyframe::new(t, path, easing))
                .collect(),
        )),
    }
}

#[test]
fn a_path_morphing_between_different_commands_plays_on_the_graph() {
    // Triangle, triangle moved, blob, blob grown: two runs of commands.
    let geometry = morph(vec![
        (0.0, triangle(-10.0), Easing::Linear),
        (1.0, triangle(10.0), Easing::Linear),
        (2.0, blob(30.0), Easing::Linear),
    ]);
    let plan = plan::compile(&scene(geometry.clone()), Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    assert_native(&scene(geometry));
    let geometry = morph(vec![
        (0.0, blob(20.0), Easing::EASE_IN_OUT),
        (1.0, triangle(0.0), Easing::Linear),
        (1.6, triangle(15.0), Easing::Linear),
        (2.0, blob(35.0), Easing::Linear),
    ]);
    assert_native(&scene(geometry));
}

#[test]
fn a_held_morph_switches_at_the_next_keyframe() {
    let geometry = morph(vec![
        (0.0, triangle(0.0), Easing::Hold),
        (1.0, blob(30.0), Easing::Linear),
        (2.0, blob(10.0), Easing::Linear),
    ]);
    assert_native(&scene(geometry));
}

#[test]
fn sampled_points_keep_only_the_samples_a_line_does_not_stand_for() {
    use dxn_vector_animation_nsh::graph::Expr;
    let mut s = star(StarKind::Star);
    s.rotation = track(&[(0.0, 0.0), (2.0, 144.0)]);
    let plan = plan::compile(&scene(Geometry::Star(s)), Matrix::IDENTITY).unwrap();
    let breakpoints: usize = plan
        .graph
        .nodes()
        .iter()
        .map(|n| match n {
            Expr::Interp { inputs, .. } => inputs.len(),
            _ => 0,
        })
        .sum();
    // 20 coordinates sampled 241 times: lines stand for most samples.
    assert!(breakpoints < 20 * 241 / 4, "{breakpoints} breakpoints");
}
