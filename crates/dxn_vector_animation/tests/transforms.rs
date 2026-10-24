//! Transform lists, motion paths and matrix interpolation.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, Easing, Interpolate, Keyframe, Matrix, MotionPath, MotionRotate, Path, Point, Track,
    TransformOp, transform::transform_at,
};

fn close(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
}

#[test]
fn the_first_operation_is_outermost() {
    // translate(10, 0) rotate(90): the point (1, 0) turns to (0, 1), then moves.
    let m = transform_at(
        &[TransformOp::translate(10.0, 0.0), TransformOp::rotate(90.0)],
        0.0,
    );
    assert!(close(m.apply(Point::new(1.0, 0.0)), Point::new(10.0, 1.0)));
}

#[test]
fn rotation_about_a_pivot() {
    let ops = [
        TransformOp::translate(50.0, 50.0),
        TransformOp::rotate(180.0),
        TransformOp::translate(-50.0, -50.0),
    ];
    let m = transform_at(&ops, 0.0);
    assert!(close(
        m.apply(Point::new(60.0, 50.0)),
        Point::new(40.0, 50.0)
    ));
}

#[test]
fn motion_follows_the_path() {
    let mut path = Path::new();
    path.move_to(Point::new(0.0, 0.0));
    path.line_to(Point::new(100.0, 0.0));
    path.line_to(Point::new(100.0, 100.0));
    let op = TransformOp::Motion(MotionPath {
        path,
        progress: Animated::Track(Track::new(vec![
            Keyframe::new(0.0, 0.0, Easing::Linear),
            Keyframe::new(1.0, 1.0, Easing::Linear),
        ])),
        rotate: MotionRotate::Auto { offset: 0.0 },
    });
    let m = transform_at(std::slice::from_ref(&op), 0.75);
    assert!(close(m.apply(Point::ZERO), Point::new(100.0, 50.0)));
    // Turned along the second segment (downwards: 90° clockwise).
    assert!(close(
        m.apply_vector(Point::new(1.0, 0.0)),
        Point::new(0.0, 1.0)
    ));
}

#[test]
fn matrices_interpolate_through_rotation() {
    let a = Matrix::IDENTITY;
    let b = Matrix::rotate(90.0);
    let half = a.interpolate(&b, 0.5);
    let expected = Matrix::rotate(45.0);
    assert!(close(
        half.apply(Point::new(1.0, 0.0)),
        expected.apply(Point::new(1.0, 0.0))
    ));
}

#[test]
fn matrices_decompose_and_recompose() {
    let m = Matrix::translate(5.0, 7.0) * Matrix::rotate(30.0) * Matrix::scale(2.0, -3.0);
    let same = m.interpolate(&m, 0.5);
    for p in [
        Point::new(1.0, 0.0),
        Point::new(0.0, 1.0),
        Point::new(3.0, 4.0),
    ] {
        assert!(close(same.apply(p), m.apply(p)));
    }
}
