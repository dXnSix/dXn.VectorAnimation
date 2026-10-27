---
title: Boopkin kill count
description: A Halloween counter - images, and a view model's number bound through converters.
group: Rive
order: 153.5
---

```example
demo: rive-file-boopkin-kill-count
height: 424
caption: It plays its state machine with its view model bound, as Rive's players binding automatically do; its counter starts at the view model's value.
```

The [Rive community](https://rive.app/community/files/24598-46450-boopkin-kill-count/)'s `boopkin-kill-count`, read by `dxn_vector_animation_rive` in the
browser and played by the NSH player as it is: its artboard's default state machine runs, a pointer over it
reaches its listeners, and its view model is bound.

## What it exercises

- 115 images
- A view model number ("Kill Count Number") bound to text through converters
- State machine layers looping over different lengths

The file is [`examples/gallery/animations-rive/24598-46450-boopkin-kill-count.riv`](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/examples/gallery/animations-rive/24598-46450-boopkin-kill-count.riv);
`vector-animation inspect` lists its inputs, listeners and what it leaves out. What the importer reads is in
[Rive](../rive.md).
