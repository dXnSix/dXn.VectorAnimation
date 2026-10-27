//! `bench <dir> [--runs <n>] [--size <points>] [--ratio <pixels a point>]
//! [--files] [--values]`: what loading and playing every animation of a
//! folder costs, summed over the folder, the best of `runs` each. Time to
//! first frame is what the player does from the file's bytes to its first
//! drawing (reading, compiling, the first rasters and sampled frames);
//! held bytes are what stays allocated for it after. A frame is the
//! engine thread's work (parts sampled frame by frame, rasters of parts
//! that move) and, apart, the reference's `frame_at`. The graph's size is
//! the UI thread's work each frame. With `--values`, JSON files are read as
//! the values an app imports them as.

use std::{
    alloc::System,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use dxn_vector_animation::Scene;
use dxn_vector_animation_nsh::{
    graph::Expr,
    plan::{self, Op, Unit},
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

use crate::option;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// Frames sampled over a playback.
const FRAMES: usize = 60;

/// What something cost: time, allocations, bytes allocated.
#[derive(Clone, Copy, Default)]
struct Cost {
    ms: f64,
    allocs: usize,
    bytes: usize,
}

impl std::ops::AddAssign for Cost {
    fn add_assign(&mut self, o: Cost) {
        self.ms += o.ms;
        self.allocs += o.allocs;
        self.bytes += o.bytes;
    }
}

/// What `f` costs, at its best of `runs` (allocations are the same each
/// run), and what it made the last time.
fn measured<T>(runs: usize, mut f: impl FnMut() -> T) -> (Cost, T) {
    let mut once = || {
        let region = Region::new(GLOBAL);
        let start = Instant::now();
        let made = f();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let stats = region.change();
        let cost = Cost {
            ms,
            allocs: stats.allocations + stats.reallocations,
            bytes: stats.bytes_allocated,
        };
        (cost, made)
    };
    let (mut best, mut out) = once();
    for _ in 1..runs {
        drop(out);
        let (cost, made) = once();
        best.ms = best.ms.min(cost.ms);
        out = made;
    }
    (best, out)
}

/// One file's numbers.
#[derive(Default)]
struct Row {
    read: Cost,
    compile: Cost,
    first: Cost,
    held: usize,
    frame: Cost,
    engine: Cost,
    nodes: usize,
    breaks: usize,
    ops: usize,
    canvases: usize,
    layers: usize,
}

impl std::ops::AddAssign<&Row> for Row {
    fn add_assign(&mut self, o: &Row) {
        self.read += o.read;
        self.compile += o.compile;
        self.first += o.first;
        self.held += o.held;
        self.frame += o.frame;
        self.engine += o.engine;
        self.nodes += o.nodes;
        self.breaks += o.breaks;
        self.ops += o.ops;
        self.canvases += o.canvases;
        self.layers += o.layers;
    }
}

impl Row {
    fn ttff(&self) -> Cost {
        let mut c = self.read;
        c += self.compile;
        c += self.first;
        c
    }
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e == "svg" || e == "json" || e == "lottie")
        {
            out.push(path);
        }
    }
}

/// The parts of `units` sampled frame by frame and rastered, and what
/// the canvases hold.
fn parts(
    units: &[Unit],
    sampled: &mut Vec<Arc<Scene>>,
    rasters: &mut Vec<Arc<Scene>>,
    row: &mut Row,
) {
    for unit in units {
        match unit {
            Unit::Canvas(ops) | Unit::Wide { ops, .. } | Unit::Area { ops, .. } => {
                row.canvases += 1;
                row.ops += ops
                    .iter()
                    .filter(|op| !matches!(op, Op::Save | Op::Restore))
                    .count();
            }
            Unit::Layer { units, .. } => {
                row.layers += 1;
                parts(units, sampled, rasters, row);
            }
            Unit::Sampled(part) => sampled.push(part.clone()),
            Unit::Raster(part) => rasters.push(part.clone()),
        }
    }
}

/// The view `scene` fits into, `size` points on its longer side.
fn view_size(scene: &Scene, size: f64) -> (f64, f64) {
    if scene.width >= scene.height {
        (size, size * scene.height / scene.width.max(1.0))
    } else {
        (size * scene.width / scene.height.max(1.0), size)
    }
}

