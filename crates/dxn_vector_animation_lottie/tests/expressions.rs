//! Expressions: evaluated at every frame and baked into keyframes, or left
//! out with a warning naming what was not understood.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Group, Import, Item, Node, Point, TransformOp};
use serde_json::{Value, json};

fn read(doc: &Value) -> Import {
    dxn_vector_animation_lottie::read(doc.to_string().as_bytes()).unwrap()
}

/// A composition of 10 frames a second, `op` frames long.
fn comp(op: f64, layers: Value) -> Value {
    json!({ "v": "5.7.0", "nm": "Main", "fr": 10, "ip": 0, "op": op, "w": 100, "h": 80, "layers": layers })
}

/// A shape layer (a square) whose transform is `ks` over the defaults.
fn layer(ind: u32, name: &str, ks: Value) -> Value {
    let mut full = json!({ "a": { "a": 0, "k": [0, 0, 0] }, "p": { "a": 0, "k": [0, 0, 0] },
        "s": { "a": 0, "k": [100, 100, 100] }, "r": { "a": 0, "k": 0 }, "o": { "a": 0, "k": 100 } });
    for (k, v) in ks.as_object().unwrap() {
        full[k] = v.clone();
    }
    json!({ "ty": 4, "ind": ind, "nm": name, "ip": 0, "op": 1000, "st": 0, "ks": full,
        "shapes": [{ "ty": "rc", "p": { "a": 0, "k": [0, 0] }, "s": { "a": 0, "k": [10, 10] }, "r": { "a": 0, "k": 0 } },
                   { "ty": "fl", "c": { "a": 0, "k": [1, 0, 0, 1] }, "o": { "a": 0, "k": 100 } }] })
}

/// A constant property `k` with expression `x`.
fn expr(k: Value, x: &str) -> Value {
    json!({ "a": 0, "k": k, "x": x })
}

/// Keyframes (frame, value), straight between them.
fn keyed(keys: &[(f64, Value)]) -> Value {
    let k: Vec<Value> = keys
        .iter()
        .map(|(t, s)| json!({ "t": t, "s": s }))
        .collect();
    json!({ "a": 1, "k": k })
}

fn group(import: &Import, ind: u32) -> &Group {
    let id = ind.to_string();
    import
        .scene
        .root
        .children
        .iter()
        .find_map(|n| match n {
            Node::Group(g) if g.id.as_deref() == Some(id.as_str()) => Some(g),
            _ => None,
        })
        .unwrap()
}

fn position(g: &Group, t: f64) -> (f64, f64) {
    g.transform
        .iter()
        .find_map(|op| match op {
            TransformOp::Translate { x, y } => Some((x.sample(t), y.sample(t))),
            _ => None,
        })
        .unwrap_or((0.0, 0.0))
}

