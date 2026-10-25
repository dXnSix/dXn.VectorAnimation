//! Dash patterns whose lengths move and trims moving at both ends play on
//! the graph and draw what the reference draws.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::assert_plan_matches as loose;
use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Easing, EllipseStart, Geometry, Group, Keyframe,
    LineCap, LineJoin, LoopMode, Node, Paint, Path, Point, RectStart, Scene, Shape, ShapePath,
    Stroke, Timeline, Track, Trim, TrimMode,
};

/// A ring spinner as svg-spinners draws one: a dash growing while its
/// offset runs on, SMIL throughout.
const RING: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
  <g>
    <circle cx="12" cy="12" r="9.5" fill="none" stroke="#000" stroke-width="3" stroke-linecap="round">
      <animate attributeName="stroke-dasharray" calcMode="spline" dur="1.5s" keySplines="0.42,0,0.58,1;0.42,0,0.58,1;0.42,0,0.58,1" keyTimes="0;0.475;0.95;1" repeatCount="indefinite" values="0 150;42 150;42 150;42 150"/>
      <animate attributeName="stroke-dashoffset" calcMode="spline" dur="1.5s" keySplines="0.42,0,0.58,1;0.42,0,0.58,1;0.42,0,0.58,1" keyTimes="0;0.475;0.95;1" repeatCount="indefinite" values="0;-16;-59;-59"/>
    </circle>
    <animateTransform attributeName="transform" dur="2s" repeatCount="indefinite" type="rotate" values="0 12 12;360 12 12"/>
  </g>
</svg>"##;

/// A path drawn on as Figma Motion exports it: a dash pattern `0 1` to
/// `1 1` along a path of length 1.
const DRAWN_ON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes kf_line_dash { 0% { stroke-dasharray: 0 1; animation-timing-function: cubic-bezier(0.4, 0, 0.2, 1); } 100% { stroke-dasharray: 1 1; } }
    #line { animation: kf_line_dash 2s linear 0s 1 both; }
  </style>
  <path id="line" pathLength="1" d="M10 80 C 30 10, 70 10, 90 80 L 50 90" fill="none" stroke="#1060e0" stroke-width="6" stroke-linejoin="round"/>
</svg>"##;

fn track(keys: &[(f64, f64)]) -> Animated<f64> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|(t, v)| Keyframe::new(*t, *v, Easing::EASE_IN_OUT))
            .collect(),
    ))
}

fn scene(children: Vec<Node>) -> Scene {
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_mode: LoopMode::Loop,
            markers: vec![],
        },
        root: Group {
            children,
            ..Group::new()
        },
    }
}

fn stroke(color: Color, cap: LineCap) -> Stroke {
    Stroke {
        paint: Paint::solid(color),
        opacity: Animated::Static(1.0),
        width: Animated::Static(5.0),
        cap,
        join: LineJoin::Miter,
        miter_limit: 4.0,
        dash: None,
    }
}

fn trimmed(paths: Vec<ShapePath>, stroke: Stroke, trim: Trim) -> Node {
    Node::shape(Shape {
        paths,
        stroke: Some(stroke),
        trim: Some(trim),
        ..Default::default()
    })
}

fn ellipse(x: f64, y: f64, r: f64) -> ShapePath {
    ShapePath::new(Geometry::Ellipse {
        center: AnimatedPoint::fixed(x, y),
        radius: AnimatedPoint::fixed(r, r),
        direction: Direction::Clockwise,
        start: EllipseStart::Top,
    })
}

fn square(x: f64, y: f64, size: f64) -> ShapePath {
    ShapePath::new(Geometry::Rect {
        center: AnimatedPoint::fixed(x, y),
        size: AnimatedPoint::fixed(size, size),
        radius: AnimatedPoint::fixed(0.0, 0.0),
        direction: Direction::Clockwise,
        start: RectStart::TopLeft,
    })
}

/// The plan draws what the reference does to a few pixels, at three times
/// the size (so a cap turned the wrong way shows).
fn assert_plan_matches(scene: &Scene, times: &[f64], w: u32, h: u32) {
    let (w, h) = (3 * w, 3 * h);
    loose(scene, times, w, h);
    for &t in times {
        let ours = common::plan_pixels(scene, t, w, h);
        let reference = common::reference_pixels(scene, t, w, h);
        let count = (common::differing(&ours, &reference) * f64::from(w * h)).round();
        assert!(count <= 12.0, "at t = {t}: {count} pixels differ");
    }
}

const TIMES: &[f64] = &[0.0, 0.1, 0.35, 0.6, 0.71, 0.9, 1.2, 1.45, 1.7, 2.0];

