//! SMIL animation: `<animate>`, `<set>`, `<animateColor>`,
//! `<animateTransform>` and `<animateMotion>`, turned into keyframes on
//! the scene's timeline as text the property parsers read, as CSS
//! animations' keyframes are.
//!
//! Begin times are resolved ahead: offsets (negative ones too) and other
//! animations' begins, ends and repeats (`a.end + 1s`, `a.repeat(2)`),
//! chains that start one another over and over included; such a cycle
//! becomes the timeline's loop. `end` lists cut instances short; repeats
//! accumulate (`accumulate="sum"`). What begins on an event (`click`)
//! plays in a segment of its own past the timeline's end, under a marker
//! `on:<element>.<event>`; `indefinite` never begins.

use std::collections::HashMap;

use dxn_vector_animation::{Easing, Keyframe, MotionRotate, Path, Point};
use roxmltree::{Node as XmlNode, NodeId};

use crate::values::{angle, color, number, time};

/// How many instances of all animations are resolved, at most.
const MAX_INSTANCES: usize = 4_000;
/// Where what plays on events goes: past any timeline, a segment for each
/// event, `CUE_STRIDE` apart, at most `CUE_SPAN` long.
const CUE_BASE: f64 = 1.0e6;
const CUE_STRIDE: f64 = 1.0e6;
const CUE_SPAN: f64 = 1.0e5;

/// Where the segment of the event whose instance begins at `t` starts.
fn cue_start(t: f64) -> f64 {
    CUE_BASE + ((t - CUE_BASE) / CUE_STRIDE).floor() * CUE_STRIDE
}

/// How far instances beginning at `t` play: the timeline's `horizon`, or
/// their event's segment.
fn limit(t: f64, horizon: f64) -> f64 {
    if t >= CUE_BASE {
        cue_start(t) + CUE_SPAN
    } else {
        horizon
    }
}
/// How many iterations of one instance are unrolled, at most.
const MAX_ITERATIONS: usize = crate::animate::MAX_ITERATIONS;

/// What a SMIL element animates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    /// `<animate>` / `<animateColor>`: an attribute or property.
    Animate,
    /// `<set>`: a value for the active duration.
    Set,
    /// `<animateTransform>` of a type: `translate`, `scale`, `rotate`,
    /// `skewX`, `skewY`.
    Transform(String),
    /// `<animateMotion>` along a path.
    Motion {
        /// The path.
        path: Path,
        /// How the element turns along it.
        rotate: MotionRotate,
        /// `keyPoints`: progress along the path at each key time.
        key_points: Option<Vec<f64>>,
    },
}

/// When an animation begins (or ends).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Begin {
    /// Seconds from the document's start.
    Offset(f64),
    /// Another animation's begin or end, plus seconds.
    Sync { id: String, end: bool, offset: f64 },
    /// Another animation's `iteration`th repeat, plus seconds.
    Repeat {
        id: String,
        iteration: u32,
        offset: f64,
    },
    /// An event (`btn.click`), plus seconds.
    Event { name: String, offset: f64 },
}

/// How many times the simple duration repeats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Repeat {
    /// This many times (fractions play part of the last).
    Count(f64),
    /// Forever.
    Indefinite,
}

/// One SMIL element.
#[derive(Debug, Clone)]
pub(crate) struct Smil {
    pub(crate) id: Option<String>,
    /// The element it animates, and its tag.
    pub(crate) target: NodeId,
    pub(crate) target_tag: String,
    pub(crate) kind: Kind,
    /// The attribute (`transform` for transforms, empty for motion).
    pub(crate) attribute: String,
    values: Option<Vec<String>>,
    from: Option<String>,
    to: Option<String>,
    by: Option<String>,
    key_times: Option<Vec<f64>>,
    key_splines: Option<Vec<[f64; 4]>>,
    calc_mode: String,
    /// `additive="sum"`.
    pub(crate) additive: bool,
    /// The simple duration; `None` when indefinite.
    dur: Option<f64>,
    repeat: Option<Repeat>,
    repeat_dur: Option<f64>,
    pub(crate) begins: Vec<Begin>,
    /// When its instances end, at the latest.
    pub(crate) ends: Vec<Begin>,
    /// `fill="freeze"`.
    freeze: bool,
    /// `accumulate="sum"`: each repeat goes on from where the last ended.
    accumulate: bool,
}

fn list(s: &str) -> Vec<String> {
    s.split(';')
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .collect()
}

fn clock(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Some(t) = time(s) {
        return Some(t);
    }
    if let Some(n) = s.strip_suffix("min") {
        return number(n).map(|v| v * 60.0);
    }
    if let Some(n) = s.strip_suffix('h') {
        return number(n).map(|v| v * 3600.0);
    }
    // `hh:mm:ss.f`, `mm:ss.f` or a bare number of seconds.
    let parts: Vec<&str> = s.split(':').collect();
    match parts.as_slice() {
        [secs] => number(secs),
        [m, secs] => Some(number(m)? * 60.0 + number(secs)?),
        [h, m, secs] => Some(number(h)? * 3600.0 + number(m)? * 60.0 + number(secs)?),
        _ => None,
    }
}

