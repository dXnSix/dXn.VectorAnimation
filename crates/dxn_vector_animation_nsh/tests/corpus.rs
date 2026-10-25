//! Every animation of a folder (`DXN_CORPUS`), compiled and compared with
//! the reference renderer at several times: a check against real files,
//! run on demand (`DXN_CORPUS=… cargo test --test corpus -- --ignored
//! --nocapture`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else if path.extension().is_some_and(|e| e == "svg" || e == "json") {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "needs DXN_CORPUS"]
fn corpus_plans_match_their_frames() {
    let Ok(dir) = std::env::var("DXN_CORPUS") else {
        return;
    };
    let mut list = Vec::new();
    files(Path::new(&dir), &mut list);
    list.sort();
    let mut worst = 0.0f64;
    for path in list {
        let name = path
            .strip_prefix(&dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        let Ok(loaded) = dxn_vector_animation_nsh::source::read(&std::fs::read(&path).unwrap())
        else {
            println!("{name}: not read");
            continue;
        };
        let scene = loaded.scene;
        let base = dxn_vector_animation::Matrix::IDENTITY;
        let plan = dxn_vector_animation_nsh::plan::compile(&scene, base);
        let plan = match plan {
            Ok(plan) => plan,
            Err(why) => {
                println!("{name}: sampled ({})", why.0);
                continue;
            }
        };
        let w: u32 = std::env::var("DXN_CORPUS_WIDTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);
        let h = ((scene.height / scene.width.max(1.0)) * f64::from(w))
            .round()
            .clamp(1.0, 2000.0) as u32;
        let d = scene.timeline.duration;
        let mut share: f64 = 0.0;
        for k in 0..5 {
            let t = d * f64::from(k) / 4.0 * 0.999;
            let ours = common::plan_pixels(&scene, t, w, h);
            let reference = common::reference_pixels(&scene, t, w, h);
            let diff = common::differing(&ours, &reference);
            if diff > 0.01
                && let Ok(save) = std::env::var("DXN_CORPUS_SAVE")
            {
                let stem = name.replace('/', "_");
                let _ = ours.save_png(Path::new(&save).join(format!("{stem}-{k}-plan.png")));
                let _ =
                    reference.save_png(Path::new(&save).join(format!("{stem}-{k}-reference.png")));
            }
            share = share.max(diff);
        }
        worst = worst.max(share);
        let parts = if plan.sampled.is_empty() {
            String::new()
        } else {
            format!(", parts sampled ({})", plan.sampled.join("; "))
        };
        println!(
            "{name}: native{parts}, up to {:.2}% of pixels differ",
            share * 100.0
        );
    }
    println!("worst: {:.2}%", worst * 100.0);
}
