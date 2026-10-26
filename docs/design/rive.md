# Rive

How dXn.VectorAnimation reads and plays Rive files (`.riv`): what the format holds, how it maps onto the
scene model, what the core, the reference renderer and the NSH player gain for it, and in which order.

## Decision

A new importer, `dxn_vector_animation_rive`, reads `.riv` files into the scene model, as
`dxn_vector_animation_lottie` reads Lottie: pure Rust, no `unsafe`, building for `wasm32-unknown-unknown`,
every count bounded by `Limits`, what it leaves out a warning. Rive does more than SVG and Lottie (state
machines, bones and meshes, constraints, layout, data binding); the scene model, the reference renderer and
the player grow to hold it, as the phases below say.

[rive-rust](https://github.com/Bantarus/rive-rust) is not a dependency. It binds Rive's C++ runtime
(`rive-runtime`, pinned at `runtime-v0.1.106`) and draws with Rive's own GPU renderer on Vulkan into a texture
for Bevy: it hands out pixels, never shapes or keyframes. It builds with clang, premake5, glslang and the
Vulkan SDK on Linux and Windows only (Metal is planned, Android untried, the web absent), is version 0.0.0
and unpublished, and has 187 `unsafe` blocks in its safe wrapper. It would bypass the scene model (the CLI,
the reference renderer, the compiled format, the player's graph) and could not build where the core must.

What is taken from it is where it points: `rive-runtime` (MIT) is the format's definition. Its `dev/defs/`
(353 JSON definitions: type keys, property keys, field types, defaults, what animates) generates the
importer's tables, and its C++ is the reference for what each object does. The importer follows one tag of
it at a time (`runtime-v0.1.106` first), named in the generated file.

## The format

- A header: the bytes `RIVE`, varuints major version (7), minor version and file id, then the table of
  contents: property keys up to a 0, and two bits for each (0 uint or bool, 1 string or bytes, 2 a 32-bit
  float, 3 a 32-bit color), packed into little-endian 32-bit words, four keys a word (in its lowest
  byte), as rive-runtime reads them.
- Then objects to the end: a varuint type key and `(property key, value)` pairs up to a 0. A uint is a
  varuint (LEB128), a bool a byte, a double a 32-bit float, a color a 32-bit ARGB word, a string or bytes a
  varuint length and the bytes. A property the importer does not know is skipped by its type in the table of
  contents; one in neither is malformed.
- Objects build the file in the order they come, as the runtime's import stack does:
  - The backboard, then each artboard. Components after an artboard (and keyframe interpolators) are its
    objects: index 0 is the artboard, and `parentId`, `objectId`, `interpolatorId`, `sourceId`,
    `targetId` and the like are indices into that list.
  - An object of a type the runtime does not know (newer than the pinned tag, or editor-only like
    `ForcedEdge`) is a null object. It takes a slot of the list most recently opened among an artboard's
    objects, a keyed property's keyframes and a state machine's inputs or layers, so later indices stay right.
  - `LinearAnimation` → `KeyedObject` (`objectId`) → `KeyedProperty` (`propertyKey`) → keyframes (`frame`,
    `interpolationType`, `interpolatorId`, a value), in that nesting.
  - `StateMachine` → inputs, layers (→ states → transitions → conditions), listeners (→ actions).
  - An asset (`ImageAsset`, `FontAsset`, `AudioAsset`, ...) is followed by its `FileAssetContents` when it is
    embedded; otherwise it is out of band (a CDN id or a name the app supplies).
- A property an object never writes has its definition's initial value (`initialValueRuntime` when there is
  one).

## What the sample files use

`examples/gallery/animations-rive/` holds twelve community files (formats 7.0 to 7.3). Every one parses to its
end with the pinned tables. What they use, by how many of the twelve:

| Files | Features |
|---|---|
| 12 | artboards, nodes, shapes, points paths, fills, strokes, solid colors, linear animations with cubic easing, state machines (layers, animation states, transitions), layout styles |
| 10–11 | ellipses, rectangles, straight and cubic detached vertices, clipping shapes, gradients, feather, nested artboards with their state machines, listeners, embedded assets |
| 6–9 | images, text (with embedded fonts), mirrored and asymmetric vertices, color and bool keyframes, bones and skins, solo, draw rules, `KeyFrameId`, view models, translation constraints, triggers |
| 2–5 | meshes, trim paths, elastic and value interpolators, follow-path, IK, transform, rotation and scale constraints, number inputs, data binding with converters, blend states, events, joysticks, layout components, stars |
| 1 | scripting (Luau scripted drawables), scrolling, audio, open-URL events, dashes, lists, custom properties |

So static shapes and linear animations (phase 1) draw part of each file, and the files play as their authors
meant only with state machines (phase 3); nested artboards, feather, text and bones (phase 2) come up in most.

## The scene from an artboard

| Rive | The scene |
|---|---|
| The file's first artboard, or one named | `Scene`: its width and height; the root group translated by the origin (`originX × width`, `originY × height`), clipped to the artboard when `clip` is set; its fills and strokes a background shape |
| `Node`, `Shape`, `Image`... transforms: `x`, `y`, `rotation` (radians), `scaleX`, `scaleY`; world = parent × translate · rotate · scale | Transform ops `[Translate, Rotate (degrees), Scale]` for each, each value a constant or a track of its own |
| Draw order: the artboard's drawables in the order of its list (draw rules move them), not the tree's | One group per drawable in draw order, its transform its ancestors' operations then its own (outermost first) |
| `opacity`: multiplies down the tree into each fill and stroke (not a layer) | The product of the chain's opacities, in each fill's and stroke's `opacity` (a track of the product: exact where one factor moves, sampled where several do) |
| `PointsPath` with straight (corner `radius`), mirrored, asymmetric and detached vertices (tangents as angle and distance) | A `Path`. Each vertex value is a track, so a moving outline is path keyframes at the union of its vertices' keyframes, sampled at the animation's frame rate where tangents turn or corners round |
| `Rectangle` (`width`, `height`, `originX`/`Y`, four corner radii), `Ellipse` | `Rect` and `Ellipse` (a path when the corners differ) |
| `Polygon`, `Star`, `Triangle` (corner radius) | A path |
| `Fill`, `Stroke` (`thickness`, cap, join), `SolidColor`, `LinearGradient`, `RadialGradient` (center at the start, radius to the end), `GradientStop`, fill rule | `Fill`, `Stroke`, `Paint` |
| `TrimPath` (`start`, `end`, `offset`; sequential, synchronized) | `Trim` (`Combined`, `EachPath`) |
| `DashPath`, `Dash` (lengths, or percentages of the path) | `Dash` |
| `ClippingShape` (a source node's shapes clip the shape's parent's descendants; fill rule; `isVisible`) | `Group.clip`, its shapes under the source's world transform |
| `blendModeValue` | `BlendMode` |
| `Image` and its embedded `ImageAsset` | `Image`, its rectangle the image's size about its origin |
| Hidden drawables (`drawableFlags`), `Solo` (`activeComponentId`) | Opacity held at 0 where hidden, or where a solo shows another child |
| The clockwise fill rule | Points paths drawn counter-clockwise turned round and holes the other way (Rive's `localClockwise` path), filled non-zero: the same unless holes reach past the outlines |

Colors are ARGB words, which Rive mixes channel by channel with straight alpha. The scene mixes them
premultiplied where alphas differ, so a color whose alpha and color both change is sampled.

## Animations

Each `LinearAnimation` of the artboard is a segment of one timeline, in the file's order, under a marker of its
name: its `duration` in frames over `fps`, played at its `speed` (negative plays it backwards), within its work
area when `enableWorkArea` is set. A property it keys plays its keyframes there; one it does not key holds the
artboard's setup value, as Rive plays an animation on a fresh artboard. Each marker carries the animation's
loop (one shot, loop, ping-pong), and the timeline's own loop is the default animation's. The default
animation is the one the file's first state machine enters first, else the first.

Keyframes: a frame number over the animation's frame rate; hold, linear and cubic interpolation. A
`CubicEaseInterpolator` is `Easing::CubicBezier`. A `CubicValueInterpolator` is a curve in the values'
space: normalized by the segment's change it is a `CubicBezier` too, and where the change is 0 it is sampled.
An `ElasticInterpolator` is sampled into keyframes at the animation's frame rate, straight between frames,
until the core has an elastic easing. `KeyFrameId` (draw targets, solos) and `KeyFrameBool` hold.

## Beyond the scene model: what the core grows

The core gains what Rive needs, each with the reference renderer's drawing and the player's mapping:

1. **Markers with a loop** (phase 1): `Marker::loop_mode`, used when a segment plays it.
2. **Done: runtime-evaluated motion, baked**: bones and skins (5), joysticks (6) and constraints. A rig
   (`rig.rs`) holds each transform component's own transform and its constraints, sorted into Rive's update
   order (parents first, targets before what they constrain, an IK chain's other children after its tip;
   a cycle in the file's order). The world of a component a constraint moves (or an IK chain bone) is
   evaluated over what it depends on, ports of `TranslationConstraint`, `RotationConstraint`,
   `ScaleConstraint`, `TransformConstraint` (target bounds taken as empty, as for all but text and
   layout), `DistanceConstraint`, `IKConstraint` (one, two and longer chains, inverted) and
   `FollowPathConstraint` (on the target's outlines, wrapped as `PathMeasure::atPercentage` wraps), with
   `Mat2D::decompose` and `compose` as Rive has them; the result in its parent's world is baked into a
   matrix (`TransformOp::Matrix`) at 60 frames a second where it moves. Checked against Rive's runtime:
   the test files draw the same (the follow-path ring within 0.1% of pixels); the snake from 3.9% of
   pixels to 0.1%, Sobo 3.2% to 0.6%, Dress Up 2.1% to 0.5%, the weather card 0.7% to 0.03%.
   **Done: draw rules** (`order.rs`): the draw targets sorted as `DependencySorter` sorts them and each
   rule's drawables spliced before or after its target's as `Artboard::sortDrawOrder` does; where a
   rule's active target or a placement is keyed, the distinct orders are merged into one sequence (their
   longest common subsequence kept once) and each copy of a moved drawable is shown while its order holds.
   **Done: clips as Rive clips.** `Clip::one_path` (the core) makes a clip of the source's outlines as one
   path under the clip's rule without baking them together, and a layer that only clips does not set what
   it holds apart (the reference renderer draws it on a copy of what is under it), as the player's Canvas
   2D clips never did: Rive's blend modes under a clipping shape mix with the artboard beneath. Drawables
   one after another under the same clipping shapes share one clip, as Rive interleaves its clip
   operations. An inner feather is the rectangle with the moved outline cut out (destination-out), as
   Rive's clockwise rule fills it: overlapping outlines are not shaded. Sobo now within 0.01% of pixels.
   **Baking**: consecutive sampled spans are simplified together, a sample dropped where a straight line
   between its neighbours draws the same (Studiorun 946k keyframes to 652k).
   **Done: text modifiers** (`modifiers.rs`): ports of `TextModifierRange` (its `RangeMapper` by
   characters, characters without spaces, words and lines over Rive's `GlyphLookup`, coverage with
   falloffs, the cubic interpolator, modes and clamping) and `TextModifierGroup` (translation, rotation,
   scale about the origin, opacity, inverted too). A modified text is drawn glyph by glyph, each glyph's
   outline baked where its place moves and its opacity a group's. Checked against Rive's runtime: the
   test files within 0.04% of pixels. Variation (shape) and follow-path modifiers are left with a warning.
3. **Done: nested artboards.** The artboard a nested artboard shows is converted on its own and drawn at
   its transform about its origin (Rive's `frameOrigin` off), clipped to it. Its tracks are built on the
   parent's clock: each nested animation is a layer of pieces of the parent's spans mapping parent
   seconds linearly to the animation's own (wrapping at its loop, held while `isPlaying` is off, at the
   remapped fraction for a remap animation), and each property takes the last layer keying it, as Rive
   applies nested animations in order. Keyframes inside a piece carry over as they are; a cut cubic easing
   is split into a cubic easing exactly (de Casteljau). When every layer plays alike in each of the
   parent's spans, the nested keys cover the longest span once and a time remap restarts them at each
   span. A nested state machine plays its entry animation (its other states in phase 3). An artboard
   without animations playing a state machine takes the nested animations' span as its timeline; with
   nothing playing, Rive advances nothing, and neither does the scene. Checked against Rive's runtime
   played frame by frame: the test files match exactly, Dress Up's owl and bubbles within its bones.
   Known cost: a remap animation whose keyed time is eased is sampled at 60 frames a second (exact
   composition is left to do).
4. **Done: feather.** Rive's renderer feathers by a Gaussian whose deviation is half the strength (the
   strength is two deviations), in the shape's lengths, and draws feathered fills only under the clockwise
   rule (as the web canvas runtime does, which feathers through the same renderer). An outer feather is the
   paint in a group blurred by the scene's `Blur`, moved by its offset in the shape's space or, by default,
   the artboard's (a group of its own under the offset, then the shape's transform). An inner feather fills
   a rectangle past the outline (three deviations and the offset) with the outline moved by the offset cut
   out, blurred and clipped to the outline. Checked against Rive's runtime: the test files draw the same
   (no pixel off by more than 8 of 255), and the samples' glows and glass panels too.
5. **Done: bones, skins and image meshes**, with nothing new in the core. A bone is a transform at its
   parent bone's tip (a root bone where it says); a skinned outline is built from its vertices deformed
   into the artboard's space (`Weight::deform`: the weighted sum of each bone's world transform times its
   inverse bind, applied to the vertex in the skin's bind transform; handles by a cubic weight's own) and
   moved back into its shape's space, sampled as the bones move. An image mesh is a group per triangle
   whose matrix takes the image's triangle (its uv in pixels) onto the mesh's, clipped to the image's
   triangle grown half a pixel so neighbors meet without a seam; skinned meshes are drawn in the
   artboard's space. Checked against Rive's runtime: the test files within 0.1% of pixels; the snake
   takes its pose. The reference renderer now draws images with an embedded color profile (the snake's
   Display P3 WebPs) in sRGB, as the browser does.
6. **Done: text** (modifiers and ellipses to come). Shaped at import with the file's embedded fonts
   (rustybuzz, as Rive shapes with HarfBuzz; variation axes applied) into glyph outlines, laid out by ports
   of rive-runtime's word breaks (`renderer.cpp`), line breaker (`line_breaker.cpp`), line spacing and
   `Text::buildRenderStyles` (sizing, origins, alignment, overflow and fit). Text keyed over time is laid
   out at each change and each layout shown while it holds. Checked against Rive's runtime with the KaTeX
   font (`fixtures/fonts`, MIT) and the weather sample's variable font: within 0.04% of pixels.
   **Done: joysticks**, a top layer applying their x and y animations at the fraction their values (or
   their handle's position in their bounds) say.
7. **Elastic easing** (phase 2): `Easing::Elastic`. **Done: strokes that do not scale** need nothing new:
   Rive draws them on the outline moved into the artboard's space, untransformed, and so does the scene (the
   shape outside its drawable's transform, the transform on its paths, which move points and not strokes;
   gradients moved into the artboard's space). Checked against Rive's runtime.
8. **Done: the default state machine as it plays untouched** (`machine.rs`): a port of
   `StateMachineLayerInstance` (any-state and state transitions, conditions on the inputs' values, exit
   times, mixes with their easings, spilled time) and of `LinearAnimationInstance::advance` (one-shot,
   loop, ping-pong), stepped a frame each 1/60 s in 32-bit floats as the runtime keeps time, until every
   layer settles (into a still pose or loops, whose common length repeats from there, nested artboards'
   loops too). Each property then takes, frame by frame, what the last layer applying it applies: a state
   that plays steadily as its animation's own keys (pieces between loop ends), a mix sampled per frame from
   the value the property had when the transition began (Rive's `AnimationReset`); where none applies it
   keeps its last value. The machine is a marker at 0 and the timeline's span. Checked against Rive's
   runtime played frame by frame: the test files draw the same; Studiorun, Sobo, the snake and the weather
   card within 0.3% of pixels. **Done: nested state machines** the same way, in each of the parent's spans,
   their inputs (`NestedBool`, `NestedNumber`) read from the parent's keys frame by frame; spans holding the
   same inputs share one run behind a time remap, and an artboard laid out over many spans plays its machine
   in the parent's state machine alone (Studiorun stays at 686k keyframes). **Done: the interactive
   machine**, kept in the importer, which bakes at Rive's raw properties: `Options::inputs` changes inputs
   over time and the machine is played again with them (each at the first frame at or after its time);
   `Interaction` hit-tests the listeners' shapes (`Scene::hit` in the core, outlines painted or not) and
   turns pointer events into input changes and nodes moved to the pointer (`Options::aligned`), reading
   the file again. Presses and releases act a frame sooner, as Rive's web runtime advances by nothing
   after them. Checked against Rive's runtime driven by mouse events: the test files draw the same, the
   weather card, tabbar and posture samples within their plain playback's differences. **Still to
   come**: blend states, Rive events, listeners on nested artboards and drags.
9. **View models and data binding** (phase 4): view models, instances and bindings to properties and
   conditions, with converters; layout components laid out at import for the artboard's size.
10. **Out of scope for now:** Luau scripting, audio, open-URL events (reported to the app as events), text
    input, scrolling.

## The player and the CLI

- Sniffing: data starting with `RIVE` is a Rive file (the CLI's commands, the player's `src`).
- Phase 1: `segment="idle"` plays an animation; `onLoad`'s markers list them.
- Phase 3 (done): the ref's `setInput(name, value)` and `fire(name)`, the event `onInput`, `onLoad`'s
  `inputs`; the pointer over the view (touches, presses, the mouse hovering) acts on the listeners through
  `Interaction`, each change read off the engine thread and swapped in where playback is. The CLI renders
  with inputs and pointers (`--input hover=true@0.5`, `--pointer down:10,20@1`) and `inspect` lists them.
  Still to come: `onStateChange` and `onEvent`.

## Testing

- `crates/dxn_vector_animation_rive/tests/`: one file per feature (`format`, `shapes`, `paths`,
  `animations`, ...) on files the tests write byte by byte, and the samples parsed to their ends.
- Against Rive: the official web runtime (`@rive-app/canvas`) in headless Chrome gives points of color off
  frames, as `browser_gallery.rs` reads Chromium's.
- Fixtures need their licences (community files are CC BY) in a `SOURCES.md`.

## Phases

1. **Done: the importer, static content and linear animations:** the crate, its generated tables, the
   reader, artboards, shapes, paths, paints, clips, solos, images, animations as markers,
   `Marker::loop_mode`, the CLI (`--artboard`, `--marker`) and the player reading `.riv`, the docs' Rive
   page. The twelve sample files read, and the player's plans of each draw as the reference does.
2. **What the runtime evaluates:** feather, strokes that do not scale, nested artboards, text, bones and
   meshes, constraints, draw rules, text modifiers (done; variation and follow-path modifiers and
   ellipses left), elastic easing.
3. **State machines**: done as they play untouched, with their inputs changed and their listeners acting on
   a pointer in the core, the CLI and the player; blend states and events to come.
4. **Data binding and layout.**
