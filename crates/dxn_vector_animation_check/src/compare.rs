//! Screenshots of the check app against the reference renderer.
//!
//! A tile is a square `tile` points wide with a one point border of
//! `#d1d5db`; the screenshot's scale is the tile's width in pixels over
//! `tile`. Inside the border the player fits its scene (contain); the
//! reference renders the same frame at that size, centered on white.

use std::path::{Path, PathBuf};

use dxn_vector_animation::compiled;
use dxn_vector_animation_render::{RenderOptions, render};
use tiny_skia as sk;

use crate::{PAGE, option};

const BORDER: [u8; 3] = [209, 213, 219];

/// A channel this far apart makes a pixel differ.
const APART: u8 = 64;

struct Entry {
    name: String,
    duration: f64,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let shots = PathBuf::from(args.first().ok_or("which screenshots?")?);
    let app = PathBuf::from(args.get(1).ok_or("which app?")?);
    let tile: f64 = option(args, "--tile")
        .map(str::parse)
        .transpose()
        .map_err(|e| format!("--tile: {e}"))?
        .unwrap_or(96.0);
    let out = option(args, "--out").map_or_else(|| shots.join("compared"), PathBuf::from);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let index: serde_json::Value = serde_json::from_slice(
        &std::fs::read(app.join("animations/index.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let entries: Vec<Entry> = index
        .as_array()
        .ok_or("index.json is not a list")?
        .iter()
        .map(|e| Entry {
            name: e["name"].as_str().unwrap_or_default().to_owned(),
            duration: e["duration"].as_f64().unwrap_or(0.0),
        })
        .collect();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&shots)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("frame-") && n.ends_with(".png"))
        })
        .collect();
    files.sort();
    let mut worst: Vec<(f64, String)> = Vec::new();
    for file in files {
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let mut parts = stem.trim_start_matches("frame-").split('-');
        let (Some(page), Some(percent)) = (parts.next(), parts.next()) else {
            continue;
        };
        let page: usize = page.parse().map_err(|_| format!("{stem}: page"))?;
        let percent: f64 = percent.parse().map_err(|_| format!("{stem}: percent"))?;
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        let mut shot =
            sk::Pixmap::decode_png(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        if display_p3(&bytes) {
            to_srgb(&mut shot);
        }
        let tiles = find_tiles(&shot, tile)?;
        println!(
            "{stem}: {} tiles at {:.3} pixels a point",
            tiles.tiles.len(),
            tiles.scale
        );
        for (i, &(x, y)) in tiles.tiles.iter().enumerate() {
            let Some(entry) = entries.get(page * PAGE + i) else {
                break;
            };
            let data = std::fs::read(app.join(format!("animations/{}.json", entry.name)))
                .map_err(|e| e.to_string())?;
            let scene =
                compiled::from_json(&data, &Default::default()).map_err(|e| e.to_string())?;
            let size = tiles.content;
            let reference = reference(&scene, entry.duration * percent / 100.0, size)?;
            let inset = tiles.border;
            let (straight, best, shift) = differ(&shot, x + inset, y + inset, &reference);
            let share = |n: usize| 100.0 * n as f64 / (size * size) as f64;
            println!(
                "  {:<12} {:>6.2}%  ({:.2}% shifted {:+},{:+})",
                entry.name,
                share(straight),
                share(best),
                shift.0,
                shift.1
            );
            worst.push((share(best), format!("{} at {percent}%", entry.name)));
            side_by_side(
                &shot,
                x + inset,
                y + inset,
                &reference,
                &out.join(format!("{}-{percent}.png", entry.name)),
            )?;
        }
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    if let Some((share, what)) = worst.first() {
        println!("worst (shifted): {share:.2}% ({what})");
    }
    Ok(())
}

/// Whether a PNG is in Display P3 (the iOS simulator's screenshots): its
/// `cICP` names P3's primaries, or its ICC profile is named for P3.
fn display_p3(png: &[u8]) -> bool {
    let mut at = 8;
    while at + 8 <= png.len() {
        let len = u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]) as usize;
        let kind = &png[at + 4..at + 8];
        let body = &png[(at + 8).min(png.len())..(at + 8 + len).min(png.len())];
        match kind {
            b"cICP" if body.first() == Some(&12) => return true,
            b"iCCP" if body.windows(2).any(|w| w == b"P3") => return true,
            b"IDAT" => return false,
            _ => {}
        }
        at += 12 + len;
    }
    false
}

