//! A reference CPU renderer for [`dxn_vector_animation`] frames, on
//! tiny-skia: golden tests, poster frames and the CLI's images.
//!
//! ```no_run
//! # fn scene() -> dxn_vector_animation::Scene { unimplemented!() }
//! let frame = scene().frame_at(0.5);
//! let png = dxn_vector_animation_render::render_png(&frame, &Default::default()).unwrap();
//! ```

use std::{collections::HashMap, sync::Arc};

use dxn_vector_animation::{
    BlendMode, Color, FillRule, Frame, FrameEffect, FrameMask, FramePaint, GradientStop, ImageData,
    Item, Layer, LineCap, LineJoin, MaskKind, Matrix, Path, PathEl, Spread, StrokeStyle,
};
use tiny_skia as sk;

/// How to render a frame.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    /// Pixels wide; `None` for the frame's width at `scale`.
    pub width: Option<u32>,
    /// Pixels high; `None` for the frame's height at `scale`.
    pub height: Option<u32>,
    /// Pixels per user unit when a size is left out.
    pub scale: f64,
    /// Drawn under the frame; transparent when `None`.
    pub background: Option<Color>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            width: None,
            height: None,
            scale: 1.0,
            background: None,
        }
    }
}

/// Why a frame could not be rendered.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RenderError {
    /// The image would have no pixels or too many.
    #[error("cannot render a {0} × {1} image")]
    Size(u32, u32),
    /// PNG encoding failed.
    #[error("PNG encoding failed: {0}")]
    Png(String),
}

/// The most pixels a rendered image may have (a 8192 × 8192 image).
const MAX_PIXELS: u64 = 8192 * 8192;

/// Renders `frame` into a premultiplied RGBA pixmap. The frame stretches
/// to the size asked for.
pub fn render(frame: &Frame, options: &RenderOptions) -> Result<sk::Pixmap, RenderError> {
    let (w, h) = size(frame, options);
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(RenderError::Size(w, h));
    }
    let base = Matrix::scale(
        if frame.width > 0.0 {
            f64::from(w) / frame.width
        } else {
            1.0
        },
        if frame.height > 0.0 {
            f64::from(h) / frame.height
        } else {
            1.0
        },
    );
    let mut renderer = Renderer {
        width: w,
        height: h,
        base,
        images: HashMap::new(),
    };
    let mut pixmap = renderer.draw(&frame.items).ok_or(RenderError::Size(w, h))?;
    if let Some(bg) = options.background {
        let mut out = sk::Pixmap::new(w, h).ok_or(RenderError::Size(w, h))?;
        out.fill(sk_color(bg));
        out.draw_pixmap(
            0,
            0,
            pixmap.as_ref(),
            &sk::PixmapPaint::default(),
            sk::Transform::identity(),
            None,
        );
        pixmap = out;
    }
    Ok(pixmap)
}

/// Renders `frame` as a PNG.
pub fn render_png(frame: &Frame, options: &RenderOptions) -> Result<Vec<u8>, RenderError> {
    render(frame, options)?
        .encode_png()
        .map_err(|e| RenderError::Png(e.to_string()))
}

/// The pixel size `options` asks for (an aspect-keeping one for a single
/// dimension).
pub fn size(frame: &Frame, options: &RenderOptions) -> (u32, u32) {
    let px = |v: f64| v.round().clamp(0.0, f64::from(u32::MAX)) as u32;
    let aspect = if frame.width > 0.0 {
        frame.height / frame.width
    } else {
        1.0
    };
    match (options.width, options.height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, px(f64::from(w) * aspect)),
        (None, Some(h)) => (px(f64::from(h) / aspect.max(1e-9)), h),
        (None, None) => (
            px(frame.width * options.scale),
            px(frame.height * options.scale),
        ),
    }
}

struct Renderer {
    width: u32,
    height: u32,
    base: Matrix,
    images: HashMap<*const ImageData, Option<Arc<sk::Pixmap>>>,
}

