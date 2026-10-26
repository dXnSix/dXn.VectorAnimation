//! Rive's text: runs of styled text shaped with the file's fonts (HarfBuzz's
//! shaping, by rustybuzz), broken into lines and laid out as rive-runtime's
//! `Text` lays them out (`font_hb.cpp`, `renderer.cpp`'s word breaks,
//! `line_breaker.cpp`, `Text::buildRenderStyles`), into glyph outlines.
//! Left to right only: right-to-left runs are laid out left to right.

use dxn_vector_animation::{Matrix, Path, Point, Rect};
use rustybuzz::ttf_parser;

/// A text style: its font, size, line height (negative: the font's),
/// letter spacing and variation axes (tag, value).
#[derive(Debug, Clone)]
pub(crate) struct Style<'a> {
    pub font: &'a [u8],
    pub size: f64,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub axes: Vec<([u8; 4], f32)>,
}

/// A run of text in one style (an index into the styles).
#[derive(Debug, Clone)]
pub(crate) struct Run {
    pub text: String,
    pub style: usize,
}

/// How a text box sizes and places its lines (Rive's `Text` properties).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Params {
    /// 0 auto width, 1 auto height, 2 fixed.
    pub sizing: u64,
    pub width: f64,
    pub height: f64,
    /// 0 left, 1 right, 2 center.
    pub align: u64,
    /// 0 wraps, 1 does not.
    pub wrap: u64,
    /// 0 visible, 1 hidden, 2 clipped, 3 ellipsis, 4 fit.
    pub overflow: u64,
    pub origin_x: f64,
    pub origin_y: f64,
    pub paragraph_spacing: f64,
    /// The first baseline at 0 (Rive's baseline origin), not the top.
    pub baseline_origin: bool,
    /// 0 top, 1 bottom, 2 middle.
    pub vertical_align: u64,
    pub fit_from_baseline: bool,
}

/// Text laid out: each style's glyph outlines (in the order styles first
/// appear), in the text's space, and the rectangle a clipped text clips to;
/// and what text modifiers need: each glyph drawn, the text, each run's
/// span of it, the glyph each character is drawn by, and each line's span.
#[derive(Debug, Clone, Default)]
pub(crate) struct Laid {
    pub styles: Vec<(usize, Path)>,
    pub clip: Option<Rect>,
    pub glyphs: Vec<Glyph>,
    /// From where the glyphs are to the text's space (its fit and origin).
    pub fit: Matrix,
    pub chars: Vec<char>,
    /// Each run's characters (`None`: an empty run, left out).
    pub spans: Vec<Option<(usize, usize)>>,
    /// The glyph each character is drawn by, and one past the last.
    pub lookup: Vec<usize>,
    pub lines: Vec<(usize, usize)>,
}

/// A glyph drawn: its style, its outline at the font size about its
/// center (Rive's glyph before modifiers), where its center goes, and the
/// first character it draws.
#[derive(Debug, Clone)]
pub(crate) struct Glyph {
    pub style: usize,
    pub outline: Path,
    pub at: Point,
    pub text_index: usize,
}

impl Laid {
    /// How many characters the glyph drawing character `i` draws
    /// (`GlyphLookup::count`).
    pub fn count(&self, i: usize) -> usize {
        let Some(&g) = self.lookup.get(i) else {
            return 1;
        };
        1 + self.lookup[i + 1..].iter().take_while(|&&v| v == g).count()
    }
}

/// A font ready to shape: its face with the style's variations, and its
/// line metrics (ascent negative, descent positive, in ems).
struct Font<'a> {
    face: rustybuzz::Face<'a>,
    upem: f64,
    ascent: f64,
    descent: f64,
}

