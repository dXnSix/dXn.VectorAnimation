//! CSS and SVG values: numbers, lengths, angles, times, colors, timing
//! functions, transform lists and the `animation` shorthand.

use dxn_vector_animation::{Color, Easing, Matrix, StepPosition};

use crate::css::split_top;

/// A number (`1.5`, `-2e3`).
pub(crate) fn number(s: &str) -> Option<f64> {
    let v: f64 = s.trim().parse().ok()?;
    v.is_finite().then_some(v)
}

/// A number or a percentage (`0.5`, `50%`) as a fraction.
pub(crate) fn fraction(s: &str) -> Option<f64> {
    let s = s.trim();
    match s.strip_suffix('%') {
        Some(p) => number(p).map(|v| v / 100.0),
        None => number(s),
    }
}

/// A length in user units; a percentage is of `reference`.
pub(crate) fn length(s: &str, reference: f64) -> Option<f64> {
    let s = s.trim();
    if let Some(p) = s.strip_suffix('%') {
        return number(p).map(|v| v / 100.0 * reference);
    }
    let split = number_end(s);
    let (n, unit) = s.split_at(split);
    let n = number(n)?;
    let scale = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        // A font size of 16 px, as browsers default to.
        "em" | "rem" => 16.0,
        "ex" => 8.0,
        _ => return None,
    };
    Some(n * scale)
}

/// Where the number at the start of `s` ends (sign, digits, a fraction,
/// an exponent).
fn number_end(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
        i += 1;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    i
}

/// An angle in degrees (`45deg`, `1.2rad`, `0.5turn`; a bare number is
/// degrees, as SVG writes them).
pub(crate) fn angle(s: &str) -> Option<f64> {
    let s = s.trim().to_ascii_lowercase();
    for (suffix, scale) in [
        ("deg", 1.0),
        ("grad", 0.9),
        ("rad", 180.0 / std::f64::consts::PI),
        ("turn", 360.0),
    ] {
        if let Some(n) = s.strip_suffix(suffix) {
            return number(n).map(|v| v * scale);
        }
    }
    number(&s)
}

/// A time in seconds (`9.3s`, `200ms`).
pub(crate) fn time(s: &str) -> Option<f64> {
    let s = s.trim().to_ascii_lowercase();
    if let Some(n) = s.strip_suffix("ms") {
        return number(n).map(|v| v / 1000.0);
    }
    s.strip_suffix('s').and_then(number)
}

/// A CSS color.
pub(crate) fn color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("transparent") {
        return Some(Color::TRANSPARENT);
    }
    let c: svgtypes::Color = s.parse().ok()?;
    Some(Color::rgba8(c.red, c.green, c.blue, c.alpha))
}

/// The `name(args)` functions of `s` in order, with any text between them;
/// `None` when something else is there.
fn functions(s: &str) -> Option<Vec<(String, Vec<&str>)>> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let close = rest[open..].find(')')? + open;
        let args = rest[open + 1..close]
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|a| !a.is_empty())
            .collect();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return None;
        }
        out.push((name.to_ascii_lowercase(), args));
        rest = rest[close + 1..]
            .trim_start()
            .trim_start_matches(',')
            .trim_start();
    }
    Some(out)
}

/// A CSS timing function.
pub(crate) fn timing_function(s: &str) -> Option<Easing> {
    let s = s.trim();
    let keyword = s.to_ascii_lowercase();
    match keyword.as_str() {
        "linear" => return Some(Easing::Linear),
        "ease" => return Some(Easing::EASE),
        "ease-in" => return Some(Easing::EASE_IN),
        "ease-out" => return Some(Easing::EASE_OUT),
        "ease-in-out" => return Some(Easing::EASE_IN_OUT),
        "step-start" => {
            return Some(Easing::Steps {
                count: 1,
                position: StepPosition::JumpStart,
            });
        }
        "step-end" => {
            return Some(Easing::Steps {
                count: 1,
                position: StepPosition::JumpEnd,
            });
        }
        _ => {}
    }
    let open = s.find('(')?;
    let name = s[..open].trim().to_ascii_lowercase();
    let inner = s[open + 1..].strip_suffix(')')?;
    match name.as_str() {
        "cubic-bezier" => {
            let v: Vec<f64> = inner.split(',').map(number).collect::<Option<_>>()?;
            let [x1, y1, x2, y2] = v.as_slice() else {
                return None;
            };
            Some(Easing::cubic_bezier(*x1, *y1, *x2, *y2))
        }
        "steps" => {
            let mut parts = inner.split(',');
            let count: u32 = parts.next()?.trim().parse().ok()?;
            let position = match parts.next().map(|p| p.trim().to_ascii_lowercase()) {
                None => StepPosition::JumpEnd,
                Some(p) => match p.as_str() {
                    "jump-start" | "start" => StepPosition::JumpStart,
                    "jump-end" | "end" => StepPosition::JumpEnd,
                    "jump-none" => StepPosition::JumpNone,
                    "jump-both" => StepPosition::JumpBoth,
                    _ => return None,
                },
            };
            Some(Easing::Steps {
                count: count.max(1),
                position,
            })
        }
        "linear" => linear_function(inner),
        _ => None,
    }
}

