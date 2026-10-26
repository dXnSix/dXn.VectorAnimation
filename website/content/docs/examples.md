---
title: Examples
description: Every check scene of the NSH player and a gallery of free animations, live in the browser, and how the examples app is built.
group: Examples
order: 150
---

These are the scenes the player is checked with on every platform, played by its web build. Each one
exercises one part of [What it plays natively](./capabilities.md).

```example
demo: all
height: 760
caption: The check scenes, each in a tile.
```

## The scenes

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

The two examples of [Playback](./playback.md), `playback` and `events`, belong to the examples app itself.

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

The three archives here are dotLottie 2 (`a/<id>.json`), a layout the importer does not read yet: it looks for
dotLottie 1's `animations/<id>.json`, so the tiles stay empty until it does. Each holds the same animation as
its Lottie JSON above.

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

## How the examples are built

The examples are an NSH app in the repository, `examples/docs`, on the player. The site's workflow builds the
dXnSix fork of NSH, writes the scenes and publishes the app for the web into the site:

```bash
cargo run --release -p dxn_vector_animation_check -- scenes examples/docs --gallery fixtures/gallery
cd examples/docs
nsh package "$PWD/App.tsx" --target web --out /tmp/examples   # /tmp/examples/Vector Animation Examples/
cp -R "/tmp/examples/Vector Animation Examples" ../../website/public/examples
```

The app reads which example to show from the page's address, `examples/index.html?demo=<name>`, through NSH's web host
(`host.js`, named by `web.host` in its `nsh.json`); without a name it shows them all. Each page embeds one in an
iframe. The grids (all the scenes, and each group of files) report their height to the page, which sizes the frame
to it: the page scrolls, not the frame, and a grid wraps into more rows on a narrow screen.

:::note Load size
The examples' web runtime holds the engine, QuickJS, the player and the importers: about 5.7 MB gzipped, loaded
once and then cached by the browser for every example on the site.
:::

## Next

[What it plays natively](./capabilities.md) explains what each scene exercises.
