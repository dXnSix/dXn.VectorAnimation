//! Drop shadows and blurs. Still shadows play on the graph: a canvas under
//! the content draws it with the shadow and takes it away again, the
//! shadow's opacity (which may move) its layer's. Blurs, and shadows of
//! more than one canvas, are drawn as images the reference renderer makes.
//! (Shadows that move, or show through the content: `shadow_casting`.)

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{
    Animated, AnimatedPoint, Blur, Color, Direction, DropShadow, Easing, Effect, EllipseStart,
    Fill, Geometry, Group, Keyframe, LoopMode, Matrix, Node, Paint, RectStart, Scene, Shape,
    ShapePath, Timeline, Track, TransformOp,
};
use dxn_vector_animation_nsh::plan::{self, Op, Unit};

const TIMES: &[f64] = &[0.0, 0.5, 1.0, 1.5, 2.0];

/// The share of pixels that differ from the reference by more than a
/// tenth, at its worst over `TIMES`.
fn worst(scene: &Scene) -> f64 {
    TIMES
        .iter()
        .map(|&t| {
            common::differing_by(
                &common::plan_pixels(scene, t, 100, 100),
                &common::reference_pixels(scene, t, 100, 100),
                24,
            )
        })
        .fold(0.0, f64::max)
}

/// Kinds of units, all levels: (canvases casting shadows, rasters).
fn kinds(units: &[Unit]) -> (usize, usize) {
    units.iter().fold((0, 0), |(s, r), u| match u {
        Unit::Canvas(ops) => (
            s + usize::from(
                ops.iter()
                    .any(|op| matches!(op, Op::Shadow { color, .. } if color & 0xff > 0)),
            ),
            r,
        ),
        Unit::Wide { ops, .. } => (
            s + usize::from(
                ops.iter()
                    .any(|op| matches!(op, Op::Shadow { color, .. } if color & 0xff > 0)),
            ),
            r,
        ),
        Unit::Layer { units, .. } => {
            let (a, b) = kinds(units);
            (s + a, r + b)
        }
        Unit::Raster(_) => (s, r + 1),
        Unit::Sampled(_) => (s, r),
    })
}

fn filled(path: ShapePath, color: Color, opacity: f64) -> Node {
    Node::shape(Shape {
        paths: vec![path],
        fill: Some(Fill {
            paint: Paint::solid(color),
            opacity: Animated::Static(opacity),
            rule: Default::default(),
        }),
        ..Default::default()
    })
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> ShapePath {
    ShapePath::new(Geometry::Rect {
        center: AnimatedPoint::fixed(x + w / 2.0, y + h / 2.0),
        size: AnimatedPoint::fixed(w, h),
        radius: AnimatedPoint::fixed(4.0, 4.0),
        direction: Direction::Clockwise,
        start: RectStart::TopLeft,
    })
}

fn circle(x: f64, y: f64, r: f64) -> ShapePath {
    ShapePath::new(Geometry::Ellipse {
        center: AnimatedPoint::fixed(x, y),
        radius: AnimatedPoint::fixed(r, r),
        direction: Direction::Clockwise,
        start: EllipseStart::Top,
    })
}

const RED: Color = Color::rgba(0.9, 0.2, 0.1, 1.0);
const BLUE: Color = Color::rgba(0.1, 0.3, 0.9, 1.0);

/// A card and a disc with `effects`, over a pale ground.
fn scene(effects: Vec<Effect>, opacity: f64) -> Scene {
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
            children: vec![
                filled(
                    rect(0.0, 0.0, 100.0, 100.0),
                    Color::rgba(0.95, 0.95, 0.9, 1.0),
                    1.0,
                ),
                Node::Group(Group {
                    effects,
                    children: vec![
                        filled(rect(20.0, 15.0, 50.0, 40.0), RED, opacity),
                        filled(circle(55.0, 60.0, 20.0), BLUE, opacity),
                    ],
                    ..Group::new()
                }),
            ],
            ..Group::new()
        },
    }
}

fn shadow(alpha: Animated<f64>, offset: AnimatedPoint, blur: f64, knockout: bool) -> Effect {
    let color = match alpha {
        Animated::Static(a) => Animated::Static(Color::rgba(0.1, 0.0, 0.2, a)),
        Animated::Track(t) => Animated::Track(Track::new(
            t.keyframes
                .into_iter()
                .map(|k| Keyframe::new(k.time, Color::rgba(0.1, 0.0, 0.2, k.value), k.easing))
                .collect(),
        )),
    };
    Effect::DropShadow(DropShadow {
        color,
        offset,
        blur: Animated::Static(blur),
        knockout,
        hard: false,
    })
}

fn track(keys: &[(f64, f64)]) -> Animated<f64> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|(t, v)| Keyframe::new(*t, *v, Easing::EASE_IN_OUT))
            .collect(),
    ))
}

#[test]
fn a_still_shadow_plays_on_the_graph() {
    let s = scene(
        vec![shadow(
            Animated::Static(0.5),
            AnimatedPoint::fixed(3.0, 6.0),
            4.0,
            true,
        )],
        1.0,
    );
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    assert_eq!(kinds(&plan.units), (1, 0));
    let share = worst(&s);
    assert!(share <= 0.004, "{:.2}% of the pixels differ", share * 100.0);
}

#[test]
fn shadows_cast_together_and_one_fading_in() {
    let s = scene(
        vec![
            shadow(
                track(&[(0.0, 0.0), (2.0, 0.6)]),
                AnimatedPoint::fixed(0.0, 8.0),
                6.0,
                true,
            ),
            shadow(
                Animated::Static(0.3),
                AnimatedPoint::fixed(-4.0, 2.0),
                1.0,
                true,
            ),
        ],
        1.0,
    );
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    assert_eq!(kinds(&plan.units), (2, 0));
    let share = worst(&s);
    assert!(share <= 0.004, "{:.2}% of the pixels differ", share * 100.0);
}

