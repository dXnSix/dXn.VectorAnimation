//! Values of expressions and what JavaScript and After Effects do with them:
//! conversions, operators on numbers and arrays (as Bodymovin's `$bm_sum`
//! and its siblings do), vector math, random numbers and noise.

use std::{cmp::Ordering, rc::Rc};

use super::syntax::Func;

/// The most elements an array holds.
pub(super) const MAX_ARRAY: usize = 1 << 16;
/// The longest string, in bytes.
pub(super) const MAX_STRING: usize = 1 << 16;
/// How deep arrays nest in arrays.
const MAX_NESTING: u8 = 16;

/// A layer: its composition and its place in the composition's `layers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct LayerId {
    pub(super) comp: usize,
    pub(super) index: usize,
}

/// An object of After Effects' expression API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Host {
    Math,
    Comp(usize),
    Layer(LayerId),
    Transform(LayerId),
    Effects(LayerId),
    /// An effect of a layer, by its place in the layer's `ef`.
    Effect(LayerId, usize),
    /// A property, by its id in the engine.
    Prop(usize),
    /// A keyframe of a property, from 0.
    Key(usize, usize),
}

/// An array's elements, and how deep arrays nest in it (1 for an array of
/// numbers).
#[derive(Debug, Clone, Default)]
pub(super) struct List {
    pub(super) items: Vec<Val>,
    pub(super) depth: u8,
}

#[derive(Debug, Clone)]
pub(super) enum Val {
    Undef,
    Null,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Arr(Rc<List>),
    Func(Rc<Func>),
    /// A built-in function, by name.
    Native(&'static str),
    /// A method of a value, taken without calling it.
    Bound(Rc<(Val, Rc<str>)>),
    Host(Host),
}

pub(super) type Res<T> = Result<T, String>;

impl Val {
    pub(super) fn str(s: impl Into<Rc<str>>) -> Res<Val> {
        let s = s.into();
        if s.len() > MAX_STRING {
            return Err("a string over 64 KB".into());
        }
        Ok(Val::Str(s))
    }

    pub(super) fn list(items: Vec<Val>) -> Res<Val> {
        if items.len() > MAX_ARRAY {
            return Err("an array of over 65536 elements".into());
        }
        let depth = items.iter().map(nesting).max().unwrap_or(0) + 1;
        if depth > MAX_NESTING {
            return Err("arrays nested too deeply".into());
        }
        Ok(Val::Arr(Rc::new(List { items, depth })))
    }

    /// Numbers as a number (one) or an array.
    pub(super) fn numbers(v: &[f64], array: bool) -> Val {
        match v {
            [n] if !array => Val::Num(*n),
            _ => Val::Arr(Rc::new(List {
                items: v.iter().map(|n| Val::Num(*n)).collect(),
                depth: 1,
            })),
        }
    }

    pub(super) fn truthy(&self) -> bool {
        match self {
            Val::Undef | Val::Null => false,
            Val::Bool(b) => *b,
            Val::Num(n) => *n != 0.0 && !n.is_nan(),
            Val::Str(s) => !s.is_empty(),
            _ => true,
        }
    }

    pub(super) fn type_of(&self) -> &'static str {
        match self {
            Val::Undef => "undefined",
            Val::Bool(_) => "boolean",
            Val::Num(_) => "number",
            Val::Str(_) => "string",
            Val::Func(_) | Val::Native(_) | Val::Bound(_) => "function",
            Val::Null | Val::Arr(_) | Val::Host(_) => "object",
        }
    }

    /// The elements of an array (none for anything else).
    pub(super) fn items(&self) -> &[Val] {
        match self {
            Val::Arr(l) => &l.items,
            _ => &[],
        }
    }

    /// The work an operation on this value does: its elements.
    pub(super) fn cost(&self) -> u64 {
        match self {
            Val::Arr(l) => l.items.len() as u64,
            Val::Str(s) => s.len() as u64 / 16,
            _ => 0,
        }
    }
}

