//! Reads Lottie animations into a [`dxn_vector_animation`] scene.
//!
//! Shape layers (groups, rectangles, ellipses, polystars and paths, fills,
//! strokes with dashes, linear and radial gradients, trim paths), null
//! layers as parents, nested compositions with time stretch, offset and
//! remapping (clipped to their size), solid layers, embedded images,
//! layer masks (add, subtract, intersect, inverted) and track mattes
//! (alpha, luminance, inverted), blend modes, keyframes with their bezier
//! easings and holds, and position keyframes with spatial tangents as
//! motion paths (auto-orient included).
//!
//! What a file holds beyond that (expressions, text layers, effects,
//! repeaters, merge paths) is left out with a warning in
//! [`Import::warnings`]. dotLottie archives are read by unpacking their
//! animation JSON first.

mod convert;
mod props;

use dxn_vector_animation::{Error, Import, Limits};

/// Whether `data` looks like a Lottie JSON document.
pub fn sniff(data: &[u8]) -> bool {
    let head = &data[..data.len().min(64 * 1024)];
    let Some(first) = head.iter().find(|b| !b.is_ascii_whitespace()) else {
        return false;
    };
    if *first != b'{' {
        return false;
    }
    let text = String::from_utf8_lossy(head);
    text.contains("\"layers\"") && text.contains("\"fr\"")
}

/// Reads the Lottie JSON document `data` with the default [`Limits`].
pub fn read(data: &[u8]) -> Result<Import, Error> {
    read_with(data, &Limits::default())
}

/// Reads the Lottie JSON document `data`, refusing it when it is over
/// `limits`.
pub fn read_with(data: &[u8], limits: &Limits) -> Result<Import, Error> {
    if data.len() > limits.max_bytes {
        return Err(Error::Limit {
            what: "bytes",
            limit: limits.max_bytes as f64,
        });
    }
    if data.starts_with(b"PK\x03\x04") {
        return Err(Error::Unsupported(
            "dotLottie archives: unpack the animation's JSON first".into(),
        ));
    }
    let root: serde_json::Value =
        serde_json::from_slice(data).map_err(|e| Error::Invalid(e.to_string()))?;
    let (scene, warnings) = convert::Converter::run(&root, *limits)?;
    scene.check(limits)?;
    Ok(Import {
        scene,
        warnings: warnings.into_iter().collect(),
    })
}
