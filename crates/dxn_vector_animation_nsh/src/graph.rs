//! The animation graph a scene becomes: interpolations of the playback
//! time and arithmetic, as NSH's native graph has them. Kept as data, so
//! the same graph is evaluated in plain Rust (tests, the sampled fallback)
//! and lowered to NSH's animated values.

use std::collections::HashMap;

use dxn_vector_animation::{Animated, Color, Easing, Keyframe};

/// A curve within an interpolation's segments: what every NSH 0.2 graph
/// eases with. Holds and CSS `steps()` and `linear()` become breakpoints
/// of their own (straight lines and jumps a microsecond long).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Curve {
    /// Constant speed.
    Linear,
    /// CSS `cubic-bezier()`.
    Bezier(f64, f64, f64, f64),
}

impl Curve {
    /// The curve for `easing`; `None` for one made of breakpoints.
    pub fn of(easing: &Easing) -> Option<Curve> {
        match easing {
            Easing::Linear => Some(Curve::Linear),
            Easing::CubicBezier { x1, y1, x2, y2 } => Some(Curve::Bezier(*x1, *y1, *x2, *y2)),
            Easing::Hold | Easing::Steps { .. } | Easing::LinearPoints { .. } => None,
        }
    }

    /// The easing it is (to evaluate it).
    fn easing(self) -> Easing {
        match self {
            Curve::Linear => Easing::Linear,
            Curve::Bezier(x1, y1, x2, y2) => Easing::CubicBezier { x1, y1, x2, y2 },
        }
    }

    /// Whether its output stays within 0..1.
    fn bounded(self) -> bool {
        match self {
            Curve::Bezier(_, y1, _, y2) => (0.0..=1.0).contains(&y1) && (0.0..=1.0).contains(&y2),
            Curve::Linear => true,
        }
    }
}

/// How long a jump of a hold or a step lasts, in seconds.
const JUMP: f64 = 1e-6;

/// A node of the graph.
pub type NodeId = usize;

/// An operand of arithmetic: a node or a constant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Operand {
    /// A node.
    Node(NodeId),
    /// A constant.
    Const(f64),
}

/// Arithmetic of two values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathOp {
    /// `a + b`.
    Add,
    /// `a - b`.
    Sub,
    /// `a * b`.
    Mul,
    /// `a / b`.
    Div,
    /// `a` modulo `b`, never negative for positive `b`.
    Mod,
}

/// A node of the graph.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// The playback time, in seconds of the scene's timeline.
    Time,
    /// `input` through the breakpoints `inputs` (ascending) to `outputs`,
    /// along `curve` within each segment, clamped at both ends.
    Interp {
        /// What is mapped.
        input: NodeId,
        /// Breakpoints.
        inputs: Vec<f64>,
        /// Their values.
        outputs: Vec<f64>,
        /// The curve within each segment.
        curve: Curve,
    },
    /// The same to colors (`0xRRGGBBAA`, mixed channel by channel).
    InterpColor {
        /// What is mapped.
        input: NodeId,
        /// Breakpoints.
        inputs: Vec<f64>,
        /// Their colors.
        colors: Vec<u32>,
        /// The curve within each segment.
        curve: Curve,
    },
    /// Arithmetic.
    Math(MathOp, NodeId, Operand),
}

/// A number of a drawing: a constant or a node's value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Num {
    /// A constant.
    Const(f64),
    /// A node.
    Node(NodeId),
}

impl Num {
    /// Whether it is the constant `v`.
    pub fn is(self, v: f64) -> bool {
        matches!(self, Num::Const(c) if c == v)
    }
}

/// A color of a drawing: a constant or a node's value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Paint {
    /// A constant (`0xRRGGBBAA`).
    Const(u32),
    /// A color node.
    Node(NodeId),
}

/// The graph: nodes made once each (equal nodes are one).
#[derive(Debug, Clone)]
pub struct Graph {
    nodes: Vec<Expr>,
    index: HashMap<String, NodeId>,
}

impl Default for Graph {
    fn default() -> Self {
        let mut g = Graph {
            nodes: Vec::new(),
            index: HashMap::new(),
        };
        g.add(Expr::Time);
        g
    }
}

/// The node of the playback time.
pub const TIME: NodeId = 0;

