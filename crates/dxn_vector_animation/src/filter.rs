//! Color effects as arithmetic: what each [`ColorOp`] does to a color, as a
//! matrix where it is one (all but transfer tables, steps and gammas, and
//! gradient maps).

use crate::{Color, ColorMatrix, ColorOp, GradientStop, Interpolate, Transfer};

/// The luminance of linear-ish RGB (Rec. 709 weights, as SVG's `saturate`
/// and CSS's `grayscale()`).
const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];

impl Default for ColorMatrix {
    fn default() -> Self {
        ColorMatrix::IDENTITY
    }
}

impl Interpolate for ColorMatrix {
    fn interpolate(&self, to: &Self, t: f64) -> Self {
        let mut out = [0.0; 20];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.0[i] + (to.0[i] - self.0[i]) * t;
        }
        ColorMatrix(out)
    }
}

impl ColorMatrix {
    /// Changes nothing.
    pub const IDENTITY: ColorMatrix = ColorMatrix([
        1.0, 0.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, 0.0,
    ]);

    /// The matrix whose color rows are `rgb` (three rows of five) and whose
    /// alpha row keeps alpha.
    fn colors(rgb: [[f64; 5]; 3]) -> ColorMatrix {
        let mut m = ColorMatrix::IDENTITY.0;
        for (r, row) in rgb.iter().enumerate() {
            m[r * 5..r * 5 + 5].copy_from_slice(row);
        }
        ColorMatrix(m)
    }

    /// The same change on every color channel: `slope × c + intercept`.
    fn linear(slope: f64, intercept: f64) -> ColorMatrix {
        ColorMatrix::colors([
            [slope, 0.0, 0.0, 0.0, intercept],
            [0.0, slope, 0.0, 0.0, intercept],
            [0.0, 0.0, slope, 0.0, intercept],
        ])
    }

    /// SVG's `saturate` (CSS `saturate()`).
    pub fn saturate(s: f64) -> ColorMatrix {
        ColorMatrix::colors([
            [
                0.213 + 0.787 * s,
                0.715 - 0.715 * s,
                0.072 - 0.072 * s,
                0.0,
                0.0,
            ],
            [
                0.213 - 0.213 * s,
                0.715 + 0.285 * s,
                0.072 - 0.072 * s,
                0.0,
                0.0,
            ],
            [
                0.213 - 0.213 * s,
                0.715 - 0.715 * s,
                0.072 + 0.928 * s,
                0.0,
                0.0,
            ],
        ])
    }

    /// SVG's `hueRotate` (CSS `hue-rotate()`), in degrees.
    pub fn hue_rotate(degrees: f64) -> ColorMatrix {
        let (sin, cos) = degrees.to_radians().sin_cos();
        ColorMatrix::colors([
            [
                0.213 + cos * 0.787 - sin * 0.213,
                0.715 - cos * 0.715 - sin * 0.715,
                0.072 - cos * 0.072 + sin * 0.928,
                0.0,
                0.0,
            ],
            [
                0.213 - cos * 0.213 + sin * 0.143,
                0.715 + cos * 0.285 + sin * 0.140,
                0.072 - cos * 0.072 - sin * 0.283,
                0.0,
                0.0,
            ],
            [
                0.213 - cos * 0.213 - sin * 0.787,
                0.715 - cos * 0.715 + sin * 0.715,
                0.072 + cos * 0.928 + sin * 0.072,
                0.0,
                0.0,
            ],
        ])
    }

    /// CSS `grayscale()`.
    pub fn grayscale(amount: f64) -> ColorMatrix {
        let k = 1.0 - amount.clamp(0.0, 1.0);
        ColorMatrix::colors([
            [
                0.2126 + 0.7874 * k,
                0.7152 - 0.7152 * k,
                0.0722 - 0.0722 * k,
                0.0,
                0.0,
            ],
            [
                0.2126 - 0.2126 * k,
                0.7152 + 0.2848 * k,
                0.0722 - 0.0722 * k,
                0.0,
                0.0,
            ],
            [
                0.2126 - 0.2126 * k,
                0.7152 - 0.7152 * k,
                0.0722 + 0.9278 * k,
                0.0,
                0.0,
            ],
        ])
    }

