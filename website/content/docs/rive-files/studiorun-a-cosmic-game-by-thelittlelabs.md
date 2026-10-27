---
title: StudioRun
description: The menu of a cosmic game by The Little Labs - thousands of shapes, blend states and nested artboards' state machines.
group: Rive
order: 155.0
---

```example
demo: rive-file-studiorun-a-cosmic-game-by-thelittlelabs
height: 424
caption: Hover over the menu's buttons to light them.
```

The [Rive community](https://rive.app/community/files/26133-49002-studiorun-a-cosmic-game-by-thelittlelabs/)'s `studiorun-a-cosmic-game-by-thelittlelabs`, read by `dxn_vector_animation_rive` in the
browser and played by the NSH player as it is: its artboard's default state machine runs, a pointer over it
reaches its listeners, and its view model is bound.

## What it exercises

- Thousands of shapes
- Blend states
- Nested artboards' state machines whose inputs the parent sets
- Hover listeners

## What is left out

The importer warns of these and plays the rest:

- The speed of a nested animation, when animated
- Three animated properties newer than the runtime tag the importer reads
- Events a nested state machine fires as it plays

The file is [`examples/gallery/animations-rive/26133-49002-studiorun-a-cosmic-game-by-thelittlelabs.riv`](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/examples/gallery/animations-rive/26133-49002-studiorun-a-cosmic-game-by-thelittlelabs.riv);
`vector-animation inspect` lists its inputs, listeners and what it leaves out. What the importer reads is in
[Rive](../rive.md).
