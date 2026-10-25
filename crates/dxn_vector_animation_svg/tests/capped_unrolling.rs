//! Work the importer caps (iterations unrolled, SMIL instances resolved)
//! is a warning when a file asks for more, never a silent drop.

#![allow(clippy::unwrap_used, clippy::expect_used)]

fn warnings(body: &str) -> Vec<String> {
    let svg =
        format!(r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg">{body}</svg>"#);
    dxn_vector_animation_svg::read(svg.as_bytes())
        .unwrap()
        .warnings
}

fn has(warnings: &[String], text: &str) -> bool {
    warnings.iter().any(|w| w.contains(text))
}

#[test]
fn css_iterations_past_the_cap_warn() {
    let w = warnings(
        r#"<style>#a { animation: k 0.0001s linear 20000 } @keyframes k { to { opacity: 0 } }</style><g id="a"><rect width="1" height="1"/></g>"#,
    );
    assert!(
        has(
            &w,
            "CSS animation `k` runs more than 10000 iterations: only the first 10000 play"
        ),
        "{w:?}"
    );
    let within = warnings(
        r#"<style>#a { animation: k 0.001s linear 10000 } @keyframes k { to { opacity: 0 } }</style><g id="a"><rect width="1" height="1"/></g>"#,
    );
    assert!(within.is_empty(), "{within:?}");
}

#[test]
fn endless_css_iterations_past_the_cap_warn() {
    // 20 s of timeline for an animation of a millisecond.
    let w = warnings(
        r#"<style>#a { animation: k 0.001s linear infinite } #b { animation: k 20s linear } @keyframes k { to { opacity: 0 } }</style><g id="a"><rect width="1" height="1"/></g><g id="b"><rect width="1" height="1"/></g>"#,
    );
    assert!(has(&w, "CSS animation `k` runs more than 10000"), "{w:?}");
}

#[test]
fn smil_iterations_past_the_cap_warn() {
    let w = warnings(
        r#"<rect width="1" height="1"><animate attributeName="x" from="0" to="1" dur="0.001s" repeatCount="indefinite"/><animate attributeName="y" from="0" to="1" dur="20s"/></rect>"#,
    );
    assert!(
        has(
            &w,
            "SMIL animation of `x` on <rect> runs more than 10000 iterations: only the first 10000 play"
        ),
        "{w:?}"
    );
}

#[test]
fn smil_instances_past_the_cap_warn() {
    let begins: Vec<String> = (0..4500)
        .map(|i| format!("{}s", f64::from(i) * 0.1))
        .collect();
    let w = warnings(&format!(
        r#"<rect width="1" height="1"><animate attributeName="x" from="0" to="1" dur="0.05s" begin="{}"/></rect>"#,
        begins.join(";")
    ));
    assert!(
        has(
            &w,
            "SMIL animations begin more than 4000 times: later beginnings are left out"
        ),
        "{w:?}"
    );
}

#[test]
fn a_cycle_past_the_cap_keeps_what_it_needs_without_a_warning() {
    // Two animations starting each other over and over: only the first
    // cycle stays, which is well within the cap.
    let w = warnings(
        r#"<rect width="1" height="1"><animate id="a" attributeName="x" from="0" to="1" dur="0.1s" begin="0s;b.end"/><animate id="b" attributeName="x" from="1" to="0" dur="0.1s" begin="a.end"/></rect>"#,
    );
    assert!(w.is_empty(), "{w:?}");
}
