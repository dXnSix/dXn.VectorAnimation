# dXn.VectorAnimation.Nsh contributor guidance

The NSH player for animated SVG and Lottie (the `dxn_vector_animation_nsh` crate): a UI set with the
`VectorAnimation` component. The design is in `README.md`.

## Boundaries

- Reading and modelling animations belongs to [dXn.VectorAnimation](https://github.com/dXnSix/dXn.VectorAnimation);
  this crate maps its scenes onto NSH and holds no format code of its own.
- It uses NSH's public API alone (Rust components, the canvas `Painter`, animated values, composites) and asks
  for no change to NSH. It depends on `nsh-core` by version requirement, never by path or Git, so apps keep
  their own NSH.
- Playback runs on NSH's native animation graph; per-frame work on the engine thread is the fallback for what
  the graph cannot move, one canvas at a time.

## Naming

The crate is `dxn_vector_animation_nsh`, its UI set `"dxn-vector-animation"`, the component `VectorAnimation`.

## Commits

Use Conventional Commits with meaningful scopes. Never add Claude co-author lines.
