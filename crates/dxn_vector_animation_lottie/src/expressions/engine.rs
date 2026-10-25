//! The interpreter: statements, operators, variables, functions and the
//! built-in functions of JavaScript and After Effects that need no
//! document. Every step is counted against a budget and recursion is
//! capped, so no expression runs forever or deep.

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use dxn_vector_animation::Easing;
use serde_json::Value;

use super::{
    keys::Keys,
    syntax::{BinOp, Expr, Func, Kind, Logic, Stmt, UnOp, short},
    value::{
        self, Host, LayerId, List, Rng, Val, compare, loose_eq, number_to_string, strict_eq,
        to_int32, to_number, to_string,
    },
};

/// Steps one property's expression may take over all its frames.
pub(super) const PROPERTY_STEPS: u64 = 4_000_000;
/// Steps all expressions of a file may take together.
pub(super) const TOTAL_STEPS: u64 = 40_000_000;
/// How deep evaluation recurses: statements, expressions and calls.
const MAX_DEPTH: usize = 256;
/// How deep function calls nest.
const MAX_CALLS: usize = 32;

/// Why an expression is left out.
#[derive(Debug, Clone)]
pub(super) enum Fail {
    /// It cannot be read: what was not understood.
    Syntax(String),
    /// It uses something not supported here.
    Unsupported(String),
    /// Its property's values are not numbers (paths, text documents).
    Kind(&'static str),
    /// It fails when it runs.
    Error(String),
    /// It reads an expression that reads it.
    Cycle,
    /// It ran out of steps.
    Budget,
    /// Its keyframes would go over the limit.
    Keyframes,
}

impl From<String> for Fail {
    fn from(message: String) -> Fail {
        Fail::Error(message)
    }
}

pub(super) type R<T> = Result<T, Fail>;

pub(super) fn unsupported(name: &str) -> Fail {
    Fail::Unsupported(format!("`{}`", short(name)))
}

pub(super) fn error(message: impl Into<String>) -> Fail {
    Fail::Error(message.into())
}

/// A composition: the root or a precomposition asset.
pub(super) struct Comp<'a> {
    pub(super) layers: &'a [Value],
    /// The JSON pointer of the object holding `layers`.
    pub(super) pointer: String,
    pub(super) name: &'a str,
    pub(super) width: f64,
    pub(super) height: f64,
}

/// Where a property's values come from.
pub(super) enum Src<'a> {
    /// An animatable property object (`{"a", "k", "x"?}`).
    Json(&'a Value),
    /// A position split into `x`, `y` (and `z`) properties.
    Split(&'a Value),
    /// A transform property the file leaves out: its default.
    Const(Vec<f64>),
}

pub(super) struct PropInfo<'a> {
    pub(super) src: Src<'a>,
    pub(super) layer: LayerId,
    pub(super) pointer: String,
    /// Seeds its wiggles and random numbers.
    pub(super) seed: u64,
    pub(super) keys: Option<Result<Rc<Keys>, Fail>>,
    pub(super) program: Option<Result<Rc<Vec<Stmt>>, Fail>>,
}

/// The document's compositions and properties, and the state of the
/// expressions evaluated so far.
pub(super) struct Engine<'a> {
    pub(super) fr: f64,
    /// The root composition's in and out points (frames).
    pub(super) root_span: (f64, f64),
    pub(super) comps: Vec<Comp<'a>>,
    pub(super) props: Vec<PropInfo<'a>>,
    pub(super) by_pointer: HashMap<String, usize>,
    /// Values of expressions read by other expressions, by property and time.
    pub(super) cache: HashMap<(usize, u64), Val>,
    /// Properties whose expressions failed: read by their keyframes.
    pub(super) failed: HashSet<usize>,
    /// The properties whose expressions are running, innermost last.
    pub(super) stack: Vec<usize>,
    pub(super) steps: u64,
    pub(super) limit: u64,
    pub(super) depth: usize,
}

/// One run of an expression: its property, time and variables.
pub(super) struct Run {
    pub(super) prop: usize,
    pub(super) layer: LayerId,
    /// Composition seconds.
    pub(super) time: f64,
    /// The property's value at `time`, before the expression.
    pub(super) value: Val,
    vars: HashMap<Rc<str>, Val>,
    /// Variables of the functions being called, innermost last.
    frames: Vec<HashMap<Rc<str>, Val>>,
    pub(super) rng: Rng,
    pub(super) seed: u64,
    /// The value of the last expression statement at the top level.
    completion: Val,
}

impl Run {
    fn get(&self, name: &str) -> Option<&Val> {
        self.frames
            .last()
            .and_then(|f| f.get(name))
            .or_else(|| self.vars.get(name))
    }

    fn declare(&mut self, name: &Rc<str>, v: Val) {
        match self.frames.last_mut() {
            Some(f) => f.insert(name.clone(), v),
            None => self.vars.insert(name.clone(), v),
        };
    }

    fn declared_here(&self, name: &str) -> bool {
        match self.frames.last() {
            Some(f) => f.contains_key(name),
            None => self.vars.contains_key(name),
        }
    }

    fn set(&mut self, name: &Rc<str>, v: Val) {
        if let Some(slot) = self.frames.last_mut().and_then(|f| f.get_mut(&**name)) {
            *slot = v;
            return;
        }
        self.vars.insert(name.clone(), v);
    }

    fn slot(&mut self, name: &str) -> Option<&mut Val> {
        if self.frames.last().is_some_and(|f| f.contains_key(name)) {
            return self.frames.last_mut().and_then(|f| f.get_mut(name));
        }
        self.vars.get_mut(name)
    }
}

enum Flow {
    Normal,
    Return(Val),
    Break,
    Continue,
}

/// Global functions, by name.
const NATIVES: [&str; 58] = [
    "wiggle",
    "temporalWiggle",
    "smooth",
    "loopIn",
    "loopOut",
    "loopInDuration",
    "loopOutDuration",
    "valueAtTime",
    "velocityAtTime",
    "speedAtTime",
    "key",
    "nearestKey",
    "effect",
    "comp",
    "posterizeTime",
    "seedRandom",
    "random",
    "gaussRandom",
    "noise",
    "linear",
    "ease",
    "easeIn",
    "easeOut",
    "clamp",
    "add",
    "sum",
    "sub",
    "mul",
    "div",
    "mod",
    "length",
    "normalize",
    "dot",
    "cross",
    "degreesToRadians",
    "radiansToDegrees",
    "framesToTime",
    "timeToFrames",
    "rgbToHsl",
    "hslToRgb",
    "hexToRgb",
    "isNaN",
    "isFinite",
    "parseInt",
    "parseFloat",
    "Number",
    "String",
    "Boolean",
    "Array",
    "$bm_sum",
    "$bm_sub",
    "$bm_mul",
    "$bm_div",
    "$bm_mod",
    "$bm_neg",
    "$bm_isInstanceOfArray",
    "Array.isArray",
    "Math.random",
];

