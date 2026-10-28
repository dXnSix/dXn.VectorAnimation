---
title: Platforms and device checks
description: Where the NSH player has been checked, how close each platform comes to the reference renderer, and what playing costs.
group: NSH player
order: 130
---

The player runs wherever NSH runs: iOS, Android, macOS (AppKit), Windows, Linux (GTK), GPUI and the web. Each
platform has been checked frame by frame against the reference renderer with the check app,
`examples/check`, which holds a set of scenes at 25, 50 and 75% of their animations and photographs them.

## How close each platform comes

The last run held 45 scenes: the check app's own 29 and 16 files of a corpus of real exports. The numbers
are the share of a tile's pixels more than a quarter of a channel apart from the reference, at the best shift
of one pixel:

| Platform | Pixels a point | Tiles off the reference, at most (live text aside) | Effects and newer scenes | Live text |
|---|---|---|---|---|
| iOS simulator | 3 | 0.14% | 0.12% | 4.3% |
| Android emulator | 2.75 | 1.6% | 0.99% | 6.8% |
| Chrome | 1 | 2.9% | 0.26% | 3.8% |
| Chrome | 2 | 1.25% | 0.22% | 4.2% |
| macOS (AppKit) | 2 | 1.42% | 0.35% | 4.2% |
| GPUI on macOS | 2 | 1.21% | 0.87% | 4.4% |
| GTK on Linux | 1 | 2.75% | 0.20% | 8.8% |
| GPUI on Linux | 1 | 3.6% | 3.6% | 9.2% |
| Windows (CI runner, the 29 own scenes) | 1 | 0.42% | 0.42% | 4.9% |

- At one pixel a point a pixel of anti-aliasing is a larger share: the corpus files of fine detail are the
  ones over 0.5% there.
- Live text is set in the platform's fonts (Roboto, DejaVu Sans, the system's), the reference in the
  Mac's: the glyphs and their widths differ, not where the lines are anchored or how they move.
- GPUI on Linux resamples a layer that moves at a fraction of a pixel, which shifts the edges of small detail.
- Windows runs in CI on every push (`.github/workflows/windows-check.yml`), failing on a missing tile or one
  more than 5% off.

No real phone has run the player yet; the emulator and simulator numbers stand in for them.

## Platform notes

- **The web** replays canvases with Canvas 2D and runs the animation graph on the browser's frames. Its canvas
  takes shadow lengths in its own pixels; the player scales them by the pixel ratio there.
- **Linux** draws canvases with cairo, which cannot place points past about plus or minus 8 million device
  pixels: rectangles that stand for everything reach 50,000 points past the view, no further.
- **NSH fixes.** The player was checked on the dXnSix fork of NSH, which carries fixes found by these checks:
  GPUI canvases that showed old frames under load, and macOS canvas shadows that fell upward. Both are on
  branches ready to be proposed to NSH.

## What playing costs

`vector-animation-check profile <dir>` reports, for each file of a folder, how long its plan takes to compile,
its graph's nodes and breakpoints, what each frame draws and the engine thread's work per frame. On the 53
corpus files at 200 points and 3 pixels a point:

| Measure | Result |
|---|---|
| Plan compile time | 13 ms at most, 50 files under 2 ms |
| Canvases | 201 at most (a repeater of a hundred copies that move, each two layers of its own) |
| Drawn again each frame | 20,328 canvas operations over 41 million pixels in all (99 million before canvases that move were sized to where they draw and moving layers took in more groups) |
| Engine thread work per frame | none, except three files: 0.12 and 0.02 ms for rasters of moving blurs, 0.015 ms for a part sampled frame by frame |

The graph's evaluation on the platforms' UI threads and frame times on real phones are not measured yet.

## Run the checks

```bash
cargo run -p dxn_vector_animation_check -- scenes examples/check --corpus <dir>
cd examples/check && nsh test check.test.tsx --target ios   # or android, web, macos, linux, gpui
cargo run -p dxn_vector_animation_check -- compare examples/check/shots examples/check --out <dir>
```

`compare` finds the tiles by their border, renders each reference at the tile's size and writes each pair side
by side. An `nsh` built from a checkout needs a local `examples/check/.cargo/config.toml` patching the `nsh-*`
crates to that checkout.

## Next

[Examples](./examples.md) shows the check scenes live; [Status and roadmap](./roadmap.md) what is left.