#[test]
fn a_knocked_out_shadow_hides_under_see_through_content() {
    let s = scene(
        vec![shadow(
            Animated::Static(0.8),
            AnimatedPoint::fixed(6.0, 6.0),
            2.0,
            true,
        )],
        0.5,
    );
    let share = worst(&s);
    assert!(share <= 0.004, "{:.2}% of the pixels differ", share * 100.0);
}

#[test]
fn a_shadow_follows_the_groups_transform() {
    let mut s = scene(
        vec![shadow(
            Animated::Static(0.5),
            AnimatedPoint::fixed(4.0, 0.0),
            2.0,
            true,
        )],
        1.0,
    );
    if let Node::Group(g) = &mut s.root.children[1] {
        g.transform = vec![
            TransformOp::Translate {
                x: Animated::Static(100.0),
                y: Animated::Static(0.0),
            },
            TransformOp::Rotate {
                degrees: Animated::Static(90.0),
            },
        ];
    }
    let share = worst(&s);
    assert!(share <= 0.004, "{:.2}% of the pixels differ", share * 100.0);
}

#[test]
fn a_blur_is_drawn_as_the_reference_draws_it() {
    let s = scene(
        vec![Effect::Blur(Blur {
            radius: track(&[(0.0, 1.0), (2.0, 5.0)]),
        })],
        1.0,
    );
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert_eq!(plan.sampled, vec!["a blur".to_owned()]);
    assert_eq!(kinds(&plan.units), (0, 1));
    assert!(worst(&s) <= 0.001);
}

#[test]
fn a_shadow_of_layers_is_drawn_as_the_reference_draws_it() {
    // A faded group of two inside: a layer of its own, a second canvas.
    let mut s = scene(
        vec![shadow(
            Animated::Static(0.5),
            AnimatedPoint::fixed(3.0, 3.0),
            2.0,
            true,
        )],
        1.0,
    );
    if let Node::Group(g) = &mut s.root.children[1] {
        let inner = std::mem::take(&mut g.children);
        g.children = vec![Node::Group(Group {
            opacity: Animated::Static(0.7),
            children: inner,
            ..Group::new()
        })];
    }
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert_eq!(kinds(&plan.units), (0, 1));
    assert!(worst(&s) <= 0.001);
}

/// A player of `scene` (compiled), and how many canvases it mounted after
/// loading and 400 ms later.
fn canvases_while_playing(scene: Scene) -> (usize, usize) {
    use std::sync::{Arc, Mutex};

    use dxn_vector_animation_nsh::{AnimationInfo, Source, vector_animation};
    use nsh_core::{MountItem, Size, ui::Root, ui::prelude::*};

    // The player reads it on the engine's thread.
    static JSON: Mutex<String> = Mutex::new(String::new());
    *JSON.lock().unwrap() = dxn_vector_animation::compiled::to_json(&scene, false);

    #[component]
    fn Player() -> impl IntoElement {
        let status = signal(String::from("loading"));
        let json = JSON.lock().unwrap().clone();
        view()
            .child(vector_animation().src(Some(Source::Text(json))).on_load(
                move |info: &AnimationInfo| status.set(format!("Loaded {:.1}s", info.duration)),
            ))
            .child(text(move || status.get()))
    }

    let mut app = nsh_testing::Headless::start(nsh_engine::EngineConfig {
        viewport: Size::new(px(200.0), px(200.0)),
        ..nsh_engine::EngineConfig::new(Root::of::<Player>())
    })
    .expect("the engine starts");
    let seen = Arc::new(Mutex::new(0usize));
    let log = seen.clone();
    app.observe(move |_, batch| {
        let mut n = log.lock().unwrap();
        *n += batch
            .items
            .iter()
            .filter(|i| matches!(i, MountItem::Canvas { .. }))
            .count();
    });
    app.wait_for("Loaded 2.0s").unwrap();
    app.wait(std::time::Duration::from_millis(150)).unwrap();
    let before = *seen.lock().unwrap();
    app.wait(std::time::Duration::from_millis(400)).unwrap();
    (before, *seen.lock().unwrap())
}

#[test]
fn a_still_blur_is_drawn_once_and_a_moving_one_as_it_moves() {
    let still = scene(
        vec![Effect::Blur(Blur {
            radius: Animated::Static(3.0),
        })],
        1.0,
    );
    let (before, after) = canvases_while_playing(still);
    assert_eq!(before, after, "a still blur is not drawn again");
    let moving = scene(
        vec![Effect::Blur(Blur {
            radius: track(&[(0.0, 1.0), (2.0, 5.0)]),
        })],
        1.0,
    );
    let (before, after) = canvases_while_playing(moving);
    assert!(after > before + 3, "{before} → {after}");
}

#[test]
fn a_shadow_whose_blur_moves_fades_between_blurs() {
    let s = scene(
        vec![Effect::DropShadow(DropShadow {
            color: Animated::Static(Color::rgba(0.1, 0.0, 0.2, 0.5)),
            offset: AnimatedPoint::fixed(2.0, 5.0),
            blur: track(&[(0.0, 2.0), (2.0, 9.0)]),
            knockout: true,
            hard: false,
        })],
        1.0,
    );
    let plan = plan::compile(&s, Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    let (shadows, rasters) = kinds(&plan.units);
    assert!(
        (3..=10).contains(&shadows) && rasters == 0,
        "{shadows} {rasters}"
    );
    let share = worst(&s);
    assert!(share <= 0.004, "{:.2}% of the pixels differ", share * 100.0);
}
