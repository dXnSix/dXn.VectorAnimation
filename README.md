# dXn.VectorAnimation.Nsh

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Animated SVG and Lottie in NSH apps, natively on iOS, Android, macOS, Windows, Linux, GPUI and the web.**
`dxn_vector_animation_nsh` is a UI set for NSH (Native Shell) with one component, `VectorAnimation`. It reads
a file with [dXn.VectorAnimation](https://github.com/dXnSix/dXn.VectorAnimation) and plays it on NSH's own
primitives: canvases whose numbers and colors NSH's native animation graph moves on the UI thread, with no
JavaScript and no engine work per frame.

> **Status: phase 1.** The component, playback, the TSX and Rust APIs and an example app, checked headless
> (`nsh test`) and against 53 real animation files; not yet on a device.

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
  or blended, is a layer (a view of its own, its opacity the view's). Masks become clips of their shapes'
  outlines (exact for opaque mask shapes; soft masks clip hard).
- **Transforms, motion paths and matrices.** Transform lists are canvas transforms of animated values; motion
  paths are lookups of the path sampled by length; animated matrices animate their decomposition; skews are
  sampled.
- **Trims and dash patterns that move.** Where only one end of the drawn part moves (drawing on or off), the
  part is a dash pattern as long as the outline, moved along it. Where both move (spinners, trims carried round
  by their offset, Figma's path trims, dash lengths that change), the part is a path of its own whose points
  the graph moves: each segment cut where the part starts and ends, from lookups of the outline sampled by
  length.
- **What the graph cannot express** (gradients that move, dash patterns of several dashes that move, animated
  stars, paths morphing between different commands) is sampled frame by frame on the engine thread
  instead, for the whole scene; the reason is logged.
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
shapes) rely on Canvas 2D's rule that a path's points are placed as they are added.

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
draw what the reference does, except masks of partial opacity or luminance, which clip hard.

`nsh test` builds the example's runner with the NSH crates the crate resolves (crates.io's at the CLI's
version); an `nsh` built from a checkout needs that checkout's crates: a local, uncommitted
`examples/gallery/.cargo/config.toml` patching `nsh-*` to it, with `nsh test .` run from `examples/gallery`
(Cargo reads that configuration from the directory it runs in).

## Roadmap

1. **Done:** the component with canvases, warped time and playback; the TSX and Rust APIs; the example app.
2. On device on every platform; soft masks and moving gradients on the graph; sampling one canvas instead of
   the whole scene; views with bound transforms for groups that only move or fade.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your
option.
