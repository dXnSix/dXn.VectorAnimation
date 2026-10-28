//! Soft masks (partly transparent, or by luminance). The masked content
//! draws on a canvas of its own, and then as much of it is taken away
//! (`destination-out`) as the mask does not show. `destination-out` leaves
//! what lies outside its drawing alone, so the platforms' canvases agree
//! on it.
//!
//! A mask whose content moves as a whole at most (in the masked group's
//! lengths) and is not a product (below) is an image of what it hides, as
//! the reference renders its content (any paints, strokes and shapes),
//! drawn under the group's and its root's transforms: what lies outside
//! the image is taken away as well where the mask shows nothing there. A
//! mask whose shapes change is the graph's, its shapes' colors and
//! opacities composited as the reference composites them (added,
//! intersected, subtracted), where the graph draws its shapes (solid
//! colors, four shapes at most); otherwise it is drawn as the reference
//! draws it (an image a frame).
//!
//! A mask of one shape, intersected with and subtracted from by others,
//! shows a product: a share everywhere, times a share on one side of each
//! shape's outline. It takes away over the whole plane, then on that side
//! of each outline: every edge is drawn once, exactly. Other masks (shapes
//! added over one another) are cut into regions by their shapes (every
//! combination of inside and outside them), each taking away what the mask
//! does not show there, after what none shows taken away everywhere: where
//! two regions meet, both edges are drawn, and a seam can show faintly.

use std::sync::Arc;

use dxn_vector_animation::{
    Animated, BlendMode, Color, FillRule, Group, ImageData, LineCap, LineJoin, Mask, MaskKind,
    Node, Paint as ScenePaint, PaintOrder, Path, PathEl, Rect, Scene, Shape, Stroke,
    outline::geometry_path,
};

use super::{Compiler, Op, Unsupported, huge_rect, static_path, static_paths, unsupported};
use crate::graph::{MathOp, NodeId, Num, Paint};

/// The most shapes a soft mask may have: its regions are every
/// combination of them.
const SHAPES: usize = 4;

/// A soft mask as drawn: for each region, its clip and how much of the
/// content it takes away; or an image of what it takes away.
pub(super) struct SoftMask {
    regions: Vec<(Vec<Op>, Num)>,
    image: Option<HiddenImage>,
    /// What it hides all of outside its outlines: clips of the content
    /// (with their outlines' own rules) before it draws.
    pub(super) clips: Vec<Op>,
}

/// An image of what a mask's content hides (its alpha) where it shows
/// all, over `rect` in its root's lengths (`setup` moving them as the root
/// moves, within the masked group's), and where everything is hidden past
/// it: outside `hole` (within the image's edge, which hides all there too);
/// the root's `opacity` showing that share of what it shows.
struct HiddenImage {
    image: Arc<ImageData>,
    rect: Rect,
    hole: Option<Rect>,
    setup: Vec<Op>,
    opacity: Num,
    /// What the opacity does not show.
    rest: Num,
    inverted: bool,
}

/// The most pixels a mask's image takes.
const MASK_IMAGE_PIXELS: f64 = 4096.0 * 4096.0;

impl SoftMask {
    /// A mask that shows nothing anywhere.
    fn nothing() -> SoftMask {
        SoftMask {
            regions: vec![(Vec::new(), Num::Const(1.0))],
            image: None,
            clips: Vec::new(),
        }
    }

    /// Whether it shows nothing anywhere, at any time.
    pub(super) fn hides_all(&self) -> bool {
        self.image.is_none()
            && self
                .regions
                .iter()
                .any(|(clip, away)| clip.is_empty() && away.is(1.0))
    }
}

/// A mask's content as it composites.
enum Tree {
    Group {
        blend: BlendMode,
        opacity: Num,
        children: Vec<Tree>,
        /// Its own mask (a chained matte): its kind, whether inverted, and
        /// what it composites.
        mask: Option<(MaskKind, bool, Box<Tree>)>,
        /// Its clip: the index of its outline among the mask's shapes.
        clip: Option<usize>,
    },
    /// A shape (its index among the mask's shapes): its luminance times its
    /// alpha, and its alpha, where it covers.
    Shape { index: usize, luma: Num, alpha: Num },
}

