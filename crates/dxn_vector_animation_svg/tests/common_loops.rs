//! Animations that repeat forever at different rates share one loop, as
//! long as it takes them to come round together (within the limits), so
//! each plays whole iterations as a browser plays it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Group, Import, LoopMode, Node};

/// A document whose groups `#a` and `#b` animate as `css` says (keyframes
/// `k`, opacity 0 to 1), with `smil` inside `#b`'s rectangle.
fn read(css: &str, smil: &str) -> Import {
    let svg = format!(
        r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg"><style>{css} @keyframes k {{ from {{ opacity: 0 }} to {{ opacity: 1 }} }}</style><g id="a"><rect width="1" height="1"/></g><g id="b"><rect id="r" width="1" height="1">{smil}</rect></g></svg>"#
    );
    dxn_vector_animation_svg::read(svg.as_bytes()).unwrap()
}

fn group<'s>(import: &'s Import, id: &str) -> &'s Group {
    import
        .scene
        .root
        .children
        .iter()
        .find_map(|n| match n {
            Node::Group(g) if g.id.as_deref() == Some(id) => Some(g),
            _ => None,
        })
        .unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn duration(import: &Import) -> f64 {
    import.scene.timeline.duration
}

#[test]
fn one_second_and_a_second_and_a_half_loop_at_three() {
    let import = read(
        "#a { animation: k 1s linear infinite } #b { animation: k 1.5s linear infinite }",
        "",
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(import.scene.timeline.loop_mode, LoopMode::Loop);
    assert!(close(duration(&import), 3.0), "{}", duration(&import));
    let (a, b) = (&group(&import, "a").opacity, &group(&import, "b").opacity);
    // Three whole iterations of one, two of the other.
    assert!(close(a.sample(2.5), 0.5));
    assert!(close(b.sample(2.25), 0.5), "{}", b.sample(2.25));
    assert!(close(b.sample(2.999_999), 1.0));
}

#[test]
fn alternating_ones_count_both_ways() {
    // Periods of 2 s (there and back) and 1.5 s.
    let import = read(
        "#a { animation: k 1s linear infinite alternate } #b { animation: k 1.5s linear infinite }",
        "",
    );
    assert!(close(duration(&import), 6.0), "{}", duration(&import));
}

#[test]
fn smil_repeats_join_the_loop() {
    let import = read(
        "#a { animation: k 1s linear infinite }",
        r#"<animate attributeName="width" from="0" to="10" dur="1.5s" repeatCount="indefinite"/>"#,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert!(close(duration(&import), 3.0), "{}", duration(&import));
    let only_smil = read(
        "",
        r#"<animate attributeName="width" from="0" to="10" dur="1s" repeatCount="indefinite"/><animate attributeName="height" from="0" to="10" dur="0.4s" repeatCount="indefinite"/>"#,
    );
    assert!(close(duration(&only_smil), 2.0), "{}", duration(&only_smil));
}

#[test]
fn what_ends_plays_once_before_the_loop() {
    // A 2.5 s animation once, and a 1 s one forever: the loop starts once
    // the first has ended and lasts one period.
    let import = read(
        "#a { animation: k 1s linear infinite } #b { animation: k 2.5s linear }",
        "",
    );
    assert!(close(import.scene.timeline.loop_start, 2.5));
    assert!(close(duration(&import), 3.5), "{}", duration(&import));
}

#[test]
fn the_loop_starts_from_the_last_delay() {
    // As before: the delay, then one iteration, over and over.
    let import = read("#a { animation: k 2s linear 0.5s infinite }", "");
    assert!(close(duration(&import), 2.5), "{}", duration(&import));
    let both = read(
        "#a { animation: k 1s linear 0.5s infinite } #b { animation: k 1.5s linear 0.5s infinite }",
        "",
    );
    assert!(close(duration(&both), 3.5), "{}", duration(&both));
}

#[test]
fn equal_durations_keep_one_iteration() {
    let import = read(
        "#a { animation: k 2s linear infinite } #b { animation: k 2s ease infinite }",
        "",
    );
    assert!(close(duration(&import), 2.0), "{}", duration(&import));
}

#[test]
fn rates_that_never_meet_within_the_limits_keep_the_longest_with_a_warning() {
    // They come round together after 10,001 s, past an hour.
    let import = read(
        "#a { animation: k 1s linear infinite } #b { animation: k 1.0001s linear infinite }",
        "",
    );
    assert!(close(duration(&import), 1.0001), "{}", duration(&import));
    assert!(
        import
            .warnings
            .iter()
            .any(|w| w.contains("1 s to 1.0001 s come round together only past 3600 s")),
        "{:?}",
        import.warnings
    );
}