const MATH: [&str; 34] = [
    "Math.abs",
    "Math.acos",
    "Math.acosh",
    "Math.asin",
    "Math.asinh",
    "Math.atan",
    "Math.atanh",
    "Math.atan2",
    "Math.cbrt",
    "Math.ceil",
    "Math.cos",
    "Math.cosh",
    "Math.exp",
    "Math.expm1",
    "Math.floor",
    "Math.fround",
    "Math.hypot",
    "Math.log",
    "Math.log1p",
    "Math.log10",
    "Math.log2",
    "Math.max",
    "Math.min",
    "Math.pow",
    "Math.random",
    "Math.round",
    "Math.sign",
    "Math.sin",
    "Math.sinh",
    "Math.sqrt",
    "Math.tan",
    "Math.tanh",
    "Math.trunc",
    "Math.clz32",
];

/// After Effects names this interpreter does not support.
const UNSUPPORTED: [&str; 40] = [
    "toComp",
    "fromComp",
    "toWorld",
    "fromWorld",
    "toCompVec",
    "fromCompVec",
    "toWorldVec",
    "fromWorldVec",
    "fromCompToSurface",
    "lookAt",
    "sourceRectAtTime",
    "sourceTime",
    "content",
    "text",
    "marker",
    "footage",
    "mask",
    "sampleImage",
    "createPath",
    "points",
    "inTangents",
    "outTangents",
    "isClosed",
    "pointOnPath",
    "tangentOnPath",
    "normalOnPath",
    "propertyGroup",
    "propertyIndex",
    "audioLevels",
    "timeToTimecode",
    "timeToNTSCTimecode",
    "timeToFeetAndFrames",
    "timeToCurrentFormat",
    "Date",
    "eval",
    "thisProject",
    "app",
    "textIndex",
    "textTotal",
    "selectorValue",
];

const ARRAY_METHODS: [&str; 18] = [
    "push",
    "pop",
    "shift",
    "unshift",
    "reverse",
    "slice",
    "concat",
    "indexOf",
    "lastIndexOf",
    "includes",
    "join",
    "toString",
    "map",
    "forEach",
    "filter",
    "reduce",
    "some",
    "every",
];

const STRING_METHODS: [&str; 17] = [
    "toLowerCase",
    "toUpperCase",
    "trim",
    "toString",
    "valueOf",
    "charAt",
    "charCodeAt",
    "indexOf",
    "lastIndexOf",
    "includes",
    "slice",
    "substring",
    "substr",
    "split",
    "concat",
    "replace",
    "repeat",
];

pub(super) fn bound(obj: Val, name: &str) -> Val {
    Val::Bound(Rc::new((obj, name.into())))
}

/// Defines the functions `stmts` declare (outside nested functions).
fn hoist(stmts: &[Stmt], scope: &mut HashMap<Rc<str>, Val>) {
    for s in stmts {
        hoist_one(s, scope);
    }
}

fn hoist_one(s: &Stmt, scope: &mut HashMap<Rc<str>, Val>) {
    match s {
        Stmt::Func(f) => {
            if let Some(name) = &f.name {
                scope.insert(name.clone(), Val::Func(f.clone()));
            }
        }
        Stmt::Block(b) => hoist(b, scope),
        Stmt::If(_, yes, no) => {
            hoist_one(yes, scope);
            if let Some(no) = no {
                hoist_one(no, scope);
            }
        }
        Stmt::For { body, .. } | Stmt::While(_, body) | Stmt::DoWhile(body, _) => {
            hoist_one(body, scope);
        }
        Stmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            hoist(body, scope);
            for b in [catch, finally].into_iter().flatten() {
                hoist(b, scope);
            }
        }
        Stmt::Switch(_, cases) => {
            for (_, body) in cases {
                hoist(body, scope);
            }
        }
        _ => {}
    }
}

fn is_place(e: &Expr) -> bool {
    match &e.kind {
        Kind::Ident(_) => true,
        Kind::Index(base, _) => is_place(base),
        _ => false,
    }
}

impl<'a> Engine<'a> {
    pub(super) fn tick(&mut self) -> R<()> {
        self.charge(1)
    }

    /// Counts `n` steps of work.
    pub(super) fn charge(&mut self, n: u64) -> R<()> {
        self.steps = self.steps.saturating_add(n);
        if self.steps > self.limit {
            return Err(Fail::Budget);
        }
        Ok(())
    }

    fn enter(&mut self) -> R<()> {
        if self.depth >= MAX_DEPTH {
            return Err(error("an expression nested too deeply"));
        }
        self.depth += 1;
        Ok(())
    }

    /// Runs program `body` for property `id` at `time` (composition
    /// seconds), its value before the expression `value`: the result, a
    /// value rather than a property.
    pub(super) fn run_program(
        &mut self,
        id: usize,
        time: f64,
        value: Val,
        body: &[Stmt],
    ) -> R<Val> {
        let info = &self.props[id];
        let mut run = Run {
            prop: id,
            layer: info.layer,
            time,
            value,
            vars: HashMap::new(),
            frames: Vec::new(),
            rng: Rng::new(info.seed ^ value::mix64(time.to_bits())),
            seed: info.seed,
            completion: Val::Undef,
        };
        hoist(body, &mut run.vars);
        let result = match self.exec_block(&mut run, body)? {
            Flow::Return(v) => v,
            _ => match run.vars.get("$bm_rt") {
                Some(v) if !matches!(v, Val::Undef) => v.clone(),
                _ => std::mem::replace(&mut run.completion, Val::Undef),
            },
        };
        self.plain(&mut run, result)
    }