impl<'a> Font<'a> {
    fn new(style: &Style<'a>) -> Option<Font<'a>> {
        let mut face = rustybuzz::Face::from_slice(style.font, 0)?;
        for (tag, value) in &style.axes {
            face.set_variation(ttf_parser::Tag::from_bytes(tag), *value);
        }
        let upem = f64::from(face.units_per_em().max(1));
        let ascent = -f64::from(face.ascender()) / upem;
        let descent = -f64::from(face.descender()) / upem;
        Some(Font {
            face,
            upem,
            ascent,
            descent,
        })
    }
}

/// A run's shaped glyphs (Rive's `GlyphRun`): `xpos` holds each glyph's x in
/// its paragraph, and one more (the run's end).
struct GlyphRun {
    font: usize,
    style: usize,
    size: f64,
    line_height: f64,
    glyphs: Vec<u16>,
    text_indices: Vec<usize>,
    advances: Vec<f64>,
    xpos: Vec<f64>,
    offsets: Vec<(f64, f64)>,
    /// Word starts and ends, in pairs (glyph indices).
    breaks: Vec<usize>,
}

/// A line: from a run's glyph to another's (excluded), where it starts and
/// its top, baseline and bottom.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Line {
    start_run: usize,
    start_glyph: usize,
    end_run: usize,
    end_glyph: usize,
    start_x: f64,
    top: f64,
    baseline: f64,
    bottom: f64,
}

impl Line {
    fn at(run: usize, glyph: usize) -> Line {
        Line {
            start_run: run,
            start_glyph: glyph,
            end_run: run,
            end_glyph: glyph,
            ..Line::default()
        }
    }

    fn is_empty(&self) -> bool {
        self.start_run == self.end_run && self.start_glyph == self.end_glyph
    }
}

fn is_white_space(c: char) -> bool {
    c <= ' ' || c == '\u{2028}' || c == '\u{200B}'
}

fn is_paragraph_end(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{2029}' | '\u{85}' | '\u{1C}' | '\u{1D}' | '\u{1E}'
    )
}

