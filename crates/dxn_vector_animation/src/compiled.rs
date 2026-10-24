//! The compiled scene format (`.vector-animation.json`): a scene saved as
//! JSON with its format name and version, so a player loads it without
//! reading SVG or Lottie again.

use crate::{Error, FORMAT, FORMAT_VERSION, Limits, Scene};

#[derive(serde::Serialize)]
struct Out<'a> {
    format: &'static str,
    version: u32,
    scene: &'a Scene,
}

#[derive(serde::Deserialize)]
struct In {
    format: String,
    version: u32,
    scene: Scene,
}

/// `scene` in the compiled format.
pub fn to_json(scene: &Scene, pretty: bool) -> String {
    let out = Out {
        format: FORMAT,
        version: FORMAT_VERSION,
        scene,
    };
    let text = if pretty {
        serde_json::to_string_pretty(&out)
    } else {
        serde_json::to_string(&out)
    };
    // A scene holds only strings, numbers and finite structures.
    text.unwrap_or_default()
}

/// Whether `data` is a compiled scene.
pub fn sniff(data: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&data[..data.len().min(256)]);
    head.trim_start().starts_with('{') && head.contains(FORMAT)
}

/// Reads a compiled scene, checked against `limits`.
pub fn from_json(data: &[u8], limits: &Limits) -> Result<Scene, Error> {
    if data.len() > limits.max_bytes {
        return Err(Error::Limit {
            what: "bytes",
            limit: limits.max_bytes as f64,
        });
    }
    let parsed: In = serde_json::from_slice(data).map_err(|e| Error::Invalid(e.to_string()))?;
    if parsed.format != FORMAT {
        return Err(Error::Invalid(format!("not a {FORMAT} file")));
    }
    if parsed.version > FORMAT_VERSION {
        return Err(Error::Unsupported(format!(
            "format version {} (this library reads up to {FORMAT_VERSION})",
            parsed.version
        )));
    }
    parsed.scene.check(limits)?;
    Ok(parsed.scene)
}
