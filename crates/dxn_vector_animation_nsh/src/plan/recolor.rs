//! Color effects on the graph: each paint drawn inside a group with color
//! effects has its colors changed as the effects change each pixel's
//! (exact where a paint is drawn alone; at edges and where see-through
//! paints overlap the reference changes the blend, a little apart), and
//! what they do to alpha, a scale, fades the group as its opacity does.

use std::{borrow::Cow, sync::Arc};

use dxn_vector_animation::{
    Animated, Color, ColorEffect, ColorOp, Easing, Effect, GradientStop, Group, ImageData,
    Keyframe, Paint as ScenePaint, Transfer,
    filter::{apply_steps, steps_at},
};

use super::{Compiler, NodeId, Num, Unsupported, unsupported};

/// Samples a second where color effects move.
const RATE: f64 = 30.0;
/// The most samples of one recolored color.
const MOST: usize = 600;

/// The times an op's values change at.
fn op_times(op: &ColorOp) -> Vec<f64> {
    fn of<T>(a: &Animated<T>) -> Vec<f64> {
        a.keyframes().iter().map(|k| k.time).collect()
    }
    match op {
        ColorOp::Matrix { matrix } => of(matrix),
        ColorOp::Saturate { amount }
        | ColorOp::Grayscale { amount }
        | ColorOp::Sepia { amount }
        | ColorOp::Invert { amount }
        | ColorOp::Opacity { amount }
        | ColorOp::Brightness { amount }
        | ColorOp::Contrast { amount } => of(amount),
        ColorOp::HueRotate { degrees } => of(degrees),
        ColorOp::LuminanceToAlpha | ColorOp::Transfer { .. } => Vec::new(),
        ColorOp::Tint {
            black,
            white,
            amount,
        } => [of(black), of(white), of(amount)].concat(),
        ColorOp::Fill { color, opacity } => [of(color), of(opacity)].concat(),
        ColorOp::Overlay { color, amount } => [of(color), of(amount)].concat(),
        ColorOp::GradientMap { stops, amount } => [of(stops), of(amount)].concat(),
    }
}

/// What `op` does to alpha at `time`: a scale, or `None` where it does
/// more (makes alpha of colors, adds to it).
fn alpha_scale(op: &ColorOp, time: f64) -> Option<f64> {
    if let ColorOp::Transfer { alpha, .. } = op {
        return match alpha.as_linear() {
            Some((slope, 0.0)) => Some(slope.max(0.0)),
            _ if *alpha == Transfer::Identity => Some(1.0),
            _ => None,
        };
    }
    if let ColorOp::GradientMap { .. } = op {
        return Some(1.0);
    }
    let m = op.matrix_at(time)?;
    let row = &m.0[15..20];
    (row[0] == 0.0 && row[1] == 0.0 && row[2] == 0.0 && row[4] == 0.0).then_some(row[3].max(0.0))
}

/// The times color effects `effects` change at, and the sampled times
/// between where they move.
fn effect_times(effects: &[ColorEffect]) -> Vec<f64> {
    let mut times: Vec<f64> = effects
        .iter()
        .flat_map(|e| e.ops.iter().flat_map(op_times))
        .collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    if times.len() < 2 {
        return times;
    }
    let mut out = Vec::new();
    for w in times.windows(2) {
        let n = (((w[1] - w[0]) * RATE).ceil() as usize).max(1);
        for k in 0..n {
            out.push(w[0] + (w[1] - w[0]) * k as f64 / n as f64);
        }
    }
    out.push(times[times.len() - 1]);
    if out.len() > MOST {
        let step = out.len().div_ceil(MOST);
        let last = out[out.len() - 1];
        out = out.into_iter().step_by(step).collect();
        out.push(last);
    }
    out
}

