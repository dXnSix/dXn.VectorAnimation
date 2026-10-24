//! SMIL animation: `<animate>`, `<set>`, `<animateColor>`,
//! `<animateTransform>` and `<animateMotion>`, turned into keyframes on
//! the scene's timeline as text the property parsers read, as CSS
//! animations' keyframes are.
//!
//! Begin times are resolved ahead: offsets (negative ones too) and other
//! animations' begins and ends (`a.end + 1s`), chains that start one
//! another over and over included; such a cycle becomes the timeline's
//! loop. Events (`click`) and `indefinite` never begin.

use std::collections::HashMap;

use dxn_vector_animation::{Easing, Keyframe, MotionRotate, Path, Point};
use roxmltree::{Node as XmlNode, NodeId};

use crate::values::{angle, color, number, time};

/// How many instances of all animations are resolved, at most.
const MAX_INSTANCES: usize = 4_000;
/// How many iterations of one instance are unrolled, at most.
const MAX_ITERATIONS: usize = 10_000;

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

/// When an animation begins.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Begin {
    /// Seconds from the document's start.
    Offset(f64),
    /// Another animation's begin or end, plus seconds.
    Sync { id: String, end: bool, offset: f64 },
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
    /// `fill="freeze"`.
    freeze: bool,
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

fn begins(s: Option<&str>, warn: &mut dyn FnMut(String)) -> Vec<Begin> {
    let Some(s) = s else {
        return vec![Begin::Offset(0.0)];
    };
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
        match (parsed, offset) {
            (Some((id, end)), Some(offset)) => out.push(Begin::Sync {
                id: id.to_owned(),
                end,
                offset,
            }),
            _ => warn(format!(
                "SMIL begin `{item}` (an event or a repeat) never begins here"
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
        if attr("end").is_some() {
            warn("SMIL `end` times are left out: animations run their whole duration".into());
        }
        if attr("accumulate") == Some("sum") {
            warn("SMIL `accumulate=\"sum\"` is left out".into());
        }
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
            begins: begins(attr("begin"), warn),
            freeze: attr("fill") == Some("freeze"),
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
    fn at_end(&self, stops: &[(f64, String, Easing)], length: f64) -> String {
        let dur = self.dur.unwrap_or(f64::INFINITY);
        if !dur.is_finite() || dur <= 0.0 {
            return stops.first().map(|s| s.1.clone()).unwrap_or_default();
        }
        let p = (length / dur).fract();
        if p <= 1e-9 {
            return stops.last().map(|s| s.1.clone()).unwrap_or_default();
        }
        stops
            .iter()
            .rev()
            .find(|s| s.0 <= p)
            .map(|s| s.1.clone())
            .unwrap_or_default()
    }

    /// What shows after an instance of active duration `length` ends:
    /// its last value when frozen, else `base`.
    fn after(&self, stops: &[(f64, String, Easing)], length: f64, base: &str) -> String {
        if self.freeze {
            self.at_end(stops, length)
        } else {
            base.to_owned()
        }
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
    anims: &[(&Smil, &[f64])],
    total: f64,
    loops: bool,
    base: &str,
) -> Vec<Keyframe<String>> {
    let stops: Vec<Vec<(f64, String, Easing)>> = anims.iter().map(|(a, _)| a.stops(base)).collect();
    // Every instance, in the order they begin (a later animation wins a tie).
    let mut instances: Vec<(f64, usize)> = anims
        .iter()
        .enumerate()
        .flat_map(|(k, (_, list))| list.iter().map(move |&b| (b, k)))
        .collect();
    instances.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut out = Vec::new();
    let first = instances.first().map_or(f64::INFINITY, |i| i.0);
    if first > 0.0 {
        let initial = match instances.last() {
            Some(&(b, k)) if loops => {
                let a = anims[k].0;
                a.after(&stops[k], a.active().min(total - b), base)
            }
            _ => base.to_owned(),
        };
        out.push(Keyframe::new(0.0, initial, Easing::Hold));
    }
    for (n, &(b, k)) in instances.iter().enumerate() {
        let (a, stops) = (anims[k].0, &stops[k]);
        let next = instances.get(n + 1).map_or(f64::INFINITY, |i| i.0);
        let dur = a.dur.unwrap_or(f64::INFINITY);
        let end = (b + a.active()).min(next);
        let mut i = 0usize;
        loop {
            // (`0 × ∞` is not 0: an indefinite duration has one iteration.)
            let start = if i == 0 { b } else { b + i as f64 * dur };
            if (i > 0 && start >= end) || start > total || i >= MAX_ITERATIONS {
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
                out.push(Keyframe::new(t, value.clone(), easing.clone()));
            }
            if !dur.is_finite() {
                break;
            }
            i += 1;
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
    /// Seconds the timeline needs.
    pub(crate) duration: f64,
    /// Whether it repeats.
    pub(crate) endless: bool,
    /// Whether the animations start one another in a cycle of `duration`.
    pub(crate) cyclic: bool,
}

/// Resolves the begin times of `anims` up to `horizon` seconds.
pub(crate) fn resolve(anims: &[Smil], horizon: f64) -> Timing {
    let n = anims.len();
    let index: HashMap<&str, usize> = anims
        .iter()
        .enumerate()
        .filter_map(|(i, a)| a.id.as_deref().map(|id| (id, i)))
        .collect();
    // Who starts when another begins or ends: (dependent, on end, offset).
    let mut dependents: Vec<Vec<(usize, bool, f64)>> = vec![Vec::new(); n];
    let mut events: Vec<(f64, usize, bool)> = Vec::new(); // (time, anim, from a syncbase)
    for (i, a) in anims.iter().enumerate() {
        for b in &a.begins {
            match b {
                Begin::Offset(t) => events.push((*t, i, false)),
                Begin::Sync { id, end, offset } => {
                    if let Some(&j) = index.get(id.as_str()) {
                        dependents[j].push((i, *end, *offset));
                    }
                }
            }
        }
    }
    let mut instances: Vec<Vec<f64>> = vec![Vec::new(); n];
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
        if t > horizon {
            break;
        }
        if instances[i].iter().any(|b| (b - t).abs() < 1e-9) {
            continue;
        }
        instances[i].push(t);
        from_sync[i].push(sync);
        count += 1;
        let active = anims[i].active();
        for &(j, on_end, offset) in &dependents[i] {
            let at = if on_end { t + active } else { t };
            if at.is_finite() {
                events.push((at + offset, j, true));
            }
        }
    }
    // A cycle: an animation begun at an offset begins again from a syncbase.
    let mut cycle: Option<f64> = None;
    for i in 0..n {
        let first_offset = instances[i]
            .iter()
            .zip(&from_sync[i])
            .find(|(_, s)| !**s)
            .map(|(t, _)| *t);
        if let Some(t0) = first_offset
            && let Some((t1, _)) = instances[i]
                .iter()
                .zip(&from_sync[i])
                .find(|(t, s)| **s && **t > t0 + 1e-9)
        {
            let period = t1 - t0;
            if period > 0.0 && cycle.is_none_or(|p| period < p) {
                cycle = Some(period);
            }
        }
    }
    if let Some(period) = cycle {
        for list in &mut instances {
            list.retain(|t| *t < period - 1e-9);
        }
        return Timing {
            instances,
            duration: period,
            endless: true,
            cyclic: true,
        };
    }
    let mut duration: f64 = 0.0;
    let mut endless = false;
    for (a, list) in anims.iter().zip(&instances) {
        let Some(&last) = list.last() else { continue };
        let active = a.active();
        if active.is_finite() {
            duration = duration.max(last + active);
        } else {
            // Held forever (a `set`), or repeating forever.
            endless |= a.repeats_forever();
            duration = duration.max(last + a.dur.unwrap_or(0.0));
        }
    }
    Timing {
        instances,
        duration,
        endless,
        cyclic: false,
    }
}
