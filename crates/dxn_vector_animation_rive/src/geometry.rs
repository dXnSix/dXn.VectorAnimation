//! Rive's outlines: vertices (straight ones rounding their corner, cubic
//! ones with tangents) built into paths as rive-runtime's `Path::buildPath`
//! builds them, and the vertices of its parametric shapes.

use std::f64::consts::PI;

use dxn_vector_animation::{Path, PathEl, Point};

/// A point of an outline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Vertex {
    /// A corner, rounded by `radius` (cut inwards when negative).
    Straight { p: Point, radius: f64 },
    /// A point with the tangents in and out of it.
    Cubic { p: Point, r#in: Point, out: Point },
}

fn sub(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}

fn add_scaled(a: Point, v: Point, s: f64) -> Point {
    Point::new(a.x + v.x * s, a.y + v.y * s)
}

/// `v` made a unit vector, and its length.
fn normalize(v: Point) -> (Point, f64) {
    let len = (v.x * v.x + v.y * v.y).sqrt();
    if len > 0.0 {
        (Point::new(v.x / len, v.y / len), len)
    } else {
        (v, 0.0)
    }
}

fn cross(a: Point, b: Point) -> f64 {
    a.x * b.y - a.y * b.x
}

fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

/// The control point distance rounding the corner between `to_prev` and
/// `to_next` (unit vectors) by `radius` ("natural rounding").
fn ideal_distance(to_prev: Point, to_next: Point, radius: f64) -> f64 {
    let angle = cross(to_prev, to_next).atan2(dot(to_prev, to_next)).abs();
    if angle == 0.0 {
        return radius;
    }
    let factor = if angle < PI / 2.0 {
        1.0 + angle.cos()
    } else {
        2.0 - angle.sin()
    };
    radius.min(4.0 / 3.0 * (PI / (2.0 * (2.0 * PI / angle))).tan() * radius * factor)
}

fn rotate_about(p: Point, about: Point, angle: f64) -> Point {
    let (s, c) = angle.sin_cos();
    let d = sub(p, about);
    Point::new(d.x * c - d.y * s + about.x, d.x * s + d.y * c + about.y)
}

/// Turns a rounded corner's control points into a cut-in one's.
fn rotate_points(next: Point, prev: Point, point: Point, out: &mut Point, r#in: &mut Point) {
    let v1 = sub(prev, next);
    let v2 = sub(point, next);
    let angle = cross(v1, v2).atan2(dot(v1, v2));
    *out = rotate_about(*out, prev, angle * 2.0);
    *r#in = rotate_about(*r#in, next, -angle * 2.0);
}