/// What a mask shows as a product: a share everywhere (its luminance
/// times alpha, and its alpha), times for each factor a share on one side
/// of a shape's outline (inside, or outside) and all on the other.
struct Product {
    luma: Num,
    alpha: Num,
    factors: Vec<(usize, bool, Num)>,
}

/// A shape of a mask: its outline, ready to clip with.
struct Outline {
    ops: Vec<Op>,
    rule: FillRule,
    /// Whether contours of it may overlap winding alike (filled once by its
    /// rule, twice by even-odd: what is outside it is not its even-odd
    /// complement).
    overlaps: bool,
}

/// Whether contours of `paths`, filled by `rule`, may overlap winding
/// alike: two whose boxes meet turning the same way, where the rule counts
/// windings.
fn contours_overlap(paths: &[Path], rule: FillRule) -> bool {
    if rule == FillRule::EvenOdd {
        return false;
    }
    // Each contour's box and the sign of its area.
    let mut contours: Vec<(Rect, bool)> = Vec::new();
    for path in paths {
        let mut points: Vec<dxn_vector_animation::Point> = Vec::new();
        let mut flush = |points: &mut Vec<dxn_vector_animation::Point>| {
            if points.len() > 2 {
                let (x0, y0, x1, y1) = points.iter().fold(
                    (
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ),
                    |(a, b, c, d), p| (a.min(p.x), b.min(p.y), c.max(p.x), d.max(p.y)),
                );
                let area: f64 = points
                    .iter()
                    .zip(points.iter().cycle().skip(1))
                    .map(|(p, q)| p.x * q.y - q.x * p.y)
                    .sum();
                contours.push((Rect::new(x0, y0, x1 - x0, y1 - y0), area > 0.0));
            }
            points.clear();
        };
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    flush(&mut points);
                    points.push(p);
                }
                PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CubicTo(_, _, p) => {
                    points.push(p)
                }
                PathEl::Close => {}
            }
        }
        flush(&mut points);
    }
    contours.iter().enumerate().any(|(i, (a, sa))| {
        contours[i + 1..]
            .iter()
            .any(|(b, sb)| sa == sb && a.intersect(*b).is_some())
    })
}

/// The paths of `s` as they start, through their transforms; `None` where
/// one moves.
fn still_paths(s: &Shape) -> Option<Vec<Path>> {
    s.paths
        .iter()
        .map(|p| {
            let animated = p.geometry.is_animated() || p.transform.iter().any(|o| o.is_animated());
            (!animated).then(|| {
                geometry_path(&p.geometry, 0.0).transformed(
                    dxn_vector_animation::transform::transform_at(&p.transform, 0.0),
                )
            })
        })
        .collect()
}

/// Whether `tree` holds a group masked by its own mask.
fn chained(tree: &Tree) -> bool {
    match tree {
        Tree::Group { mask: Some(_), .. } => true,
        Tree::Group { children, .. } => children.iter().any(chained),
        Tree::Shape { .. } => false,
    }
}

/// `g` drawn in its own lengths, as it shows all: its transform, opacity,
/// time and visibility left out.
fn rooted(g: &Group) -> Group {
    Group {
        transform: Vec::new(),
        opacity: Animated::Static(1.0),
        active: None,
        time: None,
        ..g.clone()
    }
}

/// The luminance of `c` as the reference measures it (Rec. 709 weights).
fn luminance(c: Color) -> f64 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

