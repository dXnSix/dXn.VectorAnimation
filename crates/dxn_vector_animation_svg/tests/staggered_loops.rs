//! Animations that repeat forever after different delays (a staggered
//! loader) play their delays once, as a browser plays them: the timeline's
//! loop starts where the last of them has started, so the loop is seamless.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Import, LoopMode, Node};

fn read(body: &str) -> Import {
    let svg =
        format!(r#"<svg width="30" height="10" xmlns="http://www.w3.org/2000/svg">{body}</svg>"#);
    dxn_vector_animation_svg::read(svg.as_bytes()).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Opacity of the group `id` at `t` of the timeline.
fn opacity(import: &Import, id: &str, t: f64) -> f64 {
    import
        .scene
        .root
        .children
        .iter()
        .find_map(|n| match n {
            Node::Group(g) if g.id.as_deref() == Some(id) => Some(g.opacity.sample(t)),
            _ => None,
        })
        .unwrap()
}

/// What a browser shows `elapsed` seconds in: each dot fades in over 1 s,
/// over and over, from its delay on; before it, its own opacity, 1.
fn browser(delay: f64, elapsed: f64) -> f64 {
    if elapsed < delay {
        1.0
    } else {
        (elapsed - delay) % 1.0
    }
}

/// Whether the timeline played from its start shows what a browser shows
/// `elapsed` seconds in.
fn check(import: &Import, elapsed: f64) {
    let t = import.scene.timeline.time_at(elapsed);
    for (id, delay) in DOTS {
        let (got, want) = (opacity(import, id, t), browser(delay, elapsed));
        assert!(
            (got - want).abs() < 1e-3,
            "#{id} at {elapsed} s (timeline {t} s): {got}, a browser {want}"
        );
    }
}

const DOTS: [(&str, f64); 3] = [("a", 0.0), ("b", 0.2), ("c", 0.4)];

fn loader() -> Import {
    read(
        r#"<style>
#a { animation: k 1s linear infinite }
#b { animation: k 1s linear 0.2s infinite }
#c { animation: k 1s linear 0.4s infinite }
@keyframes k { from { opacity: 0 } to { opacity: 1 } }
</style>
<g id="a"><rect width="5" height="5"/></g>
<g id="b"><rect x="10" width="5" height="5"/></g>
<g id="c"><rect x="20" width="5" height="5"/></g>"#,
    )
}

#[test]
fn a_staggered_loader_loops_from_its_last_delay() {
    let import = loader();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let timeline = &import.scene.timeline;
    assert_eq!(timeline.loop_mode, LoopMode::Loop);
    assert!(close(timeline.loop_start, 0.4), "{}", timeline.loop_start);
    assert!(close(timeline.duration, 1.4), "{}", timeline.duration);
}

#[test]
fn played_on_it_shows_what_a_browser_shows() {
    let import = loader();
    // Over five loops, off the frame grid so no sample sits on a jump.
    for step in 0..500 {
        check(&import, f64::from(step) * 0.01 + 0.003);
    }
}

#[test]
fn the_loop_is_seamless() {
    // Just before and just after each time the timeline starts over.
    let import = loader();
    let timeline = &import.scene.timeline;
    let length = timeline.duration - timeline.loop_start;
    for k in 0..4 {
        let at = timeline.duration + f64::from(k) * length;
        check(&import, at - 1e-4);
        check(&import, at + 1e-4);
    }
}

#[test]
fn smil_begin_offsets_play_once_too() {
    let import = read(
        r#"<g id="a"><rect width="5" height="5"><animate attributeName="width" from="0" to="5" dur="1s" repeatCount="indefinite"/></rect></g>
<g id="b"><rect x="10" width="5" height="5"><animate attributeName="width" from="0" to="5" dur="1s" begin="0.3s" repeatCount="indefinite"/></rect></g>"#,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let timeline = &import.scene.timeline;
    assert!(close(timeline.loop_start, 0.3), "{}", timeline.loop_start);
    assert!(close(timeline.duration, 1.3), "{}", timeline.duration);
}

#[test]
fn without_delays_the_whole_timeline_loops() {
    let import = read(
        r#"<style>#a { animation: k 1s linear infinite } @keyframes k { to { opacity: 0 } }</style><g id="a"><rect width="5" height="5"/></g>"#,
    );
    assert_eq!(import.scene.timeline.loop_start, 0.0);
    assert!(close(import.scene.timeline.duration, 1.0));
}
