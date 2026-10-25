//! Filters as effects: Figma's `<filter>` chains of drop and inner shadows
//! and blurs, `feDropShadow`, `feGaussianBlur`, color changes
//! (`feColorMatrix`, `feComponentTransfer`), CSS `blur()`, `drop-shadow()`
//! and the color functions, still and animated; other filters are left
//! out with a warning.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, AnimatedPoint, Blur, Color, ColorEffect, ColorMatrix, ColorOp, DropShadow, Effect,
    Group, Import, InnerShadow, Node, Transfer,
};

fn read(svg: &str) -> Import {
    dxn_vector_animation_svg::read(svg.as_bytes()).unwrap()
}

/// The effects of the group of id `a`.
fn effects(import: &Import) -> Vec<Effect> {
    fn find(g: &Group) -> Option<&Group> {
        if g.id.as_deref() == Some("a") {
            return Some(g);
        }
        g.children.iter().find_map(|n| match n {
            Node::Group(g) => find(g),
            _ => None,
        })
    }
    find(&import.scene.root)
        .map(|g| g.effects.clone())
        .unwrap_or_default()
}

fn shadow(rgba: (f64, f64, f64, f64), dx: f64, dy: f64, blur: f64, knockout: bool) -> Effect {
    Effect::DropShadow(DropShadow {
        color: Animated::Static(Color::rgba(rgba.0, rgba.1, rgba.2, rgba.3)),
        offset: AnimatedPoint::fixed(dx, dy),
        blur: Animated::Static(blur),
        knockout,
        hard: false,
    })
}

/// A shadow cast by the drawing's silhouette made all or nothing.
fn hard(effect: Effect) -> Effect {
    match effect {
        Effect::DropShadow(s) => Effect::DropShadow(DropShadow { hard: true, ..s }),
        other => other,
    }
}

fn with_filter(filter: &str) -> String {
    format!(
        r##"<svg width="100" height="100" viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
<g id="a" filter="url(#f)"><rect x="20" y="20" width="40" height="40" fill="#F24822"/></g>
<defs><filter id="f" x="0" y="0" width="100" height="100" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">{filter}</filter></defs>
</svg>"##
    )
}

/// A drop shadow as Figma exports one.
const FIGMA_SHADOW: &str = r#"<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="4"/>
<feGaussianBlur stdDeviation="2"/>
<feComposite in2="hardAlpha" operator="out"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.25 0"/>
<feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow_1_2"/>
<feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow_1_2" result="shape"/>"#;

#[test]
fn figmas_drop_shadow() {
    let import = read(&with_filter(FIGMA_SHADOW));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(
        effects(&import),
        vec![hard(shadow((0.0, 0.0, 0.0, 0.25), 0.0, 4.0, 2.0, true))]
    );
}

#[test]
fn figmas_two_drop_shadows_both_cast_by_the_drawing() {
    let filter = r#"<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dx="2" dy="2"/>
<feGaussianBlur stdDeviation="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"/>
<feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="8"/>
<feGaussianBlur stdDeviation="6"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0 0 0 0.3 0"/>
<feBlend mode="normal" in2="effect1_dropShadow" result="effect2_dropShadow"/>
<feBlend mode="normal" in="SourceGraphic" in2="effect2_dropShadow" result="shape"/>"#;
    let import = read(&with_filter(filter));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(
        effects(&import),
        vec![
            hard(shadow((1.0, 0.0, 0.0, 0.5), 2.0, 2.0, 1.0, false)),
            hard(shadow((0.0, 0.0, 1.0, 0.3), 0.0, 8.0, 6.0, false)),
        ]
    );
}

#[test]
fn figmas_layer_blur() {
    let filter = r#"<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
<feGaussianBlur stdDeviation="5" result="effect1_foregroundBlur_1_2"/>"#;
    let import = read(&with_filter(filter));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(
        effects(&import),
        vec![Effect::Blur(Blur {
            radius: Animated::Static(5.0)
        })]
    );
}

#[test]
fn fe_drop_shadow() {
    let import = read(&with_filter(
        r##"<feDropShadow dx="3" dy="-2" stdDeviation="4" flood-color="#00ff00" flood-opacity="0.5"/>"##,
    ));
    assert_eq!(
        effects(&import),
        vec![shadow((0.0, 1.0, 0.0, 0.5), 3.0, -2.0, 4.0, false)]
    );
}

/// An inner shadow as Figma exports one.
const FIGMA_INNER_SHADOW: &str = r#"<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="4"/>
<feGaussianBlur stdDeviation="2"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 0.1 0 0 0 0 0.2 0 0 0 0 0.3 0 0 0 0.25 0"/>"#;

