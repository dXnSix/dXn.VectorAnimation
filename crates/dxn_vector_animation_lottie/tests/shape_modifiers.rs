//! Shape modifiers: merge paths in each mode, round corners, pucker and
//! bloat, twist, zig zag and offset path change the outlines listed
//! before them in their group (nested groups' included), and the fills
//! and strokes after them draw what they leave; when anything moves, the
//! changed outline is sampled frame by frame into keyframes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, Easing, FillRule, Geometry, Group, Import, Item, Node, Path, PathEl, Scene, Shape,
};
use kurbo::Shape as _;

/// A shape layer of group items `items` (JSON) with no transform, 10
/// frames a second for two seconds.
fn read(items: &str) -> Import {
    let json = format!(
        r#"{{"v":"5.7.0","fr":10,"ip":0,"op":20,"w":200,"h":200,"layers":[
{{"ty":4,"ind":1,"ip":0,"op":20,"st":0,
 "ks":{{"o":{{"a":0,"k":100}},"p":{{"a":0,"k":[0,0]}},"a":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[100,100]}},"r":{{"a":0,"k":0}}}},
 "shapes":[{items}]}}]}}"#
    );
    let import = dxn_vector_animation_lottie::read(json.as_bytes()).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    import
}

fn still(v: f64) -> String {
    format!(r#"{{"a":0,"k":{v}}}"#)
}

/// A rectangle `w` × `h` about (`x`, `y`).
fn rect(x: f64, y: f64, w: f64, h: f64) -> String {
    format!(
        r#"{{"ty":"rc","p":{{"a":0,"k":[{x},{y}]}},"s":{{"a":0,"k":[{w},{h}]}},"r":{{"a":0,"k":0}}}}"#
    )
}

/// An ellipse `w` × `h` about (`x`, `y`).
fn ellipse(x: f64, y: f64, w: f64, h: f64) -> String {
    format!(r#"{{"ty":"el","p":{{"a":0,"k":[{x},{y}]}},"s":{{"a":0,"k":[{w},{h}]}}}}"#)
}

/// A fill by `rule` (1 non-zero, 2 even-odd).
fn fill(rule: u8) -> String {
    format!(r#"{{"ty":"fl","c":{{"a":0,"k":[1,0,0,1]}},"o":{{"a":0,"k":100}},"r":{rule}}}"#)
}

fn stroke() -> String {
    r#"{"ty":"st","c":{"a":0,"k":[0,0,1,1]},"o":{"a":0,"k":100},"w":{"a":0,"k":2},"lc":1,"lj":1,"ml":4}"#
        .into()
}

/// A group of `items` moved by (`x`, `y`).
fn group(items: &str, x: f64, y: f64) -> String {
    format!(
        r#"{{"ty":"gr","it":[{items},{{"ty":"tr","p":{{"a":0,"k":[{x},{y}]}},"a":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[100,100]}},"r":{{"a":0,"k":0}},"o":{{"a":0,"k":100}}}}]}}"#
    )
}

fn merge(mode: u8) -> String {
    format!(r#"{{"ty":"mm","mm":{mode}}}"#)
}

/// The fills of the frame at `t`: each path in the frame's space, and
/// its rule.
fn fills(scene: &Scene, t: f64) -> Vec<(kurbo::BezPath, FillRule)> {
    scene
        .frame_at(t)
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fill {
                path,
                transform,
                rule,
                ..
            } => Some((path.transformed(*transform).to_kurbo(), *rule)),
            _ => None,
        })
        .collect()
}

/// Whether any fill of the frame at `t` covers (`x`, `y`).
fn covered(scene: &Scene, t: f64, x: f64, y: f64) -> bool {
    fills(scene, t).iter().any(|(path, rule)| {
        let w = path.winding(kurbo::Point::new(x, y));
        match rule {
            FillRule::NonZero => w != 0,
            FillRule::EvenOdd => w % 2 != 0,
        }
    })
}

/// Which of `points` the frame at time 0 covers.
fn coverage(scene: &Scene, points: &[(f64, f64)]) -> Vec<bool> {
    points
        .iter()
        .map(|&(x, y)| covered(scene, 0.0, x, y))
        .collect()
}

/// The shapes of the scene, depth first.
fn shapes(scene: &Scene) -> Vec<&Shape> {
    fn walk<'s>(group: &'s Group, out: &mut Vec<&'s Shape>) {
        for child in &group.children {
            match child {
                Node::Group(g) => walk(g, out),
                Node::Shape(s) => out.push(s),
                Node::Image(_) | Node::Text(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&scene.root, &mut out);
    out
}

/// The one outline the scene's one fill draws, at time 0.
fn only_path(scene: &Scene) -> Path {
    let frame = scene.frame_at(0.0);
    let paths: Vec<&Path> = frame
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fill { path, .. } => Some(path),
            _ => None,
        })
        .collect();
    assert_eq!(paths.len(), 1, "{:?}", frame.items);
    paths[0].clone()
}

fn area(path: &Path) -> f64 {
    path.to_kurbo().area().abs()
}

// A square from (0, 0) to (20, 20) and a circle of radius 10 about
// (25, 10): points in the square only, in both, in the circle only, in
// neither.
const ONLY_SQUARE: (f64, f64) = (5.0, 10.0);
const BOTH: (f64, f64) = (17.5, 10.0);
const ONLY_CIRCLE: (f64, f64) = (30.0, 10.0);
const NEITHER: (f64, f64) = (40.0, 10.0);
const PROBES: [(f64, f64); 4] = [ONLY_SQUARE, BOTH, ONLY_CIRCLE, NEITHER];

fn merged(mode: u8, rule: u8) -> Scene {
    read(&format!(
        "{},{},{},{}",
        rect(10.0, 10.0, 20.0, 20.0),
        ellipse(25.0, 10.0, 20.0, 20.0),
        merge(mode),
        fill(rule)
    ))
    .scene
}

#[test]
fn merge_keeps_both_outlines_for_the_fill_rule() {
    let scene = merged(1, 1);
    assert_eq!(coverage(&scene, &PROBES), [true, true, true, false]);
    // Merged, not combined: even-odd leaves the overlap out, and the
    // curves stay curves.
    let scene = merged(1, 2);
    assert_eq!(coverage(&scene, &PROBES), [true, false, true, false]);
    let path = only_path(&scene);
    assert!(
        path.elements()
            .iter()
            .any(|e| matches!(e, PathEl::CubicTo(..)))
    );
}

#[test]
fn add_is_the_union() {
    for rule in [1, 2] {
        assert_eq!(
            coverage(&merged(2, rule), &PROBES),
            [true, true, true, false]
        );
    }
}

#[test]
fn subtract_takes_the_later_outlines_from_the_first() {
    assert_eq!(
        coverage(&merged(3, 1), &PROBES),
        [true, false, false, false]
    );
    // The circle first: the circle less the square.
    let scene = read(&format!(
        "{},{},{},{}",
        ellipse(25.0, 10.0, 20.0, 20.0),
        rect(10.0, 10.0, 20.0, 20.0),
        merge(3),
        fill(1)
    ))
    .scene;
    assert_eq!(coverage(&scene, &PROBES), [false, false, true, false]);
}

#[test]
fn intersect_keeps_where_they_overlap() {
    assert_eq!(
        coverage(&merged(4, 1), &PROBES),
        [false, true, false, false]
    );
}

#[test]
fn exclude_intersections_leaves_the_overlap_out() {
    for rule in [1, 2] {
        assert_eq!(
            coverage(&merged(5, rule), &PROBES),
            [true, false, true, false]
        );
    }
}

#[test]
fn a_merged_outline_is_smooth_at_large_scale() {
    // A circle of radius 1000 merged: no point of its polygon is more
    // than half a unit inside the circle.
    let scene = read(&format!(
        "{},{},{},{}",
        ellipse(0.0, 0.0, 2000.0, 2000.0),
        rect(5000.0, 0.0, 10.0, 10.0),
        merge(2),
        fill(1)
    ))
    .scene;
    let path = only_path(&scene);
    let mut worst: f64 = 0.0;
    let mut last = None;
    for el in path.elements() {
        if let PathEl::LineTo(p) | PathEl::MoveTo(p) = el {
            if let Some(q) = last
                && p.x.hypot(p.y) > 900.0
            {
                let mid = dxn_vector_animation::Point::lerp(q, *p, 0.5);
                worst = worst.max(1000.0 - mid.x.hypot(mid.y));
            }
            last = Some(*p);
        }
    }
    assert!(worst > 0.0 && worst < 0.5, "{worst}");
}

#[test]
fn a_merge_hides_the_paints_before_it_and_merges_nested_groups() {
    // The squares of two groups, one moved right by 10, intersected:
    // the fills inside the groups draw nothing, the fill after the merge
    // draws where both squares are.
    let scene = read(&format!(
        "{},{},{},{}",
        group(
            &format!("{},{}", rect(10.0, 10.0, 20.0, 20.0), fill(1)),
            0.0,
            0.0
        ),
        group(
            &format!("{},{}", rect(10.0, 10.0, 20.0, 20.0), fill(1)),
            10.0,
            0.0
        ),
        merge(4),
        fill(1)
    ))
    .scene;
    assert_eq!(fills(&scene, 0.0).len(), 1);
    assert_eq!(
        coverage(&scene, &[(5.0, 10.0), (15.0, 10.0), (25.0, 10.0)]),
        [false, true, false]
    );
}

#[test]
fn round_corners_round_a_rectangle() {
    let scene = read(&format!(
        r#"{},{{"ty":"rd","r":{}}},{}"#,
        rect(50.0, 50.0, 100.0, 100.0),
        still(10.0),
        fill(1)
    ))
    .scene;
    // The corner at (0, 0) is cut by a curve through about (2.9, 2.9).
    assert_eq!(
        coverage(&scene, &[(2.0, 2.0), (4.0, 4.0), (50.0, 0.5), (0.5, 50.0)]),
        [false, true, true, true]
    );
    let path = only_path(&scene);
    let curves = path
        .elements()
        .iter()
        .filter(|e| matches!(e, PathEl::CubicTo(..)))
        .count();
    assert_eq!(curves, 4);
}

#[test]
fn round_corners_round_a_polygon_path_but_not_its_curves() {
    // A triangle path: three corners, each two points joined by a curve.
    let triangle = r#"{"ty":"sh","ks":{"a":0,"k":{"c":true,"v":[[0,0],[100,0],[0,100]],"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]]}}}"#;
    let scene = read(&format!(
        r#"{triangle},{{"ty":"rd","r":{}}},{}"#,
        still(20.0),
        fill(1)
    ))
    .scene;
    assert_eq!(
        coverage(&scene, &[(1.0, 1.0), (10.0, 10.0), (97.0, 1.0)]),
        [false, true, false]
    );
    let path = only_path(&scene);
    // Three curves and three lines (the closing one included).
    assert_eq!(path.point_count(), 1 + 3 * 3 + 3, "{path}");
    // A polygon of the polystar rounds alike; an ellipse has no corners.
    let pentagon = r#"{"ty":"sr","sy":2,"p":{"a":0,"k":[50,50]},"pt":{"a":0,"k":5},"or":{"a":0,"k":40},"os":{"a":0,"k":0},"r":{"a":0,"k":0}}"#;
    let scene = read(&format!(
        r#"{pentagon},{{"ty":"rd","r":{}}},{}"#,
        still(10.0),
        fill(1)
    ))
    .scene;
    assert!(!covered(&scene, 0.0, 50.0, 10.5), "the top point is cut");
    assert!(covered(&scene, 0.0, 50.0, 20.0));
    let scene = read(&format!(
        r#"{},{{"ty":"rd","r":{}}},{}"#,
        ellipse(50.0, 50.0, 100.0, 100.0),
        still(10.0),
        fill(1)
    ))
    .scene;
    let path = only_path(&scene);
    assert_eq!(
        path.elements()
            .iter()
            .filter(|e| matches!(e, PathEl::CubicTo(..)))
            .count(),
        4
    );
}

#[test]
fn pucker_and_bloat_move_points_and_handles() {
    let square = rect(50.0, 50.0, 100.0, 100.0);
    let bloated = read(&format!(
        r#"{square},{{"ty":"pb","a":{}}},{}"#,
        still(50.0),
        fill(1)
    ))
    .scene;
    // The corners halfway in, the edges bulging out past the square.
    assert_eq!(
        coverage(&bloated, &[(5.0, 5.0), (50.0, -5.0), (50.0, 50.0)]),
        [false, true, true]
    );
    let puckered = read(&format!(
        r#"{square},{{"ty":"pb","a":{}}},{}"#,
        still(-50.0),
        fill(1)
    ))
    .scene;
    // The corners out, the edges pinched in.
    assert_eq!(
        coverage(&puckered, &[(50.0, 5.0), (50.0, 50.0)]),
        [false, true]
    );
    let b = only_path(&puckered).bounds().unwrap();
    assert!(
        (b.x + 25.0).abs() < 1e-9 && (b.width - 150.0).abs() < 1e-9,
        "{b:?}"
    );
}

#[test]
fn twist_turns_the_middle_more_than_the_ends() {
    // A bar along x, twisted a quarter turn about its middle.
    let scene = read(&format!(
        r#"{},{{"ty":"tw","a":{},"c":{{"a":0,"k":[0,0]}}}},{}"#,
        rect(0.0, 0.0, 100.0, 10.0),
        still(90.0),
        fill(1)
    ))
    .scene;
    // 20 from the middle it turns about 54° (clockwise on screen); its
    // ends hardly turn.
    let a = 90.0_f64 * (1.0 - 20.0 / 50.25);
    let (sin, cos) = a.to_radians().sin_cos();
    assert_eq!(
        coverage(
            &scene,
            &[(20.0 * cos, 20.0 * sin), (20.0, 0.0), (49.0, 0.0)]
        ),
        [true, false, true]
    );
    // The straight edges bend.
    let path = only_path(&scene);
    assert!(
        path.elements()
            .iter()
            .filter(|e| matches!(e, PathEl::CubicTo(..)))
            .count()
            > 8
    );
}

#[test]
fn zig_zag_has_its_ridges_and_size() {
    let zz = |pt: u8| {
        format!(
            r#"{{"ty":"zz","s":{},"r":{},"pt":{}}}"#,
            still(10.0),
            still(3.0),
            still(f64::from(pt))
        )
    };
    let scene = read(&format!(
        "{},{},{}",
        rect(50.0, 50.0, 100.0, 100.0),
        zz(1),
        fill(1)
    ))
    .scene;
    let path = only_path(&scene);
    // Three ridges a side and the four corners, straight lines between.
    let lines = path
        .elements()
        .iter()
        .filter(|e| matches!(e, PathEl::LineTo(_)))
        .count();
    assert_eq!(lines, 4 * (3 + 1), "{path}");
    // Out and in by the size: the outline reaches 10 past the square.
    let b = path.bounds().unwrap();
    assert!(
        (b.x + 10.0).abs() < 1e-9 && (b.width - 120.0).abs() < 1e-9,
        "{b:?}"
    );
    // Smooth points have handles.
    let scene = read(&format!(
        "{},{},{}",
        rect(50.0, 50.0, 100.0, 100.0),
        zz(2),
        fill(1)
    ))
    .scene;
    assert!(
        only_path(&scene)
            .elements()
            .iter()
            .all(|e| !matches!(e, PathEl::LineTo(_)))
    );
    // An open line: its ends and three ridges, alternately above and
    // below it.
    let line = r#"{"ty":"sh","ks":{"a":0,"k":{"c":false,"v":[[0,0],[100,0]],"i":[[0,0],[0,0]],"o":[[0,0],[0,0]]}}}"#;
    let scene = read(&format!("{line},{},{}", zz(1), stroke())).scene;
    let frame = scene.frame_at(0.0);
    let Some(Item::Stroke { path, .. }) = frame.items.first() else {
        panic!("{:?}", frame.items);
    };
    let ys: Vec<f64> = path
        .elements()
        .iter()
        .filter_map(|e| match e {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => Some(p.y.round()),
            _ => None,
        })
        .collect();
    assert_eq!(ys, [10.0, -10.0, 10.0, -10.0, 10.0]);
}

#[test]
fn offset_path_grows_and_shrinks_the_area() {
    let offset = |amount: f64, join: u8| {
        read(&format!(
            r#"{},{{"ty":"op","a":{},"lj":{join},"ml":{}}},{}"#,
            rect(50.0, 50.0, 100.0, 100.0),
            still(amount),
            still(4.0),
            fill(1)
        ))
        .scene
    };
    let grown = offset(10.0, 1);
    assert!((area(&only_path(&grown)) - 120.0 * 120.0).abs() < 1e-6);
    assert_eq!(
        coverage(&grown, &[(-9.0, -9.0), (-5.0, 50.0), (111.0, 50.0)]),
        [true, true, false]
    );
    let shrunk = offset(-10.0, 1);
    assert!((area(&only_path(&shrunk)) - 80.0 * 80.0).abs() < 1e-6);
    assert_eq!(
        coverage(&shrunk, &[(5.0, 50.0), (11.0, 11.0), (50.0, 50.0)]),
        [false, true, true]
    );
    // Round corners: a quarter circle of 10 about each corner.
    let round = offset(10.0, 2);
    assert_eq!(
        coverage(&round, &[(-6.0, -6.0), (-7.5, -7.5)]),
        [true, false]
    );
    let expected = 120.0 * 120.0 - (4.0 - std::f64::consts::PI) * 100.0;
    assert!((area(&only_path(&round)) - expected).abs() < 1.0);
    // Bevels: cut straight across.
    let bevel = offset(10.0, 3);
    assert_eq!(
        coverage(&bevel, &[(-4.0, -4.0), (-6.0, -6.0)]),
        [true, false]
    );
    // A circle grows by as much all round.
    let scene = read(&format!(
        r#"{},{{"ty":"op","a":{}}},{}"#,
        ellipse(0.0, 0.0, 100.0, 100.0),
        still(10.0),
        fill(1)
    ))
    .scene;
    let path = only_path(&scene);
    let expected = std::f64::consts::PI * 60.0 * 60.0;
    assert!((area(&path) - expected).abs() / expected < 1e-3);
}

#[test]
fn an_animated_input_is_baked_frame_by_frame() {
    // A square growing from 20 to 40 over a second, its corners rounded.
    let growing = r#"{"ty":"rc","p":{"a":0,"k":[0,0]},"s":{"a":1,"k":[{"t":0,"s":[20,20],"o":{"x":[0.5],"y":[0]},"i":{"x":[0.5],"y":[1]}},{"t":10,"s":[40,40]}]},"r":{"a":0,"k":0}}"#;
    let scene = read(&format!(
        r#"{growing},{{"ty":"rd","r":{}}},{}"#,
        still(4.0),
        fill(1)
    ))
    .scene;
    let shapes = shapes(&scene);
    let Geometry::Path {
        path: Animated::Track(track),
    } = &shapes[0].paths[0].geometry
    else {
        panic!("{:?}", shapes[0].paths[0].geometry);
    };
    // A keyframe a frame, straight between frames.
    let times: Vec<f64> = track.keyframes.iter().map(|k| k.time).collect();
    let frames: Vec<f64> = (0..=10).map(|f| f64::from(f) / 10.0).collect();
    assert_eq!(times.len(), frames.len());
    assert!(times.iter().zip(&frames).all(|(a, b)| (a - b).abs() < 1e-9));
    assert!(track.keyframes.iter().all(|k| k.easing == Easing::Linear));
    // Eased: a quarter in, it is less than a quarter grown.
    let width = |t: f64| only_path_at(&scene, t).bounds().unwrap().width;
    assert!((width(0.0) - 20.0).abs() < 1e-9 && (width(1.0) - 40.0).abs() < 1e-9);
    assert!(width(0.25) < 25.0);
    // Nothing moving: a constant outline.
    let scene = read(&format!(
        r#"{},{{"ty":"rd","r":{}}},{}"#,
        rect(0.0, 0.0, 20.0, 20.0),
        still(4.0),
        fill(1)
    ))
    .scene;
    assert!(matches!(
        shapes_geometry(&scene),
        Geometry::Path {
            path: Animated::Static(_)
        }
    ));
}

#[test]
fn an_animated_modifier_is_baked_and_holds_where_commands_change() {
    // A zig zag gaining ridges: the commands change, so frames hold.
    let zz = r#"{"ty":"zz","s":{"a":0,"k":5},"r":{"a":1,"k":[{"t":0,"s":[1],"o":{"x":[0],"y":[0]},"i":{"x":[1],"y":[1]}},{"t":4,"s":[3]}]},"pt":{"a":0,"k":1}}"#;
    let scene = read(&format!("{},{zz},{}", rect(0.0, 0.0, 40.0, 40.0), fill(1))).scene;
    let Geometry::Path {
        path: Animated::Track(track),
    } = shapes_geometry(&scene)
    else {
        panic!();
    };
    assert_eq!(track.keyframes.len(), 5);
    let counts: Vec<usize> = track
        .keyframes
        .iter()
        .map(|k| k.value.point_count())
        .collect();
    // 1, 1.5→2, 2, 2.5→3, 3 ridges a side (lottie-web rounds them).
    assert_eq!(counts, [9, 13, 13, 17, 17]);
    assert_eq!(track.keyframes[0].easing, Easing::Hold);
    assert_eq!(track.keyframes[1].easing, Easing::Linear);
}

fn shapes_geometry(scene: &Scene) -> &Geometry {
    &shapes(scene)[0].paths[0].geometry
}

fn only_path_at(scene: &Scene, t: f64) -> Path {
    let frame = scene.frame_at(t);
    frame
        .items
        .iter()
        .find_map(|item| match item {
            Item::Fill { path, .. } => Some(path.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_modifier_in_a_nested_group_changes_that_group_s_outlines() {
    // Inside the group: its own fill draws the rounded square.
    let scene = read(&group(
        &format!(
            r#"{},{{"ty":"rd","r":{}}},{}"#,
            rect(50.0, 50.0, 100.0, 100.0),
            still(10.0),
            fill(1)
        ),
        0.0,
        0.0,
    ))
    .scene;
    assert!(!covered(&scene, 0.0, 1.0, 1.0));
    // After the group: the group's fill draws the square as it was, the
    // fill after the modifier the rounded one, moved with the group.
    let scene = read(&format!(
        r#"{},{{"ty":"rd","r":{}}},{}"#,
        group(
            &format!("{},{}", rect(50.0, 50.0, 100.0, 100.0), fill(1)),
            100.0,
            0.0
        ),
        still(10.0),
        fill(2)
    ))
    .scene;
    let all = fills(&scene, 0.0);
    assert_eq!(all.len(), 2);
    let corner = kurbo::Point::new(101.0, 1.0);
    let by_rule = |rule: FillRule| {
        all.iter()
            .find(|(_, r)| *r == rule)
            .map(|(p, _)| p.winding(corner) != 0)
            .unwrap()
    };
    assert!(by_rule(FillRule::NonZero), "the group's own fill is square");
    assert!(!by_rule(FillRule::EvenOdd), "the later fill is rounded");
}

#[test]
fn a_trim_before_a_modifier_cuts_first() {
    // Half the square's outline (from its top right, down and along the
    // bottom), its corner rounded.
    let scene = read(&format!(
        r#"{},{{"ty":"tm","s":{},"e":{},"o":{},"m":1}},{{"ty":"rd","r":{}}},{}"#,
        rect(50.0, 50.0, 100.0, 100.0),
        still(0.0),
        still(50.0),
        still(0.0),
        still(10.0),
        stroke()
    ))
    .scene;
    let shape = shapes(&scene)[0];
    assert!(shape.trim.is_none());
    let frame = scene.frame_at(0.0);
    let Some(Item::Stroke { path, .. }) = frame.items.first() else {
        panic!("{:?}", frame.items);
    };
    assert!(!path.elements().contains(&PathEl::Close), "{path}");
    // The bottom right corner (100, 100) is cut by a curve.
    let b = path.bounds().unwrap();
    assert!(
        (b.x - 0.0).abs() < 1e-9 && (b.y - 0.0).abs() < 1e-9,
        "{b:?}"
    );
    assert!(
        path.elements()
            .iter()
            .filter(|e| matches!(e, PathEl::CubicTo(..)))
            .count()
            == 1,
        "{path}"
    );
}

/// A closed comb of `teeth` teeth 1 apart and as long as the comb is
/// wide, across (`vertical`) or along the axes.
fn comb(teeth: usize, vertical: bool) -> String {
    let mut v = Vec::new();
    for k in 0..teeth {
        let x = k as f64;
        let (a, b) = ([x, 0.0], [x + 0.5, teeth as f64]);
        v.push(a);
        v.push(b);
    }
    let v: Vec<String> = v
        .iter()
        .map(|p| {
            let (x, y) = if vertical { (p[0], p[1]) } else { (p[1], p[0]) };
            format!("[{x},{y}]")
        })
        .collect();
    let zeros = vec!["[0,0]"; v.len()].join(",");
    format!(
        r#"{{"ty":"sh","ks":{{"a":0,"k":{{"c":true,"v":[{}],"i":[{zeros}],"o":[{zeros}]}}}}}}"#,
        v.join(",")
    )
}

#[test]
fn merging_outlines_that_cross_everywhere_stays_bounded() {
    let json = format!(
        r#"{{"v":"5.7.0","fr":10,"ip":0,"op":20,"w":200,"h":200,"layers":[
{{"ty":4,"ind":1,"ip":0,"op":20,"st":0,"ks":{{}},"shapes":[{},{},{},{}]}}]}}"#,
        comb(1500, true),
        comb(1500, false),
        merge(4),
        fill(1)
    );
    let import = dxn_vector_animation_lottie::read(json.as_bytes()).unwrap();
    assert!(
        import
            .warnings
            .iter()
            .any(|w| w.contains("too detailed to merge")),
        "{:?}",
        import.warnings
    );
    // Kept as they were: both combs, one fill.
    assert_eq!(fills(&import.scene, 0.0).len(), 1);
}

#[test]
fn baking_stays_within_the_limits() {
    // Twenty frames of a growing rounded square, with room for ten.
    let growing = r#"{"ty":"rc","p":{"a":0,"k":[0,0]},"s":{"a":1,"k":[{"t":0,"s":[20,20],"o":{"x":[0],"y":[0]},"i":{"x":[1],"y":[1]}},{"t":20,"s":[40,40]}]},"r":{"a":0,"k":0}}"#;
    let json = format!(
        r#"{{"v":"5.7.0","fr":10,"ip":0,"op":20,"w":200,"h":200,"layers":[
{{"ty":4,"ind":1,"ip":0,"op":20,"st":0,"ks":{{}},"shapes":[{growing},{{"ty":"rd","r":{}}},{}]}}]}}"#,
        still(4.0),
        fill(1)
    );
    // A rounded square holds 17 points.
    let limits = dxn_vector_animation::Limits {
        max_path_points: 2 * 17 * 10,
        ..Default::default()
    };
    let import = dxn_vector_animation_lottie::read_with(json.as_bytes(), &limits).unwrap();
    assert!(
        import.warnings.iter().any(|w| w.contains("fewer frames")),
        "{:?}",
        import.warnings
    );
    let Geometry::Path {
        path: Animated::Track(track),
    } = shapes_geometry(&import.scene)
    else {
        panic!();
    };
    assert_eq!(track.keyframes.len(), 10);
    // Evenly apart, from the first keyframe to the last.
    assert_eq!(track.keyframes[0].time, 0.0);
    assert!((track.keyframes[9].time - 2.0).abs() < 1e-9);
}
