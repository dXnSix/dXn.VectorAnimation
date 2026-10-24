//! Easing curves: how a keyframe segment moves from its start value to its
//! end value.

/// How a keyframe segment moves from its start value to the next keyframe's
/// value: a function of the segment's progress (0 to 1) giving the fraction
/// of the change applied.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", tag = "type"))]
pub enum Easing {
    /// Constant speed.
    #[default]
    Linear,
    /// A cubic Bézier curve from (0, 0) to (1, 1) through two control
    /// points, as CSS `cubic-bezier(x1, y1, x2, y2)` and Lottie's out / in
    /// tangents. `x1` and `x2` are within 0..1; `y1` and `y2` may overshoot.
    CubicBezier {
        /// First control point's x.
        x1: f64,
        /// First control point's y.
        y1: f64,
        /// Second control point's x.
        x2: f64,
        /// Second control point's y.
        y2: f64,
    },
    /// The start value until the next keyframe, then its value (Lottie's
    /// hold keyframes, Figma's `HOLD`).
    Hold,
    /// CSS `steps(count, position)`: jumps between `count` levels.
    Steps {
        /// The number of intervals.
        count: u32,
        /// Where the jumps happen.
        position: StepPosition,
    },
    /// CSS `linear()`: straight lines through points (input, output), the
    /// inputs ascending.
    LinearPoints {
        /// The points: (progress in, fraction out).
        points: Vec<(f64, f64)>,
    },
}

/// Where [`Easing::Steps`] jumps (CSS `<step-position>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum StepPosition {
    /// The first jump is at the start (`jump-start`, `start`).
    JumpStart,
    /// The last jump is at the end (`jump-end`, `end`).
    #[default]
    JumpEnd,
    /// No jump at either end (`jump-none`).
    JumpNone,
    /// Jumps at both ends (`jump-both`).
    JumpBoth,
}

impl Easing {
    /// CSS `ease`.
    pub const EASE: Easing = Easing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.25,
        y2: 1.0,
    };
    /// CSS `ease-in`.
    pub const EASE_IN: Easing = Easing::CubicBezier {
        x1: 0.42,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    };
    /// CSS `ease-out`.
    pub const EASE_OUT: Easing = Easing::CubicBezier {
        x1: 0.0,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    };
    /// CSS `ease-in-out`.
    pub const EASE_IN_OUT: Easing = Easing::CubicBezier {
        x1: 0.42,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    };

    /// A cubic Bézier easing; the control points' x are clamped to 0..1.
    pub fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64) -> Easing {
        let (x1, x2) = (x1.clamp(0.0, 1.0), x2.clamp(0.0, 1.0));
        if x1 == y1 && x2 == y2 {
            return Easing::Linear;
        }
        Easing::CubicBezier { x1, y1, x2, y2 }
    }

    /// The fraction of the change applied at progress `t` (0 to 1).
    pub fn apply(&self, t: f64) -> f64 {
        match self {
            Easing::Linear => t,
            Easing::CubicBezier { x1, y1, x2, y2 } => cubic_bezier(*x1, *y1, *x2, *y2, t),
            Easing::Hold => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Easing::Steps { count, position } => steps(*count, *position, t),
            Easing::LinearPoints { points } => linear_points(points, t),
        }
    }

    /// Whether the curve is a straight line from (0, 0) to (1, 1).
    pub fn is_linear(&self) -> bool {
        matches!(self, Easing::Linear)
    }
}

/// The y of the cubic Bézier through (0, 0), (`x1`, `y1`), (`x2`, `y2`),
/// (1, 1) where its x is `t`.
fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, t: f64) -> f64 {
    if t <= 0.0 {
        return if x1 > 0.0 {
            y1 / x1 * t
        } else if y1 == 0.0 && x2 > 0.0 {
            y2 / x2 * t
        } else {
            0.0
        };
    }
    if t >= 1.0 {
        return if x2 < 1.0 {
            1.0 + (y2 - 1.0) / (x2 - 1.0) * (t - 1.0)
        } else {
            1.0
        };
    }
    // Polynomial coefficients of x(s) and y(s).
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let x_at = |s: f64| ((ax * s + bx) * s + cx) * s;
    let dx_at = |s: f64| (3.0 * ax * s + 2.0 * bx) * s + cx;
    let y_at = |s: f64| ((ay * s + by) * s + cy) * s;

    // Newton's method first, bisection when it does not converge.
    let mut s = t;
    for _ in 0..8 {
        let err = x_at(s) - t;
        if err.abs() < 1e-7 {
            return y_at(s);
        }
        let d = dx_at(s);
        if d.abs() < 1e-6 {
            break;
        }
        s -= err / d;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    s = t;
    for _ in 0..64 {
        let x = x_at(s);
        if (x - t).abs() < 1e-7 {
            break;
        }
        if x < t {
            lo = s;
        } else {
            hi = s;
        }
        s = (lo + hi) / 2.0;
    }
    y_at(s)
}

/// CSS `steps()` (Easing Functions Level 1, without the before flag).
fn steps(count: u32, position: StepPosition, t: f64) -> f64 {
    let count = count.max(1);
    let n = f64::from(count);
    let mut step = (t * n).floor();
    if matches!(position, StepPosition::JumpStart | StepPosition::JumpBoth) {
        step += 1.0;
    }
    let jumps = match position {
        StepPosition::JumpBoth => n + 1.0,
        StepPosition::JumpNone if count > 1 => n - 1.0,
        StepPosition::JumpNone => 1.0,
        _ => n,
    };
    if (0.0..=1.0).contains(&t) {
        step = step.clamp(0.0, jumps);
    }
    step / jumps
}

/// CSS `linear()`: straight lines through the points; outside them the end
/// segments extend.
fn linear_points(points: &[(f64, f64)], t: f64) -> f64 {
    match points {
        [] => t,
        [(_, y)] => *y,
        _ => {
            let i = points
                .windows(2)
                .position(|w| t <= w[1].0)
                .unwrap_or(points.len() - 2);
            let ((x0, y0), (x1, y1)) = (points[i], points[i + 1]);
            if x1 <= x0 {
                return if t < x1 { y0 } else { y1 };
            }
            y0 + (y1 - y0) * (t - x0) / (x1 - x0)
        }
    }
}
