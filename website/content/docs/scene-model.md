---
title: The scene model
description: Scene, Group, Shape, Image and Text nodes, paints, effects, clips and masks, Animated values with keyframes and easings, the timeline, and group time.
group: Concepts
order: 30
---

A `Scene` is a tree of groups, shapes, images and text whose properties are `Animated`: a constant, or a track of keyframes on one timeline. The importers build scenes from files; you can also build one in code. This one is a loader, a third of a circle carried round by its trim offset:

```rust
use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Easing, EllipseStart, Geometry, Group, Keyframe,
    LineCap, LineJoin, LoopMode, Node, Paint, Scene, Shape, ShapePath, Stroke, Timeline, Track,
    Trim, TrimMode,
};

fn loader() -> Scene {
    let ring = Shape {
        paths: vec![ShapePath::new(Geometry::Ellipse {
            center: AnimatedPoint::fixed(50.0, 50.0),
            radius: AnimatedPoint::fixed(30.0, 30.0),
            direction: Direction::Clockwise,
            start: EllipseStart::Top,
        })],
        stroke: Some(Stroke {
            paint: Paint::solid(Color::rgba(0.0, 0.6, 0.3, 1.0)),
            opacity: Animated::Static(1.0),
            width: Animated::Static(8.0),
            cap: LineCap::Round,
            join: LineJoin::Miter,
            miter_limit: 4.0,
            dash: None,
        }),
        // A third of the circle, carried round by its offset.
        trim: Some(Trim {
            start: Animated::Static(0.1),
            end: Animated::Static(0.4),
            offset: Animated::Track(Track::new(vec![
                Keyframe::new(0.0, -0.3, Easing::Linear),
                Keyframe::new(2.0, 1.9, Easing::Linear),
            ])),
            mode: TrimMode::EachPath,
        }),
        ..Shape::default()
    };
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_start: 0.0,
            loop_mode: LoopMode::Loop,
            markers: Vec::new(),
        },
        root: Group {
            children: vec![Node::shape(ring)],
            ..Group::new()
        },
    }
}
```

```example
demo: loader
height: 220
caption: The loader scene above, played by the NSH player.
```

Units throughout: coordinates are user units with y growing downwards, angles are degrees turning clockwise, times are seconds, opacities and fractions run from 0 to 1. The `Scene` itself is `width`, `height`, a `timeline` and a `root` group.

## Nodes

| Node | What it is |
|---|---|
| `Node::Group(Group)` | Nodes drawn together: transformed, faded, blended, clipped, masked and given effects as one |
| `Node::Shape(Box<Shape>)` | Outlines filled and stroked with one style; `Node::shape(shape)` boxes it |
| `Node::Image(Image)` | An encoded PNG or JPEG (`Arc<ImageData>` with its `mime` and `bytes`) stretched into `rect` |
| `Node::Text(Box<Text>)` | Spans of text set in a font by whatever draws them |

### Groups

| Field | Type | Meaning |
|---|---|---|
| `id`, `name` | `Option<String>` | The source's id (an SVG `id`, a Lottie layer's `ind`) and the name its author gave it |
| `transform` | `Vec<TransformOp>` | Applied left to right: the first operation is the outermost |
| `opacity` | `Animated<f64>` | The children fade as one; 1 by default |
| `blend` | `BlendMode` | `Normal`, CSS's blend modes (`Multiply` ... `Luminosity`), `Add`, `DestinationIn`, `DestinationOut` |
| `clip` | `Option<Clip>` | Shapes outside which nothing of the group shows, in the group's space |
| `mask` | `Option<Box<Mask>>` | Content whose alpha or luminance decides how much shows |
| `effects` | `Vec<Effect>` | Shadows, blurs and color changes of what the group draws, before its clip and mask |
| `active` | `Option<TimeRange>` | When the group shows, in its parent's time; always when `None` |
| `time` | `Option<TimeMap>` | The group's own time; its parent's when `None` |
| `children` | `Vec<Node>` | Drawn first to last, later ones on top |

`Group::new()` (or `Group::default()`) is an empty, opaque group with no transform.