fn begins(s: Option<&str>, target: Option<&str>, warn: &mut dyn FnMut(String)) -> Vec<Begin> {
    match s {
        Some(s) => times(s, "begin", target, warn),
        None => vec![Begin::Offset(0.0)],
    }
}

/// The times of a `begin` or `end` list (`what`); events without an
/// element happen on `target` (the animated element's id).
fn times(s: &str, what: &str, target: Option<&str>, warn: &mut dyn FnMut(String)) -> Vec<Begin> {
    let mut out = Vec::new();
    for item in s.split(';').map(str::trim).filter(|i| !i.is_empty()) {
        if item == "indefinite" {
            continue;
        }
        if let Some(t) = clock(item) {
            out.push(Begin::Offset(t));
            continue;
        }
        // `id.begin`, `id.end`, with an optional `± offset`.
        let (head, offset) = match item.find(['+', '-']) {
            Some(i) if i > 0 => {
                let sign = if item[i..].starts_with('-') {
                    -1.0
                } else {
                    1.0
                };
                (item[..i].trim(), clock(&item[i + 1..]).map(|t| sign * t))
            }
            _ => (item, Some(0.0)),
        };
        let parsed = head
            .strip_suffix(".end")
            .map(|id| (id, true))
            .or_else(|| head.strip_suffix(".begin").map(|id| (id, false)));
        // `id.repeat(n)`.
        let repeat = head
            .strip_suffix(')')
            .and_then(|h| h.split_once(".repeat("))
            .and_then(|(id, n)| Some((id, n.trim().parse::<u32>().ok()?)));
        // `id.beginEvent` and `id.endEvent` are its begins and ends.
        let parsed = parsed.or_else(|| {
            head.strip_suffix(".beginEvent")
                .map(|id| (id, false))
                .or_else(|| head.strip_suffix(".endEvent").map(|id| (id, true)))
        });
        // An event: `click`, `btn.click`.
        let event = {
            let (element, name) = match head.rsplit_once('.') {
                Some((element, name)) => (Some(element), name),
                None => (target, head),
            };
            (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphabetic())).then(|| {
                match element {
                    Some(e) => format!("{e}.{name}"),
                    None => name.to_owned(),
                }
            })
        };
        match (parsed, repeat, offset) {
            (Some((id, end)), _, Some(offset)) => out.push(Begin::Sync {
                id: id.to_owned(),
                end,
                offset,
            }),
            (None, Some((id, iteration)), Some(offset)) => out.push(Begin::Repeat {
                id: id.to_owned(),
                iteration,
                offset,
            }),
            (None, None, Some(offset)) if what == "begin" && event.is_some() => {
                out.push(Begin::Event {
                    name: event.unwrap_or_default(),
                    offset: offset.max(0.0),
                })
            }
            _ => warn(format!(
                "SMIL {what} `{item}` (an event) never happens here"
            )),
        }
    }
    out
}

