---
title: Status and roadmap
description: What is done, what is left, and how to build, test and contribute to dXn.VectorAnimation.
group: Project
order: 160
---

dXn.VectorAnimation is in its first phase: the scene model, the importers, the reference renderer, the CLI and
the NSH player are done and checked; the crates are not published to crates.io yet and are used as Git
dependencies.

## Done

The scene model and its inputs:

- the scene model, Figma Motion's SVG subset (CSS animations and SMIL), Lottie, the reference renderer and the
  CLI;
- effects, dotLottie archives, repeaters, Lottie text with glyphs, its animators and paths, Lottie shape
  modifiers, CSS selectors with combinators and pseudo-classes;
- inner shadows, color effects, patterns, live text, SMIL end times, accumulation and events;
- Lottie expressions, evaluated and baked into keyframes;
- Rive files, phase 1: an artboard's shapes, paints, clips, solos and images, and its linear animations as
  markers with their loops; feathered edges; strokes that do not scale; nested artboards; text;
  joysticks; bones, skins and image meshes; constraints; draw rules; text modifiers.

The NSH player:

- the component with canvases, warped time and playback; the TSX and Rust APIs; the example apps;
- moving trims, dash patterns and gradients on the graph; sampling only what the graph cannot express; soft
  masks; drop shadows that move, show through, turn with their group or are Figma's hard ones; blurs;
- live text, inner shadows, color effects, pattern fills, shadows cast by images, events and `trigger`, layers
  the platform moves and fades, clear gradient stops;
- device checks on iOS, Android, the web, Linux (GTK, GPUI), macOS (AppKit, GPUI) and Windows (CI).

## Left

- Lottie expressions on paths and text documents, and layer space transforms (`toComp`, `fromComp`).
- Rive, as its [design](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/docs/design/rive.md) says: what
  the runtime evaluates (what is left of text), then
  state machines in the core and the player, then data binding and layout.
- Real phones and their frame times.
- Why GTK in a slow virtual machine sometimes takes the check app's steps seconds late.

## Build and test

```bash
cargo fmt --check
# The core (a plain `cargo test` builds it alone):
cargo clippy --workspace --exclude dxn_vector_animation_nsh --exclude dxn_vector_animation_check --all-targets --all-features -- -D warnings
cargo test --all-features
cargo check -p dxn_vector_animation -p dxn_vector_animation_svg -p dxn_vector_animation_lottie -p dxn_vector_animation_rive --target wasm32-unknown-unknown
# The NSH player:
cargo clippy -p dxn_vector_animation_nsh -p dxn_vector_animation_check --all-targets -- -D warnings
cargo test -p dxn_vector_animation_nsh -p dxn_vector_animation_check
```

The core's crates depend on no UI framework: CI fails when one of them pulls in an `nsh-*` crate, and they
are the workspace's default members, so a plain `cargo build` builds no NSH. The player's crates build with
`-p`. The workspace's own builds of the player take the dXnSix fork of NSH through `[patch.crates-io]`.

Tests are grouped by feature, one file each in a crate's `tests/`; a change runs its own group while it is
being made and the full list once before it is committed. The player's `tests/plans.rs` replays compiled
scenes with Canvas 2D semantics and compares their pixels with the reference renderer's;
`tests/corpus.rs` does the same for a folder of real files (`DXN_CORPUS=<dir> cargo test -p
dxn_vector_animation_nsh --test corpus -- --ignored`).

## Contributing

- Conventional Commits with meaningful scopes (`feat(svg): ...`, `fix(nsh): ...`).
- A new feature gets a test file of its own, or a test in the file of its feature.
- What the player needs from the scene model is added to the core first.
- The site's pages live in `website/content/`; `website/README.md` says how to preview them.

Licensed under either of Apache License 2.0 or MIT, at your option.

## Next

[Get started](./getting-started.md) with the core, or [The NSH player](./nsh-player.md).