fn rotation(g: &Group, t: f64) -> f64 {
    g.transform
        .iter()
        .find_map(|op| match op {
            TransformOp::Rotate { degrees } => Some(degrees.sample(t)),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn warnings(import: &Import) -> String {
    import.warnings.join("\n")
}

#[test]
fn bodymovin_operators_are_baked() {
    let p = expr(
        json!([5, 5, 0]),
        "var $bm_rt;\n$bm_rt = $bm_sum(value, [$bm_mul(time, 10), $bm_div(20, 2)]);",
    );
    let r = expr(
        json!(0),
        "var $bm_rt;\n$bm_rt = $bm_sub($bm_mul($bm_mod($bm_mul(time, 10), 4), 25), $bm_neg(1));",
    );
    let o = expr(
        json!(100),
        "var $bm_rt;\n$bm_rt = $bm_isInstanceOfArray(value) ? 0 : $bm_div(value, 2);",
    );
    let import = read(&comp(
        20.0,
        json!([layer(1, "A", json!({ "p": p, "r": r, "o": o }))]),
    ));
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
    let g = group(&import, 1);
    assert_eq!(position(g, 0.0), (5.0, 15.0));
    let (x, y) = position(g, 0.5);
    assert!(near(x, 10.0) && near(y, 15.0), "{x} {y}");
    let (x, _) = position(g, 1.0);
    assert!(near(x, 15.0));
    // Frame 3: (3 mod 4) * 25 + 1; frame 4: 0 * 25 + 1.
    assert!(near(rotation(g, 0.3), 76.0));
    assert!(near(rotation(g, 0.4), 1.0));
    assert_eq!(g.opacity.sample(0.7), 0.5);
}

/// The inertial bounce, as Bodymovin exports it.
const BOUNCE: &str = "var $bm_rt;
var amp, freq, decay, n, n, t, t, v;
amp = 0.1;
freq = 2;
decay = 6;
n = 0;
if (numKeys > 0) {
    n = nearestKey(time).index;
    if (key(n).time > time) {
        n--;
    }
}
if (n == 0) {
    t = 0;
} else {
    t = $bm_sub(time, key(n).time);
}
if (n > 0 && t < 1) {
    v = velocityAtTime($bm_sub(key(n).time, $bm_div(thisComp.frameDuration, 10)));
    $bm_rt = $bm_sum(value, $bm_div($bm_mul($bm_mul(v, amp), Math.sin($bm_mul($bm_mul($bm_mul(freq, t), 2), Math.PI))), Math.exp($bm_mul(decay, t))));
} else {
    $bm_rt = value;
}";

#[test]
fn a_bounce_overshoots_and_settles() {
    let mut r = keyed(&[(0.0, json!([0])), (10.0, json!([100]))]);
    r["x"] = json!(BOUNCE);
    let import = read(&comp(40.0, json!([layer(1, "A", json!({ "r": r }))])));
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
    let g = group(&import, 1);
    assert!(
        near(rotation(g, 0.5), 50.0),
        "the keyframes before the last"
    );
    // 0.2 s after the last keyframe, at 100 a second:
    // 100 + 100 * 0.1 * sin(0.8 pi) / e^1.2.
    let expected = 100.0 + 10.0 * (0.8 * std::f64::consts::PI).sin() / 1.2_f64.exp();
    assert!(near(rotation(g, 1.2), expected), "{}", rotation(g, 1.2));
    assert!(near(rotation(g, 3.0), 100.0), "settled");
}

#[test]
fn plain_javascript_runs() {
    let r = expr(
        json!(0),
        r#"
        function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
        let total = 0;
        for (let i = 0; i < 5; i++) { if (i % 2 == 0) continue; total += i; }
        var k = 0; while (k < 3) k++;
        do { k += 10 } while (false)
        const list = [1, 2, 3]; list.push(4); list[0] = 10;
        var grid = [[0, 0], [0, 0]]; grid[1][0] = 7;
        switch (list.length) { case 4: total += 100; break; default: total = -1 }
        var label = "n" + 1;
        if (parseFloat("1e-1px") != 0.1 || parseInt("0x1F", 16) != 31 || parseInt("12.5") != 12) total = -1;
        try { undefinedThing(); } catch (e) { total += 1000; }
        if (total == 1104 && k == 13 && list[0] + grid[1][0] == 17 && label === "n1") {
            fib(10) + Math.max(1, 2) + [1, 2, 3].map(function (x) { return x * 2; })[2]
        } else {
            -1
        }"#,
    );
    let import = read(&comp(20.0, json!([layer(1, "A", json!({ "r": r }))])));
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
    assert_eq!(rotation(group(&import, 1), 0.5), 63.0);
}

#[test]
fn vector_math() {
    let p = expr(
        json!([0, 0, 0]),
        "var p = normalize([3, 4]);
         [length([0, 0], [3, 4]) + dot([1, 2], [3, 4]) + cross([1, 0, 0], [0, 1, 0])[2],
          clamp(250, 0, 100) + p[0] * 10 + length(sub(mul([2, 2], 3), div([4, 4], 2))[0])]",
    );
    let r = expr(
        json!(0),
        "radiansToDegrees(Math.PI) + degreesToRadians(180) / Math.PI + add([1, 2], [3, 4])[1]",
    );
    let import = read(&comp(
        20.0,
        json!([layer(1, "A", json!({ "p": p, "r": r }))]),
    ));
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
    let g = group(&import, 1);
    let (x, y) = position(g, 0.0);
    assert!(near(x, 17.0) && near(y, 110.0), "{x} {y}");
    assert!(near(rotation(g, 0.0), 187.0));
}

#[test]
fn wiggle_stays_within_its_amplitude_and_repeats() {
    let wiggling = |octaves: &str| {
        let p = expr(json!([50, 50, 0]), &format!("wiggle(5, 20{octaves})"));
        json!([
            layer(1, "A", json!({ "p": p.clone() })),
            layer(2, "B", json!({ "p": p }))
        ])
    };
    for octaves in ["", ", 3, 0.8"] {
        let doc = comp(300.0, wiggling(octaves));
        let (first, second) = (read(&doc), read(&doc));
        assert!(first.warnings.is_empty(), "{}", warnings(&first));
        let (a, b) = (group(&first, 1), group(&first, 2));
        let mut moved = false;
        for frame in 0..=300 {
            let t = frame as f64 / 10.0;
            let (x, y) = position(a, t);
            assert!(
                (30.0..=70.0).contains(&x) && (30.0..=70.0).contains(&y),
                "{x} {y} at {t}"
            );
            assert_eq!(
                position(a, t),
                position(group(&second, 1), t),
                "the same every run"
            );
            moved |= (x - 50.0).abs() > 10.0;
        }
        assert!(moved, "it wiggles");
        assert_ne!(position(a, 1.05), position(b, 1.05), "seeded per property");
    }
}

#[test]
fn loops() {
    let r = |x: &str| {
        let mut r = keyed(&[(0.0, json!([0])), (10.0, json!([90]))]);
        r["x"] = json!(x);
        r
    };
    let layers = json!([
        layer(1, "cycle", json!({ "r": r("loopOut('cycle')") })),
        layer(2, "pingpong", json!({ "r": r("loopOut(\"pingpong\")") })),
        layer(3, "offset", json!({ "r": r("loopOut('offset')") })),
        layer(4, "continue", json!({ "r": r("loopOut('continue')") })),
        layer(
            5,
            "duration",
            json!({ "r": r("loopOutDuration('cycle', 0.5)") })
        ),
        layer(
            6,
            "in",
            json!({ "r": {
            "a": 1, "k": [{ "t": 10, "s": [0] }, { "t": 20, "s": [90] }], "x": "loopIn('cycle')" } })
        ),
        layer(
            7,
            "last segment",
            json!({ "r": {
            "a": 1, "k": [{ "t": 0, "s": [0] }, { "t": 10, "s": [90] }, { "t": 20, "s": [0] }],
            "x": "loopOut('cycle', 1)" } })
        ),
    ]);
    let import = read(&comp(40.0, layers));
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
    let at = |ind: u32, frame: f64| rotation(group(&import, ind), frame / 10.0);
    assert!(near(at(1, 5.0), 45.0), "keyframes play as they are");
    assert!(near(at(1, 12.0), 18.0));
    assert!(near(at(1, 25.0), 45.0));
    assert!(near(at(2, 12.0), 72.0), "back");
    assert!(near(at(2, 25.0), 45.0));
    assert!(near(at(2, 22.0), 18.0), "forth again");
    assert!(
        near(at(3, 12.0), 108.0),
        "each repeat goes on from the last"
    );
    assert!(near(at(3, 35.0), 315.0));
    assert!(near(at(4, 15.0), 135.0), "at the speed it ends with");
    // The last half second (5 frames) repeats.
    assert!(near(at(5, 12.0), 63.0));
    // Before the first keyframe at frame 10.
    assert!(near(at(6, 5.0), 45.0));
    // From frame 10 (90) to 20 (0) and again.
    assert!(near(at(7, 25.0), 45.0));
    assert!(near(at(7, 21.0), 81.0));
}

#[test]
fn effect_sliders_and_controls() {
    let slider = json!({ "ty": 5, "nm": "Slider Control", "mn": "ADBE Slider Control", "en": 1,
        "ef": [{ "ty": 0, "nm": "Slider", "mn": "ADBE Slider Control-0001", "ix": 1,
                 "v": { "a": 1, "k": [{ "t": 0, "s": [0] }, { "t": 10, "s": [100] }] } }] });
    let checkbox = json!({ "ty": 5, "nm": "Check", "mn": "ADBE Checkbox Control", "en": 1,
        "ef": [{ "ty": 4, "nm": "Checkbox", "mn": "ADBE Checkbox Control-0001", "ix": 1, "v": { "a": 0, "k": 1 } }] });
    let point = json!({ "ty": 5, "nm": "Point", "mn": "ADBE Point Control", "en": 1,
        "ef": [{ "ty": 3, "nm": "Point", "mn": "ADBE Point Control-0001", "ix": 1, "v": { "a": 0, "k": [12, 34] } }] });
    let angle = json!({ "ty": 5, "nm": "Angle", "mn": "ADBE Angle Control", "en": 1,
        "ef": [{ "ty": 1, "nm": "Angle", "mn": "ADBE Angle Control-0001", "ix": 1,
                 "v": { "a": 0, "k": 30, "x": "var $bm_rt;\n$bm_rt = $bm_mul(value, 2);" } }] });
    let mut controls = layer(
        1,
        "Controls",
        json!({
            "r": expr(json!(0), "effect('Slider Control')('Slider') + (effect('Check')('Checkbox') == 1 ? 1000 : 0)"),
            "p": expr(json!([0, 0, 0]), "effect(\"Point\")(1)"),
        }),
    );
    controls["ef"] = json!([slider, checkbox, point, angle]);
    let reader = layer(
        2,
        "Reader",
        json!({
            "r": expr(json!(0), "thisComp.layer(\"Controls\").effect(\"Slider Control\").param(\"Slider\").valueAtTime(1) + thisComp.layer(1).effect(4)(1)"),
        }),
    );
    let import = read(&comp(20.0, json!([controls, reader])));
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
    let g = group(&import, 1);
    assert!(near(rotation(g, 0.5), 1050.0));
    assert_eq!(position(g, 0.0), (12.0, 34.0));
    assert!(
        near(rotation(group(&import, 2), 0.0), 160.0),
        "the angle's own expression"
    );
}

#[test]
fn another_layers_properties() {
    let a = layer(
        1,
        "A",
        json!({
            "p": keyed(&[(0.0, json!([0, 0, 0])), (10.0, json!([100, 50, 0]))]),
            "r": expr(json!(0), "time * 90"),
            "s": { "a": 0, "k": [50, 50, 100] },
        }),
    );
    let b = layer(
        2,
        "B",
        json!({
            "p": expr(json!([0, 0, 0]), "thisComp.layer('A').transform.position + [10, 0]"),
            "r": expr(json!(0), "thisComp.layer(1).transform.rotation * 2 + thisComp.layer('A').scale[0]"),
            "o": expr(json!(100), "index * 10 + thisComp.width / 10 + thisComp.layer('A').transform.position.velocity[0] / 100"),
        }),
    );
    let import = read(&comp(20.0, json!([a, b])));
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
    let g = group(&import, 2);
    let (x, y) = position(g, 0.5);
    assert!(near(x, 60.0) && near(y, 25.0), "{x} {y}");
    assert!(
        near(rotation(g, 0.5), 140.0),
        "A's rotation from its own expression"
    );
    assert!(
        near(g.opacity.sample(0.5), 0.31),
        "{}",
        g.opacity.sample(0.5)
    );
}

#[test]
fn expressions_reading_one_another_in_a_cycle() {
    let a = layer(
        1,
        "A",
        json!({ "r": expr(json!(5), "thisComp.layer('B').transform.rotation + 1") }),
    );
    let b = layer(
        2,
        "B",
        json!({ "r": expr(json!(7), "thisComp.layer('A').transform.rotation + 1") }),
    );
    let import = read(&comp(20.0, json!([a, b])));
    assert!(warnings(&import).contains("cycle"), "{}", warnings(&import));
    // A is left with its value; B reads it.
    assert_eq!(rotation(group(&import, 1), 0.0), 5.0);
    assert_eq!(rotation(group(&import, 2), 0.0), 6.0);
}

#[test]
fn keyframes_time_and_interpolation() {
    let o = {
        let mut o = keyed(&[(0.0, json!([20])), (10.0, json!([80]))]);
        o["x"] = json!(
            "key(2).value + key(2).time * 10 + numKeys - nearestKey(0.2).index - valueAtTime(0.5) / 10"
        );
        o
    };
    let p = expr(
        json!([0, 0, 0]),
        "[linear(time, 0, 1, 0, 100), ease(time, 0, 1, 0, 100)]",
    );
    let r = expr(
        json!(0),
        "posterizeTime(2); easeIn(0.5, 0, 100) < 50 && easeOut(0.5, 0, 100) > 50 ? time * 10 + framesToTime(5) + timeToFrames(1) + thisComp.frameDuration + outPoint : -1",
    );
    let import = read(&comp(
        20.0,
        json!([layer(1, "A", json!({ "o": o, "p": p, "r": r }))]),
    ));
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
    let g = group(&import, 1);
    // 80 + 10 + 2 - 1 - 5
    assert!(
        near(g.opacity.sample(0.0), 0.86),
        "{}",
        g.opacity.sample(0.0)
    );
    let (x, y) = position(g, 0.5);
    assert!(near(x, 50.0) && (y - 50.0).abs() < 1.0, "{x} {y}");
    let (x, y) = position(g, 0.2);
    assert!(near(x, 20.0) && y < 20.0, "eases in: {y}");
    // posterizeTime(2): 0.7 s shows 0.5 s.
    assert!(near(rotation(g, 0.7), 5.0 + 0.5 + 10.0 + 0.1 + 100.0));
}

#[test]
fn random_numbers_are_seeded() {
    let r = expr(json!(0), "seedRandom(4, true); random(10, 20)");
    let o = expr(json!(0), "random(100)");
    let p = expr(json!([0, 0, 0]), "[gaussRandom(0, 10), random([5, 5])[1]]");
    let doc = comp(
        20.0,
        json!([layer(1, "A", json!({ "r": r, "o": o, "p": p }))]),
    );
    let (first, second) = (read(&doc), read(&doc));
    assert!(first.warnings.is_empty(), "{}", warnings(&first));
    let g = group(&first, 1);
    let still = rotation(g, 0.0);
    assert!((10.0..20.0).contains(&still));
    assert!(
        g.transform
            .iter()
            .all(|op| !matches!(op, TransformOp::Rotate { degrees } if degrees.is_animated())),
        "timeless"
    );
    assert!(g.opacity.is_animated(), "new numbers each frame");
    for frame in 0..20 {
        let t = frame as f64 / 10.0;
        let o = g.opacity.sample(t);
        assert!((0.0..1.0).contains(&o));
        assert_eq!(o, group(&second, 1).opacity.sample(t), "the same every run");
        let (_, y) = position(g, t);
        assert!((0.0..5.0).contains(&y));
    }
}

#[test]
fn layer_time_and_precompositions() {
    // A layer starting at frame 5 runs its expression in composition time.
    let mut late = layer(
        1,
        "late",
        json!({ "r": expr(json!(0), "time * 10 + inPoint * 100") }),
    );
    late["st"] = json!(5);
    late["ip"] = json!(5);
    let inner = layer(
        1,
        "inner",
        json!({ "r": expr(json!(0), "time * 100 + thisComp.width") }),
    );
    let asset = json!({ "id": "comp_0", "nm": "Inner", "w": 40, "h": 40, "layers": [inner] });
    let precomp = json!({ "ty": 0, "ind": 2, "nm": "pre", "refId": "comp_0", "ip": 0, "op": 20, "st": 0, "w": 40, "h": 40,
        "ks": { "o": { "a": 0, "k": 100 } } });
    let mut doc = comp(20.0, json!([late, precomp]));
    doc["assets"] = json!([asset]);
    let import = read(&doc);
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
    let items = import.scene.frame_at(1.0).items;
    let late_layer = group(&import, 1);
    // The layer's tracks are in its own time: 0.5 s in is 1 s of the
    // composition.
    assert!(near(rotation(late_layer, 0.5), 10.0 + 50.0));
    assert!(!items.is_empty());
    let Node::Group(pre) = &group(&import, 2).children[0] else {
        panic!()
    };
    let inner = pre
        .children
        .iter()
        .find_map(|n| match n {
            Node::Group(g) if g.id.as_deref() == Some("1") => Some(g),
            _ => None,
        })
        .unwrap();
    assert!(
        near(rotation(inner, 0.5), 50.0 + 40.0),
        "its composition's width"
    );
}

#[test]
fn spatial_keyframes_are_followed() {
    let p = json!({ "a": 1, "x": "value", "k": [
        { "t": 0, "s": [0, 50], "to": [33, -30], "ti": [-33, -30] },
        { "t": 20, "s": [100, 50] } ] });
    let import = read(&comp(20.0, json!([layer(1, "A", json!({ "p": p }))])));
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
    let Some(Item::Fill { transform, .. }) = import.scene.frame_at(1.0).items.first().cloned()
    else {
        panic!()
    };
    let mid = transform.apply(Point::ZERO);
    assert!(
        (mid.x - 50.0).abs() < 1.0 && mid.y < 35.0,
        "arcs above: {mid:?}"
    );
}

#[test]
fn unsupported_constructs_leave_their_property() {
    let p = || keyed(&[(0.0, json!([0, 0, 0])), (10.0, json!([100, 0, 0]))]);
    let with = |x: &str| {
        let mut p = p();
        p["x"] = json!(x);
        p
    };
    let path = json!({ "ty": "sh", "ks": { "a": 0, "k": { "c": true, "v": [[0, 0], [10, 0], [10, 10]],
        "i": [[0, 0], [0, 0], [0, 0]], "o": [[0, 0], [0, 0], [0, 0]] }, "x": "createPath()" } });
    let mut shaped = layer(5, "path", json!({}));
    shaped["shapes"].as_array_mut().unwrap().insert(0, path);
    let layers = json!([
        layer(1, "toComp", json!({ "p": with("toComp([0, 0])") })),
        layer(2, "syntax", json!({ "p": with("x => x") })),
        layer(3, "undefined", json!({ "p": with("value + foo") })),
        layer(4, "fine", json!({ "r": expr(json!(0), "time * 10") })),
        shaped,
        layer(6, "dimensions", json!({ "p": with("5") })),
    ]);
    let import = read(&comp(20.0, layers));
    let w = warnings(&import);
    for part in [
        "expressions using `toComp` are left out",
        "expressions that cannot be read (arrow functions (`=>`))",
        "`foo` is not defined",
        "expressions on paths are left out",
        "a number for a property of 3 values",
    ] {
        assert!(w.contains(part), "{part}: {w}");
    }
    for ind in [1, 2, 3, 6] {
        let (x, _) = position(group(&import, ind), 0.5);
        assert!(near(x, 50.0), "keeps its keyframes: {x}");
    }
    assert!(
        near(rotation(group(&import, 4), 0.5), 5.0),
        "the rest imports"
    );
}

#[test]
fn runaway_expressions_stop() {
    let layers = json!([
        layer(1, "loop", json!({ "r": expr(json!(3), "while (true) {}") })),
        layer(
            2,
            "for",
            json!({ "o": expr(json!(50), "for (;;) { value }") })
        ),
        layer(
            3,
            "recursion",
            json!({ "r": expr(json!(4), "function f(n) { return f(n + 1) } f(0)") })
        ),
        layer(
            4,
            "array",
            json!({ "r": expr(json!(5), "var a = []; while (true) a.push(1);") })
        ),
        layer(
            5,
            "string",
            json!({ "r": expr(json!(6), "var s = 'ab'; while (true) s = s + s;") })
        ),
        layer(
            6,
            "nesting",
            json!({ "r": expr(json!(7), &format!("{}1{}", "(".repeat(500), ")".repeat(500))) })
        ),
        layer(7, "fine", json!({ "r": expr(json!(0), "time * 10") })),
    ]);
    let import = read(&comp(20.0, layers));
    let w = warnings(&import);
    for part in [
        "expressions over the evaluation budget",
        "nested too deeply",
        "an array of over 65536 elements",
        "a string over 64 KB",
    ] {
        assert!(w.contains(part), "{part}: {w}");
    }
    assert_eq!(rotation(group(&import, 1), 0.5), 3.0);
    assert_eq!(group(&import, 2).opacity.sample(0.5), 0.5);
    assert_eq!(rotation(group(&import, 3), 0.5), 4.0);
    assert!(
        near(rotation(group(&import, 7), 0.5), 5.0),
        "the rest imports"
    );
}

#[test]
fn keyframes_stay_within_the_limit() {
    let r = expr(json!(0), "Math.sin(time)");
    let mut doc = comp(20.0, json!([layer(1, "A", json!({ "r": r }))]));
    doc["op"] = json!(30_000);
    doc["layers"][0]["op"] = json!(30_000);
    let limits = dxn_vector_animation::Limits {
        max_keyframes: 10_000,
        ..Default::default()
    };
    let import =
        dxn_vector_animation_lottie::read_with(doc.to_string().as_bytes(), &limits).unwrap();
    assert!(
        warnings(&import).contains("keyframe limit"),
        "{}",
        warnings(&import)
    );
    // Straight stretches need no keyframe per frame.
    let r = expr(json!(0), "time * 10");
    let doc = comp(20.0, json!([layer(1, "A", json!({ "r": r }))]));
    let import = read(&doc);
    let g = group(&import, 1);
    let keys = g
        .transform
        .iter()
        .find_map(|op| match op {
            TransformOp::Rotate { degrees } => Some(degrees.keyframes().len()),
            _ => None,
        })
        .unwrap();
    assert_eq!(keys, 2);
}

#[test]
fn deep_expressions_within_the_caps_run() {
    let parens = format!("{}time{}", "(".repeat(60), ")".repeat(60));
    let chain = vec!["1"; 150].join(" + ");
    let calls = format!("{}1{}", "Math.abs(".repeat(40), ")".repeat(40));
    let recursion = "function f(n) { return n <= 0 ? 0 : 1 + f(n - 1) } f(20)";
    let layers = json!([
        layer(1, "parens", json!({ "r": expr(json!(0), &parens) })),
        layer(2, "chain", json!({ "r": expr(json!(0), &chain) })),
        layer(3, "calls", json!({ "r": expr(json!(0), &calls) })),
        layer(4, "recursion", json!({ "r": expr(json!(0), recursion) })),
    ]);
    let import = read(&comp(20.0, layers));
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
    assert!(near(rotation(group(&import, 1), 0.5), 0.5));
    assert_eq!(rotation(group(&import, 2), 0.5), 150.0);
    assert_eq!(rotation(group(&import, 3), 0.5), 1.0);
    assert_eq!(rotation(group(&import, 4), 0.5), 20.0);
}

#[test]
fn text_expression_selectors_are_left_to_the_text() {
    // An expression selector runs per character (`textIndex`): the text
    // conversion reads it. A range selector's offset is a property.
    let glyph = r#"{"ch":"a","size":100,"style":"Regular","w":60,"fFamily":"Test","data":{"shapes":[{"ty":"gr","it":[
        {"ty":"sh","ks":{"a":0,"k":{"c":true,"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]],"v":[[0,0],[50,0],[50,-50]]}}},
        {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}}]}]}}"#;
    let by_index = json!({ "t": 1, "a": { "a": 0, "k": 100,
        "x": "var $bm_rt;\n$bm_rt = $bm_mul(selectorValue, $bm_div(textIndex, textTotal));" } });
    let range = json!({ "t": 0, "s": { "a": 0, "k": 0 }, "e": { "a": 0, "k": 100 },
        "o": { "a": 0, "k": 0, "x": "time * 10" } });
    let animator = |s: Value| json!({ "s": s, "a": { "p": { "a": 0, "k": [0, 100] } } });
    let text = json!({ "ty": 5, "ind": 1, "ip": 0, "op": 100, "st": 0,
        "ks": { "o": { "a": 0, "k": 100 }, "p": { "a": 0, "k": [0, 0] } },
        "t": { "d": { "k": [{ "t": 0, "s": { "s": 100, "f": "Test-Regular", "t": "aa", "j": 0, "tr": 0, "lh": 120, "fc": [1, 0, 0] } }] },
               "p": {}, "m": { "g": 1, "a": { "a": 0, "k": [0, 0] } },
               "a": [animator(by_index), animator(range)] } });
    let mut doc = comp(20.0, json!([text]));
    doc["fonts"] = json!({ "list": [{ "fName": "Test-Regular", "fFamily": "Test", "fStyle": "Regular", "ascent": 70 }] });
    doc["chars"] = serde_json::from_str::<Value>(&format!("[{glyph}]")).unwrap();
    let import = read(&doc);
    assert!(import.warnings.is_empty(), "{}", warnings(&import));
}