/// The SMIL elements of `doc` and the elements they animate. `paths`
/// gives the outline of a `<path>` by id (for `<mpath>`).
pub(crate) fn collect<'a>(
    doc: &'a roxmltree::Document<'_>,
    ids: &HashMap<&'a str, XmlNode<'a, '_>>,
    paths: &dyn Fn(XmlNode<'a, '_>) -> Option<Path>,
    warn: &mut dyn FnMut(String),
) -> Vec<Smil> {
    let mut out = Vec::new();
    for node in doc.descendants().filter(|n| n.is_element()) {
        let tag = node.tag_name().name();
        if !matches!(
            tag,
            "animate" | "set" | "animateColor" | "animateTransform" | "animateMotion"
        ) {
            continue;
        }
        let href = node
            .attribute(("http://www.w3.org/1999/xlink", "href"))
            .or_else(|| node.attribute("href"))
            .and_then(|h| h.strip_prefix('#'));
        let target = match href {
            Some(id) => ids.get(id).copied(),
            None => node.parent_element(),
        };
        let Some(target) = target else {
            warn("a SMIL animation refers to an element the document does not have".into());
            continue;
        };
        let attr = |name: &str| node.attribute(name);
        let kind = match tag {
            "animateTransform" => Kind::Transform(attr("type").unwrap_or("translate").to_owned()),
            "animateMotion" => {
                let mpath = node
                    .children()
                    .find(|c| c.tag_name().name() == "mpath")
                    .and_then(|m| {
                        m.attribute(("http://www.w3.org/1999/xlink", "href"))
                            .or_else(|| m.attribute("href"))
                    })
                    .and_then(|h| ids.get(h.trim_start_matches('#')).copied())
                    .and_then(paths);
                let path = mpath
                    .or_else(|| attr("path").map(crate::convert::path_data))
                    .or_else(|| {
                        let points: Vec<Point> = match attr("values") {
                            Some(v) => list(v).iter().filter_map(|p| point(p)).collect(),
                            None => {
                                let from = attr("from").and_then(point).unwrap_or(Point::ZERO);
                                let to = attr("to")
                                    .and_then(point)
                                    .or_else(|| attr("by").and_then(point).map(|b| from + b))?;
                                vec![from, to]
                            }
                        };
                        let mut p = Path::new();
                        for (i, pt) in points.iter().enumerate() {
                            if i == 0 {
                                p.move_to(*pt);
                            } else {
                                p.line_to(*pt);
                            }
                        }
                        (points.len() > 1).then_some(p)
                    });
                let Some(path) = path else {
                    warn("an <animateMotion> without a path is left out".into());
                    continue;
                };
                let rotate = match attr("rotate").map(str::trim) {
                    Some("auto") => MotionRotate::Auto { offset: 0.0 },
                    Some("auto-reverse") => MotionRotate::Auto { offset: 180.0 },
                    Some(a) => MotionRotate::Fixed {
                        degrees: angle(a).unwrap_or(0.0),
                    },
                    None => MotionRotate::Fixed { degrees: 0.0 },
                };
                let key_points =
                    attr("keyPoints").map(|k| list(k).iter().filter_map(|v| number(v)).collect());
                Kind::Motion {
                    path,
                    rotate,
                    key_points,
                }
            }
            "set" => Kind::Set,
            _ => Kind::Animate,
        };
        let attribute = match &kind {
            Kind::Transform(_) => "transform".to_owned(),
            Kind::Motion { .. } => String::new(),
            _ => attr("attributeName").unwrap_or("").trim().to_owned(),
        };
        if attribute.is_empty() && !matches!(kind, Kind::Motion { .. }) {
            continue;
        }
        let dur = attr("dur").and_then(|d| {
            if d.trim() == "indefinite" {
                None
            } else {
                clock(d)
            }
        });
        let repeat = attr("repeatCount").map(|r| {
            if r.trim() == "indefinite" {
                Repeat::Indefinite
            } else {
                Repeat::Count(number(r).unwrap_or(1.0).max(0.0))
            }
        });
        let repeat_dur = attr("repeatDur").and_then(|r| {
            if r.trim() == "indefinite" {
                Some(f64::INFINITY)
            } else {
                clock(r)
            }
        });

        let calc_mode = attr("calcMode")
            .map(str::to_owned)
            .unwrap_or_else(|| match kind {
                Kind::Motion { .. } => "paced".into(),
                Kind::Set => "discrete".into(),
                _ => "linear".into(),
            });
        // Motion through points, not paced: each segment takes its share of
        // the time, so the progress at each point is its share of the length.
        let mut kind = kind;
        if let Kind::Motion {
            path,
            key_points: kp @ None,
            ..
        } = &mut kind
            && attr("values").is_some()
            && calc_mode != "paced"
        {
            let total = dxn_vector_animation::outline::path_length(path);
            let mut so_far = Path::new();
            let mut at = Vec::new();
            for el in path.elements() {
                so_far.0.push(*el);
                let length = dxn_vector_animation::outline::path_length(&so_far);
                at.push(if total > 0.0 { length / total } else { 0.0 });
            }
            *kp = Some(at);
        }
        out.push(Smil {
            id: attr("id").map(str::to_owned),
            target: target.id(),
            target_tag: target.tag_name().name().to_owned(),
            attribute,
            values: attr("values").map(list),
            from: attr("from").map(str::to_owned),
            to: attr("to").map(str::to_owned),
            by: attr("by").map(str::to_owned),
            key_times: attr("keyTimes").map(|k| list(k).iter().filter_map(|v| number(v)).collect()),
            key_splines: attr("keySplines").map(|k| {
                list(k)
                    .iter()
                    .filter_map(|s| {
                        let v: Vec<f64> = s
                            .split(|c: char| c == ',' || c.is_whitespace())
                            .filter(|p| !p.is_empty())
                            .filter_map(number)
                            .collect();
                        (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
                    })
                    .collect()
            }),
            calc_mode,
            additive: attr("additive") == Some("sum"),
            dur,
            repeat,
            repeat_dur,
            begins: begins(attr("begin"), target.attribute("id"), warn),
            ends: attr("end")
                .map_or_else(Vec::new, |e| times(e, "end", target.attribute("id"), warn)),
            freeze: attr("fill") == Some("freeze"),
            accumulate: attr("accumulate") == Some("sum"),
            kind,
        });
    }
    out
}

/// `x,y` or `x y`.
fn point(s: &str) -> Option<Point> {
    let v: Vec<f64> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .filter_map(number)
        .collect();
    match v.as_slice() {
        [x, y] => Some(Point::new(*x, *y)),
        [x] => Some(Point::new(*x, 0.0)),
        _ => None,
    }
}

impl Smil {
    /// The active duration of one instance (infinite: forever).
    pub(crate) fn active(&self) -> f64 {
        let Some(dur) = self.dur else {
            return self.repeat_dur.unwrap_or(f64::INFINITY);
        };
        let by_count = match self.repeat {
            Some(Repeat::Count(n)) => dur * n,
            Some(Repeat::Indefinite) => f64::INFINITY,
            None if self.repeat_dur.is_some() => f64::INFINITY,
            None => dur,
        };
        match self.repeat_dur {
            Some(rd) => by_count.min(rd),
            None => by_count,
        }
    }

    /// What one iteration plays: (offset 0..1, value, easing to the next),
    /// `base` standing for what the animation starts from where the file
    /// leaves it out.
    fn stops(&self, base: &str) -> Vec<(f64, String, Easing)> {
        let values: Vec<String> = match (&self.values, &self.kind) {
            (
                _,
                Kind::Motion {
                    key_points: Some(points),
                    ..
                },
            ) => points.iter().map(|p| p.to_string()).collect(),
            (_, Kind::Motion { .. }) => vec!["0".into(), "1".into()],
            (Some(v), _) if !v.is_empty() => v.clone(),
            _ => {
                let from = self.from.clone();
                match (&self.kind, from, &self.to, &self.by) {
                    (Kind::Set, _, Some(to), _) => vec![to.clone()],
                    (_, Some(f), Some(to), _) => vec![f, to.clone()],
                    (_, None, Some(to), _) => vec![base.to_owned(), to.clone()],
                    (_, f, None, Some(by)) => {
                        let start = f.unwrap_or_else(|| base.to_owned());
                        match (number(&start), number(by)) {
                            (Some(a), Some(b)) => vec![start, (a + b).to_string()],
                            _ => vec![start],
                        }
                    }
                    _ => vec![base.to_owned()],
                }
            }
        };
        let values: Vec<String> = values.into_iter().map(|v| self.value(&v, base)).collect();
        let n = values.len();
        if n == 1 {
            return vec![(0.0, values[0].clone(), Easing::Hold)];
        }
        let discrete = self.calc_mode == "discrete";
        // Offsets: `keyTimes` (motion's go with its `keyPoints`), else even,
        // or by distance when paced.
        let key_times = self
            .key_times
            .as_ref()
            .filter(|k| k.len() == n && self.calc_mode != "paced");
        let mut offsets: Vec<f64> = match key_times {
            Some(k) => k.clone(),
            None if self.calc_mode == "paced" => {
                paced(&values).unwrap_or_else(|| even(n, discrete))
            }
            None => even(n, discrete),
        };
        for i in 1..n {
            offsets[i] = offsets[i].max(offsets[i - 1]);
        }
        let mut out: Vec<(f64, String, Easing)> = values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let easing = if discrete {
                    Easing::Hold
                } else if self.calc_mode == "spline" {
                    self.key_splines
                        .as_ref()
                        .and_then(|s| s.get(i))
                        .map_or(Easing::Linear, |s| {
                            Easing::cubic_bezier(s[0], s[1], s[2], s[3])
                        })
                } else {
                    Easing::Linear
                };
                (offsets[i], v.clone(), easing)
            })
            .collect();
        // A discrete last value holds to the end of the iteration.
        if discrete && out.last().is_some_and(|l| l.0 < 1.0) {
            let last = out[n - 1].1.clone();
            out.push((1.0, last, Easing::Hold));
        }
        out
    }

    /// A value as the property parsers read it.
    fn value(&self, v: &str, base: &str) -> String {
        match &self.kind {
            Kind::Transform(kind) => transform_fn(kind, v),
            Kind::Animate if self.additive => match (number(base), number(v)) {
                (Some(b), Some(x)) => (b + x).to_string(),
                _ => v.to_owned(),
            },
            _ => v.to_owned(),
        }
    }

    /// The text of the value that changes nothing, of the same functions as
    /// the animation's (where a transform animation does not apply).
    pub(crate) fn identity(&self) -> String {
        match &self.kind {
            Kind::Transform(kind) => {
                let first = self
                    .values
                    .as_ref()
                    .and_then(|v| v.first().cloned())
                    .or_else(|| self.from.clone())
                    .or_else(|| self.to.clone())
                    .unwrap_or_default();
                let n: Vec<f64> = numbers(&first);
                match kind.as_str() {
                    "rotate" if n.len() >= 3 => format!("rotate(0 {} {})", n[1], n[2]),
                    "rotate" => "rotate(0)".into(),
                    "scale" => "scale(1 1)".into(),
                    "skewX" => "skewX(0)".into(),
                    "skewY" => "skewY(0)".into(),
                    _ => "translate(0 0)".into(),
                }
            }
            Kind::Motion { .. } => "0".into(),
            _ => String::new(),
        }
    }

    /// The value where an active duration of `length` stops.
    fn at_end(&self, stops: &[(f64, String, Easing)], length: f64, base: &str) -> String {
        let dur = self.dur.unwrap_or(f64::INFINITY);
        if !dur.is_finite() || dur <= 0.0 {
            return stops.first().map(|s| s.1.clone()).unwrap_or_default();
        }
        let whole = length / dur;
        let p = whole.fract();
        let (iteration, value) = if p <= 1e-9 {
            (
                (whole.round() as usize).saturating_sub(1),
                stops.last().map(|s| s.1.clone()).unwrap_or_default(),
            )
        } else {
            // Within a segment: as far along it as its easing says.
            let at = stops.iter().rposition(|s| s.0 <= p).unwrap_or(0);
            let value = match (stops.get(at), stops.get(at + 1)) {
                (Some(a), Some(b)) if b.0 > a.0 => {
                    let progress = a.2.apply((p - a.0) / (b.0 - a.0));
                    if matches!(a.2, Easing::Hold) {
                        a.1.clone()
                    } else {
                        between(&a.1, &b.1, progress)
                    }
                }
                (Some(a), _) => a.1.clone(),
                _ => String::new(),
            };
            (whole.floor() as usize, value)
        };
        self.accumulated(&value, stops, base, iteration)
    }

    /// What shows after an instance of active duration `length` ends:
    /// its last value when frozen, else `base`.
    fn after(&self, stops: &[(f64, String, Easing)], length: f64, base: &str) -> String {
        if self.freeze {
            self.at_end(stops, length, base)
        } else {
            base.to_owned()
        }
    }

    /// `value` in repeat `iteration` (from 0): with `accumulate="sum"`, the
    /// simple animation's end added `iteration` times (where the values
    /// are numbers alike).
    fn accumulated(
        &self,
        value: &str,
        stops: &[(f64, String, Easing)],
        base: &str,
        iteration: usize,
    ) -> String {
        if !self.accumulate || iteration == 0 {
            return value.to_owned();
        }
        let Some(last) = stops.last() else {
            return value.to_owned();
        };
        // What one simple animation adds: its end, less the base it was
        // added to (additive) or that `by` goes on from.
        let numbers = |v: &str| -> Vec<f64> { number_spans(v).into_iter().map(|s| s.2).collect() };
        let mut delta = numbers(&last.1);
        let from = if self.additive {
            Some(numbers(base))
        } else if self.by.is_some() && self.values.is_none() && self.to.is_none() {
            Some(numbers(
                &self.value(self.from.as_deref().unwrap_or(base), base),
            ))
        } else {
            None
        };
        if let Some(from) = from {
            if from.len() != delta.len() {
                return value.to_owned();
            }
            for (d, f) in delta.iter_mut().zip(from) {
                *d -= f;
            }
        }
        add_numbers(value, &delta, iteration as f64).unwrap_or_else(|| value.to_owned())
    }

    /// Whether it repeats forever.
    pub(crate) fn repeats_forever(&self) -> bool {
        self.repeat == Some(Repeat::Indefinite) || self.repeat_dur.is_some_and(f64::is_infinite)
    }
}

