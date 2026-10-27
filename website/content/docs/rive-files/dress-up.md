---
title: Dress-up owl
description: An owl dressed by clicking the flowers above it - images, and listeners setting a nested artboard's inputs.
group: Rive
order: 152.5
---

```example
demo: rive-file-dress-up
height: 700
caption: Move the pointer over a flower and the owl looks at it; click one to dress the owl, click the owl to take it off.
```

The [Rive community](https://rive.app/community/files/22794-42639-dress-up/)'s `dress-up`, read by `dxn_vector_animation_rive` in the
browser and played by the NSH player as it is: its artboard's default state machine runs, a pointer over it
reaches its listeners, and its view model is bound.

## What it exercises

- 154 images
- Enter and click listeners setting a nested state machine's numbers and firing its trigger
- A nested artboard's events reaching its parent's listeners

## What is left out

The importer warns of these and plays the rest:

- Events a nested state machine fires as it plays (those its listeners fire reach the parent)

The file is [`examples/gallery/animations-rive/22794-42639-dress-up.riv`](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/examples/gallery/animations-rive/22794-42639-dress-up.riv);
`vector-animation inspect` lists its inputs, listeners and what it leaves out. What the importer reads is in
[Rive](../rive.md).
