# dXn.VectorAnimation contributor guidance

Animated vector graphics independent of any renderer or UI framework (the `dxn_vector_animation*` crates in
`crates/`). What it reads and how the scene works is in `README.md`.

## Boundaries

- No crate here depends on NSH, GPUI or any UI framework. NSH's player is
  [dXn.VectorAnimation.Nsh](https://github.com/dXnSix/dXn.VectorAnimation.Nsh); it consumes these crates and
  never the other way round.
- The core crate and the importers build for `wasm32-unknown-unknown`: no threads, files or system fonts in
  them.
- Importers read untrusted documents: every size, count and depth they take is bounded by the core's
  `Limits`, and what a file holds that the scene leaves out is a warning in `Import::warnings`, never a
  silent drop.

## Validation

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check -p dxn_vector_animation -p dxn_vector_animation_svg -p dxn_vector_animation_lottie --target wasm32-unknown-unknown
```

Tests are grouped by feature: one file per feature in each crate's `tests/` (select one with
`cargo test -p <crate> --test <file>`); shared input files are in `fixtures/`. A new feature gets a test
file (or a test in the file of its feature) and runs only that group while iterating; the full list runs
once before committing.

## Naming

Crates are `dxn_vector_animation` and `dxn_vector_animation_<part>`; the binary is `vector-animation`.

## Commits

Use Conventional Commits with meaningful scopes. Never add Claude co-author lines.