/// How many points a nonlinear function of a track is sampled at, per
/// segment.
const SAMPLES_PER_SEGMENT: usize = 24;

/// How far from their corner [`Graph::above`] and [`Graph::below`] are
/// exact: lengths along outlines stay well within it, and the native graph
/// (single precision) still tells the corner from its ends.
pub const RAMP: f64 = 1.0e5;

/// How long the rise of a [`Graph::step`] is.
pub const STEP: f64 = 0.01;

impl Graph {
    /// Its nodes, in the order they were made (inputs before what reads
    /// them).
    pub fn nodes(&self) -> &[Expr] {
        &self.nodes
    }

    /// The node `expr` (made once).
    pub fn add(&mut self, expr: Expr) -> NodeId {
        let key = format!("{expr:?}");
        if let Some(&id) = self.index.get(&key) {
            return id;
        }
        let id = self.nodes.len();
        self.nodes.push(expr);
        self.index.insert(key, id);
        id
    }

    /// `a op b`, folding constants.
    pub fn math(&mut self, op: MathOp, a: Num, b: Num) -> Num {
        let apply = |x: f64, y: f64| match op {
            MathOp::Add => x + y,
            MathOp::Sub => x - y,
            MathOp::Mul => x * y,
            MathOp::Div if y != 0.0 => x / y,
            MathOp::Div => 0.0,
            MathOp::Mod if y != 0.0 => x.rem_euclid(y),
            MathOp::Mod => 0.0,
        };
        match (a, b) {
            (Num::Const(x), Num::Const(y)) => Num::Const(apply(x, y)),
            (Num::Node(_), Num::Const(0.0)) if matches!(op, MathOp::Add | MathOp::Sub) => a,
            (Num::Node(_), Num::Const(1.0)) if matches!(op, MathOp::Mul | MathOp::Div) => a,
            (Num::Const(0.0), Num::Node(_)) if op == MathOp::Add => b,
            (Num::Const(1.0), Num::Node(_)) if op == MathOp::Mul => b,
            (Num::Const(0.0), _) if op == MathOp::Mul => Num::Const(0.0),
            (_, Num::Const(0.0)) if op == MathOp::Mul => Num::Const(0.0),
            (Num::Node(x), Num::Const(y)) => {
                Num::Node(self.add(Expr::Math(op, x, Operand::Const(y))))
            }
            (Num::Node(x), Num::Node(y)) => {
                Num::Node(self.add(Expr::Math(op, x, Operand::Node(y))))
            }
            (Num::Const(x), Num::Node(y)) => match op {
                MathOp::Add | MathOp::Mul => {
                    Num::Node(self.add(Expr::Math(op, y, Operand::Const(x))))
                }
                // x - y = (y * -1) + x; x / y and x mod y need x as a node.
                MathOp::Sub => {
                    let neg = self.add(Expr::Math(MathOp::Mul, y, Operand::Const(-1.0)));
                    Num::Node(self.add(Expr::Math(MathOp::Add, neg, Operand::Const(x))))
                }
                MathOp::Div | MathOp::Mod => {
                    let xn = self.constant(x);
                    Num::Node(self.add(Expr::Math(op, xn, Operand::Node(y))))
                }
            },
        }
    }

    /// A node holding the constant `v` (for operations that need a node).
    fn constant(&mut self, v: f64) -> NodeId {
        self.add(Expr::Interp {
            input: TIME,
            inputs: vec![0.0],
            outputs: vec![v],
            curve: Curve::Linear,
        })
    }

    /// `x` through the breakpoints `inputs` (ascending) to `outputs`,
    /// straight between them, clamped at both ends.
    pub fn lookup(&mut self, x: Num, inputs: &[f64], outputs: &[f64]) -> Num {
        match x {
            Num::Const(v) => Num::Const(interpolate(
                v,
                inputs,
                Curve::Linear,
                |seg, t| outputs[seg] + (outputs[seg + 1] - outputs[seg]) * t,
                |i| outputs[i],
            )),
            Num::Node(input) => Num::Node(self.add(Expr::Interp {
                input,
                inputs: inputs.to_vec(),
                outputs: outputs.to_vec(),
                curve: Curve::Linear,
            })),
        }
    }

