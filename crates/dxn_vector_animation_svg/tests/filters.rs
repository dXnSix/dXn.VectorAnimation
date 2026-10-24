//! Filters of shadows and blurs as effects: Figma's `<filter>` chains,
//! `feDropShadow`, `feGaussianBlur`, CSS `blur()` and `drop-shadow()`,
//! still and animated; other filters are left out with a warning.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    Animated, AnimatedPoint, Blur, Color, DropShadow, Effect, Group, Import, Node,
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
    })
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
        vec![shadow((0.0, 0.0, 0.0, 0.25), 0.0, 4.0, 2.0, true)]
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
            shadow((1.0, 0.0, 0.0, 0.5), 2.0, 2.0, 1.0, false),
            shadow((0.0, 0.0, 1.0, 0.3), 0.0, 8.0, 6.0, false),
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

#[test]
fn an_inner_shadow_is_left_out_with_a_warning() {
    let filter = r#"<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dy="4"/>
<feGaussianBlur stdDeviation="2"/>
<feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
<feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.25 0"/>
<feBlend mode="normal" in2="shape" result="effect1_innerShadow_1_2"/>"#;
    let import = read(&with_filter(filter));
    assert!(effects(&import).is_empty());
    assert!(
        import.warnings.iter().any(|w| w.contains("inner shadow")),
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
fn other_css_filters_are_left_out_with_a_warning() {
    let import = read(&styled("#a { filter: grayscale(1); }"));
    assert!(effects(&import).is_empty());
    assert!(
        import.warnings.iter().any(|w| w.contains("grayscale")),
        "{:?}",
        import.warnings
    );
}