impl Renderer {
    /// Draws `items` into a new pixmap.
    fn draw(&mut self, items: &[Item]) -> Option<sk::Pixmap> {
        let mut stack: Vec<(sk::Pixmap, Option<&Layer>)> =
            vec![(sk::Pixmap::new(self.width, self.height)?, None)];
        for item in items {
            match item {
                Item::Fill {
                    path,
                    transform,
                    paint,
                    rule,
                } => {
                    let (Some(path), Some(paint)) = (sk_path(path), self.paint(paint)) else {
                        continue;
                    };
                    let rule = match rule {
                        FillRule::NonZero => sk::FillRule::Winding,
                        FillRule::EvenOdd => sk::FillRule::EvenOdd,
                    };
                    let target = &mut stack.last_mut()?.0;
                    target.fill_path(
                        &path,
                        &paint,
                        rule,
                        sk_transform(self.base * *transform),
                        None,
                    );
                }
                Item::Stroke {
                    path,
                    transform,
                    paint,
                    style,
                } => {
                    let (Some(path), Some(paint)) = (sk_path(path), self.paint(paint)) else {
                        continue;
                    };
                    let target = &mut stack.last_mut()?.0;
                    target.stroke_path(
                        &path,
                        &paint,
                        &sk_stroke(style),
                        sk_transform(self.base * *transform),
                        None,
                    );
                }
                Item::Image {
                    data,
                    rect,
                    transform,
                } => {
                    let Some(image) = self.image(data) else {
                        continue;
                    };
                    let (iw, ih) = (f64::from(image.width()), f64::from(image.height()));
                    let m = self.base
                        * *transform
                        * Matrix::translate(rect.x, rect.y)
                        * Matrix::scale(rect.width / iw, rect.height / ih);
                    let paint = sk::PixmapPaint {
                        quality: sk::FilterQuality::Bicubic,
                        ..Default::default()
                    };
                    let target = &mut stack.last_mut()?.0;
                    target.draw_pixmap(
                        0,
                        0,
                        image.as_ref().as_ref(),
                        &paint,
                        sk_transform(m),
                        None,
                    );
                }
                Item::PushLayer(layer) => {
                    stack.push((sk::Pixmap::new(self.width, self.height)?, Some(layer)))
                }
                Item::PopLayer => {
                    if stack.len() < 2 {
                        continue;
                    }
                    let (mut pixmap, layer) = stack.pop()?;
                    if let Some(layer) = layer {
                        self.compose(&mut pixmap, layer, &mut stack.last_mut()?.0);
                    }
                }
            }
        }
        // Layers left open compose as if closed.
        while stack.len() > 1 {
            let (mut pixmap, layer) = stack.pop()?;
            if let Some(layer) = layer {
                self.compose(&mut pixmap, layer, &mut stack.last_mut()?.0);
            }
        }
        stack.pop().map(|(p, _)| p)
    }

    /// Clips and masks `pixmap` as `layer` says and draws it onto `target`.
    fn compose(&mut self, pixmap: &mut sk::Pixmap, layer: &Layer, target: &mut sk::Pixmap) {
        // Blurs in turn; consecutive shadows cast together, under it.
        for run in layer.effects.chunk_by(|a, b| {
            matches!(
                (a, b),
                (
                    FrameEffect::DropShadow { .. },
                    FrameEffect::DropShadow { .. }
                )
            )
        }) {
            self.effects(pixmap, run);
        }
        if let Some(clip) = &layer.clip
            && let Some(mut mask) = sk::Mask::new(self.width, self.height)
        {
            for c in clip {
                if let Some(path) = sk_path(&c.path) {
                    let rule = match c.rule {
                        FillRule::NonZero => sk::FillRule::Winding,
                        FillRule::EvenOdd => sk::FillRule::EvenOdd,
                    };
                    mask.fill_path(&path, rule, true, sk_transform(self.base * c.transform));
                }
            }
            pixmap.apply_mask(&mask);
        }
        if let Some(m) = &layer.mask
            && let Some(mask) = self.mask(m)
        {
            pixmap.apply_mask(&mask);
        }
        let paint = sk::PixmapPaint {
            opacity: layer.opacity.clamp(0.0, 1.0) as f32,
            blend_mode: sk_blend(layer.blend),
            quality: sk::FilterQuality::Nearest,
        };
        target.draw_pixmap(
            0,
            0,
            pixmap.as_ref(),
            &paint,
            sk::Transform::identity(),
            None,
        );
    }

