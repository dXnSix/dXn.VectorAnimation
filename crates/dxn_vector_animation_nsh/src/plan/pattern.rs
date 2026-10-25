//! Pattern fills on the canvas: the shape's outline clips, and the tiles
//! that cover it draw their content (as a frame draws it: still), each
//! moved to its place. What moves in a pattern, a box that moves under
//! one, strokes and too many tiles are drawn as the reference draws them.

use dxn_vector_animation::{
    FillRule, FramePaint, Group, Item, LoopMode, Node, Paint as ScenePaint, Pattern, Scene, Shape,
    Timeline,
};

use super::{Compiler, Num, Op, Unsupported, consts, frame_items, static_paths, unsupported};

/// The most tiles a pattern fill draws.
const MAX_TILES: usize = 256;

/// Whether `g` draws the same at every time of `timeline`.
fn still(g: &Group, timeline: &Timeline) -> bool {
    let scene = Scene {
        width: 1.0,
        height: 1.0,
        timeline: Timeline {
            loop_mode: LoopMode::Once,
            ..timeline.clone()
        },
        root: g.clone(),
    };
    super::still(&scene)
}

impl Compiler {
    /// The fill of `s` with pattern `p` at alpha `a`: the tiles over its
    /// outline (which clips them), each drawing the content.
    pub(super) fn pattern_fill(
        &mut self,
        s: &Shape,
        p: &Pattern,
        a: Num,
    ) -> Result<Vec<Op>, Unsupported> {
        if self.casting.is_some() {
            return unsupported("a shadow of a pattern");
        }
        if !still(&p.content, &self.stage.timeline) {
            return unsupported("a pattern whose content moves");
        }
        // Where the outline is: the tiles over it; in the box's units, the
        // box must hold still.
        let paths = static_paths(s)?;
        let mut outline = dxn_vector_animation::Path::new();
        for path in &paths {
            outline.extend(path);
        }
        let Some(bounds) = outline.bounds() else {
            return Ok(Vec::new());
        };
        // The pattern as a frame sees it: its tile, transform and content.
        let still_shape = Shape {
            paths: paths
                .iter()
                .map(|path| {
                    dxn_vector_animation::ShapePath::new(dxn_vector_animation::Geometry::path(
                        path.clone(),
                    ))
                })
                .collect(),
            fill: Some(dxn_vector_animation::Fill {
                paint: ScenePaint::Pattern(Box::new(p.clone())),
                opacity: dxn_vector_animation::Animated::Static(1.0),
                rule: FillRule::NonZero,
            }),
            stroke: None,
            trim: None,
            ..Shape::default()
        };
        let frame = Scene {
            width: self.stage.width,
            height: self.stage.height,
            timeline: self.stage.timeline.clone(),
            root: Group {
                children: vec![Node::shape(still_shape)],
                ..Group::new()
            },
        }
        .frame_at(0.0);
        let Some(FramePaint::Pattern {
            tile,
            transform,
            items,
        }) = frame.items.iter().find_map(|i| match i {
            Item::Fill { paint, .. } => Some(paint.clone()),
            _ => None,
        })
        else {
            return Ok(Vec::new());
        };
        let Some(inverse) = transform.invert() else {
            return Ok(Vec::new());
        };
        // The outline's box in the pattern's coordinates: the tiles over it.
        let corners = [
            (bounds.x, bounds.y),
            (bounds.x + bounds.width, bounds.y),
            (bounds.x, bounds.y + bounds.height),
            (bounds.x + bounds.width, bounds.y + bounds.height),
        ]
        .map(|(x, y)| inverse.apply(dxn_vector_animation::Point::new(x, y)));
        let (x0, x1) = corners
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
                (lo.min(c.x), hi.max(c.x))
            });
        let (y0, y1) = corners
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
                (lo.min(c.y), hi.max(c.y))
            });
        let (i0, i1) = (
            ((x0 - tile.x) / tile.width).floor() as i64,
            ((x1 - tile.x) / tile.width).ceil() as i64,
        );
        let (j0, j1) = (
            ((y0 - tile.y) / tile.height).floor() as i64,
            ((y1 - tile.y) / tile.height).ceil() as i64,
        );
        let count = (i1 - i0).max(0) as u64 * (j1 - j0).max(0) as u64;
        if count as usize > MAX_TILES {
            return unsupported("a pattern of many tiles");
        }
        // One tile's drawing, its alphas times the fill's.
        let mut alpha = vec![1.0];
        let mut content = Vec::new();
        frame_items(
            &items,
            dxn_vector_animation::Matrix::IDENTITY,
            &mut alpha,
            &mut content,
        );
        let content: Vec<Op> = content
            .into_iter()
            .map(|op| match op {
                Op::GlobalAlpha(k) => Op::GlobalAlpha(self.mul(a, k)),
                other => other,
            })
            .collect();
        let mut ops = Vec::new();
        if transform != dxn_vector_animation::Matrix::IDENTITY {
            ops.push(Op::Transform(consts(transform)));
        }
        let c = Num::Const;
        for j in j0..j1 {
            for i in i0..i1 {
                ops.push(Op::Save);
                ops.push(Op::Translate(
                    c(tile.x + i as f64 * tile.width),
                    c(tile.y + j as f64 * tile.height),
                ));
                // Each tile draws within itself.
                ops.extend([
                    Op::BeginPath,
                    Op::MoveTo(c(0.0), c(0.0)),
                    Op::LineTo(c(tile.width), c(0.0)),
                    Op::LineTo(c(tile.width), c(tile.height)),
                    Op::LineTo(c(0.0), c(tile.height)),
                    Op::Close,
                    Op::Clip(FillRule::NonZero),
                ]);
                ops.extend(content.iter().cloned());
                ops.push(Op::Restore);
            }
        }
        Ok(ops)
    }
}
