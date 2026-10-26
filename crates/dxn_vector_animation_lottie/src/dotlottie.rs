//! dotLottie archives: a ZIP of `manifest.json`, the animations' JSON
//! (`animations/<id>.json`) and their images (`images/` in version 1,
//! `i/` in version 2), read within the limits: the first animation, its
//! images embedded.

use base64::Engine;
use dxn_vector_animation::{Error, Limits};
use serde_json::Value;

/// The most entries an archive may list.
const MAX_ENTRIES: usize = 10_000;

/// Whether `data` is a ZIP archive.
pub(crate) fn sniff(data: &[u8]) -> bool {
    data.starts_with(b"PK\x03\x04")
}

fn invalid(what: &str) -> Error {
    Error::Invalid(format!("dotLottie archive: {what}"))
}

fn u16_at(data: &[u8], at: usize) -> Result<usize, Error> {
    data.get(at..at + 2)
        .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
        .ok_or_else(|| invalid("cut short"))
}

fn u32_at(data: &[u8], at: usize) -> Result<usize, Error> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
        .ok_or_else(|| invalid("cut short"))
}

/// One file of the archive.
struct Entry {
    name: String,
    method: usize,
    compressed: usize,
    size: usize,
    offset: usize,
}

/// The archive's entries, from its central directory.
fn entries(data: &[u8]) -> Result<Vec<Entry>, Error> {
    // The end of central directory record, its comment at most 64 KiB.
    let from = data.len().saturating_sub(22 + 0xffff);
    let end = (from..data.len().saturating_sub(21))
        .rev()
        .find(|&i| data[i..].starts_with(b"PK\x05\x06"))
        .ok_or_else(|| invalid("no central directory"))?;
    let count = u16_at(data, end + 10)?;
    let mut at = u32_at(data, end + 16)?;
    if count == 0xffff || at == 0xffff_ffff {
        return Err(Error::Unsupported("ZIP64 dotLottie archives".into()));
    }
    if count > MAX_ENTRIES {
        return Err(Error::Limit {
            what: "archive entries",
            limit: MAX_ENTRIES as f64,
        });
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if !data.get(at..).is_some_and(|d| d.starts_with(b"PK\x01\x02")) {
            return Err(invalid("a broken central directory"));
        }
        let name_len = u16_at(data, at + 28)?;
        let extra = u16_at(data, at + 30)?;
        let comment = u16_at(data, at + 32)?;
        let name = data
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| invalid("cut short"))?;
        out.push(Entry {
            name: String::from_utf8_lossy(name).into_owned(),
            method: u16_at(data, at + 10)?,
            compressed: u32_at(data, at + 20)?,
            size: u32_at(data, at + 24)?,
            offset: u32_at(data, at + 42)?,
        });
        at += 46 + name_len + extra + comment;
    }
    Ok(out)
}

/// The bytes of `entry`, `budget` at most (what is left of the limit).
fn contents(data: &[u8], entry: &Entry, budget: usize) -> Result<Vec<u8>, Error> {
    if entry.size > budget {
        return Err(Error::Limit {
            what: "bytes unpacked",
            limit: budget as f64,
        });
    }
    let at = entry.offset;
    if !data.get(at..).is_some_and(|d| d.starts_with(b"PK\x03\x04")) {
        return Err(invalid("a broken entry"));
    }
    let start = at + 30 + u16_at(data, at + 26)? + u16_at(data, at + 28)?;
    let raw = data
        .get(start..start + entry.compressed)
        .ok_or_else(|| invalid("cut short"))?;
    match entry.method {
        0 => Ok(raw.to_vec()),
        8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, budget).map_err(|_| {
            invalid(&format!(
                "`{}` does not inflate within the limit",
                entry.name
            ))
        }),
        m => Err(Error::Unsupported(format!(
            "dotLottie entries compressed with method {m}"
        ))),
    }
}

/// The first animation of the archive `data` as Lottie JSON, its images
/// embedded, with what was left out.
pub(crate) fn unpack(data: &[u8], limits: &Limits) -> Result<(Value, Vec<String>), Error> {
    let list = entries(data)?;
    let mut budget = limits.max_bytes;
    let mut read = |entry: &Entry| -> Result<Vec<u8>, Error> {
        let bytes = contents(data, entry, budget)?;
        budget = budget.saturating_sub(bytes.len());
        Ok(bytes)
    };
    let find = |name: &str| list.iter().find(|e| e.name.trim_start_matches('/') == name);
    let mut warnings = Vec::new();
    let manifest: Option<Value> = match find("manifest.json") {
        Some(e) => serde_json::from_slice(&read(e)?).ok(),
        None => None,
    };
    let ids: Vec<String> = manifest
        .as_ref()
        .and_then(|m| m.get("animations"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    if ids.len() > 1 {
        warnings.push(format!(
            "the archive holds {} animations: the first is read",
            ids.len()
        ));
    }
    // dotLottie 1 keeps animations under `animations/`, 2 under `a/`.
    let animation = ids
        .first()
        .and_then(|id| {
            find(&format!("a/{id}.json")).or_else(|| find(&format!("animations/{id}.json")))
        })
        .or_else(|| {
            list.iter().find(|e| {
                (e.name.starts_with("animations/") || e.name.starts_with("a/"))
                    && e.name.ends_with(".json")
            })
        })
        .ok_or_else(|| invalid("no animation"))?;
    let machines = manifest
        .as_ref()
        .and_then(|m| m.get("stateMachines"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if machines > 0 {
        warnings.push(format!(
            "the archive holds {machines} state machines: they are left out"
        ));
    }
    let mut root: Value =
        serde_json::from_slice(&read(animation)?).map_err(|e| Error::Invalid(e.to_string()))?;
    // Images the animation refers to by path, embedded.
    if let Some(assets) = root.get_mut("assets").and_then(Value::as_array_mut) {
        for asset in assets {
            let (Some(p), u) = (
                asset.get("p").and_then(Value::as_str).map(str::to_owned),
                asset
                    .get("u")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            ) else {
                continue;
            };
            if asset.get("e").and_then(Value::as_i64) == Some(1) || p.starts_with("data:") {
                continue;
            }
            let joined = format!("{u}{p}");
            let candidates = [
                joined.trim_start_matches('/').to_owned(),
                format!("images/{p}"),
                format!("i/{p}"),
            ];
            let Some(entry) = candidates.iter().find_map(|c| find(c)) else {
                warnings.push(format!("the image `{p}` is not in the archive"));
                continue;
            };
            let bytes = read(entry)?;
            let mime = if p.to_ascii_lowercase().ends_with(".png") {
                "image/png"
            } else {
                "image/jpeg"
            };
            let uri = format!(
                "data:{mime};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            );
            asset["u"] = Value::String(String::new());
            asset["p"] = Value::String(uri);
            asset["e"] = Value::from(1);
        }
    }
    Ok((root, warnings))
}
