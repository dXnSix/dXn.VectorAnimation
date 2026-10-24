//! Animated SVG and Lottie in NSH apps: the `VectorAnimation` component.
//!
//! A file is read with [dXn.VectorAnimation](https://github.com/dXnSix/dXn.VectorAnimation)
//! into a scene, the scene compiled into canvases whose numbers and colors
//! are nodes of NSH's native animation graph ([`plan`], [`graph`]), and
//! playback moves one time value: the UI thread plays the whole animation
//! with no JavaScript and no engine work per frame. A part the graph cannot
//! express is sampled frame by frame on the engine thread instead, in a
//! canvas of its own beside the rest.

mod component;
pub mod graph;
mod lower;
pub mod plan;
pub mod playback;
pub mod source;
mod span;

use nsh_core::ui::prelude::*;

pub use crate::{
    component::{
        AnimationInfo, AnimationMarker, AnimationSize, ResizeMode, VectorAnimation,
        VectorAnimationApi, VectorAnimationElement, VectorAnimationEvent, VectorAnimationProps,
        fit, vector_animation,
    },
    playback::Direction,
    source::{At, PosterFrame, Repeat, Segment, Source},
};

/// The crate's UI set: what TSX imports from `dxn-vector-animation`.
#[native_module("dxn-vector-animation")]
fn native(registry: &mut Registry) {
    registry.register::<VectorAnimation>();
}
