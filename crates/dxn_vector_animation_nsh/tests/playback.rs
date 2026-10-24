//! Where playback is after some time.

use dxn_vector_animation_nsh::{Direction, Repeat, playback::Playback};

fn playback(direction: Direction, repeat: Repeat) -> Playback {
    Playback {
        start: 1.0,
        end: 3.0,
        rate: 2.0,
        direction,
        repeat,
    }
}

#[test]
fn once_forwards_and_backwards() {
    let p = playback(Direction::Normal, Repeat::Once);
    assert_eq!(p.position(1.0, 0.5), (2.0, false));
    assert_eq!(p.position(1.0, 1.0), (3.0, true));
    let r = playback(Direction::Reverse, Repeat::Once);
    assert_eq!(r.position(3.0, 0.25), (2.5, false));
    assert_eq!(r.position(3.0, 2.0), (1.0, true));
}

#[test]
fn loops_resume_mid_way() {
    let p = playback(Direction::Normal, Repeat::Forever);
    // From 2: one second of timeline to the end, then runs from the start.
    assert_eq!(p.position(2.0, 0.25), (2.5, false));
    assert_eq!(p.position(2.0, 0.75), (1.5, false));
    let three = playback(Direction::Normal, Repeat::Times(3));
    assert!(!three.position(1.0, 2.9).1);
    assert_eq!(three.position(1.0, 3.0), (3.0, true));
}

#[test]
fn alternate_comes_back() {
    let p = playback(Direction::Alternate, Repeat::Times(2));
    assert_eq!(p.position(1.0, 1.5), (2.0, false), "back half way");
    assert_eq!(p.position(1.0, 2.0), (1.0, true));
    assert_eq!(p.last(), 1.0);
}
