//! Trims that move, of fills, and of outlines that move, play on the
//! graph: the trimmed part as the graph moves it (along still outlines),
//! filled; along outlines that move, sampled over time with every segment
//! kept (those outside the part a point where it ends). They draw what
//! the reference draws.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Easing, EllipseStart, Fill, Geometry, Group,
    Keyframe, LineCap, LineJoin, LoopMode, Matrix, Node, Paint, Path, Point, RectStart, Scene,
    Shape, ShapePath, Star, StarKind, Stroke, Timeline, Track, Trim, TrimMode,
};
use dxn_vector_animation_nsh::plan;

fn track(keys: &[(f64, f64)]) -> Animated<f64> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|(t, v)| Keyframe::new(*t, *v, Easing::EASE_IN_OUT))
            .collect(),
    ))
}

/// The plan plays `scene` on the graph alone and draws what the reference
/// does.
fn assert_native(scene: &Scene) {
    let plan = plan::compile(scene, Matrix::IDENTITY).expect("it compiles");
    assert!(plan.sampled.is_empty(), "sampled: {:?}", plan.sampled);
    let times: Vec<f64> = (0..=20).map(|k| f64::from(k) * 0.1).collect();
    common::assert_plan_matches(scene, &times, 100, 100);
}

fn scene(children: Vec<Node>) -> Scene {
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
            children,
            ..Group::new()
        },
    }
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
        width: Animated::Static(5.0),
        cap: LineCap::Round,
        join: LineJoin::Round,
        miter_limit: 4.0,
        dash: None,
    })
}

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// A wave: an open path of curves and a corner.
fn wave() -> ShapePath {
    let mut path = Path::new();
    path.move_to(p(10.0, 70.0));
    path.cubic_to(p(25.0, 10.0), p(45.0, 10.0), p(55.0, 50.0));
    path.cubic_to(p(65.0, 90.0), p(85.0, 90.0), p(90.0, 30.0));
    path.line_to(p(50.0, 20.0));
    ShapePath::new(Geometry::Path {
        path: Animated::Static(path),
    })
}

fn disc() -> ShapePath {
    ShapePath::new(Geometry::Ellipse {
        center: AnimatedPoint::fixed(50.0, 50.0),
        radius: AnimatedPoint::fixed(38.0, 30.0),
        direction: Direction::Clockwise,
        start: EllipseStart::Top,
    })
}

const RED: Color = Color::rgba(0.9, 0.2, 0.1, 1.0);
const BLUE: Color = Color::rgba(0.1, 0.3, 0.9, 1.0);

#[test]
fn a_moving_trim_of_a_fill_fills_the_trimmed_part() {
    // A wave and a disc, their fills trimmed together, then each, the
    // offset wrapping round.
    for mode in [TrimMode::Combined, TrimMode::EachPath] {
        let s = scene(vec![Node::shape(Shape {
            paths: vec![wave(), disc()],
            fill: fill(RED),
            stroke: stroke(BLUE),
            trim: Some(Trim {
                start: track(&[(0.0, 0.0), (1.0, 0.3), (2.0, 0.1)]),
                end: track(&[(0.0, 0.4), (1.0, 0.9), (2.0, 1.0)]),
                offset: track(&[(0.0, 0.0), (2.0, 0.6)]),
                mode,
            }),
            ..Default::default()
        })]);
        assert_native(&s);
    }
}

#[test]
fn a_trim_of_an_outline_that_moves_plays_on_the_graph() {
    // A star turning and gaining depth, drawn on and off, filled and
    // stroked; a rectangle growing, its trim still.
    let star = ShapePath::new(Geometry::Star(Star {
        kind: StarKind::Star,
        center: AnimatedPoint::fixed(50.0, 50.0),
        points: Animated::Static(5.0),
        outer_radius: Animated::Static(40.0),
        inner_radius: track(&[(0.0, 12.0), (2.0, 24.0)]),
        outer_roundness: Animated::Static(0.0),
        inner_roundness: Animated::Static(0.0),
        rotation: track(&[(0.0, 0.0), (2.0, 72.0)]),
        direction: Direction::Clockwise,
    }));
    let s = scene(vec![Node::shape(Shape {
        paths: vec![star],
        fill: fill(RED),
        stroke: stroke(BLUE),
        trim: Some(Trim {
            start: track(&[(0.0, 0.0), (2.0, 0.5)]),
            end: track(&[(0.0, 0.2), (1.0, 1.0)]),
            offset: Animated::Static(0.0),
            mode: TrimMode::Combined,
        }),
        ..Default::default()
    })]);
    assert_native(&s);
    let rect = ShapePath::new(Geometry::Rect {
        center: AnimatedPoint::fixed(50.0, 50.0),
        size: AnimatedPoint {
            x: track(&[(0.0, 30.0), (2.0, 80.0)]),
            y: Animated::Static(50.0),
        },
        radius: AnimatedPoint::fixed(6.0, 6.0),
        direction: Direction::Clockwise,
        start: RectStart::TopLeft,
    });
    let s = scene(vec![Node::shape(Shape {
        paths: vec![rect],
        stroke: stroke(BLUE),
        trim: Some(Trim {
            start: Animated::Static(0.1),
            end: Animated::Static(0.7),
            offset: Animated::Static(0.0),
            mode: TrimMode::Combined,
        }),
        ..Default::default()
    })]);
    assert_native(&s);
}