    /// `run` applied to `pixmap`: a blur, or shadows all cast by `pixmap`
    /// and drawn under it, the first lowest.
    fn effects(&self, pixmap: &mut sk::Pixmap, run: &[FrameEffect]) {
        // The frame's lengths in pixels, along each axis.
        let (sx, sy) = (self.base.a.abs(), self.base.d.abs());
        let Some(mut out) = sk::Pixmap::new(pixmap.width(), pixmap.height()) else {
            return;
        };
        for effect in run {
            match *effect {
                FrameEffect::Blur { sigma } => {
                    blur(pixmap, sigma * sx, sigma * sy);
                    return;
                }
                FrameEffect::DropShadow {
                    color,
                    offset,
                    sigma,
                    knockout,
                } => {
                    let Some(mut shadow) = silhouette(pixmap, color) else {
                        continue;
                    };
                    blur(&mut shadow, sigma * sx, sigma * sy);
                    let Some(mut moved) = sk::Pixmap::new(pixmap.width(), pixmap.height()) else {
                        continue;
                    };
                    moved.draw_pixmap(
                        0,
                        0,
                        shadow.as_ref(),
                        &sk::PixmapPaint {
                            quality: sk::FilterQuality::Bilinear,
                            ..Default::default()
                        },
                        sk::Transform::from_translate(
                            (offset.x * self.base.a) as f32,
                            (offset.y * self.base.d) as f32,
                        ),
                        None,
                    );
                    if knockout {
                        for (o, p) in moved.pixels_mut().iter_mut().zip(pixmap.pixels()) {
                            if p.alpha() > 0 {
                                *o = sk::PremultipliedColorU8::TRANSPARENT;
                            }
                        }
                    }
                    out.draw_pixmap(
                        0,
                        0,
                        moved.as_ref(),
                        &sk::PixmapPaint::default(),
                        sk::Transform::identity(),
                        None,
                    );
                }
            }
        }
        out.draw_pixmap(
            0,
            0,
            pixmap.as_ref(),
            &sk::PixmapPaint::default(),
            sk::Transform::identity(),
            None,
        );
        *pixmap = out;
    }

    fn mask(&mut self, mask: &FrameMask) -> Option<sk::Mask> {
        let content = self.draw(&mask.items)?;
        let kind = match mask.kind {
            MaskKind::Alpha => sk::MaskType::Alpha,
            MaskKind::Luminance => sk::MaskType::Luminance,
        };
        let mut m = sk::Mask::from_pixmap(content.as_ref(), kind);
        if mask.inverted {
            m.invert();
        }
        Some(m)
    }

    fn paint(&self, paint: &FramePaint) -> Option<sk::Paint<'static>> {
        let shader = match paint {
            FramePaint::Solid(c) => sk::Shader::SolidColor(sk_color(*c)),
            FramePaint::Linear {
                start,
                end,
                stops,
                spread,
                transform,
            } => sk::LinearGradient::new(
                sk::Point::from_xy(start.x as f32, start.y as f32),
                sk::Point::from_xy(end.x as f32, end.y as f32),
                sk_stops(stops),
                sk_spread(*spread),
                sk_transform(*transform),
            )?,
            FramePaint::Radial {
                center,
                radius,
                focal,
                stops,
                spread,
                transform,
            } => sk::RadialGradient::new(
                sk::Point::from_xy(focal.x as f32, focal.y as f32),
                0.0,
                sk::Point::from_xy(center.x as f32, center.y as f32),
                radius.max(0.0) as f32,
                sk_stops(stops),
                sk_spread(*spread),
                sk_transform(*transform),
            )?,
        };
        Some(sk::Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        })
    }

    fn image(&mut self, data: &Arc<ImageData>) -> Option<Arc<sk::Pixmap>> {
        self.images
            .entry(Arc::as_ptr(data))
            .or_insert_with(|| decode(data).map(Arc::new))
            .clone()
    }
}

/// An encoded image as a premultiplied pixmap.
fn decode(data: &ImageData) -> Option<sk::Pixmap> {
    let image = image::load_from_memory(&data.bytes).ok()?.to_rgba8();
    let (w, h) = image.dimensions();
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return None;
    }
    let mut bytes = image.into_raw();
    for px in bytes.as_chunks_mut::<4>().0 {
        let a = u16::from(px[3]);
        for c in &mut px[..3] {
            *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
    }
    sk::Pixmap::from_vec(bytes, sk::IntSize::from_wh(w, h)?)
}

fn sk_color(c: Color) -> sk::Color {
    let c = c.clamped();
    sk::Color::from_rgba(c.r as f32, c.g as f32, c.b as f32, c.a as f32)
        .unwrap_or(sk::Color::TRANSPARENT)
}

fn sk_stops(stops: &[GradientStop]) -> Vec<sk::GradientStop> {
    stops
        .iter()
        .map(|s| sk::GradientStop::new(s.offset as f32, sk_color(s.color)))
        .collect()
}

