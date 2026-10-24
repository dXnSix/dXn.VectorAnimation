//! Lottie's animatable properties (`{"a": 1, "k": [keyframes]}`) turned into
//! tracks: values per dimension, easings from the out and in tangents, hold
//! keyframes, spatial position tangents.

use dxn_vector_animation::{
    Animated, Color, Easing, GradientStop, Keyframe, Path, PathEl, Point, Track,
};
use serde_json::Value;

/// A number, or the first of an array of them.
pub(crate) fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|n| n.is_finite()),
        Value::Array(a) => a.first().and_then(num),
        _ => None,
    }
}

/// The numbers of a value (one for a number).
pub(crate) fn nums(v: &Value) -> Option<Vec<f64>> {
    match v {
        Value::Number(_) => num(v).map(|n| vec![n]),
        Value::Array(a) => a.iter().map(num).collect(),
        _ => None,
    }
}

/// One keyframe as the file writes it.
#[derive(Debug, Clone)]
pub(crate) struct RawKey<'a> {
    /// Frames.
    pub(crate) time: f64,
    pub(crate) value: &'a Value,
    pub(crate) hold: bool,
    pub(crate) out_tangent: Option<&'a Value>,
    pub(crate) in_tangent: Option<&'a Value>,
    /// Spatial tangents of a position's segment to the next keyframe.
    pub(crate) to: Option<Vec<f64>>,
    pub(crate) ti: Option<Vec<f64>>,
}

impl RawKey<'_> {
    /// The easing of the segment after this keyframe, in dimension `dim`.
    pub(crate) fn easing(&self, dim: usize) -> Easing {
        if self.hold {
            return Easing::Hold;
        }
        let comp = |t: Option<&Value>, axis: &str| -> Option<f64> {
            match t?.get(axis)? {
                Value::Array(a) => a.get(dim).or_else(|| a.first()).and_then(num),
                v => num(v),
            }
        };
        match (
            comp(self.out_tangent, "x"),
            comp(self.out_tangent, "y"),
            comp(self.in_tangent, "x"),
            comp(self.in_tangent, "y"),
        ) {
            (Some(x1), Some(y1), Some(x2), Some(y2)) => Easing::cubic_bezier(x1, y1, x2, y2),
            _ => Easing::Linear,
        }
    }
}

/// A property: its constant value, or its keyframes.
pub(crate) enum Prop<'a> {
    Static(&'a Value),
    Keys(Vec<RawKey<'a>>),
}

/// Reads property `p` (`{"a": 0|1, "k": …}`); `None` when it is not one.
pub(crate) fn prop(p: &Value) -> Option<Prop<'_>> {
    let k = p.get("k")?;
    let animated = match p.get("a") {
        Some(a) => num(a).is_some_and(|a| a != 0.0),
        // Files that leave `a` out mark keyframes by their objects.
        None => matches!(k, Value::Array(a) if a.first().is_some_and(|f| f.get("t").is_some())),
    };
    if !animated {
        return Some(Prop::Static(k));
    }
    let list = k.as_array()?;
    let mut keys: Vec<RawKey<'_>> = Vec::with_capacity(list.len());
    let mut previous_end: Option<&Value> = None;
    for key in list {
        let time = key.get("t").and_then(num)?;
        // Older files give a segment's end (`e`) and leave the last
        // keyframe's value out.
        let value = match key.get("s").or(previous_end) {
            Some(v) => v,
            None => continue,
        };
        previous_end = key.get("e");
        keys.push(RawKey {
            time,
            value,
            hold: key.get("h").and_then(num).is_some_and(|h| h != 0.0),
            out_tangent: key.get("o"),
            in_tangent: key.get("i"),
            to: key.get("to").and_then(nums),
            ti: key.get("ti").and_then(nums),
        });
    }
    if keys.is_empty() {
        return None;
    }
    Some(Prop::Keys(keys))
}

/// Whether property `p` holds an expression (which is left out).
pub(crate) fn has_expression(p: &Value) -> bool {
    p.get("x").is_some_and(|x| x.is_string())
}

