# dXn.VectorAnimation

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Animated vector graphics for Rust, independent of any renderer or UI framework.** `dxn_vector_animation`
reads animated SVG and Lottie into one scene model: groups, shapes, paints and masks whose properties are
constants or keyframe tracks on a timeline. A renderer either samples the scene at a moment
(`Scene::frame_at(t)` gives a flat display list) or maps the tracks onto an animation system of its own.

> **Status: design stage.** This README is the agreed design; the code follows.

## Why

Figma Motion exports an animation as animated SVG (CSS `@keyframes` in a `<style>` block) or as Lottie and
dotLottie. resvg draws SVG as a still image, and each Lottie runtime plays its own format. This library reads
both formats into one model, so a player, a thumbnailer, a converter or a linter is written once and serves
every format.

Its first consumer is [dXn.VectorAnimation.Nsh](https://github.com/dXnSix/dXn.VectorAnimation.Nsh), a
player component for NSH apps.

## Crates

| Crate | What it is |
|---|---|
| `dxn_vector_animation` | The scene model, keyframe tracks and easings, the timeline, path geometry, interpolation, sampling (`Scene::frame_at`), and the limits that bound untrusted files |
| `dxn_vector_animation_svg` | Reads SVG: the static document, CSS animations (the subset Figma Motion writes) and SMIL |
| `dxn_vector_animation_lottie` | Reads Lottie JSON and dotLottie archives |
| `dxn_vector_animation_render` | A reference CPU renderer on tiny-skia: golden tests, poster frames, the CLI's images |
| `dxn_vector_animation_cli` | The `vector-animation` command: `inspect`, `validate`, `compile` (SVG or Lottie into a scene file), `render` (PNG frames) |

The importers and the renderer are separate crates, so a consumer takes only what it reads and draws (a
WebAssembly build that plays precompiled scenes takes the core alone).

## The scene model

- **Nodes.** Groups (transform, opacity, clip path, mask, blend mode) and shapes (a path, a fill, a stroke
  with width, dash pattern, caps, joins and trim), and images. Every property is a constant or a track.
- **Tracks.** Keyframes at times in seconds, each segment with its own easing: linear, cubic Bézier, hold (a
  step), or sampled stops (how the exporters write springs).
- **Time.** A duration, a frame-rate hint, a loop mode (once, loop, ping-pong), named markers, and time
  remapping of nested compositions.
- **Interpolation as the formats define it.** Transform lists animate function by function when their
  functions match, as CSS does, and through matrix decomposition when they differ. Colors and gradient stops
  follow each format's rule.
- **Geometry.** Paths with arc length, trim, morphing between paths of the same structure, and motion paths
  (CSS `offset-path`, Lottie's spatial tangents) as arc-length tables.
- **Two ways to play.** Sample the scene at any time, or read its tracks and drive them with a native
  animation system (what the NSH player does: its UI thread plays the tracks without per-frame work in Rust).

## Inputs

**Animated SVG** (`dxn_vector_animation_svg`)

- The static document: shapes, paths, groups and their transforms, fills and strokes, linear and radial
  gradients, clip paths, alpha masks, opacity, images, and the `<style>` sheets that style them.
- CSS animations, as Figma Motion writes them: `@keyframes` with percentage stops, a timing function per stop,
  the `animation` shorthand, `transform`, `transform-origin`, `transform-box`, `translate`, `opacity`, and
  motion along a path (`offset-path`, `offset-distance`, `offset-rotate`, `offset-anchor`).
- SMIL: `animate`, `animateTransform`, `animateMotion` and `set`, with `values`, `keyTimes`, `keySplines`,
  `calcMode`, `dur`, `repeatCount` and `begin` offsets.

**Lottie** (`dxn_vector_animation_lottie`)

- Lottie JSON and dotLottie archives: shape layers, groups, rectangles, ellipses, polystars and paths,
  fills, strokes and gradients, trim paths, alpha masks and mattes, nested compositions with time remapping,
  parenting, and repeaters (expanded into copies).

## Untrusted input

A remote animation is a document from the network. Reading one is bounded: its size, its nodes and
keyframes, nesting depth, and the layers a renderer would allocate for it. A file over a limit is refused
with an error naming the limit.

## Uses

- Players: the NSH player maps the tracks onto NSH's native animation graph.
- Renderers for other toolkits (vello, wgpu, egui, GPUI) through the sampled display list.
- Thumbnails and poster frames, on a server or at build time.
- Checking exports: the CLI renders frames to compare with Figma's video export, and lists what a file uses.
- Converting between formats.

## Roadmap

1. The scene model, Figma Motion's SVG subset, Lottie as Figma Motion exports it, the reference renderer and
   the CLI.
2. SMIL, more of Lottie (repeaters, mattes, dotLottie archives), luminance masks.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your
option.
