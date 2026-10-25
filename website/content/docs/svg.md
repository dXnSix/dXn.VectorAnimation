---
title: SVG and Figma Motion
description: What dxn_vector_animation_svg reads - the static document and its style sheets, CSS animations as Figma Motion writes them, SMIL with begins, ends and events, filters, text and patterns - and what it leaves out.
group: Inputs
order: 60
---

`dxn_vector_animation_svg` reads an SVG document and its animations into a scene: the drawing, its CSS animations (the subset Figma Motion writes) and SMIL. You reach for it for Figma Motion's animated SVG exports, and for SMIL icons and spinners.

```rust
let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40">
  <style>
    @keyframes pulse { 0% { opacity: 1; } 50% { opacity: 0.3; } 100% { opacity: 1; } }
    #dot { animation: pulse 1.2s ease-in-out infinite; }
    rect:hover { fill: red; }
  </style>
  <circle id="dot" cx="20" cy="20" r="10" fill="#0a84ff"/>
  <rect x="50" y="10" width="40" height="20" fill="gray"/>
  <foreignObject width="10" height="10"/>
</svg>"##;
let import = dxn_vector_animation_svg::read(svg)?;
println!("{} s, {:?}", import.scene.timeline.duration, import.scene.timeline.loop_mode);
for warning in &import.warnings {
    println!("left out: {warning}");
}
```

```text
1.2 s, Loop
left out: <foreignObject> is left out
left out: CSS `:hover` rules apply on interaction, which the scene does not have: it is drawn at rest
```

| Function | Does |
|---|---|
| `read(data)` | Reads with the default `Limits` |
| `read_with(data, &limits)` | Reads within your limits; a document of more than 8 × `max_nodes` XML nodes is refused |
| `sniff(data)` | Whether the first 4 KiB, past a byte order mark, XML declarations, comments and a doctype, start with `<svg` |

The document must be UTF-8 (`Error::Invalid` otherwise). A compressed `.svgz` is `Error::Unsupported`: unpack it first.

## The timeline

There is no timeline element in SVG, so the importer makes one that every animation fits in:

