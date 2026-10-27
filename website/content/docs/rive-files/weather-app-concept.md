---
title: Weather app
description: A weather app concept with two buttons - nested artboards, inner feathers and hover listeners.
group: Rive
order: 151.0
---

```example
demo: rive-file-weather-app-concept
height: 760
caption: Hover over the left or right button and click it: the buttons' listeners set their hover inputs and fire the `-` and `+` triggers; the pointer anywhere over the card moves a node after it.
```

The [Rive community](https://rive.app/community/files/18719-35183-weather-app-concept/)'s `weather-app-concept`, read by `dxn_vector_animation_rive` in the
browser and played by the NSH player as it is: its artboard's default state machine runs, a pointer over it
reaches its listeners, and its view model is bound.

## What it exercises

- Nested artboards, each with its own state machine
- Hover, click and move listeners
- Feathered fills and strokes, drawn frame by frame (the costliest file to play on the web)

## What is left out

The importer warns of these and plays the rest:

- Three inner feathers that move in a nested artboard

The file is [`examples/gallery/animations-rive/18719-35183-weather-app-concept.riv`](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/examples/gallery/animations-rive/18719-35183-weather-app-concept.riv);
`vector-animation inspect` lists its inputs, listeners and what it leaves out. What the importer reads is in
[Rive](../rive.md).
