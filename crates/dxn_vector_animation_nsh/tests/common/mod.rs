//! What the tests share: a Canvas 2D replayer (HTML semantics) of compiled
//! plans on tiny-skia, and pixel comparisons with the reference renderer.

#![allow(dead_code)]

use dxn_vector_animation::{
    BlendMode, Direction, EllipseStart, FillRule, LineCap, LineJoin, Matrix, Path, Point,
    RectStart, Scene,
    outline::{ellipse_path, rect_path},
};
use dxn_vector_animation_nsh::{
    graph::{Graph, Num, Paint},
    plan::{self, Gradient, Op, Unit},
};
use tiny_skia as sk;

/// `scene` compiled for a `w` × `h` view (contain), drawn at `time`.
pub fn plan_pixels(scene: &Scene, time: f64, w: u32, h: u32) -> sk::Pixmap {
    let base = dxn_vector_animation_nsh::fit(scene, f64::from(w), f64::from(h), Default::default());
    let plan = plan::compile(scene, base).expect("the scene compiles");
    units(&plan.units, &plan.graph, time, w, h, base)
}

/// The reference renderer's pixels of `scene` at `time`.
pub fn reference_pixels(scene: &Scene, time: f64, w: u32, h: u32) -> sk::Pixmap {
    let options = dxn_vector_animation_render::RenderOptions {
        width: Some(w),
        height: Some(h),
        ..Default::default()
    };
    dxn_vector_animation_render::render(&scene.frame_at(time), &options).expect("renders")
}

/// The share of pixels that differ by more than a quarter in a channel.
pub fn differing(a: &sk::Pixmap, b: &sk::Pixmap) -> f64 {
    differing_by(a, b, 64)
}

/// The share of pixels that differ by more than `by` in a channel.
pub fn differing_by(a: &sk::Pixmap, b: &sk::Pixmap, by: u8) -> f64 {
    let mut bad = 0usize;
    for (p, q) in a.pixels().iter().zip(b.pixels()) {
        let d = [
            p.red().abs_diff(q.red()),
            p.green().abs_diff(q.green()),
            p.blue().abs_diff(q.blue()),
            p.alpha().abs_diff(q.alpha()),
        ];
        if d.iter().any(|d| *d > by) {
            bad += 1;
        }
    }
    bad as f64 / a.pixels().len() as f64
}

/// Asserts that the plan draws `scene` as the reference does at `times`.
pub fn assert_plan_matches(scene: &Scene, times: &[f64], w: u32, h: u32) {
    for &t in times {
        let ours = plan_pixels(scene, t, w, h);
        let reference = reference_pixels(scene, t, w, h);
        let share = differing(&ours, &reference);
        if share > 0.004 {
            let dir = std::env::temp_dir();
            let _ = ours.save_png(dir.join(format!("plan-{t}.png")));
            let _ = reference.save_png(dir.join(format!("reference-{t}.png")));
            panic!(
                "at t = {t}: {:.2}% of the pixels differ (see {})",
                share * 100.0,
                dir.display()
            );
        }
    }
}

/// How far out any path point of `scene`'s plan lies at `time`, in device
/// pixels of a `w` × `h` view.
pub fn widest_point(scene: &Scene, time: f64, w: u32, h: u32) -> f64 {
    fn walk(list: &[Unit], g: &Graph, t: f64, w: u32, h: u32, base: Matrix) -> f64 {
        list.iter()
            .map(|unit| match unit {
                Unit::Canvas(ops) => {
                    let mut p = sk::Pixmap::new(w, h).unwrap();
                    let mut replay = Replay::new(g, t, w, h);
                    replay.run(ops, &mut p);
                    replay.widest.get()
                }
                Unit::Area { area, ops, .. } => {
                    let win = Window::of(*area);
                    let mut p = sk::Pixmap::new(win.w, win.h).unwrap();
                    let mut replay = Replay::new(g, t, win.w, win.h);
                    replay.run(ops, &mut p);
                    replay.widest.get()
                }
                Unit::Wide { margin, ops } => {
                    let (ew, eh) = wide_size(margin, w, h);
                    let mut p = sk::Pixmap::new(ew, eh).unwrap();
                    let mut replay = Replay::new(g, t, ew, eh);
                    replay.run(ops, &mut p);
                    replay.widest.get()
                }
                Unit::Layer { units, .. } | Unit::Clip { units, .. } => {
                    walk(units, g, t, w, h, base)
                }
                Unit::Sampled(part) => {
                    let plan = plan::from_frame(&part.frame_at(t), base);
                    walk(&plan.units, &plan.graph, t, w, h, base)
                }
                Unit::Raster(_) => 0.0,
            })
            .fold(0.0, f64::max)
    }
    let base = dxn_vector_animation_nsh::fit(scene, f64::from(w), f64::from(h), Default::default());
    let plan = plan::compile(scene, base).expect("the scene compiles");
    walk(&plan.units, &plan.graph, time, w, h, base)
}

