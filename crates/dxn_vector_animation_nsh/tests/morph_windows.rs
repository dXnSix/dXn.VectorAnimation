//! A shape whose paths morph between different commands is a copy for each
//! time between switches, up to 32 copies; one that switches more often (a
//! merge baked at every frame) is sampled frame by frame.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{differing, plan_pixels, reference_pixels};
use dxn_vector_animation::{
    Animated, Color, Easing, Fill, Geometry, Group, Keyframe, LoopMode, Matrix, Node, Paint, Path,
    Point, Scene, Shape, ShapePath, Timeline, Track,
};
use dxn_vector_animation_nsh::plan;

/// A polygon of `corners` corners round the middle, turned by `turn`.
fn polygon(corners: usize, turn: f64) -> Path {
    let mut path = Path::new();
    for i in 0..corners {
        let a = turn + std::f64::consts::TAU * i as f64 / corners as f64;
        let p = Point::new(50.0 + 40.0 * a.cos(), 50.0 + 40.0 * a.sin());
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path.close();
    path
}

/// A filled outline of `keys` keyframes over two seconds, each of three to
/// eight corners in turn (so its commands switch at every keyframe), as a
/// merge baked at every frame is.
fn scene(keys: usize) -> Scene {
    let keyframes = (0..keys)
        .map(|k| {
            let t = 2.0 * k as f64 / (keys - 1) as f64;
            Keyframe::new(t, polygon(3 + k % 6, t), Easing::Linear)
        })
        .collect();
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Loop,
            markers: vec![],
            cues: Vec::new(),
        },
        root: Group {
            children: vec![Node::shape(Shape {
                paths: vec![ShapePath::new(Geometry::Path {
                    path: Animated::Track(Track::new(keyframes)),
                })],
                fill: Some(Fill {
                    paint: Paint::solid(Color::rgba(0.9, 0.3, 0.1, 1.0)),
                    opacity: Animated::Static(1.0),
                    rule: Default::default(),
                }),
                ..Default::default()
            })],
            ..Group::new()
        },
    }
}

fn assert_drawn_as_the_reference(scene: &Scene) {
    for k in 0..=20 {
        let t = f64::from(k) * 0.1 + 0.013;
        let share = differing(
            &plan_pixels(scene, t, 100, 100),
            &reference_pixels(scene, t, 100, 100),
        );
        assert!(share <= 0.004, "at t = {t}: {:.2}% differ", share * 100.0);
    }
}

#[test]
fn a_shape_switching_commands_a_few_times_is_a_copy_for_each_time() {
    let scene = scene(32);
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert!(plan.sampled.is_empty(), "{:?}", plan.sampled);
    assert_drawn_as_the_reference(&scene);
}

#[test]
fn a_shape_switching_commands_at_every_frame_is_sampled() {
    // 120 switches: as many copies would draw every one of them every frame.
    let scene = scene(121);
    let plan = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    assert_eq!(
        plan.sampled,
        vec!["a shape whose commands change many times".to_owned()]
    );
    assert_drawn_as_the_reference(&scene);
}
