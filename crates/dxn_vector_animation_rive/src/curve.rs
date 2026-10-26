//! Rive's keyframes on the scene's timeline, and values computed from them:
//! keyframes with the scene's easings where those draw the same, sampled
//! where they do not (elastic easing, products of moving values, outlines
//! whose tangents turn).

use dxn_vector_animation::{
    Animated, Color, Easing, GradientStop, Interpolate, Keyframe, Matrix, Path, PathEl, Point,
};

/// How a keyframe moves on to the next one, as Rive interpolates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Interp {
    /// Its value until the next keyframe.
    Hold,
    /// Straight.
    Linear,
    /// A cubic Bézier easing of time (`CubicEaseInterpolator`).
    Cubic([f64; 4]),
    /// A cubic Bézier in time and value (`CubicValueInterpolator`): its
    /// control points' y are values, not fractions of the change.
    Value([f64; 4]),
    /// An elastic easing: in (0), out (1) or in and out (2).
    Elastic {
        easing: u64,
        amplitude: f64,
        period: f64,
    },
}

/// A keyframe in the scene's seconds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Key<T> {
    pub t: f64,
    pub value: T,
    pub interp: Interp,
}

/// A value the scene animates, compared to the eye.
pub(crate) trait Close: Interpolate + Clone + PartialEq {
    /// Whether two values are the same to the eye.
    fn close(&self, other: &Self) -> bool;
}

/// A value Rive keys.
pub(crate) trait Keyed: Close {
    /// The value a fraction `f` of the way to `to`, as Rive mixes.
    fn mix(&self, to: &Self, f: f64) -> Self;
    /// The value where a `CubicValueInterpolator` puts it.
    fn value_curve(&self, to: &Self, c: [f64; 4], f: f64) -> Self;
    /// The scene's easing drawing the same from `self` to `to`, when there is
    /// one.
    fn easing(&self, to: &Self, interp: Interp) -> Option<Easing>;
}

impl Keyed for f64 {
    fn mix(&self, to: &Self, f: f64) -> Self {
        self + (to - self) * f
    }

    fn value_curve(&self, to: &Self, c: [f64; 4], f: f64) -> Self {
        let t = solve_x(c[0], c[2], f);
        bezier(*self, c[1], c[3], *to, t)
    }

    fn easing(&self, to: &Self, interp: Interp) -> Option<Easing> {
        match interp {
            Interp::Value(c) => {
                let change = to - self;
                (change.abs() > 1e-9).then(|| {
                    Easing::cubic_bezier(c[0], (c[1] - self) / change, c[2], (c[3] - self) / change)
                })
            }
            other => time_easing(other),
        }
    }
}

impl Close for f64 {
    fn close(&self, other: &Self) -> bool {
        (self - other).abs() <= 1e-4 * self.abs().max(other.abs()).max(1.0)
    }
}

impl Keyed for Color {
    /// Rive mixes colors channel by channel, alpha straight.
    fn mix(&self, to: &Self, f: f64) -> Self {
        let m = |a: f64, b: f64| a + (b - a) * f;
        Color::rgba(
            m(self.r, to.r),
            m(self.g, to.g),
            m(self.b, to.b),
            m(self.a, to.a),
        )
        .clamped()
    }

    /// A value interpolator moves colors straight (Rive's `transform` of one
    /// is the factor itself).
    fn value_curve(&self, to: &Self, _: [f64; 4], f: f64) -> Self {
        self.mix(to, f)
    }

    fn easing(&self, to: &Self, interp: Interp) -> Option<Easing> {
        // The scene mixes colors premultiplied where alphas differ, which
        // draws the same only while the colors do not change.
        let alike = self.a == to.a || (self.r == to.r && self.g == to.g && self.b == to.b);
        match interp {
            Interp::Value(_) if alike => Some(Easing::Linear),
            other if alike => time_easing(other),
            _ => None,
        }
    }
}

impl Close for Color {
    fn close(&self, other: &Self) -> bool {
        [
            self.r - other.r,
            self.g - other.g,
            self.b - other.b,
            self.a - other.a,
        ]
        .iter()
        .all(|d| d.abs() <= 1.0 / 512.0)
    }
}