/// What playing the file of `bytes` costs.
fn bench(bytes: &[u8], values: bool, runs: usize, size: f64, ratio: f64) -> Option<Row> {
    let mut row = Row::default();
    let held = Region::new(GLOBAL);
    // Time to first frame, as the player goes: read, compiled for the view,
    // the first rasters and sampled frames made.
    // A JSON file as a value (a `.json` import) where asked: read from the
    // value, made beforehand.
    let value = values
        .then(|| serde_json::from_slice::<serde_json::Value>(bytes).ok())
        .flatten();
    let (read, loaded) = measured(runs, || match &value {
        Some(value) => dxn_vector_animation_nsh::source::read_json(value),
        None => dxn_vector_animation_nsh::source::read(bytes),
    });
    row.read = read;
    let scene = loaded.ok()?.scene;
    let (w, h) = view_size(&scene, size);
    let base = dxn_vector_animation_nsh::fit(&scene, w, h, Default::default());
    let (compile, compiled) = measured(runs, || plan::compile_for(&scene, base, ratio).ok());
    row.compile = compile;
    let mut sampled = Vec::new();
    let mut rasters = Vec::new();
    let mut whole = None;
    let plan = match compiled {
        Some(plan) => {
            parts(&plan.units, &mut sampled, &mut rasters, &mut row);
            row.nodes = plan.graph.nodes().len();
            row.breaks = plan
                .graph
                .nodes()
                .iter()
                .map(|n| match n {
                    Expr::Interp { inputs, .. } => inputs.len(),
                    Expr::InterpColor { inputs, .. } => inputs.len(),
                    _ => 0,
                })
                .sum();
            Some(plan)
        }
        _ => {
            // Sampled whole.
            whole = Some(Arc::new(scene.clone()));
            None
        }
    };
    if plan::needs_raster(&dxn_vector_animation::Node::Group(scene.root.clone()))
        && let Some(whole) = whole.take()
    {
        rasters.push(whole);
    }
    sampled.extend(whole);
    let (first, views) = measured(runs, || {
        let views: Vec<(Scene, bool)> = rasters
            .iter()
            .map(|part| {
                let view = plan::in_view(part, base, w, h);
                let still = plan::still(&view);
                let _ = plan::raster(&view, 0.0, ratio);
                (view, still)
            })
            .collect();
        for part in &sampled {
            let _ = plan::from_frame(&part.frame_at(0.0), base);
        }
        views
    });
    row.first = first;
    let change = held.change();
    // (Reallocations count in both as they grow or shrink.)
    row.held = change
        .bytes_allocated
        .saturating_sub(change.bytes_deallocated);
    // A frame: the engine thread's work, and the reference's `frame_at`.
    let d = scene.timeline.duration;
    let at = |k: usize| d * k as f64 / FRAMES as f64;
    let (engine, ()) = measured(runs, || {
        for k in 0..FRAMES {
            let t = at(k);
            for part in &sampled {
                let _ = plan::from_frame(&part.frame_at(t), base);
            }
            for (view, still) in &views {
                if !still {
                    let _ = plan::raster(view, t, ratio);
                }
            }
        }
    });
    row.engine = per_frame(engine);
    let (frame, ()) = measured(runs, || {
        for k in 0..FRAMES {
            let _ = scene.frame_at(at(k));
        }
    });
    row.frame = per_frame(frame);
    drop(plan);
    Some(row)
}

fn per_frame(c: Cost) -> Cost {
    Cost {
        ms: c.ms / FRAMES as f64,
        allocs: c.allocs / FRAMES,
        bytes: c.bytes / FRAMES,
    }
}

fn line(name: &str, r: &Row) {
    let ttff = r.ttff();
    println!(
        "{name:<36} {:>8.2} {:>8.2} {:>8.2} {:>8} {:>8.2} {:>8.2} {:>8.1} {:>7} {:>8.1} {:>7} {:>6} {:>7} {:>6} {:>4} {:>4}",
        r.read.ms,
        r.compile.ms,
        ttff.ms,
        ttff.allocs,
        ttff.bytes as f64 / 1e6,
        r.held as f64 / 1e6,
        r.frame.ms * 1000.0,
        r.frame.allocs,
        r.engine.ms * 1000.0,
        r.engine.allocs,
        r.nodes,
        r.breaks,
        r.ops,
        r.canvases,
        r.layers,
    );
}

pub fn run(args: &[String]) -> Result<(), String> {
    let dir = PathBuf::from(args.first().ok_or("which folder?")?);
    let number = |flag: &str, default: f64| -> Result<f64, String> {
        option(args, flag)
            .map(str::parse)
            .transpose()
            .map_err(|e| format!("{flag}: {e}"))
            .map(|v| v.unwrap_or(default))
    };
    let runs = number("--runs", 5.0)? as usize;
    let size = number("--size", 200.0)?;
    let ratio = number("--ratio", 3.0)?;
    let each = args.iter().any(|a| a == "--files");
    let values = args.iter().any(|a| a == "--values");
    let mut list = Vec::new();
    files(&dir, &mut list);
    list.sort();
    println!(
        "{:<36} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>7} {:>8} {:>7} {:>6} {:>7} {:>6} {:>4} {:>4}",
        "file",
        "read",
        "compile",
        "ttff",
        "allocs",
        "MB",
        "held MB",
        "frame",
        "allocs",
        "engine",
        "allocs",
        "nodes",
        "breaks",
        "ops",
        "cnv",
        "lyr"
    );
    println!(
        "{:<36} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>7} {:>8} {:>7}",
        "", "ms", "ms", "ms", "ttff", "ttff", "", "us", "/frame", "us", "/frame"
    );
    let mut total = Row::default();
    let mut read = 0;
    for path in &list {
        let name = path
            .strip_prefix(&dir)
            .unwrap_or(path)
            .display()
            .to_string();
        let name: String = name
            .chars()
            .rev()
            .take(36)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Some(row) = bench(&bytes, values, runs, size, ratio) else {
            println!("{name:<36} not read");
            continue;
        };
        read += 1;
        if each {
            line(&name, &row);
        }
        total += &row;
    }
    line(&format!("TOTAL ({read} files)"), &total);
    Ok(())
}
