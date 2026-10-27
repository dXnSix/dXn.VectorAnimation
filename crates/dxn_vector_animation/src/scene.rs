//! The scene: a tree of groups, shapes and images whose properties are
//! constants or keyframe tracks on one timeline.

use std::sync::Arc;

use crate::{Animated, AnimatedPoint, Color, Matrix, Path, Point, Rect};

/// An animated vector scene.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Scene {
    /// Width in user units.
    pub width: f64,
    /// Height in user units.
    pub height: f64,
    /// Duration, loop mode and markers.
    pub timeline: Timeline,
    /// The content; the scene's coordinates are its parent space.
    pub root: Group,
}

/// The scene's time: what one play-through covers and how it repeats.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Timeline {
    /// Seconds one play-through lasts.
    pub duration: f64,
    /// The frame rate the animation was made at, when the file says.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub frame_rate: Option<f64>,
    /// Where repeating starts over (seconds, from 0 to `duration`): what
    /// comes before it plays once, the loop repeats from it to the end. 0,
    /// the whole timeline, by default.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "is_zero"))]
    pub loop_start: f64,
    /// How it repeats.
    #[cfg_attr(feature = "serde", serde(default))]
    pub loop_mode: LoopMode,
    /// Named times or spans.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub markers: Vec<Marker>,
    /// Moments playback reports as it passes them (a Rive file's events),
    /// in time order.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub cues: Vec<Cue>,
}

/// A moment of the timeline that playback reports as it passes it, with
/// what it carries: a Rive event, fired by a state machine or keyed in an
/// animation.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Cue {
    /// Where it is, in seconds.
    pub time: f64,
    /// Its name.
    pub name: String,
    /// A link it opens (a Rive open-URL event), and where (`_blank`,
    /// `_parent`, `_self` or `_top`).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub url: Option<(String, String)>,
    /// Named values it carries (a Rive event's custom properties).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub properties: Vec<(String, CueValue)>,
}

/// A value a cue carries.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum CueValue {
    /// A number.
    Number(f64),
    /// A string.
    Text(String),
    /// A boolean.
    Bool(bool),
}

#[cfg(feature = "serde")]
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

impl Timeline {
    /// Where the loop starts over: [`loop_start`](Self::loop_start), or 0
    /// when it is not within the timeline.
    pub fn loop_from(&self) -> f64 {
        if self.loop_start > 0.0 && self.loop_start < self.duration {
            self.loop_start
        } else {
            0.0
        }
    }

    /// The time of the timeline shown `elapsed` seconds after it started
    /// playing, repeating as its loop mode says: once, it holds its end;
    /// looping, it plays to the end once and then from the loop start to
    /// the end over and over; ping-pong, it goes back to the loop start
    /// and forwards again. Before 0 it shows 0.
    pub fn time_at(&self, elapsed: f64) -> f64 {
        let end = self.duration.max(0.0);
        if elapsed.is_nan() || elapsed <= 0.0 {
            return 0.0;
        }
        if elapsed <= end {
            return elapsed;
        }
        let from = self.loop_from();
        let span = end - from;
        if span <= 0.0 {
            return end;
        }
        let past = elapsed - end;
        match self.loop_mode {
            LoopMode::Once => end,
            LoopMode::Loop => from + past % span,
            LoopMode::PingPong => {
                let within = past % (2.0 * span);
                if within <= span {
                    end - within
                } else {
                    from + (within - span)
                }
            }
        }
    }
}

/// How an animation repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum LoopMode {
    /// Plays once and holds its last frame.
    #[default]
    Once,
    /// Starts over at the end.
    Loop,
    /// Plays forwards, then backwards, and over.
    PingPong,
}

/// A named time or span of the timeline. One named `on:<event>` past the
/// timeline's end holds what plays when that event happens (see
/// [`Scene::with_events`]).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Marker {
    /// Its name.
    pub name: String,
    /// Where it starts, in seconds.
    pub start: f64,
    /// How long it lasts, in seconds (0 for a point in time).
    #[cfg_attr(feature = "serde", serde(default))]
    pub duration: f64,
    /// How its span repeats when it plays on its own (a Rive animation's
    /// loop); the timeline's loop mode when `None`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub loop_mode: Option<LoopMode>,
}

impl Marker {
    /// A marker named `name` from `start` lasting `duration`.
    pub fn new(name: impl Into<String>, start: f64, duration: f64) -> Self {
        Marker {
            name: name.into(),
            start,
            duration,
            loop_mode: None,
        }
    }
}

