//! SMIL animation: what Figma Motion exports use (`width`, `height`, `d`)
//! and what animated SVGs in the wild use (transforms about a center,
//! begins chained in a cycle, negative begins, motion along a path).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Item, LoopMode, Point, Scene, outline::path_length};

fn read(svg: &str) -> (Scene, Vec<String>) {
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    (import.scene, import.warnings)
}

/// The fill items' path bounds at `t`, in scene coordinates.
fn bounds(scene: &Scene, t: f64) -> Vec<(f64, f64, f64, f64)> {
    scene
        .frame_at(t)
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Fill {
                path, transform, ..
            } => {
                let b = path.transformed(*transform).bounds()?;
                Some((b.x, b.y, b.width, b.height))
            }
            _ => None,
        })
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn width_and_height_as_figma_writes_them() {
    let (scene, warnings) = read(
        r#"<svg width="200" height="100" xmlns="http://www.w3.org/2000/svg">
        <rect x="10" y="10" width="20" height="10" fill="red">
          <animate attributeName="width" values="20; 20; 100; 100" keyTimes="0; 0.25; 0.75; 1" dur="4s" calcMode="linear" repeatCount="indefinite"/>
          <animate attributeName="height" values="10; 10; 50; 50" keyTimes="0; 0.25; 0.75; 1" dur="4s" calcMode="linear" repeatCount="indefinite"/>
        </rect></svg>"#,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(scene.timeline.duration, 4.0);
    assert_eq!(scene.timeline.loop_mode, LoopMode::Loop);
    let (x, y, w, h) = bounds(&scene, 2.0)[0];
    assert!(
        close(x, 10.0) && close(y, 10.0),
        "the top left stays: {x} {y}"
    );
    assert!(close(w, 60.0) && close(h, 30.0), "{w} {h}");
}

#[test]
fn path_morphs_along_key_splines() {
    let (scene, _) = read(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <path d="M0 0L10 0L10 10Z" fill="red">
          <animate attributeName="d" values="M0 0L10 0L10 10Z; M0 0L50 0L50 50Z" keyTimes="0; 1" keySplines="0.5 0 0.5 1" calcMode="spline" dur="2s" repeatCount="indefinite"/>
        </path></svg>"#,
    );
    // The spline is symmetric: halfway at half time.
    assert!(close(bounds(&scene, 1.0)[0].2, 30.0));
    assert!(bounds(&scene, 0.5)[0].2 < 10.0 + 40.0 * 0.25, "eased in");
}

#[test]
fn rotation_about_a_center() {
    let (scene, _) = read(
        r#"<svg width="24" height="24" xmlns="http://www.w3.org/2000/svg">
        <rect x="11" y="0" width="2" height="4" fill="red">
          <animateTransform attributeName="transform" type="rotate" values="0 12 12;360 12 12" dur="2s" repeatCount="indefinite"/>
        </rect></svg>"#,
    );
    let Item::Fill { transform, .. } = &scene.frame_at(0.5).items[0] else {
        panic!()
    };
    // A quarter turn about (12, 12): the top (12, 0) goes to the right.
    let p = transform.apply(Point::new(12.0, 0.0));
    assert!(close(p.x, 24.0) && close(p.y, 12.0), "{p:?}");
}

#[test]
fn negative_begins_start_part_way() {
    let (scene, _) = read(
        r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg">
        <rect width="10" height="10" fill="red" opacity="1">
          <animate attributeName="opacity" values="0;1" dur="1s" begin="-0.25s" repeatCount="indefinite"/>
        </rect></svg>"#,
    );
    let opacity = |t: f64| match &scene.frame_at(t).items[0] {
        Item::PushLayer(l) => l.opacity,
        _ => 1.0,
    };
    assert!(close(opacity(0.0), 0.25), "{}", opacity(0.0));
    assert!(close(opacity(0.5), 0.75));
}

#[test]
fn chained_begins_loop_as_a_cycle() {
    // Three moves, each starting when the one before ends; the first also
    // at 0: a cycle of 0.6 s, each holding where it moved.
    let (scene, warnings) = read(
        r#"<svg width="30" height="10" xmlns="http://www.w3.org/2000/svg">
        <rect width="5" height="5" fill="red">
          <animate id="a" begin="0;c.end" attributeName="x" dur="0.2s" values="0;10" fill="freeze"/>
          <animate id="b" begin="a.end" attributeName="y" dur="0.2s" values="0;5" fill="freeze"/>
          <animate id="c" begin="b.end" attributeName="x" dur="0.2s" values="10;0" fill="freeze"/>
        </rect></svg>"#,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        close(scene.timeline.duration, 0.6),
        "{}",
        scene.timeline.duration
    );
    assert_eq!(scene.timeline.loop_mode, LoopMode::Loop);
    let at = |t: f64| {
        let b = bounds(&scene, t)[0];
        (b.0, b.1)
    };
    assert!(close(at(0.1).0, 5.0));
    let (x, y) = at(0.3);
    assert!(close(x, 10.0) && close(y, 2.5), "{x} {y}");
    assert!(close(at(0.5).0, 5.0));
    // At the loop's start `y` shows what it froze at in the cycle before.
    assert!(close(at(0.05).1, 5.0), "{:?}", at(0.05));
}

#[test]
fn from_to_set_and_fill() {
    let (scene, _) = read(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <circle cx="10" cy="50" r="5" fill="red">
          <animate attributeName="cx" from="10" to="90" begin="1s" dur="1s" fill="freeze"/>
          <set attributeName="fill" to="blue" begin="1.5s"/>
        </circle></svg>"#,
    );
    assert_eq!(scene.timeline.loop_mode, LoopMode::Once);
    let cx = |t: f64| {
        let b = bounds(&scene, t)[0];
        b.0 + b.2 / 2.0
    };
    assert!(close(cx(0.5), 10.0));
    assert!(close(cx(1.5), 50.0));
    assert!(close(cx(3.0), 90.0), "frozen at the end");
    let color = |t: f64| match &scene.frame_at(t).items[0] {
        Item::Fill {
            paint: dxn_vector_animation::FramePaint::Solid(c),
            ..
        } => (c.r, c.b),
        _ => (0.0, 0.0),
    };
    assert_eq!(color(1.0), (1.0, 0.0));
    assert_eq!(color(1.6), (0.0, 1.0));
}

#[test]
fn motion_along_an_mpath_turning_with_it() {
    let (scene, _) = read(
        r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
        <path id="track" d="M10 10L90 10L90 90" fill="none"/>
        <rect x="-2" y="-1" width="4" height="2" fill="red">
          <animateMotion dur="2s" rotate="auto" repeatCount="indefinite"><mpath xlink:href="#track"/></animateMotion>
        </rect></svg>"##,
    );
    let Item::Fill { transform, .. } = &scene.frame_at(1.5).items[0] else {
        panic!()
    };
    // Three quarters of the length: half way down the second segment.
    let p = transform.apply(Point::ZERO);
    assert!(close(p.x, 90.0) && close(p.y, 50.0), "{p:?}");
    assert!(
        close(transform.apply_vector(Point::new(1.0, 0.0)).y, 1.0),
        "turned to face down"
    );
}

#[test]
fn motion_through_points_gives_each_segment_its_time() {
    let (scene, _) = read(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <rect width="2" height="2" fill="red">
          <animateMotion values="0,0; 10,0; 10,90" calcMode="linear" dur="2s" fill="freeze"/>
        </rect></svg>"#,
    );
    let at = |t: f64| bounds(&scene, t)[0];
    assert!(close(at(0.5).0, 5.0), "{:?}", at(0.5));
    assert!(close(at(1.5).1, 45.0), "{:?}", at(1.5));
}

#[test]
fn additive_transforms_follow_the_attribute() {
    let (scene, _) = read(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <rect width="2" height="2" fill="red" transform="translate(50 0)">
          <animateTransform attributeName="transform" type="translate" values="0 0; 0 40" dur="2s" additive="sum" repeatCount="indefinite"/>
        </rect></svg>"#,
    );
    let (x, y, _, _) = bounds(&scene, 1.0)[0];
    assert!(close(x, 50.0) && close(y, 20.0), "{x} {y}");
}

#[test]
fn css_wins_and_undrawn_targets_are_reported() {
    let (scene, warnings) = read(
        r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg">
        <style>#r { animation: k 1s linear infinite } @keyframes k { from { opacity: 0.5 } to { opacity: 0.5 } }</style>
        <defs><filter id="f"><feGaussianBlur stdDeviation="0"><animate attributeName="stdDeviation" values="0;5" dur="1s"/></feGaussianBlur></filter></defs>
        <rect id="r" width="10" height="10" fill="red">
          <animate attributeName="opacity" values="0;1" dur="1s" repeatCount="indefinite"/>
        </rect></svg>"#,
    );
    let Item::PushLayer(l) = &scene.frame_at(0.1).items[0] else {
        panic!()
    };
    assert!(close(l.opacity, 0.5), "the CSS animation of opacity wins");
    assert!(
        warnings.iter().any(|w| w.contains("stdDeviation")),
        "{warnings:?}"
    );
}

#[test]
fn trims_with_dashes() {
    // svg-spinners' ring: a dash growing along a circle.
    let (scene, _) = read(
        r#"<svg width="24" height="24" xmlns="http://www.w3.org/2000/svg">
        <circle cx="12" cy="12" r="9.5" fill="none" stroke="black" stroke-width="3">
          <animate attributeName="stroke-dasharray" dur="1.5s" calcMode="spline" values="0 150;42 150;42 150;42 150" keyTimes="0;0.475;0.95;1" keySplines="0.42,0,0.58,1;0.42,0,0.58,1;0.42,0,0.58,1" repeatCount="indefinite"/>
        </circle></svg>"#,
    );
    let dash = |t: f64| match &scene.frame_at(t).items[0] {
        Item::Stroke { style, .. } => style.dash.clone().map(|d| d.0[0]).unwrap_or(-1.0),
        _ => -1.0,
    };
    assert!(close(dash(1.0), 42.0));
    assert!(dash(0.1) > 0.0 && dash(0.1) < 42.0);
    let Item::Stroke { path, .. } = &scene.frame_at(1.0).items[0] else {
        panic!()
    };
    assert!(path_length(path) > 59.0);
}
