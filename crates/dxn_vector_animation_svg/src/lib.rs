//! Reads animated SVG into a [`dxn_vector_animation`] scene.
//!
//! The static document: shapes, paths, groups and their transforms, fills
//! and strokes, linear and radial gradients, clip paths, masks, opacity,
//! blend modes, `use`, pattern fills, embedded PNG and JPEG images, live
//! text (`<text>`, `<tspan>`), and the `<style>`
//! sheets that style them (simple selectors). Its CSS animations, as Figma
//! Motion writes them: `@keyframes` with a timing function per stop, the
//! `animation` shorthand and longhands (delay, iterations, direction, fill
//! mode), `transform`, `transform-origin`, `transform-box`, `translate`,
//! `rotate`, `scale`, `opacity`, motion along a path (`offset-path`,
//! `offset-distance`, `offset-rotate`, `offset-anchor`), fill and stroke
//! colors and opacities, stroke width and dashes, `d`, and `filter`.
//! Filters: `<filter>` chains as Figma writes them (drop and inner shadows,
//! layer blurs), `feDropShadow`, `feGaussianBlur`, color changes
//! (`feColorMatrix`, `feComponentTransfer`), and CSS `blur()`,
//! `drop-shadow()` and the color functions.
//!
//! What a file holds beyond that (other filters, text on a path) is left
//! out with a warning in [`Import::warnings`].

mod animate;
mod convert;
mod css;
mod filter;
mod smil;
mod values;

use dxn_vector_animation::{Error, Import, Limits};

/// Whether `data` looks like an SVG document (not compressed).
pub fn sniff(data: &[u8]) -> bool {
    let text = match std::str::from_utf8(&data[..data.len().min(4096)]) {
        Ok(t) => t,
        // A cut in the middle of a character.
        Err(e) => match std::str::from_utf8(&data[..e.valid_up_to()]) {
            Ok(t) => t,
            Err(_) => return false,
        },
    };
    let mut rest = text.trim_start_matches('\u{feff}').trim_start();
    loop {
        if rest.starts_with("<?") {
            rest = rest.split_once("?>").map_or("", |(_, r)| r).trim_start();
        } else if rest.starts_with("<!--") {
            rest = rest.split_once("-->").map_or("", |(_, r)| r).trim_start();
        } else if rest.starts_with("<!") {
            rest = rest.split_once('>').map_or("", |(_, r)| r).trim_start();
        } else {
            return rest.starts_with("<svg");
        }
    }
}

/// Reads the SVG document `data` with the default [`Limits`].
pub fn read(data: &[u8]) -> Result<Import, Error> {
    read_with(data, &Limits::default())
}

/// Reads the SVG document `data`, refusing it when it is over `limits`.
pub fn read_with(data: &[u8], limits: &Limits) -> Result<Import, Error> {
    if data.len() > limits.max_bytes {
        return Err(Error::Limit {
            what: "bytes",
            limit: limits.max_bytes as f64,
        });
    }
    if data.starts_with(&[0x1f, 0x8b]) {
        return Err(Error::Unsupported(
            "compressed SVG (.svgz): unpack it first".into(),
        ));
    }
    let text = std::str::from_utf8(data)
        .map_err(|_| Error::Invalid("the document is not UTF-8".into()))?;
    let text = text.trim_start_matches('\u{feff}');
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        nodes_limit: u32::try_from(limits.max_nodes.saturating_mul(8)).unwrap_or(u32::MAX),
        ..roxmltree::ParsingOptions::default()
    };
    let doc = roxmltree::Document::parse_with_options(text, options)
        .map_err(|e| Error::Invalid(e.to_string()))?;
    let (scene, warnings) = convert::Converter::run(&doc, *limits)?;
    scene.check(limits)?;
    Ok(Import {
        scene,
        warnings: warnings.into_iter().collect(),
    })
}
