//! Infinite `alternate` CSS animations loop there and back, as a browser
//! plays them: the timeline holds a forward and a backward iteration.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Group, LoopMode, Node, Scene};

fn read(css_animation: &str) -> Scene {
    let svg = format!(
        r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg"><style>#a {{ animation: {css_animation} }} @keyframes k {{ from {{ opacity: 0 }} to {{ opacity: 1 }} }}</style><g id="a"><rect width="1" height="1"/></g></svg>"#
    );
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    import.scene
}

fn group(scene: &Scene) -> &Group {
    scene
        .root
        .children
        .iter()
        .find_map(|n| match n {
            Node::Group(g) if g.id.as_deref() == Some("a") => Some(g),
            _ => None,
        })
        .unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn alternate_plays_forwards_then_backwards_in_the_loop() {
    let scene = read("k 1s linear infinite alternate");
    assert_eq!(scene.timeline.loop_mode, LoopMode::Loop);
    assert!(
        close(scene.timeline.duration, 2.0),
        "{}",
        scene.timeline.duration
    );
    let opacity = &group(&scene).opacity;
    assert!(close(opacity.sample(0.25), 0.25));
    assert!(close(opacity.sample(1.0), 1.0));
    assert!(
        close(opacity.sample(1.25), 0.75),
        "the second iteration goes back"
    );
    // Back where it started when the loop starts over: no jump.
    assert!(close(opacity.sample(2.0), opacity.sample(0.0)));
}

#[test]
fn alternate_reverse_starts_backwards() {
    let scene = read("k 1s linear infinite alternate-reverse");
    assert!(
        close(scene.timeline.duration, 2.0),
        "{}",
        scene.timeline.duration
    );
    let opacity = &group(&scene).opacity;
    assert!(close(opacity.sample(0.25), 0.75));
    assert!(close(opacity.sample(1.25), 0.25));
    assert!(close(opacity.sample(2.0), opacity.sample(0.0)));
}

#[test]
fn a_negative_delay_starts_part_way_and_still_meets_itself() {
    let scene = read("k 1s linear -0.5s infinite alternate");
    assert!(
        close(scene.timeline.duration, 2.0),
        "{}",
        scene.timeline.duration
    );
    let opacity = &group(&scene).opacity;
    assert!(close(opacity.sample(0.0), 0.5));
    assert!(close(opacity.sample(0.75), 0.75), "on its way back");
    assert!(close(opacity.sample(2.0), opacity.sample(0.0)));
}

#[test]
fn eased_iterations_play_their_easing_backwards() {
    let scene = read("k 1s ease-in infinite alternate");
    let opacity = &group(&scene).opacity;
    // Backwards, ease-in reads as ease-out: the same values mirrored.
    for t in [0.1, 0.3, 0.6, 0.9] {
        assert!(
            close(opacity.sample(2.0 - t), opacity.sample(t)),
            "{t}: {} {}",
            opacity.sample(2.0 - t),
            opacity.sample(t)
        );
    }
}