/// Dimension `dim` of a numeric property, times `scale`, with keyframe
/// times in seconds (`fr` frames a second).
pub(crate) fn scalar(
    p: Option<&Value>,
    dim: usize,
    scale: f64,
    default: f64,
    fr: f64,
) -> Animated<f64> {
    let Some(prop) = p.and_then(prop) else {
        return Animated::Static(default);
    };
    let component = |v: &Value| -> Option<f64> {
        let all = nums(v)?;
        all.get(dim).or_else(|| all.first()).map(|n| n * scale)
    };
    match prop {
        Prop::Static(v) => Animated::Static(component(v).unwrap_or(default)),
        Prop::Keys(keys) => Animated::from_keyframes(
            keys.iter()
                .filter_map(|k| {
                    Some(Keyframe::new(
                        k.time / fr,
                        component(k.value)?,
                        k.easing(dim),
                    ))
                })
                .collect(),
            default,
        ),
    }
}

/// A color property (`[r, g, b, a]`, 0 to 1; old files 0 to 255).
pub(crate) fn color(p: Option<&Value>, fr: f64) -> Animated<Color> {
    let to_color = |v: &Value| -> Option<Color> {
        let c = nums(v)?;
        let max = c.iter().take(3).copied().fold(0.0, f64::max);
        let s = if max > 1.0 { 255.0 } else { 1.0 };
        Some(
            Color::rgba(
                *c.first()? / s,
                *c.get(1)? / s,
                *c.get(2)? / s,
                c.get(3)
                    .map_or(1.0, |a| a / if s > 1.0 { 255.0 } else { 1.0 }),
            )
            .clamped(),
        )
    };
    match p.and_then(prop) {
        None => Animated::Static(Color::BLACK),
        Some(Prop::Static(v)) => Animated::Static(to_color(v).unwrap_or(Color::BLACK)),
        Some(Prop::Keys(keys)) => Animated::from_keyframes(
            keys.iter()
                .filter_map(|k| Some(Keyframe::new(k.time / fr, to_color(k.value)?, k.easing(0))))
                .collect(),
            Color::BLACK,
        ),
    }
}

/// A Bézier shape (`{"c": closed, "v": vertices, "i": in, "o": out}`, the
/// tangents relative to their vertices) as a path.
pub(crate) fn bezier(shape: &Value) -> Option<Path> {
    // Keyframe values wrap the shape in an array.
    let shape = match shape {
        Value::Array(a) => a.first()?,
        s => s,
    };
    let points = |key: &str| -> Option<Vec<Point>> {
        shape
            .get(key)?
            .as_array()?
            .iter()
            .map(|p| {
                let v = nums(p)?;
                Some(Point::new(*v.first()?, *v.get(1)?))
            })
            .collect()
    };
    let v = points("v")?;
    let i = points("i").unwrap_or_else(|| vec![Point::ZERO; v.len()]);
    let o = points("o").unwrap_or_else(|| vec![Point::ZERO; v.len()]);
    let closed = shape.get("c").is_some_and(|c| c.as_bool().unwrap_or(false));
    let mut path = Path::new();
    let Some(&first) = v.first() else {
        return Some(path);
    };
    path.move_to(first);
    let segment = |path: &mut Path, a: usize, b: usize| {
        let (oa, ib) = (
            o.get(a).copied().unwrap_or_default(),
            i.get(b).copied().unwrap_or_default(),
        );
        if oa == Point::ZERO && ib == Point::ZERO {
            path.line_to(v[b]);
        } else {
            path.cubic_to(v[a] + oa, v[b] + ib, v[b]);
        }
    };
    for k in 0..v.len().saturating_sub(1) {
        segment(&mut path, k, k + 1);
    }
    if closed && v.len() > 1 {
        segment(&mut path, v.len() - 1, 0);
        path.close();
    } else if closed {
        path.close();
    }
    Some(path)
}

/// A shape property: the outline, morphing between keyframes.
pub(crate) fn shape(p: Option<&Value>, fr: f64) -> Option<Animated<Path>> {
    match p.and_then(prop)? {
        Prop::Static(v) => bezier(v).map(Animated::Static),
        Prop::Keys(keys) => {
            let frames: Vec<Keyframe<Path>> = keys
                .iter()
                .filter_map(|k| Some(Keyframe::new(k.time / fr, bezier(k.value)?, k.easing(0))))
                .collect();
            // Lottie draws vertices as tangents; normalize line segments of
            // one keyframe against curves of another so they morph.
            Some(Animated::from_keyframes(normalize(frames), Path::new()))
        }
    }
}