impl Close for Matrix {
    fn close(&self, other: &Self) -> bool {
        [
            (self.a, other.a),
            (self.b, other.b),
            (self.c, other.c),
            (self.d, other.d),
            (self.e, other.e),
            (self.f, other.f),
        ]
        .iter()
        .all(|(x, y)| x.close(y))
    }
}

impl Close for Point {
    fn close(&self, other: &Self) -> bool {
        self.x.close(&other.x) && self.y.close(&other.y)
    }
}

impl Close for Path {
    fn close(&self, other: &Self) -> bool {
        self.same_structure(other)
            && self
                .elements()
                .iter()
                .zip(other.elements())
                .all(|(a, b)| match (a, b) {
                    (PathEl::MoveTo(a), PathEl::MoveTo(b))
                    | (PathEl::LineTo(a), PathEl::LineTo(b)) => a.close(b),
                    (PathEl::QuadTo(a1, a), PathEl::QuadTo(b1, b)) => a1.close(b1) && a.close(b),
                    (PathEl::CubicTo(a1, a2, a), PathEl::CubicTo(b1, b2, b)) => {
                        a1.close(b1) && a2.close(b2) && a.close(b)
                    }
                    _ => true,
                })
    }
}

impl Close for Vec<GradientStop> {
    fn close(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .zip(other)
                .all(|(a, b)| a.offset.close(&b.offset) && a.color.close(&b.color))
    }
}

fn time_easing(interp: Interp) -> Option<Easing> {
    match interp {
        Interp::Hold => Some(Easing::Hold),
        Interp::Linear => Some(Easing::Linear),
        Interp::Cubic(c) => Some(Easing::cubic_bezier(c[0], c[1], c[2], c[3])),
        Interp::Value(_) | Interp::Elastic { .. } => None,
    }
}

/// A property over the scene's time: its keyframes, or its setup value when
/// nothing keys it.
#[derive(Debug, Clone)]
pub(crate) struct Curve<T> {
    pub setup: T,
    pub keys: Vec<Key<T>>,
}

impl<T: Keyed> Curve<T> {
    /// A value nothing animates.
    pub fn fixed(value: T) -> Self {
        Curve {
            setup: value,
            keys: Vec::new(),
        }
    }

    /// Whether keyframes move it.
    pub fn is_animated(&self) -> bool {
        self.keys.windows(2).any(|w| w[0].value != w[1].value) || self.wanders()
    }

    /// Whether an easing moves it away from equal keyframes.
    fn wanders(&self) -> bool {
        self.keys
            .iter()
            .any(|k| matches!(k.interp, Interp::Value(_) | Interp::Elastic { .. }))
    }

    /// The value at `t`, as Rive's `KeyedProperty::apply` sets it.
    pub fn at(&self, t: f64) -> T {
        let k = &self.keys;
        let i = k.partition_point(|key| key.t <= t);
        if k.is_empty() {
            return self.setup.clone();
        }
        if i == 0 {
            return k[0].value.clone();
        }
        if i >= k.len() {
            return k[k.len() - 1].value.clone();
        }
        let (from, to) = (&k[i - 1], &k[i]);
        if t == from.t || to.t <= from.t {
            return from.value.clone();
        }
        let f = (t - from.t) / (to.t - from.t);
        match from.interp {
            Interp::Hold => from.value.clone(),
            Interp::Linear => from.value.mix(&to.value, f),
            Interp::Cubic(c) => from.value.mix(
                &to.value,
                Easing::cubic_bezier(c[0], c[1], c[2], c[3]).apply(f),
            ),
            Interp::Value(c) => from.value.value_curve(&to.value, c, f),
            Interp::Elastic {
                easing,
                amplitude,
                period,
            } => from
                .value
                .mix(&to.value, elastic(easing, amplitude, period, f)),
        }
    }