/// A node of the scene tree.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", tag = "type"))]
pub enum Node {
    /// Nodes drawn together, transformed, faded, clipped or masked.
    Group(Group),
    /// Paths filled and stroked.
    Shape(Box<Shape>),
    /// A raster image.
    Image(Image),
    /// Text set in a font when it is drawn.
    Text(Box<Text>),
}

impl Node {
    /// A shape node.
    pub fn shape(shape: Shape) -> Node {
        Node::Shape(Box::new(shape))
    }
}

impl From<Shape> for Node {
    fn from(shape: Shape) -> Self {
        Node::shape(shape)
    }
}

impl From<Group> for Node {
    fn from(group: Group) -> Self {
        Node::Group(group)
    }
}

/// Nodes drawn together: transformed, faded, blended, clipped or masked as
/// one.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Group {
    /// The source file's id (an SVG `id`, a Lottie layer's index).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub id: Option<String>,
    /// The name its author gave it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub name: Option<String>,
    /// Applied left to right: the first operation is the outermost.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub transform: Vec<TransformOp>,
    /// 0 transparent, 1 opaque; the children fade as one.
    #[cfg_attr(feature = "serde", serde(default = "opaque"))]
    pub opacity: Animated<f64>,
    /// How the group composes with what is under it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "BlendMode::is_normal")
    )]
    pub blend: BlendMode,
    /// Shapes outside which nothing of the group shows; in the group's
    /// space (inside its transform).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub clip: Option<Clip>,
    /// Content whose alpha or luminance decides how much of the group
    /// shows; in the group's space.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub mask: Option<Box<Mask>>,
    /// Shadows, blurs and color changes of what the group draws (before its
    /// clip and mask), in the group's space, in turn: consecutive shadows
    /// are all cast by the drawing as it is before them, drop shadows under
    /// it (the first lowest) and inner shadows over it (the first lowest).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub effects: Vec<Effect>,
    /// When the group shows, in its parent's time; always when `None`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub active: Option<TimeRange>,
    /// The group's own time, for its properties and its children; its
    /// parent's when `None`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub time: Option<TimeMap>,
    /// Drawn first to last (later ones on top).
    #[cfg_attr(feature = "serde", serde(default))]
    pub children: Vec<Node>,
}

#[cfg(feature = "serde")]
fn opaque() -> Animated<f64> {
    Animated::Static(1.0)
}

impl Default for Group {
    fn default() -> Self {
        Group {
            id: None,
            name: None,
            transform: Vec::new(),
            opacity: Animated::Static(1.0),
            blend: BlendMode::Normal,
            clip: None,
            mask: None,
            effects: Vec::new(),
            active: None,
            time: None,
            children: Vec::new(),
        }
    }
}

/// What a group's drawing goes through before it composes.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", tag = "type"))]
pub enum Effect {
    /// The drawing's silhouette under it, blurred, moved and colored.
    DropShadow(DropShadow),
    /// The drawing blurred.
    Blur(Blur),
    /// A shadow inside the drawing, cast on it by what is around it.
    InnerShadow(InnerShadow),
    /// The drawing's colors changed pixel by pixel.
    Color(ColorEffect),
}

/// A shadow cast inside a group's drawing by what is around it: where the
/// drawing is, less the drawing moved and blurred, colored and laid over
/// it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct InnerShadow {
    /// Its color; its alpha is the shadow's opacity.
    pub color: Animated<Color>,
    /// How far it falls, in the group's lengths.
    pub offset: AnimatedPoint,
    /// The standard deviation of its blur, in the group's lengths.
    pub blur: Animated<f64>,
    /// Cast by the drawing's silhouette made all or nothing (Figma's
    /// `hardAlpha`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub hard: bool,
}

/// A group's drawing with its colors changed by `ops` in turn. Where the
/// drawing is transparent it stays so.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ColorEffect {
    /// The changes, first to last.
    pub ops: Vec<ColorOp>,
    /// Done in linear light (SVG filters' default `linearRGB`), not on the
    /// sRGB values.
    #[cfg_attr(feature = "serde", serde(default))]
    pub linear: bool,
}