/// CSS `linear()`: outputs with optional input percentages; missing inputs
/// are spread evenly between their neighbours.
fn linear_function(inner: &str) -> Option<Easing> {
    let mut points: Vec<(Option<f64>, f64)> = Vec::new();
    for item in inner.split(',') {
        let mut parts = item.split_whitespace();
        let output = number(parts.next()?)?;
        let inputs: Vec<f64> = parts
            .map(|p| p.strip_suffix('%').and_then(number).map(|v| v / 100.0))
            .collect::<Option<_>>()?;
        match inputs.as_slice() {
            [] => points.push((None, output)),
            [a] => points.push((Some(*a), output)),
            [a, b] => {
                points.push((Some(*a), output));
                points.push((Some(*b), output));
            }
            _ => return None,
        }
    }
    let n = points.len();
    if n < 2 {
        return None;
    }
    if points[0].0.is_none() {
        points[0].0 = Some(0.0);
    }
    if points[n - 1].0.is_none() {
        points[n - 1].0 = Some(1.0);
    }
    // Inputs never go back, and runs of missing ones are spread evenly.
    let mut last = 0.0f64;
    for p in points.iter_mut() {
        if let Some(x) = p.0.as_mut() {
            *x = x.max(last);
            last = *x;
        }
    }
    let mut i = 0;
    while i < n {
        if points[i].0.is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let mut end = i;
        while points[end].0.is_none() {
            end += 1;
        }
        let (a, b) = (points[start].0.unwrap_or(0.0), points[end].0.unwrap_or(1.0));
        for (k, p) in points[start + 1..end].iter_mut().enumerate() {
            p.0 = Some(a + (b - a) * (k + 1) as f64 / (end - start) as f64);
        }
        i = end;
    }
    Some(Easing::LinearPoints {
        points: points
            .into_iter()
            .map(|(x, y)| (x.unwrap_or(0.0), y))
            .collect(),
    })
}

/// One function of a transform list, reduced to its 2D primitive
/// (`translateX(a)` is `Translate(a, 0)`, as CSS interpolates them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TransformFn {
    Translate(f64, f64),
    Scale(f64, f64),
    Rotate(f64),
    Skew(f64, f64),
    Matrix(Matrix),
}

impl TransformFn {
    pub(crate) fn same_kind(&self, other: &TransformFn) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// The function of the same kind that changes nothing.
    pub(crate) fn identity(&self) -> TransformFn {
        match self {
            TransformFn::Translate(..) => TransformFn::Translate(0.0, 0.0),
            TransformFn::Scale(..) => TransformFn::Scale(1.0, 1.0),
            TransformFn::Rotate(_) => TransformFn::Rotate(0.0),
            TransformFn::Skew(..) => TransformFn::Skew(0.0, 0.0),
            TransformFn::Matrix(_) => TransformFn::Matrix(Matrix::IDENTITY),
        }
    }

    pub(crate) fn matrix(&self) -> Matrix {
        match *self {
            TransformFn::Translate(x, y) => Matrix::translate(x, y),
            TransformFn::Scale(x, y) => Matrix::scale(x, y),
            TransformFn::Rotate(a) => Matrix::rotate(a),
            TransformFn::Skew(x, y) => Matrix::skew(x, y),
            TransformFn::Matrix(m) => m,
        }
    }
}

