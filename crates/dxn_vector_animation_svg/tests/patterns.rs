//! Pattern fills: `<pattern>` tiles in user space or the shape's box, their
//! content in user space, the box's fractions or a view box, transformed,
//! inheriting through `href`; a pattern filling with itself is left out.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{GradientUnits, Group, Import, Matrix, Node, Paint, Pattern, Rect};

fn read(svg: &str) -> Import {
    dxn_vector_animation_svg::read(svg.as_bytes()).unwrap()
}

/// The fill of the first shape.
fn fill(import: &Import) -> Option<Paint> {
    fn find(g: &Group) -> Option<Paint> {
        g.children.iter().find_map(|n| match n {
            Node::Shape(s) => s.fill.as_ref().map(|f| f.paint.clone()),
            Node::Group(g) => find(g),
            Node::Image(_) => None,
        })
    }
    find(&import.scene.root)
}

fn pattern(import: &Import) -> Pattern {
    match fill(import) {
        Some(Paint::Pattern(p)) => *p,
        other => panic!("{other:?} ({:?})", import.warnings),
    }
}

#[test]
fn a_tiling_in_user_space() {
    let import = read(
        r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
<defs><pattern id="p" x="5" y="5" width="20" height="10" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
<rect width="10" height="10" fill="#0f0"/></pattern></defs>
<rect width="100" height="100" fill="url(#p)"/></svg>"##,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let p = pattern(&import);
    assert_eq!(p.tile, Rect::new(5.0, 5.0, 20.0, 10.0));
    assert_eq!(p.units, GradientUnits::UserSpace);
    assert_eq!(p.content_units, GradientUnits::UserSpace);
    assert!(p.view_box.is_none());
    assert!((p.transform.b - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9);
    assert_eq!(p.content.children.len(), 1);
}

#[test]
fn figmas_image_fill_in_the_boxs_units() {
    // As Figma exports an image fill: one tile the size of the box, the
    // content scaled to it.
    let import = read(
        r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
<rect x="10" y="10" width="80" height="40" fill="url(#pattern0)"/>
<defs><pattern id="pattern0" patternContentUnits="objectBoundingBox" width="1" height="1">
<use xlink:href="#shape0" transform="scale(0.01 0.02)"/></pattern>
<rect id="shape0" width="100" height="50" fill="#123456"/></defs></svg>"##,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let p = pattern(&import);
    assert_eq!(p.tile, Rect::new(0.0, 0.0, 1.0, 1.0));
    assert_eq!(p.units, GradientUnits::BoundingBox);
    assert_eq!(p.content_units, GradientUnits::BoundingBox);
    // The frame maps the tile onto the box and the content into it.
    let frame = import.scene.frame_at(0.0);
    let item = frame
        .items
        .iter()
        .find_map(|i| match i {
            dxn_vector_animation::Item::Fill { paint, .. } => Some(paint.clone()),
            _ => None,
        })
        .unwrap();
    let dxn_vector_animation::FramePaint::Pattern { tile, items, .. } = item else {
        panic!("{item:?}");
    };
    assert_eq!(tile, Rect::new(10.0, 10.0, 80.0, 40.0));
    assert!(!items.is_empty());
}

#[test]
fn a_view_box_and_inherited_attributes() {
    let import = read(
        r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
<defs><pattern id="base" width="0.25" height="0.5" viewBox="0 0 4 4" preserveAspectRatio="xMinYMax slice">
<circle cx="2" cy="2" r="2" fill="red"/></pattern>
<pattern id="p" xlink:href="#base" x="0.1"/></defs>
<rect width="100" height="100" fill="url(#p)"/></svg>"##,
    );
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let p = pattern(&import);
    assert_eq!(p.tile, Rect::new(0.1, 0.0, 0.25, 0.5));
    let vb = p.view_box.unwrap();
    assert_eq!(vb.rect, Rect::new(0.0, 0.0, 4.0, 4.0));
    assert_eq!(vb.align, Some((0.0, 1.0)));
    assert!(vb.slice);
    // Placed in a 25 × 50 tile: scaled to cover, its foot at the foot.
    let m = vb.matrix(25.0, 50.0);
    assert_eq!(m, Matrix::new(12.5, 0.0, 0.0, 12.5, 0.0, 0.0));
}

#[test]
fn a_pattern_filling_with_itself_is_left_out() {
    let import = read(
        r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
<defs><pattern id="p" width="10" height="10" patternUnits="userSpaceOnUse">
<rect width="5" height="5" fill="url(#p)"/><rect x="5" width="5" height="5" fill="blue"/></pattern></defs>
<rect width="100" height="100" fill="url(#p)"/></svg>"##,
    );
    let p = pattern(&import);
    // The inner rectangle filled with the pattern itself draws nothing.
    assert!(
        import.warnings.iter().any(|w| w.contains("itself")),
        "{:?}",
        import.warnings
    );
    assert!(!p.content.children.is_empty());
}
