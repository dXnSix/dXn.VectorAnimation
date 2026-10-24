//! Transform lists at a time, and matrices interpolated through their
//! decomposition.

use kurbo::{ParamCurve, ParamCurveArclen, ParamCurveDeriv};

use crate::{Matrix, MotionPath, MotionRotate, Path, Point, TransformOp};

/// How precisely path lengths are measured, in user units.
const ARCLEN_ACCURACY: f64 = 1e-3;

/// The matrix of transform list `ops` at `time` (the first operation
/// outermost).
pub fn transform_at(ops: &[TransformOp], time: f64) -> Matrix {
    ops.iter()
        .fold(Matrix::IDENTITY, |m, op| m * op_at(op, time))
}

/// The matrix of one operation at `time`.
pub fn op_at(op: &TransformOp, time: f64) -> Matrix {
    match op {
        TransformOp::Translate { x, y } => Matrix::translate(x.sample(time), y.sample(time)),
        TransformOp::Scale { x, y } => Matrix::scale(x.sample(time), y.sample(time)),
        TransformOp::Rotate { degrees } => Matrix::rotate(degrees.sample(time)),
        TransformOp::Skew { x, y } => Matrix::skew(x.sample(time), y.sample(time)),
        TransformOp::Matrix { matrix } => matrix.sample(time),
        TransformOp::Motion(motion) => motion_at(motion, time),
    }
}

/// The transform of a position along a path.
pub fn motion_at(motion: &MotionPath, time: f64) -> Matrix {
    let Some((point, angle)) = point_along(&motion.path, motion.progress.sample(time)) else {
        return Matrix::IDENTITY;
    };
    let degrees = match motion.rotate {
        MotionRotate::Fixed { degrees } => degrees,
        MotionRotate::Auto { offset } => angle + offset,
    };
    Matrix::translate(point.x, point.y) * Matrix::rotate(degrees)
}

/// The point a fraction `progress` of `path`'s length along it, and the
/// direction there (clockwise degrees from the x axis). A closed path
/// wraps around; an open one clamps. `None` for an empty path.
pub fn point_along(path: &Path, progress: f64) -> Option<(Point, f64)> {
    let segments = segments(path);
    let first = segments.first()?;
    let lengths: Vec<f64> = segments.iter().map(|s| s.arclen(ARCLEN_ACCURACY)).collect();
    let total: f64 = lengths.iter().sum();
    if total <= 0.0 {
        let p = first.start();
        return Some((Point::new(p.x, p.y), 0.0));
    }
    let closed = path.elements().last() == Some(&crate::PathEl::Close);
    let progress = if closed {
        progress.rem_euclid(1.0)
    } else {
        progress.clamp(0.0, 1.0)
    };
    let mut distance = progress * total;
    let mut index = segments.len() - 1;
    for (i, len) in lengths.iter().enumerate() {
        if distance <= *len || i == segments.len() - 1 {
            index = i;
            break;
        }
        distance -= len;
    }
    let segment = segments[index];
    let len = lengths[index];
    let t = if len > 0.0 {
        segment.inv_arclen(distance.min(len), ARCLEN_ACCURACY)
    } else {
        0.0
    };
    let p = segment.eval(t);
    let d = tangent(segment, t);
    Some((Point::new(p.x, p.y), d.y.atan2(d.x).to_degrees()))
}

/// The direction of `segment` at `t`, looking past degenerate control
/// points.
fn tangent(segment: kurbo::PathSeg, t: f64) -> kurbo::Vec2 {
    let d = match segment {
        kurbo::PathSeg::Line(l) => l.p1 - l.p0,
        kurbo::PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
        kurbo::PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
    };
    if d.hypot2() > 1e-18 {
        return d;
    }
    let near = if t < 0.5 { t + 1e-3 } else { t - 1e-3 };
    let (a, b) = (segment.eval(t), segment.eval(near));
    if t < 0.5 { b - a } else { a - b }
}

/// The path's drawn segments (a subpath's closing line included).
pub(crate) fn segments(path: &Path) -> Vec<kurbo::PathSeg> {
    path.to_kurbo().segments().collect()
}

/// The decomposition CSS interpolates 2D matrices through:
/// `translate · rotate(angle) · shear(skew) · scale`, where `shear(k)` is
/// `[1, 0, k, 1, 0, 0]`. A player animating matrices interpolates these
/// parts (the angle the shorter way round, see [`unwrap_angles`]) and
/// composes them again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decomposed {
    /// Translation.
    pub translate: (f64, f64),
    /// Scale along x and y.
    pub scale: (f64, f64),
    /// Rotation, clockwise degrees.
    pub angle: f64,
    /// The shear left after removing the rotation (`x += skew · y`).
    pub skew: f64,
}

/// `m`'s decomposition.
pub fn decompose(m: Matrix) -> Decomposed {
    let (mut row0, mut row1) = ((m.a, m.b), (m.c, m.d));
    let mut scale_x = row0.0.hypot(row0.1);
    let mut scale_y;
    if scale_x != 0.0 {
        row0 = (row0.0 / scale_x, row0.1 / scale_x);
    }
    let mut skew = row0.0 * row1.0 + row0.1 * row1.1;
    row1 = (row1.0 - row0.0 * skew, row1.1 - row0.1 * skew);
    scale_y = row1.0.hypot(row1.1);
    if scale_y != 0.0 {
        row1 = (row1.0 / scale_y, row1.1 / scale_y);
        skew /= scale_y;
    }
    // A flip: negate one axis so the rest is a rotation.
    if row0.0 * row1.1 - row0.1 * row1.0 < 0.0 {
        if row0.0 < row1.1 {
            scale_x = -scale_x;
            row0 = (-row0.0, -row0.1);
        } else {
            scale_y = -scale_y;
        }
        skew = -skew;
    }
    Decomposed {
        translate: (m.e, m.f),
        scale: (scale_x, scale_y),
        angle: row0.1.atan2(row0.0).to_degrees(),
        skew,
    }
}

/// The matrix of decomposition `d`.
pub fn recompose(d: Decomposed) -> Matrix {
    Matrix::translate(d.translate.0, d.translate.1)
        * Matrix::rotate(d.angle)
        * Matrix::new(1.0, 0.0, d.skew, 1.0, 0.0, 0.0)
        * Matrix::scale(d.scale.0, d.scale.1)
}

/// Angles adjusted by whole turns so each one is within 180° of the one
/// before: interpolated in order, they turn the shorter way round.
pub fn unwrap_angles(angles: &mut [f64]) {
    for i in 1..angles.len() {
        let prev = angles[i - 1];
        while angles[i] - prev > 180.0 {
            angles[i] -= 360.0;
        }
        while angles[i] - prev < -180.0 {
            angles[i] += 360.0;
        }
    }
}

/// `a` a fraction `t` of the way to `b`, through their decompositions.
pub(crate) fn interpolate_matrix(a: Matrix, b: Matrix, t: f64) -> Matrix {
    let (da, mut db) = (decompose(a), decompose(b));
    // Turn the shorter way round.
    let mut angles = [da.angle, db.angle];
    unwrap_angles(&mut angles);
    db.angle = angles[1];
    let lerp = |x: f64, y: f64| x + (y - x) * t;
    recompose(Decomposed {
        translate: (
            lerp(da.translate.0, db.translate.0),
            lerp(da.translate.1, db.translate.1),
        ),
        scale: (lerp(da.scale.0, db.scale.0), lerp(da.scale.1, db.scale.1)),
        angle: lerp(da.angle, db.angle),
        skew: lerp(da.skew, db.skew),
    })
}
