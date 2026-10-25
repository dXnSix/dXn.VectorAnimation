---
title: The NSH player
description: Play animated SVG and Lottie in NSH apps with the VectorAnimation component, from TSX or Rust, natively on every NSH platform.
group: NSH player
order: 90
---

`dxn_vector_animation_nsh` is a UI set for [NSH](https://github.com/longbridge/nsh) (Native Shell) with one
component, `VectorAnimation`. It reads a file with the importers, compiles the scene into canvases whose
numbers and colors are nodes of NSH's native animation graph, and plays it by moving one time value: the UI
thread plays the whole animation, with no JavaScript and no engine work per frame.

```example
demo: lottie
height: 220
caption: A Lottie file, played by the NSH player in the browser.
```

## Add it to an app

An app lists the crate in its `nsh.json`, beside its own crate if it has one. TSX imports it as
`"dxn-vector-animation"`:

```json
{ "rust": ["native", "path/to/dXn.VectorAnimation/crates/dxn_vector_animation_nsh"] }
```

An app with a Rust crate of its own can depend on the player instead and register the component in its set:

```rust
registry.register::<dxn_vector_animation_nsh::VectorAnimation>();
```

The crate depends on `nsh-core` by version requirement (`0.2`), so it builds with the app's own NSH:
crates.io's, a checkout or a fork. It uses NSH's public API alone.

## Play a file

```tsx
import { VectorAnimation } from "dxn-vector-animation";
import hero from "./hero.json"; // a Lottie file
import button from "./button.vector-animation.json"; // an SVG compiled ahead

<VectorAnimation src={hero} repeat={2} onEnd={() => console.log("played twice")} />
<VectorAnimation src={button} style={{ width: 320, height: 160 }} paused={!active()} />
<VectorAnimation src="https://example.com/spinner.svg" />
```

The same from Rust, with the component's builder:

```rust
use dxn_vector_animation_nsh::{Source, vector_animation};

const HERO: &[u8] = include_bytes!("hero.json");

let hero = vector_animation()
    .src(Some(Source::bytes(HERO)))
    .on_end(|| println!("played"));
```

## Where a file comes from

| `src` | Rust | What it is |
|---|---|---|
| a JSON import | `Source::Json(value)` | A Lottie file or a compiled scene, imported as JSON |
| a string | `Source::Text(text)` | The text of an SVG, a Lottie file or a compiled scene |
| a `Uint8Array` | `Source::bytes(bytes)` | The bytes of one, or of a dotLottie archive |
| a URL | `Source::url(url)` | Fetched through NSH's network stack, under the app's network capabilities |

SVG files are usually compiled ahead with `vector-animation compile button.svg` and imported as JSON: the
app then loads the scene directly, and the SVG importer's warnings show at build time instead of at run time.
See [Compiled scenes](./compiled-scenes.md).

:::tip Size the view
The view is the animation's own size unless the app sizes it. Give it a width and height (or `width: "100%"`
inside a sized parent) and `resizeMode` fits the scene into it.
:::

## What it reports

`onLoad` receives the animation's duration, size, markers, the events it plays on and the importer's
warnings; `onError` the message of a file that could not be loaded or read:

```tsx
<VectorAnimation
  src={hero}
  onLoad={(info) => console.log(`${info.duration} s, ${info.size.width} x ${info.size.height}`)}
  onError={(message) => console.warn(message)}
/>
```

Every prop, event and ref method is in [The component](./component.md); playback control in
[Playback](./playback.md).

## Next

[The component](./component.md) for the full API, then [What it plays natively](./capabilities.md).
