//! Colors mixing between keyframes.

use dxn_vector_animation::Color;

#[test]
fn different_alphas_mix_premultiplied() {
    let mixed = Color::rgba(1.0, 0.0, 0.0, 1.0).lerp(Color::rgba(0.0, 0.0, 1.0, 0.0), 0.5);
    assert_eq!(mixed, Color::rgba(1.0, 0.0, 0.0, 0.5));
}

#[test]
fn equal_alphas_mix_channel_by_channel() {
    let mixed = Color::rgba(1.0, 0.0, 0.0, 0.5).lerp(Color::rgba(0.0, 0.0, 1.0, 0.5), 0.25);
    assert_eq!(mixed, Color::rgba(0.75, 0.0, 0.25, 0.5));
}

#[test]
fn transparent_colors_keep_their_color() {
    // A gradient stop fading to transparent white stays white throughout.
    let white = Color::rgba(1.0, 1.0, 1.0, 0.0);
    assert_eq!(white.lerp(white, 0.4), white);
    let mixed = Color::rgba(1.0, 1.0, 1.0, 0.0).lerp(Color::rgba(0.0, 0.0, 0.0, 0.0), 0.5);
    assert_eq!(mixed, Color::rgba(0.5, 0.5, 0.5, 0.0));
}