/// The keyframes of one attribute on the timeline (`total` seconds),
/// animated by `anims` with their instances: at any time the instance that
/// began last applies (SMIL's priority), while active or frozen; `base`
/// shows where none does. On a timeline that `loops` a cycle, what froze
/// at the end of the cycle shows at its start.
pub(crate) fn keyframes(
    anims: &[(&Smil, &[f64], &[f64])],
    total: f64,
    loops: bool,
    base: &str,
    warn: &mut dyn FnMut(String),
) -> Vec<Keyframe<String>> {
    let stops: Vec<Vec<(f64, String, Easing)>> =
        anims.iter().map(|(a, _, _)| a.stops(base)).collect();
    // Every instance, in the order they begin (a later animation wins a
    // tie), with how long it is active.
    let mut instances: Vec<(f64, usize, f64)> = anims
        .iter()
        .enumerate()
        .flat_map(|(k, (_, list, lengths))| {
            list.iter()
                .zip(lengths.iter())
                .map(move |(&b, &l)| (b, k, l))
        })
        .collect();
    instances.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut out = Vec::new();
    let first = instances.first().map_or(f64::INFINITY, |i| i.0);
    if first > 0.0 {
        let initial = match instances.iter().rev().find(|i| i.0 < CUE_BASE) {
            Some(&(b, k, length)) if loops => {
                let a = anims[k].0;
                a.after(&stops[k], length.min(total - b), base)
            }
            _ => base.to_owned(),
        };
        out.push(Keyframe::new(0.0, initial, Easing::Hold));
    }
    for (n, &(b, k, length)) in instances.iter().enumerate() {
        let (a, stops) = (anims[k].0, &stops[k]);
        let total = limit(b, total);
        let next = instances.get(n + 1).map_or(f64::INFINITY, |i| i.0);
        let dur = a.dur.unwrap_or(f64::INFINITY);
        let end = (b + length).min(next);
        let mut i = 0usize;
        loop {
            // (`0 × ∞` is not 0: an indefinite duration has one iteration.)
            let start = if i == 0 { b } else { b + i as f64 * dur };
            if (i > 0 && start >= end) || start > total {
                break;
            }
            if i >= MAX_ITERATIONS {
                warn(format!(
                    "SMIL animation of `{}` on <{}> runs more than {MAX_ITERATIONS} iterations: \
                     only the first {MAX_ITERATIONS} play",
                    a.attribute, a.target_tag
                ));
                break;
            }
            for (offset, value, easing) in stops {
                let t = if dur.is_finite() {
                    start + offset * dur
                } else {
                    start
                };
                if t > end + 1e-9 {
                    break;
                }
                let value = a.accumulated(value, stops, base, i);
                out.push(Keyframe::new(t, value, easing.clone()));
            }
            if !dur.is_finite() {
                break;
            }
            i += 1;
        }
        // Cut inside a segment (by its end or the next instance): what it
        // shows up to the cut.
        if end.is_finite()
            && end <= total
            && out
                .last()
                .is_some_and(|k: &Keyframe<String>| k.time < end - 1e-9)
        {
            out.push(Keyframe::new(
                end,
                a.at_end(stops, end - b, base),
                Easing::Hold,
            ));
        }
        if end.is_finite() && end < next && end <= total {
            out.push(Keyframe::new(
                end,
                a.after(stops, end - b, base),
                Easing::Hold,
            ));
        }
    }
    out
}

