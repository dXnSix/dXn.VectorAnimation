//! Animated properties: a constant, or keyframes on the timeline with an
//! easing per segment.

use crate::{Color, Easing, Matrix, Path, Point};

/// A property: a constant, or a [`Track`] of keyframes.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum Animated<T> {
    /// The same value at every time.
    Static(T),
    /// Keyframes.
    Track(Track<T>),
}

impl<T: Default> Default for Animated<T> {
    fn default() -> Self {
        Animated::Static(T::default())
    }
}

impl<T> From<T> for Animated<T> {
    fn from(value: T) -> Self {
        Animated::Static(value)
    }
}

impl<T: Interpolate> Animated<T> {
    /// The value at `time` (seconds, in the timeline of the node holding
    /// it).
    pub fn sample(&self, time: f64) -> T {
        match self {
            Animated::Static(v) => v.clone(),
            Animated::Track(track) => track.sample(time),
        }
    }

    /// A track of `keyframes`, or the constant when they hold one value
    /// (none: `fallback`).
    pub fn from_keyframes(keyframes: Vec<Keyframe<T>>, fallback: T) -> Self
    where
        T: PartialEq,
    {
        match keyframes.as_slice() {
            [] => Animated::Static(fallback),
            [only] => Animated::Static(only.value.clone()),
            [first, rest @ ..] if rest.iter().all(|k| k.value == first.value) => {
                Animated::Static(first.value.clone())
            }
            _ => Animated::Track(Track::new(keyframes)),
        }
    }
}

impl<T> Animated<T> {
    /// Whether it changes over time.
    pub fn is_animated(&self) -> bool {
        matches!(self, Animated::Track(_))
    }

    /// Its keyframes (none for a constant).
    pub fn keyframes(&self) -> &[Keyframe<T>] {
        match self {
            Animated::Static(_) => &[],
            Animated::Track(track) => &track.keyframes,
        }
    }

    /// The value when it is a constant.
    pub fn as_static(&self) -> Option<&T> {
        match self {
            Animated::Static(v) => Some(v),
            Animated::Track(_) => None,
        }
    }
}

/// Keyframes, ascending in time. Before the first keyframe the property
/// holds its value, and after the last the last one's.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Track<T> {
    /// The keyframes, ascending in time.
    pub keyframes: Vec<Keyframe<T>>,
}

impl<T> Track<T> {
    /// A track of `keyframes`, sorted by time (stable: keyframes at the
    /// same time keep their order, the later one wins from that time on).
    pub fn new(mut keyframes: Vec<Keyframe<T>>) -> Self {
        keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
        Track { keyframes }
    }
}

impl<T: Interpolate> Track<T> {
    /// The value at `time`.
    pub fn sample(&self, time: f64) -> T {
        let k = &self.keyframes;
        let i = k.partition_point(|kf| kf.time <= time);
        match (i, k.len()) {
            (_, 0) => T::default(),
            (0, _) => k[0].value.clone(),
            (i, n) if i >= n => k[n - 1].value.clone(),
            (i, _) => {
                let (a, b) = (&k[i - 1], &k[i]);
                let span = b.time - a.time;
                if span <= 0.0 {
                    return b.value.clone();
                }
                let progress = ((time - a.time) / span).clamp(0.0, 1.0);
                let eased = a.easing.apply(progress);
                if matches!(a.easing, Easing::Hold) || eased == 0.0 {
                    return a.value.clone();
                }
                a.value.interpolate(&b.value, eased)
            }
        }
    }
}

/// A value at a time, and how the property moves from it to the next
/// keyframe.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Keyframe<T> {
    /// Seconds.
    pub time: f64,
    /// The value at that time.
    pub value: T,
    /// The curve of the segment from this keyframe to the next.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Easing::is_linear")
    )]
    pub easing: Easing,
}

impl<T> Keyframe<T> {
    /// `value` at `time`, moving on with `easing`.
    pub fn new(time: f64, value: T, easing: Easing) -> Self {
        Keyframe {
            time,
            value,
            easing,
        }
    }
}

/// Values a keyframe segment moves between. An empty track samples as the
/// default value.
pub trait Interpolate: Clone + Default {
    /// The value a fraction `t` of the way to `to` (`t` may leave 0..1
    /// where an easing overshoots).
    fn interpolate(&self, to: &Self, t: f64) -> Self;
}

impl Interpolate for f64 {
    fn interpolate(&self, to: &Self, t: f64) -> Self {
        self + (to - self) * t
    }
}

impl Interpolate for bool {
    fn interpolate(&self, to: &Self, t: f64) -> Self {
        if t < 1.0 { *self } else { *to }
    }
}

impl Interpolate for Point {
    fn interpolate(&self, to: &Self, t: f64) -> Self {
        self.lerp(*to, t)
    }
}

impl Interpolate for Color {
    fn interpolate(&self, to: &Self, t: f64) -> Self {
        self.lerp(*to, t)
    }
}

/// Paths of the same commands move point by point; others switch halfway,
/// as CSS interpolates values it cannot mix.
impl Interpolate for Path {
    fn interpolate(&self, to: &Self, t: f64) -> Self {
        if !self.same_structure(to) {
            return if t < 0.5 { self.clone() } else { to.clone() };
        }
        use crate::PathEl::*;
        let lerp = |a: Point, b: Point| a.lerp(b, t);
        Path(
            self.0
                .iter()
                .zip(&to.0)
                .map(|(a, b)| match (*a, *b) {
                    (MoveTo(p), MoveTo(q)) => MoveTo(lerp(p, q)),
                    (LineTo(p), LineTo(q)) => LineTo(lerp(p, q)),
                    (QuadTo(c, p), QuadTo(d, q)) => QuadTo(lerp(c, d), lerp(p, q)),
                    (CubicTo(c1, c2, p), CubicTo(d1, d2, q)) => {
                        CubicTo(lerp(c1, d1), lerp(c2, d2), lerp(p, q))
                    }
                    (a, _) => a,
                })
                .collect(),
        )
    }
}

/// Matrices move through their decomposition (translation, rotation,
/// scale, skew), as CSS interpolates transform lists it cannot match
/// function by function.
impl Interpolate for Matrix {
    fn interpolate(&self, to: &Self, t: f64) -> Self {
        crate::transform::interpolate_matrix(*self, *to, t)
    }
}

/// Lists of the same length move item by item; others switch halfway.
impl<T: Interpolate> Interpolate for Vec<T> {
    fn interpolate(&self, to: &Self, t: f64) -> Self {
        if self.len() != to.len() {
            return if t < 0.5 { self.clone() } else { to.clone() };
        }
        self.iter()
            .zip(to)
            .map(|(a, b)| a.interpolate(b, t))
            .collect()
    }
}

/// A point whose coordinates animate on their own (each may have its own
/// keyframes and easings).
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct AnimatedPoint {
    /// x.
    pub x: Animated<f64>,
    /// y.
    pub y: Animated<f64>,
}

impl AnimatedPoint {
    /// The constant point (`x`, `y`).
    pub fn fixed(x: f64, y: f64) -> Self {
        AnimatedPoint {
            x: Animated::Static(x),
            y: Animated::Static(y),
        }
    }

    /// The point at `time`.
    pub fn sample(&self, time: f64) -> Point {
        Point::new(self.x.sample(time), self.y.sample(time))
    }

    /// Whether either coordinate changes.
    pub fn is_animated(&self) -> bool {
        self.x.is_animated() || self.y.is_animated()
    }
}
