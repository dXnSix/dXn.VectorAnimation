---
title: Get started
description: Add the dxn_vector_animation crates from Git, read an animated SVG and a Lottie file, sample a frame, render it as a PNG and try the CLI.
group: Start
order: 10
---

`dxn_vector_animation` reads animated SVG and Lottie into one scene model, independent of any renderer or UI framework. You reach for it when a player, a build step or a tool has to read, check, convert or draw animations exported by Figma Motion, Bodymovin or LottieFiles.

## Install

The crates are not on crates.io and have no release tags yet. Depend on the repository's `main` branch, taking only the crates you use:

```toml
[dependencies]
dxn_vector_animation = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main" }
dxn_vector_animation_svg = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main" }
dxn_vector_animation_lottie = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main" }
dxn_vector_animation_render = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main" }
```

`Cargo.lock` records the commit the branch pointed to; `cargo update` moves to the branch's latest commit. Until tags exist, every commit to `main` can change the API.

| Crate | What it is |
|---|---|
| `dxn_vector_animation` | The scene model, tracks and easings, the timeline, sampling (`Scene::frame_at`), `Limits`, and the compiled scene format (feature `serde`) |
| `dxn_vector_animation_svg` | Reads SVG: the static document, CSS animations as Figma Motion writes them, SMIL |
| `dxn_vector_animation_lottie` | Reads Lottie JSON and dotLottie archives |
| `dxn_vector_animation_render` | The reference renderer on tiny-skia: frames to pixmaps and PNGs |
| `dxn_vector_animation_cli` | The `vector-animation` command: `inspect`, `validate`, `compile`, `render` |

| Requirement | Value |
|---|---|
| Rust edition | 2024 |
| Minimum supported Rust | 1.97 (`rust-version` in `Cargo.toml`) |
| Development toolchain | 1.99.0, pinned by `rust-toolchain.toml` and used by CI |
| `unsafe` | forbidden in every crate |
| License | MIT OR Apache-2.0 |

The only feature flag is `serde` on `dxn_vector_animation` (off by default): it serializes scenes and adds the [compiled scene format](./compiled-scenes.md).

```toml
dxn_vector_animation = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main", features = ["serde"] }
```

## Read an SVG and draw a frame

The program below reads `fixtures/figma-motion.svg` from the repository (written as Figma Motion exports: a button scaling in, a dot moving along a path, a masked gradient), samples it half a second in and writes a PNG. It reads and writes files.

```rust
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // Read an animated SVG, as Figma Motion exports it.
    let data = std::fs::read("fixtures/figma-motion.svg")?;
    let import = dxn_vector_animation_svg::read(&data)?;
    for warning in &import.warnings {
        eprintln!("left out: {warning}");
    }
    let scene = import.scene;
    println!(
        "{} × {}, {} s, {:?}",
        scene.width, scene.height, scene.timeline.duration, scene.timeline.loop_mode
    );

    // The scene half a second in, drawn as a PNG.
    let frame = scene.frame_at(0.5);
    let png = dxn_vector_animation_render::render_png(&frame, &Default::default())?;
    std::fs::write("button.png", png)?;
    Ok(())
}
```

```text
200 × 100, 2 s, Loop
```

`read` returns an `Import`: the `scene`, and `warnings` naming what the file holds that the scene leaves out. A file the importer cannot take at all (not UTF-8, broken XML, over a limit) is an `Err`. `frame_at` gives the scene at one moment as a flat display list, and `render_png` draws it at one pixel per user unit with a transparent background.

## Read a Lottie file

The Lottie importer has the same shape. It takes the bytes of a Lottie JSON document or of a dotLottie archive:

```rust
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let data = std::fs::read("fixtures/lottie-motion.json")?;
    let scene = dxn_vector_animation_lottie::read(&data)?.scene;
    let timeline = &scene.timeline;
    println!("{:?} fps, {} s", timeline.frame_rate, timeline.duration);
    for marker in &timeline.markers {
        println!("marker {} at {} s for {} s", marker.name, marker.start, marker.duration);
    }
    Ok(())
}
```

```text
Some(30.0) fps, 2 s
marker half at 1 s for 0.5 s
```

Times are seconds everywhere in the scene, whatever unit the file used: Lottie's frames are divided by its frame rate on import, and the rate is kept as a hint in `frame_rate`.

## Render at a size

`RenderOptions` sets the pixel size and a background. Give one side and the other keeps the aspect:

```rust
use dxn_vector_animation::Color;
use dxn_vector_animation_render::{RenderOptions, render_png};

let options = RenderOptions {
    width: Some(400), // the height keeps the aspect: 200
    background: Some(Color::WHITE),
    ..RenderOptions::default()
};
std::fs::write("button@2x.png", render_png(&scene.frame_at(1.0), &options)?)?;
```

The options, region rendering and fonts are in [Frames and rendering](./frames-and-rendering.md).

## From the command line

The CLI does the same without code. Install it from the repository:

```bash
cargo install --git https://github.com/dXnSix/dXn.VectorAnimation --branch main dxn_vector_animation_cli
vector-animation inspect figma-motion.svg
```

```text
figma-motion.svg
  format       animated SVG
  size         200 × 100
  timeline     2.000 s, loops
  nodes        6 groups, 4 shapes, 0 images (depth 4)
  animation    4 animated properties, 11 keyframes
  path points  4
  left out     nothing
```

`vector-animation render figma-motion.svg -t 0.5` writes the same frame as the program above. Every subcommand is in [The vector-animation CLI](./cli.md).

Next: [How it works](./how-it-works.md), then [The scene model](./scene-model.md).
