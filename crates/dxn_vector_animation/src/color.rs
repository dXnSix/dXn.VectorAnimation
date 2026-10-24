//! Colors: sRGB with straight (not premultiplied) alpha, each channel 0 to 1.

/// An sRGB color with straight alpha, each channel 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Color {
    /// Red.
    pub r: f64,
    /// Green.
    pub g: f64,
    /// Blue.
    pub b: f64,
    /// Alpha: 0 transparent, 1 opaque.
    pub a: f64,
}

impl Color {
    /// Transparent black.
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    /// Opaque black.
    pub const BLACK: Color = Color::rgba(0.0, 0.0, 0.0, 1.0);
    /// Opaque white.
    pub const WHITE: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);

    /// The color (`r`, `g`, `b`, `a`), each 0 to 1.
    pub const fn rgba(r: f64, g: f64, b: f64, a: f64) -> Self {
        Color { r, g, b, a }
    }

    /// The color of 8-bit channels.
    pub fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        let c = |v: u8| f64::from(v) / 255.0;
        Color::rgba(c(r), c(g), c(b), c(a))
    }

    /// The color with its alpha multiplied by `opacity`.
    pub fn with_opacity(self, opacity: f64) -> Color {
        Color {
            a: (self.a * opacity).clamp(0.0, 1.0),
            ..self
        }
    }

    /// The color with every channel clamped to 0..1.
    pub fn clamped(self) -> Color {
        Color::rgba(
            self.r.clamp(0.0, 1.0),
            self.g.clamp(0.0, 1.0),
            self.b.clamp(0.0, 1.0),
            self.a.clamp(0.0, 1.0),
        )
    }

    /// The color a fraction `t` of the way to `to`, mixed in premultiplied
    /// sRGB as CSS animations mix colors (equal alphas mix channel by
    /// channel, as Lottie does).
    pub fn lerp(self, to: Color, t: f64) -> Color {
        let a = self.a + (to.a - self.a) * t;
        if a <= 0.0 {
            return Color::TRANSPARENT;
        }
        let mix = |x: f64, y: f64| (x * self.a + (y * to.a - x * self.a) * t) / a;
        Color::rgba(mix(self.r, to.r), mix(self.g, to.g), mix(self.b, to.b), a).clamped()
    }
}
