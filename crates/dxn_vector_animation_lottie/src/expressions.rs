//! After Effects expressions, evaluated and baked into keyframes before the
//! conversion.
//!
//! Every animatable property holding an expression (`x`) is evaluated at
//! each frame its layer shows, by a small interpreter of the JavaScript
//! subset expressions are written in (Bodymovin's rewritten form included),
//! with After Effects' expression API read from the document: the result
//! becomes the property's keyframes, straight lines between frames, and
//! the expression is removed. An expression that uses something not
//! supported here, or fails, leaves its property as it was, with a warning
//! naming what was not understood.
//!
//! The document is untrusted: evaluation counts its steps against a budget
//! per property and per file, caps recursion, the size of arrays and
//! strings, how deep expressions read one another, and the keyframes it
//! writes (half the file's keyframe limit). Random numbers and wiggles are
//! seeded by the property, so every import gives the same scene.

mod engine;
mod host;
mod keys;
mod syntax;
mod value;

use std::collections::BTreeSet;

use dxn_vector_animation::Limits;
use serde_json::{Map, Value, json};

use engine::{Engine, Fail};
use host::Baked;

/// Bakes the expressions of document `root` into keyframes; the warnings
/// name those left out.
pub(crate) fn bake(root: &mut Value, limits: &Limits) -> Vec<String> {
    let (edits, warnings) = baked(root, limits);
    apply(root, edits);
    warnings
}

/// What [`bake`] writes into `root` (each property's JSON pointer and its
/// baked keyframes), and its warnings.
pub(crate) fn baked(root: &Value, limits: &Limits) -> (Vec<(String, Value)>, Vec<String>) {
    let mut warnings = BTreeSet::new();
    let Some(mut engine) = Engine::new(root) else {
        return (Vec::new(), Vec::new());
    };
    let mut keyframes_left = limits.max_keyframes / 2;
    let mut edits = Vec::new();
    for id in engine.expressions() {
        match engine.bake(id, keyframes_left) {
            Ok(baked) => {
                let Some(original) = engine.json(id) else {
                    continue;
                };
                let (property, written) = baked_property(original, baked);
                keyframes_left = keyframes_left.saturating_sub(written);
                edits.push((engine.props[id].pointer.clone(), property));
            }
            Err(fail) => {
                warnings.insert(warning(&fail));
            }
        }
    }
    (edits, warnings.into_iter().collect())
}

/// `edits` (of [`baked`]) written into `root`.
pub(crate) fn apply(root: &mut Value, edits: Vec<(String, Value)>) {
    for (pointer, property) in edits {
        if let Some(slot) = root.pointer_mut(&pointer) {
            *slot = property;
        }
    }
}

/// Property `original` with the baked values as its keyframes (a constant
/// when they never change), and the values written.
fn baked_property(original: &Value, baked: Baked) -> (Value, usize) {
    let mut property = original.as_object().cloned().unwrap_or_else(Map::new);
    property.remove("x");
    let frames = simplify(baked.frames);
    let constant = frames.windows(2).all(|w| w[0].1 == w[1].1);
    if constant {
        let v = frames.first().map(|f| f.1.as_slice()).unwrap_or(&[]);
        let k = if baked.array {
            json!(v)
        } else {
            json!(v.first().copied().unwrap_or(0.0))
        };
        property.insert("a".into(), json!(0));
        property.insert("k".into(), k);
        return (Value::Object(property), 0);
    }
    let dims = frames.first().map_or(1, |f| f.1.len().max(1));
    let written = frames.len() * dims;
    let keyframes = frames
        .iter()
        .map(|(t, s)| json!({ "t": t, "s": s }))
        .collect();
    property.insert("a".into(), json!(1));
    property.insert("k".into(), Value::Array(keyframes));
    (Value::Object(property), written)
}

/// Frames in time order, without those on the straight line between their
/// neighbours.
fn simplify(mut frames: Vec<(f64, Vec<f64>)>) -> Vec<(f64, Vec<f64>)> {
    frames.sort_by(|a, b| a.0.total_cmp(&b.0));
    frames.dedup_by(|b, a| a.0 == b.0);
    let mut out: Vec<(f64, Vec<f64>)> = Vec::with_capacity(frames.len());
    let mut rest = frames.into_iter().peekable();
    while let Some(frame) = rest.next() {
        if let (Some(a), Some(c)) = (out.last(), rest.peek())
            && on_line(a, &frame, c)
        {
            continue;
        }
        out.push(frame);
    }
    out
}

fn on_line(a: &(f64, Vec<f64>), b: &(f64, Vec<f64>), c: &(f64, Vec<f64>)) -> bool {
    let span = c.0 - a.0;
    if span <= 0.0 {
        return false;
    }
    let f = (b.0 - a.0) / span;
    b.1.iter().enumerate().all(|(d, &v)| {
        let x = a.1.get(d).copied().unwrap_or(v);
        let y = c.1.get(d).copied().unwrap_or(v);
        (v - (x + (y - x) * f)).abs() <= 1e-9 * (1.0 + v.abs())
    })
}

fn warning(fail: &Fail) -> String {
    let tail = "are left out: their properties keep their keyframes";
    match fail {
        Fail::Syntax(what) => format!("expressions that cannot be read ({what}) {tail}"),
        Fail::Unsupported(what) => format!("expressions using {what} {tail}"),
        Fail::Kind(what) => format!("expressions on {what} {tail}"),
        Fail::Error(what) => format!("expressions that fail ({what}) {tail}"),
        Fail::Cycle => format!("expressions that read one another in a cycle {tail}"),
        Fail::Budget => format!("expressions over the evaluation budget {tail}"),
        Fail::Keyframes => format!("expressions over the keyframe limit {tail}"),
    }
}
