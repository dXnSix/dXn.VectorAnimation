---
title: Frames and rendering
description: Scene::frame_at and the Frame display list, drawing it with the reference renderer (render_png, render, render_region, RenderOptions), its size bound, and fonts.
group: Concepts
order: 40
---

`Scene::frame_at(t)` gives the scene at one moment as a `Frame`: a flat list of drawing operations any renderer can draw. The reference renderer in `dxn_vector_animation_render` draws it with tiny-skia; you reach for it for poster frames, thumbnails, golden tests and video frames, or as the model when you write a renderer of your own.

```rust
use dxn_vector_animation::{FramePaint, Item};

let scene = dxn_vector_animation_svg::read(&std::fs::read("fixtures/figma-motion.svg")?)?.scene;
let frame = scene.frame_at(1.0);
for item in &frame.items {
    match item {
        Item::Fill { paint, transform, .. } => {
            let kind = match paint {
                FramePaint::Solid(c) => format!("solid {:.2} {:.2} {:.2} {:.2}", c.r, c.g, c.b, c.a),
                FramePaint::Linear { .. } => "linear gradient".into(),
                FramePaint::Radial { .. } => "radial gradient".into(),
                FramePaint::Pattern { .. } => "pattern".into(),
            };
            println!("fill   {kind} at ({:.2}, {:.2})", transform.e, transform.f);
        }
        Item::Stroke { style, .. } => println!("stroke {} wide", style.width),
        Item::Image { rect, .. } => println!("image  {} × {}", rect.width, rect.height),
        Item::Text { spans, .. } => println!("text   {} spans", spans.len()),
        Item::PushLayer(layer) => println!(
            "push   opacity {:.2}, clip {}, mask {}, {} effects",
            layer.opacity,
            layer.clip.is_some(),
            layer.mask.is_some(),
            layer.effects.len()
        ),
        Item::PopLayer => println!("pop"),
    }
}
println!("bounds {:?}", frame.bounds());
```

```text
fill   solid 1.00 0.00 0.00 1.00 at (19.30, 29.65)
fill   solid 0.00 0.00 1.00 1.00 at (100.00, 50.00)
push   opacity 1.00, clip false, mask true, 0 effects
fill   linear gradient at (0.00, 0.00)
pop
bounds Some(Rect { x: 19.299999999999997, y: 10.0, width: 170.7, height: 44.00000203192628 })
```

The button at its largest, the dot halfway along its path, and a gradient square in a layer masked by a circle.

## The display list