impl Compiler {
    /// `mask` as a soft mask, or `None` where its shapes clip as well (it
    /// shows all or nothing everywhere) or it holds what is not drawn so
    /// (gradients, strokes, more than [`SHAPES`] shapes, other blends).
    pub(super) fn soft_mask(
        &mut self,
        g: &Group,
        mask: &Mask,
        time: NodeId,
        depth: usize,
    ) -> Result<Option<SoftMask>, Unsupported> {
        // Its content moving as a whole at most: its root's transform and
        // opacity aside, still.
        let still = super::still(&Scene {
            root: rooted(&mask.content),
            ..self.stage.clone()
        });
        let mut outlines = Vec::new();
        let tree = self.mask_tree(&mask.content, time, &[], &mut outlines, depth)?;
        let regions = tree.filter(|_| !outlines.is_empty() && outlines.len() <= SHAPES);
        let Some(tree) = regions else {
            // Not drawn by regions (gradients, images, too many shapes): an
            // image where it does not move.
            if still {
                return self.hidden_image(g, mask, time);
            }
            // Changing shape: drawn as the reference draws it where it
            // shows part (a clip where all or nothing).
            if super::has_soft_masks(&Node::Group(Group {
                mask: Some(Box::new(mask.clone())),
                ..Group::new()
            })) {
                return unsupported("a soft mask changing shape");
            }
            return Ok(None);
        };
        // The content composites onto nothing, as a layer of its own.
        let tree = Tree::Group {
            blend: BlendMode::Normal,
            opacity: Num::Const(1.0),
            children: vec![tree],
            mask: None,
            clip: None,
        };
        if !mask.inverted
            && let Some(p) = self.product(&tree)
        {
            return Ok(self.product_mask(mask.kind, p, &outlines));
        }
        // Regions meet where shapes overlap, their edges drawn twice: an
        // image where the mask does not move.
        if still && let Some(image) = self.hidden_image(g, mask, time)? {
            return Ok(Some(image));
        }
        let mut away = Vec::with_capacity(1 << outlines.len());
        for inside in 0..1usize << outlines.len() {
            let (luma, alpha) = self.composite(&tree, inside);
            let shown = match mask.kind {
                MaskKind::Alpha => alpha,
                MaskKind::Luminance => luma,
            };
            let shown = if mask.inverted {
                self.g.math(MathOp::Sub, Num::Const(1.0), shown)
            } else {
                shown
            };
            away.push(self.g.math(MathOp::Sub, Num::Const(1.0), shown));
        }
        // All or nothing everywhere: its shapes clip, but for chained mattes,
        // which clips do not follow.
        // Showing nothing anywhere: no clip of its shapes says so.
        if away.iter().all(|a| a.is(1.0)) {
            return Ok(Some(SoftMask::nothing()));
        }
        if away.iter().all(|a| a.is(0.0) || a.is(1.0)) && !chained(&tree) {
            return Ok(None);
        }

        // What every region takes away, everywhere first; each region then
        // the rest of its share.
        let mut least = away[0];
        for &a in &away[1..] {
            // min(least, a) = least - max(least - a, 0)
            let d = self.g.math(MathOp::Sub, least, a);
            let over = self.g.above(d, 0.0);
            least = self.g.math(MathOp::Sub, least, over);
        }
        let kept = self.g.math(MathOp::Sub, Num::Const(1.0), least);
        let mut regions = vec![(Vec::new(), least)];
        let rest: Vec<Num> = away
            .iter()
            .map(|&a| {
                let left = self.g.math(MathOp::Sub, Num::Const(1.0), a);
                let share = self.g.math(MathOp::Div, left, kept);
                self.g.math(MathOp::Sub, Num::Const(1.0), share)
            })
            .collect();
        let far = self.far();
        regions.extend(
            rest.into_iter()
                .enumerate()
                .filter(|(_, a)| !a.is(0.0))
                .map(|(inside, a)| {
                    let mut ops = Vec::new();
                    for (i, outline) in outlines.iter().enumerate() {
                        let within = inside >> i & 1 == 1;
                        ops.push(Op::BeginPath);
                        if !within {
                            ops.extend(huge_rect(far));
                        }
                        ops.extend(outline.ops.iter().cloned());
                        ops.push(Op::Clip(if within {
                            outline.rule
                        } else {
                            FillRule::EvenOdd
                        }));
                    }
                    (ops, a)
                }),
        );
        regions.retain(|(_, a)| !a.is(0.0));
        Ok(Some(SoftMask {
            regions,
            image: None,
            clips: Vec::new(),
        }))
    }