fn even(n: usize, discrete: bool) -> Vec<f64> {
    let steps = if discrete { n } else { n - 1 };
    (0..n).map(|i| i as f64 / steps as f64).collect()
}

/// Offsets proportional to the distances between `values` (numbers,
/// number lists, colors); `None` for values without a distance.
fn paced(values: &[String]) -> Option<Vec<f64>> {
    let as_point = |v: &str| -> Option<Vec<f64>> {
        if let Some(c) = color(v) {
            return Some(vec![c.r * 255.0, c.g * 255.0, c.b * 255.0]);
        }
        let n = numbers(v);
        (!n.is_empty()).then_some(n)
    };
    let points: Vec<Vec<f64>> = values.iter().map(|v| as_point(v)).collect::<Option<_>>()?;
    let mut cumulative = vec![0.0];
    for w in points.windows(2) {
        let d: f64 = w[0]
            .iter()
            .zip(&w[1])
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        cumulative.push(cumulative.last().copied().unwrap_or(0.0) + d);
    }
    let total = *cumulative.last()?;
    (total > 0.0).then(|| cumulative.iter().map(|c| c / total).collect())
}

/// The numbers written in `value` (`translate(10 20)`: 10 and 20), where
/// each starts and ends; not those inside names (`skewX`, `#f0a`).
fn number_spans(value: &str) -> Vec<(usize, usize, f64)> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let starts = c.is_ascii_digit()
            || (c == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
            || ((c == b'-' || c == b'+')
                && bytes
                    .get(i + 1)
                    .is_some_and(|n| n.is_ascii_digit() || *n == b'.'));
        let in_word = i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'#');
        if !starts || in_word {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
            j += 1;
        }
        if j < bytes.len() && (bytes[j] == b'e' || bytes[j] == b'E') {
            let mut e = j + 1;
            if e < bytes.len() && (bytes[e] == b'-' || bytes[e] == b'+') {
                e += 1;
            }
            if e < bytes.len() && bytes[e].is_ascii_digit() {
                while e < bytes.len() && bytes[e].is_ascii_digit() {
                    e += 1;
                }
                j = e;
            }
        }
        if let Ok(n) = value[i..j].parse::<f64>() {
            out.push((i, j, n));
        }
        i = j;
    }
    out
}