/// One change of a [`ColorEffect`], on straight (not premultiplied)
/// colors. Amounts of 1 are the full change and 0 none, unless said.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", tag = "type"))]
pub enum ColorOp {
    /// A 4×5 matrix (SVG `feColorMatrix`'s `values`).
    Matrix {
        /// The matrix.
        matrix: Animated<ColorMatrix>,
    },
    /// CSS `saturate()`: 0 gray, 1 unchanged, more oversaturated.
    Saturate {
        /// The saturation.
        amount: Animated<f64>,
    },
    /// CSS `hue-rotate()`.
    HueRotate {
        /// Degrees.
        degrees: Animated<f64>,
    },
    /// CSS `grayscale()`.
    Grayscale {
        /// How far.
        amount: Animated<f64>,
    },
    /// CSS `sepia()`.
    Sepia {
        /// How far.
        amount: Animated<f64>,
    },
    /// CSS `invert()`.
    Invert {
        /// How far.
        amount: Animated<f64>,
    },
    /// CSS `opacity()`: the alpha times it.
    Opacity {
        /// The factor.
        amount: Animated<f64>,
    },
    /// CSS `brightness()`: the colors times it.
    Brightness {
        /// The factor.
        amount: Animated<f64>,
    },
    /// CSS `contrast()`: the colors spread from the middle gray by it.
    Contrast {
        /// The factor.
        amount: Animated<f64>,
    },
    /// SVG `luminanceToAlpha`: black whose alpha is the luminance.
    LuminanceToAlpha,
    /// Lottie's Tint: the luminance mapped from `black` to `white`.
    Tint {
        /// What black becomes.
        black: Animated<Color>,
        /// What white becomes.
        white: Animated<Color>,
        /// How far.
        amount: Animated<f64>,
    },
    /// Lottie's Fill effect: one color where the drawing is, its alpha
    /// times `opacity`.
    Fill {
        /// The color; its alpha multiplies the drawing's too.
        color: Animated<Color>,
        /// The drawing's alpha times it.
        opacity: Animated<f64>,
    },
    /// Lottie's Color Overlay layer style: a color mixed in, the alpha
    /// kept.
    Overlay {
        /// The color; its alpha times `amount` is how far.
        color: Animated<Color>,
        /// How far.
        amount: Animated<f64>,
    },
    /// Each channel through its function (SVG `feComponentTransfer`).
    Transfer {
        /// Red.
        red: Transfer,
        /// Green.
        green: Transfer,
        /// Blue.
        blue: Transfer,
        /// Alpha.
        alpha: Transfer,
    },
    /// The luminance mapped to the colors of a gradient (Lottie's Tritone).
    GradientMap {
        /// The colors from black (0) to white (1).
        stops: Animated<Vec<GradientStop>>,
        /// How far.
        amount: Animated<f64>,
    },
}

/// A 4×5 color matrix on straight RGBA: rows red, green, blue and alpha,
/// each the input's channels times its first four values plus its fifth
/// (channels 0 to 1), as SVG's `feColorMatrix` `values`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ColorMatrix(pub [f64; 20]);

/// A channel's function in a [`ColorOp::Transfer`] (SVG's `feFuncX`).
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", tag = "type"))]
pub enum Transfer {
    /// Unchanged.
    #[default]
    Identity,
    /// Straight lines through the values, evenly spread over 0..1.
    Table {
        /// The values.
        values: Vec<f64>,
    },
    /// Steps of the values, evenly spread over 0..1.
    Discrete {
        /// The values.
        values: Vec<f64>,
    },
    /// `slope × c + intercept`.
    Linear {
        /// The slope.
        slope: f64,
        /// The intercept.
        intercept: f64,
    },
    /// `amplitude × c ^ exponent + offset`.
    Gamma {
        /// The amplitude.
        amplitude: f64,
        /// The exponent.
        exponent: f64,
        /// The offset.
        offset: f64,
    },
}

/// A shadow a group's drawing casts.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct DropShadow {
    /// Its color; its alpha is the shadow's opacity.
    pub color: Animated<Color>,
    /// How far it falls, in the group's lengths.
    pub offset: AnimatedPoint,
    /// The standard deviation of its blur, in the group's lengths.
    pub blur: Animated<f64>,
    /// Hidden where the drawing is, even where it is see-through (Figma's
    /// shadows not shown behind transparent areas).
    #[cfg_attr(feature = "serde", serde(default))]
    pub knockout: bool,
    /// Cast by the drawing's silhouette made all or nothing: anything
    /// drawn at all casts the full shadow (Figma's `hardAlpha`, the
    /// coverage times 127).
    #[cfg_attr(feature = "serde", serde(default))]
    pub hard: bool,
}

/// A Gaussian blur of a group's drawing.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Blur {
    /// Its standard deviation, in the group's lengths.
    pub radius: Animated<f64>,
}

impl Group {
    /// An empty group.
    pub fn new() -> Self {
        Group::default()
    }

    /// The group's time for its parent's `time`.
    pub fn local_time(&self, time: f64) -> f64 {
        match &self.time {
            Some(map) => map.apply(time),
            None => time,
        }
    }
}