### Shapes

| Field | Meaning |
|---|---|
| `paths: Vec<ShapePath>` | Outlines filled and stroked together as one path. Each is a `Geometry` plus a transform that moves its points, not the stroke |
| `fill: Option<Fill>` | `paint`, `opacity`, `rule` (`FillRule::NonZero` or `EvenOdd`) |
| `stroke: Option<Stroke>` | `paint`, `opacity`, `width`, `cap`, `join`, `miter_limit`, `dash: Option<Dash>` (animated lengths of dash and gap, and an offset) |
| `paint_order: PaintOrder` | `FillFirst` (default) or `StrokeFirst` |
| `trim: Option<Trim>` | The part of the outlines drawn: `start`, `end` and `offset` as fractions of their length (the offset wraps round), and `mode`: `EachPath` trims each outline alike, `Combined` trims them as one line |

Outlines stay parametric where the file had them so:

| Geometry | Fields |
|---|---|
| `Geometry::Path { path }` | An `Animated<Path>`. Paths of the same commands morph point by point; others switch halfway |
| `Geometry::Rect { .. }` | `center`, `size`, `radius` (corner radii along x and y), `direction`, `start` (`TopLeft` as SVG draws, `TopRight` as Lottie draws) |
| `Geometry::Ellipse { .. }` | `center`, `radius`, `direction`, `start` (`Right` as SVG draws, `Top` as Lottie draws) |
| `Geometry::Star(Star)` | `kind` (`Star` or `Polygon`), `center`, `points`, `outer_radius`, `inner_radius`, `outer_roundness`, `inner_roundness`, `rotation`, `direction` |

`direction` and `start` decide where an outline begins, which is where trims and dashes start counting. `Geometry::path(path)` builds a constant path; `Path` has `move_to`, `line_to`, `quad_to`, `cubic_to`, `close` and parses its own `Display` form (`M0 0L10 0Z`).

### Paints

| Paint | Fields |
|---|---|
| `Paint::Solid { color }` | An `Animated<Color>`; `Paint::solid(color)` for a constant |
| `Paint::LinearGradient(..)` | `start`, `end`, `stops: Animated<Vec<GradientStop>>`, `units`, `transform`, `spread` |
| `Paint::RadialGradient(..)` | `center`, `radius`, `focal`, `stops`, `units`, `transform`, `spread` |
| `Paint::Pattern(Box<Pattern>)` | Tiles side by side, each drawing a `Group`: `tile`, `units`, `content_units`, `view_box`, `transform`, `content` |

