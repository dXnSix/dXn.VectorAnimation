---
title: Driving dashboard
description: A car dashboard driven by its view model - bindings by name, nested view models, and states that fire view model triggers.
group: Rive
order: 155.5
---

```example
demo: rive-file-futuristic-driving-ui-concept
height: 352
caption: Press the accelerator to drive: its listener sets the view model's `isAccelerating`, which the dashboard's bindings follow.
```

The [Rive community](https://rive.app/community/files/27835-52594-futuristic-driving-ui-concept/)'s `futuristic-driving-ui-concept`, read by `dxn_vector_animation_rive` in the
browser and played by the NSH player as it is: its artboard's default state machine runs, a pointer over it
reaches its listeners, and its view model is bound.

## What it exercises

- Data binding by name and nested view models
- States and transitions firing view model triggers

## What is left out

The importer warns of these and plays the rest:

- Layout: components are drawn where the file puts them
- Animated custom properties and layout widths
- Images in a format the importer does not read

The file is [`examples/gallery/animations-rive/27835-52594-futuristic-driving-ui-concept.riv`](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/examples/gallery/animations-rive/27835-52594-futuristic-driving-ui-concept.riv);
`vector-animation inspect` lists its inputs, listeners and what it leaves out. What the importer reads is in
[Rive](../rive.md).