/// How deep arrays nest in `v` (0 for anything but an array).
pub(super) fn nesting(v: &Val) -> u8 {
    match v {
        Val::Arr(l) => l.depth,
        _ => 0,
    }
}

/// Sets element `i` of `list` (growing it by one at its end) to `v`;
/// `levels` arrays enclose `list`.
pub(super) fn set_item(list: &mut List, i: f64, v: Val, levels: u8) -> Res<()> {
    let depth = nesting(&v).saturating_add(1);
    if depth.saturating_add(levels) > MAX_NESTING {
        return Err("arrays nested too deeply".into());
    }
    if i.fract() != 0.0 || i < 0.0 || i > list.items.len() as f64 {
        return Err("an element set past the end of its array".into());
    }
    let i = i as usize;
    if i == list.items.len() {
        if i >= MAX_ARRAY {
            return Err("an array of over 65536 elements".into());
        }
        list.items.push(v);
    } else {
        list.items[i] = v;
    }
    list.depth = list.depth.max(depth);
    Ok(())
}

pub(super) fn to_number(v: &Val) -> f64 {
    match v {
        Val::Undef => f64::NAN,
        Val::Null => 0.0,
        Val::Bool(b) => f64::from(u8::from(*b)),
        Val::Num(n) => *n,
        Val::Str(s) => {
            let s = s.trim();
            if s.is_empty() {
                0.0
            } else if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16).map_or(f64::NAN, |n| n as f64)
            } else {
                match s {
                    "Infinity" | "+Infinity" => f64::INFINITY,
                    "-Infinity" => f64::NEG_INFINITY,
                    _ if s.contains(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E') => {
                        f64::NAN
                    }
                    _ => s.parse().unwrap_or(f64::NAN),
                }
            }
        }
        Val::Arr(l) => match l.items.as_slice() {
            [] => 0.0,
            [one] => to_number(one),
            _ => f64::NAN,
        },
        _ => f64::NAN,
    }
}

/// JavaScript's `ToInt32`.
pub(super) fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    (n.trunc().rem_euclid(4_294_967_296.0) as u32) as i32
}

pub(super) fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else if n == 0.0 {
        "0".into()
    } else {
        format!("{n}")
    }
}

/// JavaScript's `String(v)`, cut off past [`MAX_STRING`].
pub(super) fn to_string(v: &Val) -> String {
    let mut out = String::new();
    let mut budget = MAX_STRING;
    write_string(v, &mut out, &mut budget);
    out
}

fn write_string(v: &Val, out: &mut String, budget: &mut usize) {
    if *budget == 0 || out.len() > MAX_STRING {
        return;
    }
    *budget -= 1;
    match v {
        Val::Undef => out.push_str("undefined"),
        Val::Null => out.push_str("null"),
        Val::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Val::Num(n) => out.push_str(&number_to_string(*n)),
        Val::Str(s) => out.push_str(s),
        Val::Arr(l) => {
            for (i, item) in l.items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if !matches!(item, Val::Undef | Val::Null) {
                    write_string(item, out, budget);
                }
            }
        }
        Val::Func(_) | Val::Native(_) | Val::Bound(_) => out.push_str("function"),
        Val::Host(_) => out.push_str("[object]"),
    }
}

fn numeric(v: &Val) -> bool {
    !matches!(v, Val::Arr(_) | Val::Str(_))
}

fn elementwise(a: &[Val], b: &[Val], f: impl Fn(f64, f64) -> f64) -> Res<Val> {
    let len = a.len().max(b.len());
    Val::list(
        (0..len)
            .map(|i| {
                let x = a.get(i).map_or(0.0, to_number);
                let y = b.get(i).map_or(0.0, to_number);
                Val::Num(f(x, y))
            })
            .collect(),
    )
}

