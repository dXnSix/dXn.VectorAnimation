//! The scene: a tree of groups, shapes and images whose properties are
//! constants or keyframe tracks on one timeline.

use std::sync::Arc;

use crate::{Animated, AnimatedPoint, Color, Matrix, Path, Rect};

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
    /// How it repeats.
    #[cfg_attr(feature = "serde", serde(default))]
    pub loop_mode: LoopMode,
    /// Named times or spans.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub markers: Vec<Marker>,
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

/// A named time or span of the timeline.
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
    /// Drop shadows and blurs of what the group draws (before its clip and
    /// mask), in the group's space: blurs in turn, and consecutive drop
    /// shadows all cast by the drawing as it is before them, the first
    /// lowest, under it.
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

/// Shapes outside which nothing shows: their union.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Clip {
    /// The shapes.
    pub shapes: Vec<ClipShape>,
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