    fn exec_block(&mut self, run: &mut Run, stmts: &[Stmt]) -> R<Flow> {
        for s in stmts {
            match self.exec(run, s)? {
                Flow::Normal => {}
                flow => return Ok(flow),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec(&mut self, run: &mut Run, s: &Stmt) -> R<Flow> {
        self.tick()?;
        self.enter()?;
        let flow = self.exec_inner(run, s);
        self.depth -= 1;
        flow
    }

    fn exec_inner(&mut self, run: &mut Run, s: &Stmt) -> R<Flow> {
        match s {
            Stmt::Var(list) => {
                for (name, init) in list {
                    match init {
                        Some(e) => {
                            let v = self.eval(run, e)?;
                            run.declare(name, v);
                        }
                        None if !run.declared_here(name) => run.declare(name, Val::Undef),
                        None => {}
                    }
                }
            }
            Stmt::Expr(e) => {
                let v = self.eval(run, e)?;
                if run.frames.is_empty() {
                    run.completion = v;
                }
            }
            Stmt::If(test, yes, no) => {
                let t = self.eval(run, test)?;
                if self.truthy(run, t)? {
                    return self.exec(run, yes);
                } else if let Some(no) = no {
                    return self.exec(run, no);
                }
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.exec(run, init)?;
                }
                loop {
                    self.tick()?;
                    if let Some(test) = test {
                        let t = self.eval(run, test)?;
                        if !self.truthy(run, t)? {
                            break;
                        }
                    }
                    match self.exec(run, body)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Normal | Flow::Continue => {}
                    }
                    if let Some(update) = update {
                        self.eval(run, update)?;
                    }
                }
            }
            Stmt::While(test, body) => loop {
                self.tick()?;
                let t = self.eval(run, test)?;
                if !self.truthy(run, t)? {
                    break;
                }
                match self.exec(run, body)? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Normal | Flow::Continue => {}
                }
            },
            Stmt::DoWhile(body, test) => loop {
                self.tick()?;
                match self.exec(run, body)? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Normal | Flow::Continue => {}
                }
                let t = self.eval(run, test)?;
                if !self.truthy(run, t)? {
                    break;
                }
            },
            Stmt::Block(body) => return self.exec_block(run, body),
            Stmt::Return(value) => {
                let v = match value {
                    Some(e) => self.eval(run, e)?,
                    None => Val::Undef,
                };
                return Ok(Flow::Return(v));
            }
            Stmt::Break => return Ok(Flow::Break),
            Stmt::Continue => return Ok(Flow::Continue),
            Stmt::Func(_) | Stmt::Empty => {}
            Stmt::Try {
                body,
                param,
                catch,
                finally,
            } => {
                let mut flow = self.exec_block(run, body);
                let caught = match (&flow, catch) {
                    (Err(Fail::Error(message)), Some(catch)) => Some((message.clone(), catch)),
                    _ => None,
                };
                if let Some((message, catch)) = caught {
                    if let Some(param) = param {
                        run.declare(param, Val::str(message)?);
                    }
                    flow = self.exec_block(run, catch);
                }
                if let Some(finally) = finally {
                    match self.exec_block(run, finally)? {
                        Flow::Normal => {}
                        other => return Ok(other),
                    }
                }
                return flow;
            }
            Stmt::Throw(e) => {
                let v = self.eval(run, e)?;
                let v = self.plain(run, v)?;
                return Err(error(short(&to_string(&v))));
            }
            Stmt::Switch(value, cases) => {
                let v = self.eval(run, value)?;
                let v = self.plain(run, v)?;
                let mut start = None;
                for (i, (test, _)) in cases.iter().enumerate() {
                    if let Some(test) = test {
                        let t = self.eval(run, test)?;
                        let t = self.plain(run, t)?;
                        if strict_eq(&v, &t) {
                            start = Some(i);
                            break;
                        }
                    }
                }
                let start = start.or_else(|| cases.iter().position(|(t, _)| t.is_none()));
                if let Some(start) = start {
                    for (_, body) in &cases[start..] {
                        match self.exec_block(run, body)? {
                            Flow::Normal => {}
                            Flow::Break => break,
                            flow => return Ok(flow),
                        }
                    }
                }
            }
        }
        Ok(Flow::Normal)
    }

    pub(super) fn truthy(&mut self, run: &mut Run, v: Val) -> R<bool> {
        Ok(self.plain(run, v)?.truthy())
    }

    pub(super) fn eval(&mut self, run: &mut Run, e: &Expr) -> R<Val> {
        self.tick()?;
        self.enter()?;
        let v = self.eval_inner(run, e);
        self.depth -= 1;
        v
    }

    fn eval_plain(&mut self, run: &mut Run, e: &Expr) -> R<Val> {
        let v = self.eval(run, e)?;
        self.plain(run, v)
    }

    fn eval_inner(&mut self, run: &mut Run, e: &Expr) -> R<Val> {
        Ok(match &e.kind {
            Kind::Num(n) => Val::Num(*n),
            Kind::Str(s) => Val::Str(s.clone()),
            Kind::Bool(b) => Val::Bool(*b),
            Kind::Null => Val::Null,
            Kind::Ident(name) => return self.lookup(run, name),
            Kind::Array(items) => {
                let mut list = Vec::with_capacity(items.len());
                for item in items {
                    list.push(self.eval_plain(run, item)?);
                }
                Val::list(list)?
            }
            Kind::Func(f) => Val::Func(f.clone()),
            Kind::Unary(op, operand) => {
                if *op == UnOp::TypeOf
                    && let Kind::Ident(name) = &operand.kind
                    && run.get(name).is_none()
                {
                    return match self.global(run, name) {
                        Ok(v) => Ok(Val::Str(v.type_of().into())),
                        Err(Fail::Error(_) | Fail::Unsupported(_)) => {
                            Ok(Val::Str("undefined".into()))
                        }
                        Err(other) => Err(other),
                    };
                }
                let v = self.eval_plain(run, operand)?;
                match op {
                    UnOp::Neg => {
                        self.charge(v.cost())?;
                        value::neg(&v)?
                    }
                    UnOp::Plus => Val::Num(to_number(&v)),
                    UnOp::Not => Val::Bool(!v.truthy()),
                    UnOp::BitNot => Val::Num(f64::from(!to_int32(to_number(&v)))),
                    UnOp::TypeOf => Val::Str(v.type_of().into()),
                    UnOp::Void => Val::Undef,
                }
            }
            Kind::Update {
                increment,
                prefix,
                target,
            } => {
                let old = to_number(&self.eval_plain(run, target)?);
                let new = if *increment { old + 1.0 } else { old - 1.0 };
                self.assign(run, target, Val::Num(new))?;
                Val::Num(if *prefix { new } else { old })
            }
            Kind::Binary(op, a, b) => {
                let x = self.eval_plain(run, a)?;
                let y = self.eval_plain(run, b)?;
                return self.binary(*op, &x, &y);
            }
            Kind::Logical(op, a, b) => {
                let x = self.eval(run, a)?;
                let x = self.plain(run, x)?;
                let take_right = match op {
                    Logic::And => x.truthy(),
                    Logic::Or => !x.truthy(),
                    Logic::Nullish => matches!(x, Val::Undef | Val::Null),
                };
                if take_right {
                    return self.eval(run, b);
                }
                x
            }
            Kind::Cond(test, yes, no) => {
                let t = self.eval(run, test)?;
                return if self.truthy(run, t)? {
                    self.eval(run, yes)
                } else {
                    self.eval(run, no)
                };
            }
            Kind::Assign(op, target, value) => {
                let v = match op {
                    None => self.eval(run, value)?,
                    Some(op) => {
                        let current = self.eval_plain(run, target)?;
                        let rhs = self.eval_plain(run, value)?;
                        self.binary(*op, &current, &rhs)?
                    }
                };
                self.assign(run, target, v.clone())?;
                v
            }
            Kind::Member(obj, name) => {
                let o = self.eval(run, obj)?;
                return self.member(run, o, name);
            }
            Kind::Index(obj, index) => {
                let o = self.eval(run, obj)?;
                let k = self.eval_plain(run, index)?;
                return self.index(run, o, k);
            }
            Kind::Call(callee, args) => {
                if let Kind::Member(obj, name) = &callee.kind {
                    let o = self.eval(run, obj)?;
                    let args = self.eval_args(run, args)?;
                    let place = is_place(obj).then_some(&**obj);
                    return self.call_method(run, o, name, args, place);
                }
                let f = self.eval(run, callee)?;
                let args = self.eval_args(run, args)?;
                return self.call(run, f, args);
            }
            Kind::Seq(list) => {
                let mut last = Val::Undef;
                for e in list {
                    last = self.eval(run, e)?;
                }
                last
            }
        })
    }

    fn eval_args(&mut self, run: &mut Run, args: &[Expr]) -> R<Vec<Val>> {
        args.iter().map(|a| self.eval(run, a)).collect()
    }

    /// `v` as a value: a property's or a keyframe's value at the run's time.
    pub(super) fn plain(&mut self, run: &mut Run, v: Val) -> R<Val> {
        match v {
            Val::Host(Host::Prop(id)) => self.prop_now(run, id),
            Val::Host(Host::Key(id, k)) => self.key_value(id, k),
            v => Ok(v),
        }
    }

    pub(super) fn plain_all(&mut self, run: &mut Run, args: Vec<Val>) -> R<Vec<Val>> {
        args.into_iter().map(|a| self.plain(run, a)).collect()
    }

    pub(super) fn binary(&mut self, op: BinOp, a: &Val, b: &Val) -> R<Val> {
        self.charge(a.cost() + b.cost())?;
        let (x, y) = (|| to_number(a), || to_number(b));
        let bits = |f: fn(i32, i32) -> i32| Val::Num(f64::from(f(to_int32(x()), to_int32(y()))));
        Ok(match op {
            BinOp::Add => value::sum(a, b)?,
            BinOp::Sub => value::sub(a, b)?,
            BinOp::Mul => value::mul(a, b)?,
            BinOp::Div => value::div(a, b)?,
            BinOp::Rem => value::rem(a, b)?,
            BinOp::Pow => Val::Num(x().powf(y())),
            BinOp::Eq => Val::Bool(loose_eq(a, b)),
            BinOp::Ne => Val::Bool(!loose_eq(a, b)),
            BinOp::StrictEq => Val::Bool(strict_eq(a, b)),
            BinOp::StrictNe => Val::Bool(!strict_eq(a, b)),
            BinOp::Lt => Val::Bool(compare(a, b) == Some(Ordering::Less)),
            BinOp::Le => Val::Bool(matches!(
                compare(a, b),
                Some(Ordering::Less | Ordering::Equal)
            )),
            BinOp::Gt => Val::Bool(compare(a, b) == Some(Ordering::Greater)),
            BinOp::Ge => Val::Bool(matches!(
                compare(a, b),
                Some(Ordering::Greater | Ordering::Equal)
            )),
            BinOp::BitAnd => bits(|p, q| p & q),
            BinOp::BitOr => bits(|p, q| p | q),
            BinOp::BitXor => bits(|p, q| p ^ q),
            BinOp::Shl => bits(|p, q| p.wrapping_shl(q as u32 & 31)),
            BinOp::Shr => bits(|p, q| p.wrapping_shr(q as u32 & 31)),
            BinOp::UShr => Val::Num(f64::from(
                (to_int32(x()) as u32).wrapping_shr(to_int32(y()) as u32 & 31),
            )),
            BinOp::InstanceOf => {
                Val::Bool(matches!(b, Val::Native("Array")) && matches!(a, Val::Arr(_)))
            }
        })
    }

    fn lookup(&mut self, run: &mut Run, name: &str) -> R<Val> {
        if let Some(v) = run.get(name) {
            return Ok(v.clone());
        }
        self.global(run, name)
    }

    /// A name no variable holds: After Effects' and JavaScript's globals.
    pub(super) fn global(&mut self, run: &mut Run, name: &str) -> R<Val> {
        let layer = run.layer;
        let prop = run.prop;
        Ok(match name {
            "time" => Val::Num(run.time),
            "value" => run.value.clone(),
            "thisProperty" => Val::Host(Host::Prop(prop)),
            "thisLayer" => Val::Host(Host::Layer(layer)),
            "thisComp" => Val::Host(Host::Comp(layer.comp)),
            "$bm_transform" => Val::Host(Host::Transform(layer)),
            "Math" => Val::Host(Host::Math),
            "undefined" => Val::Undef,
            "NaN" => Val::Num(f64::NAN),
            "Infinity" => Val::Num(f64::INFINITY),
            "colorDepth" => Val::Num(8.0),
            "index" | "inPoint" | "outPoint" | "startTime" | "transform" | "position" | "scale"
            | "rotation" | "opacity" | "anchorPoint" | "parent" | "hasParent" | "width"
            | "height" | "timeRemap" | "name" | "enabled" | "active" | "xPosition"
            | "yPosition" => return self.layer_member(run, layer, name),
            "numKeys" | "velocity" | "speed" => return self.prop_member(run, prop, name),
            _ => {
                if let Some(n) = NATIVES.iter().find(|n| **n == name) {
                    return Ok(Val::Native(n));
                }
                if UNSUPPORTED.contains(&name) {
                    return Err(unsupported(name));
                }
                return Err(error(format!("`{}` is not defined", short(name))));
            }
        })
    }

    /// Sets `target` (a variable or an element of an array in one).
    fn assign(&mut self, run: &mut Run, target: &Expr, v: Val) -> R<()> {
        match &target.kind {
            Kind::Ident(name) => {
                run.set(name, v);
                Ok(())
            }
            Kind::Index(base, index) => {
                let i = to_number(&self.eval_plain(run, index)?);
                let need = value::nesting(&v).saturating_add(1);
                let (list, levels) = self.place_list(run, base, need)?;
                value::set_item(list, i, v, levels)?;
                Ok(())
            }
            Kind::Member(_, name) => Err(error(format!("`{}` set on an object", short(name)))),
            _ => Err(error("an assignment to something other than a variable")),
        }
    }

    /// The array a place (`a`, `a[i]`, `a[i][j]`) holds, made its own,
    /// and how many arrays enclose it; arrays on the way down are marked
    /// as holding arrays `need` deep below the place.
    fn place_list<'r>(
        &mut self,
        run: &'r mut Run,
        place: &Expr,
        need: u8,
    ) -> R<(&'r mut List, u8)> {
        let mut keys = Vec::new();
        let mut e = place;
        let name = loop {
            match &e.kind {
                Kind::Index(base, index) => {
                    keys.push(to_number(&self.eval_plain(run, index)?));
                    e = base;
                }
                Kind::Ident(name) => break name.clone(),
                _ => return Err(error("an element set on something other than a variable")),
            }
        };
        keys.reverse();
        if run.get(&name).is_none() {
            let v = self.global(run, &name)?;
            run.set(&name, v);
        }
        let levels = keys.len();
        let Some(mut slot) = run.slot(&name) else {
            return Err(error(format!("`{}` is not defined", short(&name))));
        };
        for (n, i) in keys.iter().enumerate() {
            let list = self.unshare(slot)?;
            let depth = (levels - n) as u8 + need;
            if depth > 16 {
                return Err(error("arrays nested too deeply"));
            }
            list.depth = list.depth.max(depth);
            let next = (i.fract() == 0.0 && *i >= 0.0)
                .then_some(*i as usize)
                .and_then(|i| list.items.get_mut(i));
            let Some(next) = next else {
                return Err(error("an element set past the end of its array"));
            };
            slot = next;
        }
        let list = self.unshare(slot)?;
        Ok((list, levels as u8))
    }

    /// The array in `slot`, copied first when another value shares it.
    fn unshare<'v>(&mut self, slot: &'v mut Val) -> R<&'v mut List> {
        let Val::Arr(rc) = slot else {
            return Err(error("an element set on something other than an array"));
        };
        if Rc::strong_count(rc) > 1 {
            self.charge(rc.items.len() as u64)?;
        }
        Ok(Rc::make_mut(rc))
    }

    pub(super) fn member(&mut self, run: &mut Run, obj: Val, name: &str) -> R<Val> {
        Ok(match &obj {
            Val::Host(h) => return self.host_member(run, *h, name),
            Val::Arr(l) => match name {
                "length" => Val::Num(l.items.len() as f64),
                _ if ARRAY_METHODS.contains(&name) => bound(obj, name),
                _ => Val::Undef,
            },
            Val::Str(s) => match name {
                "length" => Val::Num(s.chars().count() as f64),
                _ if STRING_METHODS.contains(&name) => bound(obj, name),
                _ => Val::Undef,
            },
            Val::Num(_) | Val::Bool(_) => match name {
                "toFixed" | "toString" | "valueOf" => bound(obj, name),
                _ => Val::Undef,
            },
            Val::Native("Array") if name == "isArray" => Val::Native("Array.isArray"),
            Val::Undef | Val::Null => {
                return Err(error(format!(
                    "`{}` read from {}",
                    short(name),
                    to_string(&obj)
                )));
            }
            _ => Val::Undef,
        })
    }

    fn index(&mut self, run: &mut Run, obj: Val, key: Val) -> R<Val> {
        Ok(match (&obj, &key) {
            (Val::Host(Host::Prop(_) | Host::Key(..)), _) => {
                let v = self.plain(run, obj)?;
                return self.index(run, v, key);
            }
            (Val::Arr(l), Val::Num(i)) => {
                if i.fract() == 0.0 && *i >= 0.0 {
                    l.items.get(*i as usize).cloned().unwrap_or(Val::Undef)
                } else {
                    Val::Undef
                }
            }
            (Val::Str(s), Val::Num(i)) => {
                if i.fract() == 0.0 && *i >= 0.0 {
                    s.chars()
                        .nth(*i as usize)
                        .map_or(Val::Undef, |c| Val::Str(c.to_string().into()))
                } else {
                    Val::Undef
                }
            }
            (Val::Undef | Val::Null, _) => {
                return Err(error(format!("an element read from {}", to_string(&obj))));
            }
            (_, Val::Str(name)) => {
                if let (Val::Arr(_), Ok(i)) = (&obj, name.parse::<f64>()) {
                    return self.index(run, obj, Val::Num(i));
                }
                let name = name.clone();
                return self.member(run, obj, &name);
            }
            _ => Val::Undef,
        })
    }

    pub(super) fn call(&mut self, run: &mut Run, f: Val, args: Vec<Val>) -> R<Val> {
        match f {
            Val::Func(func) => self.call_function(run, &func, args),
            Val::Native(name) => self.call_native(run, name, args),
            Val::Bound(b) => {
                let (recv, name) = (b.0.clone(), b.1.clone());
                self.call_method(run, recv, &name, args, None)
            }
            Val::Host(h) => self.call_host(run, h, args),
            other => Err(error(format!("{} called as a function", other.type_of()))),
        }
    }

    fn call_function(&mut self, run: &mut Run, func: &Func, args: Vec<Val>) -> R<Val> {
        if run.frames.len() >= MAX_CALLS {
            return Err(error("function calls nested too deeply"));
        }
        let mut frame = HashMap::new();
        for (i, p) in func.params.iter().enumerate() {
            frame.insert(p.clone(), args.get(i).cloned().unwrap_or(Val::Undef));
        }
        frame.insert("arguments".into(), Val::list(args)?);
        hoist(&func.body, &mut frame);
        self.enter()?;
        run.frames.push(frame);
        let flow = self.exec_block(run, &func.body);
        run.frames.pop();
        self.depth -= 1;
        Ok(match flow? {
            Flow::Return(v) => v,
            _ => Val::Undef,
        })
    }

    pub(super) fn call_method(
        &mut self,
        run: &mut Run,
        recv: Val,
        name: &str,
        args: Vec<Val>,
        place: Option<&Expr>,
    ) -> R<Val> {
        match &recv {
            Val::Host(h) => self.host_method(run, *h, name, args),
            Val::Arr(_) => self.array_method(run, recv, name, args, place),
            Val::Str(s) => {
                let s = s.clone();
                let args = self.plain_all(run, args)?;
                self.charge(s.len() as u64 / 16 + args.iter().map(Val::cost).sum::<u64>())?;
                string_method(&s, name, &args)
            }
            Val::Num(_) | Val::Bool(_) => {
                let args = self.plain_all(run, args)?;
                number_method(&recv, name, &args)
            }
            Val::Native("Array") if name == "isArray" => {
                let args = self.plain_all(run, args)?;
                Ok(Val::Bool(matches!(args.first(), Some(Val::Arr(_)))))
            }
            _ => {
                let f = self.member(run, recv, name)?;
                self.call(run, f, args)
            }
        }
    }

    fn array_method(
        &mut self,
        run: &mut Run,
        recv: Val,
        name: &str,
        args: Vec<Val>,
        place: Option<&Expr>,
    ) -> R<Val> {
        let args = self.plain_all(run, args)?;
        // Adding and taking at the end is quick; the rest goes through the
        // array.
        let whole = if matches!(name, "push" | "pop") {
            0
        } else {
            recv.cost()
        };
        self.charge(whole + args.iter().map(Val::cost).sum::<u64>())?;
        let undef = Val::Undef;
        let arg = |i: usize| args.get(i).unwrap_or(&undef);
        let items = recv.items();
        let len = items.len() as f64;
        // An index from the start, or from the end when negative, within
        // the array.
        let at = |v: &Val, default: f64| -> usize {
            let n = match v {
                Val::Undef => default,
                v => to_number(v).trunc(),
            };
            let n = if n.is_nan() { 0.0 } else { n };
            (if n < 0.0 {
                (len + n).max(0.0)
            } else {
                n.min(len)
            }) as usize
        };
        Ok(match name {
            "push" | "pop" | "shift" | "unshift" | "reverse" => {
                let Some(place) = place else {
                    // A temporary array: what the change would give.
                    return Ok(match name {
                        "push" | "unshift" => Val::Num(len + args.len() as f64),
                        "pop" => items.last().cloned().unwrap_or(Val::Undef),
                        "shift" => items.first().cloned().unwrap_or(Val::Undef),
                        _ => Val::list(items.iter().rev().cloned().collect())?,
                    });
                };
                drop(recv);
                let need = args.iter().map(value::nesting).max().unwrap_or(0) + 1;
                let (list, levels) = self.place_list(run, place, need)?;
                match name {
                    "push" => {
                        for a in args {
                            value::set_item(list, list.items.len() as f64, a, levels)?;
                        }
                        Val::Num(list.items.len() as f64)
                    }
                    "unshift" => {
                        if list.items.len() + args.len() > value::MAX_ARRAY {
                            return Err(error("an array of over 65536 elements"));
                        }
                        list.items.splice(0..0, args);
                        list.depth = list.depth.max(need);
                        Val::Num(list.items.len() as f64)
                    }
                    "pop" => list.items.pop().unwrap_or(Val::Undef),
                    "shift" => {
                        if list.items.is_empty() {
                            Val::Undef
                        } else {
                            list.items.remove(0)
                        }
                    }
                    _ => {
                        list.items.reverse();
                        Val::Arr(Rc::new(list.clone()))
                    }
                }
            }
            "slice" => {
                let (from, to) = (at(arg(0), 0.0), at(arg(1), len));
                Val::list(items.get(from..to.max(from)).unwrap_or(&[]).to_vec())?
            }
            "concat" => {
                let mut out = items.to_vec();
                for a in &args {
                    match a {
                        Val::Arr(l) => out.extend(l.items.iter().cloned()),
                        other => out.push(other.clone()),
                    }
                    if out.len() > value::MAX_ARRAY {
                        return Err(error("an array of over 65536 elements"));
                    }
                }
                Val::list(out)?
            }
            "indexOf" => Val::Num(
                items
                    .iter()
                    .position(|x| strict_eq(x, arg(0)))
                    .map_or(-1.0, |i| i as f64),
            ),
            "lastIndexOf" => Val::Num(
                items
                    .iter()
                    .rposition(|x| strict_eq(x, arg(0)))
                    .map_or(-1.0, |i| i as f64),
            ),
            "includes" => Val::Bool(items.iter().any(|x| strict_eq(x, arg(0)))),
            "join" => {
                let sep = match arg(0) {
                    Val::Undef => ",".to_owned(),
                    v => to_string(v),
                };
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| match x {
                        Val::Undef | Val::Null => String::new(),
                        x => to_string(x),
                    })
                    .collect();
                Val::str(parts.join(&sep))?
            }
            "toString" => Val::str(to_string(&recv))?,
            "map" | "forEach" | "filter" | "some" | "every" => {
                let f = arg(0).clone();
                let items = items.to_vec();
                let mut out = Vec::new();
                for (i, x) in items.into_iter().enumerate() {
                    let r = self.call(run, f.clone(), vec![x.clone(), Val::Num(i as f64)])?;
                    let r = self.plain(run, r)?;
                    match name {
                        "map" => out.push(r),
                        "filter" if r.truthy() => out.push(x),
                        "some" if r.truthy() => return Ok(Val::Bool(true)),
                        "every" if !r.truthy() => return Ok(Val::Bool(false)),
                        _ => {}
                    }
                }
                match name {
                    "map" | "filter" => Val::list(out)?,
                    "some" => Val::Bool(false),
                    "every" => Val::Bool(true),
                    _ => Val::Undef,
                }
            }
            "reduce" => {
                let f = arg(0).clone();
                let mut items = items.iter().cloned().enumerate();
                let mut acc = match args.get(1) {
                    Some(init) => init.clone(),
                    None => match items.next() {
                        Some((_, first)) => first,
                        None => return Err(error("`reduce` of an empty array")),
                    },
                };
                for (i, x) in items {
                    let r = self.call(run, f.clone(), vec![acc, x, Val::Num(i as f64)])?;
                    acc = self.plain(run, r)?;
                }
                acc
            }
            _ => return Err(error(format!("the array method `{}`", short(name)))),
        })
    }

    pub(super) fn call_native(
        &mut self,
        run: &mut Run,
        name: &'static str,
        args: Vec<Val>,
    ) -> R<Val> {
        match name {
            "valueAtTime" | "velocityAtTime" | "speedAtTime" | "key" | "nearestKey" | "loopIn"
            | "loopOut" | "loopInDuration" | "loopOutDuration" | "wiggle" | "temporalWiggle"
            | "smooth" => {
                let prop = run.prop;
                return self.prop_method(run, prop, name, args);
            }
            _ => {}
        }
        let args = self.plain_all(run, args)?;
        self.charge(args.iter().map(Val::cost).sum())?;
        let undef = Val::Undef;
        let arg = |i: usize| args.get(i).unwrap_or(&undef);
        let n = |i: usize| to_number(arg(i));
        Ok(match name {
            "effect" => {
                let layer = run.layer;
                return self.effect(layer, arg(0));
            }
            "comp" => return self.comp_named(arg(0)),
            "posterizeTime" => {
                let fps = n(0);
                run.time = if fps == 0.0 {
                    0.0
                } else if fps.is_finite() {
                    (run.time * fps).floor() / fps
                } else {
                    run.time
                };
                run.value = self.pre_value(run.prop, run.time)?;
                Val::Undef
            }
            "seedRandom" => {
                let timeless = arg(1).truthy();
                let mut seed = run.seed ^ value::mix64(n(0).to_bits() ^ 0x5eed);
                if !timeless {
                    seed ^= value::mix64(run.time.to_bits());
                }
                run.rng = Rng::new(seed);
                Val::Undef
            }
            "random" | "gaussRandom" => random(&mut run.rng, name == "gaussRandom", &args)?,
            "Math.random" => Val::Num(run.rng.next()),
            "noise" => Val::Num(value::noise_point(
                &value::to_numbers(arg(0)).unwrap_or_default(),
            )),
            "linear" | "ease" | "easeIn" | "easeOut" => interpolate(name, &args)?,
            "clamp" => value::clamp(arg(0), arg(1), arg(2))?,
            "add" | "sum" | "$bm_sum" => value::sum(arg(0), arg(1))?,
            "sub" | "$bm_sub" => value::sub(arg(0), arg(1))?,
            "mul" | "$bm_mul" => value::mul(arg(0), arg(1))?,
            "div" | "$bm_div" => value::div(arg(0), arg(1))?,
            "mod" | "$bm_mod" => value::rem(arg(0), arg(1))?,
            "$bm_neg" => value::neg(arg(0))?,
            "$bm_isInstanceOfArray" | "Array.isArray" => Val::Bool(matches!(arg(0), Val::Arr(_))),
            "length" => Val::Num(value::length(arg(0), args.get(1))),
            "normalize" => value::div(arg(0), &Val::Num(value::length(arg(0), None)))?,
            "dot" => Val::Num(value::dot(arg(0), arg(1))),
            "cross" => value::cross(arg(0), arg(1))?,
            "degreesToRadians" => value::map_numbers(arg(0), f64::to_radians)?,
            "radiansToDegrees" => value::map_numbers(arg(0), f64::to_degrees)?,
            "framesToTime" => {
                let fps = if n(1) > 0.0 { n(1) } else { self.fr };
                Val::Num(n(0) / fps)
            }
            "timeToFrames" => {
                let t = if matches!(arg(0), Val::Undef) {
                    run.time
                } else {
                    n(0)
                };
                let fps = if n(1) > 0.0 { n(1) } else { self.fr };
                Val::Num((t * fps + 1e-9).floor())
            }
            "rgbToHsl" => value::rgb_to_hsl(arg(0)),
            "hslToRgb" => value::hsl_to_rgb(arg(0)),
            "hexToRgb" => value::hex_to_rgb(&to_string(arg(0)))
                .ok_or_else(|| error("`hexToRgb` of something other than a hex color"))?,
            "isNaN" => Val::Bool(n(0).is_nan()),
            "isFinite" => Val::Bool(n(0).is_finite()),
            "parseInt" => Val::Num(parse_int(&to_string(arg(0)), n(1))),
            "parseFloat" => Val::Num(parse_float(&to_string(arg(0)))),
            "Number" => Val::Num(if args.is_empty() { 0.0 } else { n(0) }),
            "String" => Val::str(if args.is_empty() {
                String::new()
            } else {
                to_string(arg(0))
            })?,
            "Boolean" => Val::Bool(arg(0).truthy()),
            "Array" => return Err(unsupported("Array")),
            _ => match name.strip_prefix("Math.") {
                Some(f) => math(f, &args)?,
                None => return Err(error(format!("`{name}` is not a function"))),
            },
        })
    }

    fn host_method(&mut self, run: &mut Run, h: Host, name: &str, args: Vec<Val>) -> R<Val> {
        match h {
            Host::Math => match MATH.iter().find(|f| f[5..] == *name) {
                Some(f) => self.call_native(run, f, args),
                None => Err(error(format!("`Math.{}` is not a function", short(name)))),
            },
            Host::Comp(c) if name == "layer" => {
                let args = self.plain_all(run, args)?;
                self.comp_layer(c, &args)
            }
            Host::Layer(l) if name == "effect" => {
                let args = self.plain_all(run, args)?;
                self.effect(l, args.first().unwrap_or(&Val::Undef))
            }
            Host::Effect(l, e) if name == "param" => {
                let args = self.plain_all(run, args)?;
                self.effect_param(l, e, args.first().unwrap_or(&Val::Undef))
            }
            Host::Prop(id) => self.prop_method(run, id, name, args),
            _ => {
                let f = self.host_member(run, h, name)?;
                self.call(run, f, args)
            }
        }
    }

    pub(super) fn math_member(name: &str) -> Val {
        use std::f64::consts;
        match name {
            "PI" => Val::Num(consts::PI),
            "E" => Val::Num(consts::E),
            "LN2" => Val::Num(consts::LN_2),
            "LN10" => Val::Num(consts::LN_10),
            "LOG2E" => Val::Num(consts::LOG2_E),
            "LOG10E" => Val::Num(consts::LOG10_E),
            "SQRT2" => Val::Num(consts::SQRT_2),
            "SQRT1_2" => Val::Num(consts::FRAC_1_SQRT_2),
            _ => match MATH.iter().find(|f| f[5..] == *name) {
                Some(f) => Val::Native(f),
                None => Val::Undef,
            },
        }
    }
}

