---
title: Compiled scenes
description: The serde feature and the compiled .vector-animation.json format - to_json, from_json, sniff, versioning and checks - and why players load compiled scenes.
group: Concepts
order: 50
---

A compiled scene is a `Scene` saved as JSON, `.vector-animation.json`, so a player loads it without reading SVG or Lottie again. You reach for it when animations are known at build time: read them once with the importers, ship the compiled files, and load them at run time with the core crate alone.

```rust
use std::error::Error;

use dxn_vector_animation::{Limits, compiled};

fn main() -> Result<(), Box<dyn Error>> {
    // Ahead of time: read the SVG once and save the scene.
    let scene = dxn_vector_animation_svg::read(&std::fs::read("fixtures/figma-motion.svg")?)?.scene;
    std::fs::write("button.vector-animation.json", compiled::to_json(&scene, false)?)?;

    // At run time: load it with the core alone.
    let data = std::fs::read("button.vector-animation.json")?;
    assert!(compiled::sniff(&data));
    let loaded = compiled::from_json(&data, &Limits::default())?;
    assert_eq!(loaded, scene);
    Ok(())
}
```

A compiled scene reads back equal to the scene it was written from.

## The serde feature

The `compiled` module exists with the core's `serde` feature, which also derives `Serialize` and `Deserialize` for `Scene` and every type in it:

```toml
[dependencies]
dxn_vector_animation = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main", features = ["serde"] }
```

It adds `serde`, `serde_json` and `base64` and nothing else. The importers and the renderer do not need it; the CLI and the NSH player turn it on.

| Item | Does |
|---|---|
| `compiled::to_json(&scene, pretty) -> Result<String, Error>` | The scene in the compiled format; `pretty` indents it. A number that is not finite is `Error::Unsupported` |
| `compiled::from_json(data, &limits) -> Result<Scene, Error>` | Reads a compiled scene, checked against `limits` |
| `compiled::sniff(data) -> bool` | Whether the bytes are a compiled scene: JSON whose first 256 bytes name the format |
| `FORMAT` | `"dxn-vector-animation"` |
| `FORMAT_VERSION` | `1` |

## From the command line

```bash
vector-animation compile button.svg                     # writes button.vector-animation.json
vector-animation compile hero.json -o assets/hero.vector-animation.json --pretty
```

