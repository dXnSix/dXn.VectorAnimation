# Gallery files

Free animations downloaded from [LottieFiles](https://lottiefiles.com/) (the web app,
https://app.lottiefiles.com/) in October 2026, each in the formats it offers: Lottie JSON, dotLottie and
animated SVG. They are used under LottieFiles' terms for free animations (the Lottie Simple License); their
authors are not recorded in the downloads. Shown on the documentation site's Examples page and read by the
importers' `gallery` tests.

| Files | Made with (the files' own generator tag) |
|---|---|
| `alien-button.*` | LottieFiles Figma plugin; the `.lottie` is written by `@dotlottie/dotlottie-js` 1.6.3 |
| `alphabet-a.*` | `@lottiefiles/creator` 1.103.0 |
| `navigation-bar-selection.*` | `@lottiefiles/creator` 1.108.2 |
| `boxer.json`, `boxer.svg` | LottieFiles After Effects extension 3.6.0 (Lottie 4.8) |

The files were added on the repository owner's own responsibility; LottieFiles' terms for free animations are what they were
downloaded under.

## Edits to the SVG files

Two of the downloaded SVGs did not draw what their Lottie JSON draws, so `alien-button.svg` and
`navigation-bar-selection.svg` were edited by hand (plain SVG and SMIL as before, nothing the importers had to be
taught) until a browser, the reference renderer and the JSON agree. The numbers the JSON gives are mapped as the Lottie
importer maps them: a drop shadow's softness is four deviations and its direction is clockwise from up, a Gaussian
blur's blurriness is 0.3 of one deviation. `alphabet-a.svg` and `boxer.svg` are as downloaded; they differ from their JSON
by the edge of a pixel while a shape moves fastest, no more.

- `alien-button.svg`
  - The "o" of "Connect": the letter's two contours, which the export wrote as two white paths (so the inner one drew
    a dot), are one path, as the JSON's compound path is (the contours wind the opposite way, so the inner one is a
    hole under the default `nonzero` rule).
  - The glow under the button: the button's back had a small black drop shadow (`dx` = `dy` = 1.768, deviation 0.333);
    it has the JSON's, purple `#550cdb`, opacity 1, distance 35.61 at 128.16 degrees (`dx` 28.0, `dy` 22.0) and softness
    101.41 (deviation 25.353). The button outline's blur is 15.459 x 0.3 = 4.638 (it was 2.5).
  - The alien's highlights had one blur of 2.5 between them, in six filters sharing one `id` (a browser uses the first
    for all); each has its own, 3.308, 6.616 or 8.821 (the JSON's 11.03, 22.05 and 29.40 x 0.3).
  - The UFO's fifteen radial gradients were frozen at their first frame; the JSON moves their center and radius with
    the UFO, so each has `animate` on `cx`, `cy` and `r` (from the JSON's start and end points, on the file's timing
    and easing) and no `fx`, `fy`.
- `navigation-bar-selection.svg`: a bar's colored background is a rectangle that grows from its left edge. The file
  animated `x` as if it were the rectangle's center (so the fill sat half its width off its label) on the Chat,
  Articles and Communities bars, which keep their `x` now, and moved the Question bar's group while its rectangle
  grew from `x = 0`, where the JSON grows it about its center; its `x` runs from 0 to minus half the width, with the
  width's keys.

Checked in headless Chromium (SMIL paused and set to the time) against `vector-animation render` of the JSON, at
many times over two loops: the mean difference per pixel went from 1.85 to 0.25 for `alien-button` and from 3.95 to 0.34
for `navigation-bar-selection`, which is what Chromium differs from the reference renderer on the same file by (text
and edges). `crates/dxn_vector_animation_render/tests/browser_gallery.rs` pins this.

A fifth download, "Rolling Cubes" (a 12.7 MB SVG and a 770 KB JSON with 145,000 keyframes; both read, the SVG with
SMIL on `d`, `display` and `transform` left out), is not in the repository: it is too large for the examples' web
build, which loads every scene, and its files were no longer on the desk when it was to be committed.

The file names are the downloads' names in lowercase with dashes.