fn math(name: &str, args: &[Val]) -> R<Val> {
    let n = |i: usize| args.get(i).map_or(f64::NAN, to_number);
    let x = n(0);
    let extreme = |pick: fn(f64, f64) -> f64, start: f64| {
        args.iter().map(to_number).fold(start, |acc, v| {
            if acc.is_nan() || v.is_nan() {
                f64::NAN
            } else {
                pick(acc, v)
            }
        })
    };
    Ok(Val::Num(match name {
        "abs" => x.abs(),
        "acos" => x.acos(),
        "acosh" => x.acosh(),
        "asin" => x.asin(),
        "asinh" => x.asinh(),
        "atan" => x.atan(),
        "atanh" => x.atanh(),
        "atan2" => x.atan2(n(1)),
        "cbrt" => x.cbrt(),
        "ceil" => x.ceil(),
        "cos" => x.cos(),
        "cosh" => x.cosh(),
        "exp" => x.exp(),
        "expm1" => x.exp_m1(),
        "floor" => x.floor(),
        "fround" => f64::from(x as f32),
        "hypot" => args
            .iter()
            .map(|a| to_number(a).powi(2))
            .sum::<f64>()
            .sqrt(),
        "log" => x.ln(),
        "log1p" => x.ln_1p(),
        "log10" => x.log10(),
        "log2" => x.log2(),
        "max" => extreme(f64::max, f64::NEG_INFINITY),
        "min" => extreme(f64::min, f64::INFINITY),
        "pow" => x.powf(n(1)),
        "round" => (x + 0.5).floor(),
        "sign" => {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                x
            }
        }
        "sin" => x.sin(),
        "sinh" => x.sinh(),
        "sqrt" => x.sqrt(),
        "tan" => x.tan(),
        "tanh" => x.tanh(),
        "trunc" => x.trunc(),
        "clz32" => f64::from((to_int32(x) as u32).leading_zeros()),
        _ => return Err(error(format!("`Math.{}` is not a function", short(name)))),
    }))
}