fn units(list: &[Unit], g: &Graph, t: f64, w: u32, h: u32, base: Matrix) -> sk::Pixmap {
    let view = Window { x: 0, y: 0, w, h };
    units_in(list, g, t, w, h, base, view)
}

/// A window onto the coordinates units draw in (a layer's, the view's for
/// most): its corner and its size, in pixels a point.
#[derive(Clone, Copy)]
struct Window {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

impl Window {
    fn of(r: dxn_vector_animation::Rect) -> Window {
        let (x0, y0) = (r.x.floor() as i32, r.y.floor() as i32);
        let (x1, y1) = (
            (r.x + r.width).ceil() as i32,
            (r.y + r.height).ceil() as i32,
        );
        Window {
            x: x0,
            y: y0,
            w: (x1 - x0).max(1) as u32,
            h: (y1 - y0).max(1) as u32,
        }
    }
}

/// `ops` replayed on a canvas over `element` (their origin at its corner,
/// as a platform's canvas of that size), placed in `win` and kept within
/// `keep` (in the same coordinates).
fn canvas_in(
    ops: &[Op],
    g: &Graph,
    t: f64,
    win: Window,
    element: dxn_vector_animation::Rect,
    keep: dxn_vector_animation::Rect,
) -> sk::Pixmap {
    let el = Window::of(element);
    let mut drawn = sk::Pixmap::new(el.w, el.h).unwrap();
    Replay::new(g, t, el.w, el.h).run(ops, &mut drawn);
    let mut p = sk::Pixmap::new(win.w, win.h).unwrap();
    p.draw_pixmap(
        el.x - win.x,
        el.y - win.y,
        drawn.as_ref(),
        &sk::PixmapPaint::default(),
        sk::Transform::identity(),
        None,
    );
    keep_within(&mut p, win, keep);
    p
}

/// `p` (placed in `win`) cleared outside `keep`, as a view clipping there.
fn keep_within(p: &mut sk::Pixmap, win: Window, keep: dxn_vector_animation::Rect) {
    let (x0, y0) = (keep.x - f64::from(win.x), keep.y - f64::from(win.y));
    let (x1, y1) = (x0 + keep.width, y0 + keep.height);
    let width = win.w as usize;
    for (i, px) in p.pixels_mut().iter_mut().enumerate() {
        let (px_x, px_y) = ((i % width) as f64 + 0.5, (i / width) as f64 + 0.5);
        if px_x < x0 || px_y < y0 || px_x > x1 || px_y > y1 {
            *px = sk::PremultipliedColorU8::TRANSPARENT;
        }
    }
}

/// Where `list` draws in its coordinates, about (a canvas the size of the
/// view covers it).
fn extent(list: &[Unit], w: u32, h: u32) -> dxn_vector_animation::Rect {
    let view = dxn_vector_animation::Rect::new(0.0, 0.0, f64::from(w), f64::from(h));
    list.iter()
        .map(|u| match u {
            Unit::Area { area, shown, .. } => shown.unwrap_or(*area),
            Unit::Layer {
                units,
                transform: None,
                ..
            } => extent(units, w, h),
            _ => view,
        })
        .reduce(|a, b| a.union(b))
        .unwrap_or(view)
}

fn units_in(
    list: &[Unit],
    g: &Graph,
    t: f64,
    w: u32,
    h: u32,
    base: Matrix,
    win: Window,
) -> sk::Pixmap {
    let view = dxn_vector_animation::Rect::new(0.0, 0.0, f64::from(w), f64::from(h));
    let mut out = sk::Pixmap::new(win.w, win.h).unwrap();
    for unit in list {
        let (pixmap, opacity, blend) = match unit {
            Unit::Canvas(ops) => (
                canvas_in(ops, g, t, win, view, view),
                1.0,
                BlendMode::Normal,
            ),
            // A canvas reaching past the view, shown within it.
            Unit::Wide { margin, ops } => {
                let element = dxn_vector_animation::Rect::new(
                    -margin[0],
                    -margin[1],
                    f64::from(w) + margin[0] + margin[2],
                    f64::from(h) + margin[1] + margin[3],
                );
                (
                    canvas_in(ops, g, t, win, element, view),
                    1.0,
                    BlendMode::Normal,
                )
            }
            // A canvas over an area, shown within part of it.
            Unit::Area { area, shown, ops } => (
                canvas_in(ops, g, t, win, *area, shown.unwrap_or(*area)),
                1.0,
                BlendMode::Normal,
            ),
            Unit::Layer {
                opacity,
                blend,
                units: inner,
                transform: None,
            } => (
                units_in(inner, g, t, w, h, base, win),
                g.num(*opacity, t),
                *blend,
            ),
            // A layer that moves: what it holds drawn in its coordinates,
            // then moved as the platform moves the view.
            Unit::Layer {
                opacity,
                blend,
                units: inner,
                transform: Some(tf),
            } => {
                let local = Window::of(extent(inner, w, h));
                let drawn = units_in(inner, g, t, w, h, base, local);
                let n = |v: Num| g.num(v, t);
                let m = Matrix::translate(-f64::from(win.x), -f64::from(win.y))
                    * Matrix::translate(n(tf.translate.0), n(tf.translate.1))
                    * Matrix::rotate(n(tf.rotate))
                    * Matrix::skew(n(tf.skew), 0.0)
                    * Matrix::scale(n(tf.scale.0), n(tf.scale.1))
                    * Matrix::translate(f64::from(local.x), f64::from(local.y));
                let mut p = sk::Pixmap::new(win.w, win.h).unwrap();
                p.draw_pixmap(
                    0,
                    0,
                    drawn.as_ref(),
                    &sk::PixmapPaint {
                        quality: sk::FilterQuality::Bilinear,
                        ..Default::default()
                    },
                    sk_transform(m),
                    None,
                );
                (p, g.num(*opacity, t), *blend)
            }
            // A part rasterized: the reference's pixels, as the player
            // draws them.
            Unit::Raster(part) => {
                let view = plan::in_view(part, base, f64::from(w), f64::from(h));
                let mut p = sk::Pixmap::new(win.w, win.h).unwrap();
                if let Some((image, at)) = plan::raster(&view, t, 1.0) {
                    // Into its place, scaled as the platforms scale it.
                    let image = sk::Pixmap::decode_png(&image.bytes).expect("a PNG");
                    let (iw, ih) = (f64::from(image.width()), f64::from(image.height()));
                    let m = Matrix::translate(at.x - f64::from(win.x), at.y - f64::from(win.y))
                        * Matrix::scale(at.width / iw, at.height / ih);
                    p.draw_pixmap(
                        0,
                        0,
                        image.as_ref(),
                        &sk::PixmapPaint {
                            quality: sk::FilterQuality::Bilinear,
                            ..Default::default()
                        },
                        sk_transform(m),
                        None,
                    );
                }
                (p, 1.0, BlendMode::Normal)
            }
            // A view clipping at a rectangle: what it holds, kept within.
            Unit::Clip { rect, units: inner } => {
                let mut p = units_in(inner, g, t, w, h, base, win);
                keep_within(&mut p, win, *rect);
                (p, 1.0, BlendMode::Normal)
            }
            // A part sampled frame by frame: its frame at `t`.
            Unit::Sampled(part) => {
                let plan = plan::from_frame(&part.frame_at(t), base);
                (
                    units_in(&plan.units, &plan.graph, t, w, h, base, win),
                    1.0,
                    BlendMode::Normal,
                )
            }
        };
        let paint = sk::PixmapPaint {
            opacity: opacity.clamp(0.0, 1.0) as f32,
            blend_mode: sk_blend(blend),
            ..Default::default()
        };
        out.draw_pixmap(
            0,
            0,
            pixmap.as_ref(),
            &paint,
            sk::Transform::identity(),
            None,
        );
    }
    out
}

/// The pixels of a canvas reaching `margin` past a `w` × `h` view (whole
/// points, a pixel each).
fn wide_size(margin: &[f64; 4], w: u32, h: u32) -> (u32, u32) {
    (
        w + (margin[0] + margin[2]) as u32,
        h + (margin[1] + margin[3]) as u32,
    )
}

#[derive(Clone)]
enum Style {
    Color(u32),
    Gradient(std::sync::Arc<Gradient>),
}

#[derive(Clone)]
struct State {
    ctm: Matrix,
    alpha: f64,
    fill: Style,
    stroke: Style,
    width: f64,
    cap: LineCap,
    join: LineJoin,
    miter: f64,
    dash: Vec<f64>,
    dash_offset: f64,
    clip: Option<sk::Mask>,
    blend: BlendMode,
    atop: bool,
    /// `copy`: an image replaces all the canvas holds.
    copy: bool,
    /// `shadowColor` (`0xRRGGBBAA`), `shadowBlur`, `shadowOffset`.
    shadow: (u32, f64, (f64, f64)),
}

struct Replay<'g> {
    g: &'g Graph,
    t: f64,
    w: u32,
    h: u32,
    state: State,
    stack: Vec<State>,
    /// The current path, in device space.
    path: Path,
    /// How far out a path point went, in device pixels.
    widest: std::cell::Cell<f64>,
}

