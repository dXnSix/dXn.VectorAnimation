# dXn.VectorAnimation contributor guidance

Animated vector graphics independent of any renderer or UI framework (the `dxn_vector_animation*` crates in
`crates/`). The design is in `README.md`.

## Boundaries

- No crate here depends on NSH, GPUI or any UI framework. NSH's player is
  [dXn.VectorAnimation.Nsh](https://github.com/dXnSix/dXn.VectorAnimation.Nsh); it consumes these crates and
  never the other way round.
- The core crate (`dxn_vector_animation`) builds for `wasm32-unknown-unknown`: no threads, files or system
  fonts in it.
- Importers read untrusted documents: every size, count and depth they take is bounded by the core's limits.

## Naming

Crates are `dxn_vector_animation` and `dxn_vector_animation_<part>`; the binary is `vector-animation`.

## Commits

Use Conventional Commits with meaningful scopes. Never add Claude co-author lines.
