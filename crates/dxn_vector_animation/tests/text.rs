//! Live text in frames: spans sampled at a moment with their paints, and
//! the box they are guessed to take without a font.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Easing, Fill, Font, Group, Item, Keyframe, LoopMode, Node,
    Paint, Point, Rect, Scene, Text, TextAnchor, TextSpan, Timeline, Track, text_bounds,
};

fn scene(spans: Vec<TextSpan>) -> Scene {
    Scene {
        width: 200.0,
        height: 100.0,
        timeline: Timeline {
            duration: 1.0,
            frame_rate: None,
            loop_mode: LoopMode::Once,
            markers: vec![],
        },
        root: Group {
            children: vec![Node::Text(Box::new(Text {
                id: None,
                name: None,
                spans,
            }))],
            ..Group::new()
        },
    }
}

fn span(text: &str, position: Option<(f64, f64)>, anchor: TextAnchor) -> TextSpan {
    TextSpan {
        text: text.to_owned(),
        position: position.map(|(x, y)| AnimatedPoint::fixed(x, y)),
        anchor,
        font: Font {
            size: Animated::Track(Track::new(vec![
                Keyframe::new(0.0, 10.0, Easing::Linear),
                Keyframe::new(1.0, 20.0, Easing::Linear),
            ])),
            ..Font::default()
        },
        fill: Some(Fill {
            paint: Paint::solid(Color::BLACK),
            opacity: Animated::Static(1.0),
            rule: Default::default(),
        }),
        ..TextSpan::default()
    }
}

#[test]
fn a_frame_holds_text_at_its_moment() {
    let s = scene(vec![
        span("ab", Some((10.0, 50.0)), TextAnchor::Start),
        span("cd", None, TextAnchor::Start),
    ]);
    let frame = s.frame_at(0.5);
    let [Item::Text { spans, .. }] = frame.items.as_slice() else {
        panic!("{:?}", frame.items);
    };
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].size, 15.0);
    assert_eq!(spans[0].position, Some(Point::new(10.0, 50.0)));
    assert!(spans[1].position.is_none());
    assert!(spans[0].fill.is_some());
    // Four characters of 0.6 em, from 0.9 em above the baseline.
    let b = text_bounds(spans).unwrap();
    assert!(
        (b.x - 10.0).abs() < 1e-9 && (b.width - 36.0).abs() < 1e-9,
        "{b:?}"
    );
    assert!((b.y - (50.0 - 13.5)).abs() < 1e-9);
}

#[test]
fn an_anchored_line_is_guessed_around_its_position() {
    let s = scene(vec![span("abcd", Some((100.0, 50.0)), TextAnchor::Middle)]);
    let frame = s.frame_at(0.0);
    let b = frame.bounds().unwrap();
    // 24 wide at size 10, centered on 100.
    assert_eq!(b, Rect::new(88.0, 41.0, 24.0, 12.0));
}