impl<'g> Replay<'g> {
    fn new(g: &'g Graph, t: f64, w: u32, h: u32) -> Self {
        Replay {
            g,
            t,
            w,
            h,
            state: State {
                ctm: Matrix::IDENTITY,
                alpha: 1.0,
                fill: Style::Color(0x000000ff),
                stroke: Style::Color(0x000000ff),
                width: 1.0,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                miter: 10.0,
                dash: Vec::new(),
                dash_offset: 0.0,
                clip: None,
                blend: BlendMode::Normal,
                atop: false,
                copy: false,
                shadow: (0, 0.0, (0.0, 0.0)),
            },
            stack: Vec::new(),
            path: Path::new(),
            widest: std::cell::Cell::new(0.0),
        }
    }

    fn n(&self, v: Num) -> f64 {
        self.g.num(v, self.t)
    }

    fn pt(&self, x: Num, y: Num) -> Point {
        let p = self.state.ctm.apply(Point::new(self.n(x), self.n(y)));
        self.widest
            .set(self.widest.get().max(p.x.abs()).max(p.y.abs()));
        p
    }

    fn concat(&mut self, m: Matrix) {
        self.state.ctm = self.state.ctm * m;
    }

    fn add_path(&mut self, p: &Path) {
        self.path.extend(&p.transformed(self.state.ctm));
    }