fn sk_spread(s: Spread) -> sk::SpreadMode {
    match s {
        Spread::Pad => sk::SpreadMode::Pad,
        Spread::Repeat => sk::SpreadMode::Repeat,
        Spread::Reflect => sk::SpreadMode::Reflect,
    }
}

fn sk_transform(m: Matrix) -> sk::Transform {
    sk::Transform::from_row(
        m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32,
    )
}

fn sk_path(path: &Path) -> Option<sk::Path> {
    let mut b = sk::PathBuilder::new();
    let mut open = false;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                b.move_to(p.x as f32, p.y as f32);
                open = true;
            }
            PathEl::LineTo(p) if open => b.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(c, p) if open => {
                b.quad_to(c.x as f32, c.y as f32, p.x as f32, p.y as f32)
            }
            PathEl::CubicTo(c1, c2, p) if open => b.cubic_to(
                c1.x as f32,
                c1.y as f32,
                c2.x as f32,
                c2.y as f32,
                p.x as f32,
                p.y as f32,
            ),
            PathEl::Close if open => b.close(),
            _ => {}
        }
    }
    b.finish()
}

fn sk_stroke(style: &StrokeStyle) -> sk::Stroke {
    sk::Stroke {
        width: style.width as f32,
        miter_limit: style.miter_limit as f32,
        line_cap: match style.cap {
            LineCap::Butt => sk::LineCap::Butt,
            LineCap::Round => sk::LineCap::Round,
            LineCap::Square => sk::LineCap::Square,
        },
        line_join: match style.join {
            LineJoin::Miter => sk::LineJoin::Miter,
            LineJoin::Round => sk::LineJoin::Round,
            LineJoin::Bevel => sk::LineJoin::Bevel,
        },
        dash: style.dash.as_ref().and_then(|(array, offset)| {
            sk::StrokeDash::new(array.iter().map(|d| *d as f32).collect(), *offset as f32)
        }),
    }
}

fn sk_blend(b: BlendMode) -> sk::BlendMode {
    match b {
        BlendMode::Normal => sk::BlendMode::SourceOver,
        BlendMode::Multiply => sk::BlendMode::Multiply,
        BlendMode::Screen => sk::BlendMode::Screen,
        BlendMode::Overlay => sk::BlendMode::Overlay,
        BlendMode::Darken => sk::BlendMode::Darken,
        BlendMode::Lighten => sk::BlendMode::Lighten,
        BlendMode::ColorDodge => sk::BlendMode::ColorDodge,
        BlendMode::ColorBurn => sk::BlendMode::ColorBurn,
        BlendMode::HardLight => sk::BlendMode::HardLight,
        BlendMode::SoftLight => sk::BlendMode::SoftLight,
        BlendMode::Difference => sk::BlendMode::Difference,
        BlendMode::Exclusion => sk::BlendMode::Exclusion,
        BlendMode::Hue => sk::BlendMode::Hue,
        BlendMode::Saturation => sk::BlendMode::Saturation,
        BlendMode::Color => sk::BlendMode::Color,
        BlendMode::Luminosity => sk::BlendMode::Luminosity,
        BlendMode::Add => sk::BlendMode::Plus,
        BlendMode::DestinationIn => sk::BlendMode::DestinationIn,
        BlendMode::DestinationOut => sk::BlendMode::DestinationOut,
    }
}

/// `pixmap`'s silhouette in `color`: its alpha times the color's.
fn silhouette(pixmap: &sk::Pixmap, color: Color) -> Option<sk::Pixmap> {
    let mut out = sk::Pixmap::new(pixmap.width(), pixmap.height())?;
    let c = color.clamped();
    for (o, p) in out.pixels_mut().iter_mut().zip(pixmap.pixels()) {
        let a = f64::from(p.alpha()) / 255.0 * c.a;
        let channel = |v: f64| (v * a * 255.0).round().clamp(0.0, 255.0) as u8;
        let alpha = (a * 255.0).round().clamp(0.0, 255.0) as u8;
        if let Some(px) = sk::PremultipliedColorU8::from_rgba(
            channel(c.r).min(alpha),
            channel(c.g).min(alpha),
            channel(c.b).min(alpha),
            alpha,
        ) {
            *o = px;
        }
    }
    Some(out)
}

