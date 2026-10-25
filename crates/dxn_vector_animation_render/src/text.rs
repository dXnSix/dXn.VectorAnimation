//! Text set in the system's fonts: spans shaped (kerning and ligatures
//! included) into glyph outlines, lines placed by their anchors.

use std::sync::OnceLock;

use dxn_vector_animation::{FrameTextSpan, Matrix, Path, Point, TextAnchor};

/// The system's fonts, loaded on first use.
fn database() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    })
}

/// Whether the system has any font to set text in.
pub fn fonts_available() -> bool {
    database().faces().next().is_some()
}

/// The face `span` is set in: the first of its families the system has,
/// else its sans-serif, else any.
fn face(db: &fontdb::Database, span: &FrameTextSpan) -> Option<fontdb::ID> {
    use fontdb::Family;
    let mut families: Vec<Family<'_>> = span
        .families
        .iter()
        .map(|f| {
            let f = f.trim().trim_matches(|c| c == '"' || c == '\'');
            match f.to_ascii_lowercase().as_str() {
                "serif" | "ui-serif" => Family::Serif,
                "sans-serif" | "system-ui" | "ui-sans-serif" | "-apple-system" => Family::SansSerif,
                "monospace" | "ui-monospace" => Family::Monospace,
                "cursive" => Family::Cursive,
                "fantasy" => Family::Fantasy,
                _ => Family::Name(f),
            }
        })
        .collect();
    families.push(Family::SansSerif);
    let query = fontdb::Query {
        families: &families,
        weight: fontdb::Weight(span.weight.clamp(1, 1000)),
        stretch: fontdb::Stretch::Normal,
        style: if span.italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        },
    };
    db.query(&query).or_else(|| db.faces().next().map(|f| f.id))
}

/// Glyph outlines into a path: font units scaled by `k`, y turned down,
/// from `origin`.
struct Outline<'a> {
    path: &'a mut Path,
    origin: Point,
    k: f64,
}

impl Outline<'_> {
    fn at(&self, x: f32, y: f32) -> Point {
        Point::new(
            self.origin.x + f64::from(x) * self.k,
            self.origin.y - f64::from(y) * self.k,
        )
    }
}

impl ttf_parser::OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.path.move_to(p);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.path.line_to(p);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.at(x1, y1), self.at(x, y));
        self.path.quad_to(c, p);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        self.path.cubic_to(c1, c2, p);
    }

    fn close(&mut self) {
        self.path.close();
    }
}

/// `span`'s glyphs from `origin` on the baseline, and how far they advance.
fn shape(db: &fontdb::Database, span: &FrameTextSpan, origin: Point) -> (Path, f64) {
    let empty = (Path::new(), 0.0);
    if span.text.is_empty() || span.size.is_nan() || span.size <= 0.0 {
        return empty;
    }
    let Some(id) = face(db, span) else {
        return empty;
    };
    db.with_face_data(id, |data, index| {
        let face = rustybuzz::Face::from_slice(data, index)?;
        let k = span.size / f64::from(face.units_per_em());
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(&span.text);
        let shaped = rustybuzz::shape(&face, &[], buffer);
        let infos = shaped.glyph_infos();
        let mut path = Path::new();
        let mut x = 0.0;
        for (i, (info, pos)) in infos.iter().zip(shaped.glyph_positions()).enumerate() {
            let mut outline = Outline {
                path: &mut path,
                origin: Point::new(
                    origin.x + x + f64::from(pos.x_offset) * k,
                    origin.y - f64::from(pos.y_offset) * k,
                ),
                k,
            };
            face.outline_glyph(ttf_parser::GlyphId(info.glyph_id as u16), &mut outline);
            x += f64::from(pos.x_advance) * k;
            // After each character: the last glyph of its cluster.
            if infos
                .get(i + 1)
                .is_none_or(|next| next.cluster != info.cluster)
            {
                x += span.letter_spacing;
            }
        }
        Some((path, x))
    })
    .flatten()
    .unwrap_or(empty)
}

/// The outlines of `spans`, one path for each, in their coordinates: each
/// span after the one before, unless placed anew; each line (from one
/// placed span to the next) moved so its anchor is at its position.
pub fn layout(spans: &[FrameTextSpan]) -> Vec<Path> {
    let db = database();
    let mut out: Vec<Path> = Vec::with_capacity(spans.len());
    let mut pen = Point::new(0.0, 0.0);
    // The line being set: its first span, its anchor and where it starts.
    let mut line = (0, TextAnchor::Start, 0.0);
    for (i, span) in spans.iter().enumerate() {
        if let Some(p) = span.position {
            place(&mut out[line.0..], line.1, pen.x - line.2);
            pen = p;
            line = (i, span.anchor, pen.x + span.shift.x);
        } else if i == 0 {
            line = (0, span.anchor, span.shift.x);
        }
        pen = Point::new(pen.x + span.shift.x, pen.y + span.shift.y);
        let (path, advance) = shape(db, span, pen);
        out.push(path);
        pen.x += advance;
    }
    place(&mut out[line.0..], line.1, pen.x - line.2);
    out
}

/// Moves a line `width` long so `anchor` is where it started.
fn place(paths: &mut [Path], anchor: TextAnchor, width: f64) {
    let dx = match anchor {
        TextAnchor::Start => return,
        TextAnchor::Middle => -width / 2.0,
        TextAnchor::End => -width,
    };
    for p in paths {
        *p = p.transformed(Matrix::translate(dx, 0.0));
    }
}
