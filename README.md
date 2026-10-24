# dXn.VectorAnimation.Nsh

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Animated SVG and Lottie in NSH apps, natively on iOS, Android, macOS, Windows, Linux, GPUI and the web.**
`dxn_vector_animation_nsh` is a UI set for NSH (Native Shell) with one component, `VectorAnimation`. It reads
a file with [dXn.VectorAnimation](https://github.com/dXnSix/dXn.VectorAnimation) and plays it on NSH's own
primitives: canvases whose numbers and colors NSH's native animation graph moves on the UI thread, with no
JavaScript and no engine work per frame.

> **Status: design stage.** This README is the agreed design; the code follows.

## Usage

```tsx
import { VectorAnimation } from "dxn-vector-animation";
import hero from "./hero.json"; // Lottie

export function Hero() {
  return <VectorAnimation src={hero} repeat resizeMode="contain" />;
}
```

```rust
use dxn_vector_animation_nsh::{Source, vector_animation};

const HERO: &[u8] = include_bytes!("hero.json");

vector_animation(Source::bytes(HERO))
    .repeat(true)
    .progress(&scroll_progress) // an animated value: scrubbed natively
```

An app lists the crate in `nsh.json` `"rust"` (its own UI set, `"dxn-vector-animation"`), or depends on it
from its own crate and registers `VectorAnimation` in its set.

## The component

| Prop / event | Meaning |
|---|---|
| `src` | A URL, a Lottie JSON object, a compiled scene, the text of an SVG or Lottie file, or bytes (`Uint8Array`: dotLottie, `.svgz`) |
| `paused` | Stops playback where it is; `false` plays on from there |
| `repeat` | `true`, `false` or a count; the file's loop mode by default |
| `direction` | `"normal"`, `"reverse"` or `"alternate"` |
| `rate` | Playback speed, 1 by default |
| `segment` | What plays: a marker's name or `[start, end]` in seconds |
| `resizeMode` | How the animation fits the view, as `Image`'s |
| `progress` | 0 to 1: the frame shown, set by the app (scrubbing); the Rust API takes an animated value |
| `posterFrame` | What shows under reduced motion: `"first"`, `"last"`, a time or a marker |
| `ignoreReduceMotion` | Plays under reduced motion too |
| `onLoad` | `{ duration, size, markers }` |
| `onEnd` | Playback reached its end (not while repeating forever) |
| `onError` | The file failed to load or read |
| ref `seek(seconds)` | Moves to a time |

## How it plays

- **One time value drives the scene.** Playing is a linear timing of an animated value to the end, looped as
  `repeat` says; pausing stops it, seeking sets it, `rate` scales the duration, `alternate` is a sequence there
  and back. Each track is an interpolation of that value, so the whole scene moves on the UI thread.
- **Easing per segment.** When a track's segments have different easings, each segment is interpolated from 0
  to 1 with its own easing and the results are added into one warped time (a tree of additions); the property
  is then a linear interpolation over the warped time. Tracks with the same keyframe times share it.
- **Canvases and isolation.** Content between isolated groups shares one canvas. A group that must be
  composited on its own (group opacity over overlapping content, a mask) is a canvas view of its own: its
  opacity is the view's, and its mask is drawn into it with `destination-in`.
- **Trim paths.** A stroke's dash offset; when both ends move, the stroke is drawn to its end and erased to its
  start with `destination-out`.
- **What the graph cannot move** (animated gradients, shadows, dash patterns, effects) redraws its one canvas
  from the engine thread each frame. Static filters are drawn once with resvg and shown as an image.
- **The web** plays it the same way: NSH's web runtime replays canvases with Canvas 2D and runs the animation
  graph on the browser's frames.

## Loading

- A URL is fetched through NSH's network stack, under the app's network capabilities.
- Lottie JSON is an ordinary JSON import.
- SVG and dotLottie files are compiled ahead with `vector-animation compile` (dXn.VectorAnimation's CLI) into
  a `.vector-animation.json` scene, imported as JSON.
- Rust apps embed files with `include_bytes!`.
- Native platforms read files on a worker thread; the web reads them on the page's thread, so large files are
  better compiled ahead.

## Off screen and reduced motion

- `paused` is the app's: it knows which screen is in front.
- A player laid out at zero size pauses; a list row that scrolls away is disposed with its animation.
- Under reduced motion the player shows its poster frame and runs no clock, unless `ignoreReduceMotion`.

## NSH

The crate depends on `nsh-core` by version requirement, so an app's own NSH (crates.io, a checkout, a fork)
is the one it uses. It uses NSH's public API alone: Rust components, the canvas `Painter`, animated values and
composites.

On GPUI a view's opacity is multiplied into each color it draws, so overlapping isolated groups nested in a
fading group blend each part on its own there.

## Roadmap

1. The component with canvases, warped time and playback; the TSX and Rust APIs; an example app.
2. Isolation, masks, trim paths, engine-thread redraws, resvg-drawn static filters.
3. Views with bound transforms for groups that only move or fade, so they animate without redrawing.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your
option.