/// Display P3 pixels as sRGB (both with sRGB's transfer curve).
fn to_srgb(shot: &mut sk::Pixmap) {
    const M: [[f64; 3]; 3] = [
        [1.224_940_1, -0.224_940_4, 0.0],
        [-0.042_056_9, 1.042_057_1, 0.0],
        [-0.019_637_6, -0.078_636_1, 1.098_273_5],
    ];
    let decode = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let encode = |c: f64| {
        let c = c.clamp(0.0, 1.0);
        let c = if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (c * 255.0).round() as u8
    };
    for p in shot.pixels_mut() {
        let c = p.demultiply();
        let rgb = [decode(c.red()), decode(c.green()), decode(c.blue())];
        let out: Vec<u8> = M
            .iter()
            .map(|row| encode(row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2]))
            .collect();
        if let Some(q) = sk::ColorU8::from_rgba(out[0], out[1], out[2], c.alpha())
            .premultiply()
            .into()
        {
            *p = q;
        }
    }
}

struct Tiles {
    /// Top-left corners, row by row.
    tiles: Vec<(usize, usize)>,
    scale: f64,
    /// The border's width and the size inside it, in pixels.
    border: usize,
    content: usize,
}

fn near_border(p: sk::PremultipliedColorU8) -> bool {
    let c = p.demultiply();
    [c.red(), c.green(), c.blue()]
        .iter()
        .zip(BORDER)
        .all(|(a, b)| a.abs_diff(b) <= 10)
}

/// The tiles of `shot`: squares framed by the border color.
fn find_tiles(shot: &sk::Pixmap, tile: f64) -> Result<Tiles, String> {
    let (w, h) = (shot.width() as usize, shot.height() as usize);
    let px = shot.pixels();
    // Runs of the border color along each row: (y, x, length).
    let mut runs = Vec::new();
    for y in 0..h {
        let mut x = 0;
        while x < w {
            if near_border(px[y * w + x]) {
                let start = x;
                while x < w && near_border(px[y * w + x]) {
                    x += 1;
                }
                if x - start > 40 {
                    runs.push((y, start, x - start));
                }
            } else {
                x += 1;
            }
        }
    }
    // Squares framed by runs `edge` long: a top run, and one as long below
    // it, `edge` pixels down.
    let close = |a: usize, b: usize, by: usize| a.abs_diff(b) <= by;
    let squares = |edge: usize| {
        let mut tops: Vec<(usize, usize)> = Vec::new();
        for &(y, x, len) in &runs {
            if !close(len, edge, 2) {
                continue;
            }
            let bottom = runs.iter().any(|&(y2, x2, l2)| {
                close(l2, edge, 2) && close(x2, x, 3) && close(y2, y + edge - 1, 3)
            });
            let near = |&(ty, tx): &(usize, usize)| close(ty, y, edge / 2) && close(tx, x, 3);
            if bottom && !tops.iter().any(near) {
                tops.push((y, x));
            }
        }
        tops
    };
    // The tile's edge is the run length framing the most squares, the
    // commonest of those.
    let mut counts = std::collections::HashMap::new();
    for &(_, _, len) in &runs {
        *counts.entry(len).or_insert(0usize) += 1;
    }
    let (edge, mut tops) = counts
        .iter()
        .map(|(&len, &n)| (len, n, squares(len)))
        .filter(|(_, _, tops)| !tops.is_empty())
        .max_by_key(|(len, n, tops)| (tops.len(), *n, *len))
        .map(|(len, _, tops)| (len, tops))
        .ok_or("no tiles found")?;
    let scale = edge as f64 / tile;
    let row = (10.0 * scale).max(1.0) as usize;
    tops.sort_by_key(|&(y, x)| (y / row, x));
    let border = (scale.round() as usize).max(1);
    Ok(Tiles {
        tiles: tops.into_iter().map(|(y, x)| (x, y)).collect(),
        scale,
        border,
        content: edge - 2 * border,
    })
}

/// The reference frame at `time`, fitted (contain) into `size` × `size`,
/// centered on white.
fn reference(
    scene: &dxn_vector_animation::Scene,
    time: f64,
    size: usize,
) -> Result<sk::Pixmap, String> {
    let fit = (size as f64 / scene.width).min(size as f64 / scene.height);
    let frame = scene.frame_at(time);
    let options = RenderOptions {
        scale: fit,
        background: Some(dxn_vector_animation::Color::rgba(1.0, 1.0, 1.0, 1.0)),
        ..Default::default()
    };
    let drawn = render(&frame, &options).map_err(|e| e.to_string())?;
    let mut out = sk::Pixmap::new(size as u32, size as u32).ok_or("no pixmap")?;
    out.fill(sk::Color::WHITE);
    let dx = (size as i32 - drawn.width() as i32) / 2;
    let dy = (size as i32 - drawn.height() as i32) / 2;
    out.draw_pixmap(
        dx,
        dy,
        drawn.as_ref(),
        &sk::PixmapPaint::default(),
        sk::Transform::identity(),
        None,
    );
    Ok(out)
}