    /// CSS `sepia()`.
    pub fn sepia(amount: f64) -> ColorMatrix {
        let k = 1.0 - amount.clamp(0.0, 1.0);
        ColorMatrix::colors([
            [
                0.393 + 0.607 * k,
                0.769 - 0.769 * k,
                0.189 - 0.189 * k,
                0.0,
                0.0,
            ],
            [
                0.349 - 0.349 * k,
                0.686 + 0.314 * k,
                0.168 - 0.168 * k,
                0.0,
                0.0,
            ],
            [
                0.272 - 0.272 * k,
                0.534 - 0.534 * k,
                0.131 + 0.869 * k,
                0.0,
                0.0,
            ],
        ])
    }

    /// CSS `invert()`.
    pub fn invert(amount: f64) -> ColorMatrix {
        let a = amount.clamp(0.0, 1.0);
        ColorMatrix::linear(1.0 - 2.0 * a, a)
    }

    /// CSS `opacity()`.
    pub fn opacity(amount: f64) -> ColorMatrix {
        let mut m = ColorMatrix::IDENTITY;
        m.0[18] = amount.clamp(0.0, 1.0);
        m
    }

    /// CSS `brightness()`.
    pub fn brightness(amount: f64) -> ColorMatrix {
        ColorMatrix::linear(amount.max(0.0), 0.0)
    }

    /// CSS `contrast()`.
    pub fn contrast(amount: f64) -> ColorMatrix {
        let a = amount.max(0.0);
        ColorMatrix::linear(a, 0.5 - 0.5 * a)
    }

    /// SVG's `luminanceToAlpha`.
    pub fn luminance_to_alpha() -> ColorMatrix {
        ColorMatrix([
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.2125, 0.7154, 0.0721, 0.0, 0.0,
        ])
    }

    /// Lottie's Tint: the luminance mapped from `black` to `white`, mixed
    /// in by `amount`.
    pub fn tint(black: Color, white: Color, amount: f64) -> ColorMatrix {
        let k = amount.clamp(0.0, 1.0);
        let (b, w) = ([black.r, black.g, black.b], [white.r, white.g, white.b]);
        let row = |i: usize| -> [f64; 5] {
            let mut row = [0.0; 5];
            for (j, v) in row.iter_mut().take(3).enumerate() {
                *v = k * (w[i] - b[i]) * LUMA[j] + if i == j { 1.0 - k } else { 0.0 };
            }
            row[4] = k * b[i];
            row
        };
        ColorMatrix::colors([row(0), row(1), row(2)])
    }

    /// Lottie's Fill: `color` mixed in by `amount` times its alpha, the
    /// drawing's alpha kept.
    pub fn fill(color: Color, amount: f64) -> ColorMatrix {
        let k = (amount * color.a).clamp(0.0, 1.0);
        let c = [color.r, color.g, color.b];
        let row = |i: usize| -> [f64; 5] {
            let mut row = [0.0; 5];
            row[i] = 1.0 - k;
            row[4] = k * c[i];
            row
        };
        ColorMatrix::colors([row(0), row(1), row(2)])
    }

    /// This change, then `next`.
    pub fn then(&self, next: &ColorMatrix) -> ColorMatrix {
        let (a, b) = (&self.0, &next.0);
        let mut out = [0.0; 20];
        for r in 0..4 {
            for c in 0..5 {
                let mut v = if c == 4 { b[r * 5 + 4] } else { 0.0 };
                for k in 0..4 {
                    v += b[r * 5 + k] * a[k * 5 + c];
                }
                out[r * 5 + c] = v;
            }
        }
        ColorMatrix(out)
    }

    /// The straight color `c` (red, green, blue, alpha) changed, each
    /// channel clamped to 0..1.
    pub fn apply(&self, c: [f64; 4]) -> [f64; 4] {
        let m = &self.0;
        let mut out = [0.0; 4];
        for (r, v) in out.iter_mut().enumerate() {
            let row = &m[r * 5..r * 5 + 5];
            *v = (row[0] * c[0] + row[1] * c[1] + row[2] * c[2] + row[3] * c[3] + row[4])
                .clamp(0.0, 1.0);
        }
        out
    }