    /// Where its keyframes are, and the scene's easing from each.
    pub fn breaks(&self) -> Breaks {
        if !self.is_animated() {
            return Breaks(Vec::new());
        }
        let k = &self.keys;
        let mut out = Vec::with_capacity(k.len());
        for (i, key) in k.iter().enumerate() {
            match k.get(i + 1) {
                Some(next) => out.push(Break {
                    t: key.t,
                    easing: key.value.easing(&next.value, key.interp),
                    moves: key.value != next.value
                        || matches!(key.interp, Interp::Value(_) | Interp::Elastic { .. }),
                }),
                None => out.push(Break {
                    t: key.t,
                    easing: Some(Easing::Hold),
                    moves: false,
                }),
            }
        }
        Breaks(out)
    }
}

/// A keyframe of an input to a computed value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Break {
    pub t: f64,
    /// The scene's easing to the next one, when one draws the same.
    pub easing: Option<Easing>,
    /// Whether the value changes before the next one.
    pub moves: bool,
}

/// An input's keyframes.
#[derive(Debug, Clone, Default)]
pub(crate) struct Breaks(pub Vec<Break>);

impl Breaks {
    /// The keyframes of all of `inputs`.
    pub fn all<'a>(inputs: impl IntoIterator<Item = &'a Breaks>) -> Vec<&'a Breaks> {
        inputs.into_iter().filter(|b| !b.0.is_empty()).collect()
    }
}

/// Turns values computed from keyed inputs into scene keyframes, within a
/// budget of keyframes for the samples.
#[derive(Debug)]
pub(crate) struct Baker {
    /// Samples a second where no easing draws the same.
    pub rate: f64,
    /// Sampled keyframes left.
    pub budget: usize,
    /// Whether the budget ran out (and values went straight between
    /// keyframes instead).
    pub over: bool,
}

impl Baker {
    /// The value `at` computes from `inputs` over time.
    pub fn derive<T: Close>(&mut self, inputs: &[&Breaks], at: impl Fn(f64) -> T) -> Animated<T> {
        let mut times: Vec<f64> = inputs
            .iter()
            .flat_map(|b| b.0.iter().map(|k| k.t))
            .collect();
        times.sort_by(f64::total_cmp);
        times.dedup();
        let Some(&last) = times.last() else {
            return Animated::Static(at(0.0));
        };
        let mut keys = Vec::new();
        // Consecutive sampled spans, simplified together.
        let mut run: Vec<(f64, T)> = Vec::new();
        for w in times.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (va, vb) = (at(a), at(b));
            let candidate = easing_between(inputs, a, b);
            if let Some(e) = candidate
                && [0.25, 0.5, 0.75].iter().all(|u| {
                    let expected = if matches!(e, Easing::Hold) {
                        va.clone()
                    } else {
                        va.interpolate(&vb, e.apply(*u))
                    };
                    at(a + (b - a) * u).close(&expected)
                })
            {
                self.flush(&mut run, (a, va.clone()), &mut keys);
                keys.push(Keyframe::new(a, va, e));
                continue;
            }
            let n = (((b - a) * self.rate).ceil() as usize).max(1);
            if n > self.budget {
                self.over = true;
                self.flush(&mut run, (a, va.clone()), &mut keys);
                keys.push(Keyframe::new(a, va, Easing::Linear));
                continue;
            }
            run.push((a, va));
            for i in 1..n {
                let t = a + (b - a) * i as f64 / n as f64;
                run.push((t, at(t)));
            }
        }
        self.flush(&mut run, (last, at(last)), &mut keys);
        keys.push(Keyframe::new(last, at(last), Easing::Hold));
        let fallback = at(0.0);
        Animated::from_keyframes(keys, fallback)
    }

    /// The samples of `run` a straight line between their neighbors does
    /// not draw the same, as straight keyframes, `end` the sample after
    /// them; empties `run`.
    fn flush<T: Close>(
        &mut self,
        run: &mut Vec<(f64, T)>,
        end: (f64, T),
        keys: &mut Vec<Keyframe<T>>,
    ) {
        if run.is_empty() {
            return;
        }
        run.push(end);
        let kept = straight_runs(run);
        self.budget = self.budget.saturating_sub(kept.len());
        for &i in &kept {
            let (t, v) = &run[i];
            keys.push(Keyframe::new(*t, v.clone(), Easing::Linear));
        }
        run.clear();
    }

    /// A keyed value on its own.
    pub fn curve<T: Keyed>(&mut self, curve: &Curve<T>) -> Animated<T> {
        let breaks = curve.breaks();
        self.derive(&Breaks::all([&breaks]), |t| curve.at(t))
    }
}