/// A span of time: from `start` (included) to `end` (excluded), in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct TimeRange {
    /// Start, included.
    pub start: f64,
    /// End, excluded.
    pub end: f64,
}

impl TimeRange {
    /// Whether `time` is within it.
    pub fn contains(&self, time: f64) -> bool {
        self.start <= time && time < self.end
    }
}

/// A group's own time: `(parent − offset) / scale`, then `remap` when
/// there is one (a nested composition's time remapping).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct TimeMap {
    /// Seconds the group's time starts after its parent's.
    #[cfg_attr(feature = "serde", serde(default))]
    pub offset: f64,
    /// How much slower the group's time runs (2: half speed).
    #[cfg_attr(feature = "serde", serde(default = "one"))]
    pub scale: f64,
    /// The time shown at each moment, when remapped.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub remap: Option<Animated<f64>>,
}

#[cfg(feature = "serde")]
fn one() -> f64 {
    1.0
}

impl TimeMap {
    /// The group's time for its parent's `time`.
    pub fn apply(&self, time: f64) -> f64 {
        let scale = if self.scale == 0.0 { 1.0 } else { self.scale };
        let local = (time - self.offset) / scale;
        match &self.remap {
            Some(remap) => remap.sample(local),
            None => local,
        }
    }
}

/// One operation of a transform list.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", tag = "type"))]
pub enum TransformOp {
    /// Moves by (`x`, `y`).
    Translate {
        /// Horizontal.
        x: Animated<f64>,
        /// Vertical.
        y: Animated<f64>,
    },
    /// Scales about the origin.
    Scale {
        /// Horizontal factor.
        x: Animated<f64>,
        /// Vertical factor.
        y: Animated<f64>,
    },
    /// Turns about the origin, clockwise in degrees.
    Rotate {
        /// Degrees, clockwise.
        degrees: Animated<f64>,
    },
    /// Skews: `x` degrees along the x axis, `y` along the y axis.
    Skew {
        /// Degrees along x.
        x: Animated<f64>,
        /// Degrees along y.
        y: Animated<f64>,
    },
    /// A whole matrix.
    Matrix {
        /// The matrix.
        matrix: Animated<Matrix>,
    },
    /// Moves to a point along a path, optionally turned along it.
    Motion(MotionPath),
}

impl TransformOp {
    /// A constant translation.
    pub fn translate(x: f64, y: f64) -> Self {
        TransformOp::Translate {
            x: Animated::Static(x),
            y: Animated::Static(y),
        }
    }

    /// A constant scale.
    pub fn scale(x: f64, y: f64) -> Self {
        TransformOp::Scale {
            x: Animated::Static(x),
            y: Animated::Static(y),
        }
    }

    /// A constant rotation.
    pub fn rotate(degrees: f64) -> Self {
        TransformOp::Rotate {
            degrees: Animated::Static(degrees),
        }
    }

    /// A constant matrix.
    pub fn matrix(matrix: Matrix) -> Self {
        TransformOp::Matrix {
            matrix: Animated::Static(matrix),
        }
    }

    /// Whether any of its values changes over time.
    pub fn is_animated(&self) -> bool {
        match self {
            TransformOp::Translate { x, y }
            | TransformOp::Scale { x, y }
            | TransformOp::Skew { x, y } => x.is_animated() || y.is_animated(),
            TransformOp::Rotate { degrees } => degrees.is_animated(),
            TransformOp::Matrix { matrix } => matrix.is_animated(),
            TransformOp::Motion(motion) => motion.progress.is_animated(),
        }
    }
}

/// A position along a path (CSS `offset-path`, Lottie's spatial position
/// keyframes): the transform moves the origin to the point a fraction
/// `progress` of the path's length along it, and turns it as `rotate`
/// says.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct MotionPath {
    /// The path, in the space the transform operation applies in.
    pub path: Path,
    /// How far along: 0 the start, 1 the end (a closed path wraps around).
    pub progress: Animated<f64>,
    /// How the content turns along the path.
    #[cfg_attr(feature = "serde", serde(default))]
    pub rotate: MotionRotate,
}

/// How content moving along a path turns.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", tag = "type"))]
pub enum MotionRotate {
    /// A fixed angle, clockwise in degrees.
    Fixed {
        /// Degrees, clockwise.
        degrees: f64,
    },
    /// Along the path's direction, plus an angle.
    Auto {
        /// Degrees added to the path's direction.
        offset: f64,
    },
}

impl Default for MotionRotate {
    fn default() -> Self {
        MotionRotate::Fixed { degrees: 0.0 }
    }
}

