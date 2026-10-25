//! `vector-animation-check`: the device checks of the player.
//!
//! - `scenes <app> [--corpus <dir>]` writes the check app's animations: the
//!   check set's own scenes (`scenes.rs`), and, from a corpus folder, the
//!   files of `CORPUS` that are there.
//! - `compare <shots> <app> [--tile <points>] [--out <dir>]` finds the tiles
//!   in each screenshot `frame-<page>-<percent>.png` the check app's test
//!   took, renders the reference frame of each at the tile's size, and
//!   reports the share of pixels that differ (straight, and at the best
//!   shift of a pixel, as a border of a fraction of a pixel moves the
//!   content), writing each pair side by side.
//! - `profile <dir> [--size <points>] [--ratio <pixels>]` reports what
//!   playing each file of a folder costs (`profile.rs`).

mod compare;
mod profile;
mod scenes;

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

/// Tiles on a page of the check app.
pub const PAGE: usize = 12;

/// The corpus files the check set takes when the corpus is there: one or
/// more for each kind of animation Figma Motion and the others export.
const CORPUS: &[(&str, &str)] = &[
    (
        "c-trim",
        "svg/figma/05-pixel-wall-welle-path-trim/welle-zeichnen.svg",
    ),
    (
        "c-ring",
        "svg/smil/01-svg-spinners-ring-resize/ring-resize.svg",
    ),
    ("c-tail", "svg/smil/03-svg-loaders-tail-spin/tail-spin.svg"),
    ("c-coin", "lottie/figma/08-plugin-coin/coin.json"),
    (
        "c-tristate",
        "lottie/figma/13-plugin-tristate/tristate.json",
    ),
    ("c-blend", "lottie/figma/14-plugin-blend-28/complete.json"),
    (
        "c-variant",
        "lottie/figma/11-plugin-variant-component/component1.json",
    ),
    ("c-masking", "lottie/figma/09-plugin-masking/masking.json"),
    (
        "c-trims",
        "lottie/samples/04-trim-modes/skottie-trimpath-modes.json",
    ),
    (
        "c-hatchet",
        "svg/figma/07-hatchet-illustration-mask/hatchet-5.svg",
    ),
    (
        "c-mascot",
        "svg/figma/09-bountydesk-mascot-offset-paths/out-of-scope.svg",
    ),
    (
        "c-onboard",
        "svg/figma/03-stampo-onboarding-lite/onboarding-lite.svg",
    ),
    (
        "c-luma",
        "lottie/samples/01-luma-matte/skottie-luma-matte.json",
    ),
    (
        "c-masks",
        "lottie/samples/03-mask-modes/skottie-masking-translucent.json",
    ),
    ("c-stars", "lottie/samples/12-shape-types/ShapeTypes.json"),
    (
        "c-merge",
        "lottie/samples/05-merge-paths/skottie-mergepaths-effect.json",
    ),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("scenes") => write_scenes(&args[1..]),
        Some("compare") => compare::run(&args[1..]),
        Some("profile") => profile::run(&args[1..]),
        _ => Err(
            "usage: vector-animation-check scenes <app> [--corpus <dir>]\n       \
                  vector-animation-check compare <shots> <app> [--tile <points>] [--out <dir>]\n       \
                  vector-animation-check profile <dir> [--size <points>] [--ratio <pixels a point>]"
                .into(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The value after `flag` in `args`.
pub fn option<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn write_scenes(args: &[String]) -> Result<(), String> {
    let app = PathBuf::from(args.first().ok_or("which app?")?);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut list = scenes::built_in(&fixtures)?;
    if let Some(corpus) = option(args, "--corpus") {
        for (name, file) in CORPUS {
            let path = Path::new(corpus).join(file);
            let Ok(data) = std::fs::read(&path) else {
                eprintln!("{} is not there: left out", path.display());
                continue;
            };
            let read = if file.ends_with(".svg") {
                dxn_vector_animation_svg::read(&data).map(|i| i.scene)
            } else {
                dxn_vector_animation_lottie::read(&data).map(|i| i.scene)
            };
            match read {
                Ok(scene) => list.push(((*name).to_owned(), scene)),
                Err(e) => eprintln!("{}: {e}", path.display()),
            }
        }
    }
    let dir = app.join("animations");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut index = Vec::new();
    let mut imports = String::from("// Written by `vector-animation-check scenes`.\n");
    let mut entries = String::new();
    for (i, (name, scene)) in list.iter().enumerate() {
        let json = dxn_vector_animation::compiled::to_json(scene, false)
            .map_err(|e| format!("{name}: {e}"))?;
        std::fs::write(dir.join(format!("{name}.json")), json).map_err(|e| e.to_string())?;
        index.push(serde_json::json!({
            "name": name,
            "duration": scene.timeline.duration,
            "width": scene.width,
            "height": scene.height,
        }));
        imports.push_str(&format!("import s{i} from \"./{name}.json\";\n"));
        entries.push_str(&format!("  [\"{name}\", s{i}],\n"));
    }
    std::fs::write(
        dir.join("index.json"),
        serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("index.ts"),
        format!("{imports}\nexport const scenes: [string, object][] = [\n{entries}];\n"),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "{} scenes in {} ({} pages)",
        list.len(),
        dir.display(),
        list.len().div_ceil(PAGE)
    );
    Ok(())
}
