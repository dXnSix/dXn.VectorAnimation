//! Gradients whose ends, colors or stops move play on the graph and draw
//! what the reference draws, on fills and on strokes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Easing, EllipseStart, Fill, Geometry, GradientStop,
    GradientUnits, Group, Keyframe, LineCap, LineJoin, LinearGradient, LoopMode, Matrix, Node,
    Paint, RadialGradient, RectStart, Scene, Shape, ShapePath, Spread, Stroke, Timeline, Track,
};

fn track(keys: &[(f64, f64)]) -> Animated<f64> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|(t, v)| Keyframe::new(*t, *v, Easing::EASE_IN_OUT))
            .collect(),
    ))
}

fn point(keys: &[(f64, f64, f64)]) -> AnimatedPoint {
    AnimatedPoint {
        x: track(&keys.iter().map(|k| (k.0, k.1)).collect::<Vec<_>>()),
        y: track(&keys.iter().map(|k| (k.0, k.2)).collect::<Vec<_>>()),
    }
}

fn stops(list: &[(f64, Color)]) -> Vec<GradientStop> {
    list.iter()
        .map(|&(offset, color)| GradientStop { offset, color })
        .collect()
}

fn stop_track(keys: Vec<(f64, Vec<GradientStop>)>) -> Animated<Vec<GradientStop>> {
    Animated::Track(Track::new(
        keys.into_iter()
            .map(|(t, v)| Keyframe::new(t, v, Easing::EASE_IN_OUT))
            .collect(),
    ))
}

