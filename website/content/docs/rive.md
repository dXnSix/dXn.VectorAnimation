---
title: Rive
description: What dxn_vector_animation_rive reads of a Rive file (.riv) - an artboard's shapes, paints, clips, solos and images, its linear animations as markers - and what it leaves out for now.
group: Inputs
order: 75
---

`dxn_vector_animation_rive` reads Rive files (`.riv`, the format the Rive editor exports, version 7) into a
scene. It is pure Rust, builds for the web like the other importers, and reads the file's objects with the
types and properties of one tag of Rive's own runtime (`RUNTIME_TAG`), put together as that runtime puts them.

```rust
let data = std::fs::read("icons.riv")?;
for name in dxn_vector_animation_rive::artboards(&data)? {
    println!("artboard {name}");
}
let import = dxn_vector_animation_rive::read(&data)?; // the first artboard
for marker in &import.scene.timeline.markers {
    println!("{} at {} s for {} s, {:?}", marker.name, marker.start, marker.duration, marker.loop_mode);
}
```

`read_options(data, &limits, &Options { artboard: Some("Menu".into()) })` reads another artboard.

## An artboard as a scene

| Rive | The scene |
|---|---|
| The artboard: its size, origin and clip, its own fills | The scene's size; the root group clipped to it, its content moved by the origin; a background shape |
| Drawables in Rive's draw order (the first in the file on top) | One group each, in that order, under its ancestors' transforms: translation, rotation, scale |
| Opacity, multiplied down the tree into each fill and stroke | Each fill's and stroke's `opacity` |
| Points paths: straight vertices rounding their corners, mirrored, asymmetric and detached cubic vertices | Paths, built as Rive builds them |
| Rectangles, ellipses, polygons, stars, triangles | `Rect` and `Ellipse` where they draw the same, paths otherwise |
| Fills and strokes: solid colors, linear and radial gradients, caps, joins, fill rules, blend modes | `Fill`, `Stroke`, `Paint`, `BlendMode`; the clockwise fill rule with the outlines turned as Rive turns them |
| Strokes the transform does not scale (`transformAffectsStroke` off) | The outline moved into the artboard's space, stroked there: the width and the gradient are the artboard's |
| Trim paths (sequential, synchronized) and dashes | `Trim` (combined, each path) and `Dash` |
| Feathered fills and strokes, offset in the artboard's or the shape's space | A group of the paint blurred by a Gaussian of half the strength, as Rive's renderer feathers; an inner feather is the area around the outline blurred and clipped to it. Rive draws feathered fills only under the clockwise rule, and so does the scene |
| Clipping shapes | Clips of the source's shapes |
| Solos and hidden drawables | Opacity held at 0 where a solo shows another child or a drawable is hidden |
| Embedded images (PNG, JPEG, WebP) | Images about their origin; the reference renderer draws an embedded color profile's colors in sRGB, as a browser does |
| Text: runs in styles (font size, line height, letter spacing, variation axes, fills and strokes) | Each style's glyph outlines, shaped with the file's embedded font (rustybuzz, as Rive shapes with HarfBuzz) and broken into lines and laid out as Rive does: paragraphs, wrapping, alignment, auto width, auto height and fixed sizes, origins (top or baseline), vertical alignment, paragraph spacing, and hidden, clipped and fitted overflow. Text keyed over time (a counter) is laid out at each change, each layout shown while it holds |
| Bones and skins | Bones carry what is under them (a bone sits at its parent's tip); a skin deforms its outline's vertices and handles by the weights of its bones, as Rive's runtime does |
| Image meshes | The image drawn through each triangle, moved onto the mesh's (by the bones when skinned) |
| Constraints: translation, rotation, scale, transform, distance, IK (any number of bones, inverted too) and follow-path | Applied as Rive's runtime applies them, in its update order (targets first, an IK chain's other children after its tip), with their strengths, spaces, copy factors, offsets and limits; what they move is baked into a matrix at 60 frames a second |
| Draw rules | Drawables moved before or after their target as Rive sorts its draw order; a target keyed over time draws the drawable in each place it takes, while it is there |
| Clipping shapes over several drawables | One clip over each run of drawables under the same clipping shapes, of the source's outlines as one path under the clip's rule; clips do not set what they hold apart, so blend modes inside mix with what is under them |
| Text modifiers: groups with their translation, rotation, scale, origin and opacity; ranges by characters, characters without spaces, words or lines, of a run or all, by percentage or index, with falloffs, an interpolator, offsets, strengths, modes and clamping | Each glyph moved, turned, scaled and faded as much as the ranges cover its characters, at each time they change, as Rive's runtime does |
| Joysticks | Their x and y animations applied over the others at the fraction their values say: their own (keyed or not), or where their handle node is within their bounds |
| Nested artboards | The artboard they show, converted on its own and drawn at their transform about its origin, clipped to it; its nested animations play on the parent's clock in layers, a later one overriding an earlier one where both key a property: a simple animation while it plays (`isPlaying` may be keyed) at its speed, repeating as its loop says; a state machine as Rive plays it, its inputs as the parent's animations key them (in each of the parent's spans, those holding the same inputs sharing one run of it; laid out over many spans, it plays in the parent's state machine); a remap animation at the time keyed. A nested artboard playing alike in each of the parent's animations keeps one copy of its keyframes, restarted by a time remap |

