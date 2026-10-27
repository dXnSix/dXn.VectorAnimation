//! Hit tests: whether a point is on what a group draws.

#![allow(clippy::unwrap_used)]

use dxn_vector_animation::{
    Animated, AnimatedPoint, Easing, Geometry, Group, HitIndex, Keyframe, Node, Point, Rect, Scene,
    Shape, ShapePath, Timeline, Track, TransformOp,
};

fn square(size: f64) -> Node {
    Node::shape(Shape {
        paths: vec![ShapePath::new(Geometry::Rect {
            center: AnimatedPoint::fixed(0.0, 0.0),
            size: AnimatedPoint::fixed(size, size),
            radius: AnimatedPoint::fixed(0.0, 0.0),
            direction: Default::default(),
            start: Default::default(),
        })],
        ..Default::default()
    })
}

/// A group "button" moving from x 20 to 80 over a second, holding a 10 × 10
/// square in a group of its own, under a group "panel" at y 50.
fn scene() -> Scene {
    let x = Animated::Track(Track::new(vec![
        Keyframe::new(0.0, 20.0, Easing::Linear),
        Keyframe::new(1.0, 80.0, Easing::Linear),
    ]));
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline::default(),
        root: Group {
            children: vec![Node::Group(Group {
                id: Some("panel".into()),
                transform: vec![TransformOp::translate(0.0, 50.0)],
                children: vec![Node::Group(Group {
                    id: Some("button".into()),
                    transform: vec![TransformOp::Translate {
                        x,
                        y: Animated::Static(0.0),
                    }],
                    children: vec![Node::Group(Group {
                        children: vec![square(10.0)],
                        ..Group::new()
                    })],
                    ..Group::new()
                })],
                ..Group::new()
            })],
            ..Group::new()
        },
    }
}

#[test]
fn a_point_on_a_groups_outline_hits_it_where_it_is_then() {
    let s = scene();
    assert!(s.hit("button", Point::new(22.0, 52.0), 0.0));
    assert!(!s.hit("button", Point::new(22.0, 52.0), 1.0));
    assert!(s.hit("button", Point::new(78.0, 48.0), 1.0));
    // Its parent holds it.
    assert!(s.hit("panel", Point::new(78.0, 48.0), 1.0));
    assert!(!s.hit("button", Point::new(50.0, 10.0), 0.5));
    assert!(!s.hit("nothing", Point::new(22.0, 52.0), 0.0));
}

#[test]
fn a_groups_hit_bounds_are_where_its_outlines_are_then() {
    let s = scene();
    assert_eq!(
        s.hit_bounds("button", 0.0),
        Some(Rect::new(15.0, 45.0, 10.0, 10.0))
    );
    assert_eq!(
        s.hit_bounds("button", 1.0),
        Some(Rect::new(75.0, 45.0, 10.0, 10.0))
    );
    assert_eq!(s.hit_bounds("nothing", 0.0), None);
}

#[test]
fn a_rectangle_hits_where_it_overlaps_a_groups_outlines() {
    let s = scene();
    // The button's square spans 15 to 25 across, 45 to 55 down, at 0 s.
    let around = |x: f64, y: f64| Rect::new(x - 2.0, y - 2.0, 4.0, 4.0);
    assert!(s.hit_rect("button", around(20.0, 50.0), 0.0));
    // Past its edge, by less than 2 and by more.
    assert!(s.hit_rect("button", around(26.5, 50.0), 0.0));
    assert!(!s.hit_rect("button", around(28.0, 50.0), 0.0));
    // A rectangle around all of it.
    assert!(s.hit_rect("button", Rect::new(0.0, 30.0, 50.0, 40.0), 0.0));
    assert!(!s.hit_rect("nothing", around(20.0, 50.0), 0.0));
}

#[test]
fn an_index_finds_the_groups_a_search_does() {
    let s = scene();
    let index = HitIndex::new(&s);
    let around = |x: f64, y: f64| Rect::new(x - 2.0, y - 2.0, 4.0, 4.0);
    for (id, x, y, t) in [
        ("button", 20.0, 50.0, 0.0),
        ("button", 28.0, 50.0, 0.0),
        ("button", 78.0, 48.0, 1.0),
        ("panel", 78.0, 48.0, 1.0),
        ("nothing", 20.0, 50.0, 0.0),
    ] {
        assert_eq!(
            s.hit_rect_indexed(&index, id, around(x, y), t),
            s.hit_rect(id, around(x, y), t),
            "{id} at {x}, {y}, {t} s"
        );
    }
    // The panel draws first, the button over it.
    assert!(index.order("panel") < index.order("button"));
    assert_eq!(index.order("nothing"), None);
}
