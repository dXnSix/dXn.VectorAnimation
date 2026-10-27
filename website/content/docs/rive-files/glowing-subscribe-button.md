---
title: Subscribe button
description: A glowing subscribe button - hover and click listeners firing triggers, and layers that loop over different lengths.
group: Rive
order: 151.5
---

```example
demo: rive-file-glowing-subscribe-button
height: 424
caption: Hover over the button to light it, click it to subscribe.
```

The [Rive community](https://rive.app/community/files/20749-39045-glowing-subscribe-button/)'s `glowing-subscribe-button`, read by `dxn_vector_animation_rive` in the
browser and played by the NSH player as it is: its artboard's default state machine runs, a pointer over it
reaches its listeners, and its view model is bound.

## What it exercises

- Enter, exit, click and down listeners firing triggers
- State machine layers looping over different lengths (the scene repeats every 6.9 seconds)
- 185 animated properties, 12,266 keyframes

## What is left out

The importer warns of these and plays the rest:

- The layout of a nested artboard: it is drawn where the file puts it

The file is [`examples/gallery/animations-rive/20749-39045-glowing-subscribe-button.riv`](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/examples/gallery/animations-rive/20749-39045-glowing-subscribe-button.riv);
`vector-animation inspect` lists its inputs, listeners and what it leaves out. What the importer reads is in
[Rive](../rive.md).
