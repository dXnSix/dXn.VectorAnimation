---
title: The component
description: Every prop, event and ref method of VectorAnimation, in TSX and Rust.
group: NSH player
order: 100
---

`VectorAnimation` is one view. Its props say what plays and how; its events report what it loaded and when
it ends; its ref moves playback and triggers what a file plays on events.

```tsx
<VectorAnimation
  src={hero}
  style={{ width: 240, height: 240 }}
  repeat={true}
  rate={1.5}
  resizeMode="cover"
  onLoad={(info) => markers.set(info.markers)}
/>
```

## Props

| Prop | Type | Default | Meaning |
|---|---|---|---|
| `src` | URL, text, bytes or JSON | none | What plays ([sources](./nsh-player.md#where-a-file-comes-from)) |
| `paused` | `boolean` | `false` | Stops playback where it is; `false` plays on from there |
| `repeat` | `boolean` or `number` | the file's loop mode | `true` forever, a number of times in all, `false` once |
| `direction` | `"normal"`, `"reverse"`, `"alternate"` | `"normal"` | Which way it plays; `alternate` goes there and back |
| `rate` | `number` | `1` | Playback speed |
| `segment` | marker name or `[start, end]` | all of it | What part plays, in seconds or by a marker of the file (a Rive animation plays as its loop says unless `repeat` is set) |
| `resizeMode` | `"contain"`, `"cover"`, `"stretch"`, `"center"` | `"contain"` | How the scene fits the view |
| `progress` | `number` from 0 to 1 | none | The frame shown, set by the app (scrubbing); stops playback while set |
| `posterFrame` | `"first"`, `"last"`, seconds or a marker | `"first"` | What shows when the user reduces motion |
| `ignoreReduceMotion` | `boolean` | `false` | Plays when motion is reduced too |
| `accessibilityLabel` | `string` | none | What screen readers say |

The view is the animation's own size unless the app sizes it; a sized view fits the scene as `resizeMode`
says.

## Events

| Event | Payload | When |
|---|---|---|
| `onLoad` | `AnimationInfo` | The file was read |
| `onEnd` | none | Playback reached its end; never while repeating forever |
| `onCue` | `{ name, time, url, target, properties }` | Playback passed a cue: an event a Rive file's state machine fired, or a listener fired as the pointer acted; `properties` is an object of its custom properties |
| `onInput` | `{ name, path, kind, value }` | An input of a Rive file's state machine changed: a listener under the pointer, `setInput` or `fire` changed it (a bool's value is 0 or 1, a trigger fired 1) |
| `onError` | `string` | The file could not be loaded or read |

`AnimationInfo`:

| Field | Type | What it is |
|---|---|---|
| `duration` | `number` | Seconds one play-through lasts |
| `size` | `{ width, height }` | The animation's own size, in points |
| `markers` | `{ name, start, duration }[]` | Its markers: Lottie's, a Rive file's animations, and the segments it plays on events (`on:btn.click`) |
| `events` | `string[]` | The events it has something to play on, such as `btn.click` |
| `inputs` | `{ name, path, kind, value }[]` | A Rive file's state machines' inputs, its nested artboards' too (`path` names those: `#` and each nested artboard's index, `/` between; empty for the file's own): `kind` is `"bool"`, `"number"` or `"trigger"`, `value` the value at first |
| `warnings` | `string[]` | What the file holds that the scene leaves out |

## Ref methods

| Method | What it does |
|---|---|
| `seek(seconds)` | Moves to a time on the timeline; playback goes on from there unless paused |
| `trigger(event)` | Plays what the file has for an event once from its start and holds its end |
| `setInput(name, value)` | Sets a Rive file's state machine input (a boolean or a number) now; the machine plays on from here as Rive plays it with the change |
| `fire(name)` | Fires a Rive file's state machine trigger now |
| `setInputAt(path, name, value)`, `fireAt(path, name)` | The same for the state machine a nested artboard plays, at its path (as `onLoad`'s `inputs` gives it) |

A Rive file whose state machine has listeners hears the pointer over it as Rive's web runtime does: a finger
or a button going down presses, moving moves, lifting releases and leaves; the mouse moving over it moves,
leaving it leaves. The view over it that hears the pointer has the test id `vector-animation-pointer`
(`POINTER_TEST_ID`). Each change reads the file again off the engine thread; playback goes on in the new
scene from where it is. See [Rive](./rive.md#inputs-and-listeners).

```tsx
import { VectorAnimation, type VectorAnimationRef } from "dxn-vector-animation";

let player: VectorAnimationRef | undefined;

<VectorAnimation src={button} ref={(r) => (player = r)} />
<Button onPress={() => player?.trigger("btn.click")}><Text>Play the click</Text></Button>
```

## From Rust

Each prop is a builder method of `vector_animation()`, each event an `on_` method, and the ref's methods are
`VectorAnimationApi` through a `component_ref`:

```rust
use dxn_vector_animation_nsh::{
    AnimationInfo, Direction, Repeat, ResizeMode, Segment, Source, VectorAnimation, VectorAnimationApi,
    vector_animation,
};
use nsh_core::ui::prelude::*;

let player = component_ref::<VectorAnimation>();
let duration = signal(0.0);

let view = vector_animation()
    .component_ref(&player)
    .src(Some(Source::Text(SPINNER_SVG.into())))
    .repeat(Some(Repeat::Times(3)))
    .direction(Direction::Alternate)
    .rate(2.0)
    .segment(Some(Segment::Marker("intro".into())))
    .resize_mode(ResizeMode::Cover)
    .on_load(move |info: &AnimationInfo| duration.set(info.duration))
    .on_error(|message: &String| eprintln!("{message}"));

// Later, from a handler:
if let Some(api) = player.api::<VectorAnimationApi>() {
    api.trigger("btn.click".into());
}
```

| TSX | Rust |
|---|---|
| `repeat={true}` / `{3}` / `{false}` | `Repeat::Forever` / `Repeat::Times(3)` / `Repeat::Once` |
| `segment="intro"` / `{[0.5, 1.5]}` | `Segment::Marker("intro".into())` / `Segment::Span(0.5, 1.5)` |
| `posterFrame="last"` / `{1.2}` / `"intro"` | `PosterFrame::Last` / `PosterFrame::At(At::Seconds(1.2))` / `PosterFrame::At(At::Marker(..))` |

## Next

[Playback](./playback.md) shows the props at work; [What it plays natively](./capabilities.md) what happens to a
scene.