/// `random()`, `random(max)`, `random(min, max)` (numbers or arrays), and
/// `gaussRandom` with the same arguments.
fn random(rng: &mut Rng, gauss: bool, args: &[Val]) -> R<Val> {
    let mut draw = |lo: f64, hi: f64| {
        if gauss {
            (lo + hi) / 2.0 + (hi - lo) / 2.0 * rng.gauss()
        } else {
            lo + (hi - lo) * rng.next()
        }
    };
    let (undef, zero, one) = (Val::Undef, Val::Num(0.0), Val::Num(1.0));
    let (min, max) = match args {
        [] => (&zero, &one),
        [max] | [max, Val::Undef, ..] => (&undef, max),
        [min, max, ..] => (min, max),
    };
    let min_at = |i: usize| match min {
        Val::Undef => 0.0,
        Val::Arr(l) => l.items.get(i).map_or(0.0, to_number),
        v => to_number(v),
    };
    Ok(match (min, max) {
        (_, Val::Arr(hi)) => {
            let out = (0..hi.items.len())
                .map(|i| Val::Num(draw(min_at(i), to_number(&hi.items[i]))))
                .collect();
            Val::list(out)?
        }
        (Val::Arr(lo), hi) => {
            let hi = to_number(hi);
            let out = lo
                .items
                .iter()
                .map(|l| Val::Num(draw(to_number(l), hi)))
                .collect();
            Val::list(out)?
        }
        (_, hi) => Val::Num(draw(min_at(0), to_number(hi))),
    })
}