fn inner(rgba: (f64, f64, f64, f64), dx: f64, dy: f64, blur: f64) -> Effect {
    Effect::InnerShadow(InnerShadow {
        color: Animated::Static(Color::rgba(rgba.0, rgba.1, rgba.2, rgba.3)),
        offset: AnimatedPoint::fixed(dx, dy),
        blur: Animated::Static(blur),
        hard: true,
    })
}

#[test]
fn figmas_inner_shadow() {
    let filter = format!(
        r#"<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
{FIGMA_INNER_SHADOW}
<feBlend mode="normal" in2="shape" result="effect1_innerShadow_1_2"/>"#
    );
    let import = read(&with_filter(&filter));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(
        effects(&import),
        vec![inner((0.1, 0.2, 0.3, 0.25), 0.0, 4.0, 2.0)]
    );
}

#[test]
fn figmas_drop_and_inner_shadows_and_blur() {
    // A drop shadow under, two inner shadows over, all blurred after.
    let filter = format!(
        r#"<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="2"/>
<feGaussianBlur stdDeviation="3"/>
<feComposite in2="hardAlpha" operator="out"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"/>
<feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow"/>
<feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow" result="shape"/>
{FIGMA_INNER_SHADOW}
<feBlend mode="normal" in2="shape" result="effect2_innerShadow"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dx="-3"/>
<feGaussianBlur stdDeviation="1"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.5 0"/>
<feBlend mode="normal" in2="effect2_innerShadow" result="effect3_innerShadow"/>
<feGaussianBlur stdDeviation="1.5" result="effect4_foregroundBlur"/>"#
    );
    let import = read(&with_filter(&filter));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let list = effects(&import);
    assert_eq!(list.len(), 4, "{list:?}");
    let Effect::DropShadow(drop) = &list[0] else {
        panic!("{list:?}");
    };
    // Blurred again by the blur after, as all of it is.
    assert!((drop.blur.sample(0.0) - 3.0).abs() < 1e-9);
    assert!(drop.knockout && drop.hard);
    assert_eq!(list[1], inner((0.1, 0.2, 0.3, 0.25), 0.0, 4.0, 2.0));
    assert_eq!(list[2], inner((1.0, 1.0, 1.0, 0.5), -3.0, 0.0, 1.0));
    assert_eq!(
        list[3],
        Effect::Blur(Blur {
            radius: Animated::Static(1.5)
        })
    );
}

#[test]
fn an_inner_shadow_blurred_apart_from_the_drawing_is_left_out() {
    // The inner shadow blurred again on its own after it was cast.
    let filter = format!(
        r#"<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
{FIGMA_INNER_SHADOW}
<feGaussianBlur stdDeviation="3"/>
<feBlend mode="normal" in2="shape"/>"#
    );
    let import = read(&with_filter(&filter));
    assert!(effects(&import).is_empty());
    assert!(
        import.warnings.iter().any(|w| w.contains("inner shadow")),
        "{:?}",
        import.warnings
    );
}

#[test]
fn color_matrices_and_transfers_of_the_drawing() {
    let filter = r#"<feColorMatrix type="saturate" values="0.3"/>
<feColorMatrix type="hueRotate" values="90"/>
<feComponentTransfer><feFuncR type="linear" slope="0.5" intercept="0.25"/><feFuncG type="table" tableValues="0 0.5 1"/><feFuncA type="gamma" amplitude="1" exponent="2" offset="0"/></feComponentTransfer>
<feColorMatrix type="matrix" values="1 0 0 0 0 0 1 0 0 0 0 0 1 0 0 0 0 0 0.5 0" color-interpolation-filters="linearRGB"/>"#;
    let import = read(&with_filter(filter));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let mut half = ColorMatrix::IDENTITY;
    half.0[18] = 0.5;
    assert_eq!(
        effects(&import),
        vec![
            Effect::Color(ColorEffect {
                ops: vec![
                    ColorOp::Saturate {
                        amount: Animated::Static(0.3)
                    },
                    ColorOp::HueRotate {
                        degrees: Animated::Static(90.0)
                    },
                    ColorOp::Transfer {
                        red: Transfer::Linear {
                            slope: 0.5,
                            intercept: 0.25
                        },
                        green: Transfer::Table {
                            values: vec![0.0, 0.5, 1.0]
                        },
                        blue: Transfer::Identity,
                        alpha: Transfer::Gamma {
                            amplitude: 1.0,
                            exponent: 2.0,
                            offset: 0.0
                        },
                    },
                ],
                linear: false,
            }),
            Effect::Color(ColorEffect {
                ops: vec![ColorOp::Matrix {
                    matrix: Animated::Static(half)
                }],
                linear: true,
            }),
        ]
    );
}