A `Color` is sRGB with straight alpha, four `f64` channels from 0 to 1 (`Color::rgba`, `Color::rgba8`, `Color::BLACK`, `WHITE`, `TRANSPARENT`). `GradientUnits` is `UserSpace` or `BoundingBox` (fractions of the shape's box), `Spread` is `Pad`, `Repeat` or `Reflect`.

### Text

`Text` holds `spans: Vec<TextSpan>`, set in a font when drawn. A span has its `text`, a `position` where it starts a new line (`None` goes on after the span before), a `shift`, an `anchor` (`Start`, `Middle`, `End` of its line), a `Font` (`families`, animated `size`, `weight` 100 to 900, `italic`), animated `letter_spacing`, and a fill and stroke like a shape's. Lottie text drawn with the file's own glyphs is not `Text`: it imports as shapes.

### Effects, clips and masks

| Effect | Fields |
|---|---|
| `Effect::DropShadow(DropShadow)` | `color` (its alpha is the opacity), `offset`, `blur` (standard deviation), `knockout` (hidden where the drawing is), `hard` (cast by the coverage made all or nothing, as Figma's are) |
| `Effect::InnerShadow(InnerShadow)` | `color`, `offset`, `blur`, `hard` |
| `Effect::Blur(Blur)` | `radius`: the Gaussian's standard deviation |
| `Effect::Color(ColorEffect)` | `ops: Vec<ColorOp>` applied in turn, `linear` to work in linear light |

Lengths are in the group's space. Effects apply in turn; consecutive shadows are all cast by the drawing as it is before them, drop shadows under it and inner shadows over it. `ColorOp` covers CSS's filter functions (`Saturate`, `HueRotate`, `Grayscale`, `Sepia`, `Invert`, `Opacity`, `Brightness`, `Contrast`), SVG's `Matrix`, `Transfer` and `LuminanceToAlpha`, and Lottie's `Tint`, `Fill`, `Overlay` and `GradientMap`, each amount animated.

A `Clip` is the union of its `shapes` (each a `Geometry`, a transform and a fill rule), or, with `one_path`, the area of their outlines as one path under the first shape's rule (Rive's clips: two outlines even-odd make a ring). A clip does not set its group apart: blend modes inside it mix with what is under the group, as under a Canvas 2D `clip()`. A `Mask`'s `content` is a `Group` drawn in the masked group's space; its `kind` (`MaskKind::Alpha` or `Luminance`) decides how much shows, `inverted` or not.

## Animated values

Every animatable property is an `Animated<T>`: `Animated::Static(value)`, or `Animated::Track(track)` of `Keyframe`s, each a `time`, a `value` and the `easing` of the segment after it.

```rust
use dxn_vector_animation::{Animated, Easing, Keyframe, Track};

// A constant: the same value at every time.
let still: Animated<f64> = 1.0.into();

// A track: keyframes in seconds, each with the easing of the segment after it.
let fade = Animated::Track(Track::new(vec![
    Keyframe::new(0.0, 0.0, Easing::EASE_OUT),
    Keyframe::new(0.5, 1.0, Easing::Hold),
    Keyframe::new(1.0, 0.2, Easing::Linear),
]));

println!("{}", still.sample(3.0)); // 1
println!("{:.3}", fade.sample(0.25)); // eased: 0.685, past halfway
println!("{}", fade.sample(0.75)); // held: 1 until the next keyframe
println!("{}", fade.sample(-1.0)); // before the first keyframe: its value, 0
println!("{}", fade.sample(9.0)); // after the last: its value, 0.2
println!("{} {}", still.is_animated(), fade.keyframes().len()); // false 3

// Keyframes that all hold one value collapse into a constant.
let flat = Animated::from_keyframes(
    vec![Keyframe::new(0.0, 5.0, Easing::Linear), Keyframe::new(1.0, 5.0, Easing::Linear)],
    0.0,
);
println!("{:?}", flat.as_static()); // Some(5.0)
```

`Track::new` sorts keyframes by time, stably: two keyframes at the same time make a jump, the later one winning from that time on. `sample(t)` is defined for every `t`; the time is in the timeline of the group holding the property.

Values move between keyframes through the `Interpolate` trait:

| Type | Moves |
|---|---|
| `f64`, `Point` | Straight, by the eased fraction (which may leave 0..1 where an easing overshoots) |
| `bool` | Switches at the next keyframe |
| `Color` | In premultiplied sRGB, as CSS animations mix colors; equal alphas channel by channel, as Lottie does |
| `Path` | Point by point when both have the same commands; otherwise switches halfway |
| `Matrix` | Through its decomposition (translation, rotation, scale, skew) |
| `Vec<T>` (gradient stops) | Item by item when the lengths match; otherwise switches halfway |

`AnimatedPoint` is two `Animated<f64>`, so x and y can have their own keyframes and easings; `AnimatedPoint::fixed(x, y)` is a constant point.

## Easings

| `Easing` | From |
|---|---|
| `Linear` (default) | Constant speed |
| `CubicBezier { x1, y1, x2, y2 }` | CSS `cubic-bezier()`, Lottie's out and in tangents. `x1`, `x2` within 0..1; `y1`, `y2` may overshoot |
| `Hold` | The start value until the next keyframe: Lottie's hold keyframes, Figma's `HOLD` |
| `Steps { count, position }` | CSS `steps()`; `StepPosition` is `JumpStart`, `JumpEnd` (default), `JumpNone`, `JumpBoth` |
| `LinearPoints { points }` | CSS `linear()`: straight lines through (progress, output) points |

`Easing::EASE`, `EASE_IN`, `EASE_OUT` and `EASE_IN_OUT` are CSS's keywords. `Easing::cubic_bezier(x1, y1, x2, y2)` clamps the x values and gives `Linear` for a straight curve. `apply(t)` maps a segment's progress to the fraction of the change:

```rust
use dxn_vector_animation::{Easing, StepPosition};

let curves = [
    ("linear", Easing::Linear),
    ("ease", Easing::EASE),
    ("cubic-bezier(.34,1.56,.64,1)", Easing::cubic_bezier(0.34, 1.56, 0.64, 1.0)),
    ("hold", Easing::Hold),
    ("steps(4, end)", Easing::Steps { count: 4, position: StepPosition::JumpEnd }),
    ("linear(0, 0.8 50%, 1)", Easing::LinearPoints { points: vec![(0.0, 0.0), (0.5, 0.8), (1.0, 1.0)] }),
];
for (name, easing) in curves {
    let at = |t: f64| format!("{:.3}", easing.apply(t));
    println!("{name:32} {} {} {}", at(0.2), at(0.5), at(0.9));
}
```

```text
linear                           0.200 0.500 0.900
ease                             0.295 0.802 0.994
cubic-bezier(.34,1.56,.64,1)     0.703 1.087 1.013
hold                             0.000 0.000 0.000
steps(4, end)                    0.000 0.500 0.750
linear(0, 0.8 50%, 1)            0.320 0.800 0.960
```

Springs have no easing of their own: exporters sample them into keyframes.

## Transforms

A transform is a list of `TransformOp`s, each value animated on its own, composed left to right as CSS and SVG compose them:

| Operation | Values |
|---|---|
| `Translate { x, y }`, `Scale { x, y }`, `Skew { x, y }` | Two `Animated<f64>`; skews in degrees |
| `Rotate { degrees }` | Clockwise |
| `Matrix { matrix }` | An `Animated<Matrix>` (`a b c d e f`, as SVG writes it) |
| `Motion(MotionPath)` | A point a fraction `progress` along `path`, turned by `rotate`: `MotionRotate::Fixed { degrees }` or `Auto { offset }` along the path |

```rust
use dxn_vector_animation::{Animated, Easing, Keyframe, Point, Track, TransformOp, transform::transform_at};

let grow = Animated::Track(Track::new(vec![
    Keyframe::new(0.0, 0.0, Easing::EASE_OUT),
    Keyframe::new(1.0, 1.0, Easing::Linear),
]));
// Scale about (10, 5): move the origin there, scale, move it back.
let ops = vec![
    TransformOp::translate(10.0, 5.0),
    TransformOp::Scale { x: grow.clone(), y: grow },
    TransformOp::translate(-10.0, -5.0),
];
println!("{:?}", transform_at(&ops, 0.0).apply(Point::new(0.0, 0.0))); // (10, 5)
println!("{:?}", transform_at(&ops, 1.0).apply(Point::new(0.0, 0.0))); // (0, 0)
```

The constructors `TransformOp::translate`, `scale`, `rotate` and `matrix` make constant operations. Importers keep a file's lists function by function where their functions match across keyframes, as CSS does, and animate a matrix through its decomposition where they differ.

## The timeline

| Field | Meaning |
|---|---|
| `duration` | Seconds one play-through lasts |
| `frame_rate` | The rate the animation was made at, when the file says (Lottie's `fr`); a hint, not a grid |
| `loop_start` | Where repeating starts over, from 0 (default: the whole timeline) to `duration`: what comes before it plays once, the loop repeats from it to the end. The SVG importer sets it after [delays](./svg.md#the-loop) |
| `loop_mode` | `LoopMode::Once` (default: plays and holds its last frame), `Loop` (to the end, then from the loop start, over and over), `PingPong` (to the end, back to the loop start, and on) |
| `markers` | Named times or spans: `Marker::new(name, start, duration)`, a duration of 0 for a point in time; `loop_mode`, when set, is how the span repeats when it plays on its own (a [Rive](./rive.md) animation's loop) |

`Scene::frame_at(t)` does not wrap `t`: looping is the player's. To play on a clock, map the elapsed time to scene time first with `Timeline::time_at`:

```rust
use dxn_vector_animation::Scene;

/// The frame to show `elapsed` seconds after playback started.
fn frame(scene: &Scene, elapsed: f64) -> dxn_vector_animation::Frame {
    scene.frame_at(scene.timeline.time_at(elapsed))
}
```

Up to the end it is the elapsed time. Past it, `Once` holds the end, `Loop` plays from the loop start to the end over and over, and `PingPong` goes back to the loop start and forwards again. At 2.5 s into a 2 s timeline that loops from 0 this gives 2 for `Once`, 0.5 for `Loop` and 1.5 for `PingPong`; looping from 0.5 s, `Loop` gives 1. `Timeline::loop_from()` is the loop start, or 0 when it is not within the timeline. `Scene::check` refuses a loop start outside it.

### Events

A marker named `on:<event>` (the prefix is `EVENT_PREFIX`) holds what plays when an event happens, such as an SVG's `begin="click"`. Its segment lies past the timeline's end, where the timeline never reaches. `Scene::events()` lists the names; `Scene::with_events(&[(name, seconds)])` returns a scene whose event segments are moved to when their events happened, overriding what the properties would do from then:

```rust
use dxn_vector_animation::{Item, Scene};

/// The opacity the button is drawn at, at `t`.
fn opacity(scene: &Scene, t: f64) -> f64 {
    scene.frame_at(t).items.iter().find_map(|item| match item {
        Item::PushLayer(layer) => Some(layer.opacity),
        _ => None,
    }).unwrap_or(1.0)
}

let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="60">
  <rect id="btn" width="120" height="60" fill="#2563eb">
    <animate attributeName="opacity" values="1;0.4;1" dur="0.3s" begin="click"/>
  </rect>
</svg>"##;
let scene = dxn_vector_animation_svg::read(svg)?.scene;
println!("{:?}", scene.events()); // ["btn.click"]

// The button was clicked 1.5 s in: what the click plays, plays from then.
let clicked = scene.with_events(&[("btn.click", 1.5)]);
println!("{:.2} {:.2}", opacity(&scene, 1.65), opacity(&clicked, 1.65)); // 1.00 0.40
```

Events the scene has nothing for change nothing.

## Group time

Two fields give a group a time of its own, which is how nested compositions, layers' in and out points and time remapping are represented:

- `active: Option<TimeRange>` shows the group from `start` (included) to `end` (excluded), in its parent's time.
- `time: Option<TimeMap>` maps the parent's time to the group's: `(parent - offset) / scale`, then through `remap` when there is one. A `scale` of 2 runs at half speed. `remap` is an `Animated<f64>` giving the time shown at each moment.

```rust
use dxn_vector_animation::{Animated, Easing, Group, Keyframe, TimeMap, TimeRange, Track};

// A nested composition: it starts 1 s in, plays at half speed,
// and shows from 1 s to 5 s of its parent's time.
let comp = Group {
    active: Some(TimeRange { start: 1.0, end: 5.0 }),
    time: Some(TimeMap { offset: 1.0, scale: 2.0, remap: None }),
    ..Group::new()
};
println!("{}", comp.local_time(3.0)); // (3 - 1) / 2 = 1

// Time remapping: held at 0.5 s for a second, then running on.
let remapped = Group {
    time: Some(TimeMap {
        offset: 0.0,
        scale: 1.0,
        remap: Some(Animated::Track(Track::new(vec![
            Keyframe::new(0.0, 0.5, Easing::Hold),
            Keyframe::new(1.0, 0.5, Easing::Linear),
            Keyframe::new(2.0, 1.5, Easing::Linear),
        ]))),
    }),
    ..Group::new()
};
println!("{} {}", remapped.local_time(0.7), remapped.local_time(1.5)); // 0.5 1
```

A group's properties and its children are sampled in its own time. `active` is checked in the parent's time, before the map.

Next: [Frames and rendering](./frames-and-rendering.md) turns a scene into pixels; [Compiled scenes](./compiled-scenes.md) saves one.
