---
title: Untrusted input
description: Limits and their defaults, Scene::check and Stats, the bounds inside the importers and the Lottie expression budget, renderer bounds, and when a file is an error or a warning.
group: Inputs
order: 80
---

An animation downloaded at run time is a document from the network. Reading one is bounded by `Limits`: a file over a limit is refused with an error naming it, before it can take unbounded memory or time. Pass tighter limits than the defaults when the files are not your own:

```rust
use dxn_vector_animation::{Error, Import, Limits};

/// Limits for animations downloaded at run time: small files, short timelines.
const REMOTE: Limits = Limits {
    max_bytes: 2 * 1024 * 1024,
    max_nodes: 5_000,
    max_keyframes: 50_000,
    max_depth: 32,
    max_path_points: 200_000,
    max_duration: 60.0,
};

fn read_remote(data: &[u8]) -> Result<Import, Error> {
    if dxn_vector_animation_lottie::sniff(data) {
        dxn_vector_animation_lottie::read_with(data, &REMOTE)
    } else {
        dxn_vector_animation_svg::read_with(data, &REMOTE)
    }
}

let long = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
  <rect width="10" height="10"><animate attributeName="x" values="0;5" dur="90s"/></rect>
</svg>"#;
match read_remote(long) {
    Err(Error::Limit { what, limit }) => println!("refused: over {limit} {what}"),
    Err(e) => println!("refused: {e}"),
    Ok(import) => println!("read, {} warnings", import.warnings.len()),
}
```

```text
refused: over 60 seconds of timeline
```

## Limits

`Limits` is a `Copy` struct of public fields; `Limits::default()` holds the defaults. The same limits apply to SVG, Lottie, dotLottie and [compiled scenes](./compiled-scenes.md).

| Field | Default | Bounds |
|---|---|---|
| `max_bytes` | 32 MiB (33,554,432) | The file's size; a dotLottie archive's unpacked bytes too |
| `max_nodes` | 100,000 | Groups, shapes (text included) and images; an SVG's XML nodes at 8 times it |
| `max_keyframes` | 1,000,000 | Keyframes of every track together |
| `max_depth` | 64 | Groups nested in groups, masks, patterns and nested compositions included |
| `max_path_points` | 4,000,000 | Points of every path and every keyframe of a path together, motion paths included |
| `max_duration` | 3600.0 | Seconds of timeline |

The error is `Error::Limit { what, limit }`, displayed as `the file exceeds the limit of {limit} {what}`. `what` is one of `bytes`, `bytes unpacked`, `archive entries`, `nodes`, `keyframes`, `levels of nesting`, `path points`, `seconds of timeline` and `text animator samples`.

### Checking a scene

The importers count while they read and check the finished scene once more. `Scene::check(&limits)` does that check on any scene, such as one deserialized from your own format, and returns its `Stats`:

```rust
use dxn_vector_animation::Limits;

let scene = dxn_vector_animation_lottie::read(&std::fs::read("fixtures/lottie-motion.json")?)?.scene;
let stats = scene.check(&Limits::default())?;
println!("{stats:?}");
```

```text
Stats { groups: 6, shapes: 3, images: 0, animated_properties: 3, keyframes: 6, depth: 4, path_points: 0 }
```

`Scene::stats()` counts without checking. `vector-animation inspect` prints the same counts.

## Bounds inside the importers

Some work grows with what a file asks for rather than with its size: iterations to unroll, copies to make, outlines to bake. The importers cap it:

| Where | Bound | Past it |
|---|---|---|
| SVG: SMIL | 4,000 animation instances resolved | Later instances are left out, with a warning |
| SVG: CSS and SMIL | 10,000 iterations of one animation unrolled | Later iterations are left out, with a warning |
| SVG: animations repeating forever | One [loop](./svg.md#the-loop) for all of them within `max_duration` and 10,000 iterations of each | The loop is as long as the longest, with a warning |
| Lottie: repeaters | 1,000 copies; 10,000 steps of the transform for one copy | 1,000 copies are kept, and copies further away stop at 10,000 steps, each with a warning |
| Lottie: shape modifiers | Baked outlines within half of `max_path_points` and of `max_keyframes`; 20,000 sampled moments and 200,000 points for one outline | Fewer moments or zig zag ridges, or the modifier left out, with a warning |
| Lottie: merge paths | 20,000 points of flattened outlines and 1,000,000 overlapping segment pairs for one merge | The outlines stay unmerged, with a warning |
| Lottie: text animators | 2,000,000 letters × animators for one text document; 4,000,000 letters × animators × sampled times; all text keyframes within half of `max_keyframes` | `Error::Limit` (`the file exceeds the limit of 2000000 text animator samples`); sampled less often, with a warning; `Error::Limit` (`keyframes`) when even two samples are too many |
| dotLottie | 10,000 archive entries | `Error::Limit` (`archive entries`) |

### Lottie expressions

Expressions are programs. The interpreter runs each within a budget, and an expression over any bound is left out: its property keeps its keyframes, with a warning such as `expressions over the evaluation budget are left out: their properties keep their keyframes`.

| Bound | Value |
|---|---|
| Steps for one property, over all its frames | 4,000,000 |
| Steps for all expressions of a file | 40,000,000 |
| Recursion of statements, expressions and calls | 256 levels |
| Nested function calls | 32 |
| Array length | 65,536 elements |
| String length | 65,536 bytes |
| Arrays nested in arrays | 16 |
| Expression source | 256 KiB |
| Nesting while parsing / depth of one expression's tree | 128 / 160 |
| Expressions reading one another in a chain | 16 |
| Keyframes written by all expressions | Half of `max_keyframes` |

Random numbers and wiggles are seeded by their property, so every import of a file gives the same scene.

## Sampling and rendering

- `Scene::frame_at` follows nested groups 256 levels deep at most (the importers refuse far less), drops groups whose matrix is not finite, and caps blur and shadow deviations at 10,000 scene lengths.
- The reference renderer refuses images of more than 8192 × 8192 pixels with `RenderError::Size`, and draws a pattern's tile at 4096 pixels a side at most. Clamp a size that comes from a file before rendering it.
- `vector-animation render --fps` renders at most 100,001 frames.

## Errors or warnings

| Outcome | When | What to do |
|---|---|---|
| `Err(Error::Limit { .. })` | Over a limit | Refuse the file, or raise the limit if the file is yours |
| `Err(Error::Invalid(..))` | Not a valid document | Refuse the file |
| `Err(Error::Unsupported(..))` | Nothing of it can be represented (`.svgz`, ZIP64) | Convert the file |
| `Ok(import)` with warnings | Parts left out or approximated | Show them to whoever made the file; fail a build with `vector-animation validate --strict` |

Next: [The vector-animation CLI](./cli.md), or the inputs: [SVG](./svg.md) and [Lottie](./lottie.md).
