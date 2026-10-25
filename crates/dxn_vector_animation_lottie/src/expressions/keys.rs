//! A numeric property's own keyframes, sampled at any frame as lottie-web
//! samples them: each dimension with its easing, holds, and positions along
//! the curves their spatial tangents draw.

use dxn_vector_animation::Easing;
use kurbo::{CubicBez, ParamCurve, ParamCurveArclen};
use serde_json::Value;

use crate::props::{Prop, nums, prop};

/// How closely arc lengths are measured.
const ACCURACY: f64 = 1e-3;

/// A property's values: a constant, or keyframes at frames of its layer.
#[derive(Debug)]
pub(super) struct Keys {
    /// Values per keyframe (the most any keyframe has).
    pub(super) dims: usize,
    pub(super) constant: Vec<f64>,
    pub(super) frames: Vec<KeyFrame>,
}

#[derive(Debug)]
pub(super) struct KeyFrame {
    /// The frame, in the layer's time.
    pub(super) t: f64,
    pub(super) v: Vec<f64>,
    hold: bool,
    /// The easing to the next keyframe, per dimension.
    easings: Vec<Easing>,
    /// The curve to the next keyframe, and its length.
    path: Option<(CubicBez, f64)>,
}

impl Keys {
    pub(super) fn constant(v: Vec<f64>) -> Keys {
        Keys {
            dims: v.len(),
            constant: v,
            frames: Vec::new(),
        }
    }

    /// Whether its values are arrays (more than one number).
    pub(super) fn array(&self) -> bool {
        self.dims > 1
    }

    /// The keyframes of property `p`, or what its values are when they are
    /// not numbers.
    pub(super) fn read(p: &Value) -> Result<Keys, &'static str> {
        let keys = match prop(p) {
            None => return Err("properties without values"),
            Some(Prop::Static(v)) => {
                return nums(v).map(Keys::constant).ok_or_else(|| what(v));
            }
            Some(Prop::Keys(keys)) => keys,
        };
        let mut values = Vec::with_capacity(keys.len());
        for k in &keys {
            values.push(nums(k.value).ok_or_else(|| what(k.value))?);
        }
        let dims = values.iter().map(Vec::len).max().unwrap_or(0);
        let mut frames: Vec<KeyFrame> = Vec::with_capacity(keys.len());
        for (i, (k, v)) in keys.iter().zip(&values).enumerate() {
            let path = match (values.get(i + 1), &k.to, &k.ti) {
                (Some(next), Some(to), Some(ti))
                    if v.len() >= 2
                        && next.len() >= 2
                        && to.iter().chain(ti).take(4).any(|c| *c != 0.0) =>
                {
                    let c = |a: &[f64], i: usize| a.get(i).copied().unwrap_or(0.0);
                    let bez = CubicBez::new(
                        (v[0], v[1]),
                        (v[0] + c(to, 0), v[1] + c(to, 1)),
                        (next[0] + c(ti, 0), next[1] + c(ti, 1)),
                        (next[0], next[1]),
                    );
                    let length = bez.arclen(ACCURACY);
                    length.is_finite().then_some((bez, length))
                }
                _ => None,
            };
            frames.push(KeyFrame {
                t: k.time,
                v: v.clone(),
                hold: k.hold,
                easings: (0..dims.max(1)).map(|d| k.easing(d)).collect(),
                path,
            });
        }
        Ok(Keys {
            dims,
            constant: values.first().cloned().unwrap_or_default(),
            frames,
        })
    }

    /// The values at frame `f` of the layer.
    pub(super) fn sample(&self, f: f64) -> Vec<f64> {
        let k = &self.frames;
        let i = k.partition_point(|x| x.t <= f);
        let (Some(a), Some(b)) = (i.checked_sub(1).and_then(|p| k.get(p)), k.get(i)) else {
            return match (k.first(), k.last()) {
                (Some(first), _) if i == 0 => first.v.clone(),
                (_, Some(last)) => last.v.clone(),
                _ => self.constant.clone(),
            };
        };
        let span = b.t - a.t;
        if span <= 0.0 {
            return b.v.clone();
        }
        if a.hold {
            return a.v.clone();
        }
        let u = ((f - a.t) / span).clamp(0.0, 1.0);
        let mut out: Vec<f64> = (0..self.dims)
            .map(|d| {
                let x = a.v.get(d).copied().unwrap_or(0.0);
                let y = b.v.get(d).copied().unwrap_or(x);
                let easing = a.easings.get(d).unwrap_or(&Easing::Linear);
                x + (y - x) * easing.apply(u)
            })
            .collect();
        if let Some((bez, length)) = &a.path {
            let e = a.easings.first().map_or(u, |e| e.apply(u)).clamp(0.0, 1.0);
            let s = if *length > 0.0 {
                bez.inv_arclen(e * length, ACCURACY)
            } else {
                e
            };
            let p = bez.eval(s);
            if let [x, y, ..] = out.as_mut_slice() {
                (*x, *y) = (p.x, p.y);
            }
        }
        out
    }
}

/// What a value that is not numbers holds.
fn what(v: &Value) -> &'static str {
    let v = match v {
        Value::Array(a) => a.first().unwrap_or(v),
        v => v,
    };
    if v.get("v").is_some() {
        "paths"
    } else if v.get("t").is_some_and(Value::is_string) {
        "text documents"
    } else {
        "values that are not numbers"
    }
}
