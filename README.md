# dXn.VectorAnimation.Nsh

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Animated SVG and Lottie in NSH apps, natively on iOS, Android, macOS, Windows, Linux, GPUI and the web.**
`dxn_vector_animation_nsh` is a UI set for NSH (Native Shell) with one component, `VectorAnimation`. It reads
a file with [dXn.VectorAnimation](https://github.com/dXnSix/dXn.VectorAnimation) and plays it on NSH's own
primitives: canvases whose numbers and colors NSH's native animation graph moves on the UI thread, with no
JavaScript and no engine work per frame.

> **Status: phase 1.** The component, playback, the TSX and Rust APIs and an example app, checked headless
> (`nsh test`), against 53 real animation files, and frame by frame in the iOS simulator, Chrome, the
> Android emulator, and GTK and GPUI on Linux; not yet on Windows, nor GPUI on macOS.

## Usage

An app lists the crate in `nsh.json` (its own UI set, imported as `"dxn-vector-animation"`):

```json
{ "rust": ["native", "path/to/dXn.VectorAnimation.Nsh/crates/dxn_vector_animation_nsh"] }
```

or depends on it from its own crate and registers `VectorAnimation` in its set
(`registry.register::<dxn_vector_animation_nsh::VectorAnimation>()`).

```tsx
import { VectorAnimation } from "dxn-vector-animation";
import hero from "./hero.json"; // a Lottie file
import button from "./button.vector-animation.json"; // an SVG compiled ahead

<VectorAnimation src={hero} repeat={2} onEnd={() => …} />
<VectorAnimation src={button} style={{ width: 320, height: 160 }} paused={!active()} />
<VectorAnimation src="https://example.com/spinner.svg" />
```

```rust
use dxn_vector_animation_nsh::{Source, vector_animation};

const HERO: &[u8] = include_bytes!("hero.json");

vector_animation().src(Some(Source::bytes(HERO))).on_end(|| …)
```

SVG files are compiled ahead with dXn.VectorAnimation's CLI (`vector-animation compile button.svg`) and
imported as JSON; Lottie JSON imports as it is; URLs are fetched through NSH's network stack, under the app's
network capabilities. `examples/gallery` is an app using each.

## The component

| Prop / event | Meaning |
|---|---|
| `src` | A URL, the text of an SVG, Lottie or compiled scene, its bytes (`Uint8Array`), or a Lottie or compiled scene's JSON |
| `paused` | Stops playback where it is; `false` plays on from there |
| `repeat` | `true` forever, a number of times in all, `false` once; the file's loop mode when left out |
| `direction` | `"normal"`, `"reverse"` or `"alternate"` |
| `rate` | Playback speed, 1 by default |
| `segment` | What plays: a marker's name or `[start, end]` in seconds |
| `resizeMode` | `"contain"` (default), `"cover"`, `"stretch"` or `"center"` |
| `progress` | 0 to 1: the frame shown, set by the app (scrubbing); stops playback while set |
| `posterFrame` | What shows when the user reduces motion: `"first"` (default), `"last"`, seconds or a marker |
| `ignoreReduceMotion` | Plays when motion is reduced too |
| `accessibilityLabel` | What screen readers say |
| `onLoad` | `{ duration, size: { width, height }, markers, warnings }` |
| `onEnd` | Playback reached its end (not while repeating forever) |
| `onError` | The file could not be loaded or read: the message |
| ref `seek(seconds)` | Moves to a time; playback goes on from there unless paused |

The view is the animation's own size unless the app sizes it.

## How it plays

- **One time value drives the scene.** Playing is a linear timing of an animated value from the segment's
  start to its end, looped as `repeat` says; pausing stops it, seeking sets it, `rate` scales the duration,
  `alternate` runs there and back. Each property is an interpolation of that value, so the whole scene moves
  on the UI thread.
- **Easing per segment.** A track whose segments share an easing is one interpolation. Segments of different
  easings share a warped time (each segment eased from 0 to 1, added up); overshooting easings (Back) add
  each segment's change on its own.
- **Canvases and layers.** Content between layers shares one canvas. A group faded over overlapping content,
  or blended, is a layer (a view of its own, its opacity the view's). Masks of opaque shapes are clips of
  their outlines. Soft masks (partly transparent shapes, luminance, chained mattes) draw the content on a
  canvas of its own and then take away (`destination-out`) as much of it as the mask does not show, the
  mask's value the graph's: its shapes' colors and opacities composited as the reference composites them
  (added, intersected, subtracted). A mask of one shape intersected with or subtracted from by others takes
  away over the whole plane and then on one side of each outline, every edge drawn once; masks of shapes
  added over one another take away region by region of the plane their shapes cut it into (up to four
  shapes), where a faint seam can show along edges inside the mask. `destination-out` leaves what lies
  outside its drawing alone, so the platforms agree on it. A clip within the mask (an SVG mask's region) is one
  more of its shapes. Gradient-filled mask shapes, and content drawn on more than one canvas (layers within
  it), clip hard.
- **Transforms, motion paths and matrices.** Transform lists are canvas transforms of animated values; motion
  paths are lookups of the path sampled by length; animated matrices animate their decomposition; skews are
  sampled.
- **Outlines that move.** A path morphing between keyframes of the same commands is a track per coordinate.
  One morphing between different commands switches halfway, as the reference does: a copy of the shape for
  each run of the same commands, shown only in its time. Stars and rectangles with elliptical corners have
  their points sampled over time (120 times a second, at least 24 a segment, keeping only the samples a
  straight line between their neighbours does not stand for), each coordinate a lookup of the time; a star
  whose number of points changes is a contour for each number, moved far out of sight outside its time.
- **Trims and dash patterns that move.** Where only one end of the drawn part moves (drawing on or off), the
  part is a dash pattern as long as the outline, moved along it. Where both move (spinners, trims carried round
  by their offset, Figma's path trims, dash lengths that change), the part is a path of its own whose points
  the graph moves: each segment cut where the part starts and ends, from lookups of the outline sampled by
  length. A dash pattern of several dashes, or repeating along the outline, is such a part for each dash of
  each repetition that reaches the outline (up to 64).
- **Gradients that move.** NSH's canvas gradients are still, so a gradient whose ends move is a still one
  placed by a transform the graph moves (a radial one too, while its focal point keeps its place within the
  circle). One whose colors change is its keyframes' gradients faded into one another; a linear one whose
  stops move (Figma's shines) is a gradient of its own between each two stops, in bands the graph moves.
  Stops that move while their colors change, and stops of a radial gradient that move, are gradients sampled
  over time (no stop moving more than 2% of the gradient between two samples) faded into one another.
  Paints of several gradients draw on a canvas of their own: faded keyframes add up within a clip of the
  shape, and bands of opaque colors color the shape drawn once, each from its first stop over the one before
  (bands whose edges met showed seams on Android). A stroke so painted clips to its outline.
- **Drop shadows and blurs.** A still drop shadow is a canvas under the content drawing it with the canvas's
  shadow and then taking it away again (as opaque as it covers: the shadow hidden under it), the shadow's
  opacity, which may move, the layer's. A shadow whose blur moves is several such canvases at blurs along its
  way (each a share past the one before, up to ten), faded into one another as it moves. Blurs, shadows that
  move otherwise, and shadows of content on more than one canvas are drawn as images the reference renderer
  makes at the display's pixels: once where they do not move, frame by frame where they do.
- **What the graph cannot express** (gradient stops that change in number, trims of outlines that move,
  moving trims of fills, more than 64 moving dashes, outlines changing their commands more than 32 times)
  is sampled frame by frame on the engine thread instead, that part alone: a canvas of its own drawing the
  part within copies of the groups it is in (their clips, masks and transforms; the layers it is drawn in
  fade and blend it), the rest of the scene still on the graph. Sampled siblings with nothing drawn between
  share one canvas. A group whose own transform, clip or mask the graph cannot express is sampled whole. The
  reasons are logged. None of the 53 corpus files has a part sampled.
- **The web** plays it the same way: NSH's web runtime replays canvases with Canvas 2D and runs the animation
  graph on the browser's frames.

## Off screen and reduced motion

- `paused` is the app's: it knows which screen is in front. A list row that scrolls away is disposed with
  its animation.
- Under reduced motion the player shows its poster frame and runs no clock, unless `ignoreReduceMotion`.

## NSH

The crate depends on `nsh-core` by version requirement, so an app's own NSH (crates.io, a checkout, a fork)
is the one it uses. It uses NSH's public API alone: Rust components, the canvas `Painter`, animated values and
composites. This workspace's own builds and tests take the dXnSix fork of NSH (`[patch.crates-io]`).

Paths built under a transform (shapes whose outlines move apart from their stroke, clips and masks of moving
shapes) rely on Canvas 2D's rule that a path's points are placed as they are added. The player composites
only with source-over, `lighter` and `source-atop`: the platforms' canvases differ on operations that clear
what lies outside the drawing (`destination-in`). Rectangles that stand for everything reach 50,000 points
past the view, not further: cairo (the Linux canvas) cannot place points past about ±8 million device
pixels. Each canvas restores all it saves, as NSH's web runtime keeps one context from drawing to drawing.

## Development

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
nsh test examples/gallery
```

`tests/plans.rs` replays compiled scenes with Canvas 2D semantics and compares their pixels with the
reference renderer's at many times; `tests/component.rs` runs the component on a headless engine.
`tests/corpus.rs` does the same for every file of a folder (`DXN_CORPUS=<dir> cargo test --test corpus --
--ignored --nocapture`); on 53 real files (Figma Motion SVG and Lottie exports among them) the native plans
draw what the reference does to under 1.5% of the pixels at 200 pixels wide, except a timeline of hatching
narrower than two pixels, whose edges anti-alias apart (13% at that size, 3% at 1,000 pixels). The replayer
draws images as the reference decodes them.

`nsh test` builds the example's runner with the NSH crates the crate resolves (crates.io's at the CLI's
version); an `nsh` built from a checkout needs that checkout's crates: a local, uncommitted
`examples/gallery/.cargo/config.toml` patching `nsh-*` to it, with `nsh test .` run from `examples/gallery`
(Cargo reads that configuration from the directory it runs in).

## Device checks

`examples/check` plays a check set, twelve tiles a page, and its test holds each page at 25, 50 and 75% and
photographs it on the platform it runs on; `vector-animation-check` (`crates/dxn_vector_animation_check`)
writes the set and sets the photographs beside the reference renderer's frames:

```sh
cargo run -p dxn_vector_animation_check -- scenes examples/check --corpus <dir>
cd examples/check && nsh test check.test.tsx --target ios   # or web, android, linux, gpui
cargo run -p dxn_vector_animation_check -- compare examples/check/shots examples/check --out <dir>
```

The set is the check app's own scenes (fixtures, spinners, trims, moving gradients) and, with `--corpus`, the
corpus files the tool lists. The comparison finds the tiles by their border, renders each reference at the
tile's size, and reports the share of pixels that differ, straight and at the best shift of a pixel (a border
a fraction of a pixel wide moves the content); it writes each pair side by side. The iOS simulator's
screenshots are Display P3 and converted to sRGB first.

- The web build needs a clang that targets wasm (Homebrew's LLVM first on the `PATH` on macOS).
- `nsh test` with an `nsh` built from a checkout needs a local `.cargo/config.toml` in `examples/check`
  patching `nsh-*` to that checkout, as for the gallery.
- GPUI on macOS photographs nothing while the screen is locked.

On the iOS simulator, Chrome, the Android emulator, and GTK and GPUI on Linux, the tiles differ from the
reference in under 0.5% of their pixels at the device's scale (up to 3% in Chrome at one pixel a point, where
a pixel of anti-aliasing is a larger share), except masks of partial opacity or luminance, which clip hard.

## What playing costs

`vector-animation-check profile <dir> [--size <points>] [--ratio <pixels a point>]` reports, for each file of a
folder: how long its plan takes to compile, its graph's nodes and breakpoints, what each frame draws
(canvases, layers, operations), and the engine thread's work each frame for parts sampled frame by frame and
for rasters of parts that move. On the 53 corpus files at 200 points and 3 pixels a point, plans compile in
7 ms at most (most under 2), and the engine thread does no work a frame but for two files whose blurs or
shadows move (9 and 1 ms a frame: each raster renders only where its part draws). The graph's evaluation on
the platforms' UI threads, and frame times on real phones, are not measured here.

## Roadmap

1. **Done:** the component with canvases, warped time and playback; the TSX and Rust APIs; the example app;
   moving trims, dash patterns and gradients on the graph; the iOS simulator and the web.
2. Windows, and GPUI on macOS; views with bound transforms for groups that only move or fade.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your
option.