## Animations

The artboard's default state machine comes first, at 0, under a marker of its name: it plays as Rive's
runtime plays it, with its inputs as they are unless they are changed (below). Each layer enters its first
state; transitions whose conditions hold for the inputs' values are taken, at their exit times, mixing
from one state into the next over their durations and easings; a state of negative speed plays its
animation backwards from its end; a blend state plays its animations together, a 1D blend mixing the two
around its number input's value, a direct blend each by its own input or value (each applied in turn over
what the property had, or over its first animation's first keys where it resets); later layers override
earlier ones where they key the same property,
and a property no state applies keeps its last value. The timeline's span is the machine's until it
settles and then what repeats (`loop_start` where it settles), so a player with no `segment` plays what
the file plays when it opens.

Each linear animation of the artboard is a span of the timeline under a [marker](./scene-model.md#the-timeline) of its
name, one after another with a second between them, after the state machine. The marker carries the
animation's loop (once, loop, ping-pong), and the player plays a marker as its loop says. An animation's work area and speed (backwards too) set what
of it plays and how fast. Where an animation does not key a property, the property holds its setup value.

Keyframes keep their holds, straight lines and cubic easings; Rive's value curves become cubic easings of the
change. Elastic easings, colors whose alpha and color both change (Rive mixes them straight, the scene
premultiplied), products of moving opacities and outlines whose tangents turn are sampled at 60 frames a
second, straight between samples, within half of the keyframe limit.

```tsx
import { VectorAnimation } from "dxn-vector-animation";

<VectorAnimation src="https://example.com/icons.riv" segment="hover" />
```

## Inputs and listeners

`Options::inputs` changes the default state machine's inputs over time (`InputChange { time, input, value }`:
a bool, a number, or a trigger fired): the machine plays as Rive's runtime plays it with each change made at
the first frame at or after its time, and its span runs on until it settles after the last change.
`machine(data, &options)` lists the machine's inputs, with their values at first, and its listeners: the
shapes they listen on (a node's are the shapes under it; their ids are the scene's group ids), the pointer
events they act on (enter, exit, down, up, move, click) and what they do (set, toggle or fire an input,
move a node to the pointer).

The state machines nested artboards play take part too: `machine` lists them under their parent's
(`Machine::nested`), each at a path of nested artboards (`#` and each one's index in the artboard holding
it, `/` between; an `InputChange`'s `path` may name them instead), and a change at a path is made to that
machine, which plays it in the parent's spans.

`Interaction` plays a file as a user does. `set(time, input, value)` (or `set_at` with a path) changes an
input; `pointer(time, kind, point)` hit-tests the listeners' shapes at a point of the scene, nested
artboards' too ([`Scene::hit_rect`](./scene-model.md#hit-tests): the box 2 units around the pointer
against their outlines under the non-zero rule, painted or not, front to back, a shape flagged opaque
hiding what is behind it, as Rive hits them) and makes what they do. A pointer pressed on a listener's
shape drags it as it moves, wherever it goes, from its second move (Rive's web runtime starts the drag with
the first). An event a listener fires reaches the listeners listening for it: its machine's own, then its
nested artboard's parent's, and on up (only the file's own reach the app, as cues). Each change reads the
file again (`import()`); `Interaction::forward()` reads it from the change's time on (`Options::from`:
values computed from others, skinned meshes and constraints, are sampled from the span that time falls in,
and held before), so that a change costs a player playing on the same however long it has played. A press, a release and a finger lifted act at once, as Rive's web runtime
advances the machine by nothing after them; a move, a leaving and an input set by hand act from the next
frame. A node moved to the pointer holds there, or moves as far as the pointer did when it keeps its
offset, until a key moves it again.

```rust
use dxn_vector_animation::{Limits, Point};
use dxn_vector_animation_rive::{Interaction, Options, Pointer};

let mut play = Interaction::new(data, Limits::default(), Options::default())?;
play.pointer(1.0, Pointer::Down, Point::new(215.0, 212.0))?;
play.pointer(1.05, Pointer::Lift, Point::new(215.0, 212.0))?; // a click
let scene = &play.import().scene;
```

The player does the same: the pointer over a Rive file (a finger or a button going down, moving and
lifting; the mouse moving over it and leaving) acts on its listeners, the ref's `setInput(name, value)` and
`fire(name)` change its inputs, `onInput` reports each change and `onLoad`'s `inputs` lists them. Each
change is made off the engine thread, the file read from the change on, and playback goes on in the new
scene from where it is.