A `Frame` is `width`, `height` (the scene's, in user units) and `items`, drawn first to last:

| Item | Draws |
|---|---|
| `Item::Fill { path, transform, paint, rule }` | `path` in its own coordinates, `transform` taking it to the scene's |
| `Item::Stroke { path, transform, paint, style }` | The same, the transform applying to the stroke too; `StrokeStyle` has `width`, `cap`, `join`, `miter_limit`, `dash: Option<(Vec<f64>, f64)>` |
| `Item::Image { data, rect, transform }` | An encoded image stretched into `rect` |
| `Item::Text { spans, transform }` | `FrameTextSpan`s: the text, its position, anchor, font families, size, weight, italic, letter spacing, fill and stroke |
| `Item::PushLayer(Layer)` | Starts drawing into a layer |
| `Item::PopLayer` | Composes the last layer pushed onto what is under it |

Paints are `FramePaint::Solid(Color)`, `Linear { start, end, stops, spread, transform }`, `Radial { center, radius, focal, stops, spread, transform }` and `Pattern { tile, transform, items }`, with the fill's or stroke's opacity already folded into their colors.

A `Layer` carries what applies to everything drawn in it, in this order: its `effects` (`FrameEffect::DropShadow`, `InnerShadow`, `Blur`, `Color`, with offsets and blur deviations already in the frame's space), then its `clip` (`Vec<ClipPath>`, a union) and `mask` (`FrameMask`: `kind`, `inverted`, and `items` in scene coordinates), then `opacity` and `blend` as it composes. `Frame::bounds()` is where the items draw at most: strokes as far as they reach, layers grown by three deviations of their blurs and shadows, within their clips and masks.

What `frame_at` does on the way:

- A group becomes a layer only when it needs one: opacity under 1, a blend mode, a clip, a mask or effects. Other groups vanish into their children's matrices.
- Groups outside their `active` range, groups at opacity 0, paints that would draw nothing and strokes of no width are left out of the list.
- Paths are final: rectangles, ellipses and stars are outlined, trims cut, the shape's outlines joined. An odd dash list is repeated, as SVG and Canvas 2D repeat it.
- Bounding-box gradients and patterns are mapped onto the shape's box.

### Writing a renderer

A renderer walks the list with a stack. On `PushLayer`, start an offscreen surface; on `PopLayer`, apply the layer's effects to what was drawn, clip and mask it, and compose it with its opacity and blend mode. Fills, strokes, images and text go to the surface on top of the stack. That maps directly onto Canvas 2D, Skia or any 2D API with offscreen layers. `dxn_vector_animation::filter::apply_steps` applies a `FrameEffect::Color`'s steps to a color, and the reference renderer's source is a complete example of the rest.

## The reference renderer

| Function | Returns |
|---|---|
| `render_png(&frame, &options)` | `Result<Vec<u8>, RenderError>`: PNG bytes |
| `render(&frame, &options)` | `Result<tiny_skia::Pixmap, RenderError>`: premultiplied RGBA pixels |
| `render_region(&frame, region, scale)` | A part of the frame (`Rect` in its space) at `scale` pixels per unit |
| `size(&frame, &options)` | The pixel size `options` asks for |
| `fonts_available()` | Whether the system has any font to set text in |
| `text_layout(&spans)` | The glyph outlines of text spans, one `Path` each |

`RenderOptions`:

| Field | Default | Meaning |
|---|---|---|
| `width: Option<u32>` | `None` | Pixels wide |
| `height: Option<u32>` | `None` | Pixels high |
| `scale: f64` | `1.0` | Pixels per user unit, used when neither size is given |
| `background: Option<Color>` | `None` (transparent) | Drawn under the frame |

With both sizes the frame stretches to fill them; with one, the other keeps the aspect.

```rust
use dxn_vector_animation::Rect;
use dxn_vector_animation_render::{RenderOptions, render, render_region, size};

let frame = scene.frame_at(1.0);

// The whole frame into a premultiplied RGBA pixmap (tiny-skia's).
let options = RenderOptions { height: Some(300), ..RenderOptions::default() };
println!("{:?}", size(&frame, &options)); // (600, 300): the width keeps the aspect
let pixmap = render(&frame, &options)?;

// Only the gradient disc at the right, 2 pixels a unit.
let part = render_region(&frame, Rect::new(140.0, 0.0, 60.0, 60.0), 2.0)?;
println!("{} × {}", part.width(), part.height()); // 120 × 120
std::fs::write("part.png", part.encode_png()?)?;
```

`render` and `render_region` return a tiny-skia 0.12 `Pixmap`: its methods (`width`, `data`, `pixel`, `encode_png`) work without naming the type; add `tiny-skia = "0.12"` to your dependencies to name it, for example in a field. A region renders as that part of the whole frame does, effects reaching in from outside it included.

To export every frame of an animation, sample at its frame rate:

```rust
use dxn_vector_animation_render::{RenderOptions, render_png};

let scene = dxn_vector_animation_lottie::read(&std::fs::read("fixtures/lottie-motion.json")?)?.scene;
let fps = scene.timeline.frame_rate.unwrap_or(30.0);
let options = RenderOptions { scale: 2.0, ..RenderOptions::default() };
std::fs::create_dir_all("frames")?;
let last = (scene.timeline.duration * fps).floor() as usize;
for i in 0..=last {
    let png = render_png(&scene.frame_at(i as f64 / fps), &options)?;
    std::fs::write(format!("frames/{i:04}.png"), png)?;
}
```

That writes 61 frames for the 2 s fixture at 30 fps, both ends included. `vector-animation render --fps` does the same from the [CLI](./cli.md).

### Bounds

:::warning Images over 8192 × 8192 pixels are refused
`render`, `render_png` and `render_region` refuse an image of no pixels or of more than 8192 × 8192 (67,108,864) pixels with `RenderError::Size(width, height)` before drawing anything: `cannot render a 20000 × 10000 image`. Clamp sizes that come from users or files.
:::

A pattern's tile is drawn at 4096 pixels a side at most. Blurs are three box blurs along each axis, as SVG's `feGaussianBlur` suggests (a true Gaussian kernel for small deviations), and shadows and blurs take what is drawn past the frame's edges into account, so an edge shows what would fall into it. `RenderError::Png` reports a failed PNG encoding.

## Fonts

Live text (SVG `<text>`, Lottie text without glyphs) is set when it is drawn. The reference renderer loads the system's fonts with fontdb on first use and shapes each span with rustybuzz (kerning and ligatures included):

- A span's `families` are tried in order. CSS's generic names map to the system's: `serif`, `sans-serif`, `monospace`, `cursive`, `fantasy`, and `system-ui`, `-apple-system`, `ui-sans-serif`, `ui-serif`, `ui-monospace`.
- Then the system's sans-serif, then any font at all. The weight and italic choose the face.
- With no fonts (`fonts_available()` is `false`, as in a bare container), text draws nothing.

Text therefore looks a little different from machine to machine, as it does in browsers. Lottie text exported with its glyphs (`chars`) is shapes, not text, and draws the same everywhere.

Next: [Compiled scenes](./compiled-scenes.md), or the [CLI](./cli.md) for rendering without code.
