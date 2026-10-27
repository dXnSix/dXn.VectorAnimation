//! Whether a point falls on what a group draws: the outlines of the shapes
//! in it, as a player hit-tests what a pointer is over.

use std::collections::HashMap;

use kurbo::{Line, ParamCurve as _, Shape as _};

use crate::{
    Group, Matrix, Node, Path, Point, Rect, Scene, outline::geometry_path, transform::transform_at,
};

/// How deep the search goes.
const MAX_DEPTH: usize = 256;

impl Scene {
    /// Whether `point` (in the scene's coordinates) is inside an outline of
    /// a shape in the group with id `id` at `time`: the group nearest the
    /// root of those with that id, its shapes' paths filled under the
    /// non-zero rule (strokes, clips and opacity aside).
    pub fn hit(&self, id: &str, point: Point, time: f64) -> bool {
        let p = kurbo::Point::new(point.x, point.y);
        let mut hit = false;
        if let Some((group, m, time)) = self.target(id, time) {
            outlines(group, m, time, 0, &mut |path| {
                hit = hit || path.to_kurbo().winding(p) != 0;
            });
        }
        hit
    }

    /// Whether the outlines [`Scene::hit`] tests for the group with id `id`
    /// at `time` overlap `rect` (in the scene's coordinates): a point of it
    /// inside them, or their edges in it. A pointer hits a little around
    /// itself so (Rive's runtime tests the box 2 units around it).
    pub fn hit_rect(&self, id: &str, rect: Rect, time: f64) -> bool {
        self.hit_rect_in(None, id, rect, time)
    }

    /// [`Scene::hit_rect`], the group found through `index` (made of this
    /// scene) rather than searched for.
    pub fn hit_rect_indexed(&self, index: &HitIndex, id: &str, rect: Rect, time: f64) -> bool {
        self.hit_rect_in(Some(index), id, rect, time)
    }

    fn hit_rect_in(&self, index: Option<&HitIndex>, id: &str, rect: Rect, time: f64) -> bool {
        let mut hit = false;
        let target = match index {
            Some(index) => self.indexed(index, id, time),
            None => self.target(id, time),
        };
        if let Some((group, m, time)) = target {
            outlines(group, m, time, 0, &mut |path| {
                hit = hit || overlaps(&path.to_kurbo(), rect);
            });
        }
        hit
    }

    /// The bounds (in the scene's coordinates) of the outlines [`Scene::hit`]
    /// tests for the group with id `id` at `time`, or `None` when it has
    /// none.
    pub fn hit_bounds(&self, id: &str, time: f64) -> Option<Rect> {
        let mut bounds: Option<Rect> = None;
        if let Some((group, m, time)) = self.target(id, time) {
            outlines(group, m, time, 0, &mut |path| {
                if let Some(b) = path.bounds() {
                    bounds = Some(bounds.map_or(b, |a| a.union(b)));
                }
            });
        }
        bounds
    }

    /// The group with id `id` nearest the root, active at `time`: it, what
    /// draws it and its local time.
    fn target(&self, id: &str, time: f64) -> Option<(&Group, Matrix, f64)> {
        // Breadth first: the group nearest the root wins.
        let mut level: Vec<(&Group, Matrix, f64)> = vec![(&self.root, Matrix::IDENTITY, time)];
        for _ in 0..MAX_DEPTH {
            let mut next = Vec::new();
            for &(group, parent, time) in &level {
                if group.active.is_some_and(|range| !range.contains(time)) {
                    continue;
                }
                let time = group.local_time(time);
                let m = parent * transform_at(&group.transform, time);
                if group.id.as_deref() == Some(id) {
                    return Some((group, m, time));
                }
                for child in &group.children {
                    if let Node::Group(g) = child {
                        next.push((g, m, time));
                    }
                }
            }
            if next.is_empty() {
                return None;
            }
            level = next;
        }
        None
    }
}

impl Scene {
    /// The group with id `id` [`Scene::target`] finds, through `index`:
    /// the first of the id's groups, nearest the root first, active at
    /// `time` with all that holds it.
    fn indexed(&self, index: &HitIndex, id: &str, time: f64) -> Option<(&Group, Matrix, f64)> {
        'paths: for path in index.at.get(id)? {
            let mut group = &self.root;
            let mut m = Matrix::IDENTITY;
            let mut time = time;
            for k in std::iter::once(None).chain(path.iter().map(|&k| Some(k as usize))) {
                if let Some(k) = k {
                    group = match group.children.get(k) {
                        Some(Node::Group(g)) => g,
                        // An index of another scene.
                        _ => continue 'paths,
                    };
                }
                if group.active.is_some_and(|range| !range.contains(time)) {
                    continue 'paths;
                }
                time = group.local_time(time);
                m = m * transform_at(&group.transform, time);
            }
            if group.id.as_deref() == Some(id) {
                return Some((group, m, time));
            }
        }
        None
    }
}

