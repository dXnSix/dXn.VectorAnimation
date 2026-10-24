//! Filters as the scene's effects: `<filter>` chains of shadows and blurs
//! (as Figma writes them: the source's alpha hardened, moved, blurred,
//! colored, cut out and blended under the source; `feDropShadow`;
//! `feGaussianBlur` of the source) and CSS `filter` functions (`blur()`,
//! `drop-shadow()`, `url()`), still or animated.

use dxn_vector_animation::{Animated, AnimatedPoint, Blur, Color, DropShadow, Effect, Keyframe};
use roxmltree::Node as XmlNode;

use crate::values::{color, length, number};

/// What a filter primitive's input or result holds, as far as shadows and
/// blurs go.
#[derive(Debug, Clone)]
enum Image {
    /// Nothing.
    Empty,
    /// The element's drawing, blurred.
    Source { sigma: f64 },
    /// Its silhouette, moved, blurred and colored.
    Shade(Shade),
    /// Images over one another, the lowest first.
    Stack(Vec<Image>),
}

#[derive(Debug, Clone, PartialEq)]
struct Shade {
    dx: f64,
    dy: f64,
    sigma: f64,
    /// The color; its alpha multiplies the silhouette's.
    color: Color,
    /// Its alpha turned all or nothing (Figma's `hardAlpha`).
    hard: bool,
    /// Cut out where the drawing is.
    knockout: bool,
}

impl Shade {
    fn source_alpha() -> Shade {
        Shade {
            dx: 0.0,
            dy: 0.0,
            sigma: 0.0,
            color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            hard: false,
            knockout: false,
        }
    }
}

fn combined(a: f64, b: f64) -> f64 {
    (a * a + b * b).sqrt()
}

/// Blurs `image` by `sigma` (a stack's images each on its own).
fn blurred(image: Image, sigma: f64) -> Image {
    match image {
        Image::Empty => Image::Empty,
        Image::Source { sigma: s } => Image::Source {
            sigma: combined(s, sigma),
        },
        Image::Shade(mut shade) => {
            shade.sigma = combined(shade.sigma, sigma);
            Image::Shade(shade)
        }
        Image::Stack(list) => Image::Stack(list.into_iter().map(|i| blurred(i, sigma)).collect()),
    }
}

fn flat(image: Image, out: &mut Vec<Image>) {
    match image {
        Image::Empty => {}
        Image::Stack(list) => list.into_iter().for_each(|i| flat(i, out)),
        other => out.push(other),
    }
}

fn attr<'a>(node: XmlNode<'a, '_>, name: &str) -> Option<&'a str> {
    node.attribute(name)
}

fn num(node: XmlNode, name: &str, default: f64) -> f64 {
    attr(node, name).and_then(number).unwrap_or(default)
}