/// A transform list, as the `transform` attribute (unitless numbers,
/// `rotate(a cx cy)`) or the CSS property (units) writes it. `box_size` is
/// what percentages of translations are of. `none` is the empty list.
pub(crate) fn transform_list(s: &str, box_size: (f64, f64)) -> Option<Vec<TransformFn>> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for (name, args) in functions(s)? {
        let len = |i: usize, reference: f64| args.get(i).and_then(|a| length(a, reference));
        let num = |i: usize| args.get(i).and_then(|a| fraction(a));
        let ang = |i: usize| args.get(i).and_then(|a| angle(a));
        let n = args.len();
        match (name.as_str(), n) {
            ("translate", 1) => out.push(TransformFn::Translate(len(0, box_size.0)?, 0.0)),
            ("translate" | "translate3d", 2 | 3) => out.push(TransformFn::Translate(
                len(0, box_size.0)?,
                len(1, box_size.1)?,
            )),
            ("translatex", 1) => out.push(TransformFn::Translate(len(0, box_size.0)?, 0.0)),
            ("translatey", 1) => out.push(TransformFn::Translate(0.0, len(0, box_size.1)?)),
            ("translatez", 1) => out.push(TransformFn::Translate(0.0, 0.0)),
            ("scale", 1) => {
                let v = num(0)?;
                out.push(TransformFn::Scale(v, v));
            }
            ("scale" | "scale3d", 2 | 3) => out.push(TransformFn::Scale(num(0)?, num(1)?)),
            ("scalex", 1) => out.push(TransformFn::Scale(num(0)?, 1.0)),
            ("scaley", 1) => out.push(TransformFn::Scale(1.0, num(0)?)),
            ("rotate" | "rotatez", 1) => out.push(TransformFn::Rotate(ang(0)?)),
            ("rotate", 3) => {
                let (cx, cy) = (len(1, box_size.0)?, len(2, box_size.1)?);
                out.push(TransformFn::Translate(cx, cy));
                out.push(TransformFn::Rotate(ang(0)?));
                out.push(TransformFn::Translate(-cx, -cy));
            }
            ("skew", 1) => out.push(TransformFn::Skew(ang(0)?, 0.0)),
            ("skew", 2) => out.push(TransformFn::Skew(ang(0)?, ang(1)?)),
            ("skewx", 1) => out.push(TransformFn::Skew(ang(0)?, 0.0)),
            ("skewy", 1) => out.push(TransformFn::Skew(0.0, ang(0)?)),
            ("matrix", 6) => {
                let v: Vec<f64> = args.iter().map(|a| number(a)).collect::<Option<_>>()?;
                out.push(TransformFn::Matrix(Matrix::new(
                    v[0], v[1], v[2], v[3], v[4], v[5],
                )));
            }
            _ => return None,
        }
    }
    Some(out)
}

/// The product of a transform list.
pub(crate) fn list_matrix(list: &[TransformFn]) -> Matrix {
    list.iter().fold(Matrix::IDENTITY, |m, f| m * f.matrix())
}

/// How many times an animation runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Iterations {
    Count(f64),
    Infinite,
}

/// `animation-direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlayDirection {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

/// `animation-fill-mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FillMode {
    None,
    Forwards,
    Backwards,
    Both,
}

/// One CSS animation of an element.
#[derive(Debug, Clone)]
pub(crate) struct AnimationSpec {
    pub(crate) name: String,
    pub(crate) duration: f64,
    pub(crate) easing: Easing,
    pub(crate) delay: f64,
    pub(crate) iterations: Iterations,
    pub(crate) direction: PlayDirection,
    pub(crate) fill: FillMode,
}

impl Default for AnimationSpec {
    fn default() -> Self {
        AnimationSpec {
            name: String::new(),
            duration: 0.0,
            easing: Easing::EASE,
            delay: 0.0,
            iterations: Iterations::Count(1.0),
            direction: PlayDirection::Normal,
            fill: FillMode::None,
        }
    }
}

impl AnimationSpec {
    /// Seconds from the start of the timeline to the end of its last
    /// iteration (one period when endless).
    pub(crate) fn span(&self) -> f64 {
        match self.iterations {
            Iterations::Count(n) => self.delay.max(0.0) + self.duration * n.max(0.0),
            Iterations::Infinite => self.delay.max(0.0) + self.period(),
        }
    }