impl Compiler {
    /// Takes the color effects of `g` on: each paint inside recolored, and
    /// what they do to alpha, a scale, returned for the group's opacity.
    /// `None` when it has none. The caller ends them with
    /// [`Compiler::end_colors`].
    pub(super) fn begin_colors(
        &mut self,
        g: &Group,
        local: NodeId,
    ) -> Result<Option<Num>, Unsupported> {
        let effects: Vec<ColorEffect> = g
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::Color(c) => Some(c.clone()),
                _ => None,
            })
            .collect();
        if effects.is_empty() {
            return Ok(None);
        }
        let shadow_at = g
            .effects
            .iter()
            .position(|e| matches!(e, Effect::DropShadow(_) | Effect::InnerShadow(_)));
        let color_at = g
            .effects
            .iter()
            .rposition(|e| matches!(e, Effect::Color(_)));
        if let (Some(s), Some(c)) = (shadow_at, color_at)
            && c > s
        {
            return unsupported("a color change of a shadow");
        }
        let times = effect_times(&effects);
        let probe: Vec<f64> = if times.is_empty() {
            vec![0.0]
        } else {
            times.clone()
        };
        // Each probe's alpha scale, all ops' together.
        let mut scales = Vec::with_capacity(probe.len());
        for &t in &probe {
            let mut k = 1.0;
            for op in effects.iter().flat_map(|e| &e.ops) {
                match alpha_scale(op, t) {
                    Some(s) => k *= s,
                    None => return unsupported("a color change of alpha"),
                }
            }
            scales.push(k);
        }
        self.recolor.push(effects);
        if scales.iter().all(|k| (k - 1.0).abs() < 1e-9) {
            return Ok(Some(Num::Const(1.0)));
        }
        if scales.len() == 1 {
            return Ok(Some(Num::Const(scales[0])));
        }
        let keys: Vec<Keyframe<f64>> = probe
            .iter()
            .zip(&scales)
            .map(|(&t, &k)| Keyframe::new(t, k, Easing::Linear))
            .collect();
        let track = Animated::from_keyframes(keys, scales[0]);
        Ok(Some(self.g.track(&track, local, |v| v.clamp(0.0, 1.0))))
    }

    /// Ends the color effects [`Compiler::begin_colors`] took on.
    pub(super) fn end_colors(&mut self) {
        self.recolor.pop();
    }

    /// `c` at `time` through the color effects in force (innermost first),
    /// its alpha kept (their alpha scales fade the groups).
    fn recolor_at(&self, c: Color, time: f64) -> Color {
        let mut out = [c.r, c.g, c.b, c.a];
        for effects in self.recolor.iter().rev() {
            for e in effects {
                out = apply_steps(&steps_at(&e.ops, time), e.linear, out);
            }
        }
        Color::rgba(out[0], out[1], out[2], c.a)
    }

    /// Whether any effect in force moves.
    fn colors_move(&self) -> bool {
        self.recolor
            .iter()
            .flatten()
            .any(|e| e.ops.iter().any(|op| !op_times(op).is_empty()))
    }

    /// `color` through the color effects in force.
    pub(super) fn recolored(&self, color: &Animated<Color>) -> Animated<Color> {
        if self.recolor.is_empty() {
            return color.clone();
        }
        if !self.colors_move() {
            return match color {
                Animated::Static(c) => Animated::Static(self.recolor_at(*c, 0.0)),
                Animated::Track(track) => {
                    let mut track = track.clone();
                    for k in &mut track.keyframes {
                        k.value = self.recolor_at(k.value, 0.0);
                    }
                    Animated::Track(track)
                }
            };
        }
        let mut times = effect_times(&self.recolor.iter().flatten().cloned().collect::<Vec<_>>());
        times.extend(color.keyframes().iter().map(|k| k.time));
        times.sort_by(f64::total_cmp);
        times.dedup();
        let keys = times
            .iter()
            .map(|&t| Keyframe::new(t, self.recolor_at(color.sample(t), t), Easing::Linear))
            .collect();
        Animated::from_keyframes(keys, self.recolor_at(color.sample(0.0), 0.0))
    }

    /// `paint` through the color effects in force: colors recolored, and
    /// gradients' stops where the effects do not move.
    pub(super) fn recolored_paint<'p>(
        &self,
        paint: &'p ScenePaint,
    ) -> Result<Cow<'p, ScenePaint>, Unsupported> {
        if self.recolor.is_empty() {
            return Ok(Cow::Borrowed(paint));
        }
        let stops = |s: &Animated<Vec<GradientStop>>| -> Result<_, Unsupported> {
            if self.colors_move() {
                return unsupported("a color change moving over a gradient");
            }
            let map = |list: &Vec<GradientStop>| -> Vec<GradientStop> {
                list.iter()
                    .map(|g| GradientStop {
                        offset: g.offset,
                        color: self.recolor_at(g.color, 0.0),
                    })
                    .collect()
            };
            Ok(match s {
                Animated::Static(list) => Animated::Static(map(list)),
                Animated::Track(track) => {
                    let mut track = track.clone();
                    for k in &mut track.keyframes {
                        k.value = map(&k.value);
                    }
                    Animated::Track(track)
                }
            })
        };
        Ok(Cow::Owned(match paint {
            ScenePaint::Solid { color } => ScenePaint::Solid {
                color: self.recolored(color),
            },
            ScenePaint::LinearGradient(g) => {
                let mut g = g.clone();
                g.stops = stops(&g.stops)?;
                ScenePaint::LinearGradient(g)
            }
            ScenePaint::RadialGradient(g) => {
                let mut g = g.clone();
                g.stops = stops(&g.stops)?;
                ScenePaint::RadialGradient(g)
            }
            ScenePaint::Pattern(_) => return unsupported("a color change of a pattern"),
        }))
    }

    /// An image through the color effects in force (a PNG; where the
    /// effects do not move).
    pub(super) fn recolored_image(
        &self,
        data: &Arc<ImageData>,
    ) -> Result<Arc<ImageData>, Unsupported> {
        if self.recolor.is_empty() {
            return Ok(data.clone());
        }
        if self.colors_move() {
            return unsupported("a color change moving over an image");
        }
        let Ok(mut pixmap) = tiny_skia::Pixmap::decode_png(&data.bytes) else {
            return unsupported("a color change of an image other than a PNG");
        };
        for px in pixmap.pixels_mut() {
            let a = px.alpha();
            if a == 0 {
                continue;
            }
            let k = f64::from(a) / 255.0;
            let straight = |v: u8| (f64::from(v) / 255.0 / k).min(1.0);
            let c = self.recolor_at(
                Color::rgba(
                    straight(px.red()),
                    straight(px.green()),
                    straight(px.blue()),
                    k,
                ),
                0.0,
            );
            let channel = |v: f64| ((v * k * 255.0).round().clamp(0.0, 255.0) as u8).min(a);
            if let Some(p) = tiny_skia::PremultipliedColorU8::from_rgba(
                channel(c.r),
                channel(c.g),
                channel(c.b),
                a,
            ) {
                *px = p;
            }
        }
        let Some(bytes) = super::png(&pixmap) else {
            return unsupported("an image that does not encode");
        };
        Ok(Arc::new(ImageData {
            mime: "image/png".into(),
            bytes,
        }))
    }
}