fn each(a: &[Val], f: impl Fn(f64) -> f64) -> Res<Val> {
    Val::list(a.iter().map(|x| Val::Num(f(to_number(x)))).collect())
}

/// The first element changed by `f` (Bodymovin adds a number to an array's
/// first element).
fn first(a: &[Val], f: impl Fn(f64) -> f64) -> Res<Val> {
    let mut items = a.to_vec();
    if let Some(x) = items.first_mut() {
        *x = Val::Num(f(to_number(x)));
    }
    Val::list(items)
}

/// `a + b`: strings joined, arrays added element by element.
pub(super) fn sum(a: &Val, b: &Val) -> Res<Val> {
    match (a, b) {
        (Val::Str(_), _) | (_, Val::Str(_)) => {
            let mut s = to_string(a);
            s.push_str(&to_string(b));
            Val::str(s)
        }
        (Val::Arr(x), Val::Arr(y)) => elementwise(&x.items, &y.items, |p, q| p + q),
        (Val::Arr(x), n) => first(&x.items, |p| p + to_number(n)),
        (n, Val::Arr(y)) => first(&y.items, |q| to_number(n) + q),
        _ => Ok(Val::Num(to_number(a) + to_number(b))),
    }
}

/// `a - b`, arrays element by element.
pub(super) fn sub(a: &Val, b: &Val) -> Res<Val> {
    match (a, b) {
        (Val::Arr(x), Val::Arr(y)) => elementwise(&x.items, &y.items, |p, q| p - q),
        (Val::Arr(x), n) => first(&x.items, |p| p - to_number(n)),
        (n, Val::Arr(y)) => first(&y.items, |q| to_number(n) - q),
        _ => Ok(Val::Num(to_number(a) - to_number(b))),
    }
}

/// `a * b`: an array scaled by a number.
pub(super) fn mul(a: &Val, b: &Val) -> Res<Val> {
    match (a, b) {
        (Val::Arr(_), Val::Arr(_)) => Err("arrays multiplied by arrays".into()),
        (Val::Arr(x), n) | (n, Val::Arr(x)) if numeric(n) => {
            let n = to_number(n);
            each(&x.items, |p| p * n)
        }
        _ => Ok(Val::Num(to_number(a) * to_number(b))),
    }
}

/// `a / b`: an array divided by a number.
pub(super) fn div(a: &Val, b: &Val) -> Res<Val> {
    match (a, b) {
        (Val::Arr(_), Val::Arr(_)) => Err("arrays divided by arrays".into()),
        (Val::Arr(x), n) if numeric(n) => {
            let n = to_number(n);
            each(&x.items, |p| p / n)
        }
        (n, Val::Arr(y)) if numeric(n) => {
            let n = to_number(n);
            each(&y.items, |q| n / q)
        }
        _ => Ok(Val::Num(to_number(a) / to_number(b))),
    }
}

pub(super) fn rem(a: &Val, b: &Val) -> Res<Val> {
    if matches!(a, Val::Arr(_)) || matches!(b, Val::Arr(_)) {
        return Err("the remainder (`%`) of arrays".into());
    }
    Ok(Val::Num(to_number(a) % to_number(b)))
}

pub(super) fn neg(a: &Val) -> Res<Val> {
    match a {
        Val::Arr(x) => each(&x.items, |p| -p),
        _ => Ok(Val::Num(-to_number(a))),
    }
}

/// `a == b`.
pub(super) fn loose_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Undef | Val::Null, Val::Undef | Val::Null) => true,
        (Val::Undef | Val::Null, _) | (_, Val::Undef | Val::Null) => false,
        (Val::Str(x), Val::Str(y)) => x == y,
        (Val::Arr(_), Val::Arr(_))
        | (Val::Func(_), _)
        | (_, Val::Func(_))
        | (Val::Native(_), _)
        | (_, Val::Native(_))
        | (Val::Bound(_), _)
        | (_, Val::Bound(_))
        | (Val::Host(_), _)
        | (_, Val::Host(_)) => strict_eq(a, b),
        (Val::Arr(_), Val::Str(_)) | (Val::Str(_), Val::Arr(_)) => to_string(a) == to_string(b),
        _ => to_number(a) == to_number(b),
    }
}