/// The samples to keep of `samples` (the last left for what follows):
/// those a straight line between the kept ones around them does not draw
/// the same, looking at most 32 samples ahead.
fn straight_runs<T: Close>(samples: &[(f64, T)]) -> Vec<usize> {
    const AHEAD: usize = 32;
    let last = samples.len() - 1;
    let mut kept = vec![0];
    let mut from = 0;
    while from < last {
        let mut to = from + 1;
        for j in from + 2..=(from + AHEAD).min(last) {
            let (ta, va) = &samples[from];
            let (tb, vb) = &samples[j];
            let straight = samples[from + 1..j].iter().all(|(t, v)| {
                let u = (t - ta) / (tb - ta);
                v.close(&va.interpolate(vb, u))
            });
            if !straight {
                break;
            }
            to = j;
        }
        if to < last {
            kept.push(to);
        }
        from = to;
    }
    kept
}

/// The easing every input moving between `a` and `b` agrees on: `Hold` when
/// none moves, `None` when they differ or one has no easing of the scene.
fn easing_between(inputs: &[&Breaks], a: f64, b: f64) -> Option<Easing> {
    let mut agreed: Option<Easing> = None;
    for input in inputs {
        let k = &input.0;
        let i = k.partition_point(|key| key.t <= a);
        if i == 0 || i >= k.len() {
            continue;
        }
        let (from, to) = (&k[i - 1], &k[i]);
        if !from.moves {
            continue;
        }
        let easing = from.easing.clone()?;
        // An easing drawn over part of its span is another curve; straight
        // lines and holds stay the same.
        let whole = from.t == a && to.t == b;
        if !whole && !matches!(easing, Easing::Linear | Easing::Hold) {
            return None;
        }
        match &agreed {
            None => agreed = Some(easing),
            Some(e) if *e == easing => {}
            Some(_) => return None,
        }
    }
    Some(agreed.unwrap_or(Easing::Hold))
}

/// The cubic easing drawing the part of easing `c` (x1, y1, x2, y2) from
/// `fa` to `fb` of its time, the change in that part made 0 to 1: a part of
/// a cubic Bézier is one too. `None` when the value does not change there.
pub(crate) fn sub_easing(c: [f64; 4], fa: f64, fb: f64) -> Option<[f64; 4]> {
    let (ta, tb) = (solve_x(c[0], c[2], fa), solve_x(c[0], c[2], fb));
    let p = [(0.0, 0.0), (c[0], c[1]), (c[2], c[3]), (1.0, 1.0)];
    let lerp =
        |a: (f64, f64), b: (f64, f64), t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    // The curve's part from `t0` to `t1`, by de Casteljau's construction.
    let split = |q: [(f64, f64); 4], t: f64| {
        let (a, b, c) = (
            lerp(q[0], q[1], t),
            lerp(q[1], q[2], t),
            lerp(q[2], q[3], t),
        );
        let (d, e) = (lerp(a, b, t), lerp(b, c, t));
        let f = lerp(d, e, t);
        ([q[0], a, d, f], [f, e, c, q[3]])
    };
    let (left, _) = split(p, tb);
    let part = if tb > 0.0 {
        split(left, ta / tb).1
    } else {
        left
    };
    let (w, h) = (part[3].0 - part[0].0, part[3].1 - part[0].1);
    if w <= 0.0 || h.abs() < 1e-9 {
        return None;
    }
    let x = |q: (f64, f64)| (q.0 - part[0].0) / w;
    let y = |q: (f64, f64)| (q.1 - part[0].1) / h;
    Some([x(part[1]), y(part[1]), x(part[2]), y(part[2])])
}

/// The y of the cubic Bézier from `p0` to `p3` at parameter `t`.
pub(crate) fn bezier(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let u = 1.0 - t;
    u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3
}