```tsx
let player: VectorAnimationRef | undefined;

<VectorAnimation src={menu} ref={(r) => (player = r)} onInput={(i) => console.log(i.name, i.value)} />
<Button onPress={() => player?.setInput("open", true)}><Text>Open</Text></Button>
```

The CLI renders with changes (`--input open=true@0.5`, `--input bounce@1` fires a trigger,
`--input '#7/hover=true@1'` a nested artboard's machine's) and pointers (`--pointer down:215,212@1 --pointer
lift:215,212@1.05`, printing what they change), and `inspect` lists the machines, their inputs and their
listeners with where their shapes are.

```sh
vector-animation inspect icons.riv                      # artboard size, one marker per animation
vector-animation render icons.riv --marker hover --fps 30
vector-animation render menu.riv --artboard Menu -t 0.5
```

```example
demo: rive-switch
height: 260
caption: A click on the switch toggles its bool input "on" through its listener; the button sets it with `setInput`.
```

## Events

The events the default state machine fires are the timeline's [cues](./scene-model.md#the-timeline), where
they happen: those its states fire as they are entered and left, its transitions as they start and end
(after their mix), and those the animations its states play key, as the animations pass their keyframes
(across loops and ping-pong turns too). A cue carries its event's name, an open-URL event's link and target,
and its custom properties (numbers, strings, booleans). Rive's web runtime reports an event in the frame after
the one it happened in, with how long ago it happened; the cue is the moment itself. Listeners fire events
too: `Interaction::take_fired` gives those the pointer fired, as cues at their times.

The player reports each cue as playback passes it (`onCue`), and those listeners fire as the pointer acts;
`inspect` lists the timeline's cues.

```example
demo: rive-events
height: 220
caption: The ball's state fires "started" as it is entered, its animation "left" and "right" at each side: `onCue` reports them.
```

## Data binding

An artboard bound to a view model plays with its first instance bound, as Rive's editor and players binding
automatically (`autoBind`) do: each binding to its target sets the property it binds, an object's (a color,
a number, a text run's text, a solo's active child by index or name, ...; its value from then on, where no
animation keys it) or one the state machine reads, by its path or (a binding by name) by the names the file's
manifest gives it, so that its conditions on view model values hold as the
values compare (numbers ordered; booleans, enums, strings and colors equal or not) and a 1D blend on a view
model blends at its value.

A binding reads through nested view models and runs its value through its converter as Rive's runtime does:
operations (by a value, or by a view model's number), normalizers and degrees to radians, range mappers
(clamped, wrapping, reversed, eased by an interpolator), rounding, to-string (decimals, trailing zeros,
thousands separators, color formats) and to-number, string padding and trimming, boolean negation, trigger
counts, interpolators easing a change over their duration (numbers and colors, a change made during another
smoothed from where it is), and groups of them. The artboards it shows through nested artboards bind too, in their host's data
context or in the instance their host's path names within it, their paths read up the contexts as Rive's
`DataContext` reads them.

`Options::values` changes the instance over time, as an app sets values through Rive's view model instance
API (`render --value 'level=5@1'`, the player's `setValue` and `fireValue`): from the frame Rive applies a
change in, what is bound to the property follows it, the state machines' conditions on it hold as it
compares, and a condition on a value changing or a trigger fired holds in that frame. A value an animation
keys too is set in that frame after the animations, as Rive applies bindings: where a layer keys it, it shows
in that frame alone; where none does, it holds. States and transitions firing a view model trigger
(`StateMachineFireTrigger`) fire it in the frame they start or end, every layer following it there. A
listener changing a
view model (`ListenerViewModelChange`) writes the value its bindable property holds through that property's
binding to the view model, as the pointer reaches the listener: a press firing a view model trigger, often.
A property the file's instance writes no value of is left out, as Rive copies the instance.

```example
demo: rive-progress
height: 320
caption: A view model drives the bar: its width through an interpolating converter and a range mapper, its label through a to-string, its color eased; the buttons call `setValue` and `fireValue`.
```

`Options::unbound` reads the file as a player binding nothing does. `view_model(data, &options)` gives the
view model bound and its instance's values; `inspect` prints them, and the listeners that set them.

Formula, list and scripted converters are left out for now, and two-way bindings write nothing back to the
instance from what they target.

## Left out for now

What a file holds beyond that is left out with a warning (`Import::warnings`, `inspect`, `onLoad`):

- other state machines than the default, the events a nested artboard's state machine fires as it plays
  (those its listeners fire reach its parent), audio; formula, list and scripted converters;
- text variation and follow-path modifiers and text ellipses, layout, draggable and scroll constraints,
  scripting and audio.

These come in the phases of the [design](https://github.com/dXnSix/dXn.VectorAnimation/blob/main/docs/design/rive.md):
what the runtime evaluates (what is left of text), what is left of data binding, then layout.

Next: [Limits](./limits.md), which bound a Rive file as they bound the others.