    fn run(&mut self, ops: &[Op], out: &mut sk::Pixmap) {
        for op in ops {
            match op {
                Op::Save => self.stack.push(self.state.clone()),
                Op::Restore => {
                    if let Some(s) = self.stack.pop() {
                        self.state = s;
                    }
                }
                Op::Translate(x, y) => self.concat(Matrix::translate(self.n(*x), self.n(*y))),
                Op::Scale(x, y) => self.concat(Matrix::scale(self.n(*x), self.n(*y))),
                Op::Rotate(a) => self.concat(Matrix::rotate(self.n(*a).to_degrees())),
                Op::Transform(m) => {
                    let v = m.map(|v| self.n(v));
                    self.concat(Matrix::new(v[0], v[1], v[2], v[3], v[4], v[5]));
                }
                Op::BeginPath => self.path = Path::new(),
                Op::MoveTo(x, y) => {
                    let p = self.pt(*x, *y);
                    self.path.move_to(p);
                }
                Op::LineTo(x, y) => {
                    let p = self.pt(*x, *y);
                    self.path.line_to(p);
                }
                Op::QuadTo(v) => {
                    let (c, p) = (self.pt(v[0], v[1]), self.pt(v[2], v[3]));
                    self.path.quad_to(c, p);
                }
                Op::CubicTo(v) => {
                    let (c1, c2, p) = (
                        self.pt(v[0], v[1]),
                        self.pt(v[2], v[3]),
                        self.pt(v[4], v[5]),
                    );
                    self.path.cubic_to(c1, c2, p);
                }
                Op::RoundRect([x, y, w, h], r) => {
                    let (x, y, w, h, r) =
                        (self.n(*x), self.n(*y), self.n(*w), self.n(*h), self.n(*r));
                    let p = rect_path(
                        Point::new(x + w / 2.0, y + h / 2.0),
                        Point::new(w, h),
                        Point::new(r, r),
                        Direction::Clockwise,
                        RectStart::TopLeft,
                    );
                    self.add_path(&p);
                }
                Op::Ellipse([cx, cy, rx, ry], ccw) => {
                    let dir = if *ccw {
                        Direction::CounterClockwise
                    } else {
                        Direction::Clockwise
                    };
                    let p = ellipse_path(
                        Point::new(self.n(*cx), self.n(*cy)),
                        Point::new(self.n(*rx), self.n(*ry)),
                        dir,
                        EllipseStart::Right,
                    );
                    self.add_path(&p);
                }
                Op::EllipseArc([cx, cy, rx, ry, start, end], _) => {
                    // As canvas draws it: from `start` to `end` (the plan
                    // never asks for more than a turn), in short steps.
                    let (cx, cy, rx, ry) = (self.n(*cx), self.n(*cy), self.n(*rx), self.n(*ry));
                    let (start, end) = (self.n(*start), self.n(*end));
                    let steps = ((end - start).abs() / 0.005).ceil().max(1.0) as usize;
                    let mut p = Path::new();
                    for i in 0..=steps {
                        let t = start + (end - start) * i as f64 / steps as f64;
                        let point = Point::new(cx + rx * t.cos(), cy + ry * t.sin());
                        if i == 0 {
                            p.move_to(point);
                        } else {
                            p.line_to(point);
                        }
                    }
                    self.add_path(&p);
                }
                Op::Close => self.path.close(),
                Op::Fill(rule) => self.fill(*rule, out),
                Op::Stroke => self.stroke(out),
                Op::Clip(rule) => self.clip(*rule),
                Op::FillColor(c) => self.state.fill = Style::Color(self.paint(*c)),
                Op::StrokeColor(c) => self.state.stroke = Style::Color(self.paint(*c)),
                Op::FillGradient(g) => self.state.fill = Style::Gradient(g.clone()),
                Op::StrokeGradient(g) => self.state.stroke = Style::Gradient(g.clone()),
                Op::LineWidth(w) => self.state.width = self.n(*w),
                Op::LineCap(c) => self.state.cap = *c,
                Op::LineJoin(j) => self.state.join = *j,
                Op::MiterLimit(m) => self.state.miter = *m,
                Op::LineDash(d) => self.state.dash = d.clone(),
                Op::LineDashOffset(o) => self.state.dash_offset = self.n(*o),
                Op::GlobalAlpha(a) => self.state.alpha = self.n(*a).clamp(0.0, 1.0),
                Op::Blend(b) => {
                    self.state.blend = *b;
                    self.state.atop = false;
                    self.state.copy = false;
                }
                Op::Atop => self.state.atop = true,
                Op::Copy => self.state.copy = true,
                Op::Shadow {
                    color,
                    blur,
                    offset,
                } => self.state.shadow = (*color, *blur, *offset),
                Op::Image(data, rect) => self.image(data, *rect, out),
                Op::Text {
                    text,
                    font,
                    anchor,
                    stroke,
                } => {
                    // Set as the reference sets it, in the system's fonts.
                    let span = dxn_vector_animation::FrameTextSpan {
                        text: text.to_string(),
                        position: Some(Point::new(0.0, 0.0)),
                        shift: Point::new(0.0, 0.0),
                        anchor: *anchor,
                        families: font.families.clone(),
                        size: font.size,
                        weight: font.weight,
                        italic: font.italic,
                        letter_spacing: 0.0,
                        fill: None,
                        stroke: None,
                        paint_order: Default::default(),
                    };
                    let outline = dxn_vector_animation_render::text_layout(&[span]);
                    let kept = std::mem::replace(&mut self.path, Path::new());
                    if let Some(p) = outline.first() {
                        self.add_path(p);
                    }
                    if *stroke {
                        self.stroke(out);
                    } else {
                        self.fill(FillRule::NonZero, out);
                    }
                    self.path = kept;
                }
            }
        }
    }