/// Lays out `runs` in `styles` as `params` say.
pub(crate) fn lay_out(runs: &[Run], styles: &[Style<'_>], params: &Params) -> Laid {
    let fonts: Vec<Option<Font<'_>>> = styles.iter().map(Font::new).collect();
    // The text and each run's span of it (empty runs and runs without a
    // font are left out, as Rive's `makeStyled` leaves them).
    let mut text: Vec<char> = Vec::new();
    let mut spans: Vec<(usize, usize, usize)> = Vec::new();
    let mut run_spans = Vec::with_capacity(runs.len());
    for run in runs {
        if run.text.is_empty() || fonts.get(run.style).is_none_or(Option::is_none) {
            run_spans.push(None);
            continue;
        }
        let start = text.len();
        text.extend(run.text.chars());
        spans.push((start, text.len(), run.style));
        run_spans.push(Some((start, text.len())));
    }
    let paragraphs = shape(&text, &spans, styles, &fonts);
    if paragraphs.is_empty() {
        return Laid::default();
    }
    let width = if params.sizing == 0 {
        -1.0
    } else {
        params.width
    };
    let mut lines: Vec<Vec<Line>> = Vec::with_capacity(paragraphs.len());
    let mut paragraph_width = width;
    for runs in &paragraphs {
        let broken = break_lines(
            runs,
            if width < 0.0 || params.wrap == 1 {
                -1.0
            } else {
                width
            },
        );
        if width < 0.0 {
            paragraph_width = paragraph_width.max(max_width(&broken, runs));
        }
        lines.push(broken);
    }
    for (i, (runs, lines)) in paragraphs.iter().zip(&mut lines).enumerate() {
        line_spacing(i == 0, lines, runs, &fonts, paragraph_width, params.align);
    }
    let mut laid = render(&paragraphs, &lines, &fonts, params);
    // Rive's `GlyphLookup`: the characters up to a glyph's first are the
    // glyph before's.
    let mut lookup = vec![0; text.len() + 1];
    let (mut glyph, mut last) = (0usize, 0usize);
    for run in paragraphs.iter().flatten() {
        for &index in &run.text_indices {
            for slot in lookup.iter_mut().take(index.min(text.len())).skip(last) {
                *slot = glyph.saturating_sub(1);
            }
            last = index;
            glyph += 1;
        }
    }
    for slot in lookup.iter_mut().take(text.len()).skip(last) {
        *slot = glyph.saturating_sub(1);
    }
    lookup[text.len()] = if text.is_empty() {
        0
    } else {
        lookup[text.len() - 1] + 1
    };
    laid.lookup = lookup;
    laid.chars = text;
    laid.spans = run_spans;
    for (runs, lines) in paragraphs.iter().zip(&lines) {
        for line in lines {
            let (Some(from), Some(end)) = (
                runs.get(line.start_run)
                    .and_then(|r| r.text_indices.get(line.start_glyph)),
                runs.get(line.end_run)
                    .and_then(|r| r.text_indices.get(line.end_glyph.saturating_sub(1))),
            ) else {
                continue;
            };
            laid.lines.push((*from, *end + laid.count(*end)));
        }
    }
    laid
}

/// The paragraphs of `text` shaped, each its glyph runs, word breaks found.
fn shape(
    text: &[char],
    spans: &[(usize, usize, usize)],
    styles: &[Style<'_>],
    fonts: &[Option<Font<'_>>],
) -> Vec<Vec<GlyphRun>> {
    let mut paragraphs: Vec<Vec<GlyphRun>> = Vec::new();
    let mut runs: Vec<GlyphRun> = Vec::new();
    let mut close = |runs: &mut Vec<GlyphRun>| {
        // X positions from the advances, in logical order.
        let mut pos = 0.0;
        for run in runs.iter_mut() {
            for xp in &mut run.xpos {
                let advance = *xp;
                *xp = pos;
                pos += advance;
            }
        }
        if !runs.is_empty() {
            paragraphs.push(std::mem::take(runs));
        }
    };
    for &(start, end, style) in spans {
        let mut from = start;
        while from < end {
            // A paragraph ends after its separator.
            let mut to = from;
            while to < end && !is_paragraph_end(text[to]) {
                to += 1;
            }
            let ends_paragraph = to < end;
            if ends_paragraph {
                to += 1;
            }
            if let Some(run) = shape_run(text, from, to, style, styles, fonts) {
                runs.push(run);
            }
            if ends_paragraph {
                close(&mut runs);
            }
            from = to;
        }
    }
    close(&mut runs);
    find_breaks(text, &mut paragraphs);
    paragraphs
}

/// The glyphs of `text[from..to]` in `style` (Rive's `shape_run`).
fn shape_run(
    text: &[char],
    from: usize,
    to: usize,
    style: usize,
    styles: &[Style<'_>],
    fonts: &[Option<Font<'_>>],
) -> Option<GlyphRun> {
    let font = fonts.get(style)?.as_ref()?;
    let s = &styles[style];
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    // Clusters are char indices from `from`.
    for (i, c) in text[from..to].iter().enumerate() {
        buffer.add(*c, i as u32);
    }
    buffer.set_direction(rustybuzz::Direction::LeftToRight);
    buffer.guess_segment_properties();
    let shaped = rustybuzz::shape(&font.face, &[], buffer);
    let scale = s.size / font.upem;
    let infos = shaped.glyph_infos();
    let positions = shaped.glyph_positions();
    if infos.is_empty() {
        return None;
    }
    let mut run = GlyphRun {
        font: style,
        style,
        size: s.size,
        line_height: s.line_height,
        glyphs: Vec::with_capacity(infos.len()),
        text_indices: Vec::with_capacity(infos.len()),
        advances: Vec::with_capacity(infos.len()),
        xpos: Vec::with_capacity(infos.len() + 1),
        offsets: Vec::with_capacity(infos.len()),
        breaks: Vec::new(),
    };
    for (info, pos) in infos.iter().zip(positions) {
        let advance = f64::from(pos.x_advance) * scale + s.letter_spacing;
        run.glyphs.push(info.glyph_id as u16);
        run.text_indices.push(from + info.cluster as usize);
        run.advances.push(advance);
        run.xpos.push(advance);
        run.offsets.push((
            f64::from(pos.x_offset) * scale,
            -f64::from(pos.y_offset) * scale,
        ));
    }
    run.xpos.push(0.0);
    Some(run)
}

/// Word starts and ends of every run (Rive's `Font::shapeText`): a break
/// where whitespace starts or ends, and two at a forced line break.
fn find_breaks(text: &[char], paragraphs: &mut [Vec<GlyphRun>]) {
    let mut want_white_space = false;
    let mut breaks: Vec<usize> = Vec::new();
    let mut last: Option<(usize, usize)> = None;
    for p in 0..paragraphs.len() {
        for r in 0..paragraphs[p].len() {
            if let Some((lp, lr)) = last {
                paragraphs[lp][lr].breaks = std::mem::take(&mut breaks);
            }
            let run = &paragraphs[p][r];
            for (glyph, &offset) in run.text_indices.iter().enumerate() {
                let c = text[offset];
                if c == '\n' || c == '\u{2028}' {
                    breaks.push(glyph);
                    breaks.push(glyph);
                }
                if want_white_space == is_white_space(c) {
                    breaks.push(glyph);
                    want_white_space = !want_white_space;
                }
            }
            last = Some((p, r));
        }
    }
    if let Some((lp, lr)) = last {
        let count = paragraphs[lp][lr].glyphs.len();
        if want_white_space {
            breaks.push(count);
        } else {
            // The rest of the run.
            let back = breaks.last().copied().unwrap_or(0);
            breaks.push(back);
            breaks.push(count);
        }
        paragraphs[lp][lr].breaks = breaks;
    }
}

/// The widest line of `lines`.
fn max_width(lines: &[Line], runs: &[GlyphRun]) -> f64 {
    lines
        .iter()
        .map(|l| runs[l.end_run].xpos[l.end_glyph] - runs[l.start_run].xpos[l.start_glyph])
        .fold(0.0, f64::max)
}

/// A position in the breaks of a paragraph's runs (Rive's `WordMarker`).
#[derive(Clone, Copy)]
struct Marker {
    run: usize,
    index: isize,
}

impl Marker {
    fn next(&mut self, runs: &[GlyphRun]) -> bool {
        self.index += 2;
        while self.index >= runs[self.run].breaks.len() as isize {
            self.index -= runs[self.run].breaks.len() as isize;
            self.run += 1;
            if self.run >= runs.len() {
                return false;
            }
        }
        true
    }

    fn at(&self, runs: &[GlyphRun]) -> usize {
        runs[self.run].breaks[self.index as usize]
    }
}

/// A position in a paragraph's glyphs (Rive's `RunIterator`, without word
/// joiners).
#[derive(Clone, Copy, PartialEq)]
struct Cursor {
    run: usize,
    index: usize,
}

impl Cursor {
    fn back(&mut self, runs: &[GlyphRun]) -> bool {
        if self.index == 0 {
            if self.run == 0 {
                return false;
            }
            self.run -= 1;
            if runs[self.run].glyphs.is_empty() {
                self.index = 0;
                return self.back(runs);
            }
            self.index = runs[self.run].glyphs.len() - 1;
        } else {
            self.index -= 1;
        }
        true
    }

    fn forward(&mut self, runs: &[GlyphRun]) -> bool {
        if self.index == runs[self.run].glyphs.len() {
            if self.run + 1 >= runs.len() {
                return false;
            }
            self.run += 1;
            self.index = 0;
            if runs[self.run].glyphs.is_empty() {
                return self.forward(runs);
            }
        } else {
            self.index += 1;
        }
        true
    }

    fn x(&self, runs: &[GlyphRun]) -> f64 {
        runs[self.run].xpos[self.index]
    }
}

/// The lines of a paragraph wrapped at `width` (negative: not wrapped), as
/// Rive's `GlyphLine::BreakLines` breaks them.
fn break_lines(runs: &[GlyphRun], width: f64) -> Vec<Line> {
    let max_line_width = if width < 0.0 { f64::MAX } else { width };
    let mut lines = Vec::new();
    if runs.is_empty() {
        return lines;
    }
    let mut limit = max_line_width;
    let mut advance_word = false;
    let mut start = Marker { run: 0, index: -2 };
    let mut end = Marker { run: 0, index: -1 };
    if !start.next(runs) || !end.next(runs) {
        return lines;
    }
    let mut line = Line::default();
    let mut break_index = end.at(runs);
    let mut break_run = end.run;
    let mut last_end_glyph = end.index;
    let mut start_break_index = start.at(runs);
    let mut start_break_run = start.run;
    let mut x = runs[end.run].xpos[break_index];
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 1_000_000 {
            break;
        }
        if advance_word {
            last_end_glyph = end.index;
            if !start.next(runs) || !end.next(runs) {
                break;
            }
            advance_word = false;
            break_index = end.at(runs);
            break_run = end.run;
            start_break_index = start.at(runs);
            start_break_run = start.run;
            x = runs[end.run].xpos[break_index];
        }
        let forced = break_run == start_break_run && break_index == start_break_index;
        if !forced && x > limit {
            let start_run = start.run;
            if line.start_run == start_run && line.start_glyph == start_break_index {
                // A whole word overflowed: break it until it fits.
                let mut can_break_more = true;
                while can_break_more && x > limit {
                    let line_start = Cursor {
                        run: line.start_run,
                        index: line.start_glyph,
                    };
                    let mut line_end = Cursor {
                        run: end.run,
                        index: end.at(runs),
                    };
                    loop {
                        if !line_end.back(runs) {
                            can_break_more = false;
                            break;
                        } else if line_end.x(runs) <= limit {
                            if line_start == line_end && !line_end.forward(runs) {
                                can_break_more = false;
                            } else {
                                line.end_run = line_end.run;
                                line.end_glyph = line_end.index;
                            }
                            break;
                        }
                    }
                    if can_break_more {
                        limit = line_end.x(runs) + max_line_width;
                        if !line.is_empty() {
                            lines.push(line);
                        }
                        line = Line::at(line_end.run, line_end.index);
                    }
                }
                if !can_break_more {
                    // Nothing more to break: take the word whole.
                    line.end_run = end.run;
                    line.end_glyph = end.at(runs);
                    advance_word = true;
                }
            } else {
                // The word overflowed: it starts a new line.
                let start_x = runs[start.run].xpos[start.at(runs)];
                limit = start_x + max_line_width;
                if !line.is_empty() || start.index - last_end_glyph > 1 {
                    lines.push(line);
                }
                line = Line::at(start_run, start_break_index);
            }
        } else {
            line.end_run = end.run;
            line.end_glyph = end.at(runs);
            advance_word = true;
            if forced {
                lines.push(line);
                let start_x = runs[start.run].xpos[start.at(runs) + 1];
                limit = start_x + max_line_width;
                line = Line::at(start.run, start_break_index + 1);
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Each line's top, baseline, bottom and start (Rive's
/// `GlyphLine::ComputeLineSpacing`).
fn line_spacing(
    first_line: bool,
    lines: &mut [Line],
    runs: &[GlyphRun],
    fonts: &[Option<Font<'_>>],
    width: f64,
    align: u64,
) {
    let mut first = first_line;
    let mut y = 0.0;
    for line in lines.iter_mut() {
        let (mut asc, mut real_ascent, mut des) = (0.0f64, 0.0f64, 0.0f64);
        for run in &runs[line.start_run..=line.end_run] {
            let Some(font) = fonts.get(run.font).and_then(Option::as_ref) else {
                continue;
            };
            let (a, d) = if run.line_height < 0.0 {
                (font.ascent * run.size, font.descent * run.size)
            } else {
                let baseline = -font.ascent;
                let height = baseline + font.descent;
                let actual = -(baseline / height) * run.line_height;
                (actual, run.line_height + actual)
            };
            real_ascent = real_ascent.min(font.ascent * run.size);
            asc = asc.min(a);
            des = des.max(d);
        }
        line.top = y;
        if first {
            y = -real_ascent;
            first = false;
        } else {
            y -= asc;
        }
        line.baseline = y;
        y += des;
        line.bottom = y;
        let line_width =
            runs[line.end_run].xpos[line.end_glyph] - runs[line.start_run].xpos[line.start_glyph];
        line.start_x = match align {
            1 => width - line_width,
            2 => width / 2.0 - line_width / 2.0,
            _ => 0.0,
        };
    }
}

/// The glyph outlines placed (Rive's `Text::buildRenderStyles`, without
/// modifiers and ellipses).
fn render(
    paragraphs: &[Vec<GlyphRun>],
    lines: &[Vec<Line>],
    fonts: &[Option<Font<'_>>],
    params: &Params,
) -> Laid {
    let fixed = params.sizing == 2;
    // Bounds.
    let mut y = 0.0;
    let mut min_y = 0.0;
    let mut max_width: f64 = 0.0;
    if params.baseline_origin
        && let Some(first) = lines.first().and_then(|l| l.first())
    {
        y -= first.baseline;
        min_y = y;
    }
    for (runs, lines) in paragraphs.iter().zip(lines) {
        for line in lines {
            let width = runs[line.end_run].xpos[line.end_glyph]
                - runs[line.start_run].xpos[line.start_glyph];
            max_width = max_width.max(width);
        }
        if let Some(last) = lines.last() {
            y += last.bottom;
        }
        y += params.paragraph_spacing;
    }
    let total_height = y;
    let (bx, by, bw, bh) = match params.sizing {
        0 => (
            0.0,
            min_y,
            max_width,
            min_y.max(total_height - params.paragraph_spacing) - min_y,
        ),
        1 => (
            0.0,
            min_y,
            params.width,
            min_y.max(total_height - params.paragraph_spacing) - min_y,
        ),
        _ => (0.0, min_y, params.width, params.height),
    };
    let vertical_offset = match params.vertical_align {
        2 => (total_height - bh) / 2.0,
        1 => total_height - bh,
        _ => 0.0,
    };
    let clip = (params.overflow == 2).then_some(Rect {
        x: bx,
        y: by + vertical_offset,
        width: bw,
        height: bh,
    });
    let mut styles: Vec<(usize, Path)> = Vec::new();
    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut cur_y = min_y;
    let mut min_x = f64::MAX;
    'lines: for (runs, lines) in paragraphs.iter().zip(lines) {
        for line in lines {
            // Lines hidden or clipped past a fixed box.
            if fixed && (params.overflow == 1 || params.overflow == 2) {
                let hidden = params.overflow == 1;
                let (near, far) = if hidden {
                    (line.top, line.bottom)
                } else {
                    (line.bottom, line.top)
                };
                match params.vertical_align {
                    0 if cur_y + far > params.height => break 'lines,
                    2 if cur_y + near < total_height / 2.0 - params.height / 2.0 => continue,
                    2 if cur_y + far > total_height / 2.0 + params.height / 2.0 => break 'lines,
                    1 if cur_y + near < total_height - params.height => continue,
                    _ => {}
                }
            }
            let render_y = cur_y + line.baseline;
            let mut cur_x = line.start_x;
            min_x = min_x.min(cur_x);
            for (r, run) in runs
                .iter()
                .enumerate()
                .take(line.end_run + 1)
                .skip(line.start_run)
            {
                let from = if r == line.start_run {
                    line.start_glyph
                } else {
                    0
                };
                let to = if r == line.end_run {
                    line.end_glyph
                } else {
                    run.glyphs.len()
                };
                let Some(font) = fonts.get(run.font).and_then(Option::as_ref) else {
                    continue;
                };
                for g in from..to {
                    let (ox, oy) = run.offsets[g];
                    let center = run.advances[g] / 2.0;
                    let em = glyph_path(font, run.glyphs[g]);
                    let at = Matrix::new(run.size, 0.0, 0.0, run.size, cur_x + ox, render_y + oy);
                    let outline = em.transformed(at);
                    match styles.iter_mut().find(|(s, _)| *s == run.style) {
                        Some((_, path)) => path.extend(&outline),
                        None => styles.push((run.style, outline)),
                    }
                    glyphs.push(Glyph {
                        style: run.style,
                        outline: em
                            .transformed(Matrix::new(run.size, 0.0, 0.0, run.size, -center, 0.0)),
                        at: Point::new(cur_x + center + ox, render_y + oy),
                        text_index: run.text_indices[g],
                    });
                    cur_x += run.advances[g];
                }
            }
        }
        if let Some(last) = lines.last() {
            cur_y += last.bottom;
        }
        cur_y += params.paragraph_spacing;
    }
    // Fit, then the origin and vertical alignment.
    let mut scale = 1.0;
    let mut x_offset = -bw * params.origin_x;
    let mut y_offset = -bh * params.origin_y;
    if params.overflow == 4 {
        let x_scale = if params.sizing != 0 && max_width > bw {
            bw / max_width
        } else {
            1.0
        };
        let baseline = if params.fit_from_baseline {
            lines
                .first()
                .and_then(|l| l.first())
                .map_or(0.0, |l| l.baseline)
        } else {
            0.0
        };
        let y_scale = if fixed && total_height > bh {
            (bh - baseline) / (total_height - baseline)
        } else {
            1.0
        };
        if x_scale != 1.0 || y_scale != 1.0 {
            scale = x_scale.min(y_scale).max(0.0);
            y_offset += baseline * (1.0 - scale);
            let min_x = if min_x == f64::MAX { 0.0 } else { min_x };
            match params.align {
                2 => x_offset += (bw - max_width * scale) / 2.0 - min_x * scale,
                1 => x_offset += bw - max_width * scale - min_x * scale,
                _ => {}
            }
        }
    }
    if params.vertical_align != 0 && fixed {
        y_offset = -bh * params.origin_y;
        if params.vertical_align == 2 {
            y_offset += (bh - total_height * scale) / 2.0;
        } else {
            y_offset += bh - total_height * scale;
        }
    }
    let m = Matrix::new(scale, 0.0, 0.0, scale, x_offset, y_offset);
    Laid {
        glyphs,
        fit: m,
        styles: styles
            .into_iter()
            .map(|(s, p)| (s, p.transformed(m)))
            .collect(),
        clip: clip.map(|c| {
            let a = m.apply(Point::new(c.x, c.y));
            Rect {
                x: a.x,
                y: a.y,
                width: c.width * scale,
                height: c.height * scale,
            }
        }),
        ..Laid::default()
    }
}

/// Glyph `id`'s outline in ems, y down.
fn glyph_path(font: &Font<'_>, id: u16) -> Path {
    struct Builder {
        path: Path,
        scale: f64,
    }
    impl Builder {
        fn p(&self, x: f32, y: f32) -> Point {
            Point::new(f64::from(x) * self.scale, -f64::from(y) * self.scale)
        }
    }
    impl ttf_parser::OutlineBuilder for Builder {
        fn move_to(&mut self, x: f32, y: f32) {
            let p = self.p(x, y);
            self.path.move_to(p);
        }
        fn line_to(&mut self, x: f32, y: f32) {
            let p = self.p(x, y);
            self.path.line_to(p);
        }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            let (c, p) = (self.p(x1, y1), self.p(x, y));
            self.path.quad_to(c, p);
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            let (c1, c2, p) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
            self.path.cubic_to(c1, c2, p);
        }
        fn close(&mut self) {
            self.path.close();
        }
    }
    let mut builder = Builder {
        path: Path::default(),
        scale: 1.0 / font.upem,
    };
    font.face
        .outline_glyph(ttf_parser::GlyphId(id), &mut builder);
    builder.path
}