    /// `max(x - at, 0)`, for `x` within [`RAMP`] of `at`.
    pub fn above(&mut self, x: Num, at: f64) -> Num {
        self.lookup(x, &[at - RAMP, at, at + RAMP], &[0.0, 0.0, RAMP])
    }

    /// `max(at - x, 0)`, for `x` within [`RAMP`] of `at`.
    pub fn below(&mut self, x: Num, at: f64) -> Num {
        self.lookup(x, &[at - RAMP, at, at + RAMP], &[RAMP, 0.0, 0.0])
    }

    /// 1 where `x` is past `at` (beyond it when `up`, short of it when
    /// not), 0 where it is not, rising over [`STEP`].
    pub fn step(&mut self, x: Num, at: f64, up: bool) -> Num {
        if up {
            self.lookup(x, &[at, at + STEP], &[0.0, 1.0])
        } else {
            self.lookup(x, &[at - STEP, at], &[1.0, 0.0])
        }
    }

    /// The sum of `items`, as a balanced tree (the native graph evaluates
    /// recursively, to a fixed depth).
    pub fn sum(&mut self, items: &[Num]) -> Num {
        match items {
            [] => Num::Const(0.0),
            [one] => *one,
            _ => {
                let (a, b) = items.split_at(items.len() / 2);
                let (a, b) = (self.sum(a), self.sum(b));
                self.math(MathOp::Add, a, b)
            }
        }
    }

    /// Track `value` read at time node `time`, each value mapped by `f`
    /// (which must be linear for the result to be exact: scales, offsets).
    pub fn track(&mut self, value: &Animated<f64>, time: NodeId, f: impl Fn(f64) -> f64) -> Num {
        let keys = match value {
            Animated::Static(v) => return Num::Const(f(*v)),
            Animated::Track(t) => &t.keyframes,
        };
        match keys.as_slice() {
            [] => return Num::Const(f(0.0)),
            [one] => return Num::Const(f(one.value)),
            _ => {}
        }
        let times: Vec<f64> = keys.iter().map(|k| k.time).collect();
        let values: Vec<f64> = keys.iter().map(|k| f(k.value)).collect();
        let easings: Vec<&Easing> = keys[..keys.len() - 1].iter().map(|k| &k.easing).collect();
        // One curve throughout: one interpolation.
        if let Some(curve) = uniform(&easings) {
            return Num::Node(self.add(Expr::Interp {
                input: time,
                inputs: times,
                outputs: values,
                curve,
            }));
        }
        // Curves that stay within their segment share a warped time;
        // overshooting ones add each segment's change on its own.
        if easings.iter().all(|e| bounded(e)) {
            let warp = self.warp(&times, &easings, time);
            let index: Vec<f64> = (0..times.len()).map(|i| i as f64).collect();
            return Num::Node(self.add(Expr::Interp {
                input: warp,
                inputs: index,
                outputs: values,
                curve: Curve::Linear,
            }));
        }
        let mut parts = vec![Num::Const(values[0])];
        for i in 0..easings.len() {
            let delta = values[i + 1] - values[i];
            if delta == 0.0 {
                continue;
            }
            parts.push(self.segment(times[i], times[i + 1], easings[i], time, delta));
        }
        self.sum(&parts)
    }

    /// A color track read at `time`.
    pub fn color_track(&mut self, value: &Animated<Color>, time: NodeId, opacity: f64) -> Paint {
        let keys: &[Keyframe<Color>] = match value {
            Animated::Static(c) => return Paint::Const(pack(c.with_opacity(opacity))),
            Animated::Track(t) => &t.keyframes,
        };
        match keys {
            [] => return Paint::Const(0),
            [one] => return Paint::Const(pack(one.value.with_opacity(opacity))),
            _ => {}
        }
        let times: Vec<f64> = keys.iter().map(|k| k.time).collect();
        let colors: Vec<u32> = keys
            .iter()
            .map(|k| pack(k.value.with_opacity(opacity)))
            .collect();
        let easings: Vec<&Easing> = keys[..keys.len() - 1].iter().map(|k| &k.easing).collect();
        if let Some(curve) = uniform(&easings) {
            return Paint::Node(self.add(Expr::InterpColor {
                input: time,
                inputs: times,
                colors,
                curve,
            }));
        }
        let warp = self.warp(&times, &easings, time);
        let index: Vec<f64> = (0..times.len()).map(|i| i as f64).collect();
        Paint::Node(self.add(Expr::InterpColor {
            input: warp,
            inputs: index,
            colors,
            curve: Curve::Linear,
        }))
    }