    /// Seconds after which it plays the same again: one iteration, two
    /// when it alternates (forwards, then backwards).
    pub(crate) fn period(&self) -> f64 {
        match self.direction {
            PlayDirection::Normal | PlayDirection::Reverse => self.duration,
            PlayDirection::Alternate | PlayDirection::AlternateReverse => 2.0 * self.duration,
        }
    }
}

fn direction(s: &str) -> Option<PlayDirection> {
    Some(match s {
        "normal" => PlayDirection::Normal,
        "reverse" => PlayDirection::Reverse,
        "alternate" => PlayDirection::Alternate,
        "alternate-reverse" => PlayDirection::AlternateReverse,
        _ => return None,
    })
}

fn fill_mode(s: &str) -> Option<FillMode> {
    Some(match s {
        "none" => FillMode::None,
        "forwards" => FillMode::Forwards,
        "backwards" => FillMode::Backwards,
        "both" => FillMode::Both,
        _ => return None,
    })
}

fn iterations(s: &str) -> Option<Iterations> {
    if s == "infinite" {
        return Some(Iterations::Infinite);
    }
    number(s).filter(|n| *n >= 0.0).map(Iterations::Count)
}

/// The animations of an `animation` shorthand (comma-separated).
pub(crate) fn animation_shorthand(value: &str) -> Vec<AnimationSpec> {
    split_top(value, ',')
        .into_iter()
        .filter_map(|one| {
            let mut spec = AnimationSpec::default();
            let mut times = 0;
            let mut name = None;
            let tokens: Vec<&str> = split_top(one.trim(), ' ')
                .into_iter()
                .filter(|t| !t.is_empty())
                .collect();
            let alone = tokens.len() == 1;
            for token in tokens {
                let lower = token.to_ascii_lowercase();
                if let Some(t) = time(&lower) {
                    if times == 0 {
                        spec.duration = t.max(0.0);
                    } else {
                        spec.delay = t;
                    }
                    times += 1;
                } else if let Some(e) = timing_function(token) {
                    spec.easing = e;
                } else if let Some(i) = iterations(&lower) {
                    spec.iterations = i;
                } else if let Some(d) = direction(&lower) {
                    spec.direction = d;
                } else if let Some(f) = fill_mode(&lower).filter(|_| !alone) {
                    // `none` is the fill mode unless it stands alone.
                    spec.fill = f;
                } else if lower != "running" && lower != "paused" {
                    name = Some(token.to_owned());
                }
            }
            spec.name = name?;
            (spec.name != "none").then_some(spec)
        })
        .collect()
}

/// Applies the `animation-*` longhand `name` with `value` to `specs`
/// (one entry per animation, repeated over the list as CSS does).
pub(crate) fn animation_longhand(specs: &mut Vec<AnimationSpec>, name: &str, value: &str) {
    let items: Vec<String> = split_top(value, ',')
        .into_iter()
        .map(|s| s.trim().to_owned())
        .collect();
    if name == "animation-name" {
        let mut out = Vec::new();
        for (i, item) in items.iter().enumerate() {
            if item == "none" {
                continue;
            }
            let mut spec = specs.get(i).cloned().unwrap_or_default();
            spec.name = item.clone();
            out.push(spec);
        }
        *specs = out;
        return;
    }
    for (i, spec) in specs.iter_mut().enumerate() {
        let Some(item) = items.get(i % items.len().max(1)) else {
            continue;
        };
        let lower = item.to_ascii_lowercase();
        match name {
            "animation-duration" => {
                if let Some(t) = time(&lower) {
                    spec.duration = t.max(0.0);
                }
            }
            "animation-delay" => {
                if let Some(t) = time(&lower) {
                    spec.delay = t;
                }
            }
            "animation-timing-function" => {
                if let Some(e) = timing_function(item) {
                    spec.easing = e;
                }
            }
            "animation-iteration-count" => {
                if let Some(n) = iterations(&lower) {
                    spec.iterations = n;
                }
            }
            "animation-direction" => {
                if let Some(d) = direction(&lower) {
                    spec.direction = d;
                }
            }
            "animation-fill-mode" => {
                if let Some(f) = fill_mode(&lower) {
                    spec.fill = f;
                }
            }
            _ => {}
        }
    }
}