/// `a === b`.
pub(super) fn strict_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Undef, Val::Undef) | (Val::Null, Val::Null) => true,
        (Val::Bool(x), Val::Bool(y)) => x == y,
        (Val::Num(x), Val::Num(y)) => x == y,
        (Val::Str(x), Val::Str(y)) => x == y,
        (Val::Arr(x), Val::Arr(y)) => Rc::ptr_eq(x, y),
        (Val::Func(x), Val::Func(y)) => Rc::ptr_eq(x, y),
        (Val::Native(x), Val::Native(y)) => x == y,
        (Val::Bound(x), Val::Bound(y)) => Rc::ptr_eq(x, y),
        (Val::Host(x), Val::Host(y)) => x == y,
        _ => false,
    }
}

/// How `a` compares with `b` for `<` and its siblings (none when either is
/// not a number).
pub(super) fn compare(a: &Val, b: &Val) -> Option<Ordering> {
    match (a, b) {
        (Val::Str(x), Val::Str(y)) => Some(x.cmp(y)),
        _ => to_number(a).partial_cmp(&to_number(b)),
    }
}

/// Numbers of a number or an array of numbers.
pub(super) fn to_numbers(v: &Val) -> Option<Vec<f64>> {
    match v {
        Val::Num(n) => Some(vec![*n]),
        Val::Bool(b) => Some(vec![f64::from(u8::from(*b))]),
        Val::Arr(l) => l
            .items
            .iter()
            .map(|x| match x {
                Val::Num(n) => Some(*n),
                Val::Bool(b) => Some(f64::from(u8::from(*b))),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// `length(a, b)`: the distance between two points (from the origin
/// without `b`), or between two numbers.
pub(super) fn length(a: &Val, b: Option<&Val>) -> f64 {
    match a {
        Val::Arr(x) => {
            let y = b.map(Val::items).unwrap_or(&[]);
            let len = if b.is_some() {
                x.items.len().min(y.len())
            } else {
                x.items.len()
            };
            (0..len)
                .map(|i| {
                    let d = y.get(i).map_or(0.0, to_number) - to_number(&x.items[i]);
                    d * d
                })
                .sum::<f64>()
                .sqrt()
        }
        _ => (to_number(a) - b.map_or(0.0, to_number)).abs(),
    }
}

pub(super) fn dot(a: &Val, b: &Val) -> f64 {
    match (a, b) {
        (Val::Arr(x), Val::Arr(y)) => x
            .items
            .iter()
            .zip(&y.items)
            .map(|(p, q)| to_number(p) * to_number(q))
            .sum(),
        _ => to_number(a) * to_number(b),
    }
}

pub(super) fn cross(a: &Val, b: &Val) -> Res<Val> {
    let c = |v: &Val, i: usize| v.items().get(i).map_or(0.0, to_number);
    let (a0, a1, a2) = (c(a, 0), c(a, 1), c(a, 2));
    let (b0, b1, b2) = (c(b, 0), c(b, 1), c(b, 2));
    Ok(Val::numbers(
        &[a1 * b2 - a2 * b1, a2 * b0 - a0 * b2, a0 * b1 - a1 * b0],
        true,
    ))
}

/// `clamp(v, a, b)`: within the limits (either way round), element by
/// element for arrays.
pub(super) fn clamp(v: &Val, a: &Val, b: &Val) -> Res<Val> {
    let limit = |l: &Val, i: usize| match l {
        Val::Arr(x) => x.items.get(i).map_or(f64::NAN, to_number),
        _ => to_number(l),
    };
    let one = |x: f64, i: usize| {
        let (lo, hi) = (limit(a, i), limit(b, i));
        let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) };
        x.max(lo).min(hi)
    };
    match v {
        Val::Arr(x) => Val::list(
            x.items
                .iter()
                .enumerate()
                .map(|(i, p)| Val::Num(one(to_number(p), i)))
                .collect(),
        ),
        _ => Ok(Val::Num(one(to_number(v), 0))),
    }
}

/// `a` to `b` at `f` (0 to 1), element by element for arrays.
pub(super) fn mix(a: &Val, b: &Val, f: f64) -> Res<Val> {
    let one = |x: f64, y: f64| x + (y - x) * f;
    match (a, b) {
        (Val::Arr(x), Val::Arr(y)) => Val::list(
            x.items
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let p = to_number(p);
                    Val::Num(one(p, y.items.get(i).map_or(p, to_number)))
                })
                .collect(),
        ),
        (Val::Arr(x), n) => {
            let n = to_number(n);
            each(&x.items, |p| one(p, n))
        }
        (n, Val::Arr(y)) => {
            let n = to_number(n);
            each(&y.items, |q| one(n, q))
        }
        _ => Ok(Val::Num(one(to_number(a), to_number(b)))),
    }
}