/// How a group composes with what is under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[allow(missing_docs)] // CSS's blend modes and Porter-Duff operators, by their CSS names.
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    /// Adds the colors (Lottie's add, CSS `plus-lighter`).
    Add,
    /// Keeps what is under the group where the group is drawn (a mask's
    /// intersection).
    DestinationIn,
    /// Removes what is under the group where the group is drawn (a mask's
    /// subtraction).
    DestinationOut,
}

impl BlendMode {
    /// Whether it is [`BlendMode::Normal`].
    pub fn is_normal(&self) -> bool {
        matches!(self, BlendMode::Normal)
    }
}

/// Shapes outside which nothing shows: their union, or the area of their
/// outlines as one path ([`Clip::one_path`]).
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Clip {
    /// The shapes.
    pub shapes: Vec<ClipShape>,
    /// Whether the shapes' outlines are one path, filled under the first
    /// shape's rule (as Rive clips): two outlines even-odd make a ring, and
    /// outlines turning opposite ways cancel under the non-zero rule.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub one_path: bool,
}

/// One shape of a [`Clip`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ClipShape {
    /// The outline.
    pub geometry: Geometry,
    /// Its transform within the clip, outermost first.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub transform: Vec<TransformOp>,
    /// Which areas are inside.
    #[cfg_attr(feature = "serde", serde(default))]
    pub rule: FillRule,
}

/// Content whose alpha or luminance decides how much of a group shows.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Mask {
    /// Which of the content's channels masks.
    pub kind: MaskKind,
    /// Shows the group where the mask does not.
    #[cfg_attr(feature = "serde", serde(default))]
    pub inverted: bool,
    /// What masks, drawn in the masked group's space.
    pub content: Group,
}

/// Which of a mask's channels decides how much shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum MaskKind {
    /// Its opacity.
    #[default]
    Alpha,
    /// Its brightness times its opacity.
    Luminance,
}

/// Paths filled and stroked with one style.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Shape {
    /// The source file's id.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub id: Option<String>,
    /// The name its author gave it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub name: Option<String>,
    /// The outlines, filled and stroked together as one path.
    pub paths: Vec<ShapePath>,
    /// The fill.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub fill: Option<Fill>,
    /// The stroke.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub stroke: Option<Stroke>,
    /// Which of fill and stroke is drawn first.
    #[cfg_attr(feature = "serde", serde(default))]
    pub paint_order: PaintOrder,
    /// The part of the outlines drawn.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub trim: Option<Trim>,
}

/// One outline of a [`Shape`] and its transform within the shape.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ShapePath {
    /// The outline.
    pub geometry: Geometry,
    /// Moves its points (not the stroke), outermost first.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub transform: Vec<TransformOp>,
}

impl ShapePath {
    /// `geometry` without a transform.
    pub fn new(geometry: Geometry) -> Self {
        ShapePath {
            geometry,
            transform: Vec::new(),
        }
    }
}

/// An outline.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", tag = "type"))]
pub enum Geometry {
    /// Path data, morphing between keyframes of the same commands.
    Path {
        /// The path.
        path: Animated<Path>,
    },
    /// A rectangle, rounded when its radii are not zero.
    Rect {
        /// Its center.
        center: AnimatedPoint,
        /// Width and height.
        size: AnimatedPoint,
        /// Corner radii along x and y.
        radius: AnimatedPoint,
        /// Which way its outline runs.
        #[cfg_attr(feature = "serde", serde(default))]
        direction: Direction,
        /// Where its outline starts.
        #[cfg_attr(feature = "serde", serde(default))]
        start: RectStart,
    },
    /// An ellipse.
    Ellipse {
        /// Its center.
        center: AnimatedPoint,
        /// Radii along x and y.
        radius: AnimatedPoint,
        /// Which way its outline runs.
        #[cfg_attr(feature = "serde", serde(default))]
        direction: Direction,
        /// Where its outline starts.
        #[cfg_attr(feature = "serde", serde(default))]
        start: EllipseStart,
    },
    /// A star or a regular polygon (Lottie's polystar).
    Star(Star),
}

impl Geometry {
    /// A constant path.
    pub fn path(path: Path) -> Self {
        Geometry::Path {
            path: Animated::Static(path),
        }
    }

    /// Whether its outline changes over time.
    pub fn is_animated(&self) -> bool {
        match self {
            Geometry::Path { path } => path.is_animated(),
            Geometry::Rect {
                center,
                size,
                radius,
                ..
            } => center.is_animated() || size.is_animated() || radius.is_animated(),
            Geometry::Ellipse { center, radius, .. } => {
                center.is_animated() || radius.is_animated()
            }
            Geometry::Star(star) => {
                star.center.is_animated()
                    || star.points.is_animated()
                    || star.outer_radius.is_animated()
                    || star.inner_radius.is_animated()
                    || star.outer_roundness.is_animated()
                    || star.inner_roundness.is_animated()
                    || star.rotation.is_animated()
            }
        }
    }
}