/// `linear`, `ease`, `easeIn` and `easeOut`, with three or five arguments.
fn interpolate(name: &str, args: &[Val]) -> R<Val> {
    let n = |i: usize| args.get(i).map_or(f64::NAN, to_number);
    let (t, t0, t1, v1, v2, scaled) = match args {
        [_, v1, v2] => (n(0), 0.0, 1.0, v1, v2, false),
        [_, _, _, v1, v2, ..] => (n(0), n(1), n(2), v1, v2, true),
        _ => {
            return Err(error(format!("`{name}` with {} arguments", args.len())));
        }
    };
    let progress = if name == "linear" {
        let (lo, hi) = if t1 < t0 { (t1, t0) } else { (t0, t1) };
        if t <= lo {
            return Ok(v1.clone());
        }
        if t >= hi {
            return Ok(v2.clone());
        }
        (t - lo) / (hi - lo)
    } else {
        let p = if !scaled {
            t
        } else if t1 == t0 {
            if t >= t1 { 1.0 } else { 0.0 }
        } else {
            (t - t0) / (t1 - t0)
        };
        // lottie-web's curves for After Effects' easing functions.
        let easing = match name {
            "easeIn" => Easing::cubic_bezier(0.333, 0.0, 0.833, 0.833),
            "easeOut" => Easing::cubic_bezier(0.167, 0.167, 0.667, 1.0),
            _ => Easing::cubic_bezier(0.33, 0.0, 0.667, 1.0),
        };
        easing.apply(p.clamp(0.0, 1.0))
    };
    Ok(value::mix(v1, v2, progress)?)
}