- The duration is the longest animation: its delay plus its duration times its iterations. SMIL begins and ends count, chained ones included.
- The loop mode is `Loop` when any animation repeats forever (`infinite`, `repeatCount="indefinite"`), `Once` otherwise. SMIL animations that start one another in a cycle make the cycle the loop.
- The loop start is where the loop starts over: what comes before it plays once (see [the loop](#the-loop)).
- There is no frame rate, and the only markers are those of [events](#events).

### The loop

A browser plays an animation that repeats forever on its own clock: its delay once, then its iterations. The timeline plays what comes before the loop once and then holds them all in one loop, long enough for every one of them to come round together, so each plays whole iterations as it does in a browser:

- An animation's period is one iteration, or two for `alternate` and `alternate-reverse`, which play forwards and then backwards.
- The loop starts (`Timeline::loop_start`) where the last of them has started (its positive delay or SMIL `begin`) and every animation that ends has ended. What comes before plays once: a staggered loader's delays, an intro.
- The loop is the least common multiple of the periods; the timeline ends one loop after the loop start.
- A negative delay starts part way through, as in a browser.

| Animations that repeat forever | Loop start | Timeline |
|---|---|---|
| 1 s and 1.5 s | 0 | 3 s (three iterations and two) |
| 1 s `alternate` | 0 | 2 s (there and back) |
| 1 s `alternate` and 1.5 s | 0 | 6 s |
| 1 s, next to one of 2.5 s that plays once | 2.5 s | 3.5 s |
| 2 s after a 0.5 s delay | 0.5 s | 2.5 s |
| Three of 1 s, 0.2 s apart (a staggered loader) | 0.4 s | 1.4 s |

Without delays or animations that end, the loop start is 0 and the whole timeline loops.

Figma Motion gives every animation of an export the same duration, so its loop is one iteration. The loop stays within `max_duration` (an hour by default, see [Untrusted input](./limits.md)) and 10,000 iterations of any one animation. Periods that come round together only later (1 s and 1.0001 s, after 10,001 s) keep a timeline as long as the longest of them, looping from 0, with a warning that the others are cut short.

## Figma Motion's CSS animations

Figma Motion writes each layer's animation as `@keyframes` in a `<style>` block, one per property, with a timing function per stop:

```example
demo: figma
height: 240
caption: fixtures/figma-motion.svg, Figma Motion's CSS animations, played by the NSH player.
```

```css
#Button { animation: kf_Button_transform_0 2s linear infinite, kf_Button_opacity_0 2s linear infinite; transform-origin: 0 0; transform-box: view-box; }
@keyframes kf_Button_opacity_0 {
  0% { opacity: 0; animation-timing-function: cubic-bezier(0.5, 0, 0.5, 1); }
  50% { opacity: 1; animation-timing-function: linear; }
  100% { opacity: 1; }
}
#One_tile { offset-path: path('M0 50 C 50 0 150 100 200 50'); offset-rotate: 0deg; offset-anchor: 0px 0px; animation: kf_One_tile_offset_distance_0 2s linear infinite; }
```

What the importer reads of CSS animations:

| Part | Read |
|---|---|
| Declarations | The `animation` shorthand and `animation-name`, `-duration`, `-delay`, `-iteration-count`, `-direction`, `-fill-mode`; `animation-timing-function` per keyframe stop |
| Timing functions | `linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`, `cubic-bezier()`, `steps()`, `linear()` |
| Animated properties | `transform`, `translate`, `rotate`, `scale`, `opacity`, `visibility`, `fill`, `stroke`, `fill-opacity`, `stroke-opacity`, `stroke-width`, `stroke-dasharray`, `stroke-dashoffset`, `d`, `filter`, `offset-distance`, `font-size`, `letter-spacing`, and the geometry attributes (`x`, `y`, `width`, `height`, `rx`, `ry`, `cx`, `cy`, `r`, `x1`, `y1`, `x2`, `y2`, `points`) |
| Placement | `transform-origin`, `transform-box`, motion along `offset-path: path()` with `offset-distance`, `offset-rotate`, `offset-anchor` |

Keyframe stops that leave a property out take its static value, delays and fill modes hold values as CSS does, and iterations and alternating directions are unrolled into keyframes on the timeline. Transform lists animate function by function where the keyframes' functions match, and as matrices where they differ.

### The static document

- Elements: `svg` (nested ones too), `g`, `a`, `switch`, `symbol`, `use`, `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`, `path`, `image` (embedded PNG and JPEG `data:` URIs), `text`.
- Paint: fills and strokes (dashes and `pathLength` included), linear and radial gradients, patterns, opacity, `mix-blend-mode`, clip paths, alpha and luminance masks, `display` and `visibility`, `paint-order`.
- Style sheets with the cascade and CSS specificity: type, id, class and attribute selectors (`[a]`, `=`, `~=`, `|=`, `^=`, `$=`, `*=`), the pseudo-classes `:first-child`, `:last-child`, `:only-child`, `:nth-child()`, `:nth-last-child()` and their `-of-type` forms, `:empty`, `:root`, `:any-link`, `:not()`, `:is()` and `:where()`, joined by descendant, child, adjacent and sibling combinators.
- Pseudo-elements draw nothing in SVG. Rules for interaction states (`:hover`, `:active`, `:focus`, ...) are drawn at rest with a warning. At-rules other than `@keyframes` (`@media` included) and `@import` are left out with a warning.

## SMIL

Figma Motion writes SMIL for what CSS cannot animate (`width`, `height`, path `d`); icon sets use it throughout.

```example
demo: ring
height: 200
caption: An SMIL spinner - stroke-dasharray, stroke-dashoffset and a rotating animateTransform.
```

| Part | Read |
|---|---|
| Elements | `<animate>`, `<set>`, `<animateColor>`, `<animateTransform>` (`translate`, `scale`, `rotate`, `skewX`, `skewY`; additive or replacing the attribute), `<animateMotion>` (a `path`, an `<mpath>` or `values` points; `rotate` as an angle, `auto` or `auto-reverse`; `keyPoints`) |
| Values | `values`, `from` / `to` / `by`, `keyTimes`, `keySplines`, `calcMode` (`discrete`, `linear`, `paced`, `spline`) |
| Timing | `dur`, `repeatCount`, `repeatDur`, `fill="freeze"`, `accumulate="sum"`, `additive="sum"` |
| `begin` | Offsets (negative ones start part way), other animations' begins, ends and repeats (`a.end + 1s`, `a.repeat(2)`), and events (`click`, `btn.mouseover + 0.5s`); `indefinite` never begins |
| `end` | Offsets and other animations' begins and ends cut instances short |

Several animations of one attribute apply in the order they began, and CSS wins over SMIL on the same property. At most 4,000 animation instances on the timeline and 4,000 on events are resolved (a cycle of begins cannot use up what clicks need), and 10,000 iterations of one are unrolled; a file that asks for more gets a warning saying what is left out. SMIL animations that repeat forever share [the loop](#the-loop) with CSS ones.

### Events

What begins on an event plays in a segment of its own past the timeline's end, under a marker named `on:<element>.<event>` (`on:<event>` when the animated element has no id). The timeline never reaches it until the event is given a time:

```rust
let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="60">
  <rect id="btn" width="120" height="60" fill="#2563eb">
    <animate attributeName="opacity" values="1;0.4;1" dur="0.3s" begin="click"/>
  </rect>
</svg>"##;
let scene = dxn_vector_animation_svg::read(svg)?.scene;
println!("{:?}", scene.events()); // ["btn.click"]
let clicked = scene.with_events(&[("btn.click", 1.5)]); // clicked 1.5 s in
```

`vector-animation render --event btn.click=1.5` does the same from the CLI, and the NSH player's `trigger(event)` plays the segment when the event happens. [The scene model](./scene-model.md#events) samples the result. An `end` on an event never happens here; it is left out with a warning.

## Filters

Filters become group [effects](./scene-model.md#effects-clips-and-masks), each value animatable:

- `<filter>` chains as Figma exports drop shadows (several of them), inner shadows and layer blurs: the source's alpha hardened, moved, blurred, colored, cut out and blended under the source, or taken from it and blended over it.
- `feDropShadow`, `feGaussianBlur`, and color changes of the source: `feColorMatrix` of every type and `feComponentTransfer`, in linear light unless `color-interpolation-filters` says `sRGB`.
- CSS `filter` of `blur()`, `drop-shadow()`, `grayscale()`, `sepia()`, `saturate()`, `hue-rotate()`, `invert()`, `opacity()`, `brightness()`, `contrast()` and `url()`, still or animated by `@keyframes`.

A filter holding anything else is left out whole, with a warning naming what it holds: ``filter `#glow` is left out: it holds <feMorphology>``.

## Text and patterns

- `<text>`, `<tspan>` and `<a>` become text spans set in a font when drawn, placed by `x`, `y`, `dx`, `dy` and `text-anchor`, with `font-family`, `font-size`, `font-weight`, `font-style`, `letter-spacing`, fills and strokes. Whitespace collapses unless `xml:space="preserve"`.
- `<pattern>` fills tile in user space or the shape's box, their content in user space, the box's fractions or a view box, with `patternTransform`; attributes and content are inherited through `href`. Figma's image fills are such patterns.

## What is left out

Each of these reads, with a warning in `Import::warnings`:

| In the file | What happens |
|---|---|
| A position for each character (`x="1 2 3"`) | The first places the text |
| `<textPath>` | Laid out on a line |
| Floods, morphology, lighting, displacement, shadows of a recolored drawing | The filter is left out |
| `<foreignObject>`, `<text>` in a clip path | Left out |
| An `<image>` that is not an embedded PNG or JPEG | Left out |
| `offset-path` other than `path()`, `clip-path` and `mask` other than `url()` | Left out |
| Animation of a property the scene does not animate | That animation is left out |
| More than 10,000 iterations of one animation, more than 4,000 SMIL instances on the timeline or on events | The rest is left out |
| Animations repeating forever that come round together only past the limits | The timeline is as long as the longest and loops from 0; the others are cut short |
| A `<use>` of a missing element or of itself | Left out |
| Rules for interaction states, unsupported selectors, at-rules other than `@keyframes` | The rule does not apply |
| SMIL `end` on an event | Never happens |

Next: [Lottie and dotLottie](./lottie.md), or [Untrusted input](./limits.md) for what bounds reading a file.
