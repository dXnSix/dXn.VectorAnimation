//! CSS selectors beyond simple ones: combinators (descendant, child,
//! adjacent, sibling), attribute selectors, structural pseudo-classes
//! (`-child`, `-of-type`, `:empty`, `:root`), `:not()`, `:is()` and
//! `:where()` of compound lists, with CSS specificity; pseudo-elements
//! draw nothing, and rules for states of interaction (`:hover`) never
//! match, with a warning.

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
fn pseudo_elements_draw_nothing() {
    // SVG's elements have no pseudo-elements: such rules match nothing,
    // as browsers draw them.
    let import = read(
        "rect::before { fill: red } rect:after { fill: red } rect { fill: blue }",
        RECTS,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(fills(&import), vec![BLUE, BLUE, BLUE]);
}

#[test]
fn rules_for_interaction_are_drawn_at_rest_with_a_warning() {
    let import = read("rect:hover { fill: red } rect { fill: blue }", RECTS);
    assert_eq!(fills(&import), vec![BLUE, BLUE, BLUE]);
    assert!(
        import.warnings.iter().any(|w| w.contains(":hover")),
        "{:?}",
        import.warnings
    );
}

#[test]
fn of_type_empty_and_root() {
    let body = r#"<circle r="1"/><rect width="1" height="1"/><circle r="1"/><rect width="1" height="1"/><g><rect width="1" height="1"/></g>"#;
    let import = read(
        "rect:first-of-type { fill: red } circle:nth-last-of-type(1) { fill: blue } g rect:only-of-type { fill: red }",
        body,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(fills(&import), vec![BLACK, RED, BLUE, BLACK, RED]);
    let import = read(
        ":root > rect:empty { fill: blue }",
        r#"<rect width="1" height="1"/><g><rect width="1" height="1"/></g>"#,
    );
    assert_eq!(fills(&import), vec![BLUE, BLACK]);
}

#[test]
fn is_where_and_not_of_lists() {
    // `:is()` weighs as its heaviest compound, `:where()` as nothing.
    let import = read(
        ":is(#a, .z) rect { fill: red } .c { fill: blue } :where(#a) .c { fill: black }",
        RECTS,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(fills(&import), vec![RED, RED, BLUE]);
    let import = read("rect:not(.c, #q) { fill: red }", RECTS);
    assert_eq!(fills(&import), vec![RED, BLACK, BLACK]);
}

#[test]
fn rules_apply_in_order_whatever_an_element_is_matched_by() {
    // Selectors of an id, classes in either order, a tag and an attribute
    // alone, over elements naming a class twice: later rules of equal
    // weight win, as in the sheet's order.
    let css = ".a.b { fill: red } rect.b.a { fill: blue } [data-x] { fill: red } \
               #one.a { fill: blue } rect { fill: red } .b { fill: blue }";
    let import = read(
        css,
        r#"<rect class="b a b" width="1" height="1"/><rect id="one" class="a a" width="1" height="1"/><rect data-x="1" class="b" width="1" height="1"/><circle data-x="1" r="1"/>"#,
    );
    assert_eq!(fills(&import), vec![BLUE, BLUE, BLUE, RED]);
}

#[test]
fn important_declarations_win_however_written() {
    // `!important` in any case, after comments, wins over a later rule and
    // the `style` attribute; the last of two in a value is the one.
    let css = "rect { fill: blue !IMPORTANT } /* note */ rect { fill: red } \
               circle { fill: red /* x */ ! important } circle { fill: blue !Important }";
    let import = read(
        css,
        r#"<rect width="1" height="1" style="fill: red"/><circle r="1"/>"#,
    );
    assert_eq!(fills(&import), vec![BLUE, BLUE]);
}