/// `f` of a number, or of each element of an array.
pub(super) fn map_numbers(v: &Val, f: impl Fn(f64) -> f64) -> Res<Val> {
    match v {
        Val::Arr(x) => each(&x.items, f),
        _ => Ok(Val::Num(f(to_number(v)))),
    }
}

/// `rgbToHsl([r, g, b, a])`, each 0 to 1.
pub(super) fn rgb_to_hsl(v: &Val) -> Val {
    let c = |i: usize| v.items().get(i).map_or(0.0, to_number);
    let (r, g, b, a) = (c(0), c(1), c(2), v.items().get(3).map_or(1.0, to_number));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let (mut h, mut s) = (0.0, 0.0);
    if max != min {
        let d = max - min;
        s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        } / 6.0;
    }
    Val::numbers(&[h, s, l, a], true)
}

/// `hslToRgb([h, s, l, a])`, each 0 to 1.
pub(super) fn hsl_to_rgb(v: &Val) -> Val {
    let c = |i: usize| v.items().get(i).map_or(0.0, to_number);
    let (h, s, l, a) = (c(0), c(1), c(2), v.items().get(3).map_or(1.0, to_number));
    if s == 0.0 {
        return Val::numbers(&[l, l, l, a], true);
    }
    let hue = |p: f64, q: f64, t: f64| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    Val::numbers(
        &[
            hue(p, q, h + 1.0 / 3.0),
            hue(p, q, h),
            hue(p, q, h - 1.0 / 3.0),
            a,
        ],
        true,
    )
}

/// `hexToRgb("#rrggbb")` (or `"#rrggbbaa"`, `"rgb"`), each 0 to 1.
pub(super) fn hex_to_rgb(s: &str) -> Option<Val> {
    let hex = s.trim().trim_start_matches('#').trim_start_matches("0x");
    let digits: Vec<u32> = hex.chars().map(|c| c.to_digit(16)).collect::<Option<_>>()?;
    let pairs: Vec<f64> = match digits.len() {
        3 | 4 => digits.iter().map(|d| f64::from(d * 17) / 255.0).collect(),
        6 | 8 => digits
            .chunks(2)
            .map(|p| f64::from(p[0] * 16 + p[1]) / 255.0)
            .collect(),
        _ => return None,
    };
    let mut rgba = pairs;
    if rgba.len() == 3 {
        rgba.push(1.0);
    }
    Some(Val::numbers(&rgba, true))
}