fn parse_int(s: &str, radix: f64) -> f64 {
    let s = s.trim();
    let (negative, s) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"));
    let (radix, s) = match (radix.is_finite() && radix != 0.0, hex) {
        (true, Some(hex)) if radix == 16.0 => (16, hex),
        (true, _) => (radix as u32, s),
        (false, Some(hex)) => (16, hex),
        (false, None) => (10, s),
    };
    if !(2..=36).contains(&radix) {
        return f64::NAN;
    }
    let mut n = 0.0;
    let mut any = false;
    for c in s.chars() {
        let Some(d) = c.to_digit(radix) else { break };
        n = n * f64::from(radix) + f64::from(d);
        any = true;
    }
    match (any, negative) {
        (false, _) => f64::NAN,
        (true, true) => -n,
        (true, false) => n,
    }
}

fn parse_float(s: &str) -> f64 {
    let s = s.trim_start();
    // Digits, points, exponents, and signs first or after an exponent.
    let mut end = 0;
    let mut previous = None;
    for (i, c) in s.char_indices() {
        let sign = matches!(c, '+' | '-') && matches!(previous, None | Some('e' | 'E'));
        if !(c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E') || sign) {
            break;
        }
        end = i + c.len_utf8();
        previous = Some(c);
    }
    // The longest prefix that reads as a number.
    (1..=end)
        .rev()
        .find_map(|k| s.get(..k).and_then(|p| p.parse().ok()))
        .unwrap_or(f64::NAN)
}

