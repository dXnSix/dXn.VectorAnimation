---
title: Lottie and dotLottie
description: What dxn_vector_animation_lottie reads - layers, shapes and shape modifiers, expressions evaluated and baked within budgets, text with glyphs and animators, effects, layer styles and dotLottie archives - and what it leaves out.
group: Inputs
order: 70
---

`dxn_vector_animation_lottie` reads Lottie JSON (Bodymovin's export, LottieFiles' downloads) and dotLottie archives into a scene. Each layer becomes a group named after it, so you can find a layer and read its tracks:

```rust
use dxn_vector_animation::{Group, Node, TransformOp};

/// The group of the layer named `name` (Lottie's `nm`), anywhere in the tree.
fn layer<'a>(group: &'a Group, name: &str) -> Option<&'a Group> {
    group.children.iter().find_map(|child| match child {
        Node::Group(g) if g.name.as_deref() == Some(name) => Some(g),
        Node::Group(g) => layer(g, name),
        _ => None,
    })
}

let data = std::fs::read("fixtures/lottie-motion.json")?;
let scene = dxn_vector_animation_lottie::read(&data)?.scene;
let boxed = layer(&scene.root, "Box").ok_or("no layer Box")?;
println!("id {:?}, shows {:?}", boxed.id, boxed.active);
for op in &boxed.transform {
    if let TransformOp::Translate { x, .. } = op {
        for key in x.keyframes() {
            println!("x = {} at {} s, then {:?}", key.value, key.time, key.easing);
        }
    }
}
```

```text
id Some("1"), shows Some(TimeRange { start: 0.0, end: 2.0 })
x = 20 at 0 s, then CubicBezier { x1: 0.33, y1: 0.0, x2: 0.67, y2: 1.0 }
x = 80 at 1 s, then Linear
```

```example
demo: lottie
height: 220
caption: fixtures/lottie-motion.json - a parented layer, a trim path and eased position keyframes.
```

| Function | Does |
|---|---|
| `read(data)` | Reads Lottie JSON or a dotLottie archive with the default `Limits` |
| `read_with(data, &limits)` | Reads within your limits; an archive's unpacked bytes count too |
| `sniff(data)` | A ZIP archive, or a JSON object whose first or last 64 KiB hold `"layers"` and `"fr"` or `"op"` (exporters write the header first or last) |

A document needs `fr` (above 0), `op` and `layers`; without them it is `Error::Invalid` (``invalid file: no frame rate (`fr`)``).

## The timeline

| Scene | From |
|---|---|
| `timeline.duration` | `(op - ip) / fr` seconds |
| `timeline.frame_rate` | `Some(fr)` |
| `timeline.loop_mode` | Always `Loop`: Lottie has no loop setting, and players loop by default |
| `timeline.markers` | `markers`: `cm` the name, `tm` and `dr` in frames, in seconds from the in point |
| Scene time 0 | The composition's in point, `ip` |

Keyframe times are frames in the file and seconds in the scene. Bezier easings (`o` and `i` tangents) become `Easing::CubicBezier`, hold keyframes (`h: 1`) `Easing::Hold`.

## Layers

| `ty` | Layer | In the scene |
|---|---|---|
| 0 | Precomposition | A group with a time of its own (start time, stretch, time remapping), clipped to its size |
| 1 | Solid | A rectangle of its color |
| 2 | Image | An embedded (`data:`) image |
| 3 | Null | Nothing drawn: a parent for other layers' transforms |
| 4 | Shape | Its shape contents |
| 5 | Text | Glyph shapes, or text set in a font |

Every layer is a group with `id` from `ind`, `name` from `nm`, and `active` from `ip` to `op` (a layer playing backwards too). Around it: its transform (anchor, position, scale, rotation, skew, opacity) and its parents', position keyframes with spatial tangents as a motion path (auto-orient included), blend modes, layer masks (add, subtract, intersect, inverted, with opacity), and track mattes (alpha and luminance, inverted).

## Shapes and modifiers

Shape layers read groups (`gr`) with their transforms (`tr`), rectangles (`rc`), ellipses (`el`), polystars (`sr`) and paths (`sh`); fills and strokes (`fl`, `st`, with dashes), linear and radial gradient fills and strokes (`gf`, `gs`, a highlight moving the focal point); trim paths (`tm`), simultaneously or individually. A fill or stroke paints the outlines listed before it in its group; styles listed first draw on top, as in After Effects.

Repeaters (`rp`) copy what comes before them through their transform, once more each, faded from the start opacity to the end one, above or below, as lottie-web repeats them: at most 1,000 copies.

