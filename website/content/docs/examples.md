---
title: Examples
description: A gallery of free animations, Rive files played live, and every check scene of the NSH player, in the browser, and how the examples app is built.
group: Examples
order: 150
---

Live examples of the player in the browser: free animations from LottieFiles, played from each of the formats the
player reads, Rive files played as they are (their state machines, listeners, events and view models live), and
below them the scenes the player is checked with on every platform.

## Animations from the wild

Free animations from [LottieFiles](https://lottiefiles.com/), downloaded in each of the three formats the
player reads. Each file is compiled ahead and played as it is; a tile's name is its file's name in
`fixtures/gallery/`, and [`SOURCES.md`](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/fixtures/gallery/SOURCES.md)
there records where they come from.

### Lottie JSON

```example
demo: lottie-files
height: 260
caption: Lottie JSON files, each in a tile.
```

### dotLottie

```example
demo: dotlottie-files
height: 260
caption: dotLottie files (`.lottie` archives), each in a tile.
```

The archives are dotLottie 2 (`a/<id>.json`); two of them carry state machines, which the importer leaves out and
says so. Each holds the same animation as its Lottie JSON above.

### Animated SVG

```example
demo: svg-files
height: 260
caption: Animated SVG files as Figma Motion and LottieFiles write them, each in a tile.
```

| File | Formats | What it shows |
|---|---|---|
| `alien-button` | JSON, dotLottie, SVG | A glowing button with gradients, seven markers |
| `alphabet-a` | JSON, dotLottie, SVG | A letter with moving eyes and gradient shading, three seconds |
| `navigation-bar-selection` | JSON, dotLottie, SVG | A navigation bar's selection moving between four tabs, one marker each |
| `boxer` | JSON, SVG | A character animation from After Effects (Lottie 4.8, 30 frames a second) |

## Rive files

Rive files the player reads as they are, not compiled ahead: their state machine plays as Rive's runtime plays it,
the pointer reaches its listeners, its events come back as cues, and the app sets its inputs and its view model's
values. They are built by the importer's tests (`crates/dxn_vector_animation_rive/tests/docs_examples.rs`, written to
`fixtures/docs/`) and checked against Rive's own runtime frame by frame. See [Rive](./rive.md).

### A switch: a listener and an input

```example
demo: rive-switch
height: 260
caption: A click on the switch toggles its state machine's bool input "on" (a listener); the button sets it with `setInput`, and `onInput` reports each change.
```

### A ball: events

```example
demo: rive-events
height: 220
caption: The ball fires "started", then "left" and "right" as it reaches each side; `onCue` reports them as playback passes them.
```

### A progress bar: data binding

```example
demo: rive-progress
height: 320
caption: The bar's view model sets its width (through an interpolating converter and a range mapper), its label (to text) and its color; `setValue` changes "value" and "tint", `fireValue` fires "celebrate", which its state machine pulses on.
```

| File | What it shows |
|---|---|
| `switch.riv` | A state machine with a bool input, eased transitions and a click listener toggling it |
| `travel.riv` | An animation keying events, and a state firing one as it is entered |
| `progress.riv` | A view model bound through converters (interpolator, range mapper, to-string), a color eased, a trigger a transition waits on |

## The scenes

These are the scenes the player is checked with on every platform, played by its web build. Each one
exercises one part of [What it plays natively](./capabilities.md).

```example
demo: all
height: 760
caption: The check scenes, each in a tile.
```

