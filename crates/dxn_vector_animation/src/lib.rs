//! Animated vector graphics, independent of any renderer or UI framework.
//!
//! A [`Scene`] is a tree of [`Group`]s, [`Shape`]s and [`Image`]s whose
//! properties are [`Animated`]: constants, or [`Track`]s of [`Keyframe`]s
//! with an [`Easing`] per segment, on one [`Timeline`]. Importers
//! (`dxn_vector_animation_svg`, `dxn_vector_animation_lottie`,
//! `dxn_vector_animation_rive`) read files into it. A renderer either samples it ([`Scene::frame_at`] gives a [`Frame`],
//! a flat display list) or reads the tracks and drives them with an
//! animation system of its own.
//!
//! Coordinates are user units with y growing downwards; angles are degrees,
//! clockwise; times are seconds.

mod animated;
mod color;
#[cfg(feature = "serde")]
pub mod compiled;
mod easing;
mod events;
pub mod filter;
mod frame;
mod geom;
mod hit;
mod limits;
pub mod outline;
mod scene;
pub mod transform;

pub use animated::{Animated, AnimatedPoint, Interpolate, Keyframe, Track};
pub use color::Color;
pub use easing::{Easing, StepPosition};
pub use events::EVENT_PREFIX;
pub use frame::{
    ClipPath, Frame, FrameEffect, FrameMask, FramePaint, FrameTextSpan, Item, Layer, StrokeStyle,
    text_bounds,
};
pub use geom::{Matrix, Path, PathEl, Point, Rect};
pub use limits::{Error, Import, Limits, Stats};
pub use scene::{
    BlendMode, Blur, Clip, ClipShape, ColorEffect, ColorMatrix, ColorOp, Dash, Direction,
    DropShadow, Effect, EllipseStart, Fill, FillRule, Font, Geometry, GradientStop, GradientUnits,
    Group, Image, ImageData, InnerShadow, LineCap, LineJoin, LinearGradient, LoopMode, Marker,
    Mask, MaskKind, MotionPath, MotionRotate, Node, Paint, PaintOrder, Pattern, RadialGradient,
    RectStart, Scene, Shape, ShapePath, Spread, Star, StarKind, Stroke, Text, TextAnchor, TextSpan,
    TimeMap, TimeRange, Timeline, Transfer, TransformOp, Trim, TrimMode, ViewBox,
};

/// The name and version of the compiled scene format
/// (`.vector-animation.json`).
pub const FORMAT: &str = "dxn-vector-animation";
/// The compiled scene format's version.
pub const FORMAT_VERSION: u32 = 1;
