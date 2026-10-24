//! CSS animations turned into keyframes on the scene's timeline: each
//! animated property's `@keyframes` stops, unrolled over the animation's
//! delay, iterations, direction and fill mode.

use std::collections::HashMap;

use dxn_vector_animation::{Easing, Keyframe, StepPosition};

use crate::{
    css::Stylesheet,
    values::{AnimationSpec, FillMode, Iterations, PlayDirection},
};

/// At most this many iterations of one animation are unrolled.
const MAX_ITERATIONS: usize = 10_000;

/// The timeline the document's animations need: how long one play-through
/// is and whether it repeats.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TimelineSpan {
    pub(crate) duration: f64,
    pub(crate) endless: bool,
}

impl TimelineSpan {
    pub(crate) fn add(&mut self, spec: &AnimationSpec) {
        self.duration = self.duration.max(spec.span());
        self.endless |= spec.iterations == Iterations::Infinite;
    }
}

/// The keyframes, as text, of every property `specs` animate on one element:
/// `base` gives a property's value where no keyframe sets it.
pub(crate) fn property_keyframes(
    specs: &[AnimationSpec],
    sheet: &Stylesheet,
    base: &dyn Fn(&str) -> String,
    total: f64,
) -> HashMap<String, Vec<Keyframe<String>>> {
    let mut out = HashMap::new();
    for spec in specs {
        let Some(blocks) = sheet.keyframes.get(&spec.name) else {
            continue;
        };
        if spec.duration <= 0.0 {
            continue;
        }
        // Each property's stops: (offset, value, easing of the segment after).
        let mut stops: HashMap<String, Vec<(f64, String, Easing)>> = HashMap::new();
        for block in blocks {
            let easing = block
                .declarations
                .iter()
                .rev()
                .find(|d| d.name == "animation-timing-function")
                .and_then(|d| crate::values::timing_function(&d.value))
                .unwrap_or_else(|| spec.easing.clone());
            for decl in &block.declarations {
                if decl.name.starts_with("animation") {
                    continue;
                }
                let list = stops.entry(decl.name.clone()).or_default();
                for &offset in &block.offsets {
                    // A later block at the same offset wins.
                    list.retain(|(o, _, _)| *o != offset);
                    list.push((offset, decl.value.clone(), easing.clone()));
                }
            }
        }
        for (name, mut list) in stops {
            list.sort_by(|a, b| a.0.total_cmp(&b.0));
            let base_value = base(&name);
            if list.first().is_none_or(|s| s.0 > 0.0) {
                list.insert(0, (0.0, base_value.clone(), spec.easing.clone()));
            }
            if list.last().is_none_or(|s| s.0 < 1.0) {
                list.push((1.0, base_value.clone(), Easing::Linear));
            }
            out.insert(name, unroll(spec, &list, &base_value, total));
        }
    }
    out
}

/// The stops of one iteration, played forwards or backwards: (offset in
/// the iteration, value, easing to the next).
fn iteration_stops(stops: &[(f64, String, Easing)], reversed: bool) -> Vec<(f64, String, Easing)> {
    if !reversed {
        return stops.to_vec();
    }
    let n = stops.len();
    (0..n)
        .rev()
        .map(|i| {
            let easing = if i > 0 {
                reversed_easing(&stops[i - 1].2)
            } else {
                Easing::Linear
            };
            (1.0 - stops[i].0, stops[i].1.clone(), easing)
        })
        .collect()
}

/// `stops` placed on the timeline: the delay, each iteration and what
/// shows before and after, as the fill mode says.
fn unroll(
    spec: &AnimationSpec,
    stops: &[(f64, String, Easing)],
    base: &str,
    total: f64,
) -> Vec<Keyframe<String>> {
    let reversed = |i: usize| match spec.direction {
        PlayDirection::Normal => false,
        PlayDirection::Reverse => true,
        PlayDirection::Alternate => i % 2 == 1,
        PlayDirection::AlternateReverse => i.is_multiple_of(2),
    };
    let delay = spec.delay.max(0.0);
    let count = match spec.iterations {
        Iterations::Count(n) => n.ceil().max(0.0) as usize,
        Iterations::Infinite => (((total - delay) / spec.duration).ceil().max(1.0)) as usize,
    }
    .min(MAX_ITERATIONS);
    let mut out = Vec::new();
    if count == 0 {
        out.push(Keyframe::new(0.0, base.to_owned(), Easing::Linear));
        return out;
    }
    if delay > 0.0 {
        let before = if matches!(spec.fill, FillMode::Backwards | FillMode::Both) {
            iteration_stops(stops, reversed(0))[0].1.clone()
        } else {
            base.to_owned()
        };
        out.push(Keyframe::new(0.0, before, Easing::Hold));
    }
    let mut last = None;
    for i in 0..count {
        let start = delay + i as f64 * spec.duration;
        if i > 0 && start >= total {
            break;
        }
        for (offset, value, easing) in iteration_stops(stops, reversed(i)) {
            out.push(Keyframe::new(
                start + offset * spec.duration,
                value.clone(),
                easing,
            ));
            last = Some(value);
        }
    }
    if let Iterations::Count(_) = spec.iterations {
        let end = delay + count as f64 * spec.duration;
        if end < total {
            let after = match (spec.fill, last) {
                (FillMode::Forwards | FillMode::Both, Some(v)) => v,
                _ => base.to_owned(),
            };
            out.push(Keyframe::new(end, after, Easing::Linear));
        }
    }
    out
}

/// The easing a segment follows when played backwards.
pub(crate) fn reversed_easing(e: &Easing) -> Easing {
    match e {
        Easing::Linear => Easing::Linear,
        Easing::CubicBezier { x1, y1, x2, y2 } => Easing::CubicBezier {
            x1: 1.0 - x2,
            y1: 1.0 - y2,
            x2: 1.0 - x1,
            y2: 1.0 - y1,
        },
        Easing::Hold => Easing::Steps {
            count: 1,
            position: StepPosition::JumpStart,
        },
        Easing::Steps { count, position } => Easing::Steps {
            count: *count,
            position: match position {
                StepPosition::JumpStart => StepPosition::JumpEnd,
                StepPosition::JumpEnd => StepPosition::JumpStart,
                other => *other,
            },
        },
        Easing::LinearPoints { points } => Easing::LinearPoints {
            points: points
                .iter()
                .rev()
                .map(|(x, y)| (1.0 - x, 1.0 - y))
                .collect(),
        },
    }
}