/// Which way an outline runs; it decides where trimming starts and how
/// overlapping outlines fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum Direction {
    /// Clockwise on screen (y down).
    #[default]
    Clockwise,
    /// Counter-clockwise on screen.
    CounterClockwise,
}

/// Where a rectangle's outline starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum RectStart {
    /// The top edge after the top-left corner, as SVG draws `<rect>`.
    #[default]
    TopLeft,
    /// The right edge below the top-right corner, as Lottie draws its
    /// rectangles.
    TopRight,
}

/// Where an ellipse's outline starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum EllipseStart {
    /// The rightmost point, as SVG draws `<ellipse>` and `<circle>`.
    #[default]
    Right,
    /// The topmost point, as Lottie draws its ellipses.
    Top,
}

/// A star or a regular polygon.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Star {
    /// Star or polygon.
    pub kind: StarKind,
    /// Its center.
    pub center: AnimatedPoint,
    /// The number of points (or sides).
    pub points: Animated<f64>,
    /// The radius of the points.
    pub outer_radius: Animated<f64>,
    /// The radius of the valleys (stars only).
    pub inner_radius: Animated<f64>,
    /// How round the points are, 0 to 1.
    pub outer_roundness: Animated<f64>,
    /// How round the valleys are, 0 to 1 (stars only).
    pub inner_roundness: Animated<f64>,
    /// Clockwise degrees; 0 has a point at the top.
    pub rotation: Animated<f64>,
    /// Which way its outline runs.
    #[cfg_attr(feature = "serde", serde(default))]
    pub direction: Direction,
}

/// Whether a [`Star`] has valleys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum StarKind {
    /// Points and valleys.
    #[default]
    Star,
    /// A regular polygon.
    Polygon,
}

/// The inside of a shape.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Fill {
    /// The color or gradient.
    pub paint: Paint,
    /// 0 to 1, multiplied into the paint.
    #[cfg_attr(feature = "serde", serde(default = "opaque"))]
    pub opacity: Animated<f64>,
    /// Which areas are inside.
    #[cfg_attr(feature = "serde", serde(default))]
    pub rule: FillRule,
}

/// The outline of a shape.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Stroke {
    /// The color or gradient.
    pub paint: Paint,
    /// 0 to 1, multiplied into the paint.
    #[cfg_attr(feature = "serde", serde(default = "opaque"))]
    pub opacity: Animated<f64>,
    /// Line width.
    pub width: Animated<f64>,
    /// Line ends.
    #[cfg_attr(feature = "serde", serde(default))]
    pub cap: LineCap,
    /// Corners.
    #[cfg_attr(feature = "serde", serde(default))]
    pub join: LineJoin,
    /// Where miter joins turn into bevels.
    #[cfg_attr(feature = "serde", serde(default = "miter_limit"))]
    pub miter_limit: f64,
    /// Dashes.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub dash: Option<Dash>,
}

#[cfg(feature = "serde")]
fn miter_limit() -> f64 {
    4.0
}

/// A stroke's dashes: lengths of dash and gap in turn, and where the
/// pattern starts.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Dash {
    /// Dash and gap lengths in turn.
    pub array: Vec<Animated<f64>>,
    /// How far into the pattern the stroke starts.
    #[cfg_attr(feature = "serde", serde(default))]
    pub offset: Animated<f64>,
}

/// Which areas a path fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum FillRule {
    /// SVG `nonzero`.
    #[default]
    NonZero,
    /// SVG `evenodd`.
    EvenOdd,
}

/// Line ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum LineCap {
    /// Square, at the end.
    #[default]
    Butt,
    /// Round.
    Round,
    /// Square, half the width past the end.
    Square,
}

/// Corners of a stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum LineJoin {
    /// Pointed, up to the miter limit.
    #[default]
    Miter,
    /// Round.
    Round,
    /// Cut off.
    Bevel,
}

/// Which of a shape's fill and stroke is drawn first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum PaintOrder {
    /// The fill, then the stroke over it.
    #[default]
    FillFirst,
    /// The stroke, then the fill over it.
    StrokeFirst,
}