/// The effects of the `<filter>` `node`, or why it is left out.
pub(crate) fn element(node: XmlNode) -> Result<Vec<Effect>, String> {
    if attr(node, "primitiveUnits") == Some("objectBoundingBox") {
        return Err("primitives in the box's units".into());
    }
    let mut results: Vec<(String, Image)> = Vec::new();
    let mut last = Image::Source { sigma: 0.0 };
    let input =
        |name: Option<&str>, last: &Image, results: &[(String, Image)]| match name.map(str::trim) {
            None | Some("") => Ok(last.clone()),
            Some("SourceGraphic") => Ok(Image::Source { sigma: 0.0 }),
            Some("SourceAlpha") => Ok(Image::Shade(Shade::source_alpha())),
            Some(n) => results
                .iter()
                .rev()
                .find(|(r, _)| r == n)
                .map(|(_, i)| i.clone())
                .ok_or_else(|| format!("the input `{n}`")),
        };
    for primitive in node.children().filter(|n| n.is_element()) {
        let tag = primitive.tag_name().name();
        let a = || input(attr(primitive, "in"), &last, &results);
        let b = || input(attr(primitive, "in2"), &last, &results);
        let out = match tag {
            "feFlood" => {
                if num(primitive, "flood-opacity", 1.0) == 0.0 {
                    Image::Empty
                } else {
                    return Err("a flood of color".into());
                }
            }
            "feColorMatrix" => color_matrix(primitive, a()?)?,
            "feOffset" => match a()? {
                Image::Shade(mut s) => {
                    s.dx += num(primitive, "dx", 0.0);
                    s.dy += num(primitive, "dy", 0.0);
                    Image::Shade(s)
                }
                Image::Empty => Image::Empty,
                _ => return Err("a moved drawing".into()),
            },
            "feGaussianBlur" => {
                let list: Vec<f64> = attr(primitive, "stdDeviation")
                    .unwrap_or("0")
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|p| !p.is_empty())
                    .filter_map(number)
                    .collect();
                let sigma = match list.as_slice() {
                    [s] => *s,
                    [x, y] if (x - y).abs() < 1e-9 => *x,
                    _ => return Err("a blur of two deviations".into()),
                };
                blurred(a()?, sigma.max(0.0))
            }
            "feComposite" => {
                let (top, under) = (a()?, b()?);
                match attr(primitive, "operator").unwrap_or("over") {
                    "over" => Image::Stack(vec![under, top]),
                    "out" => match (top, under) {
                        (Image::Shade(mut s), Image::Shade(cut))
                            if cut.hard && cut.dx == 0.0 && cut.dy == 0.0 && cut.sigma == 0.0 =>
                        {
                            s.knockout = true;
                            Image::Shade(s)
                        }
                        _ => return Err("a composite cut by another".into()),
                    },
                    "arithmetic" => return Err("an inner shadow".into()),
                    other => return Err(format!("a composite `{other}`")),
                }
            }
            "feBlend" => match attr(primitive, "mode").unwrap_or("normal") {
                "normal" => Image::Stack(vec![b()?, a()?]),
                other => return Err(format!("a blend `{other}`")),
            },
            "feMerge" => {
                let mut list = Vec::new();
                for n in primitive
                    .children()
                    .filter(|n| n.has_tag_name("feMergeNode"))
                {
                    list.push(input(attr(n, "in"), &last, &results)?);
                }
                Image::Stack(list)
            }
            "feDropShadow" => {
                let c = attr(primitive, "flood-color")
                    .and_then(color)
                    .unwrap_or(Color::rgba(0.0, 0.0, 0.0, 1.0));
                let opacity = num(primitive, "flood-opacity", 1.0).clamp(0.0, 1.0);
                let sigma = num(primitive, "stdDeviation", 2.0).max(0.0);
                let source = a()?;
                let shade = Shade {
                    dx: num(primitive, "dx", 2.0),
                    dy: num(primitive, "dy", 2.0),
                    sigma,
                    color: c.with_opacity(opacity),
                    hard: false,
                    knockout: false,
                };
                match source {
                    Image::Source { .. } => Image::Stack(vec![Image::Shade(shade), source]),
                    _ => return Err("a drop shadow of a shadow".into()),
                }
            }
            other => return Err(format!("<{other}>")),
        };
        if let Some(name) = attr(primitive, "result") {
            results.push((name.to_owned(), out.clone()));
        }
        last = out;
    }
    let mut list = Vec::new();
    flat(last, &mut list);
    let at = list
        .iter()
        .position(|i| matches!(i, Image::Source { .. }))
        .ok_or("no drawing in the result")?;
    if at + 1 != list.len() {
        return Err("an inner shadow or an overlay".into());
    }
    let Image::Source { sigma: blur } = list[at] else {
        return Err("no drawing in the result".into());
    };
    let mut effects = Vec::new();
    for image in &list[..at] {
        let Image::Shade(s) = image else {
            return Err("a second drawing".into());
        };
        // The blur that follows blurs the shadows too.
        let sigma = (s.sigma * s.sigma - blur * blur).max(0.0).sqrt();
        effects.push(Effect::DropShadow(DropShadow {
            color: Animated::Static(s.color),
            offset: AnimatedPoint::fixed(s.dx, s.dy),
            blur: Animated::Static(sigma),
            knockout: s.knockout,
        }));
    }
    if blur > 0.0 {
        effects.push(Effect::Blur(Blur {
            radius: Animated::Static(blur),
        }));
    }
    Ok(effects)
}

/// An `feColorMatrix` of a silhouette: Figma's hardening of the alpha
/// (`… 0 0 0 127 0`), or its coloring (`0 0 0 0 r … 0 0 0 a 0`).
fn color_matrix(primitive: XmlNode, image: Image) -> Result<Image, String> {
    let Image::Shade(mut s) = image else {
        return Err("a color matrix of a drawing".into());
    };
    if attr(primitive, "type").is_some_and(|t| t != "matrix") {
        return Err("a color matrix other than `matrix`".into());
    }
    let m: Vec<f64> = attr(primitive, "values")
        .unwrap_or("")
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .filter_map(number)
        .collect();
    if m.len() != 20 {
        return Err("a color matrix".into());
    }
    // Colors from constants only, alpha from alpha only.
    let constant_rows = (0..3).all(|r| m[r * 5..r * 5 + 4].iter().all(|v| *v == 0.0));
    let alpha_row = m[15] == 0.0 && m[16] == 0.0 && m[17] == 0.0 && m[19] == 0.0;
    if !constant_rows || !alpha_row {
        return Err("a color matrix".into());
    }
    let k = m[18];
    if k > 1.0 {
        s.hard = true;
    } else {
        s.color = Color::rgba(m[4], m[9], m[14], s.color.a * k).clamped();
    }
    Ok(Image::Shade(s))
}

