//! Text modifiers as Rive's runtime applies them (`TextModifierGroup`,
//! `TextModifierRange`): each group's ranges give every character a
//! coverage, and each glyph is moved, turned, scaled and faded by its
//! groups as much as its characters are covered.

use std::collections::HashMap;

use dxn_vector_animation::{Matrix, Point};

use crate::{
    curve::{Breaks, Curve, bezier, solve_x},
    defs::{self, props as p},
    text::{Glyph, Laid},
};

/// Number properties by key, as they move.
#[derive(Default)]
pub(crate) struct Values(pub HashMap<u16, Curve<f64>>);

impl Values {
    fn at(&self, key: u16, t: f64) -> f64 {
        match self.0.get(&key) {
            Some(c) => c.at(t),
            None => defs::initial(key).unwrap_or(0.0),
        }
    }

    fn breaks(&self, out: &mut Vec<Breaks>) {
        out.extend(self.0.values().map(Curve::breaks));
    }
}

/// A range of a group.
pub(crate) struct Range {
    pub values: Values,
    /// The run it covers (an index into the text's runs), or all of them.
    pub run: Option<usize>,
    /// Its cubic interpolator (x1, y1, x2, y2), easing its falloffs.
    pub interpolator: Option<[f64; 4]>,
}

/// A modifier group of a text.
pub(crate) struct Group {
    pub values: Values,
    pub ranges: Vec<Range>,
}

const ORIGIN: u64 = 1;
const TRANSLATION: u64 = 1 << 2;
const ROTATION: u64 = 1 << 3;
const SCALE: u64 = 1 << 4;
const OPACITY: u64 = 1 << 5;
const INVERT_OPACITY: u64 = 1 << 6;

impl Group {
    /// When what the group does changes.
    pub fn breaks(&self, out: &mut Vec<Breaks>) {
        self.values.breaks(out);
        for range in &self.ranges {
            range.values.breaks(out);
        }
    }

    /// Each character's coverage at `t` (`TextModifierGroup::computeCoverage`).
    fn coverage(&self, laid: &Laid, t: f64) -> Vec<f64> {
        let mut coverage = vec![0.0; laid.chars.len()];
        for range in &self.ranges {
            range.cover(laid, t, &mut coverage);
        }
        coverage
    }

    /// The flags of what the group changes.
    fn flags(&self, t: f64) -> u64 {
        self.values.at(p::TEXT_MODIFIER_GROUP_MODIFIER_FLAGS, t) as u64
    }

    /// `ctm` changed by the group at `amount` (`TextModifierGroup::transform`).
    fn transform(&self, amount: f64, ctm: Matrix, t: f64) -> Matrix {
        let flags = self.flags(t);
        if amount == 0.0 || flags & (TRANSLATION | ROTATION | SCALE) == 0 {
            return ctm;
        }
        let v = |key| self.values.at(key, t);
        let (mut x, mut y, mut sx, mut sy, mut rotation) = (0.0, 0.0, 1.0, 1.0, 0.0);
        if flags & TRANSLATION != 0 {
            x = v(p::TEXT_MODIFIER_GROUP_X) * amount;
            y = v(p::TEXT_MODIFIER_GROUP_Y) * amount;
        }
        if flags & SCALE != 0 {
            sx = 1.0 - amount + v(p::TEXT_MODIFIER_GROUP_SCALE_X) * amount;
            sy = 1.0 - amount + v(p::TEXT_MODIFIER_GROUP_SCALE_Y) * amount;
        }
        if flags & ROTATION != 0 {
            rotation = v(p::TEXT_MODIFIER_GROUP_ROTATION) * amount;
        }
        let (sin, cos) = rotation.sin_cos();
        let transform = Matrix::new(cos * sx, sin * sx, -sin * sy, cos * sy, x, y);
        let mut ctm = ctm;
        let origin = flags & ORIGIN != 0;
        let (ox, oy) = (
            v(p::TEXT_MODIFIER_GROUP_ORIGIN_X),
            v(p::TEXT_MODIFIER_GROUP_ORIGIN_Y),
        );
        if origin {
            ctm.e += ox;
            ctm.f += oy;
        }
        ctm = transform * ctm;
        if origin {
            ctm.e -= ox;
            ctm.f -= oy;
        }
        ctm
    }

    /// The glyph opacity `current` faded by the group at `amount`.
    fn opacity(&self, current: f64, amount: f64, t: f64) -> f64 {
        let flags = self.flags(t);
        if flags & OPACITY == 0 {
            return current;
        }
        let opacity = self.values.at(p::TEXT_MODIFIER_GROUP_OPACITY, t);
        if flags & INVERT_OPACITY != 0 {
            current * (1.0 - amount) + opacity * amount
        } else {
            current * opacity * amount
        }
    }
}