/// A Gaussian blur of `pixmap`, of standard deviations `sx` and `sy` in
/// pixels: three box blurs along each axis (as SVG's `feGaussianBlur`
/// suggests), a kernel of the Gaussian itself for small ones; outside the
/// pixmap is transparent.
fn blur(pixmap: &mut sk::Pixmap, sx: f64, sy: f64) {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let mut data: Vec<f32> = pixmap.data().iter().map(|&v| f32::from(v)).collect();
    if sx > 0.0 {
        blur_axis(&mut data, w, h, 4, w * 4, sx);
    }
    if sy > 0.0 {
        blur_axis(&mut data, h, w, w * 4, 4, sy);
    }
    let out = pixmap.data_mut();
    for (px, v) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(data.as_chunks::<4>().0)
    {
        let a = v[3].round().clamp(0.0, 255.0);
        px[3] = a as u8;
        for c in 0..3 {
            // Premultiplied: no channel above the alpha.
            px[c] = v[c].round().clamp(0.0, a) as u8;
        }
    }
}

/// Blurs lines of `len` pixels (`step` apart in `data`), `count` of them
/// (`stride` apart), with standard deviation `sigma`.
fn blur_axis(data: &mut [f32], len: usize, count: usize, step: usize, stride: usize, sigma: f64) {
    let mut line = vec![0.0f32; len * 4];
    let mut tmp = vec![0.0f32; len * 4];
    for i in 0..count {
        let start = i * stride;
        for x in 0..len {
            let at = start + x * step;
            line[x * 4..x * 4 + 4].copy_from_slice(&data[at..at + 4]);
        }
        if sigma < 2.0 {
            gaussian(&line, &mut tmp, sigma);
            std::mem::swap(&mut line, &mut tmp);
        } else {
            let d =
                (sigma * 3.0 * (2.0 * std::f64::consts::PI).sqrt() / 4.0 + 0.5).floor() as usize;
            let d = d.clamp(1, len.max(1) * 2);
            if d % 2 == 1 {
                for _ in 0..3 {
                    box_blur(&line, &mut tmp, d, (d - 1) / 2);
                    std::mem::swap(&mut line, &mut tmp);
                }
            } else {
                // Two boxes centered on the boundaries either side of the
                // pixel, and one a pixel wider centered on it.
                box_blur(&line, &mut tmp, d, d / 2);
                box_blur(&tmp, &mut line, d, d / 2 - 1);
                box_blur(&line, &mut tmp, d + 1, d / 2);
                std::mem::swap(&mut line, &mut tmp);
            }
        }
        for x in 0..len {
            let at = start + x * step;
            data[at..at + 4].copy_from_slice(&line[x * 4..x * 4 + 4]);
        }
    }
}

/// The average of each pixel's `size` pixels from `before` before it.
fn box_blur(src: &[f32], dst: &mut [f32], size: usize, before: usize) {
    let len = src.len() / 4;
    let mut sum = [0.0f32; 4];
    let at = |x: isize| -> Option<usize> { (x >= 0 && (x as usize) < len).then_some(x as usize) };
    // The window of pixel 0: from -before to size - 1 - before.
    for x in 0..size as isize {
        if let Some(i) = at(x - before as isize) {
            for c in 0..4 {
                sum[c] += src[i * 4 + c];
            }
        }
    }
    let n = size as f32;
    for x in 0..len {
        for c in 0..4 {
            dst[x * 4 + c] = sum[c] / n;
        }
        let x = x as isize;
        if let Some(i) = at(x + size as isize - before as isize) {
            for c in 0..4 {
                sum[c] += src[i * 4 + c];
            }
        }
        if let Some(i) = at(x - before as isize) {
            for c in 0..4 {
                sum[c] -= src[i * 4 + c];
            }
        }
    }
}

/// A Gaussian kernel of `sigma` (out to three of it) over `src`.
fn gaussian(src: &[f32], dst: &mut [f32], sigma: f64) {
    let len = src.len() / 4;
    let r = (sigma * 3.0).ceil() as isize;
    let weights: Vec<f32> = (-r..=r)
        .map(|k| (-(k * k) as f64 / (2.0 * sigma * sigma)).exp() as f32)
        .collect();
    let total: f32 = weights.iter().sum();
    for x in 0..len as isize {
        let mut acc = [0.0f32; 4];
        for (k, w) in (-r..=r).zip(&weights) {
            let i = x + k;
            if i >= 0 && (i as usize) < len {
                for c in 0..4 {
                    acc[c] += src[i as usize * 4 + c] * w;
                }
            }
        }
        for c in 0..4 {
            dst[x as usize * 4 + c] = acc[c] / total;
        }
    }
}