/// A CSS filter function.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Function {
    /// `blur(<length>)`: the deviation.
    Blur(f64),
    /// `drop-shadow(<color>? <length> <length> <length>?)`.
    DropShadow {
        dx: f64,
        dy: f64,
        sigma: f64,
        color: Color,
    },
    /// `url(#id)`.
    Url(String),
}

/// The functions of a CSS `filter` value, or the first one it cannot read.
pub(crate) fn functions(s: &str) -> Result<Vec<Function>, String> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let open = rest.find('(').ok_or_else(|| rest.to_owned())?;
        let name = rest[..open].trim().to_ascii_lowercase();
        // The matching parenthesis (colors nest theirs).
        let mut depth = 0;
        let mut close = None;
        for (i, c) in rest[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close.ok_or_else(|| rest.to_owned())?;
        let args = rest[open + 1..close].trim();
        out.push(match name.as_str() {
            "blur" => Function::Blur(if args.is_empty() {
                0.0
            } else {
                length(args, 0.0).ok_or_else(|| format!("blur({args})"))?
            }),
            "drop-shadow" => drop_shadow(args).ok_or_else(|| format!("drop-shadow({args})"))?,
            "url" => Function::Url(
                args.trim_matches(|c| c == '"' || c == '\'')
                    .trim_start_matches('#')
                    .to_owned(),
            ),
            _ => return Err(format!("{name}()")),
        });
        rest = rest[close + 1..].trim_start();
    }
    Ok(out)
}

/// `drop-shadow()`'s arguments: two or three lengths and a color, either
/// side; its blur is a radius, twice the deviation.
fn drop_shadow(args: &str) -> Option<Function> {
    let mut lengths = Vec::new();
    let mut c = None;
    let mut rest = args;
    while !rest.trim().is_empty() {
        rest = rest.trim_start();
        // A color function, or one word.
        let end = if let Some(open) = rest.find('(')
            && rest[..open].chars().all(|ch| ch.is_ascii_alphabetic())
        {
            rest[open..].find(')')? + open + 1
        } else {
            rest.find(char::is_whitespace).unwrap_or(rest.len())
        };
        let word = &rest[..end];
        match length(word, 0.0) {
            Some(v) => lengths.push(v),
            None => c = Some(color(word)?),
        }
        rest = &rest[end..];
    }
    let (dx, dy, blur) = match lengths.as_slice() {
        [dx, dy] => (*dx, *dy, 0.0),
        [dx, dy, blur] => (*dx, *dy, *blur),
        _ => return None,
    };
    Some(Function::DropShadow {
        dx,
        dy,
        sigma: blur.max(0.0) / 2.0,
        color: c.unwrap_or(Color::rgba(0.0, 0.0, 0.0, 1.0)),
    })
}

/// The effects of CSS filter functions animated through `keys` (each
/// keyframe's functions alike), or `None` where they differ.
pub(crate) fn animated(keys: &[Keyframe<Vec<Function>>]) -> Option<Vec<Effect>> {
    let first = keys.first()?;
    let alike = keys.iter().all(|k| {
        k.value.len() == first.value.len()
            && k.value
                .iter()
                .zip(&first.value)
                .all(|(a, b)| std::mem::discriminant(a) == std::mem::discriminant(b))
    });
    if !alike {
        return None;
    }
    let track = |f: &dyn Fn(&Function) -> f64, i: usize| {
        Animated::from_keyframes(
            keys.iter()
                .map(|k| Keyframe::new(k.time, f(&k.value[i]), k.easing.clone()))
                .collect(),
            0.0,
        )
    };
    let mut out = Vec::new();
    for (i, function) in first.value.iter().enumerate() {
        match function {
            Function::Blur(_) => out.push(Effect::Blur(Blur {
                radius: track(&|f| if let Function::Blur(v) = f { *v } else { 0.0 }, i),
            })),
            Function::DropShadow { .. } => {
                let part = |f: &Function| match f {
                    Function::DropShadow {
                        dx,
                        dy,
                        sigma,
                        color,
                    } => (*dx, *dy, *sigma, *color),
                    _ => (0.0, 0.0, 0.0, Color::TRANSPARENT),
                };
                let colors = Animated::from_keyframes(
                    keys.iter()
                        .map(|k| Keyframe::new(k.time, part(&k.value[i]).3, k.easing.clone()))
                        .collect(),
                    Color::TRANSPARENT,
                );
                out.push(Effect::DropShadow(DropShadow {
                    color: colors,
                    offset: AnimatedPoint {
                        x: track(&|f| part(f).0, i),
                        y: track(&|f| part(f).1, i),
                    },
                    blur: track(&|f| part(f).2, i),
                    knockout: false,
                }));
            }
            Function::Url(_) => return None,
        }
    }
    Some(out)
}
