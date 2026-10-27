//! Reads Lottie animations into a [`dxn_vector_animation`] scene.
//!
//! Shape layers (groups, rectangles, ellipses, polystars and paths, fills,
//! strokes with dashes, linear and radial gradients, trim paths), null
//! layers as parents, nested compositions with time stretch, offset and
//! remapping (clipped to their size), solid layers, embedded images,
//! layer masks (add, subtract, intersect, inverted) and track mattes
//! (alpha, luminance, inverted), blend modes, keyframes with their bezier
//! easings and holds, position keyframes with spatial tangents as motion
//! paths (auto-orient included), repeaters, the layer effects Drop
//! Shadow, Gaussian Blur, Tint, Fill, Tritone and Levels, and the layer
//! styles drop shadow, inner shadow and color overlay. Text without glyphs
//! is set in its font when drawn.
//!
//! Text layers drawn with the file's glyphs (`chars`), laid out, animated
//! and laid along paths as lottie-web does it: text animators with range
//! selectors (in percent or by index, over characters, words or lines,
//! each shape, eased, smoothed, randomized deterministically) or After
//! Effects' default expression selector, changing the anchor, position,
//! scale, skew, rotation, opacity, colors, stroke width and tracking of
//! each character; text on a mask's path. An animated character is a
//! group of its own, constant where what selects it is and sampled at the
//! file's frame rate where that moves: the keyframes of all text together
//! within half of [`Limits::max_keyframes`].
//!
//! Shape modifiers change the outlines listed before them in their group
//! (nested groups' included) for the fills and strokes after them: merge
//! paths (merge, add, subtract, intersect, exclude intersections, as
//! skottie merges, each outline's inside by the non-zero rule; the fills
//! and strokes before a merge draw nothing, as in After Effects), round
//! corners, pucker and bloat, zig zag and offset path (as lottie-web
//! changes outlines), and twist (points turned about its center, the
//! more the nearer). What they make is outline geometry: a constant path,
//! or, while anything it depends on moves, a path keyframe each frame,
//! within half the limits of path points and keyframes.
//!
//! Expressions are evaluated and baked into keyframes before the
//! conversion: each property's expression runs at every frame its layer
//! shows, and its results become keyframes, straight between frames. The
//! interpreter reads Bodymovin's rewritten form (`$bm_rt`, `$bm_sum`,
//! `$bm_sub`, `$bm_mul`, `$bm_div`, `$bm_mod`, `$bm_neg`) and the
//! JavaScript expressions are written in (variables, functions, `if`,
//! `switch`, loops, `try`, arrays, strings, `Math`, vector math on arrays),
//! with After Effects' expression API: `time`, `value`, `valueAtTime`,
//! `velocity`, `speed`, keyframes (`key`, `numKeys`, `nearestKey`),
//! `loopIn` / `loopOut` and their `Duration` forms (cycle, pingpong,
//! offset, continue), `wiggle`, `temporalWiggle`, `smooth`, `seedRandom`,
//! `random`, `gaussRandom`, `noise`, `linear` / `ease` / `easeIn` /
//! `easeOut`, `posterizeTime`, time, angle and color conversions,
//! `thisComp`, `comp(name)`, `thisLayer` and other layers (their
//! transforms, time remapping, timing and parents), and expression
//! controls (`effect(name)(property)`); a property read from another is
//! sampled after its own expression. Wiggles and random numbers are seeded
//! by their property, so every import gives the same scene. Evaluation is
//! bounded by a step budget, recursion caps and the keyframe limit. The
//! expression selectors of text animators run per character: the text
//! reads them (above).
//!
//! What a file holds beyond that (expressions using anything else or
//! failing, which keep their keyframes; wiggly and other expression text
//! selectors, animators of text without glyphs, other effects and layer
//! styles) is left out with a warning in [`Import::warnings`]. dotLottie
//! archives (ZIP) are read too: their first animation, its images
//! embedded.

mod convert;
mod dotlottie;
mod expressions;
mod modifiers;
mod props;

use dxn_vector_animation::{Error, Import, Limits};

/// Whether `data` looks like a Lottie JSON document or a dotLottie archive.
pub fn sniff(data: &[u8]) -> bool {
    if dotlottie::sniff(data) {
        return true;
    }
    let head = &data[..data.len().min(64 * 1024)];
    let Some(first) = head.iter().find(|b| !b.is_ascii_whitespace()) else {
        return false;
    };
    if *first != b'{' {
        return false;
    }
    // Exporters write the header (`v`, `fr`, `ip`, `op`) first or last.
    let tail = &data[data.len().saturating_sub(64 * 1024)..];
    let (head, tail) = (String::from_utf8_lossy(head), String::from_utf8_lossy(tail));
    let has = |key: &str| head.contains(key) || tail.contains(key);
    has("\"layers\"") && (has("\"fr\"") || has("\"op\""))
}

/// Reads the Lottie JSON document or dotLottie archive `data` with the
/// default [`Limits`].
pub fn read(data: &[u8]) -> Result<Import, Error> {
    read_with(data, &Limits::default())
}

/// Reads the Lottie JSON document or dotLottie archive `data`, refusing it
/// when it is over `limits` (an archive's unpacked bytes too).
pub fn read_with(data: &[u8], limits: &Limits) -> Result<Import, Error> {
    if data.len() > limits.max_bytes {
        return Err(Error::Limit {
            what: "bytes",
            limit: limits.max_bytes as f64,
        });
    }
    let (root, left_out) = if dotlottie::sniff(data) {
        dotlottie::unpack(data, limits)?
    } else {
        (
            serde_json::from_slice(data).map_err(|e| Error::Invalid(e.to_string()))?,
            Vec::new(),
        )
    };
    convert_root(root, left_out, limits)
}

/// Reads the Lottie document `root`, a JSON value (an app's import of a
/// `.json` file), within `limits`: as [`read_with`] reads its text.
pub fn read_value(root: serde_json::Value, limits: &Limits) -> Result<Import, Error> {
    convert_root(root, Vec::new(), limits)
}

fn convert_root(
    mut root: serde_json::Value,
    mut left_out: Vec<String>,
    limits: &Limits,
) -> Result<Import, Error> {
    left_out.extend(expressions::bake(&mut root, limits));
    let (scene, warnings) = convert::Converter::run(&root, *limits)?;
    scene.check(limits)?;
    left_out.extend(warnings);
    Ok(Import {
        scene,
        warnings: left_out,
    })
}