    fn paint(&self, c: Paint) -> u32 {
        self.g.paint(c, self.t)
    }

    fn sk_paint(&self, style: &Style) -> Option<sk::Paint<'static>> {
        let alpha = self.state.alpha;
        let color = |rgba: u32, extra: f64| {
            let c = dxn_vector_animation_nsh::graph::unpack(rgba);
            sk::Color::from_rgba(c.r as f32, c.g as f32, c.b as f32, (c.a * extra) as f32).unwrap()
        };
        let shader = match style {
            Style::Color(c) => sk::Shader::SolidColor(color(*c, alpha)),
            Style::Gradient(g) => {
                let ts = sk_transform(self.state.ctm);
                match &**g {
                    Gradient::Linear { start, end, stops } => sk::LinearGradient::new(
                        sk::Point::from_xy(start.x as f32, start.y as f32),
                        sk::Point::from_xy(end.x as f32, end.y as f32),
                        stops
                            .iter()
                            .map(|(o, c)| sk::GradientStop::new(*o as f32, color(*c, alpha)))
                            .collect(),
                        sk::SpreadMode::Pad,
                        ts,
                    )?,
                    Gradient::Radial {
                        focal,
                        center,
                        radius,
                        stops,
                    } => sk::RadialGradient::new(
                        sk::Point::from_xy(focal.x as f32, focal.y as f32),
                        0.0,
                        sk::Point::from_xy(center.x as f32, center.y as f32),
                        *radius as f32,
                        stops
                            .iter()
                            .map(|(o, c)| sk::GradientStop::new(*o as f32, color(*c, alpha)))
                            .collect(),
                        sk::SpreadMode::Pad,
                        ts,
                    )?,
                }
            }
        };
        Some(sk::Paint {
            shader,
            anti_alias: true,
            blend_mode: if self.state.atop {
                sk::BlendMode::SourceAtop
            } else {
                sk_blend(self.state.blend)
            },
            ..Default::default()
        })
    }

    /// An image as the reference decodes and places it, drawn with the
    /// state's alpha, blend and clip.
    fn image(
        &mut self,
        data: &std::sync::Arc<dxn_vector_animation::ImageData>,
        rect: dxn_vector_animation::Rect,
        out: &mut sk::Pixmap,
    ) {
        let frame = dxn_vector_animation::Frame {
            width: f64::from(self.w),
            height: f64::from(self.h),
            items: vec![dxn_vector_animation::Item::Image {
                data: data.clone(),
                rect,
                transform: self.state.ctm,
            }],
        };
        let options = dxn_vector_animation_render::RenderOptions {
            width: Some(self.w),
            height: Some(self.h),
            ..Default::default()
        };
        let Ok(image) = dxn_vector_animation_render::render(&frame, &options) else {
            return;
        };
        // An image casts a shadow too (of its alpha, as drawn).
        if self.state.shadow.0 & 0xff > 0 {
            let mut shape = sk::Pixmap::new(self.w, self.h).unwrap();
            shape.draw_pixmap(
                0,
                0,
                image.as_ref(),
                &sk::PixmapPaint {
                    opacity: self.state.alpha.clamp(0.0, 1.0) as f32,
                    ..Default::default()
                },
                sk::Transform::identity(),
                None,
            );
            self.cast_shadow(&shape, out);
        }
        if self.state.copy {
            out.fill(sk::Color::TRANSPARENT);
        }
        let paint = sk::PixmapPaint {
            opacity: self.state.alpha.clamp(0.0, 1.0) as f32,
            blend_mode: sk_blend(self.state.blend),
            ..Default::default()
        };
        out.draw_pixmap(
            0,
            0,
            image.as_ref(),
            &paint,
            sk::Transform::identity(),
            self.state.clip.as_ref(),
        );
    }

    fn fill(&mut self, rule: FillRule, out: &mut sk::Pixmap) {
        let (Some(path), Some(paint)) =
            (sk_path(&self.path), self.sk_paint(&self.state.fill.clone()))
        else {
            return;
        };
        if self.state.shadow.0 & 0xff > 0 {
            let mut shape = sk::Pixmap::new(self.w, self.h).unwrap();
            let mut plain = paint.clone();
            plain.blend_mode = sk::BlendMode::SourceOver;
            shape.fill_path(
                &path,
                &plain,
                sk_rule(rule),
                sk::Transform::identity(),
                None,
            );
            self.cast_shadow(&shape, out);
        }
        if self.unbounded() {
            // Canvas' `destination-in` changes what the shape leaves out too.
            let mut shape = sk::Pixmap::new(self.w, self.h).unwrap();
            let mut plain = paint.clone();
            plain.blend_mode = sk::BlendMode::SourceOver;
            shape.fill_path(
                &path,
                &plain,
                sk_rule(rule),
                sk::Transform::identity(),
                None,
            );
            self.compose(&shape, out);
            return;
        }
        out.fill_path(
            &path,
            &paint,
            sk_rule(rule),
            sk::Transform::identity(),
            self.state.clip.as_ref(),
        );
    }

    /// Whether the composite operation changes the canvas where a drawing
    /// leaves it out.
    fn unbounded(&self) -> bool {
        !self.state.atop && self.state.blend == BlendMode::DestinationIn
    }

    /// `shape` (a drawing on a canvas of its own) composed onto `out` with
    /// the composite operation, over the whole canvas within the clip.
    fn compose(&self, shape: &sk::Pixmap, out: &mut sk::Pixmap) {
        out.draw_pixmap(
            0,
            0,
            shape.as_ref(),
            &sk::PixmapPaint {
                blend_mode: sk_blend(self.state.blend),
                ..Default::default()
            },
            sk::Transform::identity(),
            self.state.clip.as_ref(),
        );
    }

    /// The shadow of `shape` (what a drawing paints) as Canvas 2D casts it:
    /// its silhouette in the shadow color, blurred by half the shadow blur,
    /// moved, with the composite operation and clip.
    fn cast_shadow(&self, shape: &sk::Pixmap, out: &mut sk::Pixmap) {
        let (color, blur, (dx, dy)) = self.state.shadow;
        let c = dxn_vector_animation_nsh::graph::unpack(color);
        let mut shadow = sk::Pixmap::new(self.w, self.h).unwrap();
        for (o, p) in shadow.pixels_mut().iter_mut().zip(shape.pixels()) {
            let a = f64::from(p.alpha()) / 255.0 * c.a;
            let ch = |v: f64| ((v * a * 255.0).round() as u8).min((a * 255.0).round() as u8);
            *o = sk::PremultipliedColorU8::from_rgba(
                ch(c.r),
                ch(c.g),
                ch(c.b),
                (a * 255.0).round() as u8,
            )
            .unwrap_or(sk::PremultipliedColorU8::TRANSPARENT);
        }
        box_blur3(&mut shadow, blur / 2.0);
        out.draw_pixmap(
            0,
            0,
            shadow.as_ref(),
            &sk::PixmapPaint {
                blend_mode: if self.state.atop {
                    sk::BlendMode::SourceAtop
                } else {
                    sk_blend(self.state.blend)
                },
                quality: sk::FilterQuality::Bilinear,
                ..Default::default()
            },
            sk::Transform::from_translate(dx as f32, dy as f32),
            self.state.clip.as_ref(),
        );
    }

    fn stroke(&mut self, out: &mut sk::Pixmap) {
        let Some(inv) = self.state.ctm.invert() else {
            return;
        };
        let (Some(path), Some(paint)) = (
            sk_path(&self.path.transformed(inv)),
            self.sk_paint(&self.state.stroke.clone()),
        ) else {
            return;
        };
        let stroke = sk::Stroke {
            width: self.state.width as f32,
            miter_limit: self.state.miter as f32,
            line_cap: match self.state.cap {
                LineCap::Butt => sk::LineCap::Butt,
                LineCap::Round => sk::LineCap::Round,
                LineCap::Square => sk::LineCap::Square,
            },
            line_join: match self.state.join {
                LineJoin::Miter => sk::LineJoin::Miter,
                LineJoin::Round => sk::LineJoin::Round,
                LineJoin::Bevel => sk::LineJoin::Bevel,
            },
            dash: (!self.state.dash.is_empty())
                .then(|| {
                    sk::StrokeDash::new(
                        self.state.dash.iter().map(|d| *d as f32).collect(),
                        self.state.dash_offset as f32,
                    )
                })
                .flatten(),
        };
        if self.state.shadow.0 & 0xff > 0 {
            let mut shape = sk::Pixmap::new(self.w, self.h).unwrap();
            let mut plain = paint.clone();
            plain.blend_mode = sk::BlendMode::SourceOver;
            shape.stroke_path(&path, &plain, &stroke, sk_transform(self.state.ctm), None);
            self.cast_shadow(&shape, out);
        }
        out.stroke_path(
            &path,
            &paint,
            &stroke,
            sk_transform(self.state.ctm),
            self.state.clip.as_ref(),
        );
    }

    fn clip(&mut self, rule: FillRule) {
        let mut mask = sk::Mask::new(self.w, self.h).unwrap();
        if let Some(path) = sk_path(&self.path) {
            mask.fill_path(&path, sk_rule(rule), true, sk::Transform::identity());
        }
        if let Some(old) = &self.state.clip {
            for (a, b) in mask.data_mut().iter_mut().zip(old.data()) {
                *a = ((u16::from(*a) * u16::from(*b)) / 255) as u8;
            }
        }
        self.state.clip = Some(mask);
    }
}