    /// The soft mask of product `p`: what it does not show of its share
    /// taken away everywhere, then on one side of each outline; `None`
    /// where it shows all or nothing everywhere (its shapes clip).
    fn product_mask(
        &mut self,
        kind: MaskKind,
        p: Product,
        outlines: &[Outline],
    ) -> Option<SoftMask> {
        let share = match kind {
            MaskKind::Alpha => p.alpha,
            MaskKind::Luminance => p.luma,
        };
        let far = self.far();
        let mut regions = vec![(Vec::new(), self.g.math(MathOp::Sub, Num::Const(1.0), share))];
        let mut clips = Vec::new();
        for (index, inside, f) in p.factors {
            let outline = &outlines[index];
            // Nothing outside an outline whose contours may overlap: a clip,
            // as the outline fills (the content drawn through it, each
            // drawing's edge on its own).
            if !inside && f.is(0.0) && outline.overlaps {
                clips.push(Op::BeginPath);
                clips.extend(outline.ops.iter().cloned());
                clips.push(Op::Clip(outline.rule));
                continue;
            }
            let mut ops = vec![Op::BeginPath];
            if !inside {
                ops.extend(huge_rect(far));
            }
            ops.extend(outline.ops.iter().cloned());
            ops.push(Op::Clip(if inside {
                outline.rule
            } else {
                FillRule::EvenOdd
            }));
            regions.push((ops, self.g.math(MathOp::Sub, Num::Const(1.0), f)));
        }
        regions.retain(|(_, a)| !a.is(0.0));
        let mask = SoftMask {
            regions,
            image: None,
            clips,
        };
        if mask.hides_all() {
            return Some(SoftMask::nothing());
        }
        if mask.regions.iter().all(|(_, a)| a.is(1.0)) {
            return None;
        }
        Some(mask)
    }

    /// `tree` as a product, where it composites to one: its first shape,
    /// then shapes intersected with it, and single shapes subtracted.
    fn product(&mut self, tree: &Tree) -> Option<Product> {
        match tree {
            Tree::Shape { index, luma, alpha } => Some(Product {
                luma: *luma,
                alpha: *alpha,
                factors: vec![(*index, false, Num::Const(0.0))],
            }),
            Tree::Group { mask: Some(_), .. } => None,
            Tree::Group {
                opacity,
                children,
                clip,
                ..
            } => {
                // None while nothing shows yet.
                let mut acc: Option<Product> = None;
                for child in children {
                    let blend = match child {
                        Tree::Group { blend, .. } => *blend,
                        Tree::Shape { .. } => BlendMode::Normal,
                    };
                    acc = match (acc, blend) {
                        (None, BlendMode::Normal) => Some(self.product(child)?),
                        (None, _) => None,
                        (Some(mut p), BlendMode::DestinationIn) => {
                            let c = self.product(child)?;
                            p.luma = self.mul(p.luma, c.alpha);
                            p.alpha = self.mul(p.alpha, c.alpha);
                            p.factors.extend(c.factors);
                            Some(p)
                        }
                        (Some(mut p), BlendMode::DestinationOut) => {
                            let c = self.product(child)?;
                            let [(index, false, zero)] = c.factors.as_slice() else {
                                return None;
                            };
                            if !zero.is(0.0) {
                                return None;
                            }
                            let left = self.g.math(MathOp::Sub, Num::Const(1.0), c.alpha);
                            p.factors.push((*index, true, left));
                            Some(p)
                        }
                        (Some(_), _) => return None,
                    };
                }
                let mut p = acc.unwrap_or(Product {
                    luma: Num::Const(0.0),
                    alpha: Num::Const(0.0),
                    factors: Vec::new(),
                });
                p.luma = self.mul(p.luma, *opacity);
                p.alpha = self.mul(p.alpha, *opacity);
                // Nothing outside its clip.
                if let Some(i) = clip {
                    p.factors.push((*i, false, Num::Const(0.0)));
                }
                Some(p)
            }
        }
    }