    /// Whether alpha comes out as it goes in.
    pub fn keeps_alpha(&self) -> bool {
        self.0[15..20] == [0.0, 0.0, 0.0, 1.0, 0.0]
    }

    /// Whether the colors come out as they go in, whatever the alpha
    /// does.
    pub fn keeps_colors(&self) -> bool {
        self.0[..15] == ColorMatrix::IDENTITY.0[..15]
    }
}

impl Transfer {
    /// The channel value `c` (0..1) through the function, clamped to 0..1.
    pub fn apply(&self, c: f64) -> f64 {
        let c = c.clamp(0.0, 1.0);
        let v = match self {
            Transfer::Identity => c,
            Transfer::Table { values } => match values.len() {
                0 => c,
                1 => values[0],
                n => {
                    let at = c * (n - 1) as f64;
                    let k = (at.floor() as usize).min(n - 2);
                    values[k] + (values[k + 1] - values[k]) * (at - k as f64)
                }
            },
            Transfer::Discrete { values } => match values.len() {
                0 => c,
                n => values[((c * n as f64).floor() as usize).min(n - 1)],
            },
            Transfer::Linear { slope, intercept } => slope * c + intercept,
            Transfer::Gamma {
                amplitude,
                exponent,
                offset,
            } => amplitude * c.powf(*exponent) + offset,
        };
        if v.is_finite() { v.clamp(0.0, 1.0) } else { c }
    }

    /// Its slope and intercept when it is a straight line.
    pub fn as_linear(&self) -> Option<(f64, f64)> {
        match self {
            Transfer::Identity => Some((1.0, 0.0)),
            Transfer::Linear { slope, intercept } => Some((*slope, *intercept)),
            Transfer::Table { values } if values.len() == 2 => {
                Some((values[1] - values[0], values[0]))
            }
            Transfer::Gamma {
                amplitude,
                exponent,
                offset,
            } if *exponent == 1.0 => Some((*amplitude, *offset)),
            _ => None,
        }
    }
}

/// One step of a color effect at a moment.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorStep {
    /// A matrix (consecutive ones multiplied into one).
    Matrix(ColorMatrix),
    /// Red, green, blue and alpha each through its function.
    Transfer([Transfer; 4]),
    /// The luminance mapped to a gradient's colors, mixed in by `amount`.
    GradientMap {
        /// The colors from black to white.
        stops: Vec<GradientStop>,
        /// How far.
        amount: f64,
    },
}

impl ColorOp {
    /// Its matrix at `time`, when it is one.
    pub fn matrix_at(&self, time: f64) -> Option<ColorMatrix> {
        Some(match self {
            ColorOp::Matrix { matrix } => matrix.sample(time),
            ColorOp::Saturate { amount } => ColorMatrix::saturate(amount.sample(time)),
            ColorOp::HueRotate { degrees } => ColorMatrix::hue_rotate(degrees.sample(time)),
            ColorOp::Grayscale { amount } => ColorMatrix::grayscale(amount.sample(time)),
            ColorOp::Sepia { amount } => ColorMatrix::sepia(amount.sample(time)),
            ColorOp::Invert { amount } => ColorMatrix::invert(amount.sample(time)),
            ColorOp::Opacity { amount } => ColorMatrix::opacity(amount.sample(time)),
            ColorOp::Brightness { amount } => ColorMatrix::brightness(amount.sample(time)),
            ColorOp::Contrast { amount } => ColorMatrix::contrast(amount.sample(time)),
            ColorOp::LuminanceToAlpha => ColorMatrix::luminance_to_alpha(),
            ColorOp::Tint {
                black,
                white,
                amount,
            } => ColorMatrix::tint(black.sample(time), white.sample(time), amount.sample(time)),
            ColorOp::Fill { color, amount } => {
                ColorMatrix::fill(color.sample(time), amount.sample(time))
            }
            ColorOp::Transfer {
                red,
                green,
                blue,
                alpha,
            } => {
                let [(rs, ri), (gs, gi), (bs, bi), (as_, ai)] = [
                    red.as_linear()?,
                    green.as_linear()?,
                    blue.as_linear()?,
                    alpha.as_linear()?,
                ];
                ColorMatrix([
                    rs, 0.0, 0.0, 0.0, ri, //
                    0.0, gs, 0.0, 0.0, gi, //
                    0.0, 0.0, bs, 0.0, bi, //
                    0.0, 0.0, 0.0, as_, ai,
                ])
            }
            ColorOp::GradientMap { .. } => return None,
        })
    }

