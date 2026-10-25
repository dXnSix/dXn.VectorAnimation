//! CSS selectors beyond simple ones: combinators (descendant, child,
//! adjacent, sibling), attribute selectors, structural pseudo-classes and
//! `:not()`, with CSS specificity; pseudo-elements are left out with a
//! warning.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Color, Group, Import, Node, Paint};

fn read(css: &str, body: &str) -> Import {
    let svg = format!(
        r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg"><style>{css}</style>{body}</svg>"#
    );
    dxn_vector_animation_svg::read(svg.as_bytes()).unwrap()
}

/// The fill colors of the scene's shapes, in drawing order.
fn fills(import: &Import) -> Vec<Option<Color>> {
    fn walk(g: &Group, out: &mut Vec<Option<Color>>) {
        for n in &g.children {
            match n {
                Node::Group(g) => walk(g, out),
                Node::Shape(s) => out.push(s.fill.as_ref().and_then(|f| match &f.paint {
                    Paint::Solid { color } => color.as_static().copied(),
                    _ => None,
                })),
                Node::Image(_) | Node::Text(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&import.scene.root, &mut out);
    out
}

const RED: Option<Color> = Some(Color::rgba(1.0, 0.0, 0.0, 1.0));
const BLUE: Option<Color> = Some(Color::rgba(0.0, 0.0, 1.0, 1.0));
const BLACK: Option<Color> = Some(Color::rgba(0.0, 0.0, 0.0, 1.0));

const RECTS: &str = r#"<g id="a"><rect width="1" height="1"/><g><rect class="c" width="1" height="1"/></g></g><rect class="c" width="1" height="1"/>"#;

#[test]
fn descendant_and_child_combinators() {
    let import = read("#a rect { fill: red } #a > rect { fill: blue }", RECTS);
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(fills(&import), vec![BLUE, RED, BLACK]);
}

#[test]
fn specificity_counts_every_compound() {
    // `#a rect` (1, 0, 1) beats `.c` (0, 1, 0), whatever the order.
    let import = read("#a rect { fill: red } .c { fill: blue }", RECTS);
    assert_eq!(fills(&import), vec![RED, RED, BLUE]);
}

#[test]
fn sibling_combinators() {
    let body = r#"<rect id="x" width="1" height="1"/><circle r="1"/><rect width="1" height="1"/><rect width="1" height="1"/>"#;
    let import = read("#x + circle { fill: red } #x ~ rect { fill: blue }", body);
    assert_eq!(fills(&import), vec![BLACK, RED, BLUE, BLUE]);
}

#[test]
fn attribute_selectors() {
    let body = r#"<rect data-k="on" width="1" height="1"/><rect class="a b" width="1" height="1"/><rect id="pre-x" width="1" height="1"/><rect width="2" height="1"/>"#;
    let import = read(
        r#"[data-k="on"] { fill: red } [class~=b] { fill: blue } [id^=pre] { fill: red } rect[width='2'] { fill: blue }"#,
        body,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(fills(&import), vec![RED, BLUE, RED, BLUE]);
}

#[test]
fn structural_pseudo_classes_and_not() {
    let body = r#"<g><rect width="1" height="1"/><rect class="skip" width="1" height="1"/><rect width="1" height="1"/><rect width="1" height="1"/></g>"#;
    let import = read(
        "rect:first-child { fill: red } rect:nth-child(2n+3):not(.skip) { fill: blue } rect:last-child { fill: red }",
        body,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(fills(&import), vec![RED, BLACK, BLUE, RED]);
}

#[test]
fn pseudo_elements_are_left_out_with_a_warning() {
    let import = read("rect::before { fill: red } rect { fill: blue }", RECTS);
    assert_eq!(fills(&import), vec![BLUE, BLUE, BLUE]);
    assert!(
        import.warnings.iter().any(|w| w.contains("::before")),
        "{:?}",
        import.warnings
    );
}
