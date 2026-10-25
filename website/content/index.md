---
title: dXn.VectorAnimation
description: Animated SVG and Lottie for Rust - one scene model for both formats, a reference renderer and CLI, and a player that plays them natively in NSH apps.
eyebrow: Animated vector graphics for Rust
headline: Animated SVG and Lottie in one Rust scene model.
lede: >-
  dxn_vector_animation reads Figma Motion's animated SVG (CSS and SMIL), Lottie and dotLottie into one
  scene of groups, shapes and paints whose properties are keyframe tracks. Sample it into a display list,
  render it with the reference renderer, or play it: the NSH player maps the tracks onto NSH's native
  animation graph, so an animation runs on the UI thread on iOS, Android, macOS, Windows, Linux and the web.
install: |
  [dependencies]
  dxn_vector_animation = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main" }
  dxn_vector_animation_svg = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main" }
facts:
  - { label: Inputs, value: "SVG (CSS, SMIL), Lottie, dotLottie" }
  - { label: Rust, value: "1.97+, edition 2024" }
  - { label: Core targets, value: "native and wasm32" }
  - { label: Player, value: "NSH, 7 platforms checked" }
  - { label: Unsafe, value: "forbidden" }
  - { label: License, value: "MIT OR Apache-2.0" }
heroTitle: main.rs
heroLang: rust
heroCode: |
  use dxn_vector_animation_render::{render_png, RenderOptions};

  let data = std::fs::read("button.svg")?; // a Figma Motion export
  let import = dxn_vector_animation_svg::read(&data)?;
  for warning in &import.warnings {
      eprintln!("left out: {warning}"); // nothing is dropped silently
  }

  let scene = import.scene;
  println!("{} s at {}x{}", scene.timeline.duration, scene.width, scene.height);

  let frame = scene.frame_at(0.5); // the display list half a second in
  let png = render_png(&frame, &RenderOptions::default())?;
  std::fs::write("button-0.5s.png", png)?;
features:
  - title: Figma Motion's SVG
    text: CSS @keyframes the way Figma Motion writes them, SMIL with begin and end times, events and accumulation, filters as effects, live text and patterns.
    link: ./docs/svg.md
  - title: Lottie and dotLottie
    text: Layers, shapes, shape modifiers, text with its animators, effects and layer styles, and expressions evaluated and baked into keyframes within a budget.
    link: ./docs/lottie.md
  - title: One scene model
    text: Groups, shapes, images and text whose every property is a constant or a keyframe track on one timeline, so a player or a converter is written once.
    link: ./docs/scene-model.md
  - title: Reference renderer and CLI
    text: A tiny-skia renderer for golden tests and poster frames, and a vector-animation command that inspects, validates, compiles and renders files.
    link: ./docs/frames-and-rendering.md
  - title: Native playback in NSH
    text: The VectorAnimation component compiles a scene into canvases whose numbers NSH's animation graph moves on the UI thread, with no JavaScript per frame.
    link: ./docs/nsh-player.md
  - title: Bounded untrusted input
    text: Every size, count, depth and expression step a file may take is bounded by Limits, and what a file holds that the scene leaves out is a warning.
    link: ./docs/limits.md
next:
  - { title: Get started, text: "Read an SVG, sample a frame, render a PNG.", link: ./docs/getting-started.md }
  - { title: The NSH player, text: "Play an animation in an NSH app.", link: ./docs/nsh-player.md }
  - { title: What it plays natively, text: "Trims, gradients, masks, shadows and effects on the graph.", link: ./docs/capabilities.md }
  - { title: Examples, text: "Every check scene, live in the browser.", link: ./docs/examples.md }
---

## One model for two formats

An SVG and a Lottie file read into the same `Scene`: the importers differ, everything after them does not.

```rust
let svg = dxn_vector_animation_svg::read(&std::fs::read("loader.svg")?)?.scene;
let lottie = dxn_vector_animation_lottie::read(&std::fs::read("loader.json")?)?.scene;

for scene in [&svg, &lottie] {
    let frame = scene.frame_at(scene.timeline.duration / 2.0);
    println!("{} items half way", frame.items.len());
}
```

The scene's nodes, paints and tracks are described in [The scene model](./docs/scene-model.md).

## Played on the UI thread

In an NSH app the `VectorAnimation` component plays a file natively. This is its web build, playing a Figma
Motion export:

```example
demo: figma
height: 240
caption: A Figma Motion SVG export, played by the NSH player in the browser.
```

```tsx
import { VectorAnimation } from "dxn-vector-animation";
import button from "./button.vector-animation.json"; // compiled ahead with the CLI

<VectorAnimation src={button} style={{ width: 320, height: 160 }} paused={!active()} />
```

How a scene becomes canvases and graph nodes is in [What it plays natively](./docs/capabilities.md).

## Compile once, play anywhere

The CLI checks a file and compiles it into the scene format players load, with the importers out of the
loop:

```bash
vector-animation validate --strict hero.svg
vector-animation compile hero.svg            # hero.vector-animation.json
vector-animation render hero.svg --fps 30 --width 600
```

The commands and their flags are in [The vector-animation CLI](./docs/cli.md).

## Effects and masks, live

Drop and inner shadows, blurs, soft masks and color effects play on the graph where it can express them, and
frame by frame where it cannot:

```example
demo: inner-shadows
height: 240
caption: Inner shadows whose offsets and colors move on the graph.
```

Every check scene is on the [Examples](./docs/examples.md) page.
