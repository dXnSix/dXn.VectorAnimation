//! Text layers drawn with the file's glyphs (`chars`), laid out and
//! animated as lottie-web lays out and animates text: glyphs at a
//! hundredth of the size advanced by their widths and the tracking, lines
//! justified, box text wrapped at spaces, keyframed documents in turn;
//! text animators (range selectors over characters, words or lines, and
//! the properties they change) and text laid along a mask's path.
//!
//! Text without animators or a path is one shape of all its glyphs. With
//! them each character is a group of its own: its transform, opacity and
//! colors are constants where what selects and changes it is, and where
//! that moves they are sampled at the file's frame rate (straight between
//! frames), as lottie-web computes them frame by frame.

use std::f64::consts::PI;

use dxn_vector_animation::{
    Animated, Color, Easing, Error, Fill, FillRule, Group, Keyframe, LineCap, LineJoin, Node,
    Paint, PaintOrder, Path, PathEl, Point, Shape, ShapePath, Stroke, TimeRange, Track,
    TransformOp,
};
use kurbo::{ParamCurve, ParamCurveArclen, ParamCurveDeriv};
use serde_json::Value;

use super::{Converter, frame_times, simplify};
use crate::props::{color, has_expression, num, nums, scalar, shape};

/// The most letters × animators × sampled times one text document is
/// baked at; documents beyond it are sampled less often.
const MAX_WORK: usize = 4_000_000;

/// How exactly lengths along a text path are measured.
const ACCURACY: f64 = 1e-4;

const EXPRESSIONS: &str = "expressions are left out: their properties keep their keyframes";

