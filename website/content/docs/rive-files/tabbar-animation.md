---
title: Tab bar
description: A tab bar whose icons animate when picked - clicks on nested artboards, and Rive events reaching listeners.
group: Rive
order: 152.0
---

```example
demo: rive-file-tabbar-animation
height: 477
caption: Click a tab's icon: its listener fires the tab's trigger, and the events its nested artboard fires as it plays reach the parent's listeners, which light the tab.
```

The [Rive community](https://rive.app/community/files/20751-39047-tabbar-animation/)'s `tabbar-animation`, read by `dxn_vector_animation_rive` in the
browser and played by the NSH player as it is: its artboard's default state machine runs, a pointer over it
reaches its listeners, and its view model is bound.

## What it exercises

- Click listeners on nested artboards
- Events from a nested artboard's listeners reaching its parent's listeners
- Glows drawn with feathers

## What is left out

The importer warns of these and plays the rest:

- Eight inner feathers that move
- Events a nested state machine fires as it plays (those its listeners fire reach the parent)

The file is [`examples/gallery/animations-rive/20751-39047-tabbar-animation.riv`](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/examples/gallery/animations-rive/20751-39047-tabbar-animation.riv);
`vector-animation inspect` lists its inputs, listeners and what it leaves out. What the importer reads is in
[Rive](../rive.md).