/// The value a fraction `t` of the way from `a` to `b`: colors mixed,
/// numbers moved one by one where both have as many (`a` where neither).
fn between(a: &str, b: &str, t: f64) -> String {
    if let (Some(x), Some(y)) = (color(a), color(b)) {
        let c = x.lerp(y, t);
        return format!(
            "rgba({},{},{},{})",
            (c.r * 255.0).round(),
            (c.g * 255.0).round(),
            (c.b * 255.0).round(),
            c.a
        );
    }
    let (from, to) = (number_spans(a), number_spans(b));
    if from.len() != to.len() {
        return a.to_owned();
    }
    let delta: Vec<f64> = from.iter().zip(&to).map(|(x, y)| y.2 - x.2).collect();
    add_numbers(a, &delta, t).unwrap_or_else(|| a.to_owned())
}

/// `value` with `k` times `delta` added to its numbers in turn, units and
/// the text around them kept; `None` where the counts differ.
fn add_numbers(value: &str, delta: &[f64], k: f64) -> Option<String> {
    let spans = number_spans(value);
    if spans.len() != delta.len() || spans.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(value.len() + 8);
    let mut at = 0;
    for ((i, j, n), d) in spans.into_iter().zip(delta) {
        out.push_str(&value[at..i]);
        out.push_str(&(n + k * d).to_string());
        at = j;
    }
    out.push_str(&value[at..]);
    Some(out)
}