    /// A value sampled from `f` over the keyframe times `times` (for
    /// nonlinear functions of tracks: `tan` of a skew, a radius from two
    /// points).
    pub fn sampled(&mut self, times: &[f64], time: NodeId, f: impl Fn(f64) -> f64) -> Num {
        let mut times = times.to_vec();
        times.sort_by(f64::total_cmp);
        times.dedup();
        if times.len() < 2 {
            return Num::Const(f(times.first().copied().unwrap_or(0.0)));
        }
        let mut inputs = Vec::new();
        for w in times.windows(2) {
            for k in 0..SAMPLES_PER_SEGMENT {
                inputs.push(w[0] + (w[1] - w[0]) * k as f64 / SAMPLES_PER_SEGMENT as f64);
            }
        }
        inputs.push(*times.last().unwrap_or(&0.0));
        let outputs = inputs.iter().map(|&t| f(t)).collect();
        Num::Node(self.add(Expr::Interp {
            input: time,
            inputs,
            outputs,
            curve: Curve::Linear,
        }))
    }

    /// Time warped so each keyframe segment's easing applies: 0 at the
    /// first keyframe, 1 at the second, ... (shared by tracks of the same
    /// keyframe times and easings).
    fn warp(&mut self, times: &[f64], easings: &[&Easing], time: NodeId) -> NodeId {
        let parts: Vec<Num> = (0..easings.len())
            .map(|i| self.segment(times[i], times[i + 1], easings[i], time, 1.0))
            .collect();
        match self.sum(&parts) {
            Num::Node(n) => n,
            Num::Const(c) => self.constant(c),
        }
    }

    /// 0 before segment `a`..`b`, `delta` after it, eased between.
    fn segment(&mut self, a: f64, b: f64, easing: &Easing, time: NodeId, delta: f64) -> Num {
        if b <= a {
            // A jump at `a`.
            return Num::Node(self.add(Expr::Interp {
                input: time,
                inputs: vec![a, a + 1e-6],
                outputs: vec![0.0, delta],
                curve: Curve::Linear,
            }));
        }
        let span = b - a;
        let (inputs, outputs, curve) = match (easing, Curve::of(easing)) {
            (_, Some(curve)) => (vec![a, b], vec![0.0, delta], curve),
            (Easing::LinearPoints { points }, None) => {
                let mut ins: Vec<f64> = points.iter().map(|(x, _)| a + span * x).collect();
                let outs: Vec<f64> = points.iter().map(|(_, y)| y * delta).collect();
                for i in 1..ins.len() {
                    ins[i] = ins[i].max(ins[i - 1]);
                }
                (ins, outs, Curve::Linear)
            }
            (Easing::Steps { count, .. }, None) => {
                // The level at the start, then each jump at its boundary.
                let count = (*count).clamp(1, 1000);
                let mut ins = vec![a];
                let mut outs = vec![easing.apply(0.0) * delta];
                for k in 1..=count {
                    let x = f64::from(k) / f64::from(count);
                    let at = a + span * x;
                    ins.push(at - JUMP.min(span / f64::from(count) / 2.0));
                    outs.push(easing.apply(x - 1e-9) * delta);
                    ins.push(at);
                    outs.push(easing.apply(x) * delta);
                }
                (ins, outs, Curve::Linear)
            }
            // A hold: the start until the next keyframe, then its value.
            _ => (
                vec![a, b - JUMP.min(span / 2.0), b],
                vec![0.0, 0.0, delta],
                Curve::Linear,
            ),
        };
        Num::Node(self.add(Expr::Interp {
            input: time,
            inputs,
            outputs,
            curve,
        }))
    }

    /// The value of node `id` at playback time `time`.
    pub fn eval(&self, id: NodeId, time: f64) -> f64 {
        self.eval_depth(id, time, 0)
    }