#[test]
fn ring_spinner() {
    let import = dxn_vector_animation_svg::read(RING.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_plan_matches(&import.scene, TIMES, 96, 96);
}

#[test]
fn path_drawn_on_with_a_dash_pattern() {
    let import = dxn_vector_animation_svg::read(DRAWN_ON.as_bytes()).unwrap();
    assert_plan_matches(&import.scene, TIMES, 100, 100);
}

#[test]
fn trim_moving_at_both_ends_around_closed_outlines() {
    // Round caps over the circle's start, butt caps and miters over the
    // square's corner where it starts.
    let trim = Trim {
        start: track(&[(0.0, 0.0), (1.0, 0.1), (2.0, 0.7)]),
        end: track(&[(0.0, 0.05), (1.0, 0.8), (2.0, 0.9)]),
        offset: track(&[(0.0, 0.6), (2.0, 0.95)]),
        mode: TrimMode::EachPath,
    };
    let circle = trimmed(
        vec![ellipse(30.0, 30.0, 18.0)],
        stroke(Color::rgba(0.9, 0.2, 0.1, 1.0), LineCap::Round),
        trim.clone(),
    );
    let corner = trimmed(
        vec![square(68.0, 68.0, 36.0)],
        stroke(Color::rgba(0.1, 0.3, 0.9, 0.8), LineCap::Butt),
        trim,
    );
    assert_plan_matches(&scene(vec![circle, corner]), TIMES, 100, 100);
}

#[test]
fn trim_spinning_round_with_its_offset() {
    // A loader: a fixed arc carried round more than once, start after end.
    let trim = Trim {
        start: Animated::Static(0.4),
        end: Animated::Static(0.1),
        offset: Animated::Track(Track::new(vec![
            Keyframe::new(0.0, -0.3, Easing::Linear),
            Keyframe::new(2.0, 1.9, Easing::Linear),
        ])),
        mode: TrimMode::EachPath,
    };
    let ring = trimmed(
        vec![ellipse(50.0, 50.0, 30.0)],
        stroke(Color::rgba(0.0, 0.6, 0.3, 1.0), LineCap::Round),
        trim,
    );
    assert_plan_matches(&scene(vec![ring]), TIMES, 100, 100);
}

#[test]
fn trim_across_open_outlines_one_after_another() {
    let mut zigzag = Path::new();
    zigzag.move_to(Point::new(10.0, 20.0));
    zigzag.line_to(Point::new(40.0, 50.0));
    zigzag.line_to(Point::new(70.0, 20.0));
    zigzag.quad_to(Point::new(90.0, 40.0), Point::new(80.0, 60.0));
    let mut wave = Path::new();
    wave.move_to(Point::new(10.0, 85.0));
    wave.cubic_to(
        Point::new(30.0, 55.0),
        Point::new(60.0, 115.0),
        Point::new(90.0, 80.0),
    );
    let trim = Trim {
        start: track(&[(0.0, 0.0), (2.0, 0.6)]),
        end: track(&[(0.0, 0.3), (2.0, 1.0)]),
        offset: Animated::Static(0.0),
        mode: TrimMode::Combined,
    };
    let line = trimmed(
        vec![
            ShapePath::new(Geometry::Path {
                path: Animated::Static(zigzag),
            }),
            ShapePath::new(Geometry::Path {
                path: Animated::Static(wave),
            }),
        ],
        stroke(Color::rgba(0.5, 0.1, 0.6, 1.0), LineCap::Square),
        trim,
    );
    assert_plan_matches(&scene(vec![line]), TIMES, 100, 100);
}

/// The ring spinner in a player.
#[nsh_core::ui::prelude::component]
fn Spinner() -> impl nsh_core::ui::prelude::IntoElement {
    use dxn_vector_animation_nsh::{AnimationInfo, Source, vector_animation};
    use nsh_core::ui::prelude::*;
    let status = signal(String::from("loading"));
    view()
        .child(
            vector_animation()
                .src(Some(Source::Text(RING.into())))
                .on_load(move |info: &AnimationInfo| {
                    status.set(format!("Loaded {:.1}s", info.duration))
                })
                .on_error(move |e: &String| status.set(format!("Error {e}"))),
        )
        .child(text(move || status.get()))
}

#[test]
fn plays_on_the_native_graph_of_the_engine() {
    use std::sync::{Arc, Mutex};

    use nsh_core::{MountItem, Size, animation::AnimationOp, ui::Root, ui::prelude::px};

    let mut app = nsh_testing::Headless::start(nsh_engine::EngineConfig {
        viewport: Size::new(px(200.0), px(200.0)),
        ..nsh_engine::EngineConfig::new(Root::of::<Spinner>())
    })
    .expect("the engine starts");
    let seen = Arc::new(Mutex::new((0usize, false)));
    let log = seen.clone();
    app.observe(move |_, batch| {
        let mut s = log.lock().unwrap();
        for item in &batch.items {
            match item {
                MountItem::Canvas { drawing, .. } => s.0 = s.0.max(drawing.slots.len()),
                MountItem::Animation(AnimationOp::StartComposite { .. }) => s.1 = true,
                _ => {}
            }
        }
    });
    // The dash's 1.5 s and the turn's 2 s come round together at 6 s.
    app.wait_for("Loaded 6.0s").unwrap();
    let drawn = seen.clone();
    let _ = app.wait_until(
        "the ring drawn and playback started",
        std::time::Duration::from_secs(10),
        move |_| matches!(*drawn.lock().unwrap(), (s, true) if s > 0),
    );
    let (slots, started) = *seen.lock().unwrap();
    assert!(slots > 0, "the ring's points are animated values");
    assert!(started, "it plays on the graph, not frame by frame");
}