    /// Its step at `time`.
    pub fn step_at(&self, time: f64) -> ColorStep {
        if let Some(m) = self.matrix_at(time) {
            return ColorStep::Matrix(m);
        }
        match self {
            ColorOp::Transfer {
                red,
                green,
                blue,
                alpha,
            } => ColorStep::Transfer([red.clone(), green.clone(), blue.clone(), alpha.clone()]),
            ColorOp::GradientMap { stops, amount } => ColorStep::GradientMap {
                stops: stops.sample(time),
                amount: amount.sample(time).clamp(0.0, 1.0),
            },
            // Every other op is a matrix.
            _ => ColorStep::Matrix(ColorMatrix::IDENTITY),
        }
    }
}

/// The steps of `ops` at `time`, consecutive matrices multiplied into one
/// and matrices that change nothing left out.
pub fn steps_at(ops: &[ColorOp], time: f64) -> Vec<ColorStep> {
    let mut out: Vec<ColorStep> = Vec::new();
    for op in ops {
        match (op.step_at(time), out.last_mut()) {
            (ColorStep::Matrix(m), Some(ColorStep::Matrix(last))) => *last = last.then(&m),
            (step, _) => out.push(step),
        }
    }
    out.retain(|s| !matches!(s, ColorStep::Matrix(m) if *m == ColorMatrix::IDENTITY));
    out
}

fn to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// The color `stops` (ascending offsets) give at `t`.
pub fn gradient_color(stops: &[GradientStop], t: f64) -> Color {
    let Some(first) = stops.first() else {
        return Color::TRANSPARENT;
    };
    if t <= first.offset {
        return first.color;
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.offset {
            let span = b.offset - a.offset;
            let k = if span > 0.0 {
                (t - a.offset) / span
            } else {
                1.0
            };
            return a.color.lerp(b.color, k);
        }
    }
    stops[stops.len() - 1].color
}

/// The straight color `c` (red, green, blue, alpha; 0..1) through `steps`,
/// in linear light when `linear`.
pub fn apply_steps(steps: &[ColorStep], linear: bool, c: [f64; 4]) -> [f64; 4] {
    let mut c = c;
    if linear {
        for v in &mut c[..3] {
            *v = to_linear(*v);
        }
    }
    for step in steps {
        c = match step {
            ColorStep::Matrix(m) => m.apply(c),
            ColorStep::Transfer(f) => [
                f[0].apply(c[0]),
                f[1].apply(c[1]),
                f[2].apply(c[2]),
                f[3].apply(c[3]),
            ],
            ColorStep::GradientMap { stops, amount } => {
                let l = LUMA[0] * c[0] + LUMA[1] * c[1] + LUMA[2] * c[2];
                let g = gradient_color(stops, l);
                let mix = |x: f64, y: f64| x + (y - x) * amount;
                [mix(c[0], g.r), mix(c[1], g.g), mix(c[2], g.b), c[3]]
            }
        };
    }
    if linear {
        for v in &mut c[..3] {
            *v = to_srgb(v.clamp(0.0, 1.0));
        }
    }
    c
}

/// A color through `steps` (see [`apply_steps`]).
pub fn apply_to_color(steps: &[ColorStep], linear: bool, c: Color) -> Color {
    let [r, g, b, a] = apply_steps(steps, linear, [c.r, c.g, c.b, c.a]);
    Color::rgba(r, g, b, a)
}
