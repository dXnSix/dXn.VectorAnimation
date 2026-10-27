//! `profile <dir> [--size <points>] [--ratio <pixels a point>]`: what
//! playing each animation of a folder costs the player. On the graph:
//! how long its plan takes to compile, its nodes and breakpoints, and
//! what each frame draws (canvases, layers, operations). On the engine
//! thread, each frame: the parts sampled frame by frame and the rasters
//! of parts that move (blurs), timed; still rasters once. How long the
//! file takes to read, and for a Rive file, the longest a pointer acting
//! on one of its listeners takes (each change reads the scene again) and
//! what a pointer sweeping over it costs a move.

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use dxn_vector_animation::{Point, Scene};
use dxn_vector_animation_nsh::{
    graph::Expr,
    plan::{self, Op, Unit},
};

use dxn_vector_animation_rive::{Interaction, Pointer};

use crate::option;

/// What a plan holds.
#[derive(Default)]
struct Count {
    canvases: usize,
    layers: usize,
    ops: usize,
    sampled: Vec<std::sync::Arc<Scene>>,
    rasters: Vec<std::sync::Arc<Scene>>,
}

fn count(units: &[Unit], c: &mut Count) {
    for unit in units {
        match unit {
            Unit::Canvas(ops) | Unit::Wide { ops, .. } | Unit::Area { ops, .. } => {
                c.canvases += 1;
                c.ops += ops
                    .iter()
                    .filter(|op| !matches!(op, Op::Save | Op::Restore))
                    .count();
            }
            Unit::Layer { units, .. } => {
                c.layers += 1;
                count(units, c);
            }
            Unit::Sampled(part) => c.sampled.push(part.clone()),
            Unit::Raster(part) => c.rasters.push(part.clone()),
        }
    }
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e == "svg" || e == "json" || e == "lottie" || e == "riv")
        {
            out.push(path);
        }
    }
}