| Name | What it shows |
|---|---|
| `figma` | A Figma Motion SVG export (CSS `@keyframes`): `fixtures/figma-motion.svg` |
| `lottie` | A Lottie file: `fixtures/lottie-motion.json` |
| `ring` | A ring spinner as svg-spinners draws one: a dash growing while its offset runs on, SMIL throughout |
| `drawn-on` | A path drawn on as Figma Motion exports it: a dash pattern `0 1` to `1 1` along a path of length 1 |
| `trims` | Trims moving at both ends over a circle's start and a square's corner |
| `loader` | A fixed arc carried round more than once by its offset |
| `shine` | A light band running along a gradient, narrowing, on a fill and on its rim (Figma's coin) |
| `sweep` | Gradients whose ends move: a linear one on a square, a radial one whose circle grows |
| `fade` | Gradient colors that change, fading to transparent, on a fill and on a stroke whose gradient moves too |
| `shadow` | Figma's drop shadows: a card's still one, and a disc's whose blur and opacity move |
| `blur` | Layer blurs: a still glow (drawn once) and a disc coming into focus |
| `soft-mask` | A luminance mask of gray and white, and an alpha mask of a half-transparent disc, over moving content |
| `turning-clip` | A clip whose shape turns |
| `turning-mask` | A half-transparent soft mask of one shape, turning |
| `turning-masks` | A soft mask of two overlapping shapes turning apart |
| `moving-shadows` | A shadow falling further as its card rises, one showing through a half-clear disc, one changing hue |
| `card-shadows` | Figma's hard shadows of a card clipping its content, turning and shrinking, and of a half-clear one |
| `mask-images` | Masks drawn as images: a gradient fading content out, six overlapping half-clear discs, a sliding mask |
| `trims-moving` | A star turning and deepening, drawn on and off; a disc's fill trimmed as it runs round |
| `stop-counts` | A gradient gaining and losing stops as they move |
| `dash-slots` | A hundred dashes round a growing circle, their offset running on |
| `switching-star` | A star of five points and six, forty times over, turning |
| `live-text` | A centered title growing and changing color, a line sliding in, a stroked word |
| `inner-shadows` | Figma's inner shadows: a card's, under a drop shadow, and a moving disc's |
| `color-effects` | Shapes turning gray and back, a hue going round |
| `patterns` | A tiling turned in a moving circle, and Figma's image fill |
| `shadow-images` | Shadows of what the canvas cannot cast as shapes: clipped content, and a disc casting its own shadow |
| `moving-layers` | A badge turning and growing along a path, a card fading: layers the platform moves |
| `clear-stops` | A gradient whose middle stop fades in from clear as it moves, then out again |

The two examples of [Playback](./playback.md), `playback` and `events`, and the Rive files' three, `rive-switch`,
`rive-events` and `rive-progress`, belong to the examples app itself.

## How the examples are built

The examples are an NSH app in the repository, `examples/docs`, on the player. The site's workflow builds the
dXnSix fork of NSH, writes the scenes (and the Rive files, as their bytes) and publishes the app for the web into the
site:

```bash
cargo run --release -p dxn_vector_animation_check -- scenes examples/docs --gallery fixtures/gallery --rive fixtures/docs
cd examples/docs
nsh package "$PWD/App.tsx" --target web --out /tmp/examples   # /tmp/examples/Vector Animation Examples/
cp -R "/tmp/examples/Vector Animation Examples" ../../website/public/examples
```

The app reads which example to show from the page's address, `examples/index.html?demo=<name>`, through NSH's web host
(`host.js`, named by `web.host` in its `nsh.json`); without a name it shows them all. Each page embeds one in an
iframe. The grids (all the scenes, and each group of files) report their height to the page, which sizes the frame
to it: the page scrolls, not the frame, and a grid wraps into more rows on a narrow screen.

Every `VectorAnimation` in the app sets `ignoreReduceMotion`, so the examples play for a reader who prefers reduced
motion as well (see [Playback](./playback.md#reduced-motion-and-off-screen)); an app of your own shows the still
frame to that reader unless it sets the prop too.

:::note Load size
The examples' web runtime holds the engine, QuickJS, the player and the importers: about 5.7 MB gzipped, loaded
once and then cached by the browser for every example on the site.
:::

## Next

[What it plays natively](./capabilities.md) explains what each scene exercises.
