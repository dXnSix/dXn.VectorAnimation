//! Points, rectangles, affine matrices and paths: the scene's geometry, in
//! user units (y grows downwards, angles turn clockwise).

use std::fmt;

/// A point (or a vector) in user units.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Point {
    /// Horizontal, growing to the right.
    pub x: f64,
    /// Vertical, growing downwards.
    pub y: f64,
}

impl Point {
    /// The origin.
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    /// The point (`x`, `y`).
    pub const fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    /// The point a fraction `t` of the way to `to`.
    pub fn lerp(self, to: Point, t: f64) -> Point {
        Point::new(self.x + (to.x - self.x) * t, self.y + (to.y - self.y) * t)
    }

    /// Its distance from `other`.
    pub fn distance(self, other: Point) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

impl std::ops::Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Mul<f64> for Point {
    type Output = Point;
    fn mul(self, s: f64) -> Point {
        Point::new(self.x * s, self.y * s)
    }
}

/// An axis-aligned rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Rect {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

impl Rect {
    /// The smallest rectangle holding both.
    pub fn union(self, other: Rect) -> Rect {
        let (x0, y0) = (self.x.min(other.x), self.y.min(other.y));
        let x1 = (self.x + self.width).max(other.x + other.width);
        let y1 = (self.y + self.height).max(other.y + other.height);
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Where both are, if anywhere.
    pub fn intersect(self, other: Rect) -> Option<Rect> {
        let (x0, y0) = (self.x.max(other.x), self.y.max(other.y));
        let x1 = (self.x + self.width).min(other.x + other.width);
        let y1 = (self.y + self.height).min(other.y + other.height);
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    /// Grown by `by` on every side.
    pub fn grown(self, by: f64) -> Rect {
        Rect::new(
            self.x - by,
            self.y - by,
            self.width + 2.0 * by,
            self.height + 2.0 * by,
        )
    }

    /// The rectangle at (`x`, `y`) of `width` × `height`.
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
}

/// A 2D affine matrix as SVG writes it: `x' = a·x + c·y + e`,
/// `y' = b·x + d·y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Matrix {
    /// Scale and shear of x.
    pub a: f64,
    /// Shear of y by x.
    pub b: f64,
    /// Shear of x by y.
    pub c: f64,
    /// Scale and shear of y.
    pub d: f64,
    /// Horizontal translation.
    pub e: f64,
    /// Vertical translation.
    pub f: f64,
}

impl Default for Matrix {
    fn default() -> Self {
        Matrix::IDENTITY
    }
}

impl Matrix {
    /// No transformation.
    pub const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// The matrix `[a b c d e f]`.
    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Matrix { a, b, c, d, e, f }
    }

    /// A translation.
    pub const fn translate(x: f64, y: f64) -> Self {
        Matrix::new(1.0, 0.0, 0.0, 1.0, x, y)
    }

    /// A scale about the origin.
    pub const fn scale(x: f64, y: f64) -> Self {
        Matrix::new(x, 0.0, 0.0, y, 0.0, 0.0)
    }

    /// A rotation about the origin, clockwise in degrees.
    pub fn rotate(degrees: f64) -> Self {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Matrix::new(cos, sin, -sin, cos, 0.0, 0.0)
    }

    /// A skew: `x_degrees` along the x axis, `y_degrees` along the y axis
    /// (CSS `skew(x, y)`).
    pub fn skew(x_degrees: f64, y_degrees: f64) -> Self {
        Matrix::new(
            1.0,
            y_degrees.to_radians().tan(),
            x_degrees.to_radians().tan(),
            1.0,
            0.0,
            0.0,
        )
    }

    /// The matrix applying `self` first, then `next` (`next · self`).
    pub fn then(self, next: Matrix) -> Matrix {
        next * self
    }

    /// The point `p` transformed.
    pub fn apply(self, p: Point) -> Point {
        Point::new(
            self.a * p.x + self.c * p.y + self.e,
            self.b * p.x + self.d * p.y + self.f,
        )
    }

    /// The vector `v` transformed (without the translation).
    pub fn apply_vector(self, v: Point) -> Point {
        Point::new(self.a * v.x + self.c * v.y, self.b * v.x + self.d * v.y)
    }

    /// The determinant.
    pub fn determinant(self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// The inverse, if the matrix is invertible.
    pub fn invert(self) -> Option<Matrix> {
        let det = self.determinant();
        if det.abs() < 1e-12 || !det.is_finite() {
            return None;
        }
        let inv = 1.0 / det;
        Some(Matrix::new(
            self.d * inv,
            -self.b * inv,
            -self.c * inv,
            self.a * inv,
            (self.c * self.f - self.d * self.e) * inv,
            (self.b * self.e - self.a * self.f) * inv,
        ))
    }

    /// Whether every entry is finite.
    pub fn is_finite(self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|v| v.is_finite())
    }

