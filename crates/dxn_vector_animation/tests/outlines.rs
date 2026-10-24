//! Rectangles, ellipses, stars and trimming.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, AnimatedPoint, Direction, EllipseStart, PathEl, Point, RectStart, Star, StarKind,
    Trim, TrimMode,
    outline::{ellipse_path, path_length, rect_path, star_path, trim_paths},
};

fn first_point(path: &dxn_vector_animation::Path) -> Point {
    match path.elements()[0] {
        PathEl::MoveTo(p) => p,
        _ => panic!("no move"),
    }
}

#[test]
fn rect_starts_where_its_format_does() {
    let c = Point::new(50.0, 50.0);
    let s = Point::new(100.0, 60.0);
    let svg = rect_path(c, s, Point::ZERO, Direction::Clockwise, RectStart::TopLeft);
    assert_eq!(first_point(&svg), Point::new(0.0, 20.0));
    let lottie = rect_path(c, s, Point::ZERO, Direction::Clockwise, RectStart::TopRight);
    assert_eq!(first_point(&lottie), Point::new(100.0, 20.0));
    assert!((path_length(&svg) - 320.0).abs() < 1e-6);
}

#[test]
fn rounded_rect_is_shorter_than_its_box() {
    let p = rect_path(
        Point::new(0.0, 0.0),
        Point::new(100.0, 100.0),
        Point::new(10.0, 10.0),
        Direction::Clockwise,
        RectStart::TopLeft,
    );
    let len = path_length(&p);
    let expected = 4.0 * 80.0 + 2.0 * std::f64::consts::PI * 10.0;
    assert!((len - expected).abs() < 0.1, "{len}");
}

#[test]
fn ellipse_length_and_start() {
    let p = ellipse_path(
        Point::ZERO,
        Point::new(10.0, 10.0),
        Direction::Clockwise,
        EllipseStart::Top,
    );
    assert_eq!(first_point(&p), Point::new(0.0, -10.0));
    assert!((path_length(&p) - 2.0 * std::f64::consts::PI * 10.0).abs() < 0.05);
}

#[test]
fn counter_clockwise_keeps_the_start() {
    let cw = ellipse_path(
        Point::ZERO,
        Point::new(10.0, 10.0),
        Direction::Clockwise,
        EllipseStart::Right,
    );
    let ccw = ellipse_path(
        Point::ZERO,
        Point::new(10.0, 10.0),
        Direction::CounterClockwise,
        EllipseStart::Right,
    );
    assert_eq!(first_point(&cw), first_point(&ccw));
    // The first arc goes down clockwise, up counter-clockwise.
    let y = |p: &dxn_vector_animation::Path| match p.elements()[1] {
        PathEl::CubicTo(_, _, end) => end.y,
        _ => 0.0,
    };
    assert!(y(&cw) > 0.0 && y(&ccw) < 0.0);
}

#[test]
fn star_has_twice_its_points() {
    let star = Star {
        kind: StarKind::Star,
        center: AnimatedPoint::fixed(0.0, 0.0),
        points: Animated::Static(5.0),
        outer_radius: Animated::Static(10.0),
        inner_radius: Animated::Static(4.0),
        outer_roundness: Animated::Static(0.0),
        inner_roundness: Animated::Static(0.0),
        rotation: Animated::Static(0.0),
        direction: Direction::Clockwise,
    };
    let p = star_path(&star, 0.0);
    assert_eq!(p.elements().len(), 1 + 10 + 1);
    assert!((first_point(&p).y + 10.0).abs() < 1e-9);
}

#[test]
fn trim_draws_a_part() {
    let mut line = dxn_vector_animation::Path::new();
    line.move_to(Point::new(0.0, 0.0));
    line.line_to(Point::new(100.0, 0.0));
    let trim = |s: f64, e: f64, o: f64| Trim {
        start: Animated::Static(s),
        end: Animated::Static(e),
        offset: Animated::Static(o),
        mode: TrimMode::EachPath,
    };
    let half = trim_paths(std::slice::from_ref(&line), &trim(0.25, 0.75, 0.0), 0.0);
    assert!((path_length(&half) - 50.0).abs() < 1e-6);
    assert_eq!(first_point(&half), Point::new(25.0, 0.0));
    // Wrapping past the end: two pieces.
    let wrapped = trim_paths(std::slice::from_ref(&line), &trim(0.0, 0.5, 0.75), 0.0);
    assert!((path_length(&wrapped) - 50.0).abs() < 1e-6);
    assert!(trim_paths(std::slice::from_ref(&line), &trim(0.5, 0.5, 0.0), 0.0).is_empty());
}

#[test]
fn combined_trim_runs_across_paths() {
    let seg = |y: f64| {
        let mut p = dxn_vector_animation::Path::new();
        p.move_to(Point::new(0.0, y));
        p.line_to(Point::new(100.0, y));
        p
    };
    let trim = Trim {
        start: Animated::Static(0.0),
        end: Animated::Static(0.5),
        offset: Animated::Static(0.0),
        mode: TrimMode::Combined,
    };
    let out = trim_paths(&[seg(0.0), seg(10.0)], &trim, 0.0);
    assert!((path_length(&out) - 100.0).abs() < 1e-6);
}