/// Milliseconds `f` takes, at its best of `runs`.
fn timed(runs: usize, mut f: impl FnMut()) -> f64 {
    (0..runs.max(1))
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .fold(f64::INFINITY, f64::min)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let dir = PathBuf::from(args.first().ok_or("which folder?")?);
    let size: f64 = option(args, "--size")
        .map(str::parse)
        .transpose()
        .map_err(|e| format!("--size: {e}"))?
        .unwrap_or(200.0);
    let ratio: f64 = option(args, "--ratio")
        .map(str::parse)
        .transpose()
        .map_err(|e| format!("--ratio: {e}"))?
        .unwrap_or(3.0);
    let mut list = Vec::new();
    files(&dir, &mut list);
    list.sort();
    println!(
        "{:<44} {:>8} {:>8} {:>10} {:>8} {:>6} {:>7} {:>4} {:>4} {:>6} {:>8} {:>9}",
        "file",
        "read",
        "act",
        "sweep",
        "compile",
        "nodes",
        "breaks",
        "cnv",
        "lyr",
        "ops",
        "sampled",
        "raster"
    );
    println!(
        "{:<44} {:>8} {:>8} {:>10} {:>8} {:>6} {:>7} {:>4} {:>4} {:>6} {:>8} {:>9}",
        "", "ms", "ms", "mean/max", "ms", "", "", "", "", "/frame", "ms/frame", "ms/frame"
    );
    for path in list {
        let name = path
            .strip_prefix(&dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        let name: String = name
            .chars()
            .rev()
            .take(44)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let mut loaded = None;
        let read = timed(1, || {
            loaded = Some(dxn_vector_animation_nsh::source::read(&bytes))
        });
        let Some(Ok(loaded)) = loaded else {
            println!("{name:<44} not read");
            continue;
        };
        let (act, sweep) =
            loaded
                .interaction
                .map_or((String::from("-"), String::from("-")), |mut play| {
                    let act = acting(&mut play);
                    let (mean, longest) = sweeping(&mut play, size, ratio);
                    (format!("{act:.1}"), format!("{mean:.1}/{longest:.0}"))
                });
        let scene = loaded.scene;
        let (w, h) = if scene.width >= scene.height {
            (size, size * scene.height / scene.width.max(1.0))
        } else {
            (size * scene.width / scene.height.max(1.0), size)
        };
        let base = dxn_vector_animation_nsh::fit(&scene, w, h, Default::default());
        let mut compiled = None;
        let compile = timed(3, || compiled = Some(plan::compile(&scene, base)));
        let Some(Ok(plan)) = compiled else {
            // Sampled whole: every frame on the engine thread.
            let d = scene.timeline.duration;
            let frame = timed(1, || {
                for k in 0..30 {
                    let t = d * f64::from(k) / 30.0;
                    let _ = plan::from_frame(&scene.frame_at(t), base);
                }
            }) / 30.0;
            println!(
                "{name:<44} {read:>8.1} {act:>8} {sweep:>10} {compile:>8.2} {:>6} {:>7} {:>4} {:>4} {:>6} {frame:>8.3} {:>9}",
                "-", "-", "-", "-", "-", "-"
            );
            continue;
        };
        let breaks: usize = plan
            .graph
            .nodes()
            .iter()
            .map(|n| match n {
                Expr::Interp { inputs, .. } => inputs.len(),
                Expr::InterpColor { inputs, .. } => inputs.len(),
                _ => 0,
            })
            .sum();
        let mut c = Count::default();
        count(&plan.units, &mut c);
        let d = scene.timeline.duration;
        let sampled = if c.sampled.is_empty() {
            0.0
        } else {
            timed(1, || {
                for k in 0..30 {
                    let t = d * f64::from(k) / 30.0;
                    for part in &c.sampled {
                        let _ = plan::from_frame(&part.frame_at(t), base);
                    }
                }
            }) / 30.0
        };
        let moving: Vec<&std::sync::Arc<Scene>> =
            c.rasters.iter().filter(|p| !plan::still(p)).collect();
        let raster = if moving.is_empty() {
            0.0
        } else {
            timed(1, || {
                for k in 0..10 {
                    let t = d * f64::from(k) / 10.0;
                    for part in &moving {
                        let view = plan::in_view(part, base, w, h);
                        let _ = plan::raster(&view, t, ratio);
                    }
                }
            }) / 10.0
        };
        let rasters = if c.rasters.is_empty() {
            String::from("0")
        } else {
            format!(
                "{raster:.2}{}",
                if moving.len() < c.rasters.len() {
                    "+"
                } else {
                    ""
                }
            )
        };
        println!(
            "{name:<44} {read:>8.1} {act:>8} {sweep:>10} {compile:>8.2} {:>6} {breaks:>7} {:>4} {:>4} {:>6} {sampled:>8.3} {rasters:>9}",
            plan.graph.nodes().len(),
            c.canvases,
            c.layers,
            c.ops,
        );
    }
    Ok(())
}

/// The longest a pointer takes to act on a Rive file's listeners, in
/// milliseconds: moved onto, pressed and released on the middle of each
/// listener's targets, one after another.
fn acting(play: &mut Interaction) -> f64 {
    let points: Vec<Point> = play
        .machine()
        .map(|m| {
            let scene = &play.import().scene;
            m.all()
                .iter()
                .flat_map(|m| &m.listeners)
                .filter_map(|l| {
                    l.targets
                        .iter()
                        .filter_map(|id| scene.hit_bounds(id, 0.0))
                        .reduce(|a, b| a.union(b))
                })
                .map(|b| Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0))
                .collect()
        })
        .unwrap_or_default();
    let mut longest = 0.0_f64;
    let mut time = 0.5;
    for point in points.into_iter().take(8) {
        for kind in [Pointer::Move, Pointer::Down, Pointer::Up] {
            longest = longest.max(timed(1, || {
                let _ = play.pointer(time, kind, point);
            }));
            time += 0.1;
        }
    }
    longest
}

/// What a pointer sweeping over a Rive file costs a move, in
/// milliseconds: the mean and the longest of 200 moves zigzagging over the
/// scene, 60 a second, each change compiled again with its rasters (as the
/// player does, `size` points at `ratio` pixels a point).
fn sweeping(play: &mut Interaction, size: f64, ratio: f64) -> (f64, f64) {
    let (w, h) = (play.import().scene.width, play.import().scene.height);
    let mut total = 0.0;
    let mut longest = 0.0_f64;
    let n = 200;
    for k in 0..n {
        let u = f64::from(k) / f64::from(n);
        let point = Point::new(w * (0.1 + 0.8 * (u * 7.0).fract()), h * (0.1 + 0.8 * u));
        let time = 10.0 + f64::from(k) / 60.0;
        let ms = timed(1, || {
            if play.pointer(time, Pointer::Move, point).unwrap_or(false) {
                let scene = &play.import().scene;
                let base = dxn_vector_animation_nsh::fit(scene, size, size, Default::default());
                if let Ok(plan) = plan::compile(scene, base) {
                    let mut c = Count::default();
                    count(&plan.units, &mut c);
                    for part in &c.rasters {
                        let view = plan::in_view(part, base, size, size);
                        let _ = plan::raster(&view, time, ratio);
                    }
                }
            }
        });
        total += ms;
        longest = longest.max(ms);
    }
    (total / f64::from(n), longest)
}