/// Pixels of the tile at (`x`, `y`) in `shot` that differ from
/// `reference`: straight, and at the best shift of a pixel (with it).
fn differ(
    shot: &sk::Pixmap,
    x: usize,
    y: usize,
    reference: &sk::Pixmap,
) -> (usize, usize, (i32, i32)) {
    let size = reference.width() as usize;
    let w = shot.width() as usize;
    let count = |sx: i32, sy: i32| {
        let mut bad = 0;
        for ry in 1..size - 1 {
            for rx in 1..size - 1 {
                let tx = (x + rx) as i32 + sx;
                let ty = (y + ry) as i32 + sy;
                let a = shot.pixels()[ty as usize * w + tx as usize].demultiply();
                let b = reference.pixels()[ry * size + rx].demultiply();
                let apart = a.red().abs_diff(b.red()) > APART
                    || a.green().abs_diff(b.green()) > APART
                    || a.blue().abs_diff(b.blue()) > APART;
                bad += usize::from(apart);
            }
        }
        bad
    };
    let straight = count(0, 0);
    let mut best = (straight, (0, 0));
    for sy in -1..=1 {
        for sx in -1..=1 {
            let n = count(sx, sy);
            if n < best.0 {
                best = (n, (sx, sy));
            }
        }
    }
    (straight, best.0, best.1)
}

fn side_by_side(
    shot: &sk::Pixmap,
    x: usize,
    y: usize,
    reference: &sk::Pixmap,
    path: &Path,
) -> Result<(), String> {
    let size = reference.width();
    let mut out = sk::Pixmap::new(2 * size + 4, size).ok_or("no pixmap")?;
    out.fill(sk::Color::from_rgba8(255, 0, 255, 255));
    let tile = shot
        .clone_rect(sk::IntRect::from_xywh(x as i32, y as i32, size, size).ok_or("tile")?)
        .ok_or("tile outside the screenshot")?;
    let paint = sk::PixmapPaint::default();
    out.draw_pixmap(0, 0, tile.as_ref(), &paint, sk::Transform::identity(), None);
    out.draw_pixmap(
        size as i32 + 4,
        0,
        reference.as_ref(),
        &paint,
        sk::Transform::identity(),
        None,
    );
    out.save_png(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// A white shot `scale` pixels a point with tiles of `tile` points
    /// framed at `corners` (points).
    fn shot(scale: usize, tile: usize, corners: &[(usize, usize)]) -> sk::Pixmap {
        let mut p = sk::Pixmap::new(400 * scale as u32, 300 * scale as u32).unwrap();
        p.fill(sk::Color::WHITE);
        let w = p.width() as usize;
        let border = sk::ColorU8::from_rgba(BORDER[0], BORDER[1], BORDER[2], 255).premultiply();
        let edge = tile * scale;
        for &(x, y) in corners {
            let (x, y) = (x * scale, y * scale);
            for i in 0..edge {
                for b in 0..scale {
                    for (px, py) in [
                        (x + i, y + b),
                        (x + i, y + edge - 1 - b),
                        (x + b, y + i),
                        (x + edge - 1 - b, y + i),
                    ] {
                        p.pixels_mut()[py * w + px] = border;
                    }
                }
            }
        }
        p
    }

    #[test]
    fn one_tile_is_found() {
        let t = find_tiles(&shot(1, 96, &[(12, 120)]), 96.0).unwrap();
        assert_eq!(t.tiles, vec![(12, 120)]);
        assert_eq!(t.scale, 1.0);
    }

    #[test]
    fn tiles_are_found_row_by_row_at_the_shots_scale() {
        let corners = [(12, 20), (116, 20), (220, 20), (12, 140), (116, 140)];
        let t = find_tiles(&shot(3, 96, &corners), 96.0).unwrap();
        let found: Vec<_> = t.tiles.iter().map(|&(x, y)| (x / 3, y / 3)).collect();
        assert_eq!(found, corners);
        assert_eq!(t.scale, 3.0);
        assert_eq!(t.border, 3);
    }

    #[test]
    fn a_shot_without_tiles_says_so() {
        assert!(find_tiles(&shot(2, 96, &[]), 96.0).is_err());
    }
}