fn linear(start: AnimatedPoint, end: AnimatedPoint, stops: Animated<Vec<GradientStop>>) -> Paint {
    Paint::LinearGradient(LinearGradient {
        start,
        end,
        stops,
        units: GradientUnits::UserSpace,
        transform: Matrix::IDENTITY,
        spread: Spread::Pad,
    })
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

fn rect(x: f64, y: f64, w: f64, h: f64) -> ShapePath {
    ShapePath::new(Geometry::Rect {
        center: AnimatedPoint::fixed(x + w / 2.0, y + h / 2.0),
        size: AnimatedPoint::fixed(w, h),
        radius: AnimatedPoint::fixed(6.0, 6.0),
        direction: Direction::Clockwise,
        start: RectStart::TopLeft,
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

fn filled(path: ShapePath, paint: Paint, opacity: f64) -> Node {
    Node::shape(Shape {
        paths: vec![path],
        fill: Some(Fill {
            paint,
            opacity: Animated::Static(opacity),
            rule: Default::default(),
        }),
        ..Default::default()
    })
}

fn stroked(path: ShapePath, paint: Paint, width: f64, cap: LineCap) -> Node {
    Node::shape(Shape {
        paths: vec![path],
        stroke: Some(Stroke {
            paint,
            opacity: Animated::Static(1.0),
            width: Animated::Static(width),
            cap,
            join: LineJoin::Round,
            miter_limit: 4.0,
            dash: None,
        }),
        ..Default::default()
    })
}

const RED: Color = Color::rgba(0.9, 0.1, 0.1, 1.0);
const GOLD: Color = Color::rgba(1.0, 0.8, 0.1, 1.0);
const BLUE: Color = Color::rgba(0.1, 0.3, 0.9, 1.0);
const WHITE: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);

const TIMES: &[f64] = &[0.0, 0.2, 0.5, 0.8, 1.0, 1.3, 1.6, 2.0];

/// The plan compiles natively and draws what the reference does to a few
/// pixels, at twice the size.
fn assert_plan_matches(scene: &Scene) {
    dxn_vector_animation_nsh::plan::compile(scene, Matrix::IDENTITY).expect("plays natively");
    let (w, h) = (200, 200);
    for &t in TIMES {
        let ours = common::plan_pixels(scene, t, w, h);
        let reference = common::reference_pixels(scene, t, w, h);
        let count = (common::differing(&ours, &reference) * f64::from(w * h)).round();
        if count > 40.0 {
            let dir = std::env::temp_dir();
            let _ = ours.save_png(dir.join(format!("plan-{t}.png")));
            let _ = reference.save_png(dir.join(format!("reference-{t}.png")));
            panic!("at t = {t}: {count} pixels differ (see {})", dir.display());
        }
    }
}

#[test]
fn linear_gradient_whose_ends_move() {
    let paint = linear(
        point(&[(0.0, 10.0, 10.0), (2.0, 60.0, 30.0)]),
        point(&[(0.0, 90.0, 20.0), (1.0, 30.0, 90.0), (2.0, 70.0, 80.0)]),
        Animated::Static(stops(&[(0.0, RED), (0.4, GOLD), (1.0, BLUE)])),
    );
    let shape = filled(rect(10.0, 10.0, 80.0, 80.0), paint, 0.8);
    assert_plan_matches(&scene(vec![shape]));
}

#[test]
fn radial_gradient_whose_circle_and_focal_point_move() {
    // The focal point a third of the radius out, turning with the circle.
    let center = point(&[(0.0, 40.0, 40.0), (2.0, 60.0, 55.0)]);
    let radius = track(&[(0.0, 20.0), (2.0, 45.0)]);
    let at = |t: f64| {
        let (c, r) = (center.sample(t), radius.sample(t));
        let a = t * 1.5;
        (c.x + r / 3.0 * a.cos(), c.y + r / 3.0 * a.sin())
    };
    let times: Vec<f64> = (0..=40).map(|i| f64::from(i) * 0.05).collect();
    let focal = AnimatedPoint {
        x: Animated::Track(Track::new(
            times
                .iter()
                .map(|&t| Keyframe::new(t, at(t).0, Easing::Linear))
                .collect(),
        )),
        y: Animated::Track(Track::new(
            times
                .iter()
                .map(|&t| Keyframe::new(t, at(t).1, Easing::Linear))
                .collect(),
        )),
    };
    let paint = Paint::RadialGradient(RadialGradient {
        center: AnimatedPoint {
            x: Animated::Track(Track::new(
                times
                    .iter()
                    .map(|&t| Keyframe::new(t, center.sample(t).x, Easing::Linear))
                    .collect(),
            )),
            y: Animated::Track(Track::new(
                times
                    .iter()
                    .map(|&t| Keyframe::new(t, center.sample(t).y, Easing::Linear))
                    .collect(),
            )),
        },
        radius: Animated::Track(Track::new(
            times
                .iter()
                .map(|&t| Keyframe::new(t, radius.sample(t), Easing::Linear))
                .collect(),
        )),
        focal,
        stops: Animated::Static(stops(&[(0.0, WHITE), (0.5, GOLD), (1.0, RED)])),
        units: GradientUnits::UserSpace,
        transform: Matrix::IDENTITY,
        spread: Spread::Pad,
    });
    let shape = filled(ellipse(50.0, 50.0, 45.0), paint, 1.0);
    assert_plan_matches(&scene(vec![shape]));
}

#[test]
fn gradient_colors_that_change() {
    let colors = stop_track(vec![
        (
            0.0,
            stops(&[(0.1, BLUE), (1.0, Color::rgba(1.0, 1.0, 1.0, 0.0))]),
        ),
        (
            1.0,
            stops(&[(0.1, RED), (1.0, Color::rgba(1.0, 1.0, 1.0, 0.0))]),
        ),
        (
            2.0,
            stops(&[(0.1, GOLD), (1.0, Color::rgba(1.0, 1.0, 1.0, 0.0))]),
        ),
    ]);
    let fill = filled(
        rect(10.0, 10.0, 80.0, 35.0),
        linear(
            AnimatedPoint::fixed(10.0, 0.0),
            AnimatedPoint::fixed(90.0, 0.0),
            colors.clone(),
        ),
        0.9,
    );
    // On a stroke, its ends moving as well.
    let stroke = stroked(
        ellipse(50.0, 72.0, 18.0),
        linear(
            point(&[(0.0, 30.0, 60.0), (2.0, 40.0, 90.0)]),
            point(&[(0.0, 70.0, 80.0), (2.0, 75.0, 55.0)]),
            colors,
        ),
        6.0,
        LineCap::Round,
    );
    assert_plan_matches(&scene(vec![fill, stroke]));
}

/// As Figma's coin: a light band running along the gradient, narrowing, on
/// its fill and its rim.
fn coin_scene() -> Scene {
    let gold = Color::rgba(0.96, 0.72, 0.0, 1.0);
    let light = Color::rgba(1.0, 0.91, 0.64, 1.0);
    let sweep = Animated::Track(Track::new(vec![
        Keyframe::new(
            0.0,
            stops(&[(0.96, gold), (0.985, light), (1.0, gold)]),
            Easing::cubic_bezier(0.535, -0.005, 0.455, 0.985),
        ),
        Keyframe::new(
            1.0,
            stops(&[(0.26, gold), (0.49, light), (0.72, gold)]),
            Easing::cubic_bezier(0.535, -0.005, 0.455, 0.985),
        ),
        Keyframe::new(
            2.0,
            stops(&[(0.0, gold), (0.015, light), (0.028, gold)]),
            Easing::Linear,
        ),
    ]));
    let coin = filled(
        ellipse(50.0, 50.0, 40.0),
        linear(
            AnimatedPoint::fixed(20.0, 80.0),
            AnimatedPoint::fixed(80.0, 20.0),
            sweep.clone(),
        ),
        0.85,
    );
    let rim = stroked(
        ellipse(50.0, 50.0, 44.0),
        linear(
            AnimatedPoint::fixed(10.0, 90.0),
            AnimatedPoint::fixed(90.0, 10.0),
            sweep,
        ),
        4.0,
        LineCap::Butt,
    );
    scene(vec![coin, rim])
}

#[test]
fn shine_whose_stops_sweep_across() {
    assert_plan_matches(&coin_scene());
}

#[test]
fn opaque_bands_overlap_instead_of_meeting() {
    // Edges of bands that meet leave a seam where a platform's
    // anti-aliasing does not add up (Android's): opaque bands color the
    // shape drawn once, each over the one before.
    use dxn_vector_animation_nsh::plan::{Op, Unit};
    let plan = dxn_vector_animation_nsh::plan::compile(&coin_scene(), Matrix::IDENTITY).unwrap();
    let ops: Vec<&Op> = plan
        .units
        .iter()
        .flat_map(|u| match u {
            Unit::Canvas(ops) | Unit::Wide { ops, .. } => ops.iter().collect::<Vec<_>>(),
            Unit::Layer { .. } | Unit::Sampled(_) | Unit::Raster(_) => Vec::new(),
        })
        .collect();
    assert!(ops.iter().any(|op| matches!(op, Op::Atop)));
    assert!(
        !ops.iter()
            .any(|op| matches!(op, Op::Blend(dxn_vector_animation::BlendMode::Add)))
    );
}

#[test]
fn what_stands_for_everything_stays_within_reach() {
    // Cairo (the Linux canvas) places nothing past about ±8 million device
    // pixels: the bands of a gradient whose length changes a hundredfold,
    // drawn large, stay well within that.
    let light = Color::rgba(1.0, 0.91, 0.64, 1.0);
    let sweep = stop_track(vec![
        (0.0, stops(&[(0.2, GOLD), (0.3, light), (0.4, GOLD)])),
        (2.0, stops(&[(0.6, GOLD), (0.7, light), (0.8, GOLD)])),
    ]);
    let paint = linear(
        point(&[(0.0, 50.0, 50.0), (2.0, 10.0, 10.0)]),
        point(&[(0.0, 50.5, 50.5), (2.0, 90.0, 90.0)]),
        sweep,
    );
    let s = scene(vec![filled(ellipse(50.0, 50.0, 40.0), paint, 1.0)]);
    for t in [0.0, 0.5, 1.0, 2.0] {
        let widest = common::widest_point(&s, t, 1200, 1200);
        assert!(widest < 2.0e6, "at t = {t}: {widest} device pixels out");
    }
}

#[test]
fn transformed_gradient_on_a_stroke() {
    // As svg-loaders' tail spin: a gradient in the stroke's bounding box.
    let paint = Paint::LinearGradient(LinearGradient {
        start: AnimatedPoint::fixed(0.0, 0.0),
        end: AnimatedPoint::fixed(1.0, 0.0),
        stops: Animated::Static(stops(&[
            (0.0, Color::rgba(0.1, 0.3, 0.9, 0.0)),
            (0.6, Color::rgba(0.1, 0.3, 0.9, 0.6)),
            (1.0, BLUE),
        ])),
        units: GradientUnits::BoundingBox,
        transform: Matrix::IDENTITY,
        spread: Spread::Pad,
    });
    let ring = stroked(ellipse(50.0, 50.0, 30.0), paint, 8.0, LineCap::Round);
    assert_plan_matches(&scene(vec![ring]));
}

/// The coin in a player, as a compiled scene.
#[nsh_core::ui::prelude::component]
fn Coin() -> impl nsh_core::ui::prelude::IntoElement {
    use dxn_vector_animation_nsh::{AnimationInfo, Source, vector_animation};
    use nsh_core::ui::prelude::*;
    let json = dxn_vector_animation::compiled::to_json(&coin_scene(), false);
    let status = signal(String::from("loading"));
    view()
        .child(
            vector_animation()
                .src(Some(Source::Text(json)))
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
        ..nsh_engine::EngineConfig::new(Root::of::<Coin>())
    })
    .expect("the engine starts");
    let seen = Arc::new(Mutex::new((0usize, false)));
    let log = seen.clone();
    app.observe(move |_, batch| {
        let mut s = log.lock().unwrap();
        for item in &batch.items {
            match item {
                MountItem::Canvas { .. } => s.0 += 1,
                MountItem::Animation(AnimationOp::StartComposite { .. }) => s.1 = true,
                _ => {}
            }
        }
    });
    app.wait_for("Loaded 2.0s").unwrap();
    app.wait(std::time::Duration::from_millis(100)).unwrap();
    let (canvases, started) = *seen.lock().unwrap();
    assert!(
        canvases >= 2,
        "each shine on a canvas of its own: {canvases}"
    );
    assert!(started, "it plays on the graph, not frame by frame");
}