/// The corner at straight vertex `pos` between `prev` and `next`: where the
/// curve starts, its control points and where it ends.
fn corner(prev: &Vertex, pos: Point, radius: f64, next: &Vertex) -> (Point, Point, Point, Point) {
    let prev_point = match *prev {
        Vertex::Cubic { out, .. } => out,
        Vertex::Straight { p, .. } => p,
    };
    let next_point = match *next {
        Vertex::Cubic { r#in, .. } => r#in,
        Vertex::Straight { p, .. } => p,
    };
    let (to_prev, prev_len) = normalize(sub(prev_point, pos));
    let (to_next, next_len) = normalize(sub(next_point, pos));
    let render = (prev_len / 2.0).min(next_len / 2.0).min(radius.abs());
    let ideal = ideal_distance(to_prev, to_next, render);
    let start = add_scaled(pos, to_prev, render);
    let mut c1 = add_scaled(pos, to_prev, render - ideal);
    let mut c2 = add_scaled(pos, to_next, render - ideal);
    let end = add_scaled(pos, to_next, render);
    if radius < 0.0 {
        rotate_points(end, start, pos, &mut c1, &mut c2);
    }
    (start, c1, c2, end)
}

/// The path through `vertices`.
pub(crate) fn build(vertices: &[Vertex], closed: bool) -> Path {
    let mut path = Path::default();
    let n = vertices.len();
    if n < 2 {
        return path;
    }
    let (mut out, mut prev_cubic, start, start_in, start_cubic);
    match vertices[0] {
        Vertex::Cubic { p, r#in, out: o } => {
            start_cubic = true;
            prev_cubic = true;
            start_in = r#in;
            out = o;
            start = p;
            path.move_to(start);
        }
        Vertex::Straight { p, radius } if radius != 0.0 => {
            start_cubic = false;
            let (s, c1, c2, end) = corner(&vertices[n - 1], p, radius, &vertices[1]);
            start = s;
            start_in = s;
            path.move_to(s);
            path.cubic_to(c1, c2, end);
            out = end;
            prev_cubic = false;
        }
        Vertex::Straight { p, .. } => {
            start_cubic = false;
            prev_cubic = false;
            start = p;
            start_in = p;
            out = p;
            path.move_to(p);
        }
    }
    for i in 1..n {
        match vertices[i] {
            Vertex::Cubic { p, r#in, out: o } => {
                path.cubic_to(out, r#in, p);
                prev_cubic = true;
                out = o;
            }
            Vertex::Straight { p, radius } if radius != 0.0 => {
                let (s, c1, c2, end) = corner(&vertices[i - 1], p, radius, &vertices[(i + 1) % n]);
                if prev_cubic {
                    path.cubic_to(out, s, s);
                } else {
                    path.line_to(s);
                }
                path.cubic_to(c1, c2, end);
                out = end;
                prev_cubic = false;
            }
            Vertex::Straight { p, .. } => {
                if prev_cubic {
                    path.cubic_to(out, p, p);
                    prev_cubic = false;
                } else {
                    path.line_to(p);
                }
                out = p;
            }
        }
    }
    if closed {
        if prev_cubic || start_cubic {
            path.cubic_to(out, start_in, start);
        } else {
            path.line_to(start);
        }
        path.close();
    }
    path
}

/// `path` running the other way: each contour from its end to its start, as
/// rive-runtime's `addPathBackwards` turns it.
pub(crate) fn reverse(path: &Path) -> Path {
    let mut out = Path::default();
    let mut contour: Vec<PathEl> = Vec::new();
    let flush = |contour: &mut Vec<PathEl>, out: &mut Path| {
        let Some(PathEl::MoveTo(first)) = contour.first().copied() else {
            contour.clear();
            return;
        };
        let closed = matches!(contour.last(), Some(PathEl::Close));
        // Each segment's end point, the start first.
        let mut points = vec![first];
        let mut segments = Vec::new();
        for el in &contour[1..] {
            match *el {
                PathEl::LineTo(p) => {
                    segments.push(PathEl::LineTo(p));
                    points.push(p);
                }
                PathEl::QuadTo(c, p) => {
                    segments.push(PathEl::QuadTo(c, p));
                    points.push(p);
                }
                PathEl::CubicTo(c1, c2, p) => {
                    segments.push(PathEl::CubicTo(c1, c2, p));
                    points.push(p);
                }
                PathEl::MoveTo(_) | PathEl::Close => {}
            }
        }
        out.move_to(points[points.len() - 1]);
        for (i, seg) in segments.iter().enumerate().rev() {
            let to = points[i];
            match *seg {
                PathEl::LineTo(_) => out.line_to(to),
                PathEl::QuadTo(c, _) => out.quad_to(c, to),
                PathEl::CubicTo(c1, c2, _) => out.cubic_to(c2, c1, to),
                _ => {}
            }
        }
        if closed {
            out.close();
        }
        contour.clear();
    };
    for el in path.elements() {
        if matches!(el, PathEl::MoveTo(_)) {
            flush(&mut contour, &mut out);
        }
        contour.push(*el);
    }
    flush(&mut contour, &mut out);
    out
}

/// A rectangle's corners from its top left, clockwise, with their radii
/// (top left, top right, bottom right, bottom left).
pub(crate) fn rectangle(w: f64, h: f64, ox: f64, oy: f64, radii: [f64; 4]) -> Vec<Vertex> {
    let (x, y) = (-ox * w, -oy * h);
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
        .iter()
        .zip(radii)
        .map(|(&(px, py), radius)| Vertex::Straight {
            p: Point::new(px, py),
            radius,
        })
        .collect()
}

/// A regular polygon's corners, the first at the top; with `inner` (a
/// fraction of the size), a star's points and valleys in turn.
pub(crate) fn polygon(
    w: f64,
    h: f64,
    ox: f64,
    oy: f64,
    points: usize,
    corner: f64,
    inner: Option<f64>,
) -> Vec<Vertex> {
    let (hw, hh) = (w / 2.0, h / 2.0);
    let (cx, cy) = (-ox * w + hw, -oy * h + hh);
    let count = if inner.is_some() { points * 2 } else { points };
    let inc = 2.0 * PI / count as f64;
    (0..count)
        .map(|i| {
            let angle = -PI / 2.0 + inc * i as f64;
            let scale = match inner {
                Some(r) if i % 2 == 1 => r,
                _ => 1.0,
            };
            Vertex::Straight {
                p: Point::new(cx + angle.cos() * hw * scale, cy + angle.sin() * hh * scale),
                radius: corner,
            }
        })
        .collect()
}

/// A triangle's corners: the top middle, the bottom right, the bottom left.
pub(crate) fn triangle(w: f64, h: f64, ox: f64, oy: f64) -> Vec<Vertex> {
    let (x, y) = (-ox * w, -oy * h);
    [(x + w / 2.0, y), (x + w, y + h), (x, y + h)]
        .iter()
        .map(|&(px, py)| Vertex::Straight {
            p: Point::new(px, py),
            radius: 0.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rounded_square_rounds_each_corner_as_a_quarter_circle() {
        let path = build(&rectangle(100.0, 100.0, 0.0, 0.0, [10.0; 4]), true);
        let els = path.elements();
        // Starts below the top left corner on the left edge, then rounds it.
        assert_eq!(els[0], PathEl::MoveTo(Point::new(0.0, 10.0)));
        let PathEl::CubicTo(c1, _, end) = els[1] else {
            panic!("{els:?}");
        };
        assert_eq!(end, Point::new(10.0, 0.0));
        assert!(
            (c1.y - (10.0 - 10.0 * 0.552_284_749_831)).abs() < 1e-6,
            "{c1:?}"
        );
    }

    #[test]
    fn reversing_twice_gives_the_path_back() {
        let path = build(&rectangle(100.0, 50.0, 0.5, 0.5, [8.0; 4]), true);
        assert_eq!(reverse(&reverse(&path)), path);
        let PathEl::MoveTo(start) = reverse(&path).elements()[0] else {
            panic!("no move");
        };
        let PathEl::MoveTo(first) = path.elements()[0] else {
            panic!("no move");
        };
        assert_eq!(start, first);
    }

    #[test]
    fn straight_points_make_lines() {
        let path = build(&triangle(10.0, 10.0, 0.0, 0.0), true);
        assert_eq!(
            path.elements(),
            &[
                PathEl::MoveTo(Point::new(5.0, 0.0)),
                PathEl::LineTo(Point::new(10.0, 10.0)),
                PathEl::LineTo(Point::new(0.0, 10.0)),
                PathEl::LineTo(Point::new(5.0, 0.0)),
                PathEl::Close,
            ]
        );
    }
}
