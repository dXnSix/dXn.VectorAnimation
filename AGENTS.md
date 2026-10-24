# dXn.VectorAnimation.Nsh contributor guidance

The NSH player for animated SVG and Lottie (the `dxn_vector_animation_nsh` crate): a UI set with the
`VectorAnimation` component. How it plays is in `README.md`.

## Boundaries

- Reading and modelling animations belongs to [dXn.VectorAnimation](https://github.com/dXnSix/dXn.VectorAnimation);
  this crate maps its scenes onto NSH and holds no format code of its own. A change the player needs in the
  scene model is made there first, pushed, and taken here with `cargo update -p dxn_vector_animation`.
- It uses NSH's public API alone (Rust components, the canvas `Painter`, animated values, composites) and asks
  for no change to NSH. It depends on `nsh-core` by version requirement, never by path or Git, so apps keep
  their own NSH; this workspace's own builds take the dXnSix fork through `[patch.crates-io]`.
- Playback runs on NSH's native animation graph; a scene the graph cannot express is sampled frame by frame
  on the engine thread (`plan::Unsupported` says why, logged at info).
- The compiler's output (`plan`, `graph`) is plain data: tests replay it with Canvas 2D semantics and compare
  the pixels with the reference renderer's (`tests/common`). A new mapping gets a scene in `tests/plans.rs`.

## Validation

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Tests are grouped by feature, one file each in `crates/dxn_vector_animation_nsh/tests/` (`cargo test --test
plans`); `component.rs` runs the component on a headless engine. The example app's TSX tests run with an
`nsh` CLI: `nsh test examples/gallery`.

## Naming

The crate is `dxn_vector_animation_nsh`, its UI set `"dxn-vector-animation"`, the component `VectorAnimation`.

## Commits

Use Conventional Commits with meaningful scopes. Never add Claude co-author lines.
