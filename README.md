# dXn.VectorAnimation.Nsh

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Animated SVG and Lottie in NSH apps, natively on iOS, Android, macOS, Windows, Linux, GPUI and the web.**
`dxn_vector_animation_nsh` is a UI set for NSH (Native Shell) with one component, `VectorAnimation`. It reads
a file with [dXn.VectorAnimation](https://github.com/dXnSix/dXn.VectorAnimation) and plays it on NSH's own
primitives: canvases whose numbers and colors NSH's native animation graph moves on the UI thread, with no
JavaScript and no engine work per frame.

> **Status: phase 1.** The component, playback, the TSX and Rust APIs and an example app, checked headless
> (`nsh test`), against 53 real animation files, and frame by frame in the iOS simulator, Chrome, the
> Android emulator, GTK and GPUI on Linux, and GPUI on macOS. Windows is compiled for, not yet run; no real
> phone has run it yet.

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
| `onLoad` | `{ duration, size: { width, height }, markers, events, warnings }` |
| `onEnd` | Playback reached its end (not while repeating forever) |
| `onError` | The file could not be loaded or read: the message |
| ref `seek(seconds)` | Moves to a time; playback goes on from there unless paused |
| ref `trigger(event)` | Plays what the file has for an event (`btn.click`, from `events`: an SVG's `begin="click"`) once from its start and holds its end; the timeline's animations play from their start meanwhile |

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
  canvas of its own and then take away (`destination-out`, which leaves what lies outside its drawing alone,
  so the platforms agree on it) as much of it as the mask does not show:
  - A mask of one shape intersected with or subtracted from by others is a product: it takes away a share
    over the whole plane and then on one side of each outline, every edge drawn once. All of what lies
    outside an outline whose contours may overlap winding alike (lettering) is a clip with its own rule.
  - Any other mask whose content moves as a whole at most (in the masked group's lengths) is an image of
    what it hides, as the reference renders its content (gradients, strokes, images, any number of shapes),
    at the display's pixels, drawn under the group's and the mask's own transforms, the mask's opacity
    taking away first: no seams where its shapes overlap.
  - A mask whose shapes change is the graph's (its shapes' colors and opacities composited as the reference
    composites them: added, intersected, subtracted), taking away region by region of the plane its shapes cut
    it into (up to four solid shapes), where a faint seam can show along edges inside it. One the graph does
    not draw so (gradients, more shapes) is drawn as the reference draws it.
  - Masked content on more than one canvas is masked on each, in the groups' setup again; where canvases
    overlap under a partial mask, what is under shows through the one above a little.
- **Transforms, motion paths and matrices.** Transform lists are canvas transforms of animated values; motion
  paths are lookups of the path sampled by length; animated matrices animate their decomposition; skews are
  sampled.
- **Outlines that move.** A path morphing between keyframes of the same commands is a track per coordinate. One
  morphing between different commands switches halfway, as the reference does: a copy of the shape for each run
  of the same commands (up to 32), shown only in its time. Stars and rectangles with elliptical corners have
  their points sampled over time (120 times a second, at least 24 a segment, keeping only the samples a straight
  line between their neighbours does not stand for), each coordinate a lookup of the time; a star whose number of
  points changes is a contour for each number, moved far out of sight outside its time: the times of the same
  commands share one contour, so a star switching between two counts forty times is two contours (up to 32
  different commands, changing up to 1024 times).
- **Trims and dash patterns that move.** Where only one end of the drawn part moves (drawing on or off), the
  part is a dash pattern as long as the outline, moved along it. Where both move (spinners, trims carried round
  by their offset, Figma's path trims, dash lengths that change), the part is a path of its own whose points
  the graph moves: each segment cut where the part starts and ends, from lookups of the outline sampled by
  length. A dash pattern of several dashes, or repeating along the outline, is such a part for each dash,
  each repetition a slot going round the outline as the offset moves (wrapping past its end, out of sight),
  so as many are drawn as reach the outline at once, however far the offset runs (up to 256). A moving trim
  of a fill fills such a part. A trim, moving or not, of an outline that moves is sampled as moving outlines
  are, every segment kept (those outside the part a point where it ends), an outline whose commands change
  included.
- **Gradients that move.** NSH's canvas gradients are still, so a gradient whose ends move is a still one
  placed by a transform the graph moves (a radial one too, while its focal point keeps its place within the
  circle). One whose colors change is its keyframes' gradients faded into one another; a linear one whose
  stops move (Figma's shines) is a gradient of its own between each two stops, in bands the graph moves.
  Stops that move while their colors change, stops of a radial gradient that move, and stops that change in
  number (switching halfway, as the reference switches them), are gradients sampled over time (no stop moving
  more than 2% of the gradient between two samples) faded into one another. A wholly clear stop has the
  color it goes to just after its time and the one it came from just before (where those differ, two
  gradients meet at its time).
  Paints of several gradients draw on a canvas of their own: faded keyframes add up within a clip of the
  shape, and bands of opaque colors color the shape drawn once, each from its first stop over the one before
  (bands whose edges met showed seams on Android). A stroke so painted clips to its outline.
- **Drop shadows.** The canvas's shadow is still and clipped with what casts it, so a group's shadows are
  cast by its casters: its content compiled again as a silhouette (each shape black, as see-through as it,
  or all or nothing for Figma's shadows, whose silhouette is the coverage made all or nothing), without
  clips: a clip that one of the group's shapes fills whole (a Figma frame clipping its content) stands for
  what it clips. Each shadow is a layer under the content, its opacity the layer's:
  - Where the shadow is still and hidden under the content (knocked out, or the content opaque), the casters
    are drawn in place with it and taken away again.
  - Where its offset moves, or it shows through see-through content, the casters are drawn past the view on
    a canvas reaching past it, shown within it: only their shadows fall into the view, moved by a
    translation the graph moves, then clipped by the groups' clips (`destination-in`) and knocked out as the
    content asks.
  - A group that turns or scales casts in its own lengths, on a canvas over where the shadow falls, in a
    layer moving as the group does (a view transform from the group's matrix sampled over time).
  - A hue that moves colors the shadow over; a blur that moves is several such shadows at blurs along its
    way (up to ten), faded into one another.
  On the web the canvas takes shadow lengths in its pixels; the player scales them by the pixel ratio there.
  - Where the canvas cannot cast from the content's operations (content that casts shadows itself, clipped
    otherwise or masked, or on several canvases) but the content holds still within its group, an image of
    its alpha (the reference renderer's, at the display's pixels) casts the shadow: the group may still
    move, turn and fade, and the shadow move.
- **Inner shadows.** A canvas over the content: its silhouette in the shadow's color, less the silhouette
  moved and blurred, which casts from past the view into it and takes away as it falls (`destination-out`).
  Offsets and colors move on the graph, moving blurs fade between still ones.
- **Color effects** (CSS's and SVG's color filters, Lottie's tint, fill, tritone, levels and color overlay)
  recolor each paint inside: colors, gradients' stops, text, PNG images and shadows; moving effects are
  sampled into color tracks, and what they do to alpha, a scale, fades the group. Exact where a paint is
  drawn alone; where see-through paints overlap and at edges the reference changes the blend, a little
  apart. What changes alpha otherwise, or moves over gradients and images, is drawn as the reference draws
  it.
- **Pattern fills.** The outline clips; the tiles over it draw their still content, each moved to its place
  (tiles in user space or the shape's box, with their own transform, Figma's image fills; up to 256 tiles).
- **Live text.** Each line is one `fillText` (and `strokeText`) the platform sets in its fonts, placed,
  sized and colored by the graph; lines of several styles and spaced letters are sampled (the fallback sets
  them in one style). The platform's fonts are not the reference's: glyphs differ a little.
- **Groups that only move or fade** are layers the platform moves and fades: the content drawn once in the
  group's own lengths (scaled up as far as it grows, to stay sharp) on canvases over where it is, the layer
  moved by a view transform (the group's matrix to the view, sampled over the playback and on both sides of
  where it jumps). The canvases never draw again as it moves; as a whole the layer is resampled, so thin
  lines soften a little while it moves at one pixel a point. Groups needing the view's coordinates (shadows,
  clips or masks around them) are drawn as before.
- **Events.** What a file plays on an event (an SVG's `begin="click"`) is a segment of its own; the ref's
  `trigger(event)` moves it to the start and plays it once.
- **Blurs** are drawn as images the reference renderer makes: once where they do not move, frame by frame
  where they do, kept by frame for when the loop comes round. What is blurred throughout is made at as few
  pixels as keep its blur's deviation three of them, and scaled up.
- **What the graph cannot express** (more than 256 moving dashes, outlines of more than 32 different
  commands, paths morphing between different commands more than 31 times, as a merge baked at every frame
  does, shadows of moving content the canvas cannot cast from operations) is sampled frame by frame on the
  engine thread instead (drawn as the reference draws it where it holds effects or soft masks), that part
  alone: a canvas of its own drawing the part within copies of the groups it is in (their clips, masks and
  transforms; the layers it is drawn in fade and blend it), the rest of the scene still on the graph.
  Sampled siblings with nothing drawn between share one canvas. A group whose own transform, clip or mask
  the graph cannot express is sampled whole. The reasons are logged. One of the 53 corpus files has a part
  sampled: Lottie's merge paths, baked at every frame.
- **Approximations that remain.** Figma's knock-outs and hard silhouettes count any coverage of a pixel;
  the canvas counts the coverage (a faint ring along edges that do not fall on whole pixels). A mask whose
  overlapping see-through shapes change is taken away region by region, with a faint seam along edges
  inside it. Masked content on more than one canvas lets a little of what is under show through the canvas
  above where the mask shows part.
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

The set is the check app's own scenes (fixtures, spinners, trims, moving gradients, drop shadows still and
moving, seen through, turning with a card that clips its content, blurs, soft masks, masks whose shapes turn,
masks drawn as images, trims of moving outlines, stops changing in number, a hundred moving dashes, a star
switching its points, live text, inner shadows, color effects, pattern fills, shadows cast by images, groups
that only move or fade, clear stops) and, with `--corpus`, the corpus files the tool lists. The comparison
finds the tiles by their border, renders each reference at the tile's size, and reports the share of pixels
that differ, straight and at the best shift of a pixel (a border a fraction of a pixel wide moves the
content); it writes each pair side by side. The iOS simulator's screenshots are Display P3 and converted to
sRGB first.

- The web build needs a clang that targets wasm (Homebrew's LLVM first on the `PATH` on macOS).
- `nsh test` with an `nsh` built from a checkout needs a local `.cargo/config.toml` in `examples/check`
  patching `nsh-*` to that checkout, as for the gallery.
- GPUI on macOS photographs nothing while the screen is locked.

- The web at two pixels a point: `nsh test --target web --browser <script>` with a script that starts Chrome
  with `--force-device-scale-factor=2` (the runner's own Chrome draws at one).

Last run (45 scenes, the check app's 29 and 16 corpus files, each at 25, 50 and 75%; NSH from the fork's
`main` at 4faf2cfd, upstream merged, 2026-10-25):

| Platform | Pixels a point | Tiles off the reference by, at most (live text aside) | This round's scenes (inner shadows, color effects, patterns, image shadows, moving layers, clear stops) | Live text |
|---|---|---|---|---|
| iOS simulator | 3 | 0.14% | 0.12% | 4.3% |
| Android emulator | 2.75 | 1.6% (anti-aliasing of fine strokes and bands) | 0.99% | 6.8% |
| Chrome | 1 | 2.9% | 0.26% | 3.8% |
| Chrome | 2 | 1.25% | 0.22% | 4.2% |
| GTK on Linux | 1 | 2.75% | 0.20% | 8.8% |
| GPUI on Linux | 1 | 3.6% (the moving layer of small checks) | 3.6% | 9.2% |
| GPUI on macOS | 2 | 1.21% | 0.87% | 4.4% |

At one pixel a point a pixel of anti-aliasing is a larger share: the corpus files of fine detail are the ones
over 0.5% there. Live text is set in the platform's fonts (Roboto, DejaVu Sans, the system's), the reference in
the Mac's: the glyphs and their widths differ, not where the lines are anchored or how they move. The pattern
disc moves as a layer the platform moves: GPUI on Linux resamples it at a fraction of a pixel, which shifts every
edge of its six-point checks; the platforms at two or three pixels a point keep it within 1%. GPUI on macOS
photographs nothing while the screen is locked. GPUI showed some tiles held at a moment at another moment of
their animation, on a busy display (two pixels a point): each canvas whose values move its whole drawing queued a
worker rasterization every frame, and the workers fell behind. The fork's NSH keeps one in flight per canvas
(dXnSix/nsh#38, to go upstream as `pr/gpui/01-canvas-one-job`); with it, no tile shows another moment.

## What playing costs

`vector-animation-check profile <dir> [--size <points>] [--ratio <pixels a point>]` reports, for each file of a
folder: how long its plan takes to compile, its graph's nodes and breakpoints, what each frame draws
(canvases, layers, operations), and the engine thread's work each frame for parts sampled frame by frame and
for rasters of parts that move. On the 53 corpus files at 200 points and 3 pixels a point, plans compile in
21 ms at most (a repeater of a hundred copies that move, each a layer of its own: 201 canvases, the most; 49
files under 2 ms), and the engine thread does no work a frame but for three files: rasters of blurs that
move (0.36 and 0.11 ms a frame, the first time round: each raster renders only where its part draws, a blur
at fewer pixels, and a loop's frames are kept) and merge paths baked at every frame, sampled (0.02 ms). The
graph's evaluation on the platforms' UI threads, and frame times on real phones, are not measured here.

## Roadmap

1. **Done:** the component with canvases, warped time and playback; the TSX and Rust APIs; the example app;
   moving trims, dash patterns and gradients on the graph; sampling only what the graph cannot express; soft
   masks (as images where they move as a whole); drop shadows that move, show through, turn with their group
   or are Figma's hard ones, on the graph; blurs; dotLottie; device checks on iOS, Android, the web, Linux
   (GTK, GPUI) and GPUI on macOS.
2. **Done:** live text; inner shadows; color effects; pattern fills; shadows cast by images of still
   content (shadows of shadows, of content clipped, masked or on several canvases); events and the ref's
   `trigger`; layers the platform moves and fades for groups that only move or fade; clear gradient stops.
3. Windows run (it compiles; running it needs a Windows machine or CI), real phones and their frame times.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your
option.
