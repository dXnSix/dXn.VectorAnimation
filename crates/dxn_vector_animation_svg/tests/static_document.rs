//! The static document: shapes, styles, paints, clips, masks, `use`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{FramePaint, Item, LineCap, MaskKind, Node};

fn items(svg: &str) -> Vec<Item> {
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    import.scene.frame_at(0.0).items
}

#[test]
fn sniffs_svg_documents() {
    assert!(dxn_vector_animation_svg::sniff(
        b"<?xml version=\"1.0\"?>\n<!-- x --><svg/>"
    ));
    assert!(!dxn_vector_animation_svg::sniff(b"{\"v\":\"5.7\"}"));
}

#[test]
fn the_view_box_scales_into_the_viewport() {
    let svg = r#"<svg width="200" height="100" viewBox="0 0 20 10" xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10" fill="red"/></svg>"#;
    let list = items(svg);
    let [Item::Fill { transform, .. }] = list.as_slice() else {
        panic!()
    };
    assert_eq!(transform.a, 10.0);
}

#[test]
fn style_sheets_cascade() {
    let svg = r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg">
      <style>.a { fill: blue } rect#b { fill: lime }</style>
      <g fill="red"><rect class="a" width="5" height="5"/><rect id="b" class="a" width="5" height="5"/><rect width="5" height="5" style="fill: #000"/></g>
    </svg>"#;
    let colors: Vec<_> = items(svg)
        .into_iter()
        .filter_map(|i| match i {
            Item::Fill {
                paint: FramePaint::Solid(c),
                ..
            } => Some((c.r, c.g, c.b)),
            _ => None,
        })
        .collect();
    assert_eq!(
        colors,
        vec![(0.0, 0.0, 1.0), (0.0, 1.0, 0.0), (0.0, 0.0, 0.0)]
    );
}

#[test]
fn fill_none_inherits_and_strokes_draw() {
    let svg = r#"<svg width="10" height="10" fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M0 0L10 10" stroke="black" stroke-width="2" stroke-linecap="round" stroke-dasharray="1 2"/>
    </svg>"#;
    let list = items(svg);
    let [Item::Stroke { style, .. }] = list.as_slice() else {
        panic!()
    };
    assert_eq!(style.width, 2.0);
    assert_eq!(style.cap, LineCap::Round);
    assert_eq!(style.dash.as_ref().unwrap().0, vec![1.0, 2.0]);
}

#[test]
fn gradients_and_alpha_masks() {
    let import =
        dxn_vector_animation_svg::read(include_bytes!("../../../fixtures/figma-motion.svg"))
            .unwrap();
    let masked = import.scene.root.children.iter().find_map(|n| match n {
        Node::Group(g) if g.mask.is_some() => Some(g),
        _ => None,
    });
    let mask = masked.unwrap().mask.as_ref().unwrap();
    assert_eq!(mask.kind, MaskKind::Alpha);
    let frame = import.scene.frame_at(0.0);
    let gradient = frame.items.iter().find_map(|i| match i {
        Item::Fill {
            paint: FramePaint::Linear { stops, .. },
            ..
        } => Some(stops.clone()),
        _ => None,
    });
    let stops = gradient.unwrap();
    assert_eq!(stops.len(), 2);
    assert!((stops[1].color.a - 0.5).abs() < 1e-9);
}

#[test]
fn use_and_clip_paths() {
    let svg = r##"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
      <defs><rect id="r" width="4" height="4"/><clipPath id="c"><circle cx="5" cy="5" r="5"/></clipPath></defs>
      <use xlink:href="#r" x="3" y="2" fill="green" clip-path="url(#c)"/>
    </svg>"##;
    let list = items(svg);
    let Item::PushLayer(layer) = &list[0] else {
        panic!("{list:?}")
    };
    assert_eq!(layer.clip.as_ref().unwrap().len(), 1);
    let Item::Fill { transform, .. } = &list[1] else {
        panic!()
    };
    assert_eq!((transform.e, transform.f), (3.0, 2.0));
}

#[test]
fn foreign_objects_are_reported() {
    let svg =
        r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg"><foreignObject/></svg>"#;
    let import = dxn_vector_animation_svg::read(svg.as_bytes()).unwrap();
    assert!(
        import
            .warnings
            .iter()
            .any(|w| w.contains("<foreignObject>"))
    );
}

#[test]
fn limits_refuse_large_files() {
    let limits = dxn_vector_animation::Limits {
        max_bytes: 10,
        ..Default::default()
    };
    let err =
        dxn_vector_animation_svg::read_with(b"<svg xmlns='http://www.w3.org/2000/svg'/>", &limits)
            .unwrap_err();
    assert!(matches!(err, dxn_vector_animation::Error::Limit { .. }));
}