fn string_method(s: &str, name: &str, args: &[Val]) -> R<Val> {
    let text = |i: usize| args.get(i).map(to_string).unwrap_or_default();
    let n = |i: usize, default: f64| match args.get(i) {
        None | Some(Val::Undef) => default,
        Some(v) => to_number(v),
    };
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as f64;
    let within = |x: f64| -> usize {
        let x = if x.is_nan() { 0.0 } else { x.trunc() };
        (if x < 0.0 {
            (len + x).max(0.0)
        } else {
            x.min(len)
        }) as usize
    };
    let piece = |a: usize, b: usize| -> String {
        if a < b {
            chars[a..b].iter().collect()
        } else {
            String::new()
        }
    };
    let char_index = |byte: Option<usize>| byte.map_or(-1.0, |b| s[..b].chars().count() as f64);
    Ok(match name {
        "toLowerCase" => Val::str(s.to_lowercase())?,
        "toUpperCase" => Val::str(s.to_uppercase())?,
        "trim" => Val::str(s.trim())?,
        "toString" | "valueOf" => Val::Str(s.into()),
        "charAt" => {
            let i = n(0, 0.0).trunc();
            let c = (i >= 0.0).then(|| chars.get(i as usize)).flatten();
            Val::str(c.map(char::to_string).unwrap_or_default())?
        }
        "charCodeAt" => {
            let i = n(0, 0.0);
            Val::Num(
                (i >= 0.0 && i.fract() == 0.0)
                    .then(|| chars.get(i as usize))
                    .flatten()
                    .map_or(f64::NAN, |c| f64::from(u32::from(*c))),
            )
        }
        "indexOf" => Val::Num(char_index(s.find(&text(0)))),
        "lastIndexOf" => Val::Num(char_index(s.rfind(&text(0)))),
        "includes" => Val::Bool(s.contains(&text(0))),
        "slice" => Val::str(piece(within(n(0, 0.0)), within(n(1, len))))?,
        "substring" => {
            let clamp = |x: f64| (if x.is_nan() { 0.0 } else { x.clamp(0.0, len) }) as usize;
            let (a, b) = (clamp(n(0, 0.0)), clamp(n(1, len)));
            Val::str(piece(a.min(b), a.max(b)))?
        }
        "substr" => {
            let start = within(n(0, 0.0));
            let count = n(1, len).max(0.0);
            let end = (start as f64 + count).min(len) as usize;
            Val::str(piece(start, end))?
        }
        "split" => {
            let parts: Vec<Val> = match args.first() {
                None | Some(Val::Undef) => vec![Val::Str(s.into())],
                Some(_) => {
                    let sep = text(0);
                    if sep.is_empty() {
                        chars
                            .iter()
                            .map(|c| Val::Str(c.to_string().into()))
                            .collect()
                    } else {
                        s.split(sep.as_str()).map(|p| Val::Str(p.into())).collect()
                    }
                }
            };
            Val::list(parts)?
        }
        "concat" => {
            let mut out = s.to_owned();
            for a in args {
                out.push_str(&to_string(a));
                if out.len() > value::MAX_STRING {
                    return Err(error("a string over 64 KB"));
                }
            }
            Val::str(out)?
        }
        "replace" => Val::str(s.replacen(&text(0), &text(1), 1))?,
        "repeat" => {
            let count = n(0, 0.0);
            if !(0.0..=f64::from(u16::MAX)).contains(&count)
                || s.len() as f64 * count.floor() > value::MAX_STRING as f64
            {
                return Err(error("a string over 64 KB"));
            }
            Val::str(s.repeat(count as usize))?
        }
        _ => return Err(error(format!("the string method `{}`", short(name)))),
    })
}

fn number_method(v: &Val, name: &str, args: &[Val]) -> R<Val> {
    let x = to_number(v);
    Ok(match name {
        "toFixed" => {
            let digits = args.first().map_or(0.0, to_number);
            let digits = if digits.is_nan() {
                0
            } else {
                digits.clamp(0.0, 100.0) as usize
            };
            if x.is_finite() {
                Val::str(format!("{x:.digits$}"))?
            } else {
                Val::str(number_to_string(x))?
            }
        }
        "toString" => Val::str(to_string(v))?,
        "valueOf" => v.clone(),
        _ => return Err(error(format!("the number method `{}`", short(name)))),
    })
}