    /// `mask` (of `g`, whose content moves as a whole at most) as an image
    /// of what its content hides, at the display's pixels as large as `g`
    /// and the content's root get.
    fn hidden_image(
        &mut self,
        g: &Group,
        mask: &Mask,
        time: NodeId,
    ) -> Result<Option<SoftMask>, Unsupported> {
        let root = &mask.content;
        let content = Scene {
            root: rooted(root),
            ..self.stage.clone()
        };
        let frame = content.frame_at(0.0);
        let local = match &root.time {
            Some(map) => self.local_time(map, time),
            None => time,
        };
        let visible = match root.active {
            Some(range) => self.visibility(range, local),
            None => Num::Const(1.0),
        };
        let opacity = self.g.track(&root.opacity, local, |o| o.clamp(0.0, 1.0));
        let opacity = self.mul(opacity, visible);
        let Some(bounds) = frame.bounds().filter(|_| !opacity.is(0.0)) else {
            // Nothing drawn: all hidden, or (inverted) nothing.
            return Ok(Some(if mask.inverted {
                SoftMask {
                    regions: Vec::new(),
                    image: None,
                    clips: Vec::new(),
                }
            } else {
                SoftMask::nothing()
            }));
        };
        let setup = self.transform(&root.transform, local)?;
        let moves = super::probe_scale(&root.transform, self.stage.timeline.duration);
        let mut k = self.pixels * self.largest_scale(g).max(1e-6) * moves;
        let area = bounds.width * bounds.height * k * k;
        if area > MASK_IMAGE_PIXELS {
            k *= (MASK_IMAGE_PIXELS / area).sqrt();
        }
        // Whole pixels, with room for what hides all at the edge.
        let pad = 4.0 / k;
        let px = |v: f64| (v * k).floor() / k;
        let (x0, y0) = (px(bounds.x - pad), px(bounds.y - pad));
        let x1 = ((bounds.x + bounds.width + pad) * k).ceil() / k;
        let y1 = ((bounds.y + bounds.height + pad) * k).ceil() / k;
        let rect = Rect::new(x0, y0, x1 - x0, y1 - y0);
        let Ok(pixmap) = dxn_vector_animation_render::render_region(&frame, rect, k) else {
            return Ok(None);
        };
        let shown = tiny_skia::Mask::from_pixmap(
            pixmap.as_ref(),
            match mask.kind {
                MaskKind::Alpha => tiny_skia::MaskType::Alpha,
                MaskKind::Luminance => tiny_skia::MaskType::Luminance,
            },
        );
        let Some(mut hidden) = tiny_skia::Pixmap::new(pixmap.width(), pixmap.height()) else {
            return Ok(None);
        };
        for (px, &m) in hidden.pixels_mut().iter_mut().zip(shown.data()) {
            let a = if mask.inverted { m } else { 255 - m };
            *px = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, a)
                .unwrap_or(tiny_skia::PremultipliedColorU8::TRANSPARENT);
        }
        let Some(bytes) = super::png(&hidden) else {
            return Ok(None);
        };
        let half = 2.0 / k;
        Ok(Some(SoftMask {
            regions: Vec::new(),
            clips: Vec::new(),
            image: Some(HiddenImage {
                image: Arc::new(ImageData {
                    mime: "image/png".into(),
                    bytes,
                }),
                rect,
                hole: (!mask.inverted).then(|| {
                    Rect::new(
                        rect.x + half,
                        rect.y + half,
                        rect.width - 2.0 * half,
                        rect.height - 2.0 * half,
                    )
                }),
                setup,
                opacity,
                rest: self.g.math(MathOp::Sub, Num::Const(1.0), opacity),
                inverted: mask.inverted,
            }),
        }))
    }

    /// What applies `mask` to the content drawn before it on the canvas.
    pub(super) fn soft_mask_ops(&self, mask: &SoftMask) -> Vec<Op> {
        let far = self.far();
        if let Some(hidden) = &mask.image {
            let mut ops = vec![
                Op::Save,
                Op::Blend(BlendMode::DestinationOut),
                Op::FillColor(Paint::Const(0x0000_00ff)),
            ];
            // Showing a share of what it shows: the rest taken away first
            // everywhere (or, inverted, a share of what it hides).
            let alpha = if hidden.inverted {
                hidden.opacity
            } else {
                if !hidden.opacity.is(1.0) {
                    ops.extend([Op::GlobalAlpha(hidden.rest), Op::BeginPath]);
                    ops.extend(huge_rect(far));
                    ops.push(Op::Fill(FillRule::NonZero));
                }
                Num::Const(1.0)
            };
            ops.extend(hidden.setup.iter().cloned());
            ops.extend([
                Op::GlobalAlpha(alpha),
                Op::Image(hidden.image.clone(), hidden.rect),
            ]);
            if let Some(hole) = hidden.hole {
                let c = Num::Const;
                ops.push(Op::BeginPath);
                ops.extend(huge_rect(far));
                ops.extend([
                    Op::MoveTo(c(hole.x), c(hole.y)),
                    Op::LineTo(c(hole.x + hole.width), c(hole.y)),
                    Op::LineTo(c(hole.x + hole.width), c(hole.y + hole.height)),
                    Op::LineTo(c(hole.x), c(hole.y + hole.height)),
                    Op::Close,
                    Op::Fill(FillRule::EvenOdd),
                ]);
            }
            ops.push(Op::Restore);
            return ops;
        }
        let mut ops = vec![
            Op::Save,
            Op::Blend(BlendMode::DestinationOut),
            Op::FillColor(Paint::Const(0x0000_00ff)),
        ];
        for (clip, away) in &mask.regions {
            ops.push(Op::Save);
            ops.extend(clip.iter().cloned());
            ops.push(Op::BeginPath);
            ops.extend(huge_rect(far));
            ops.push(Op::GlobalAlpha(*away));
            ops.push(Op::Fill(FillRule::NonZero));
            ops.push(Op::Restore);
        }
        ops.push(Op::Restore);
        ops
    }

    /// `g` of a mask's content as it composites, its shapes' outlines
    /// (through `outer`, the transforms of the groups it is in) added to
    /// `outlines`; `None` for content not drawn as a soft mask.
    fn mask_tree(
        &mut self,
        g: &Group,
        time: NodeId,
        outer: &[Op],
        outlines: &mut Vec<Outline>,
        depth: usize,
    ) -> Result<Option<Tree>, Unsupported> {
        if depth > 128 {
            return unsupported("masks nested too deep");
        }
        if !matches!(
            g.blend,
            BlendMode::Normal | BlendMode::DestinationIn | BlendMode::DestinationOut
        ) {
            return Ok(None);
        }
        let visible = match g.active {
            Some(range) => self.visibility(range, time),
            None => Num::Const(1.0),
        };
        let local = match &g.time {
            Some(map) => self.local_time(map, time),
            None => time,
        };
        let opacity = self.g.track(&g.opacity, local, |o| o.clamp(0.0, 1.0));
        let opacity = self.mul(opacity, visible);
        let mut transforms = outer.to_vec();
        transforms.extend(self.transform(&g.transform, local)?);
        // Its clip: one more shape, all or nothing.
        let clip = match &g.clip {
            Some(clip) => {
                let mut ops = vec![Op::Save];
                ops.extend(transforms.iter().cloned());
                for shape in &clip.shapes {
                    ops.push(Op::Save);
                    ops.extend(self.transform(&shape.transform, local)?);
                    ops.extend(self.geometry(&shape.geometry, local)?);
                    ops.push(Op::Restore);
                }
                ops.push(Op::Restore);
                let rule = clip.shapes.first().map_or(FillRule::NonZero, |s| s.rule);
                outlines.push(Outline {
                    ops,
                    rule,
                    overlaps: clip.shapes.len() > 1 && rule == FillRule::NonZero,
                });
                Some(outlines.len() - 1)
            }
            None => None,
        };
        let mask = match &g.mask {
            Some(m) => match self.mask_tree(&m.content, local, &transforms, outlines, depth + 1)? {
                Some(tree) => Some((m.kind, m.inverted, Box::new(tree))),
                None => return Ok(None),
            },
            None => None,
        };
        let mut children = Vec::new();
        for child in &g.children {
            match child {
                Node::Group(inner) => {
                    match self.mask_tree(inner, local, &transforms, outlines, depth + 1)? {
                        Some(tree) => children.push(tree),
                        None => return Ok(None),
                    }
                }
                Node::Shape(s) => {
                    if s.trim.is_some() {
                        return Ok(None);
                    }
                    let fill = match &s.fill {
                        Some(fill) => {
                            let ScenePaint::Solid { color } = &fill.paint else {
                                return Ok(None);
                            };
                            let mut ops = vec![Op::Save];
                            ops.extend(transforms.iter().cloned());
                            for p in &s.paths {
                                ops.extend(self.shape_path(p, local)?);
                            }
                            ops.push(Op::Restore);
                            let overlaps = match still_paths(s) {
                                Some(paths) => contours_overlap(&paths, fill.rule),
                                None => fill.rule == FillRule::NonZero,
                            };
                            Some((ops, fill.rule, color, &fill.opacity, overlaps))
                        }
                        None => None,
                    };
                    let stroke = match &s.stroke {
                        Some(stroke) => {
                            let ScenePaint::Solid { color } = &stroke.paint else {
                                return Ok(None);
                            };
                            let Some(outline) = self.stroked_outline(s, stroke) else {
                                return Ok(None);
                            };
                            let mut ops = vec![Op::Save];
                            ops.extend(transforms.iter().cloned());
                            ops.extend(static_path(&outline));
                            ops.push(Op::Restore);
                            let overlaps =
                                contours_overlap(std::slice::from_ref(&outline), FillRule::NonZero);
                            Some((ops, FillRule::NonZero, color, &stroke.opacity, overlaps))
                        }
                        None => None,
                    };
                    let parts = match s.paint_order {
                        PaintOrder::FillFirst => [fill, stroke],
                        PaintOrder::StrokeFirst => [stroke, fill],
                    };
                    for (ops, rule, color, opacity, overlaps) in parts.into_iter().flatten() {
                        let index = outlines.len();
                        outlines.push(Outline {
                            ops,
                            rule,
                            overlaps,
                        });
                        let (luma, alpha) = self.color_parts(color, local);
                        let opacity = self.g.track(opacity, local, |o| o.clamp(0.0, 1.0));
                        let alpha = self.mul(alpha, opacity);
                        let luma = self.mul(luma, alpha);
                        children.push(Tree::Shape { index, luma, alpha });
                    }
                }
                Node::Image(_) | Node::Text(_) => return Ok(None),
            }
        }
        Ok(Some(Tree::Group {
            blend: g.blend,
            opacity,
            children,
            mask,
            clip,
        }))
    }

    /// The outline of the stroke of `s`, where it is still: its paths, the
    /// stroke's width and dash pattern do not move.
    fn stroked_outline(&self, s: &Shape, stroke: &Stroke) -> Option<Path> {
        let width = *stroke.width.as_static()?;
        let paths = static_paths(s).ok()?;
        let mut style = kurbo::Stroke::new(width.max(0.0))
            .with_caps(match stroke.cap {
                LineCap::Butt => kurbo::Cap::Butt,
                LineCap::Round => kurbo::Cap::Round,
                LineCap::Square => kurbo::Cap::Square,
            })
            .with_join(match stroke.join {
                LineJoin::Miter => kurbo::Join::Miter,
                LineJoin::Round => kurbo::Join::Round,
                LineJoin::Bevel => kurbo::Join::Bevel,
            })
            .with_miter_limit(stroke.miter_limit);
        if let Some(dash) = &stroke.dash {
            let mut array = Vec::with_capacity(dash.array.len());
            for d in &dash.array {
                array.push(d.as_static()?.max(0.0));
            }
            if array.len() % 2 == 1 {
                array.extend_from_within(..);
            }
            if array.iter().sum::<f64>() > 0.0 {
                style = style.with_dashes(*dash.offset.as_static()?, array);
            }
        }
        let mut outline = Path::new();
        for p in &paths {
            let stroked = kurbo::stroke(
                p.to_kurbo().iter(),
                &style,
                &kurbo::StrokeOpts::default(),
                self.tolerance(),
            );
            outline.extend(&Path::from_kurbo(&stroked));
        }
        Some(outline)
    }

    /// The luminance and the alpha of `color` at `time`.
    fn color_parts(&mut self, color: &Animated<Color>, time: NodeId) -> (Num, Num) {
        match color {
            Animated::Static(c) => (Num::Const(luminance(*c)), Num::Const(c.a)),
            Animated::Track(track) => {
                let times: Vec<f64> = track.keyframes.iter().map(|k| k.time).collect();
                let luma = self.g.sampled(&times, time, |t| luminance(color.sample(t)));
                let alpha = self.g.sampled(&times, time, |t| color.sample(t).a);
                (luma, alpha)
            }
        }
    }

    /// The luminance times alpha, and the alpha, that `tree` composites to
    /// in the region inside the shapes of the bits of `inside`.
    fn composite(&mut self, tree: &Tree, inside: usize) -> (Num, Num) {
        match tree {
            Tree::Shape { index, luma, alpha } => {
                if inside >> index & 1 == 1 {
                    (*luma, *alpha)
                } else {
                    (Num::Const(0.0), Num::Const(0.0))
                }
            }
            Tree::Group {
                opacity,
                children,
                mask,
                clip,
                ..
            } => {
                if clip.is_some_and(|i| inside >> i & 1 == 0) {
                    return (Num::Const(0.0), Num::Const(0.0));
                }
                let (mut l, mut a) = (Num::Const(0.0), Num::Const(0.0));
                for child in children {
                    let blend = match child {
                        Tree::Group { blend, .. } => *blend,
                        Tree::Shape { .. } => BlendMode::Normal,
                    };
                    let (cl, ca) = self.composite(child, inside);
                    let rest = self.g.math(MathOp::Sub, Num::Const(1.0), ca);
                    (l, a) = match blend {
                        // Source over.
                        BlendMode::Normal => {
                            let lr = self.mul(l, rest);
                            let ar = self.mul(a, rest);
                            (
                                self.g.math(MathOp::Add, cl, lr),
                                self.g.math(MathOp::Add, ca, ar),
                            )
                        }
                        BlendMode::DestinationIn => (self.mul(l, ca), self.mul(a, ca)),
                        _ => (self.mul(l, rest), self.mul(a, rest)),
                    };
                }
                if let Some((kind, inverted, tree)) = mask {
                    // Its own mask's content composites onto nothing.
                    let (ml, ma) = self.composite(tree, inside);
                    let blend = match tree.as_ref() {
                        Tree::Group { blend, .. } => *blend,
                        Tree::Shape { .. } => BlendMode::Normal,
                    };
                    let shown = match (blend, kind) {
                        (BlendMode::Normal, MaskKind::Alpha) => ma,
                        (BlendMode::Normal, MaskKind::Luminance) => ml,
                        _ => Num::Const(0.0),
                    };
                    let shown = if *inverted {
                        self.g.math(MathOp::Sub, Num::Const(1.0), shown)
                    } else {
                        shown
                    };
                    l = self.mul(l, shown);
                    a = self.mul(a, shown);
                }
                (self.mul(l, *opacity), self.mul(a, *opacity))
            }
        }
    }
}