/// Paths of the same vertices with lines in some and curves in others,
/// as curves throughout, so they interpolate point by point.
fn normalize(frames: Vec<Keyframe<Path>>) -> Vec<Keyframe<Path>> {
    let as_cubics = |p: &Path| -> Path {
        let mut out = Path::new();
        let mut current = Point::ZERO;
        let mut start = Point::ZERO;
        for el in p.elements() {
            match *el {
                PathEl::MoveTo(a) => {
                    out.move_to(a);
                    current = a;
                    start = a;
                }
                PathEl::LineTo(a) => {
                    out.cubic_to(current, a, a);
                    current = a;
                }
                PathEl::CubicTo(c1, c2, a) => {
                    out.cubic_to(c1, c2, a);
                    current = a;
                }
                PathEl::QuadTo(c, a) => {
                    out.quad_to(c, a);
                    current = a;
                }
                PathEl::Close => {
                    out.close();
                    current = start;
                }
            }
        }
        out
    };
    let mixed = frames
        .windows(2)
        .any(|w| !w[0].value.same_structure(&w[1].value));
    if !mixed {
        return frames;
    }
    frames
        .into_iter()
        .map(|k| Keyframe::new(k.time, as_cubics(&k.value), k.easing))
        .collect()
}

/// A gradient's colors (`{"p": count, "k": [offset, r, g, b, …, offset,
/// alpha, …]}`): the color stops, with the alpha stops' opacity at each.
pub(crate) fn gradient_stops(g: Option<&Value>, fr: f64) -> Animated<Vec<GradientStop>> {
    let Some(g) = g else {
        return Animated::Static(Vec::new());
    };
    let count = g.get("p").and_then(num).unwrap_or(0.0).max(0.0) as usize;
    let to_stops = |v: &Value| -> Option<Vec<GradientStop>> {
        let flat = nums(v)?;
        let colors = flat.get(..count * 4)?;
        let alphas: Vec<(f64, f64)> = flat[count * 4..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| (c[0], c[1]))
            .collect();
        let alpha_at = |offset: f64| -> f64 {
            match alphas.as_slice() {
                [] => 1.0,
                [(_, a)] => *a,
                list => {
                    let i = list
                        .iter()
                        .position(|(o, _)| *o >= offset)
                        .unwrap_or(list.len() - 1);
                    if i == 0 {
                        return list[0].1;
                    }
                    let ((o0, a0), (o1, a1)) = (list[i - 1], list[i]);
                    if o1 <= o0 || offset >= o1 {
                        a1
                    } else {
                        a0 + (a1 - a0) * (offset - o0) / (o1 - o0)
                    }
                }
            }
        };
        Some(
            colors
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| GradientStop {
                    offset: c[0].clamp(0.0, 1.0),
                    color: Color::rgba(c[1], c[2], c[3], alpha_at(c[0])).clamped(),
                })
                .collect(),
        )
    };
    match g.get("k").and_then(prop) {
        None => Animated::Static(Vec::new()),
        Some(Prop::Static(v)) => Animated::Static(to_stops(v).unwrap_or_default()),
        Some(Prop::Keys(keys)) => Animated::from_keyframes(
            keys.iter()
                .filter_map(|k| Some(Keyframe::new(k.time / fr, to_stops(k.value)?, k.easing(0))))
                .collect(),
            Vec::new(),
        ),
    }
}

/// The keyframe times (seconds) of property `p`.
pub(crate) fn key_times(p: Option<&Value>, fr: f64) -> Vec<f64> {
    match p.and_then(prop) {
        Some(Prop::Keys(keys)) => keys.iter().map(|k| k.time / fr).collect(),
        _ => Vec::new(),
    }
}

/// A track sampling `f` at `times` (linear between them), or the constant
/// at time 0 when there are none.
pub(crate) fn sampled(times: &[f64], f: impl Fn(f64) -> f64) -> Animated<f64> {
    let mut times = times.to_vec();
    times.sort_by(f64::total_cmp);
    times.dedup();
    if times.len() < 2 {
        return Animated::Static(f(times.first().copied().unwrap_or(0.0)));
    }
    Animated::Track(Track::new(
        times
            .iter()
            .map(|&t| Keyframe::new(t, f(t), Easing::Linear))
            .collect(),
    ))
}
