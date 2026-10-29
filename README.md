# dXn.VectorAnimation

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Documentation:** https://dxnsix.github.io/dXn.VectorAnimation/

**Animated SVG and Lottie for Rust, independent of any renderer or UI framework.** `dxn_vector_animation`
reads Figma Motion's animated SVG (CSS and SMIL), Lottie and dotLottie into one scene model: groups, shapes,
paints and masks whose properties are constants or keyframe tracks on a timeline. A renderer either samples
the scene at a moment (`Scene::frame_at(t)` gives a flat display list) or maps the tracks onto an animation
system of its own. `dxn_vector_animation_nsh` does the latter for [NSH](https://github.com/dXnSix/nsh) apps,
natively on iOS, Android, macOS, Windows, Linux and the web.

## Highlights

| Area | What you get |
|---|---|
| **One scene model** | Animated SVG, Lottie and dotLottie read into the same tracks, easings and timeline, so a player, a thumbnailer, a converter or a linter is written once. |
| **Figma Motion's SVG** | CSS `@keyframes` and SMIL, selectors with combinators, filters, patterns and live text. |
| **Lottie** | Shapes, shape modifiers, text with its animators, effects, layer styles and expressions, evaluated and baked into keyframes. |
| **Reference renderer and CLI** | A tiny-skia renderer for golden tests and poster frames, and a `vector-animation` command to inspect, validate, compile and render files. |
| **Native playback** | The NSH player turns a scene into canvases that NSH's animation graph moves on the UI thread, with no JavaScript or engine work per frame. |
| **Bounded input** | Every size, count and depth read from a file is limited, and what the scene leaves out is a warning, never a silent drop. |

## Installation

The crates are not on crates.io and have no release tags yet. Depend on the `main` branch, taking only the
crates you use:

```toml
[dependencies]
dxn_vector_animation = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main" }
dxn_vector_animation_svg = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main" }
dxn_vector_animation_lottie = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main" }
dxn_vector_animation_render = { git = "https://github.com/dXnSix/dXn.VectorAnimation", branch = "main" }
```

`Cargo.lock` records the commit; `cargo update` moves to the latest one. Until tags exist, every commit to
`main` can change the API.

- **Rust:** edition 2024, MSRV **1.97**.
- **Targets:** the core crate and the importers build for `wasm32-unknown-unknown`.
- **Feature:** `serde` on `dxn_vector_animation` adds the compiled scene format.

| Crate | What it is |
|---|---|
| `dxn_vector_animation` | The scene model, tracks and easings, the timeline, sampling and `Limits` |
| `dxn_vector_animation_svg` | Reads animated SVG |
| `dxn_vector_animation_lottie` | Reads Lottie JSON and dotLottie archives |
| `dxn_vector_animation_render` | The reference CPU renderer |
| `dxn_vector_animation_cli` | The `vector-animation` command |
| `dxn_vector_animation_nsh` | The NSH player: the `VectorAnimation` component |
| `dxn_vector_animation_check` | The player's device check tool |

## A quick tour

### Read, sample and render

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

### Use the command line

```sh
vector-animation inspect button.svg                 # size, timeline, contents, what is left out
vector-animation validate --strict hero.json        # fails on errors (and warnings)
vector-animation compile hero.json                  # hero.vector-animation.json, a compiled scene
vector-animation render button.svg --fps 30 --width 600 --background white
```

### Play in an NSH app

List the player in `nsh.json`, then use the component:

```json
{ "rust": ["native", "path/to/dXn.VectorAnimation/crates/dxn_vector_animation_nsh"] }
```

```tsx
import { VectorAnimation } from "dxn-vector-animation";
import hero from "./hero.json"; // a Lottie file

<VectorAnimation src={hero} repeat={2} onEnd={() => …} />
<VectorAnimation src="https://example.com/spinner.svg" style={{ width: 320, height: 160 }} />
```

The component's props, how the scene maps onto NSH, the platforms it was checked on and the live examples are
in the [documentation](https://dxnsix.github.io/dXn.VectorAnimation/).

## Status

Phase 1. The scene model, both importers, the reference renderer, the CLI and the NSH player are in place and
checked against 53 real animation files on iOS, Android, the web, Linux, macOS and Windows; no real phone has
run the player yet. What is left is on the [roadmap](https://dxnsix.github.io/dXn.VectorAnimation/docs/roadmap).

## Development

See [AGENTS.md](AGENTS.md) for the validation commands. The documentation site is in `website/`.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your
option.