    fn eval_depth(&self, id: NodeId, time: f64, depth: u32) -> f64 {
        if depth > 64 {
            return 0.0;
        }
        match self.nodes.get(id) {
            Some(Expr::Time) => time,
            Some(Expr::Interp {
                input,
                inputs,
                outputs,
                curve,
            }) => {
                let x = self.eval_depth(*input, time, depth + 1);
                interpolate(
                    x,
                    inputs,
                    *curve,
                    |seg, t| outputs[seg] + (outputs[seg + 1] - outputs[seg]) * t,
                    |i| outputs[i],
                )
            }
            Some(Expr::InterpColor { .. }) => 0.0,
            Some(Expr::Math(op, a, b)) => {
                let a = self.eval_depth(*a, time, depth + 1);
                let b = match b {
                    Operand::Node(n) => self.eval_depth(*n, time, depth + 1),
                    Operand::Const(c) => *c,
                };
                match op {
                    MathOp::Add => a + b,
                    MathOp::Sub => a - b,
                    MathOp::Mul => a * b,
                    MathOp::Div if b != 0.0 => a / b,
                    MathOp::Mod if b != 0.0 => a.rem_euclid(b),
                    _ => 0.0,
                }
            }
            None => 0.0,
        }
    }

    /// The color of node `id` at playback time `time`.
    pub fn eval_color(&self, id: NodeId, time: f64) -> u32 {
        match self.nodes.get(id) {
            Some(Expr::InterpColor {
                input,
                inputs,
                colors,
                curve,
            }) => {
                let x = self.eval(*input, time);
                interpolate(
                    x,
                    inputs,
                    *curve,
                    |seg, t| lerp_color(colors[seg], colors[seg + 1], t.clamp(0.0, 1.0)),
                    |i| colors[i],
                )
            }
            _ => 0,
        }
    }

    /// `n` at `time`.
    pub fn num(&self, n: Num, time: f64) -> f64 {
        match n {
            Num::Const(c) => c,
            Num::Node(id) => self.eval(id, time),
        }
    }

    /// `p` at `time`.
    pub fn paint(&self, p: Paint, time: f64) -> u32 {
        match p {
            Paint::Const(c) => c,
            Paint::Node(id) => self.eval_color(id, time),
        }
    }
}

/// The curve all `easings` share, when they share one NSH has.
fn uniform(easings: &[&Easing]) -> Option<Curve> {
    let first = Curve::of(easings.first()?)?;
    easings
        .iter()
        .all(|e| Curve::of(e) == Some(first))
        .then_some(first)
}

fn bounded(e: &Easing) -> bool {
    match Curve::of(e) {
        Some(c) => c.bounded(),
        None => match e {
            Easing::LinearPoints { points } => points.iter().all(|(_, y)| (0.0..=1.0).contains(y)),
            _ => true,
        },
    }
}

/// NSH's interpolation: the segment holding `x` (an end one clamps), the
/// curve within it.
fn interpolate<T>(
    x: f64,
    inputs: &[f64],
    curve: Curve,
    mix: impl Fn(usize, f64) -> T,
    at: impl Fn(usize) -> T,
) -> T {
    let n = inputs.len();
    if n == 1 || x <= inputs[0] {
        return at(0);
    }
    if x >= inputs[n - 1] {
        return at(n - 1);
    }
    let seg = inputs.windows(2).position(|w| x <= w[1]).unwrap_or(n - 2);
    let (a, b) = (inputs[seg], inputs[seg + 1]);
    let t = if b == a { 0.0 } else { (x - a) / (b - a) };
    let t = if curve == Curve::Linear {
        t
    } else {
        curve.easing().apply(t)
    };
    mix(seg, t)
}

/// A color as `0xRRGGBBAA`.
pub fn pack(c: Color) -> u32 {
    let c = c.clamped();
    let b = |v: f64| (v * 255.0).round() as u32;
    (b(c.r) << 24) | (b(c.g) << 16) | (b(c.b) << 8) | b(c.a)
}

/// The color of `0xRRGGBBAA`.
pub fn unpack(c: u32) -> Color {
    Color::rgba8((c >> 24) as u8, (c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn lerp_color(a: u32, b: u32, t: f64) -> u32 {
    let mut out = 0u32;
    for shift in [24, 16, 8, 0] {
        let ca = f64::from((a >> shift) & 0xff);
        let cb = f64::from((b >> shift) & 0xff);
        out |= ((ca + (cb - ca) * t).round() as u32 & 0xff) << shift;
    }
    out
}
