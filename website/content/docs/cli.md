---
title: The vector-animation CLI
description: The vector-animation command - inspect, validate, compile and render animated SVG, Lottie, dotLottie and compiled scenes - with every flag, its output and exit codes.
group: Tools
order: 140
---

`vector-animation` reads any file the library reads and inspects it, checks it, compiles it into a scene or renders it to PNG. You reach for it in build scripts and CI, and to see what the library makes of a file before writing code.

```bash
cargo install --git https://github.com/dXnSix/dXn.VectorAnimation --branch main dxn_vector_animation_cli
vector-animation validate --strict hero.json
```

From a checkout, `cargo run -p dxn_vector_animation_cli -- <command>` runs it without installing.

| Command | Does |
|---|---|
| `inspect <FILE>` | Prints the file's size, timeline, contents and what it leaves out |
| `validate [--strict] <FILE>` | Reads the file and reports whether it plays; fails on errors, and on warnings with `--strict` |
| `compile [-o <OUTPUT>] [--pretty] <FILE>` | Writes the scene as a compiled `.vector-animation.json` |
| `render [OPTIONS] <FILE>` | Renders frames as PNG images |

Every command takes an animated SVG, a Lottie JSON, a dotLottie archive or a compiled scene, and tells them apart by their content, not their extension: a compiled scene first, then Lottie (a ZIP archive, or JSON that is not SVG), then SVG. Files are read with the default [limits](./limits.md).

## inspect

```bash
vector-animation inspect lottie-motion.json
```

```text
lottie-motion.json
  format       Lottie
  size         100 × 100
  timeline     2.000 s at 30 fps, loops
  marker       half at 1.000 s for 0.500 s
  nodes        6 groups, 3 shapes, 0 images (depth 4)
  animation    3 animated properties, 6 keyframes
  path points  0
  left out     nothing
```

| Line | Shows |
|---|---|
| `format` | `animated SVG`, `Lottie` or `compiled scene` |
| `size` | The scene's width and height in user units |
| `timeline` | Duration, frame rate when the file has one, and `once`, `loops` or `ping-pong` |
| `marker` | Each named marker: start and duration |
| `event` | Each event the file plays something on, and the `render` flag that plays it |
| `nodes`, `animation`, `path points` | The scene's [`Stats`](./limits.md#checking-a-scene) |
| `left out` | Each warning, or `nothing` |

An SVG whose button animates on `begin="click"` lists its event:

```text
  event        btn.click: plays for 0.300 s (render --event btn.click=<seconds>)
```

## validate

```bash
vector-animation validate figma-motion.svg
vector-animation validate --strict foreign.svg
```

```text
figma-motion.svg: valid animated SVG
warning: <foreignObject> is left out
foreign.svg: 1 warning(s)
```

| Flag | Effect |
|---|---|
| `--strict` | Fails when the file has any warning |

Warnings are printed to stdout, one `warning:` line each. Without `--strict` a file with warnings is valid; with it, the command prints the count and exits with 1. A file that cannot be read prints `error: <file>: <reason>` to stderr and exits with 1:

```text
error: broken.json: invalid file: no frame rate (`fr`)
```

## compile

```bash
vector-animation compile button.svg
vector-animation compile hero.json -o assets/hero.vector-animation.json --pretty
```

```text
button.vector-animation.json
assets/hero.vector-animation.json
```

| Flag | Default | Effect |
|---|---|---|
| `-o`, `--output <OUTPUT>` | Next to the file, its extension replaced by `.vector-animation.json` | Where to write; `-` for stdout |
| `--pretty` | off | Indented JSON |

Warnings go to stderr, the path written to stdout. With `-o -` the scene itself goes to stdout instead, for a pipe:

```bash
vector-animation compile button.svg -o - | gzip > button.vector-animation.json.gz
```

A scene holding a number JSON cannot hold (NaN or an infinity) is not written: the command prints the error and exits with 1. The format is described in [Compiled scenes](./compiled-scenes.md).

## render

```bash
vector-animation render figma-motion.svg --time 0.5 --width 400 --background white
vector-animation render lottie-motion.json --fps 2
vector-animation render lottie-motion.json -t 0 -t 1 -o shots
```

```text
figma-motion.png  t=0.500s
lottie-motion-frames/lottie-motion-0000.png  t=0.000s
lottie-motion-frames/lottie-motion-0001.png  t=0.500s
lottie-motion-frames/lottie-motion-0002.png  t=1.000s
lottie-motion-frames/lottie-motion-0003.png  t=1.500s
lottie-motion-frames/lottie-motion-0004.png  t=2.000s
shots/lottie-motion-0000.png  t=0.000s
shots/lottie-motion-0001.png  t=1.000s
```

| Flag | Default | Effect |
|---|---|---|
| `-t`, `--time <TIMES>` | `0` | A time to render, in seconds; repeat it for several |
| `--fps <FPS>` | | Renders the timeline from 0 to its end every 1/FPS seconds (at most 100,001 frames); cannot be combined with `--time` |
| `--width <WIDTH>` | | Pixels wide; the height keeps the aspect unless given |
| `--height <HEIGHT>` | | Pixels high |
| `--scale <SCALE>` | `1` | Pixels per user unit when no size is given |
| `--background <BACKGROUND>` | transparent | `#rrggbb`, `#rrggbbaa`, `white`, `black` or `transparent` |
| `--event <EVENTS>` | | An event that happened, `name=seconds`; repeat it for several. What the file plays on it plays from then |
| `-o`, `--output <OUTPUT>` | Next to the file | A PNG file for one frame, or a directory for several |

Without `-o`, one frame goes to `<stem>.png` next to the file and several to a `<stem>-frames/` directory there. An `-o` ending in `.png` is the file for a single frame; any other `-o` is a directory, created when missing. Frames in a directory are named `<stem>-0000.png`, `<stem>-0001.png` and so on. Each line printed is a file and the time it shows.

Times are rendered as given, not wrapped into the loop. `--event` moves what the file plays on an event to that time, as [`Scene::with_events`](./scene-model.md#events) does:

```bash
vector-animation render press.svg --event btn.click=0.5 -t 0.6 -o pressed.png
```

Rendering uses the [reference renderer](./frames-and-rendering.md#the-reference-renderer), so live text is set in the machine's fonts and images over 8192 × 8192 pixels are refused.

## Exit codes

| Code | When |
|---|---|
| 0 | Success |
| 1 | The file could not be read or written, a flag value was invalid (`--fps 0`, `--event bad`, an unknown color), or `validate --strict` found warnings |
| 2 | The command line itself is wrong: an unknown flag, a missing file argument, `--fps` with `--time` |

Errors are printed to stderr as `error: <message>`.

Next: [Compiled scenes](./compiled-scenes.md), or [Untrusted input](./limits.md) for the limits every command reads within.