impl Range {
    /// The units the range counts (`RangeMapper`): their first characters
    /// and lengths, and the character after the run.
    fn units(&self, laid: &Laid) -> (Vec<(usize, usize)>, usize) {
        let (start, end) = match self.run {
            Some(r) => laid.spans.get(r).copied().flatten().unwrap_or((0, 0)),
            None => (0, laid.chars.len()),
        };
        let mut units = Vec::new();
        let add = |units: &mut Vec<(usize, usize)>, from: usize, to: usize| {
            if to > start && end > from {
                let (a, b) = (start.max(from), end.min(to));
                if b > a {
                    units.push((a, b - a));
                }
            }
        };
        if laid.chars.is_empty() {
            return (units, end);
        }
        match self.values.at(p::TEXT_MODIFIER_RANGE_UNITS_VALUE, 0.0) as u64 {
            // Words: runs of what is not white space.
            2 => {
                let (mut want_space, mut count, mut from) = (false, 0, 0);
                for (index, &c) in laid.chars.iter().enumerate() {
                    if want_space == is_white_space(c) {
                        if want_space {
                            add(&mut units, from, from + count);
                            count = 0;
                        } else {
                            from = index;
                        }
                        want_space = !want_space;
                    }
                    if want_space {
                        count += 1;
                    }
                }
                if count > 0 {
                    add(&mut units, from, from + count);
                }
            }
            3 => {
                for &(from, to) in &laid.lines {
                    add(&mut units, from, to);
                }
            }
            units_value => {
                let mut i = start;
                while i < end {
                    if units_value == 1 && is_white_space(laid.chars[i]) {
                        i += 1;
                        continue;
                    }
                    // Ligatures are not broken.
                    let n = laid.count(i);
                    units.push((i, n));
                    i += n;
                }
            }
        }
        (units, end)
    }

    /// Adds the range's coverage at `t` to `coverage`
    /// (`TextModifierRange::computeCoverage`).
    fn cover(&self, laid: &Laid, t: f64, coverage: &mut [f64]) {
        let (units, end) = self.units(laid);
        if units.is_empty() {
            return;
        }
        let v = |key| self.values.at(key, t);
        let offset = v(p::TEXT_MODIFIER_RANGE_OFFSET);
        let scale = if v(p::TEXT_MODIFIER_RANGE_TYPE_VALUE) as u64 == 1 {
            1.0
        } else {
            units.len() as f64
        };
        let at = |key| (v(key) + offset) * scale;
        let (from, to) = (
            at(p::TEXT_MODIFIER_RANGE_MODIFY_FROM),
            at(p::TEXT_MODIFIER_RANGE_MODIFY_TO),
        );
        let (falloff_from, falloff_to) = (
            at(p::TEXT_MODIFIER_RANGE_FALLOFF_FROM),
            at(p::TEXT_MODIFIER_RANGE_FALLOFF_TO),
        );
        let ease = |c: f64| match self.interpolator {
            Some([x1, y1, x2, y2]) => bezier(0.0, y1, y2, 1.0, solve_x(x1, x2, c)),
            None => c,
        };
        let coverage_at = |u: f64| {
            if to < from || u < from || u > to {
                0.0
            } else if u < falloff_from {
                let range = (falloff_from - from).max(0.0);
                ease(if range == 0.0 {
                    1.0
                } else {
                    (u - from).max(0.0) / range
                })
            } else if u > falloff_to {
                let range = (to - falloff_to).max(0.0);
                ease(if range == 0.0 {
                    1.0
                } else {
                    1.0 - ((u - falloff_to) / range).min(1.0)
                })
            } else {
                1.0
            }
        };
        let strength = v(p::TEXT_MODIFIER_RANGE_STRENGTH);
        let mode = v(p::TEXT_MODIFIER_RANGE_MODE_VALUE) as u64;
        let clamp = v(p::TEXT_MODIFIER_RANGE_CLAMP) != 0.0;
        for (k, &(first, length)) in units.iter().enumerate() {
            let c = strength * coverage_at(k as f64 + 0.5);
            for slot in coverage.iter_mut().skip(first).take(length) {
                let current = match mode {
                    1 => *slot - c,
                    2 => *slot * c,
                    3 => slot.min(c),
                    4 => slot.max(c),
                    5 => (*slot - c).abs(),
                    _ => *slot + c,
                };
                *slot = if clamp {
                    current.clamp(0.0, 1.0)
                } else {
                    current
                };
            }
            // What lies between units is not covered.
            let next = units.get(k + 1).map_or(end, |u| u.0);
            for slot in coverage.iter_mut().take(next).skip(first + length) {
                *slot = 0.0;
            }
        }
    }
}

fn is_white_space(c: char) -> bool {
    c <= ' ' || c == '\u{2028}' || c == '\u{200B}'
}

/// Glyph `glyph` of `laid` at `t`, modified by `groups`: from its outline's
/// space to the text's, and its opacity.
pub(crate) fn glyph_at(groups: &[Group], laid: &Laid, glyph: &Glyph, t: f64) -> (Matrix, f64) {
    let count = laid.count(glyph.text_index);
    let mut ctm = Matrix::IDENTITY;
    let mut opacity = 1.0;
    for group in groups {
        let coverage = group.coverage(laid, t);
        let amount = (0..count)
            .map(|k| coverage.get(glyph.text_index + k).copied().unwrap_or(0.0))
            .sum::<f64>()
            / count as f64;
        ctm = group.transform(amount, ctm, t);
        opacity = group.opacity(opacity, amount, t);
    }
    let at: Point = glyph.at;
    (laid.fit * Matrix::translate(at.x, at.y) * ctm, opacity)
}