Shape modifiers change the outlines listed before them in their group (nested groups' included) for the styles after them:

| Modifier | `ty` | Does |
|---|---|---|
| Merge paths | `mm` | Merge, add, subtract, intersect, exclude intersections, as skottie merges; the styles before a merge draw nothing, as in After Effects |
| Round corners | `rd` | Rounds sharp corners (rectangles' too), not curves |
| Pucker and bloat | `pb` | Moves points and handles toward or away from the center |
| Zig zag | `zz` | Ridges along the outline, as lottie-web makes them |
| Offset path | `op` | Grows or shrinks the outline; corners mitered, rounded or beveled |
| Twist | `tw` | Turns points about the center, the more the nearer |

What a modifier makes is outline geometry: a constant path, or, while anything it depends on moves, a path keyframe for each frame, within half the limits of path points and keyframes. Past those, the modifier samples fewer frames or is left out, with a warning.

## Expressions

Expressions are evaluated and baked into keyframes before the conversion: each property's expression runs at every frame its layer shows, and its results become keyframes, straight lines between frames. The interpreter reads Bodymovin's rewritten form (`$bm_rt`, `$bm_sum` and its siblings) and the JavaScript expressions are written in (variables, functions, `if`, `switch`, loops, `try`, arrays, strings, `Math`, vector math), with After Effects' expression API:

- `time`, `value`, `valueAtTime`, `velocity`, `speed`, `key`, `numKeys`, `nearestKey`
- `loopIn`, `loopOut` and their `Duration` forms (`cycle`, `pingpong`, `offset`, `continue`)
- `wiggle`, `temporalWiggle`, `smooth`, `seedRandom`, `random`, `gaussRandom`, `noise`
- `linear`, `ease`, `easeIn`, `easeOut`, `posterizeTime`, time, angle and color conversions
- `thisComp`, `comp(name)`, `thisLayer` and other layers' transforms, timing and parents, and expression controls (`effect(name)(property)`: sliders, angles, points, colors, checkboxes, layers)

Random numbers and wiggles are seeded by their property, so every import of a file gives the same scene. A property read by another expression is sampled after its own expression runs.

An expression that uses anything else, reads expressions in a cycle, fails, or runs over its budget keeps its property's keyframes, with a warning such as ``expressions using `toComp` are left out: their properties keep their keyframes``. Evaluation is bounded: 4,000,000 steps a property, 40,000,000 a file, and caps on recursion, arrays, strings and keyframes ([Untrusted input](./limits.md#lottie-expressions) lists them).

## Text

- Text with the file's glyphs (`chars`) is drawn as shapes, laid out as lottie-web lays it out: glyphs advanced by their widths and the tracking, lines justified, box text wrapped at spaces, keyframed documents in turn.
- Text animators, as lottie-web animates text: range selectors (start, end and offset in percent or by index; over characters, characters excluding spaces, words or lines; square, ramp up, ramp down, triangle, round and smooth shapes; ease high and low; randomized order, the same every time) and the expression selector After Effects writes by default. They change the anchor, position, scale, skew, rotation, opacity, fill and stroke colors, hue, saturation, brightness, stroke width and tracking of each character; several animators combine. Each animated character is a group of its own, sampled at the file's frame rate where what selects or changes it moves.
- Text on a path follows a layer mask's outline.
- Text without glyphs is set in the file's font when drawn (family, weight and style from its font list), one line at a time, justified; it is not wrapped and its animators are left out.

## Effects and layer styles

| Lottie | In the scene |
|---|---|
| Drop Shadow effect (25) | `Effect::DropShadow`, converted as lottie-web converts it |
| Gaussian Blur effect (29) | `Effect::Blur`; a blur along one axis blurs along both, with a warning |
| Tint (20), Fill (21), Tritone (23), Levels (24) | `Effect::Color` with `ColorOp::Tint`, `Fill`, `GradientMap`, `Transfer` tables |
| Expression controls (5) | Nothing drawn; their values feed expressions |
| Drop shadow and inner shadow layer styles | `Effect::DropShadow`, `Effect::InnerShadow` |
| Color overlay layer style | `ColorOp::Overlay` |

Effects come before layer styles. Disabled effects are skipped.

## dotLottie

A dotLottie file is a ZIP of `manifest.json`, the animations (`animations/<id>.json` in version 1, `a/<id>.json` in version 2) and their images (`images/` in version 1, `i/` in version 2). The importer reads both versions:

- The first animation in the manifest is read; when there are more, a warning says how many.
- State machines (`s/` in version 2) are left out; a warning says how many.
- Its images are embedded in the scene. An image missing from the archive is a warning.
- Entries stored or deflated read; other compression methods and ZIP64 archives are `Error::Unsupported`. At most 10,000 entries, and the unpacked bytes stay within `max_bytes`.

## What is left out

Each of these reads, with a warning:

| In the file | What happens |
|---|---|
| Expressions on paths and text documents, or using layer space transforms (`toComp`, `fromComp`), `sourceRectAtTime`, shape contents, text or markers | The property keeps its keyframes |
| Wiggly and other expression text selectors, and animators with only those | Left out |
| Animators and paths of text without glyphs | Left out; text on a path is laid on a line |
| Other effects (strokes of masks, wipes, distortions) | Left out, named |
| Stroke, outer and inner glow, bevel and emboss, satin, gradient overlay layer styles | Left out, named |
| 3D layers | Drawn flat; 3D rotations left out |
| Images that are not embedded | Left out |
| Mask expansion; lighten, darken and difference masks | Left out; drawn as added |
| Hard mix blend mode | Drawn as normal |
| A track matte without its matte layer, a parenting cycle | Left out |

Next: [Untrusted input](./limits.md), or [Compiled scenes](./compiled-scenes.md) to ship Lottie files pre-read.