/// The part of a shape's outlines drawn, as fractions of their length.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Trim {
    /// Where the drawn part starts, 0 to 1.
    pub start: Animated<f64>,
    /// Where it ends, 0 to 1.
    pub end: Animated<f64>,
    /// Added to both, wrapping around (a full turn is 1).
    #[cfg_attr(feature = "serde", serde(default))]
    pub offset: Animated<f64>,
    /// Whether each outline is trimmed alike or all as one.
    #[cfg_attr(feature = "serde", serde(default))]
    pub mode: TrimMode,
}

/// How a [`Trim`] treats several outlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum TrimMode {
    /// Each outline is trimmed alike (Lottie's "simultaneously").
    #[default]
    EachPath,
    /// The outlines are trimmed as one line, one after another (Lottie's
    /// "individually").
    Combined,
}

/// A color or gradient.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", tag = "type"))]
pub enum Paint {
    /// One color.
    Solid {
        /// The color.
        color: Animated<Color>,
    },
    /// Colors along a line.
    LinearGradient(LinearGradient),
    /// Colors around a center.
    RadialGradient(RadialGradient),
    /// Content repeated in tiles.
    Pattern(Box<Pattern>),
}

impl Paint {
    /// A constant color.
    pub fn solid(color: Color) -> Self {
        Paint::Solid {
            color: Animated::Static(color),
        }
    }
}

/// Colors along a line from `start` to `end`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct LinearGradient {
    /// Where offset 0 is.
    pub start: AnimatedPoint,
    /// Where offset 1 is.
    pub end: AnimatedPoint,
    /// The colors.
    pub stops: Animated<Vec<GradientStop>>,
    /// The space of the points.
    #[cfg_attr(feature = "serde", serde(default))]
    pub units: GradientUnits,
    /// Applied to the gradient within its space.
    #[cfg_attr(feature = "serde", serde(default))]
    pub transform: Matrix,
    /// What is drawn past the ends.
    #[cfg_attr(feature = "serde", serde(default))]
    pub spread: Spread,
}

/// Colors from a focal point out to a circle.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct RadialGradient {
    /// The circle's center.
    pub center: AnimatedPoint,
    /// Its radius: where offset 1 is.
    pub radius: Animated<f64>,
    /// Where offset 0 is (the center by default).
    pub focal: AnimatedPoint,
    /// The colors.
    pub stops: Animated<Vec<GradientStop>>,
    /// The space of the points.
    #[cfg_attr(feature = "serde", serde(default))]
    pub units: GradientUnits,
    /// Applied to the gradient within its space.
    #[cfg_attr(feature = "serde", serde(default))]
    pub transform: Matrix,
    /// What is drawn past the circle.
    #[cfg_attr(feature = "serde", serde(default))]
    pub spread: Spread,
}

/// Content repeated in tiles (SVG's `<pattern>`): tiles of `tile`'s size
/// side by side from `tile`'s corner, each drawing `content`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Pattern {
    /// One tile: its corner and size, in `units`.
    pub tile: Rect,
    /// The space of `tile`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub units: GradientUnits,
    /// The space of `content` (without a view box): the shape's space or
    /// fractions of the shape's bounding box, from the tile's corner.
    #[cfg_attr(feature = "serde", serde(default))]
    pub content_units: GradientUnits,
    /// The content's coordinates placed in each tile, when given.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub view_box: Option<ViewBox>,
    /// Applied to the tiles within their space.
    #[cfg_attr(feature = "serde", serde(default))]
    pub transform: Matrix,
    /// What each tile draws.
    pub content: Group,
}

/// A rectangle of coordinates placed in a viewport (SVG's `viewBox` and
/// `preserveAspectRatio`).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ViewBox {
    /// The coordinates shown.
    pub rect: Rect,
    /// Where it sits in the room left along each axis (0 start, 0.5 middle,
    /// 1 end), scaled alike along both; `None` stretches it to fill.
    pub align: Option<(f64, f64)>,
    /// Scaled to cover the viewport (cut), not to fit in it.
    #[cfg_attr(feature = "serde", serde(default))]
    pub slice: bool,
}

impl ViewBox {
    /// The matrix placing it in a viewport of `width` × `height` from the
    /// origin.
    pub fn matrix(&self, width: f64, height: f64) -> Matrix {
        let vb = self.rect;
        if vb.width <= 0.0 || vb.height <= 0.0 {
            return Matrix::IDENTITY;
        }
        let (sx, sy) = (width / vb.width, height / vb.height);
        let Some((ax, ay)) = self.align else {
            return Matrix::new(sx, 0.0, 0.0, sy, -vb.x * sx, -vb.y * sy);
        };
        let s = if self.slice { sx.max(sy) } else { sx.min(sy) };
        let (free_x, free_y) = (width - vb.width * s, height - vb.height * s);
        Matrix::new(
            s,
            0.0,
            0.0,
            s,
            free_x * ax - vb.x * s,
            free_y * ay - vb.y * s,
        )
    }
}