`compile` takes an SVG, a Lottie JSON or dotLottie file (or a compiled scene), prints the warnings to stderr and the path it wrote to stdout; `-o -` writes the scene to stdout instead. See [The vector-animation CLI](./cli.md#compile).

## The format

This SVG, a dot pulsing:

```html
<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40">
  <style>
    @keyframes pulse { 0% { opacity: 1; } 50% { opacity: 0.3; } 100% { opacity: 1; } }
    #dot { animation: pulse 1.2s ease-in-out infinite; }
  </style>
  <circle id="dot" cx="20" cy="20" r="10" fill="#0a84ff"/>
</svg>
```

compiles with `--pretty` to:

```json
{
  "format": "dxn-vector-animation",
  "version": 1,
  "scene": {
    "width": 40.0,
    "height": 40.0,
    "timeline": {
      "duration": 1.2,
      "loopMode": "loop"
    },
    "root": {
      "opacity": 1.0,
      "children": [
        {
          "type": "group",
          "id": "dot",
          "opacity": {
            "keyframes": [
              {
                "time": 0.0,
                "value": 1.0,
                "easing": { "type": "cubicBezier", "x1": 0.42, "y1": 0.0, "x2": 0.58, "y2": 1.0 }
              },
              {
                "time": 0.6,
                "value": 0.3,
                "easing": { "type": "cubicBezier", "x1": 0.42, "y1": 0.0, "x2": 0.58, "y2": 1.0 }
              },
              {
                "time": 1.2,
                "value": 1.0,
                "easing": { "type": "cubicBezier", "x1": 0.42, "y1": 0.0, "x2": 0.58, "y2": 1.0 }
              }
            ]
          },
          "children": [
            {
              "type": "shape",
              "id": "dot",
              "paths": [
                {
                  "geometry": {
                    "type": "ellipse",
                    "center": { "x": 20.0, "y": 20.0 },
                    "radius": { "x": 10.0, "y": 10.0 },
                    "direction": "clockwise",
                    "start": "right"
                  }
                }
              ],
              "fill": {
                "paint": {
                  "type": "solid",
                  "color": { "r": 0.0392156862745098, "g": 0.5176470588235295, "b": 1.0, "a": 1.0 }
                },
                "opacity": 1.0,
                "rule": "nonZero"
              },
              "paintOrder": "fillFirst"
            }
          ]
        }
      ]
    }
  }
}
```

(Short objects are joined on one line here; the CLI writes one value a line.) The JSON mirrors the [scene model](./scene-model.md):

| Rule | Example |
|---|---|
| An envelope of `format`, `version` and `scene` | `"format": "dxn-vector-animation", "version": 1` |
| Field names in camelCase | `loopMode`, `paintOrder`, `frameRate` |
| Enum variants tagged by `type`, unit variants as camelCase strings | `"type": "cubicBezier"`, `"loopMode": "loop"` |
| An `Animated` value is the plain value when constant, `{ "keyframes": [...] }` when animated | `"opacity": 1.0` |
| A keyframe's easing is left out when linear | the last keyframe of a linear track has only `time` and `value` |
| Empty lists, `None` and defaults are left out | no `markers`, no `transform`, no `loopStart` when it is 0 |
| Paths are path data of absolute `M`, `L`, `Q`, `C` and `Z` | `"M0 50C50 0 150 100 200 50"` |
| Images are `data:` URIs, base64 | `"data:image/png;base64,iVBOR..."` |

The minified form of this file is 846 bytes.

## Versions and checks

`from_json` treats a compiled scene as untrusted input, like the importers treat files:

1. It refuses a file over `limits.max_bytes` with `Error::Limit`.
2. It reads the envelope's `format` and `version`; a document that is not JSON or lacks them is `Error::Invalid` (``invalid file: missing field `version` at line 1 column 33``).
3. It checks `format`: another name is `Error::Invalid` (`invalid file: not a dxn-vector-animation file`).
4. It checks `version`: a newer one is `Error::Unsupported` (`unsupported: format version 2 (this library reads up to 1)`), whatever its scene holds.
5. It parses the scene; one that is not a scene is `Error::Invalid` (``invalid file: missing field `width` at line 1 column 55``).
6. It checks the scene against the limits with `Scene::check`: nodes, keyframes, depth, path points and duration.

`to_json` checks the other way: JSON has no NaN or infinity (serde_json would write `null`, which no scene reads back), so a scene holding one is refused with `Error::Unsupported`, naming where it is: ``unsupported: `scene.root.children[0].opacity` is NaN, which the compiled format cannot hold``.

`FORMAT_VERSION` is 1. Compile with the same library commit the player is built with, so both agree on the scene types.

A timeline's `loopStart` (where its loop starts over, after delays that play once) came within version 1: it is written only when set, and a file without it loops its whole timeline. A reader from before it ignores the field and loops a file that has one from 0, as SVG imports looped before: delays play again each time round, which is how the same SVG played then, not a different meaning, so the version stays 1.

:::tip Your own container
Because the scene types implement `Serialize` and `Deserialize`, a `Scene` can live inside your own JSON (or any serde format). Then nothing checks it: call `scene.check(&limits)` yourself after deserializing data you did not write.
:::

## Why players load compiled scenes

- **A smaller player.** Loading needs the core with `serde` only. The SVG and Lottie importers, their CSS, SMIL and expression interpreters, stay in the build tool. This matters most for [WebAssembly](./how-it-works.md#webassembly) builds.
- **Work done once.** Selectors are matched, CSS and SMIL animations unrolled into keyframes, Lottie expressions evaluated and baked, shape modifiers applied, at compile time, not on every load.
- **Warnings at build time.** `compile` prints what the file leaves out when the designer can still change it; `vector-animation validate --strict` can fail the build.
- **Plain JSON.** A bundler imports it as any JSON file. The NSH player's TSX apps import SVG animations this way (`import button from "./button.vector-animation.json"`); see [The NSH player](./nsh-player.md).

Next: the inputs, [SVG and Figma Motion](./svg.md), [Lottie and dotLottie](./lottie.md) and [Rive](./rive.md).
