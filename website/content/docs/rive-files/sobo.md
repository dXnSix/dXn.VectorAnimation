---
title: Sobo
description: A forest creature that reacts to a tap - a nested artboard's listener firing a view model trigger.
group: Rive
order: 156.5
---

```example
demo: rive-file-sobo
height: 555
caption: Tap the creature: its nested artboard's listener fires the view model trigger `trigState`, which its state machine follows.
```

The [Rive community](https://rive.app/community/files/28761-54484-sobo/)'s `sobo`, read by `dxn_vector_animation_rive` in the
browser and played by the NSH player as it is: its artboard's default state machine runs, a pointer over it
reaches its listeners, and its view model is bound.

## What it exercises

- A listener in a nested artboard firing a view model trigger
- A view model's numbers and trigger bound to the state machine

## What is left out

The importer warns of these and plays the rest:

- A nested artboard's second state machine

The file is [`examples/gallery/animations-rive/28761-54484-sobo.riv`](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/examples/gallery/animations-rive/28761-54484-sobo.riv);
`vector-animation inspect` lists its inputs, listeners and what it leaves out. What the importer reads is in
[Rive](../rive.md).
