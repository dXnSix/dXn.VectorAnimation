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
    units(&plan.units, &plan.graph, time, w, h)
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
    let mut bad = 0usize;
    for (p, q) in a.pixels().iter().zip(b.pixels()) {
        let d = [
            p.red().abs_diff(q.red()),
            p.green().abs_diff(q.green()),
            p.blue().abs_diff(q.blue()),
            p.alpha().abs_diff(q.alpha()),
        ];
        if d.iter().any(|d| *d > 64) {
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
    fn walk(list: &[Unit], g: &Graph, t: f64, w: u32, h: u32) -> f64 {
        list.iter()
            .map(|unit| match unit {
                Unit::Canvas(ops) => {
                    let mut p = sk::Pixmap::new(w, h).unwrap();
                    let mut replay = Replay::new(g, t, w, h);
                    replay.run(ops, &mut p);
                    replay.widest.get()
                }
                Unit::Layer { units, .. } => walk(units, g, t, w, h),
            })
            .fold(0.0, f64::max)
    }
    let base = dxn_vector_animation_nsh::fit(scene, f64::from(w), f64::from(h), Default::default());
    let plan = plan::compile(scene, base).expect("the scene compiles");
    walk(&plan.units, &plan.graph, time, w, h)
}

fn units(list: &[Unit], g: &Graph, t: f64, w: u32, h: u32) -> sk::Pixmap {
    let mut out = sk::Pixmap::new(w, h).unwrap();
    for unit in list {
        let (pixmap, opacity, blend) = match unit {
            Unit::Canvas(ops) => {
                let mut p = sk::Pixmap::new(w, h).unwrap();
                Replay::new(g, t, w, h).run(ops, &mut p);
                (p, 1.0, BlendMode::Normal)
            }
            Unit::Layer {
                opacity,
                blend,
                units: inner,
            } => (units(inner, g, t, w, h), g.num(*opacity, t), *blend),
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
                }
                Op::Atop => self.state.atop = true,
                Op::Image(..) => {}
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

    fn fill(&mut self, rule: FillRule, out: &mut sk::Pixmap) {
        let (Some(path), Some(paint)) =
            (sk_path(&self.path), self.sk_paint(&self.state.fill.clone()))
        else {
            return;
        };
        out.fill_path(
            &path,
            &paint,
            sk_rule(rule),
            sk::Transform::identity(),
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