/// The numbers in `s` (with units dropped).
fn numbers(s: &str) -> Vec<f64> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .filter_map(|p| crate::values::length(p, 0.0).or_else(|| angle(p)))
        .collect()
}

/// An `<animateTransform>` value as a transform function.
fn transform_fn(kind: &str, v: &str) -> String {
    let n = numbers(v);
    let get = |i: usize, default: f64| n.get(i).copied().unwrap_or(default);
    match kind {
        "scale" => {
            let x = get(0, 1.0);
            format!("scale({} {})", x, get(1, x))
        }
        "rotate" if n.len() >= 3 => {
            format!("rotate({} {} {})", get(0, 0.0), get(1, 0.0), get(2, 0.0))
        }
        "rotate" => format!("rotate({})", get(0, 0.0)),
        "skewX" => format!("skewX({})", get(0, 0.0)),
        "skewY" => format!("skewY({})", get(0, 0.0)),
        _ => format!("translate({} {})", get(0, 0.0), get(1, 0.0)),
    }
}

/// When each animation begins, and the timeline they need.
#[derive(Debug, Clone, Default)]
pub(crate) struct Timing {
    /// Each animation's begin times, ascending.
    pub(crate) instances: Vec<Vec<f64>>,
    /// How long each of those instances is active (its `end` cutting it
    /// short).
    pub(crate) lengths: Vec<Vec<f64>>,
    /// Seconds the timeline needs.
    pub(crate) duration: f64,
    /// Whether it repeats.
    pub(crate) endless: bool,
    /// What repeats forever: when its last instance begins, and its
    /// duration.
    pub(crate) repeats: Vec<(f64, f64)>,
    /// Whether the animations start one another in a cycle of `duration`.
    pub(crate) cyclic: bool,
    /// The events something plays on: each one's name, where its segment
    /// starts and how long it is.
    pub(crate) cues: Vec<(String, f64, f64)>,
}

