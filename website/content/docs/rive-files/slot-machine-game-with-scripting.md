---
title: Slot machine
description: A slot machine built with Rive's scripting and layout, both left out - it draws little of what it should.
group: Rive
order: 154.5
---

```example
demo: rive-file-slot-machine-game-with-scripting
height: 760
caption: Shown for what the importer leaves out: a file made with Rive's Luau scripts and layout plays only its plain shapes and animations.
```

The [Rive community](https://rive.app/community/files/25759-53726-slot-machine-game-with-scripting/)'s `slot-machine-game-with-scripting`, read by `dxn_vector_animation_rive` in the
browser and played by the NSH player as it is: its artboard's default state machine runs, a pointer over it
reaches its listeners, and its view model is bound.

## What it exercises

- Nested artboards three deep
- Data binding

## What is left out

The importer warns of these and plays the rest:

- Scripted drawables and formula converters (Luau scripting)
- Layout: components are drawn where the file puts them
- Component lists and draggable constraints

The file is [`examples/gallery/animations-rive/25759-53726-slot-machine-game-with-scripting.riv`](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/examples/gallery/animations-rive/25759-53726-slot-machine-game-with-scripting.riv);
`vector-animation inspect` lists its inputs, listeners and what it leaves out. What the importer reads is in
[Rive](../rive.md).