/// SplitMix64's mixing: a well spread hash of `z`.
pub(super) fn mix64(z: u64) -> u64 {
    let mut z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// FNV-1a of `s`: a seed that stays the same from run to run.
pub(super) fn hash_str(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Deterministic random numbers.
#[derive(Debug, Clone)]
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(seed: u64) -> Rng {
        Rng(mix64(seed))
    }

    /// A number in 0..1.
    pub(super) fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        (mix64(self.0) >> 11) as f64 / (1_u64 << 53) as f64
    }

    /// A normally distributed number, nine in ten within -1..1 (After
    /// Effects' `gaussRandom` spread).
    pub(super) fn gauss(&mut self) -> f64 {
        let u = self.next().max(f64::MIN_POSITIVE);
        let v = self.next();
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos() / 1.645
    }
}

/// A lattice value of noise `seed` at `i`, in -1..1.
fn lattice(seed: u64, i: i64) -> f64 {
    let h = mix64(seed ^ (i as u64).wrapping_mul(0xd6e8_feb8_6659_fd93));
    (h >> 11) as f64 / (1_u64 << 52) as f64 - 1.0
}

/// Smooth noise in -1..1: values and gradients on whole numbers, eased
/// between them, the same for the same `seed` and `x`.
pub(super) fn noise(seed: u64, x: f64) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    let cell = x.floor();
    let f = x - cell;
    let i = cell as i64;
    let fade = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let (a, b) = (lattice(seed, i), lattice(seed, i.wrapping_add(1)));
    let value = a + (b - a) * fade;
    let gradient_seed = seed ^ 0x5851_f42d_4c95_7f2d;
    let (ga, gb) = (
        lattice(gradient_seed, i) * f,
        lattice(gradient_seed, i.wrapping_add(1)) * (f - 1.0),
    );
    // Gradient noise stays within half its gradients.
    let gradient = 2.0 * (ga + (gb - ga) * fade);
    (0.5 * (value + gradient)).clamp(-1.0, 1.0)
}

/// Noise of several octaves, each twice the frequency of the one before
/// and `amp_mult` times its amplitude, scaled to stay within -1..1.
pub(super) fn fractal(seed: u64, x: f64, octaves: f64, amp_mult: f64) -> f64 {
    let octaves = if octaves.is_finite() {
        octaves.floor().clamp(1.0, 16.0) as u32
    } else {
        1
    };
    let amp_mult = if amp_mult.is_finite() { amp_mult } else { 0.5 };
    let (mut sum, mut weight, mut amp, mut freq) = (0.0, 0.0, 1.0, 1.0);
    for octave in 0..octaves {
        sum += amp * noise(mix64(seed ^ u64::from(octave)), x * freq);
        weight += f64::abs(amp);
        amp *= amp_mult;
        freq *= 2.0;
    }
    if weight > 1.0 { sum / weight } else { sum }
}

/// Smooth noise of a point in up to three dimensions, in -1..1 (After
/// Effects' `noise`).
pub(super) fn noise_point(p: &[f64]) -> f64 {
    let p: Vec<f64> = p.iter().take(3).copied().collect();
    if p.iter().any(|x| !x.is_finite()) {
        return 0.0;
    }
    if p.len() <= 1 {
        return noise(0, p.first().copied().unwrap_or(0.0));
    }
    let cells: Vec<i64> = p.iter().map(|x| x.floor() as i64).collect();
    let fades: Vec<f64> = p
        .iter()
        .map(|x| {
            let f = x - x.floor();
            f * f * f * (f * (f * 6.0 - 15.0) + 10.0)
        })
        .collect();
    let mut total = 0.0;
    for corner in 0..(1_usize << p.len()) {
        let mut h = 0x2545_f491_4f6c_dd1d_u64;
        let mut weight = 1.0;
        for (d, (&cell, &fade)) in cells.iter().zip(&fades).enumerate() {
            let up = (corner >> d) & 1 == 1;
            let c = if up { cell.wrapping_add(1) } else { cell };
            h = mix64(h ^ (c as u64));
            weight *= if up { fade } else { 1.0 - fade };
        }
        total += weight * ((h >> 11) as f64 / (1_u64 << 52) as f64 - 1.0);
    }
    total.clamp(-1.0, 1.0)
}
