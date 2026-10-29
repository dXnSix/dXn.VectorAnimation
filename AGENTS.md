# dXn.VectorAnimation contributor guidance

Animated vector graphics independent of any renderer or UI framework (the scene model, the SVG and Lottie
importers, the reference renderer and the CLI: `dxn_vector_animation`, `_svg`, `_lottie`, `_render`, `_cli`),
and the NSH player built on them (`dxn_vector_animation_nsh`, a UI set with the `VectorAnimation` component, and
`dxn_vector_animation_check`, its device check tool). What it reads, how the scene works and how the player plays
it is on the site (`website/content/`; see Documentation below); `README.md` is the short overview.

## Boundaries

- The core crates (`dxn_vector_animation`, `_svg`, `_lottie`, `_render`, `_cli`) depend on no UI framework: not
  NSH, not GPUI. CI fails when any of them (dev-dependencies included) pulls in an `nsh-*` crate. The NSH crates
  consume the core and never the other way round; they are left out of `default-members`, so a plain `cargo
  build` or `cargo test` builds no NSH.
- The core crate and the importers build for `wasm32-unknown-unknown`: no threads, files or system fonts in
  them.
- Importers read untrusted documents: every size, count and depth they take is bounded by the core's
  `Limits`, and what a file holds that the scene leaves out is a warning in `Import::warnings`, never a
  silent drop.
- The player maps scenes onto NSH and holds no format code of its own: what it needs from the scene model is
  added to the core.
- It uses NSH's public API alone (Rust components, the canvas `Painter`, animated values, composites) and asks
  for no change to NSH. It depends on `nsh-core` by version requirement, never by path or Git, so apps keep
  their own NSH; this workspace's own builds take the dXnSix fork through `[patch.crates-io]`.
- Playback runs on NSH's native animation graph; a scene the graph cannot express is sampled frame by frame
  on the engine thread (`plan::Unsupported` says why, logged at info).
- The player's compiler output (`plan`, `graph`) is plain data: tests replay it with Canvas 2D semantics and
  compare the pixels with the reference renderer's (`tests/common`). A new mapping gets a scene in
  `tests/plans.rs`, or a test file of its own when it is a feature of its own (`tests/moving_dashes.rs`).

## Validation

The core:

```sh
cargo fmt --check
cargo clippy --workspace --exclude dxn_vector_animation_nsh --exclude dxn_vector_animation_check --all-targets --all-features -- -D warnings
cargo test --all-features
cargo check -p dxn_vector_animation -p dxn_vector_animation_svg -p dxn_vector_animation_lottie --target wasm32-unknown-unknown
```

The NSH player:

```sh
cargo clippy -p dxn_vector_animation_nsh -p dxn_vector_animation_check --all-targets -- -D warnings
cargo test -p dxn_vector_animation_nsh -p dxn_vector_animation_check
```

Tests are grouped by feature: one file per feature in each crate's `tests/` (select one with
`cargo test -p <crate> --test <file>`); shared input files are in `fixtures/`. A new feature gets a test
file (or a test in the file of its feature) and runs only that group while iterating; the full list of what
it touches (the core's, the player's, or both) runs once before committing. The player's `component.rs` runs
the component on a headless engine; the example apps' TSX tests run with an `nsh` CLI (`nsh test
examples/gallery`, and `examples/check` on devices: the site's "Platforms and device checks" page).

## Documentation

The site (https://dxnsix.github.io/dXn.VectorAnimation/) is `website/`, installed from the dXn.Docs
workbench (its engine and its `content/vectoranimation/`); its pages are `website/content/`. Its live
examples are `examples/docs`, an NSH app the Pages workflow (`.github/workflows/docs.yml`) publishes for the
web with the dXnSix fork of NSH into `website/public/examples/` (ignored), shown by the pages' `example`
blocks. A change to what the player does updates the page that describes it.

## Naming

Crates are `dxn_vector_animation` and `dxn_vector_animation_<part>`; the binary is `vector-animation`. The
player's UI set is `"dxn-vector-animation"`, its component `VectorAnimation`.

## Commits

Use Conventional Commits with meaningful scopes. Never add Claude co-author lines.