    /// The matrix as kurbo's.
    pub fn to_kurbo(self) -> kurbo::Affine {
        kurbo::Affine::new([self.a, self.b, self.c, self.d, self.e, self.f])
    }
}

/// `self · other`: the matrix applying `other` first, then `self` (CSS and
/// SVG compose transform lists this way, left to right).
impl std::ops::Mul for Matrix {
    type Output = Matrix;

    fn mul(self, o: Matrix) -> Matrix {
        let s = self;
        Matrix::new(
            s.a * o.a + s.c * o.b,
            s.b * o.a + s.d * o.b,
            s.a * o.c + s.c * o.d,
            s.b * o.c + s.d * o.d,
            s.a * o.e + s.c * o.f + s.e,
            s.b * o.e + s.d * o.f + s.f,
        )
    }
}

/// One element of a [`Path`], in absolute coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathEl {
    /// Starts a subpath.
    MoveTo(Point),
    /// A straight line.
    LineTo(Point),
    /// A quadratic Bézier curve: control point, end.
    QuadTo(Point, Point),
    /// A cubic Bézier curve: two control points, end.
    CubicTo(Point, Point, Point),
    /// A line back to the subpath's start.
    Close,
}

impl PathEl {
    fn kind(self) -> u8 {
        match self {
            PathEl::MoveTo(_) => 0,
            PathEl::LineTo(_) => 1,
            PathEl::QuadTo(..) => 2,
            PathEl::CubicTo(..) => 3,
            PathEl::Close => 4,
        }
    }

    fn map(self, f: impl Fn(Point) -> Point) -> PathEl {
        match self {
            PathEl::MoveTo(p) => PathEl::MoveTo(f(p)),
            PathEl::LineTo(p) => PathEl::LineTo(f(p)),
            PathEl::QuadTo(c, p) => PathEl::QuadTo(f(c), f(p)),
            PathEl::CubicTo(c1, c2, p) => PathEl::CubicTo(f(c1), f(c2), f(p)),
            PathEl::Close => PathEl::Close,
        }
    }
}

/// A path: subpaths of lines and Bézier curves, in absolute coordinates.
///
/// Serialized as compact path data (`M0 0L10 0C…Z`, absolute commands only).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Path(pub Vec<PathEl>);

impl Path {
    /// An empty path.
    pub const fn new() -> Self {
        Path(Vec::new())
    }

    /// Its elements.
    pub fn elements(&self) -> &[PathEl] {
        &self.0
    }

    /// Whether it has no elements.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Starts a subpath at `p`.
    pub fn move_to(&mut self, p: Point) {
        self.0.push(PathEl::MoveTo(p));
    }

    /// Adds a line to `p`.
    pub fn line_to(&mut self, p: Point) {
        self.0.push(PathEl::LineTo(p));
    }

    /// Adds a quadratic curve.
    pub fn quad_to(&mut self, c: Point, p: Point) {
        self.0.push(PathEl::QuadTo(c, p));
    }

    /// Adds a cubic curve.
    pub fn cubic_to(&mut self, c1: Point, c2: Point, p: Point) {
        self.0.push(PathEl::CubicTo(c1, c2, p));
    }

    /// Closes the subpath.
    pub fn close(&mut self) {
        self.0.push(PathEl::Close);
    }

    /// Adds `other`'s elements.
    pub fn extend(&mut self, other: &Path) {
        self.0.extend_from_slice(&other.0);
    }

    /// The path with every point transformed by `m`.
    pub fn transformed(&self, m: Matrix) -> Path {
        Path(self.0.iter().map(|el| el.map(|p| m.apply(p))).collect())
    }