fn sk_rule(rule: FillRule) -> sk::FillRule {
    match rule {
        FillRule::NonZero => sk::FillRule::Winding,
        FillRule::EvenOdd => sk::FillRule::EvenOdd,
    }
}

fn sk_transform(m: Matrix) -> sk::Transform {
    sk::Transform::from_row(
        m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32,
    )
}

fn sk_path(path: &Path) -> Option<sk::Path> {
    use dxn_vector_animation::PathEl;
    let mut b = sk::PathBuilder::new();
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => b.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => b.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(c, p) => b.quad_to(c.x as f32, c.y as f32, p.x as f32, p.y as f32),
            PathEl::CubicTo(c1, c2, p) => b.cubic_to(
                c1.x as f32,
                c1.y as f32,
                c2.x as f32,
                c2.y as f32,
                p.x as f32,
                p.y as f32,
            ),
            PathEl::Close => b.close(),
        }
    }
    b.finish()
}

fn sk_blend(b: BlendMode) -> sk::BlendMode {
    match b {
        BlendMode::Normal => sk::BlendMode::SourceOver,
        BlendMode::Multiply => sk::BlendMode::Multiply,
        BlendMode::Screen => sk::BlendMode::Screen,
        BlendMode::Add => sk::BlendMode::Plus,
        BlendMode::DestinationIn => sk::BlendMode::DestinationIn,
        BlendMode::DestinationOut => sk::BlendMode::DestinationOut,
        _ => sk::BlendMode::SourceOver,
    }
}

