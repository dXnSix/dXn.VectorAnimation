---
title: Playback
description: Pause, speed, direction, repeats, segments, scrubbing, events and reduced motion with VectorAnimation.
group: NSH player
order: 110
---

Playback is one time value moving from the segment's start to its end. Every property of the scene is a
function of that value on NSH's animation graph, so pausing, seeking, speeding up or reversing changes one
number and the whole scene follows on the UI thread.

```example
demo: playback
height: 300
caption: Pause, speed, direction and scrubbing on a Figma Motion export.
```

```tsx
const paused = signal(false);
const rate = signal(1);
const direction = signal<"normal" | "reverse" | "alternate">("normal");
const progress = signal<number | undefined>(undefined);

<VectorAnimation
  src={button}
  paused={paused()}
  rate={rate()}
  direction={direction()}
  progress={progress()}
/>
```

## Pause and speed

`paused` stops the time value where it is, and `false` plays on from there. `rate` scales the playback
speed: `2` plays a two second animation in one second, `0.5` in four.

## Direction and repeats

| Props | What plays |
|---|---|
| none | The file's own loop mode: once, looped, or there and back (Lottie's and SMIL's ping-pong) |
| `repeat={true}` | Forever |
| `repeat={3}` | Three times in all, then `onEnd` |
| `repeat={false}` | Once, then `onEnd` |
| `direction="reverse"` | From the end to the start |
| `direction="alternate"` | To the end, then back, and on |

A run that ended starts over from the beginning when it plays again.

## Delays that play once

A timeline can loop from a loop start rather than from 0: an SVG whose animations repeat forever after
delays (a staggered loader, three dots 0.2 s apart) plays its delays once, as a browser does. The player
plays the timeline from its start to its end once, then from the loop start to the end on every later run:

| Props | With a loop start |
|---|---|
| none, `repeat={true}`, `repeat={3}` | The first run from the start, every later run from the loop start |
| `direction="alternate"` (and the file's ping-pong) | To the end, back to the loop start, and on |
| `direction="reverse"` | Every run from the end to the start: the whole timeline |
| `segment` | The segment plays whole every run |
| `progress` | 0 to 1 over the whole timeline, delays included |
| `seek(seconds)` | From that time to the end, then from the loop start |
| Reduced motion | The poster frame, as for any timeline (`first` is 0) |

A paused player resumed before the loop start plays the rest of the delays first.

## Segments

`segment` plays a part of the timeline: a marker's span (Lottie markers, a Rive file's animations) or
`[start, end]` in seconds. `repeat` and `direction` apply to the segment; a marker with a loop of its own (a
Rive animation's) repeats as it says unless `repeat` is set.

```tsx
<VectorAnimation src={hero} segment="intro" repeat={false} onEnd={() => segment.set("loop")} />
<VectorAnimation src={hero} segment={[1.0, 2.5]} repeat={true} />
```

## Scrubbing

`progress` from 0 to 1 shows one frame of the segment and stops playback while it is set; `undefined` hands
the time back to playback, which goes on from where the frame was. The ref's `seek(seconds)` moves playback
to a time without stopping it:

```tsx
<VectorAnimation src={hero} progress={dragging() ? position() : undefined} ref={(r) => (player = r)} />
<Button onPress={() => player?.seek(0)}><Text>Restart</Text></Button>
```

:::note One run at a time
A change of `progress`, `paused`, `segment` or the source stops the run playing as a whole, also a looping
one, before the new state takes over: a hold never plays on.
:::

## Events

An SVG's SMIL animations that begin on an event (`begin="click"` on an element with `id="btn"`) are a
segment of their own, outside the timeline. `onLoad` lists them in `events`, and the ref's `trigger` plays
one once from its start and holds its end; the timeline's own animations play from their start meanwhile.

```example
demo: events
height: 220
caption: A button whose SMIL animation begins on a click, played by trigger("btn.click").
```

```tsx
let player: VectorAnimationRef | undefined;

<VectorAnimation src={clickableSvg} ref={(r) => (player = r)} onLoad={(info) => console.log(info.events)} />
<Button onPress={() => player?.trigger("btn.click")}><Text>Click</Text></Button>
```

`seek` gives the timeline back: what an event played makes way for it.

## Reduced motion and off screen

When the user turns on reduced motion, the player shows its poster frame and runs no clock: the first frame
of the segment by default, or `posterFrame="last"`, a time in seconds or a marker's start.
`ignoreReduceMotion` plays anyway, for animations that carry meaning (a progress indicator).

The examples on this site set `ignoreReduceMotion`, so they play even when your system asks for reduced motion
(Windows' "Animation effects" turned off, for one, which browsers report that way): they are here to be seen. An
app that does not set it shows the still frame to such a reader.

`paused` is the app's to set: it knows which screen is in front. A list row that scrolls away is disposed
with its animation.

## Next

[What it plays natively](./capabilities.md) for what the graph moves; [Platforms](./platforms.md) for where it
was checked.
