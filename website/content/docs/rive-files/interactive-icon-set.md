---
title: Icon set
description: A set of icons that animate under the pointer - a nested artboard per icon, each with its own state machine and hover listeners.
group: Rive
order: 154.0
---

```example
demo: rive-file-interactive-icon-set
height: 760
caption: Move the pointer over an icon: its nested state machine's boolean turns on as it enters and off as it leaves.
```

The [Rive community](https://rive.app/community/files/25691-49048-interactive-icon-set/)'s `interactive-icon-set`, read by `dxn_vector_animation_rive` in the
browser and played by the NSH player as it is: its artboard's default state machine runs, a pointer over it
reaches its listeners, and its view model is bound.

## What it exercises

- Many nested artboards, each with its state machine
- Enter and exit listeners on nested artboards

The file is [`examples/gallery/animations-rive/25691-49048-interactive-icon-set.riv`](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/examples/gallery/animations-rive/25691-49048-interactive-icon-set.riv);
`vector-animation inspect` lists its inputs, listeners and what it leaves out. What the importer reads is in
[Rive](../rive.md).