/// Resolves the begin times of `anims` up to `horizon` seconds.
pub(crate) fn resolve(anims: &[Smil], horizon: f64, warn: &mut dyn FnMut(String)) -> Timing {
    let n = anims.len();
    let index: HashMap<&str, usize> = anims
        .iter()
        .enumerate()
        .filter_map(|(i, a)| a.id.as_deref().map(|id| (id, i)))
        .collect();
    // Who starts when another begins, ends or repeats: (dependent, when,
    // offset).
    #[derive(Clone, Copy)]
    enum When {
        Begin,
        End,
        Repeat(u32),
    }
    let mut dependents: Vec<Vec<(usize, When, f64)>> = vec![Vec::new(); n];
    let mut events: Vec<(f64, usize, bool)> = Vec::new(); // (time, anim, from a syncbase)
    let mut cue_names: Vec<String> = Vec::new();
    for (i, a) in anims.iter().enumerate() {
        for b in &a.begins {
            match b {
                Begin::Offset(t) => events.push((*t, i, false)),
                Begin::Event { name, offset } => {
                    let k = match cue_names.iter().position(|n| n == name) {
                        Some(k) => k,
                        None => {
                            cue_names.push(name.clone());
                            cue_names.len() - 1
                        }
                    };
                    let at = CUE_BASE + k as f64 * CUE_STRIDE + offset.min(CUE_SPAN);
                    events.push((at, i, false));
                }
                Begin::Sync { id, end, offset } => {
                    if let Some(&j) = index.get(id.as_str()) {
                        let when = if *end { When::End } else { When::Begin };
                        dependents[j].push((i, when, *offset));
                    }
                }
                Begin::Repeat {
                    id,
                    iteration,
                    offset,
                } => {
                    if let Some(&j) = index.get(id.as_str()) {
                        dependents[j].push((i, When::Repeat(*iteration), *offset));
                    }
                }
            }
        }
    }
    // An instance begun at `t` is active this long: its active duration,
    // cut at the first `end` offset from `t` on.
    let length = |i: usize, t: f64| {
        anims[i]
            .ends
            .iter()
            .filter_map(|e| match e {
                Begin::Offset(e) if *e >= t => Some(e - t),
                _ => None,
            })
            .fold(anims[i].active(), f64::min)
    };
    let mut instances: Vec<Vec<f64>> = vec![Vec::new(); n];
    let mut lengths: Vec<Vec<f64>> = vec![Vec::new(); n];
    let mut from_sync: Vec<Vec<bool>> = vec![Vec::new(); n];
    let mut count = 0usize;
    while !events.is_empty() && count < MAX_INSTANCES {
        // The earliest event next.
        let (k, _) = events
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.0.total_cmp(&b.1.0))
            .unwrap_or((0, &(0.0, 0, false)));
        let (t, i, sync) = events.swap_remove(k);
        if t > limit(t, horizon) {
            continue;
        }
        if instances[i].iter().any(|b| (b - t).abs() < 1e-9) {
            continue;
        }
        let active = length(i, t);
        instances[i].push(t);
        lengths[i].push(active);
        from_sync[i].push(sync);
        count += 1;
        for &(j, when, offset) in &dependents[i] {
            let at = match when {
                When::Begin => t,
                When::End => t + active,
                When::Repeat(k) => match anims[i].dur {
                    Some(d) if f64::from(k) * d < active => t + f64::from(k) * d,
                    _ => f64::INFINITY,
                },
            };
            if at.is_finite() {
                events.push((at + offset, j, true));
            }
        }
    }
    // Beginnings past the cap.
    let left: Vec<f64> = events
        .iter()
        .filter(|&&(t, i, _)| {
            t <= limit(t, horizon) && !instances[i].iter().any(|b| (b - t).abs() < 1e-9)
        })
        .map(|e| e.0)
        .collect();
    let mut capped = |kept: &dyn Fn(f64) -> bool| {
        if left.iter().any(|&t| kept(t)) {
            warn(format!(
                "SMIL animations begin more than {MAX_INSTANCES} times: later beginnings are \
                 left out"
            ));
        }
    };
    // Ends at other animations' begins and ends cut instances short too
    // (what began on their ends kept the whole length).
    for i in 0..n {
        let mut at: Vec<f64> = Vec::new();
        for e in &anims[i].ends {
            let Begin::Sync { id, end, offset } = e else {
                continue;
            };
            let Some(&j) = index.get(id.as_str()) else {
                continue;
            };
            for (b, l) in instances[j].iter().zip(&lengths[j]) {
                at.push(if *end { b + l } else { *b } + offset);
            }
        }
        for (b, l) in instances[i].iter().zip(lengths[i].iter_mut()) {
            for e in at.iter().filter(|e| **e >= *b) {
                *l = l.min(e - b);
            }
        }
    }
    // What plays on each event: from its segment's start to the last end
    // in it.
    let cues: Vec<(String, f64, f64)> = cue_names
        .into_iter()
        .enumerate()
        .map(|(k, name)| {
            let start = CUE_BASE + k as f64 * CUE_STRIDE;
            let end = instances
                .iter()
                .zip(&lengths)
                .flat_map(|(list, lengths)| list.iter().zip(lengths))
                .filter(|(b, _)| **b >= start && **b < start + CUE_SPAN)
                .map(|(b, l)| (b + l).min(start + CUE_SPAN))
                .fold(start, f64::max);
            (name, start, end - start)
        })
        .collect();
    // A cycle: an animation begun at an offset begins again from a syncbase
    // (on the timeline).
    let mut cycle: Option<f64> = None;
    for i in 0..n {
        let first_offset = instances[i]
            .iter()
            .zip(&from_sync[i])
            .find(|(t, s)| !**s && **t < CUE_BASE)
            .map(|(t, _)| *t);
        if let Some(t0) = first_offset
            && let Some((t1, _)) = instances[i]
                .iter()
                .zip(&from_sync[i])
                .find(|(t, s)| **s && **t > t0 + 1e-9 && **t < CUE_BASE)
        {
            let period = t1 - t0;
            if period > 0.0 && cycle.is_none_or(|p| period < p) {
                cycle = Some(period);
            }
        }
    }
    if let Some(period) = cycle {
        capped(&|t| t < period - 1e-9 || t >= CUE_BASE);
        for (list, lengths) in instances.iter_mut().zip(lengths.iter_mut()) {
            let kept: (Vec<f64>, Vec<f64>) = list
                .iter()
                .zip(lengths.iter())
                .filter(|(t, _)| **t < period - 1e-9 || **t >= CUE_BASE)
                .unzip();
            (*list, *lengths) = kept;
        }
        return Timing {
            instances,
            lengths,
            duration: period,
            endless: true,
            repeats: Vec::new(),
            cyclic: true,
            cues,
        };
    }
    capped(&|_| true);
    let mut duration: f64 = 0.0;
    let mut endless = false;
    let mut repeats = Vec::new();
    for ((a, list), lengths) in anims.iter().zip(&instances).zip(&lengths) {
        // The last instance on the timeline.
        let Some(at) = list.iter().rposition(|t| *t < CUE_BASE) else {
            continue;
        };
        let (last, active) = (list[at], lengths[at]);
        if active.is_finite() {
            duration = duration.max(last + active);
        } else {
            // Held forever (a `set`), or repeating forever.
            if a.repeats_forever() {
                endless = true;
                if let Some(dur) = a.dur {
                    repeats.push((last, dur));
                }
            }
            duration = duration.max(last + a.dur.unwrap_or(0.0));
        }
    }
    Timing {
        instances,
        lengths,
        duration,
        endless,
        repeats,
        cyclic: false,
        cues,
    }
}