impl<'a> Converter<'a> {
    /// A text layer (`t`): each keyframed document shown until the next,
    /// with the layer's animators and path.
    pub(super) fn text(&mut self, layer: &'a Value) -> Result<Vec<Node>, Error> {
        let fr = self.fr;
        if self.chars.is_empty() {
            self.warn("text layers without glyphs are left out: export them with glyphs (chars)");
            return Ok(Vec::new());
        }
        let t = layer.get("t");
        let more = t.and_then(|t| t.get("m"));
        let alignment = more.and_then(|m| m.get("a"));
        let animation = Animation {
            animators: self.animators(t.and_then(|t| t.get("a"))),
            grouping: more.and_then(|m| m.get("g")).and_then(num).unwrap_or(1.0) as i64,
            alignment: [
                scalar(alignment, 0, 1.0, 0.0, fr),
                scalar(alignment, 1, 1.0, 0.0, fr),
            ],
            path: self.text_path(layer, t.and_then(|t| t.get("p"))),
        };
        let docs: Vec<(f64, &'a Value)> = t
            .and_then(|t| t.get("d"))
            .and_then(|d| d.get("k"))
            .and_then(Value::as_array)
            .map(|k| {
                k.iter()
                    .filter_map(|k| {
                        Some((k.get("t").and_then(num).unwrap_or(0.0) / fr, k.get("s")?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut out = Vec::new();
        for (i, (start, doc)) in docs.iter().enumerate() {
            let shown = (
                if i == 0 { f64::NEG_INFINITY } else { *start },
                docs.get(i + 1).map_or(f64::INFINITY, |d| d.0),
            );
            let Some(nodes) = self.document(doc, &animation, shown)? else {
                continue;
            };
            out.push(Node::Group(Group {
                active: (docs.len() > 1).then_some(TimeRange {
                    start: shown.0,
                    end: shown.1,
                }),
                children: nodes,
                ..Group::new()
            }));
        }
        Ok(out)
    }

    /// The animators (`t.a`) that change something, each with its
    /// selector.
    fn animators(&mut self, list: Option<&'a Value>) -> Vec<Animator> {
        let fr = self.fr;
        let mut out = Vec::new();
        let list = list
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        for (n, animator) in list.iter().enumerate() {
            let props = animator.get("a");
            let get = |key: &str| props.and_then(|p| p.get(key));
            const KEYS: [&str; 16] = [
                "a", "p", "s", "sk", "sa", "r", "rx", "ry", "o", "sw", "sc", "fc", "fh", "fs",
                "fb", "t",
            ];
            if KEYS.iter().any(|k| get(k).is_some_and(has_expression)) {
                self.warn(EXPRESSIONS);
            }
            if get("rx").is_some() || get("ry").is_some() {
                self.warn("3D rotations are left out");
            }
            let Some(selector) = self.selector(animator.get("s"), n) else {
                continue;
            };
            let pair = |key: &str, scale: f64, default: f64| {
                get(key).map(|p| {
                    [
                        scalar(Some(p), 0, scale, default, fr),
                        scalar(Some(p), 1, scale, default, fr),
                    ]
                })
            };
            let one = |key: &str, scale: f64| get(key).map(|p| scalar(Some(p), 0, scale, 0.0, fr));
            let animator = Animator {
                selector,
                anchor: pair("a", 1.0, 0.0),
                position: pair("p", 1.0, 0.0),
                scale: pair("s", 0.01, 1.0),
                skew: get("sk").map(|sk| {
                    (
                        scalar(Some(sk), 0, 1.0, 0.0, fr),
                        scalar(get("sa"), 0, 1.0, 0.0, fr),
                    )
                }),
                rotation: one("r", 1.0),
                opacity: get("o").map(|o| scalar(Some(o), 0, 0.01, 1.0, fr)),
                stroke_width: one("sw", 1.0),
                stroke_color: get("sc").map(|c| color(Some(c), fr)),
                fill_color: get("fc").map(|c| color(Some(c), fr)),
                hue: one("fh", 1.0),
                saturation: one("fs", 0.01),
                brightness: one("fb", 0.01),
                tracking: one("t", 1.0),
            };
            if animator.changes_something() {
                out.push(animator);
            }
        }
        out
    }

    /// An animator's selector (`s`, one or a list): its range selector, or
    /// the expression selector After Effects writes by default
    /// (`selectorValue * textIndex / textTotal`). `None`, with a warning,
    /// when it has neither; without any selector the animator changes
    /// every character fully.
    fn selector(&mut self, s: Option<&'a Value>, n: usize) -> Option<Selector> {
        static ALL: Value = Value::Null;
        let list: Vec<&'a Value> = match s {
            None => Vec::new(),
            Some(Value::Array(a)) => a.iter().collect(),
            Some(v) => vec![v],
        };
        let mut chosen: Vec<(&'a Value, bool)> = Vec::new();
        for selector in &list {
            match selector.get("t").and_then(num).unwrap_or(0.0) as i64 {
                0 => chosen.push((selector, false)),
                1 if index_over_total(selector) => chosen.push((selector, true)),
                1 => self.warn(
                    "text expression selectors are left out, and animators with no other selector",
                ),
                _ => self.warn(
                    "wiggly text selectors are left out, and animators with no other selector",
                ),
            }
        }
        if chosen.len() > 1 {
            self.warn("text animators with several selectors keep the first");
        }
        let (selector, by_index) = match chosen.first() {
            Some(c) => *c,
            None if list.is_empty() => (&ALL, false),
            None => return None,
        };
        let get = |key: &str| selector.get(key);
        let keys = ["s", "e", "o", "xe", "ne", "sm"];
        if keys.iter().any(|k| get(k).is_some_and(has_expression))
            || (!by_index && get("a").is_some_and(has_expression))
        {
            self.warn(EXPRESSIONS);
        }
        let fr = self.fr;
        Some(Selector {
            by_index,
            start: scalar(get("s"), 0, 1.0, 0.0, fr),
            end: get("e").map(|e| scalar(Some(e), 0, 1.0, 100.0, fr)),
            offset: scalar(get("o"), 0, 1.0, 0.0, fr),
            amount: scalar(get("a"), 0, 0.01, 1.0, fr),
            ease_high: scalar(get("xe"), 0, 1.0, 0.0, fr),
            ease_low: scalar(get("ne"), 0, 1.0, 0.0, fr),
            smoothness: scalar(get("sm"), 0, 1.0, 100.0, fr),
            index_units: get("r").and_then(num) == Some(2.0),
            based: get("b").and_then(num).unwrap_or(1.0) as i64,
            shape: get("sh").and_then(num).unwrap_or(1.0) as i64,
            randomize: get("rn").and_then(num).is_some_and(|r| r != 0.0),
            seed: 0x7E57_5EED ^ n as u64,
        })
    }

    /// The path text is laid along (`t.p`): the outline of the layer's
    /// mask `m`, and the path options.
    fn text_path(&mut self, layer: &'a Value, p: Option<&'a Value>) -> Option<TextPath> {
        let p = p?;
        let index = p.get("m")?;
        let fr = self.fr;
        let outline = num(index)
            .filter(|i| *i >= 0.0)
            .and_then(|i| layer.get("masksProperties")?.as_array()?.get(i as usize))
            .and_then(|mask| shape(mask.get("pt"), fr));
        let Some(outline) = outline else {
            self.warn("text on a path whose mask is missing is laid out on a line");
            return None;
        };
        if ["f", "l", "r", "p", "a"]
            .iter()
            .any(|k| p.get(k).is_some_and(has_expression))
        {
            self.warn(EXPRESSIONS);
        }
        Some(TextPath {
            outline,
            first_margin: scalar(p.get("f"), 0, 1.0, 0.0, fr),
            last_margin: scalar(p.get("l"), 0, 1.0, 0.0, fr),
            reverse: scalar(p.get("r"), 0, 1.0, 0.0, fr),
            perpendicular: scalar(p.get("p"), 0, 1.0, 1.0, fr),
            force: scalar(p.get("a"), 0, 1.0, 0.0, fr),
        })
    }

    /// One text document (`s`) shown during `shown`: its glyphs filled and
    /// stroked, as one shape, or a group a character when animated.
    fn document(
        &mut self,
        doc: &'a Value,
        animation: &Animation,
        shown: (f64, f64),
    ) -> Result<Option<Vec<Node>>, Error> {
        let layout = self.layout(doc, animation.grouping);
        if animation.animators.is_empty() && animation.path.is_none() {
            return self.still_text(&layout);
        }
        self.animated_text(&layout, animation, shown)
    }

    /// The letters of document `doc` where lottie-web lays them out
    /// without animators.
    fn layout(&mut self, doc: &'a Value, grouping: i64) -> Layout {
        let text = doc.get("t").and_then(Value::as_str).unwrap_or("");
        let size = doc.get("s").and_then(num).unwrap_or(10.0);
        let font = doc.get("f").and_then(Value::as_str).unwrap_or("");
        let justify = doc.get("j").and_then(num).unwrap_or(0.0) as i64;
        let tracking = doc.get("tr").and_then(num).unwrap_or(0.0) / 1000.0 * size;
        let line_height = doc.get("lh").and_then(num).unwrap_or(size * 1.2);
        let shift = doc.get("ls").and_then(num).unwrap_or(0.0);
        let k = size / 100.0;
        let entry = self
            .fonts
            .iter()
            .find(|f| f.get("fName").and_then(Value::as_str) == Some(font))
            .copied();
        let family = entry
            .and_then(|f| f.get("fFamily").and_then(Value::as_str))
            .unwrap_or(font);
        let style = entry
            .and_then(|f| f.get("fStyle").and_then(Value::as_str))
            .unwrap_or("");
        let ascent = entry
            .and_then(|f| f.get("ascent").and_then(num))
            .unwrap_or(75.0);
        let chars = self.chars.clone();
        // The glyph of `ch`: its index among the chars and its advance.
        let find = |ch: char| -> Option<(usize, f64)> {
            let ch = ch.to_string();
            let fits = |c: &Value, strict: bool| {
                c.get("ch").and_then(Value::as_str) == Some(ch.as_str())
                    && (!strict
                        || (c.get("fFamily").and_then(Value::as_str) == Some(family)
                            && c.get("style").and_then(Value::as_str) == Some(style)))
            };
            let i = chars
                .iter()
                .position(|c| fits(c, true))
                .or_else(|| chars.iter().position(|c| fits(c, false)))?;
            Some((i, chars[i].get("w").and_then(num).unwrap_or(0.0)))
        };
        let advance = |c: char| find(c).map_or(0.0, |(_, w)| w * k) + tracking;
        let measure = |s: &str| s.chars().map(advance).sum::<f64>();
        // Lines: breaks, and in a box, wraps at spaces.
        let normalized = text.replace("\r\n", "\r").replace(['\n', '\u{3}'], "\r");
        let box_size = doc
            .get("sz")
            .and_then(nums)
            .filter(|s| s.len() >= 2 && s[0] > 0.0);
        let box_at = doc.get("ps").and_then(nums).filter(|p| p.len() >= 2);
        let mut lines: Vec<String> = Vec::new();
        for paragraph in normalized.split('\r') {
            let Some(sz) = &box_size else {
                lines.push(paragraph.to_owned());
                continue;
            };
            let mut line = String::new();
            for word in paragraph.split_inclusive(' ') {
                if !line.is_empty() && measure(&line) + measure(word.trim_end()) > sz[0] {
                    lines.push(std::mem::take(&mut line).trim_end().to_owned());
                }
                line.push_str(word);
            }
            lines.push(line.trim_end().to_owned());
        }
        let (x0, y0, box_width) = match (&box_at, &box_size) {
            (Some(ps), Some(sz)) => (ps[0], ps[1] + ascent * k, sz[0]),
            _ => (0.0, 0.0, 0.0),
        };
        let mut letters = Vec::new();
        let mut line_widths = Vec::new();
        for (row, line) in lines.iter().enumerate() {
            if row > 0 {
                letters.push(Letter::line_break(row));
            }
            let width = measure(line) - if line.is_empty() { 0.0 } else { tracking };
            line_widths.push(width);
            // Left, right or centered: on the anchor, or within the box.
            let start = if box_width > 0.0 {
                x0 + match justify {
                    1 => box_width - width,
                    2 => (box_width - width) / 2.0,
                    _ => 0.0,
                }
            } else {
                match justify {
                    1 => -width,
                    2 => -width / 2.0,
                    _ => 0.0,
                }
            };
            let y = y0 + row as f64 * line_height - shift;
            let mut pen = 0.0;
            for c in line.chars() {
                let found = find(c);
                if found.is_none() && !c.is_whitespace() {
                    self.warn(format!("text has characters with no glyph (`{c}`)"));
                }
                let advance = found.map_or(0.0, |(_, w)| w * k);
                letters.push(Letter {
                    glyph: found.map(|(i, _)| i),
                    advance,
                    newline: false,
                    space: c == ' ',
                    line: row,
                    x: start + pen,
                    y,
                    pen,
                    group_width: advance,
                    group_offset: 0.0,
                    group: 0,
                    extra: 0.0,
                });
                pen += advance + tracking;
            }
        }
        group(&mut letters, grouping);
        let color = |key: &str| -> Option<Color> {
            let c = doc.get(key).and_then(nums)?;
            let s = if c.iter().take(3).any(|v| *v > 1.0) {
                255.0
            } else {
                1.0
            };
            Some(Color::rgba(
                *c.first()? / s,
                *c.get(1)? / s,
                *c.get(2)? / s,
                1.0,
            ))
        };
        Layout {
            letters,
            line_widths,
            size,
            k,
            tracking,
            line_height,
            justify,
            shift,
            fill: color("fc"),
            stroke: color("sc"),
            stroke_width: doc.get("sw").and_then(num).unwrap_or(0.0),
            // `of`: the stroke over the fill.
            stroke_over: doc.get("of").and_then(Value::as_bool).unwrap_or(true),
        }
    }

    /// Text without animators: one shape of all its glyphs.
    fn still_text(&mut self, layout: &Layout) -> Result<Option<Vec<Node>>, Error> {
        let k = layout.k;
        let mut paths = Vec::new();
        for letter in &layout.letters {
            let Some(index) = letter.glyph else {
                continue;
            };
            for glyph in self.glyph(index)? {
                let mut transform = vec![
                    TransformOp::translate(letter.x, letter.y),
                    TransformOp::scale(k, k),
                ];
                transform.extend(glyph.transform);
                paths.push(ShapePath {
                    geometry: glyph.geometry,
                    transform,
                });
            }
        }
        if paths.is_empty() {
            return Ok(None);
        }
        self.count_nodes(paths.len())?;
        let fill = layout.fill.map(Animated::Static);
        let stroke = layout
            .stroke
            .filter(|_| layout.stroke_width > 0.0)
            .map(|c| (Animated::Static(c), Animated::Static(layout.stroke_width)));
        Ok(Some(vec![Node::shape(layout.shape(paths, fill, stroke))]))
    }

    /// Animated text (or text on a path): a group a character, each with
    /// its transform, opacity and colors at every sampled time.
    fn animated_text(
        &mut self,
        layout: &Layout,
        animation: &Animation,
        shown: (f64, f64),
    ) -> Result<Option<Vec<Node>>, Error> {
        let k = layout.k;
        // What each letter draws: its glyph at the size.
        let mut drawn: Vec<(usize, Vec<ShapePath>)> = Vec::new();
        let mut count = 0;
        for (i, letter) in layout.letters.iter().enumerate() {
            let Some(index) = letter.glyph else {
                continue;
            };
            let paths: Vec<ShapePath> = self
                .glyph(index)?
                .into_iter()
                .map(|glyph| {
                    let mut transform = vec![TransformOp::scale(k, k)];
                    transform.extend(glyph.transform);
                    ShapePath {
                        geometry: glyph.geometry,
                        transform,
                    }
                })
                .collect();
            if paths.is_empty() {
                continue;
            }
            count += paths.len() + 2;
            drawn.push((i, paths));
        }
        if drawn.is_empty() {
            return Ok(None);
        }
        self.count_nodes(count)?;
        let animators = &animation.animators;
        // A moving path is measured again at every sampled time.
        let path_work = animation.path.as_ref().map_or(0, |p| {
            p.outline
                .keyframes()
                .iter()
                .map(|k| k.value.0.len())
                .max()
                .unwrap_or(0)
        });
        let work = layout.letters.len().max(1) * animators.len().max(1) + path_work;
        if work > MAX_WORK / 2 {
            return Err(Error::Limit {
                what: "text animator samples",
                limit: MAX_WORK as f64,
            });
        }
        let indexes: Vec<(Vec<f64>, f64)> = animators
            .iter()
            .map(|a| indexes(&layout.letters, &a.selector))
            .collect();
        let keys = animation.key_times();
        let budget = (self.limits.max_keyframes / 2).saturating_sub(self.text_keyframes);
        let letters: Vec<usize> = drawn.iter().map(|d| d.0).collect();
        let slots = slots(animators);
        let on_path = animation.path.is_some();
        let stroke_color_moves = animators.iter().any(|a| a.stroke_color.is_some());
        let stroke_width_moves = animators.iter().any(|a| a.stroke_width.is_some());
        let stroked = (layout.stroke.is_some() || stroke_color_moves)
            && (layout.stroke_width > 0.0 || stroke_width_moves);
        let filled = layout.fill.is_some();
        let mut most = MAX_WORK / work;
        let mut cut = false;
        let (channels, keyframes) = loop {
            let (times, fewer) = sample_times(&keys, shown, self.fr, most);
            cut |= fewer;
            let channels = bake(layout, animation, &indexes, &times, &letters);
            let keyframes: usize = channels
                .iter()
                .map(|c| c.keyframes(on_path, filled, stroked))
                .sum();
            if keyframes <= budget {
                break (channels, keyframes);
            }
            if times.len() <= 2 {
                return Err(Error::Limit {
                    what: "keyframes",
                    limit: self.limits.max_keyframes as f64,
                });
            }
            cut = true;
            most = (times.len() * budget / keyframes)
                .min(times.len() / 2)
                .max(2);
        };
        if cut {
            self.warn(
                "text animators are sampled less often than every frame to stay within the limits",
            );
        }
        self.text_keyframes += keyframes;
        let mut out = Vec::with_capacity(drawn.len());
        for ((_, paths), ch) in drawn.into_iter().zip(channels) {
            let fill = layout.fill.map(|_| ch.fill.color());
            let stroke = stroked.then(|| (ch.stroke.color(), ch.stroke_width.scalar()));
            out.push(Node::Group(Group {
                transform: ch.transform(&slots, on_path),
                opacity: ch.opacity.scalar(),
                children: vec![Node::shape(layout.shape(paths, fill, stroke))],
                ..Group::new()
            }));
        }
        Ok(Some(out))
    }

    /// Counts `count` more nodes against the limit.
    fn count_nodes(&mut self, count: usize) -> Result<(), Error> {
        self.nodes += count;
        if self.nodes > self.limits.max_nodes {
            return Err(Error::Limit {
                what: "nodes",
                limit: self.limits.max_nodes as f64,
            });
        }
        Ok(())
    }

    /// The outlines of glyph `index` of `chars`, at size 100.
    fn glyph(&mut self, index: usize) -> Result<Vec<ShapePath>, Error> {
        if let Some(paths) = self.glyphs.get(&index) {
            return Ok(paths.clone());
        }
        let Some(char) = self.chars.get(index).copied() else {
            return Ok(Vec::new());
        };
        let items = char
            .get("data")
            .and_then(|d| d.get("shapes"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let (_, outlines) = self.shapes(items)?;
        let paths: Vec<ShapePath> = outlines.into_iter().map(|o| o.path).collect();
        self.glyphs.insert(index, paths.clone());
        Ok(paths)
    }
}

/// Whether expression selector `selector` holds the expression After
/// Effects writes by default, `selectorValue * textIndex / textTotal`
/// (as Bodymovin exports it too).
fn index_over_total(selector: &Value) -> bool {
    let Some(x) = selector
        .get("a")
        .and_then(|a| a.get("x"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    let mut e: String = x.chars().filter(|c| !c.is_whitespace()).collect();
    for prefix in ["var$bm_rt;", "$bm_rt="] {
        if let Some(rest) = e.strip_prefix(prefix) {
            e = rest.to_owned();
        }
    }
    matches!(
        e.trim_end_matches(';'),
        "selectorValue*textIndex/textTotal"
            | "mul(selectorValue,div(textIndex,textTotal))"
            | "$bm_mul(selectorValue,$bm_div(textIndex,textTotal))"
    )
}

/// What animates a text layer's characters.
struct Animation {
    animators: Vec<Animator>,
    /// How characters group about an anchor (`m.g`): 1 characters, 2
    /// words, 3 lines.
    grouping: i64,
    /// Where in its group a character's anchor is (`m.a`): percent of the
    /// group's width and of the line's height.
    alignment: [Animated<f64>; 2],
    path: Option<TextPath>,
}

impl Animation {
    /// The times anything of it has keyframes at.
    fn key_times(&self) -> Vec<f64> {
        let mut out = Vec::new();
        for animator in &self.animators {
            animator.key_times(&mut out);
        }
        for a in &self.alignment {
            times(a, &mut out);
        }
        if let Some(path) = &self.path {
            times(&path.outline, &mut out);
            for a in [
                &path.first_margin,
                &path.last_margin,
                &path.reverse,
                &path.perpendicular,
                &path.force,
            ] {
                times(a, &mut out);
            }
        }
        out
    }
}

/// The path text is laid along, and how.
struct TextPath {
    outline: Animated<Path>,
    /// Where along it the text starts.
    first_margin: Animated<f64>,
    /// With force alignment, where the text ends, from the path's end.
    last_margin: Animated<f64>,
    /// Non-zero: along the path backwards.
    reverse: Animated<f64>,
    /// Non-zero: characters turn with the path.
    perpendicular: Animated<f64>,
    /// Non-zero: characters spread from one margin to the other.
    force: Animated<f64>,
}

/// One text animator: how much its selector selects each character, and
/// what it changes by that much.
struct Animator {
    selector: Selector,
    anchor: Option<[Animated<f64>; 2]>,
    position: Option<[Animated<f64>; 2]>,
    /// Fractions.
    scale: Option<[Animated<f64>; 2]>,
    /// Skew and skew axis, degrees.
    skew: Option<(Animated<f64>, Animated<f64>)>,
    /// Degrees, clockwise.
    rotation: Option<Animated<f64>>,
    /// 0 to 1.
    opacity: Option<Animated<f64>>,
    stroke_width: Option<Animated<f64>>,
    stroke_color: Option<Animated<Color>>,
    fill_color: Option<Animated<Color>>,
    /// Degrees.
    hue: Option<Animated<f64>>,
    /// Fractions.
    saturation: Option<Animated<f64>>,
    brightness: Option<Animated<f64>>,
    /// Pixels.
    tracking: Option<Animated<f64>>,
}

impl Animator {
    fn changes_something(&self) -> bool {
        self.anchor.is_some()
            || self.position.is_some()
            || self.scale.is_some()
            || self.skew.is_some()
            || self.rotation.is_some()
            || self.opacity.is_some()
            || self.stroke_width.is_some()
            || self.stroke_color.is_some()
            || self.fill_color.is_some()
            || self.hue.is_some()
            || self.saturation.is_some()
            || self.brightness.is_some()
            || self.tracking.is_some()
    }

    fn key_times(&self, out: &mut Vec<f64>) {
        let s = &self.selector;
        for a in [
            &s.start,
            &s.offset,
            &s.amount,
            &s.ease_high,
            &s.ease_low,
            &s.smoothness,
        ] {
            times(a, out);
        }
        let scalars = [
            s.end.as_ref(),
            self.skew.as_ref().map(|s| &s.0),
            self.skew.as_ref().map(|s| &s.1),
            self.rotation.as_ref(),
            self.opacity.as_ref(),
            self.stroke_width.as_ref(),
            self.hue.as_ref(),
            self.saturation.as_ref(),
            self.brightness.as_ref(),
            self.tracking.as_ref(),
        ];
        for a in scalars.into_iter().flatten() {
            times(a, out);
        }
        for pair in [&self.anchor, &self.position, &self.scale]
            .into_iter()
            .flatten()
        {
            times(&pair[0], out);
            times(&pair[1], out);
        }
        for c in [&self.stroke_color, &self.fill_color].into_iter().flatten() {
            times(c, out);
        }
    }

    /// Its values at time `t`.
    fn at(&self, t: f64) -> Values {
        let pair =
            |p: &Option<[Animated<f64>; 2]>| p.as_ref().map(|p| [p[0].sample(t), p[1].sample(t)]);
        let one = |p: &Option<Animated<f64>>| p.as_ref().map(|p| p.sample(t));
        let rgb = |c: &Option<Animated<Color>>| {
            c.as_ref().map(|c| {
                let c = c.sample(t);
                [c.r, c.g, c.b]
            })
        };
        Values {
            anchor: pair(&self.anchor),
            position: pair(&self.position),
            scale: pair(&self.scale),
            skew: self.skew.as_ref().map(|(k, a)| (k.sample(t), a.sample(t))),
            rotation: one(&self.rotation),
            opacity: one(&self.opacity),
            stroke_width: one(&self.stroke_width),
            stroke_color: rgb(&self.stroke_color),
            fill_color: rgb(&self.fill_color),
            hue: one(&self.hue),
            saturation: one(&self.saturation),
            brightness: one(&self.brightness),
            tracking: one(&self.tracking),
        }
    }
}

/// An animator's values at one time.
struct Values {
    anchor: Option<[f64; 2]>,
    position: Option<[f64; 2]>,
    scale: Option<[f64; 2]>,
    skew: Option<(f64, f64)>,
    rotation: Option<f64>,
    opacity: Option<f64>,
    stroke_width: Option<f64>,
    stroke_color: Option<[f64; 3]>,
    fill_color: Option<[f64; 3]>,
    hue: Option<f64>,
    saturation: Option<f64>,
    brightness: Option<f64>,
    tracking: Option<f64>,
}

/// A range selector (lottie-web's `TextSelectorProperty`): characters,
/// words or lines from a start to an end (plus an offset), in percent or
/// by index, weighted by a shape, eased and smoothed, times the amount.
struct Selector {
    /// The default expression selector instead: `textIndex / textTotal`.
    by_index: bool,
    start: Animated<f64>,
    /// None: the last one.
    end: Option<Animated<f64>>,
    offset: Animated<f64>,
    /// A fraction.
    amount: Animated<f64>,
    /// Percent.
    ease_high: Animated<f64>,
    ease_low: Animated<f64>,
    smoothness: Animated<f64>,
    /// Start, end and offset count units (else percent of them).
    index_units: bool,
    /// What it counts: 1 characters, 2 characters but spaces, 3 words, 4
    /// lines.
    based: i64,
    /// 1 square, 2 ramp up, 3 ramp down, 4 triangle, 5 round, 6 smooth.
    shape: i64,
    /// In an order shuffled once (deterministically).
    randomize: bool,
    seed: u64,
}

/// A selector's values at one time.
struct Range {
    start: f64,
    end: f64,
    easing: Easing,
    smoothness: f64,
    amount: f64,
}

impl Selector {
    /// Its range at time `t`, in units of the `total` it counts.
    fn at(&self, t: f64, total: f64) -> Range {
        let divisor = if self.index_units { 1.0 } else { 100.0 / total };
        let offset = self.offset.sample(t) / divisor;
        let end = self
            .end
            .as_ref()
            .map_or(if self.index_units { total } else { 100.0 }, |e| {
                e.sample(t)
            });
        let mut start = self.start.sample(t) / divisor + offset;
        let mut end = end / divisor + offset;
        if start > end {
            std::mem::swap(&mut start, &mut end);
        }
        let (low, high) = (self.ease_low.sample(t), self.ease_high.sample(t));
        let (mut x1, mut y1, mut x2, mut y2) = (0.0, 0.0, 1.0, 1.0);
        if low > 0.0 {
            x1 = low / 100.0;
        } else {
            y1 = -low / 100.0;
        }
        if high > 0.0 {
            x2 = 1.0 - high / 100.0;
        } else {
            y2 = 1.0 + high / 100.0;
        }
        Range {
            start,
            end,
            easing: Easing::cubic_bezier(x1, y1, x2, y2),
            smoothness: self.smoothness.sample(t),
            amount: self.amount.sample(t),
        }
    }

    /// How much it selects the unit at `index` of `total` (lottie-web's
    /// `getMult`).
    fn weight(&self, r: &Range, index: f64, total: f64) -> f64 {
        if self.by_index {
            return if total > 0.0 {
                (index + 1.0) / total
            } else {
                0.0
            };
        }
        let (s, e) = (r.start, r.end);
        let ramp = || (0.5 / (e - s) + (index - s) / (e - s)).clamp(0.0, 1.0);
        let mut m = match self.shape {
            2 if e == s => f64::from(index >= e),
            2 => ramp(),
            3 if e == s => f64::from(index < e),
            3 => 1.0 - ramp(),
            4..=6 if e == s => 0.0,
            4 => {
                let m = ramp();
                if m < 0.5 {
                    m * 2.0
                } else {
                    1.0 - 2.0 * (m - 0.5)
                }
            }
            5 => {
                let span = e - s;
                let i = (index + 0.5 - s).max(0.0).min(span);
                let x = -span / 2.0 + i;
                let a = span / 2.0;
                (1.0 - x * x / (a * a)).max(0.0).sqrt()
            }
            6 => {
                let i = (index + 0.5 - s).max(0.0).min(e - s);
                (1.0 + (PI + 2.0 * PI * i / (e - s)).cos()) / 2.0
            }
            _ if index >= s.floor() => {
                if index - s < 0.0 {
                    (e.min(1.0) - (s - index)).clamp(0.0, 1.0)
                } else {
                    (e - index).clamp(0.0, 1.0)
                }
            }
            _ => 0.0,
        };
        m = r.easing.apply(m);
        if r.smoothness != 100.0 {
            let smoothness = match r.smoothness * 0.01 {
                0.0 => 1e-8,
                s => s,
            };
            let threshold = 0.5 - smoothness * 0.5;
            m = if m < threshold {
                0.0
            } else {
                ((m - threshold) / smoothness).min(1.0)
            };
        }
        m * r.amount
    }
}

/// One text document laid out as lottie-web lays it out, before
/// animators.
struct Layout {
    letters: Vec<Letter>,
    /// Each line's width.
    line_widths: Vec<f64>,
    size: f64,
    /// The glyphs' scale: a hundredth of the size.
    k: f64,
    /// Between letters.
    tracking: f64,
    line_height: f64,
    /// 0 left, 1 right, 2 centered.
    justify: i64,
    /// Baseline shift (up).
    shift: f64,
    fill: Option<Color>,
    stroke: Option<Color>,
    stroke_width: f64,
    stroke_over: bool,
}

impl Layout {
    /// A shape of the document's style drawing `paths`.
    fn shape(
        &self,
        paths: Vec<ShapePath>,
        fill: Option<Animated<Color>>,
        stroke: Option<(Animated<Color>, Animated<f64>)>,
    ) -> Shape {
        Shape {
            paths,
            fill: fill.map(|color| Fill {
                paint: Paint::Solid { color },
                opacity: Animated::Static(1.0),
                rule: FillRule::NonZero,
            }),
            stroke: stroke.map(|(color, width)| Stroke {
                paint: Paint::Solid { color },
                opacity: Animated::Static(1.0),
                width,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                miter_limit: 4.0,
                dash: None,
            }),
            paint_order: if self.stroke_over {
                PaintOrder::FillFirst
            } else {
                PaintOrder::StrokeFirst
            },
            ..Shape::default()
        }
    }
}

/// A character of a laid out document, or a line break (lottie-web's
/// letters).
struct Letter {
    /// Its glyph among the chars; none draws nothing.
    glyph: Option<usize>,
    /// Its advance without the tracking (`l`).
    advance: f64,
    newline: bool,
    space: bool,
    line: usize,
    /// Where its glyph's origin is without animators: justified, in the
    /// box, on the baseline.
    x: f64,
    y: f64,
    /// How far along its line it starts (`xPos` without animators).
    pen: f64,
    /// The width of the group it turns and scales with (`an`), its offset
    /// in it (`add`), the group's number (`ind`) and the width after the
    /// group (`extra`).
    group_width: f64,
    group_offset: f64,
    group: usize,
    extra: f64,
}

impl Letter {
    fn line_break(line: usize) -> Letter {
        Letter {
            glyph: None,
            advance: 0.0,
            newline: true,
            space: false,
            line,
            x: 0.0,
            y: 0.0,
            pen: 0.0,
            group_width: 0.0,
            group_offset: 0.0,
            group: 0,
            extra: 0.0,
        }
    }
}

/// Groups letters about shared anchors as lottie-web does (`m.g`): words
/// (2) or lines (3) share their width and number; otherwise each letter
/// is a group.
fn group(letters: &mut [Letter], grouping: i64) {
    let len = letters.len();
    let mut size = 0.0;
    let mut first = 0;
    let mut index = 0;
    for i in 0..len {
        let letter = &mut letters[i];
        letter.group_width = letter.advance;
        letter.group_offset = size;
        let (l, newline, space) = (letter.advance, letter.newline, letter.space);
        let ends = match grouping {
            2 => newline || space || i == len - 1,
            3 => newline || i == len - 1,
            _ => {
                letter.group = index;
                letter.extra = 0.0;
                index += 1;
                continue;
            }
        };
        size += l;
        if ends {
            if newline || (grouping == 2 && space) {
                size -= l;
            }
            for letter in &mut letters[first..=i] {
                letter.group_width = size;
                letter.group = index;
                letter.extra = l;
            }
            first = i + 1;
            index += 1;
            size = 0.0;
        }
    }
}

/// Each letter's unit for `selector` (lottie-web's `anIndexes`): what it
/// counts before it, in an order shuffled once when it randomizes; and
/// how many it counts.
fn indexes(letters: &[Letter], selector: &Selector) -> (Vec<f64>, f64) {
    let mut count = 0usize;
    let mut out = Vec::with_capacity(letters.len());
    let mut pool = Vec::new();
    for (i, letter) in letters.iter().enumerate() {
        out.push(count);
        let last = i + 1 == letters.len();
        let counted = match selector.based {
            2 => !letter.newline && !letter.space,
            3 => letter.newline || letter.space || last,
            4 => letter.newline || last,
            _ => !letter.newline,
        };
        if counted {
            pool.push(count);
            count += 1;
        }
    }
    if selector.randomize {
        let mut state = selector.seed;
        let mut current = None;
        let mut shuffled = 0;
        for index in &mut out {
            if current != Some(*index) {
                current = Some(*index);
                shuffled = if pool.is_empty() {
                    *index
                } else {
                    let k = (splitmix(&mut state) % pool.len() as u64) as usize;
                    pool.swap_remove(k)
                };
            }
            *index = shuffled;
        }
    }
    (out.into_iter().map(|i| i as f64).collect(), count as f64)
}

/// SplitMix64: the next of a deterministic sequence of random numbers.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The times to sample a document shown during `shown` at: the frames
/// (`fr` a second) from the first to the last of `keys` while it shows, at
/// most `most` of them; one time when nothing moves while it shows. Also
/// whether `most` cut the frames.
fn sample_times(keys: &[f64], shown: (f64, f64), fr: f64, most: usize) -> (Vec<f64>, bool) {
    let mut keys: Vec<f64> = keys.iter().copied().filter(|t| t.is_finite()).collect();
    let lo = keys.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = keys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if keys.is_empty() {
        return (vec![0.0], false);
    }
    let (a, b) = (lo.max(shown.0), hi.min(shown.1));
    if a >= b {
        return (vec![if shown.1 <= lo { lo } else { hi }], false);
    }
    keys.retain(|t| *t > a && *t < b);
    keys.push(a);
    keys.push(b);
    keys.sort_by(f64::total_cmp);
    keys.dedup();
    let frames = ((b - a) * fr).ceil().max(1.0) as usize;
    let cut = frames > most.max(2).saturating_sub(keys.len()).max(1);
    (frame_times(&keys, fr, most), cut)
}

/// The rotations and skews between an animated character's scale and its
/// position, outermost first: each animator's skew (about its axis) then
/// rotation, the last animator's outermost; neighbouring rotations add
/// up into one.
fn slots(animators: &[Animator]) -> Vec<Slot> {
    let mut slots: Vec<Slot> = Vec::new();
    let rotate = |slots: &mut Vec<Slot>, term: (usize, Term)| match slots.last_mut() {
        Some(slot) if !slot.skew => slot.terms.push(term),
        _ => slots.push(Slot {
            skew: false,
            terms: vec![term],
        }),
    };
    for (j, animator) in animators.iter().enumerate().rev() {
        if animator.rotation.is_some() {
            rotate(&mut slots, (j, Term::Rotation));
        }
        if animator.skew.is_some() {
            // lottie-web's `skewFromAxis(-skew, axis)`.
            rotate(&mut slots, (j, Term::Axis(-1.0)));
            slots.push(Slot {
                skew: true,
                terms: vec![(j, Term::Skew)],
            });
            rotate(&mut slots, (j, Term::Axis(1.0)));
        }
    }
    slots
}

/// A rotation (or a skew along x) of an animated character: the sum of
/// its terms.
struct Slot {
    skew: bool,
    terms: Vec<(usize, Term)>,
}

#[derive(Clone, Copy)]
enum Term {
    /// An animator's rotation.
    Rotation,
    /// An animator's skew axis, this way round.
    Axis(f64),
    /// An animator's skew, negated.
    Skew,
}

/// Each drawn letter's values at each time in `times`, as lottie-web's
/// `TextAnimatorProperty.getMeasures` computes them.
fn bake(
    layout: &Layout,
    animation: &Animation,
    indexes: &[(Vec<f64>, f64)],
    times: &[f64],
    drawn: &[usize],
) -> Vec<Channels> {
    let letters = &layout.letters;
    let animators = &animation.animators;
    let n = letters.len();
    let slots = slots(animators);
    let mut channels: Vec<Channels> = drawn.iter().map(|_| Channels::new(slots.len())).collect();
    let mut channel_of = vec![None; n];
    for (c, &i) in drawn.iter().enumerate() {
        channel_of[i] = Some(c);
    }
    let mut mults = vec![0.0; animators.len() * n];
    let y_offset = layout.size * 1.2 * 0.714;
    let justify = layout.justify;
    let still_course = animation
        .path
        .as_ref()
        .and_then(|p| match (&p.outline, &p.reverse) {
            (Animated::Static(outline), Animated::Static(r)) => {
                Some(Course::new(outline, *r != 0.0))
            }
            _ => None,
        });
    for &t in times {
        let values: Vec<Values> = animators.iter().map(|a| a.at(t)).collect();
        for (j, animator) in animators.iter().enumerate() {
            let (units, total) = &indexes[j];
            let range = animator.selector.at(t, *total);
            for (i, letter) in letters.iter().enumerate() {
                mults[j * n + i] = if letter.newline {
                    0.0
                } else {
                    animator.selector.weight(&range, units[i], *total)
                };
            }
        }
        let align = [
            animation.alignment[0].sample(t),
            animation.alignment[1].sample(t),
        ];
        let align_y = align[1] * y_offset * 0.01;
        // Text on a path: the path now, and where along it the text is.
        let moving_course = match &animation.path {
            Some(p) if still_course.is_none() => Some(Course::new(
                &p.outline.sample(t),
                p.reverse.sample(t) != 0.0,
            )),
            _ => None,
        };
        let path = animation.path.as_ref().and_then(|p| {
            Some(PathAt {
                course: still_course.as_ref().or(moving_course.as_ref())?,
                first_margin: p.first_margin.sample(t),
                last_margin: p.last_margin.sample(t),
                perpendicular: p.perpendicular.sample(t) != 0.0,
                force: p.force.sample(t) != 0.0,
            })
        });
        // Justified lines move back by half (or all) the tracking added
        // along them.
        let justified = if path.is_none() && matches!(justify, 1 | 2) {
            justify_offsets(letters, justify, &values, &mults)
        } else {
            Vec::new()
        };
        let mut shift = 0.0;
        let mut current = path.as_ref().map_or(0.0, |p| p.first_margin);
        let mut line_seen = None;
        let mut group_seen: Option<usize> = None;
        for (i, letter) in letters.iter().enumerate() {
            if letter.newline {
                shift = 0.0;
                current = path.as_ref().map_or(0.0, |p| p.first_margin);
                continue;
            }
            let m = |j: usize| mults[j * n + i];
            let align_x = align[0] * letter.group_width * 0.005;
            let half = letter.group_width / 2.0 - letter.group_offset;
            // Where along the path: the group's middle, moved by the
            // animators' horizontal positions and anchors.
            let mut head = None;
            if let Some(p) = &path {
                let total = p.course.total();
                if line_seen != Some(letter.line) {
                    let width = layout.line_widths.get(letter.line).copied().unwrap_or(0.0);
                    current += match justify {
                        1 => total - width,
                        2 => (total - width) / 2.0,
                        _ => 0.0,
                    };
                    line_seen = Some(letter.line);
                }
                if group_seen != Some(letter.group) {
                    if let Some(g) = group_seen {
                        current += letters.get(g).map_or(0.0, |l| l.extra);
                    }
                    current += letter.group_width / 2.0;
                    group_seen = Some(letter.group);
                }
                current += align_x;
                let mut along = 0.0;
                for (j, v) in values.iter().enumerate() {
                    if let Some(position) = v.position {
                        along += position[0] * m(j);
                    }
                    if let Some(anchor) = v.anchor {
                        along += anchor[0] * m(j);
                    }
                }
                if p.force {
                    let first = letters.first().map_or(0.0, |l| l.group_width);
                    let last = letters.last().map_or(0.0, |l| l.group_width);
                    let spread = total + p.last_margin - p.first_margin - first * 0.5 - last * 0.5;
                    current = first * 0.5
                        + p.first_margin
                        + if n > 1 {
                            spread * letter.group as f64 / (n - 1) as f64
                        } else {
                            0.0
                        };
                }
                let (point, angle) = p.course.at(current + along);
                head = Some((point, if p.perpendicular { angle } else { 0.0 }));
            }
            // Tracking: along the line (but before its first letter, unless
            // justified), or along the path.
            for (j, v) in values.iter().enumerate() {
                let Some(tracking) = v.tracking else {
                    continue;
                };
                if path.is_some() {
                    if justify != 0 {
                        current += tracking * m(j);
                    }
                } else if letter.pen + shift != 0.0 || justify != 0 {
                    shift += tracking * m(j);
                }
            }
            let mut anchor = [0.0; 2];
            let mut position = [0.0; 2];
            let mut scale = [1.0; 2];
            let mut opacity = 1.0;
            let mut fill = layout.fill.map_or([0.0; 3], |c| [c.r, c.g, c.b]);
            let mut stroke = layout.stroke.map_or([0.0; 3], |c| [c.r, c.g, c.b]);
            let mut stroke_width = layout.stroke_width;
            for (j, v) in values.iter().enumerate() {
                let m = m(j);
                if let Some(a) = v.anchor {
                    anchor[0] += a[0] * m;
                    anchor[1] += a[1] * m;
                }
                if let Some(p) = v.position {
                    position[0] += p[0] * m;
                    position[1] += p[1] * m;
                }
                if let Some(s) = v.scale {
                    scale[0] *= 1.0 + (s[0] - 1.0) * m;
                    scale[1] *= 1.0 + (s[1] - 1.0) * m;
                }
                if let Some(o) = v.opacity {
                    opacity += (o * m - opacity) * m;
                }
                if let Some(w) = v.stroke_width {
                    stroke_width += w * m;
                }
                if let Some(c) = v.stroke_color {
                    for k in 0..3 {
                        stroke[k] += (c[k] - stroke[k]) * m;
                    }
                }
                if layout.fill.is_some() {
                    if let Some(c) = v.fill_color {
                        for k in 0..3 {
                            fill[k] += (c[k] - fill[k]) * m;
                        }
                    }
                    if let Some(h) = v.hue {
                        fill = add_hue(fill, h * m);
                    }
                    if let Some(s) = v.saturation {
                        fill = add_saturation(fill, s * m);
                    }
                    if let Some(b) = v.brightness {
                        fill = add_brightness(fill, b * m);
                    }
                }
            }
            if path.is_some() {
                current -= align_x;
                if letters
                    .get(i + 1)
                    .is_some_and(|next| next.group != letter.group)
                {
                    current += letter.group_width / 2.0 + layout.tracking;
                }
            }
            let Some(c) = channel_of[i] else {
                continue;
            };
            let ch = &mut channels[c];
            let inner = [-(half + align_x) - anchor[0], -align_y - anchor[1]];
            let mid = match head {
                Some((point, angle)) => {
                    ch.head[0].push1(t, point.x);
                    ch.head[1].push1(t, point.y);
                    ch.head[2].push_angle(t, angle);
                    let row = letter.line as f64 * layout.line_height;
                    [0.0, position[1] - layout.shift + align_y + row]
                }
                None => [
                    letter.x
                        + shift
                        + justified.get(i).copied().unwrap_or(0.0)
                        + half
                        + align_x
                        + position[0],
                    letter.y + align_y + position[1],
                ],
            };
            for k in 0..2 {
                ch.mid[k].push1(t, mid[k]);
                ch.inner[k].push1(t, inner[k]);
                ch.merged[k].push1(t, mid[k] + inner[k]);
                ch.scale[k].push1(t, scale[k]);
            }
            for (slot, series) in slots.iter().zip(&mut ch.slots) {
                let value: f64 = slot
                    .terms
                    .iter()
                    .map(|&(j, term)| {
                        let v = &values[j];
                        let (skew, axis) = v.skew.unwrap_or((0.0, 0.0));
                        m(j) * match term {
                            Term::Rotation => v.rotation.unwrap_or(0.0),
                            Term::Axis(sign) => sign * axis,
                            Term::Skew => -skew,
                        }
                    })
                    .sum();
                series.push1(t, value);
            }
            ch.opacity.push1(t, opacity);
            ch.fill.push(t, fill);
            ch.stroke.push(t, stroke);
            ch.stroke_width.push1(t, stroke_width.max(0.0));
        }
    }
    channels
}

/// A text path at one time.
struct PathAt<'c> {
    course: &'c Course,
    first_margin: f64,
    last_margin: f64,
    perpendicular: bool,
    force: bool,
}

/// What lottie-web moves each line by when the text is justified right
/// (1) or centered (2): back by the tracking animators add along it (a
/// share of the first letters' in every later line too, as it does).
fn justify_offsets(letters: &[Letter], justify: i64, values: &[Values], mults: &[f64]) -> Vec<f64> {
    let n = letters.len();
    let factor = if justify == 2 { -0.5 } else { -1.0 };
    let mut out = vec![0.0; n];
    let (mut sum, mut first_letters, mut last) = (0.0, 0.0, 0);
    let mut new_line = true;
    for (i, letter) in letters.iter().enumerate() {
        if letter.newline {
            if sum != 0.0 {
                sum += first_letters;
            }
            out[last..i].fill(sum);
            last = i;
            sum = 0.0;
            new_line = true;
            continue;
        }
        for (j, v) in values.iter().enumerate() {
            let Some(tracking) = v.tracking else {
                continue;
            };
            if new_line && justify == 2 {
                first_letters += tracking * factor;
            }
            sum += tracking * mults[j * n + i] * factor;
        }
        new_line = false;
    }
    if sum != 0.0 {
        sum += first_letters;
    }
    out[last..].fill(sum);
    out
}

/// `color` with its hue turned by `degrees` (lottie-web's `addHueToRGB`).
fn add_hue(color: [f64; 3], degrees: f64) -> [f64; 3] {
    let [mut h, s, v] = hsv(color);
    h += degrees / 360.0;
    if h > 1.0 {
        h -= 1.0;
    } else if h < 0.0 {
        h += 1.0;
    }
    rgb(h, s, v)
}

/// `color` with `amount` more saturation, within 0 to 1.
fn add_saturation(color: [f64; 3], amount: f64) -> [f64; 3] {
    let [h, s, v] = hsv(color);
    rgb(h, (s + amount).clamp(0.0, 1.0), v)
}

/// `color` with `amount` more brightness (value), within 0 to 1.
fn add_brightness(color: [f64; 3], amount: f64) -> [f64; 3] {
    let [h, s, v] = hsv(color);
    rgb(h, s, (v + amount).clamp(0.0, 1.0))
}

/// Hue (0 to 1), saturation and value of an RGB color.
fn hsv([r, g, b]: [f64; 3]) -> [f64; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let s = if max == 0.0 { 0.0 } else { d / max };
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        (g - b + d * if g < b { 6.0 } else { 0.0 }) / (6.0 * d)
    } else if max == g {
        (b - r + d * 2.0) / (6.0 * d)
    } else {
        (r - g + d * 4.0) / (6.0 * d)
    };
    [h, s, max]
}

/// The RGB color of hue `h` (0 to 1), saturation `s` and value `v`.
fn rgb(h: f64, s: f64, v: f64) -> [f64; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    match (i as i64).rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// The path text is laid along: its first subpath's segments and how far
/// along each one ends.
struct Course {
    segments: Vec<kurbo::PathSeg>,
    ends: Vec<f64>,
    closed: bool,
}

impl Course {
    fn new(path: &Path, reverse: bool) -> Course {
        let mut first = Path::new();
        let mut closed = false;
        for (i, el) in path.elements().iter().enumerate() {
            if i > 0 && matches!(el, PathEl::MoveTo(_)) {
                break;
            }
            first.0.push(*el);
            if matches!(el, PathEl::Close) {
                closed = true;
                break;
            }
        }
        let mut segments: Vec<kurbo::PathSeg> = first.to_kurbo().segments().collect();
        if reverse {
            segments.reverse();
            for s in &mut segments {
                *s = s.reverse();
            }
        }
        let mut total = 0.0;
        let ends = segments
            .iter()
            .map(|s| {
                total += s.arclen(ACCURACY);
                total
            })
            .collect();
        Course {
            segments,
            ends,
            closed,
        }
    }

    fn total(&self) -> f64 {
        self.ends.last().copied().unwrap_or(0.0)
    }

    /// The point `s` along the path and its direction there (clockwise
    /// degrees): a closed path wraps around, an open one goes straight on
    /// past its ends.
    fn at(&self, s: f64) -> (Point, f64) {
        let total = self.total();
        let (Some(first), Some(last)) = (self.segments.first(), self.segments.last()) else {
            return (Point::ZERO, 0.0);
        };
        let point = |p: kurbo::Point| Point::new(p.x, p.y);
        let degrees = |d: kurbo::Vec2| d.y.atan2(d.x).to_degrees();
        if total <= 0.0 {
            return (point(first.start()), 0.0);
        }
        let s = if self.closed { s.rem_euclid(total) } else { s };
        let unit = |d: kurbo::Vec2| {
            if d.hypot2() > 0.0 {
                d.normalize()
            } else {
                kurbo::Vec2::new(1.0, 0.0)
            }
        };
        if s < 0.0 {
            let d = unit(tangent(*first, 0.0));
            return (point(first.start() + d * s), degrees(d));
        }
        if s > total {
            let d = unit(tangent(*last, 1.0));
            return (point(last.end() + d * (s - total)), degrees(d));
        }
        let i = self
            .ends
            .partition_point(|e| *e < s)
            .min(self.segments.len() - 1);
        let begin = if i == 0 { 0.0 } else { self.ends[i - 1] };
        let length = self.ends[i] - begin;
        let segment = self.segments[i];
        let u = if length > 0.0 {
            segment.inv_arclen((s - begin).clamp(0.0, length), ACCURACY)
        } else {
            0.0
        };
        (point(segment.eval(u)), degrees(tangent(segment, u)))
    }
}

/// The direction of `segment` at `t`, looking past degenerate control
/// points.
fn tangent(segment: kurbo::PathSeg, t: f64) -> kurbo::Vec2 {
    let d = match segment {
        kurbo::PathSeg::Line(l) => l.p1 - l.p0,
        kurbo::PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
        kurbo::PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
    };
    if d.hypot2() > 1e-18 {
        return d;
    }
    let near = if t < 0.5 { t + 1e-3 } else { t - 1e-3 };
    let (a, b) = (segment.eval(t), segment.eval(near));
    if t < 0.5 { b - a } else { a - b }
}

/// The keyframe times of `a`.
fn times<T>(a: &Animated<T>, out: &mut Vec<f64>) {
    out.extend(a.keyframes().iter().map(|k| k.time));
}

/// An animated character's values over time.
struct Channels {
    /// On a path: where (x, y) and which way (degrees).
    head: [Series; 3],
    /// The position the character's anchor goes to (on a path, across
    /// it).
    mid: [Series; 2],
    /// Rotations and skews about the anchor, outermost first.
    slots: Vec<Series>,
    scale: [Series; 2],
    /// From the glyph's origin to its anchor.
    inner: [Series; 2],
    /// `mid` and `inner` together, for a character that neither turns nor
    /// scales.
    merged: [Series; 2],
    opacity: Series,
    fill: Series,
    stroke: Series,
    stroke_width: Series,
}

impl Channels {
    fn new(slots: usize) -> Channels {
        Channels {
            head: Default::default(),
            mid: Default::default(),
            slots: (0..slots).map(|_| Series::default()).collect(),
            scale: Default::default(),
            inner: Default::default(),
            merged: Default::default(),
            opacity: Series::default(),
            fill: Series::default(),
            stroke: Series::default(),
            stroke_width: Series::default(),
        }
    }

    /// Whether the character neither turns, skews nor scales.
    fn upright(&self) -> bool {
        let is = |s: &Series, value: f64| s.constant().is_some_and(|v| v[0] == value);
        self.slots.iter().all(|s| is(s, 0.0)) && self.scale.iter().all(|s| is(s, 1.0))
    }

    /// The keyframes its moving values take.
    fn keyframes(&self, on_path: bool, filled: bool, stroked: bool) -> usize {
        let count = |series: &[Series]| series.iter().map(Series::keyframes).sum::<usize>();
        let mut n = self.opacity.keyframes();
        if on_path {
            n += count(&self.head);
        }
        if self.upright() {
            n += count(&self.merged);
        } else {
            n += count(&self.mid) + count(&self.slots) + count(&self.scale) + count(&self.inner);
        }
        if filled {
            n += self.fill.keyframes();
        }
        if stroked {
            n += self.stroke.keyframes() + self.stroke_width.keyframes();
        }
        n
    }

    /// The character's transform, outermost first: along the path, to its
    /// position, turned, skewed and scaled about its anchor.
    fn transform(&self, slots: &[Slot], on_path: bool) -> Vec<TransformOp> {
        let translate = |s: &[Series; 2]| TransformOp::Translate {
            x: s[0].scalar(),
            y: s[1].scalar(),
        };
        let mut about: Vec<TransformOp> = slots
            .iter()
            .zip(&self.slots)
            .map(|(slot, series)| {
                let value = series.scalar();
                if slot.skew {
                    TransformOp::Skew {
                        x: value,
                        y: Animated::Static(0.0),
                    }
                } else {
                    TransformOp::Rotate { degrees: value }
                }
            })
            .collect();
        about.push(TransformOp::Scale {
            x: self.scale[0].scalar(),
            y: self.scale[1].scalar(),
        });
        let mut ops = Vec::new();
        if on_path {
            ops.push(TransformOp::Translate {
                x: self.head[0].scalar(),
                y: self.head[1].scalar(),
            });
            ops.push(TransformOp::Rotate {
                degrees: self.head[2].scalar(),
            });
        }
        if self.upright() {
            ops.push(translate(&self.merged));
        } else {
            ops.push(translate(&self.mid));
            ops.extend(about);
            ops.push(translate(&self.inner));
        }
        simplify(ops)
    }
}

/// A value of up to three numbers sampled over time, keeping only the
/// samples that straight lines between the kept ones do not give back.
#[derive(Default)]
struct Series {
    kept: Vec<(f64, [f64; 3])>,
    pending: Option<(f64, [f64; 3])>,
}

impl Series {
    fn push(&mut self, t: f64, v: [f64; 3]) {
        let Some(&(kt, kv)) = self.kept.last() else {
            self.kept.push((t, v));
            return;
        };
        if let Some((pt, pv)) = self.pending {
            let f = if t > kt { (pt - kt) / (t - kt) } else { 0.0 };
            if !(0..3).all(|i| close(pv[i], kv[i] + (v[i] - kv[i]) * f)) {
                self.kept.push((pt, pv));
            }
        }
        self.pending = Some((t, v));
    }

    fn push1(&mut self, t: f64, v: f64) {
        self.push(t, [v, 0.0, 0.0]);
    }

    /// An angle, turned by whole turns to be nearest the last one.
    fn push_angle(&mut self, t: f64, degrees: f64) {
        let last = self.pending.or(self.kept.last().copied()).map(|s| s.1[0]);
        let degrees = match last {
            Some(l) => l + (degrees - l + 180.0).rem_euclid(360.0) - 180.0,
            None => degrees,
        };
        self.push1(t, degrees);
    }

    fn samples(&self) -> impl Iterator<Item = &(f64, [f64; 3])> {
        self.kept.iter().chain(&self.pending)
    }

    /// Its value when it does not change.
    fn constant(&self) -> Option<[f64; 3]> {
        let first = self.kept.first()?.1;
        self.samples()
            .all(|s| (0..3).all(|i| close(s.1[i], first[i])))
            .then_some(first)
    }

    fn keyframes(&self) -> usize {
        if self.constant().is_some() {
            0
        } else {
            self.samples().count()
        }
    }

    fn scalar(&self) -> Animated<f64> {
        if let Some(v) = self.constant() {
            return Animated::Static(v[0]);
        }
        Animated::Track(Track::new(
            self.samples()
                .map(|&(t, v)| Keyframe::new(t, v[0], Easing::Linear))
                .collect(),
        ))
    }

    fn color(&self) -> Animated<Color> {
        let color = |v: [f64; 3]| Color::rgba(v[0], v[1], v[2], 1.0).clamped();
        if let Some(v) = self.constant() {
            return Animated::Static(color(v));
        }
        Animated::Track(Track::new(
            self.samples()
                .map(|&(t, v)| Keyframe::new(t, color(v), Easing::Linear))
                .collect(),
        ))
    }
}

/// Whether `a` and `b` are the same but for rounding.
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()))
}
