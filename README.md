# dXn.VectorAnimation

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Animated vector graphics for Rust, independent of any renderer or UI framework.** `dxn_vector_animation`
reads animated SVG and Lottie into one scene model: groups, shapes, paints and masks whose properties are
constants or keyframe tracks on a timeline. A renderer either samples the scene at a moment
(`Scene::frame_at(t)` gives a flat display list) or maps the tracks onto an animation system of its own.

> **Status: phase 1.** The scene model, Figma Motion's animated SVG (CSS and SMIL), Lottie, the reference
> renderer and the CLI. dotLottie archives and repeaters come next ([Roadmap](#roadmap)).

## Why

Figma Motion exports an animation as animated SVG (CSS `@keyframes` in a `<style>` block) or as Lottie and
dotLottie. resvg draws SVG as a still image, and each Lottie runtime plays its own format. This library reads
both formats into one model, so a player, a thumbnailer, a converter or a linter is written once and serves
every format.

Its first consumer is [dXn.VectorAnimation.Nsh](https://github.com/dXnSix/dXn.VectorAnimation.Nsh), a
player component for NSH apps.

## Use

```rust
let data = std::fs::read("button.svg")?;
let import = dxn_vector_animation_svg::read(&data)?; // or dxn_vector_animation_lottie::read
for warning in &import.warnings {
    eprintln!("left out: {warning}");
}
let scene = import.scene;
let frame = scene.frame_at(0.5); // the display list half a second in
let png = dxn_vector_animation_render::render_png(&frame, &Default::default())?;
```

```sh
vector-animation inspect button.svg                 # size, timeline, contents, what is left out
vector-animation validate --strict hero.json        # fails on errors (and warnings)
vector-animation compile hero.json                  # hero.vector-animation.json, a compiled scene
vector-animation render button.svg --fps 30 --width 600 --background white
```

## Crates

| Crate | What it is |
|---|---|
| `dxn_vector_animation` | The scene model, keyframe tracks and easings, the timeline, path geometry, interpolation, sampling (`Scene::frame_at`), the limits that bound untrusted files, and (feature `serde`) the compiled scene format |
| `dxn_vector_animation_svg` | Reads SVG: the static document, CSS animations (the subset Figma Motion writes) and SMIL |
| `dxn_vector_animation_lottie` | Reads Lottie JSON |
| `dxn_vector_animation_render` | A reference CPU renderer on tiny-skia: golden tests, poster frames, the CLI's images |
| `dxn_vector_animation_cli` | The `vector-animation` command: `inspect`, `validate`, `compile` (SVG or Lottie into a scene file), `render` (PNG frames) |

The importers and the renderer are separate crates, so a consumer takes only what it reads and draws. The
core and the importers build for WebAssembly; a build that plays compiled scenes takes the core alone.

## The scene model

- **Nodes.** Groups (transform, opacity, clip, mask, blend mode, the time they show, a time of their own) and
  shapes (outlines filled and stroked together, with dashes and trim), and images. Every property is a
  constant or a track.
- **Tracks.** Keyframes at times in seconds, each segment with its own easing: linear, cubic Bézier, hold, CSS
  `steps()` and `linear()`. Springs arrive as the exporters sample them.
- **Time.** A duration, a frame-rate hint, a loop mode (once, loop, ping-pong), named markers; groups offset,
  stretch and remap their time (nested compositions).
- **Transforms** are lists of operations (translate, scale, rotate, skew, matrix, motion along a path), each
  value animated on its own. Transform lists animate function by function when their functions match, as CSS
  does, and through matrix decomposition when they differ.
- **Geometry.** Paths morph between keyframes of the same commands; rectangles, ellipses and stars stay
  parametric; motion paths and trims measure arc length.
- **Effects.** Groups cast drop shadows (color, offset, blur, knocked out under the drawing or not, cast by
  its coverage or by its silhouette made all or nothing as Figma's are) and blur (Gaussian), each value
  animated; consecutive shadows are all cast by the drawing. The reference renderer
  blurs as SVG suggests, with three box blurs along each axis, drawing past the frame's edges as far as what
  is drawn there casts or blurs into it.
- **Two ways to play.** Sample the scene at any time, or read its tracks and drive them with a native
  animation system (what the NSH player does: its UI thread plays the tracks without per-frame work in Rust).
- **Compiled scenes.** `compiled::to_json` / `from_json` save and load a scene as `.vector-animation.json`, so
  a player loads it without reading SVG or Lottie again.

## Inputs

**Animated SVG** (`dxn_vector_animation_svg`)

- The static document: shapes, paths, groups and their transforms, fills and strokes (dashes and
  `pathLength` included), linear and radial gradients, clip paths, alpha and luminance masks, opacity, blend
  modes, `use` and `symbol`, nested `svg`, embedded PNG and JPEG images, and `<style>` sheets with the
  cascade: selectors of types, ids, classes, attributes (`[a]`, `=`, `~=`, `|=`, `^=`, `$=`, `*=`) and
  structural pseudo-classes (`:first-child`, `:last-child`, `:only-child`, `:nth-child()`,
  `:nth-last-child()`, `:not()`), joined by descendant, child, adjacent and sibling combinators, with CSS
  specificity.
- CSS animations, as Figma Motion writes them: `@keyframes` with a timing function per stop, the `animation`
  shorthand and longhands (delay, iterations, direction, fill mode), `transform`, `transform-origin`,
  `transform-box`, `translate`, `rotate`, `scale`, `opacity`, motion along a path (`offset-path`,
  `offset-distance`, `offset-rotate`, `offset-anchor`), fill and stroke colors and opacities, stroke width
  and dashes, and `d`.
- SMIL, which Figma Motion writes for what CSS cannot animate (`width`, `height`, path `d`): `<animate>`,
  `<set>`, `<animateColor>`, `<animateTransform>` (additive or replacing the attribute) and `<animateMotion>`
  (a `path`, an `<mpath>`, points; `rotate`, `keyPoints`), with `values`, `from` / `to` / `by`, `keyTimes`,
  `keySplines`, every `calcMode`, `dur`, `repeatCount`, `repeatDur` and `fill`. Begins are offsets (negative
  ones too) or other animations' begins and ends; animations that start one another in a cycle become the
  timeline's loop, and several animations of one attribute apply in the order they began. Shapes' geometry
  animates (`x`, `y`, `width`, `height`, `rx`, `ry`, `cx`, `cy`, `r`, line ends, `points`).
- Filters of shadows and blurs: `<filter>` chains as Figma exports drop shadows (several of them) and layer
  blurs (the source's alpha hardened, moved, blurred, colored, cut out, blended under the source),
  `feDropShadow`, `feGaussianBlur`, and CSS `filter` of `blur()`, `drop-shadow()` and `url()`, still or
  animated by `@keyframes`.
- Left out with a warning: text (export it as outlines), other filters (inner shadows, color filters),
  patterns, pseudo-elements and other pseudo-classes, SMIL begins on events, `end` times and `accumulate`.

**Lottie** (`dxn_vector_animation_lottie`)

- Shape layers (groups, rectangles, ellipses, polystars and paths; fills, strokes with dashes, linear and
  radial gradients; trim paths), null layers as parents, nested compositions with time stretch, offset and
  remapping, solid layers, embedded images, layer masks (add, subtract, intersect, inverted), track mattes
  (alpha and luminance, inverted), blend modes, bezier easings and holds, and position keyframes with spatial
  tangents as motion paths (auto-orient included), and the layer effects Drop Shadow and Gaussian Blur
  (converted as lottie-web converts them).
- Repeaters, as lottie-web repeats: copies through the repeater's transform once more each (the offset
  moving too: in closed form where the copies only turn and scale, as each copy's matrix sampled frame by
  frame otherwise), faded from the start opacity to the end one, above or below.
- Text layers drawn with the file's glyphs (`chars`), laid out as lottie-web lays them out: glyphs at a
  hundredth of the size advanced by their widths and the tracking, lines justified, box text wrapped at
  spaces, keyframed documents in turn.
- dotLottie archives (versions 1 and 2): the first animation, its images embedded, unpacked within the limits.
- Shape modifiers, changing the outlines listed before them in their group (nested groups' included) for the
  fills and strokes after them: merge paths (merge, add, subtract, intersect and exclude intersections, as
  skottie merges, on outlines flattened finely; the fills and strokes before a merge draw nothing, as in After
  Effects), round corners, pucker and bloat, zig zag and offset path (as lottie-web changes outlines; offset
  corners mitered, rounded or beveled), and twist. What they make is outline geometry: a constant path, or,
  while anything it depends on moves, a path keyframe each frame (within half the limits of path points and
  keyframes).
- Left out with a warning: expressions, text animators, text without glyphs, other effects.

## Untrusted input

A remote animation is a document from the network. Reading one is bounded by `Limits`: its size in bytes,
its nodes, keyframes, nesting depth, path points and timeline length. A file over a limit is refused with an
error naming the limit. The renderer refuses images over 8192 × 8192 pixels.

## Development

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check -p dxn_vector_animation -p dxn_vector_animation_svg -p dxn_vector_animation_lottie --target wasm32-unknown-unknown
```

Tests are grouped by feature, one file each in a crate's `tests/` (`cargo test -p dxn_vector_animation_svg
--test figma_animations`); the files they read are in `fixtures/`.

## Roadmap

1. **Done:** the scene model, Figma Motion's SVG subset, Lottie, the reference renderer and the CLI;
   effects, dotLottie archives, repeaters, Lottie text with glyphs, CSS selectors with combinators, Lottie
   shape modifiers.
2. Text animators, inner shadows.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your
option.