#[test]
fn a_color_matrix_of_a_filter_without_its_light_works_in_linear_light() {
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
<g id="a" filter="url(#f)"><rect x="20" y="20" width="40" height="40" fill="#F24822"/></g>
<filter id="f"><feColorMatrix type="saturate" values="0"/></filter></svg>"##;
    let import = read(svg);
    let list = effects(&import);
    assert!(
        matches!(&list[..], [Effect::Color(c)] if c.linear),
        "{list:?}"
    );
}

#[test]
fn shadows_of_a_recolored_drawing_are_left_out_with_a_warning() {
    let filter = r#"<feColorMatrix in="SourceGraphic" type="saturate" values="0" result="gray"/>
<feDropShadow in="gray" dx="2" dy="2" stdDeviation="1"/>"#;
    let import = read(&with_filter(filter));
    assert!(effects(&import).is_empty());
    assert!(
        import.warnings.iter().any(|w| w.contains("colors changed")),
        "{:?}",
        import.warnings
    );
}

fn styled(css: &str) -> String {
    format!(
        r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg"><style>{css}</style>
<g id="a"><rect x="20" y="20" width="40" height="40" fill="#F24822"/></g></svg>"##
    )
}

#[test]
fn css_blur_and_drop_shadow() {
    let import = read(&styled(
        "#a { filter: blur(3px) drop-shadow(2px 4px 6px rgba(0, 0, 255, 0.4)); }",
    ));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!(
        effects(&import),
        vec![
            Effect::Blur(Blur {
                radius: Animated::Static(3.0)
            }),
            // A drop-shadow's blur is a radius: twice the deviation.
            shadow((0.0, 0.0, 1.0, 0.4), 2.0, 4.0, 3.0, false),
        ]
    );
}

#[test]
fn css_blur_animated() {
    let import = read(&styled(
        "#a { animation: k 2s linear both; } @keyframes k { from { filter: blur(0px); } to { filter: blur(8px); } }",
    ));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let effects = effects(&import);
    let [Effect::Blur(blur)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(blur.radius.sample(0.0), 0.0);
    assert_eq!(blur.radius.sample(1.0), 4.0);
    assert_eq!(blur.radius.sample(2.0), 8.0);
}

#[test]
fn css_color_functions() {
    let import = read(&styled(
        "#a { filter: grayscale(50%) hue-rotate(0.25turn) blur(2px) invert() opacity(1.5) brightness(1.2) contrast(80%) sepia(1) saturate(2); }",
    ));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let s = |v: f64| Animated::Static(v);
    assert_eq!(
        effects(&import),
        vec![
            Effect::Color(ColorEffect {
                ops: vec![
                    ColorOp::Grayscale { amount: s(0.5) },
                    ColorOp::HueRotate { degrees: s(90.0) },
                ],
                linear: false,
            }),
            Effect::Blur(Blur { radius: s(2.0) }),
            Effect::Color(ColorEffect {
                ops: vec![
                    ColorOp::Invert { amount: s(1.0) },
                    // Past the full change is the full change.
                    ColorOp::Opacity { amount: s(1.0) },
                    ColorOp::Brightness { amount: s(1.2) },
                    ColorOp::Contrast { amount: s(0.8) },
                    ColorOp::Sepia { amount: s(1.0) },
                    ColorOp::Saturate { amount: s(2.0) },
                ],
                linear: false,
            }),
        ]
    );
}

#[test]
fn css_hue_rotation_animated() {
    let import = read(&styled(
        "#a { animation: k 2s linear both; } @keyframes k { from { filter: hue-rotate(0deg) saturate(1); } to { filter: hue-rotate(360deg) saturate(0); } }",
    ));
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    let effects = effects(&import);
    let [Effect::Color(color)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let [ColorOp::HueRotate { degrees }, ColorOp::Saturate { amount }] = color.ops.as_slice()
    else {
        panic!("{color:?}");
    };
    // Angles move, not matrices: halfway is a half turn.
    assert_eq!(degrees.sample(1.0), 180.0);
    assert_eq!(amount.sample(1.0), 0.5);
}

#[test]
fn other_css_filters_are_left_out_with_a_warning() {
    let import = read(&styled("#a { filter: shimmer(1); }"));
    assert!(effects(&import).is_empty());
    assert!(
        import.warnings.iter().any(|w| w.contains("shimmer")),
        "{:?}",
        import.warnings
    );
}