/// Where a scene's groups with ids are, so that hit tests find them at once
/// rather than searching the scene (as [`Scene::hit_rect`] does each time):
/// for a player hit-testing a pointer's every move over many targets.
#[derive(Debug, Clone, Default)]
pub struct HitIndex {
    /// Each id's groups, nearest the root first (breadth first, as
    /// [`Scene::hit`] searches): the children's indices from the root.
    at: HashMap<String, Vec<Box<[u32]>>>,
    /// Each id's place in the order the scene draws its groups (its first
    /// group's).
    order: HashMap<String, usize>,
}

impl HitIndex {
    /// The index of `scene`'s groups with ids.
    pub fn new(scene: &Scene) -> HitIndex {
        let mut index = HitIndex::default();
        let mut level: Vec<(&Group, Vec<u32>)> = vec![(&scene.root, Vec::new())];
        for _ in 0..MAX_DEPTH {
            let mut next = Vec::new();
            for (group, path) in level {
                if let Some(id) = &group.id {
                    index
                        .at
                        .entry(id.clone())
                        .or_default()
                        .push(path.clone().into_boxed_slice());
                }
                for (k, child) in group.children.iter().enumerate() {
                    if let (Node::Group(g), Ok(k)) = (child, u32::try_from(k)) {
                        let mut path = path.clone();
                        path.push(k);
                        next.push((g, path));
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            level = next;
        }
        fn walk(g: &Group, order: &mut HashMap<String, usize>, depth: usize) {
            if depth > MAX_DEPTH {
                return;
            }
            if let Some(id) = &g.id {
                let n = order.len();
                order.entry(id.clone()).or_insert(n);
            }
            for c in &g.children {
                if let Node::Group(c) = c {
                    walk(c, order, depth + 1);
                }
            }
        }
        walk(&scene.root, &mut index.order, 0);
        index
    }

    /// Where the first group with id `id` comes in the order the scene draws
    /// its groups with ids (depth first, what is drawn later over what is
    /// drawn earlier).
    pub fn order(&self, id: &str) -> Option<usize> {
        self.order.get(id).copied()
    }
}

/// Calls `f` with the outline of each shape in `group` (drawn by `m`) at
/// `time`, in the scene's coordinates.
fn outlines(group: &Group, m: Matrix, time: f64, depth: usize, f: &mut dyn FnMut(&Path)) {
    if depth > MAX_DEPTH || !m.is_finite() {
        return;
    }
    for child in &group.children {
        match child {
            Node::Group(g) => {
                if !g.active.is_some_and(|range| !range.contains(time)) {
                    let time = g.local_time(time);
                    outlines(g, m * transform_at(&g.transform, time), time, depth + 1, f);
                }
            }
            Node::Shape(s) => {
                let mut path = Path::default();
                for sp in &s.paths {
                    let local = m * transform_at(&sp.transform, time);
                    path.extend(&geometry_path(&sp.geometry, time).transformed(local));
                }
                f(&path);
            }
            _ => {}
        }
    }
}

/// Whether `path`, filled under the non-zero rule, overlaps `rect`.
fn overlaps(path: &kurbo::BezPath, rect: Rect) -> bool {
    let r = kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
    let corners = [
        kurbo::Point::new(r.x0, r.y0),
        kurbo::Point::new(r.x1, r.y0),
        kurbo::Point::new(r.x1, r.y1),
        kurbo::Point::new(r.x0, r.y1),
    ];
    // A corner or the middle inside the fill.
    if corners
        .iter()
        .chain([&r.center()])
        .any(|&p| path.winding(p) != 0)
    {
        return true;
    }
    // An edge of the fill in the rectangle, or crossing its sides.
    let sides: Vec<Line> = (0..4)
        .map(|k| Line::new(corners[k], corners[(k + 1) % 4]))
        .collect();
    path.segments().any(|seg| {
        r.contains(seg.start())
            || sides
                .iter()
                .any(|&side| !seg.intersect_line(side).is_empty())
    })
}
