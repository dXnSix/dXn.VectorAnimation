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
pub(crate) const MAX_ITERATIONS: usize = 10_000;

/// Periods line up on a grid of this many steps a second (microseconds).
const GRAIN: f64 = 1e6;

/// The timeline the document's animations need: how long one play-through
/// is, whether it repeats, and from where.
#[derive(Debug, Clone, Default)]
pub(crate) struct TimelineSpan {
    /// Until every animation has ended, or has played once what it repeats.
    pub(crate) duration: f64,
    pub(crate) endless: bool,
    /// What repeats forever: when each starts, and its period.
    pub(crate) repeats: Vec<(f64, f64)>,
    /// When the last of what does not repeat forever ends.
    pub(crate) ends: f64,
    /// Where the loop starts over: what comes before plays once.
    pub(crate) loop_start: f64,
}

impl TimelineSpan {
    pub(crate) fn add(&mut self, spec: &AnimationSpec) {
        self.duration = self.duration.max(spec.span());
        if spec.iterations == Iterations::Infinite {
            self.endless = true;
            self.repeat(spec.delay.max(0.0), spec.period());
        } else {
            self.ends = self.ends.max(spec.span());
        }
    }

    /// Something that repeats every `period` seconds from `start` on.
    pub(crate) fn repeat(&mut self, start: f64, period: f64) {
        if period > 0.0 && period.is_finite() && start.is_finite() {
            self.repeats.push((start, period));
        }
    }

    /// Makes the timeline end in the loop of what repeats forever, as a
    /// browser plays it: delays and what ends play once, up to the loop
    /// start (where the last of them to start has started and everything
    /// else has ended); the loop is as long as it takes all of them to come
    /// round together (their periods' least common multiple). Where that
    /// is longer than `max_duration` or takes one of them more than
    /// [`MAX_ITERATIONS`], the timeline stays as long as the longest and
    /// loops from 0, and the warning says so.
    pub(crate) fn close_loop(&mut self, max_duration: f64) -> Option<String> {
        if self.repeats.is_empty() {
            return None;
        }
        if let Some((start, length)) = self.common_loop(max_duration) {
            self.loop_start = start;
            self.duration = start + length;
            return None;
        }
        let steps = |p: f64| (p * GRAIN).round();
        let (shortest, longest) = self
            .repeats
            .iter()
            .fold((f64::INFINITY, 0.0_f64), |(lo, hi), &(_, p)| {
                (lo.min(p), hi.max(p))
            });
        (steps(shortest) != steps(longest)).then(|| {
            format!(
                "animations repeating forever every {shortest} s to {longest} s come round \
                 together only past {max_duration} s or {MAX_ITERATIONS} iterations: the \
                 timeline loops at {} s, cutting some of them short",
                self.duration
            )
        })
    }

    /// Where the common loop starts and how long it is, when it is within
    /// the limits.
    fn common_loop(&self, max_duration: f64) -> Option<(f64, f64)> {
        let mut lcm: u128 = 1;
        let (mut start, mut shortest) = (0.0_f64, f64::INFINITY);
        for &(s, period) in &self.repeats {
            let steps = ((period * GRAIN).round() as u128).max(1);
            lcm = (lcm / gcd(lcm, steps)).checked_mul(steps)?;
            if lcm as f64 / GRAIN > max_duration {
                return None;
            }
            start = start.max(s);
            shortest = shortest.min(period);
        }
        let common = lcm as f64 / GRAIN;
        let start = start.max(self.ends);
        let length = start + common;
        (length <= max_duration && length / shortest <= MAX_ITERATIONS as f64)
            .then_some((start, common))
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The keyframes, as text, of every property `specs` animate on one element:
/// `base` gives a property's value where no keyframe sets it.
pub(crate) fn property_keyframes(
    specs: &[AnimationSpec],
    sheet: &Stylesheet,
    base: &dyn Fn(&str) -> String,
    total: f64,
    warn: &mut dyn FnMut(String),
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
            out.insert(name, unroll(spec, &list, &base_value, total, warn));
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
    warn: &mut dyn FnMut(String),
) -> Vec<Keyframe<String>> {
    let reversed = |i: usize| match spec.direction {
        PlayDirection::Normal => false,
        PlayDirection::Reverse => true,
        PlayDirection::Alternate => i % 2 == 1,
        PlayDirection::AlternateReverse => i.is_multiple_of(2),
    };
    // A negative delay starts part-way through.
    let delay = spec.delay;
    let wanted = match spec.iterations {
        Iterations::Count(n) => n.ceil().max(0.0),
        // (A timeline its iterations fill exactly takes no more.)
        Iterations::Infinite => ((total - delay) / spec.duration - 1e-9).ceil().max(1.0),
    };
    if wanted > MAX_ITERATIONS as f64 {
        warn(format!(
            "CSS animation `{}` runs more than {MAX_ITERATIONS} iterations: only the first \
             {MAX_ITERATIONS} play",
            spec.name
        ));
    }
    let count = (wanted as usize).min(MAX_ITERATIONS);
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
    let before = out.len();
    for i in 0..count {
        let start = delay + i as f64 * spec.duration;
        if i > 0 && start >= total - 1e-9 {
            break;
        }
        for (offset, value, easing) in iteration_stops(stops, reversed(i)) {
            out.push(Keyframe::new(start + offset * spec.duration, value, easing));
        }
    }
    if let Iterations::Count(_) = spec.iterations {
        let end = delay + count as f64 * spec.duration;
        if end < total {
            // The last value an iteration ended with.
            let last = out[before..].last().map(|k| k.value.clone());
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