    /// Whether `other` has the same commands in the same order, so the two
    /// interpolate point by point.
    pub fn same_structure(&self, other: &Path) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(&other.0)
                .all(|(a, b)| a.kind() == b.kind())
    }

    /// The number of points it holds.
    pub fn point_count(&self) -> usize {
        self.0
            .iter()
            .map(|el| match el {
                PathEl::MoveTo(_) | PathEl::LineTo(_) => 1,
                PathEl::QuadTo(..) => 2,
                PathEl::CubicTo(..) => 3,
                PathEl::Close => 0,
            })
            .sum()
    }

    /// The path as kurbo's.
    pub fn to_kurbo(&self) -> kurbo::BezPath {
        let mut out = kurbo::BezPath::new();
        let k = |p: Point| kurbo::Point::new(p.x, p.y);
        let mut open = false;
        for el in &self.0 {
            match *el {
                PathEl::MoveTo(p) => {
                    out.move_to(k(p));
                    open = true;
                }
                PathEl::LineTo(p) if open => out.line_to(k(p)),
                PathEl::QuadTo(c, p) if open => out.quad_to(k(c), k(p)),
                PathEl::CubicTo(c1, c2, p) if open => out.curve_to(k(c1), k(c2), k(p)),
                PathEl::Close if open => out.close_path(),
                // Drawing before any `MoveTo` starts at the origin.
                PathEl::LineTo(p) => {
                    out.move_to(kurbo::Point::ZERO);
                    out.line_to(k(p));
                    open = true;
                }
                PathEl::QuadTo(c, p) => {
                    out.move_to(kurbo::Point::ZERO);
                    out.quad_to(k(c), k(p));
                    open = true;
                }
                PathEl::CubicTo(c1, c2, p) => {
                    out.move_to(kurbo::Point::ZERO);
                    out.curve_to(k(c1), k(c2), k(p));
                    open = true;
                }
                PathEl::Close => {}
            }
        }
        out
    }

    /// A path from kurbo's.
    pub fn from_kurbo(path: &kurbo::BezPath) -> Path {
        let p = |p: kurbo::Point| Point::new(p.x, p.y);
        Path(
            path.elements()
                .iter()
                .map(|el| match *el {
                    kurbo::PathEl::MoveTo(a) => PathEl::MoveTo(p(a)),
                    kurbo::PathEl::LineTo(a) => PathEl::LineTo(p(a)),
                    kurbo::PathEl::QuadTo(a, b) => PathEl::QuadTo(p(a), p(b)),
                    kurbo::PathEl::CurveTo(a, b, c) => PathEl::CubicTo(p(a), p(b), p(c)),
                    kurbo::PathEl::ClosePath => PathEl::Close,
                })
                .collect(),
        )
    }

    /// Its bounding box (of the curves, not of their control points), or
    /// `None` when it is empty.
    pub fn bounds(&self) -> Option<Rect> {
        if self.0.iter().all(|el| matches!(el, PathEl::Close)) {
            return None;
        }
        let b = kurbo::Shape::bounding_box(&self.to_kurbo());
        Some(Rect::new(b.x0, b.y0, b.width(), b.height()))
    }

    /// Parses path data as [`Path`]'s `Display` writes it (absolute `M`,
    /// `L`, `Q`, `C` and `Z`).
    pub fn parse(text: &str) -> Option<Path> {
        let mut out = Path::new();
        let mut chars = text.char_indices().peekable();
        let mut numbers = Vec::with_capacity(6);
        let mut command = None;
        let flush = |command: Option<char>, numbers: &mut Vec<f64>, out: &mut Path| -> bool {
            let Some(c) = command else {
                return numbers.is_empty();
            };
            let pt = |i: usize| Point::new(numbers[i], numbers[i + 1]);
            let ok = match (c, numbers.len()) {
                ('M', 2) => {
                    out.move_to(pt(0));
                    true
                }
                ('L', 2) => {
                    out.line_to(pt(0));
                    true
                }
                ('Q', 4) => {
                    out.quad_to(pt(0), pt(2));
                    true
                }
                ('C', 6) => {
                    out.cubic_to(pt(0), pt(2), pt(4));
                    true
                }
                ('Z', 0) => {
                    out.close();
                    true
                }
                _ => false,
            };
            numbers.clear();
            ok
        };
        while let Some(&(i, c)) = chars.peek() {
            if c.is_ascii_whitespace() || c == ',' {
                chars.next();
            } else if matches!(c, 'M' | 'L' | 'Q' | 'C' | 'Z') {
                if !flush(command, &mut numbers, &mut out) {
                    return None;
                }
                command = Some(c);
                chars.next();
            } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
                let start = i;
                let mut end = i + c.len_utf8();
                chars.next();
                while let Some(&(j, d)) = chars.peek() {
                    let exponent_sign = (d == '-' || d == '+') && text[..j].ends_with(['e', 'E']);
                    if d.is_ascii_digit() || d == '.' || d == 'e' || d == 'E' || exponent_sign {
                        end = j + d.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                numbers.push(text[start..end].parse().ok()?);
            } else {
                return None;
            }
        }
        flush(command, &mut numbers, &mut out).then_some(out)
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = |f: &mut fmt::Formatter<'_>, p: Point| write!(f, "{} {}", p.x, p.y);
        for el in &self.0 {
            match *el {
                PathEl::MoveTo(a) => {
                    f.write_str("M")?;
                    p(f, a)?;
                }
                PathEl::LineTo(a) => {
                    f.write_str("L")?;
                    p(f, a)?;
                }
                PathEl::QuadTo(a, b) => {
                    f.write_str("Q")?;
                    p(f, a)?;
                    f.write_str(" ")?;
                    p(f, b)?;
                }
                PathEl::CubicTo(a, b, c) => {
                    f.write_str("C")?;
                    p(f, a)?;
                    f.write_str(" ")?;
                    p(f, b)?;
                    f.write_str(" ")?;
                    p(f, c)?;
                }
                PathEl::Close => f.write_str("Z")?,
            }
        }
        Ok(())
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Path {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Path {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Path::parse(&text).ok_or_else(|| serde::de::Error::custom("invalid path data"))
    }
}