/// A gradient's color at an offset.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct GradientStop {
    /// 0 to 1.
    pub offset: f64,
    /// The color there.
    pub color: Color,
}

impl crate::Interpolate for GradientStop {
    fn interpolate(&self, to: &Self, t: f64) -> Self {
        GradientStop {
            offset: self.offset + (to.offset - self.offset) * t,
            color: self.color.lerp(to.color, t),
        }
    }
}

/// The space of a gradient's points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum GradientUnits {
    /// The shape's space.
    #[default]
    UserSpace,
    /// Fractions of the shape's bounding box.
    BoundingBox,
}

/// What a gradient draws past its ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum Spread {
    /// The end colors.
    #[default]
    Pad,
    /// The gradient again.
    Repeat,
    /// The gradient again, mirrored.
    Reflect,
}

/// Text set in a font by whatever draws it (SVG's `<text>`): spans placed
/// on baselines, one after another unless placed anew.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Text {
    /// The source file's id.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub id: Option<String>,
    /// The name its author gave it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub name: Option<String>,
    /// The spans, in order.
    pub spans: Vec<TextSpan>,
}

/// A run of text in one style.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct TextSpan {
    /// The characters.
    pub text: String,
    /// Where it starts a new line of text (a point of the baseline, placed
    /// by `anchor`); `None` goes on after the span before it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub position: Option<AnimatedPoint>,
    /// Moved by, from where it would start.
    #[cfg_attr(feature = "serde", serde(default))]
    pub shift: Point,
    /// Which point of the line (the spans from one placed anew to the
    /// next) its position is.
    #[cfg_attr(feature = "serde", serde(default))]
    pub anchor: TextAnchor,
    /// The font.
    pub font: Font,
    /// Added after each character.
    #[cfg_attr(feature = "serde", serde(default))]
    pub letter_spacing: Animated<f64>,
    /// The fill.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub fill: Option<Fill>,
    /// The stroke.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub stroke: Option<Stroke>,
    /// Which of fill and stroke is drawn first.
    #[cfg_attr(feature = "serde", serde(default))]
    pub paint_order: PaintOrder,
}

/// Which point of a line of text its position is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum TextAnchor {
    /// Its start.
    #[default]
    Start,
    /// Its middle.
    Middle,
    /// Its end.
    End,
}

/// A font: the first of `families` the renderer has, else its default.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Font {
    /// Family names, the preferred first; CSS's generic names
    /// (`sans-serif`, `serif`, `monospace`, ...) stand for the renderer's.
    pub families: Vec<String>,
    /// The size (the em), in user units.
    pub size: Animated<f64>,
    /// The weight, 100 to 900 (400 regular, 700 bold).
    #[cfg_attr(feature = "serde", serde(default = "regular"))]
    pub weight: u16,
    /// Italic or oblique.
    #[cfg_attr(feature = "serde", serde(default))]
    pub italic: bool,
}

#[cfg(feature = "serde")]
fn regular() -> u16 {
    400
}

impl Default for Font {
    fn default() -> Self {
        Font {
            families: vec!["sans-serif".to_owned()],
            size: Animated::Static(16.0),
            weight: 400,
            italic: false,
        }
    }
}

/// A raster image, drawn into `rect`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Image {
    /// The source file's id.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub id: Option<String>,
    /// The name its author gave it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub name: Option<String>,
    /// The encoded image.
    pub data: Arc<ImageData>,
    /// Where it is drawn, stretched to fill.
    pub rect: Rect,
}

/// An encoded image (PNG, JPEG, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct ImageData {
    /// Its media type (`image/png`).
    pub mime: String,
    /// The encoded bytes.
    pub bytes: Vec<u8>,
}

#[cfg(feature = "serde")]
impl serde::Serialize for ImageData {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use base64::Engine;
        s.collect_str(&format_args!(
            "data:{};base64,{}",
            self.mime,
            base64::engine::general_purpose::STANDARD.encode(&self.bytes)
        ))
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ImageData {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use base64::Engine;
        let text = String::deserialize(d)?;
        let invalid = || serde::de::Error::custom("expected a base64 data: URI");
        let rest = text.strip_prefix("data:").ok_or_else(invalid)?;
        let (mime, data) = rest.split_once(";base64,").ok_or_else(invalid)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| invalid())?;
        Ok(ImageData {
            mime: mime.to_owned(),
            bytes,
        })
    }
}
