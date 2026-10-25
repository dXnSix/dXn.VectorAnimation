//! Live text: `<text>` and its `<tspan>`s as spans set in a font when
//! drawn: placed where they say or after the span before, their fonts,
//! anchors, spacing and paints inherited, whitespace collapsed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Group, Import, Node, Paint, Point, Text, TextAnchor,
};

fn read(svg: &str) -> Import {
    dxn_vector_animation_svg::read(svg.as_bytes()).unwrap()
}

fn text(import: &Import) -> Text {
    fn find(g: &Group) -> Option<Text> {
        g.children.iter().find_map(|n| match n {
            Node::Text(t) => Some((**t).clone()),
            Node::Group(g) => find(g),
            _ => None,
        })
    }
    find(&import.scene.root).unwrap_or_else(|| panic!("no text ({:?})", import.warnings))
}

fn svg(body: &str) -> String {
    format!(r#"<svg width="200" height="100" xmlns="http://www.w3.org/2000/svg">{body}</svg>"#)
}

#[test]
fn a_text_with_its_font() {
    let import = read(&svg(
        r##"<text x="10" y="50" font-family="'Helvetica Neue', Arial, sans-serif" font-size="24" font-weight="bold" font-style="italic" fill="#f00" letter-spacing="2">Hello</text>"##,
    ));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let t = text(&import);
    let [span] = t.spans.as_slice() else {
        panic!("{t:?}");
    };
    assert_eq!(span.text, "Hello");
    assert_eq!(span.position, Some(AnimatedPoint::fixed(10.0, 50.0)));
    assert_eq!(
        span.font.families,
        vec!["Helvetica Neue", "Arial", "sans-serif"]
    );
    assert_eq!(span.font.size, Animated::Static(24.0));
    assert_eq!(span.font.weight, 700);
    assert!(span.font.italic);
    assert_eq!(span.letter_spacing, Animated::Static(2.0));
    let fill = span.fill.as_ref().unwrap();
    assert_eq!(fill.paint, Paint::solid(Color::rgba(1.0, 0.0, 0.0, 1.0)));
    assert!(span.stroke.is_none());
}

#[test]
fn tspans_go_on_or_start_anew_and_inherit() {
    let import = read(&svg(
        r##"<text x="100" y="40" text-anchor="middle" font-size="20" fill="blue">
    Total:  <tspan font-weight="700" fill="green" dx="4">42</tspan>
    <tspan x="100" dy="1.5em" font-size="50%">items</tspan>
</text>"##,
    ));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let t = text(&import);
    let texts: Vec<&str> = t.spans.iter().map(|s| s.text.as_str()).collect();
    // Whitespace collapsed; none before the text or after it.
    assert_eq!(texts, vec!["Total: ", "42", " ", "items"]);
    let s = &t.spans;
    assert_eq!(s[0].position, Some(AnimatedPoint::fixed(100.0, 40.0)));
    assert_eq!(s[0].anchor, TextAnchor::Middle);
    assert_eq!(s[1].position, None);
    assert_eq!(s[1].shift, Point::new(4.0, 0.0));
    assert_eq!(s[1].font.weight, 700);
    assert_eq!(
        s[1].fill.as_ref().unwrap().paint,
        Paint::solid(Color::rgba(0.0, 128.0 / 255.0, 0.0, 1.0))
    );
    // A new line: placed at x, the baseline moved down 1.5 of its font
    // size (half the text's).
    assert_eq!(s[3].position, Some(AnimatedPoint::fixed(100.0, 40.0)));
    assert_eq!(s[3].shift, Point::new(0.0, 15.0));
    assert_eq!(s[3].font.size, Animated::Static(10.0));
}

#[test]
fn preserved_spaces_stay() {
    let import = read(&svg(
        r#"<text xml:space="preserve" x="0" y="20">  two  spaces </text>"#,
    ));
    assert_eq!(text(&import).spans[0].text, "  two  spaces ");
}

#[test]
fn a_position_for_each_character_warns() {
    let import = read(&svg(r#"<text x="0 10 20" y="20">abc</text>"#));
    assert_eq!(
        text(&import).spans[0].position,
        Some(AnimatedPoint::fixed(0.0, 20.0))
    );
    assert!(
        import.warnings.iter().any(|w| w.contains("each character")),
        "{:?}",
        import.warnings
    );
}

#[test]
fn unfilled_or_empty_text_draws_nothing() {
    let import = read(&svg(
        r#"<text x="0" y="20" fill="none">a</text><text x="0" y="20">   </text>"#,
    ));
    fn any_text(g: &Group) -> bool {
        g.children.iter().any(|n| match n {
            Node::Text(_) => true,
            Node::Group(g) => any_text(g),
            _ => false,
        })
    }
    assert!(!any_text(&import.scene.root));
}
