//! Whether a point falls on what a group draws: the outlines of the shapes
//! in it, as a player hit-tests what a pointer is over.

use kurbo::Shape as _;

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