/// A Gaussian blur of deviation `sigma` (pixels) as three box blurs along
/// each axis, SVG's way (as the reference blurs).
fn box_blur3(pixmap: &mut sk::Pixmap, sigma: f64) {
    if sigma <= 0.0 {
        return;
    }
    // Small ones by a kernel of the Gaussian itself, as the reference.
    let kernel: Vec<f32> = if sigma < 2.0 {
        let r = (sigma * 3.0).ceil() as isize;
        let w: Vec<f32> = (-r..=r)
            .map(|k| (-((k * k) as f64) / (2.0 * sigma * sigma)).exp() as f32)
            .collect();
        let total: f32 = w.iter().sum();
        w.into_iter().map(|v| v / total).collect()
    } else {
        Vec::new()
    };
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let mut data: Vec<f32> = pixmap.data().iter().map(|&v| f32::from(v)).collect();
    let d =
        ((sigma * 3.0 * (2.0 * std::f64::consts::PI).sqrt() / 4.0 + 0.5).floor() as usize).max(1);
    let pass = |data: &mut Vec<f32>, len: usize, count: usize, step: usize, stride: usize| {
        let mut line = vec![0.0f32; len * 4];
        let mut tmp = vec![0.0f32; len * 4];
        for i in 0..count {
            for x in 0..len {
                let at = i * stride + x * step;
                line[x * 4..x * 4 + 4].copy_from_slice(&data[at..at + 4]);
            }
            if !kernel.is_empty() {
                let r = (kernel.len() / 2) as isize;
                for x in 0..len {
                    let mut acc = [0.0f32; 4];
                    for (k, w) in kernel.iter().enumerate() {
                        let j = x as isize + k as isize - r;
                        if j >= 0 && (j as usize) < len {
                            for c in 0..4 {
                                acc[c] += line[j as usize * 4 + c] * w;
                            }
                        }
                    }
                    tmp[x * 4..x * 4 + 4].copy_from_slice(&acc);
                }
                std::mem::swap(&mut line, &mut tmp);
            }
            let boxes: Vec<(usize, usize)> = if !kernel.is_empty() {
                Vec::new()
            } else if d % 2 == 1 {
                vec![(d, (d - 1) / 2); 3]
            } else {
                vec![(d, d / 2), (d, d / 2 - 1), (d + 1, d / 2)]
            };
            for (size, before) in boxes {
                for x in 0..len {
                    let mut acc = [0.0f32; 4];
                    for k in 0..size {
                        let j = x as isize + k as isize - before as isize;
                        if j >= 0 && (j as usize) < len {
                            for c in 0..4 {
                                acc[c] += line[j as usize * 4 + c];
                            }
                        }
                    }
                    for c in 0..4 {
                        tmp[x * 4 + c] = acc[c] / size as f32;
                    }
                }
                std::mem::swap(&mut line, &mut tmp);
            }
            for x in 0..len {
                let at = i * stride + x * step;
                data[at..at + 4].copy_from_slice(&line[x * 4..x * 4 + 4]);
            }
        }
    };
    pass(&mut data, w, h, 4, w * 4);
    pass(&mut data, h, w, w * 4, 4);
    for (px, v) in pixmap.data_mut().chunks_mut(4).zip(data.chunks(4)) {
        let a = v[3].round().clamp(0.0, 255.0);
        px[3] = a as u8;
        for c in 0..3 {
            px[c] = v[c].round().clamp(0.0, a) as u8;
        }
    }
}