/// The parameter where the time Bézier through (0, `x1`, `x2`, 1) is `x`.
pub(crate) fn solve_x(x1: f64, x2: f64, x: f64) -> f64 {
    let (x1, x2) = (x1.clamp(0.0, 1.0), x2.clamp(0.0, 1.0));
    let (mut lo, mut hi) = (0.0, 1.0);
    let mut t = x;
    for _ in 0..8 {
        let err = bezier(0.0, x1, x2, 1.0, t) - x;
        if err.abs() < 1e-7 {
            return t;
        }
        let u = 1.0 - t;
        let d = 3.0 * u * u * x1 + 6.0 * u * t * (x2 - x1) + 3.0 * t * t * (1.0 - x2);
        if d.abs() < 1e-6 {
            break;
        }
        t -= err / d;
    }
    t = x.clamp(0.0, 1.0);
    for _ in 0..40 {
        let v = bezier(0.0, x1, x2, 1.0, t);
        if (v - x).abs() < 1e-7 {
            break;
        }
        if v < x {
            lo = t;
        } else {
            hi = t;
        }
        t = (lo + hi) / 2.0;
    }
    t
}

/// Rive's elastic easing (`ElasticEase`) at `f`.
fn elastic(easing: u64, amplitude: f64, period: f64, f: f64) -> f64 {
    use std::f64::consts::PI;
    let period = if period == 0.0 { 0.5 } else { period };
    let s = if amplitude < 1.0 {
        period / 4.0
    } else {
        period / (2.0 * PI) * (1.0 / amplitude).asin()
    };
    let actual = |time: f64| {
        if amplitude < 1.0 {
            let t = s.abs();
            let abs = time.abs();
            if abs < t {
                let l = abs / t;
                return amplitude * l + (1.0 - l);
            }
        }
        amplitude
    };
    let wave = |time: f64| ((time - s) * (2.0 * PI) / period).sin();
    match easing {
        0 => {
            let time = f - 1.0;
            -(actual(time) * 2f64.powf(10.0 * time) * ((-time - s) * (2.0 * PI) / period).sin())
        }
        1 => actual(f) * 2f64.powf(-10.0 * f) * wave(f) + 1.0,
        2 => {
            let time = f * 2.0 - 1.0;
            if time < 0.0 {
                -0.5 * actual(time)
                    * 2f64.powf(10.0 * time)
                    * ((-time - s) * (2.0 * PI) / period).sin()
            } else {
                0.5 * (actual(time) * 2f64.powf(-10.0 * time) * wave(time)) + 1.0
            }
        }
        _ => f,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elastic_ends_where_it_should() {
        for e in 0..3 {
            assert!(elastic(e, 1.0, 0.5, 0.0).abs() < 0.01, "{e}");
            assert!((elastic(e, 1.0, 0.5, 1.0) - 1.0).abs() < 0.01, "{e}");
        }
    }

    #[test]
    fn parts_of_cubic_easings_are_cubic_easings() {
        let c = [0.42, 0.0, 0.58, 1.0];
        let e = Easing::cubic_bezier(c[0], c[1], c[2], c[3]);
        let (fa, fb) = (0.2, 0.7);
        let part = sub_easing(c, fa, fb).unwrap();
        let p = Easing::cubic_bezier(part[0], part[1], part[2], part[3]);
        let (ya, yb) = (e.apply(fa), e.apply(fb));
        for u in [0.1, 0.35, 0.6, 0.9] {
            let expected = (e.apply(fa + (fb - fa) * u) - ya) / (yb - ya);
            assert!((p.apply(u) - expected).abs() < 1e-4, "{u}");
        }
    }

    #[test]
    fn value_curves_normalize_to_easings() {
        let c = [0.3, 40.0, 0.7, 90.0];
        let curve = Curve {
            setup: 0.0,
            keys: vec![
                Key {
                    t: 0.0,
                    value: 20.0,
                    interp: Interp::Value(c),
                },
                Key {
                    t: 1.0,
                    value: 100.0,
                    interp: Interp::Hold,
                },
            ],
        };
        let e = 20.0f64.easing(&100.0, Interp::Value(c)).unwrap();
        for u in [0.1, 0.4, 0.8] {
            let expected = 20.0 + 80.0 * e.apply(u);
            assert!((curve.at(u) - expected).abs() < 1e-3, "{u}");
        }
    }
}
