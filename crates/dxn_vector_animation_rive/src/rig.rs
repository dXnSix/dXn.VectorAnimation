//! Constraints as Rive's runtime applies them: each transform component's
//! world transform in the runtime's update order, its constraints applied
//! as it updates (`TransformComponent::updateWorldTransform`), evaluated at
//! a time and baked by the converter into matrices.

use std::{
    cmp::Reverse,
    collections::{BTreeSet, BinaryHeap, HashMap},
    f64::consts::PI,
};

use dxn_vector_animation::{
    Geometry, Matrix, Path, Point, outline::geometry_path, transform::point_along,
};

use crate::{
    curve::{Breaks, Curve},
    defs::{self, props as p},
};

/// A matrix in the parts Rive decomposes it into (`TransformComponents`).
#[derive(Debug, Clone, Copy)]
struct Parts {
    x: f64,
    y: f64,
    scale_x: f64,
    scale_y: f64,
    rotation: f64,
    skew: f64,
}

impl Parts {
    /// `Mat2D::decompose`.
    fn of(m: Matrix) -> Parts {
        let denom = m.a * m.a + m.b * m.b;
        let scale_x = denom.sqrt();
        Parts {
            x: m.e,
            y: m.f,
            scale_x,
            scale_y: if scale_x == 0.0 {
                0.0
            } else {
                (m.a * m.d - m.c * m.b) / scale_x
            },
            rotation: m.b.atan2(m.a),
            skew: (m.a * m.c + m.b * m.d).atan2(denom),
        }
    }

    /// `Mat2D::compose` (which takes the skew as a factor, not the angle
    /// `decompose` gives).
    fn matrix(&self) -> Matrix {
        let (sin, cos) = self.rotation.sin_cos();
        let mut m = Matrix::new(
            cos * self.scale_x,
            sin * self.scale_x,
            -sin * self.scale_y,
            cos * self.scale_y,
            self.x,
            self.y,
        );
        if self.skew != 0.0 {
            m.c += m.a * self.skew;
            m.d += m.b * self.skew;
        }
        m
    }
}

/// The turn from angle `from` to angle `to` the shorter way round, each
/// taken within a whole turn first, as Rive's constraints turn.
fn turn(from: f64, to: f64) -> f64 {
    let diff = to % (2.0 * PI) - from % (2.0 * PI);
    if diff > PI {
        diff - 2.0 * PI
    } else if diff < -PI {
        diff + 2.0 * PI
    } else {
        diff
    }
}

fn translation(m: Matrix) -> Point {
    Point::new(m.e, m.f)
}

fn atan2(v: Point) -> f64 {
    v.y.atan2(v.x)
}

fn sub(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}

fn length(v: Point) -> f64 {
    v.x.hypot(v.y)
}

/// A component's own transform: `translate · rotate · scale`, a bone's
/// length too.
pub(crate) struct Local {
    pub(crate) x: Curve<f64>,
    pub(crate) y: Curve<f64>,
    pub(crate) rotation: Curve<f64>,
    pub(crate) scale_x: Curve<f64>,
    pub(crate) scale_y: Curve<f64>,
    pub(crate) length: Option<Curve<f64>>,
}

impl Local {
    fn matrix(&self, t: f64) -> Matrix {
        Parts {
            x: self.x.at(t),
            y: self.y.at(t),
            scale_x: self.scale_x.at(t),
            scale_y: self.scale_y.at(t),
            rotation: self.rotation.at(t),
            skew: 0.0,
        }
        .matrix()
    }

    fn breaks(&self, out: &mut Vec<Breaks>) {
        for c in [
            &self.x,
            &self.y,
            &self.rotation,
            &self.scale_x,
            &self.scale_y,
        ]
        .into_iter()
        .chain(&self.length)
        {
            out.push(c.breaks());
        }
    }
}

/// What a constraint does.
pub(crate) enum Kind {
    Translation,
    Rotation,
    Scale,
    /// Copies the target's whole transform.
    Transform,
    Distance,
    /// Turns the bones of `chain` (the top first, the constrained bone
    /// last) so the last one's tip reaches the target.
    Ik {
        chain: Vec<usize>,
    },
    /// Moves along the outlines of the target, each a path component and
    /// its outline.
    FollowPath {
        paths: Vec<(usize, Geometry)>,
    },
}

/// A constraint on a component.
pub(crate) struct Constraint {
    pub(crate) kind: Kind,
    pub(crate) target: Option<usize>,
    /// Its number properties (bools as 0 and 1), by key.
    pub(crate) values: HashMap<u16, Curve<f64>>,
    /// When the outlines it follows change.
    pub(crate) breaks: Vec<Breaks>,
}

impl Constraint {
    fn at(&self, key: u16, t: f64) -> f64 {
        match self.values.get(&key) {
            Some(c) => c.at(t),
            None => defs::initial(key).unwrap_or(0.0),
        }
    }

    fn flag(&self, key: u16, t: f64) -> bool {
        self.at(key, t) != 0.0
    }
}

/// The world transforms of an artboard's components, as Rive's runtime
/// updates them.
#[derive(Default)]
pub(crate) struct Rig {
    /// The component each takes its parent world from (none: the
    /// artboard's, the identity).
    pub(crate) parent: Vec<Option<usize>>,
    pub(crate) locals: HashMap<usize, Local>,
    /// The constraints of each constrained component, in order.
    pub(crate) constraints: HashMap<usize, Vec<Constraint>>,
    /// The transform components in update order.
    order: Vec<usize>,
    /// Where each sits in `order`.
    rank: HashMap<usize, usize>,
    /// What updates before each component.
    before: HashMap<usize, Vec<usize>>,
    /// The constrained component whose update sets each IK chain bone's
    /// world transform.
    tip: HashMap<usize, usize>,
}

impl Rig {
    /// A rig of an artboard of `n` objects.
    pub(crate) fn new(n: usize) -> Rig {
        Rig {
            parent: vec![None; n],
            ..Rig::default()
        }
    }

    /// Sorts the components into Rive's update order: parents before
    /// children, targets before what they constrain, an IK chain's other
    /// children after its tip.
    pub(crate) fn sort(&mut self, children: &[Vec<usize>]) {
        let mut after: HashMap<usize, BTreeSet<usize>> = HashMap::new();
        let mut edge = |a: usize, b: usize| {
            if a != b {
                after.entry(a).or_default().insert(b);
            }
        };
        for &i in self.locals.keys() {
            if let Some(pi) = self.parent[i] {
                edge(pi, i);
            }
        }
        for (&i, list) in &self.constraints {
            for c in list {
                if let Some(target) = c.target {
                    edge(target, i);
                }
                match &c.kind {
                    Kind::FollowPath { paths } => {
                        for (pi, _) in paths {
                            edge(*pi, i);
                        }
                    }
                    Kind::Ik { chain } => {
                        for (k, &bone) in
                            chain.iter().enumerate().take(chain.len().saturating_sub(1))
                        {
                            self.tip.insert(bone, i);
                            let next = chain[k + 1];
                            for &child in &children[bone] {
                                if child != next && self.locals.contains_key(&child) {
                                    edge(i, child);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut incoming: HashMap<usize, usize> = self.locals.keys().map(|&i| (i, 0)).collect();
        for list in after.values() {
            for b in list {
                if let Some(n) = incoming.get_mut(b) {
                    *n += 1;
                }
            }
        }
        let mut ready: BinaryHeap<Reverse<usize>> = incoming
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(&i, _)| Reverse(i))
            .collect();
        let mut order = Vec::with_capacity(incoming.len());
        while let Some(Reverse(i)) = ready.pop() {
            order.push(i);
            for &b in after.get(&i).into_iter().flatten() {
                if let Some(n) = incoming.get_mut(&b) {
                    *n -= 1;
                    if *n == 0 {
                        ready.push(Reverse(b));
                    }
                }
            }
        }
        // A cycle (which Rive refuses to sort) updates in the file's order.
        let placed: BTreeSet<usize> = order.iter().copied().collect();
        let mut rest: Vec<usize> = incoming
            .keys()
            .filter(|i| !placed.contains(i))
            .copied()
            .collect();
        rest.sort_unstable();
        order.extend(rest);
        for (&a, list) in &after {
            for &b in list {
                self.before.entry(b).or_default().push(a);
            }
        }
        self.rank = order.iter().enumerate().map(|(k, &i)| (i, k)).collect();
        self.order = order;
    }

    /// Whether component `i`'s world is not its parent's times its own
    /// transform.
    pub(crate) fn moves(&self, i: usize) -> bool {
        self.constraints.contains_key(&i) || self.tip.contains_key(&i)
    }

    /// The components whose update decides component `i`'s world, `i`
    /// with them, in update order.
    pub(crate) fn closure(&self, i: usize) -> Vec<usize> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![i];
        while let Some(c) = stack.pop() {
            if !seen.insert(c) {
                continue;
            }
            stack.extend(self.before.get(&c).into_iter().flatten());
            stack.extend(self.tip.get(&c));
        }
        let mut list: Vec<usize> = seen
            .into_iter()
            .filter(|c| self.rank.contains_key(c))
            .collect();
        list.sort_by_key(|c| self.rank[c]);
        list
    }

    /// When what `closure` depends on changes.
    pub(crate) fn breaks(&self, closure: &[usize]) -> Vec<Breaks> {
        let mut out = Vec::new();
        for i in closure {
            if let Some(l) = self.locals.get(i) {
                l.breaks(&mut out);
            }
            for c in self.constraints.get(i).into_iter().flatten() {
                out.extend(c.values.values().map(Curve::breaks));
                out.extend(c.breaks.iter().cloned());
            }
        }
        out
    }

    /// The world transforms of `closure` (in update order) at `t`.
    pub(crate) fn worlds(&self, closure: &[usize], t: f64) -> HashMap<usize, Matrix> {
        let mut s = State {
            rig: self,
            t,
            world: HashMap::with_capacity(closure.len()),
            local: HashMap::with_capacity(closure.len()),
        };
        for &i in closure {
            let Some(l) = self.locals.get(&i) else {
                continue;
            };
            let local = l.matrix(t);
            s.local.insert(i, local);
            s.world.insert(i, s.parent_world(i) * local);
            for c in self.constraints.get(&i).into_iter().flatten() {
                s.constrain(c, i);
            }
        }
        s.world
    }
}

/// The transforms of one update.
struct State<'a> {
    rig: &'a Rig,
    t: f64,
    world: HashMap<usize, Matrix>,
    local: HashMap<usize, Matrix>,
}

/// A bone of an IK chain as the solver works on it (`BoneChainLink`).
struct Link {
    bone: usize,
    parent_world_inverse: Matrix,
    parts: Parts,
    angle: f64,
}

impl State<'_> {
    fn world(&self, i: usize) -> Matrix {
        self.world.get(&i).copied().unwrap_or(Matrix::IDENTITY)
    }

    /// `getParentWorld`.
    fn parent_world(&self, i: usize) -> Matrix {
        self.rig
            .parent
            .get(i)
            .copied()
            .flatten()
            .map_or(Matrix::IDENTITY, |p| self.world(p))
    }

    /// Component `i`'s own value of `x`, `y`, rotation or scale.
    fn own(&self, i: usize, part: fn(&Local) -> &Curve<f64>) -> f64 {
        self.rig.locals.get(&i).map_or(0.0, |l| part(l).at(self.t))
    }

    fn constrain(&mut self, c: &Constraint, i: usize) {
        match &c.kind {
            Kind::Translation => self.translation(c, i),
            Kind::Rotation => self.rotation(c, i),
            Kind::Scale => self.scale(c, i),
            Kind::Transform => self.transform(c, i),
            Kind::Distance => self.distance(c, i),
            Kind::Ik { chain } => self.ik(c, chain),
            Kind::FollowPath { paths } => self.follow_path(c, i, paths),
        }
    }

    /// The target's world transform in the constraint's source space.
    fn source(&self, c: &Constraint, target: usize, mut b: Matrix) -> Option<Matrix> {
        if c.flag(p::TRANSFORM_SPACE_CONSTRAINT_SOURCE_SPACE_VALUE, self.t) {
            b = self.parent_world(target).invert()? * b;
        }
        Some(b)
    }

    fn translation(&mut self, c: &Constraint, i: usize) {
        let t = self.t;
        let a = self.world(i);
        let ta = translation(a);
        let dest_local = c.flag(p::TRANSFORM_SPACE_CONSTRAINT_DEST_SPACE_VALUE, t);
        let mut tb = match c.target {
            None => ta,
            Some(target) => {
                let Some(b) = self.source(c, target, self.world(target)) else {
                    return;
                };
                let mut tb = translation(b);
                let axes = [
                    (
                        p::TRANSFORM_COMPONENT_CONSTRAINT_DOES_COPY,
                        p::TRANSFORM_COMPONENT_CONSTRAINT_COPY_FACTOR,
                        ta.x,
                        self.own(i, |l| &l.x),
                    ),
                    (
                        p::TRANSFORM_COMPONENT_CONSTRAINT_Y_DOES_COPY_Y,
                        p::TRANSFORM_COMPONENT_CONSTRAINT_Y_COPY_FACTOR_Y,
                        ta.y,
                        self.own(i, |l| &l.y),
                    ),
                ];
                let offset = c.flag(p::TRANSFORM_COMPONENT_CONSTRAINT_OFFSET, t);
                for (k, (copies, factor, from, own)) in axes.into_iter().enumerate() {
                    let v = if k == 0 { &mut tb.x } else { &mut tb.y };
                    if !c.flag(copies, t) {
                        *v = if dest_local { 0.0 } else { from };
                    } else {
                        *v *= c.at(factor, t);
                        if offset {
                            *v += own;
                        }
                    }
                }
                if dest_local {
                    tb = self.parent_world(i).apply(tb);
                }
                tb
            }
        };
        let clamp_local = c.flag(p::TRANSFORM_COMPONENT_CONSTRAINT_MIN_MAX_SPACE_VALUE, t);
        if clamp_local {
            let Some(inverse) = self.parent_world(i).invert() else {
                return;
            };
            tb = inverse.apply(tb);
        }
        tb.x = self.clamped(c, tb.x, false);
        tb.y = self.clamped(c, tb.y, true);
        if clamp_local {
            tb = self.parent_world(i).apply(tb);
        }
        let s = c.at(p::CONSTRAINT_STRENGTH, t);
        let w = self.world.entry(i).or_insert(Matrix::IDENTITY);
        w.e = ta.x * (1.0 - s) + tb.x * s;
        w.f = ta.y * (1.0 - s) + tb.y * s;
    }

    /// `v` within the constraint's minimum and maximum (its y ones when
    /// `y`).
    fn clamped(&self, c: &Constraint, mut v: f64, y: bool) -> f64 {
        let t = self.t;
        let (max, max_value, min, min_value) = if y {
            (
                p::TRANSFORM_COMPONENT_CONSTRAINT_Y_MAX_Y,
                p::TRANSFORM_COMPONENT_CONSTRAINT_Y_MAX_VALUE_Y,
                p::TRANSFORM_COMPONENT_CONSTRAINT_Y_MIN_Y,
                p::TRANSFORM_COMPONENT_CONSTRAINT_Y_MIN_VALUE_Y,
            )
        } else {
            (
                p::TRANSFORM_COMPONENT_CONSTRAINT_MAX,
                p::TRANSFORM_COMPONENT_CONSTRAINT_MAX_VALUE,
                p::TRANSFORM_COMPONENT_CONSTRAINT_MIN,
                p::TRANSFORM_COMPONENT_CONSTRAINT_MIN_VALUE,
            )
        };
        if c.flag(max, t) && v > c.at(max_value, t) {
            v = c.at(max_value, t);
        }
        if c.flag(min, t) && v < c.at(min_value, t) {
            v = c.at(min_value, t);
        }
        v
    }

    /// The target's parts for a rotation or scale constraint, with `copy`
    /// setting what it copies; then clamped by `clamp` in the min-max
    /// space. `None` where Rive gives up (a parent world without inverse).
    fn copied_parts(
        &self,
        c: &Constraint,
        i: usize,
        a: Parts,
        copy: impl Fn(&mut Parts, bool),
        clamp: impl Fn(&mut Parts),
    ) -> Option<Parts> {
        let t = self.t;
        let mut b = match c.target {
            None => a,
            Some(target) => {
                let mut b = Parts::of(self.source(c, target, self.world(target))?);
                let dest_local = c.flag(p::TRANSFORM_SPACE_CONSTRAINT_DEST_SPACE_VALUE, t);
                copy(&mut b, dest_local);
                if dest_local {
                    b = Parts::of(self.parent_world(i) * b.matrix());
                }
                b
            }
        };
        let clamp_local = c.flag(p::TRANSFORM_COMPONENT_CONSTRAINT_MIN_MAX_SPACE_VALUE, t);
        if clamp_local {
            let inverse = self.parent_world(i).invert()?;
            b = Parts::of(inverse * b.matrix());
        }
        clamp(&mut b);
        if clamp_local {
            b = Parts::of(self.parent_world(i) * b.matrix());
        }
        Some(b)
    }

    fn rotation(&mut self, c: &Constraint, i: usize) {
        let t = self.t;
        let a = Parts::of(self.world(i));
        let own = self.own(i, |l| &l.rotation);
        let copy = |b: &mut Parts, dest_local: bool| {
            if !c.flag(p::TRANSFORM_COMPONENT_CONSTRAINT_DOES_COPY, t) {
                b.rotation = if dest_local { 0.0 } else { a.rotation };
            } else {
                b.rotation *= c.at(p::TRANSFORM_COMPONENT_CONSTRAINT_COPY_FACTOR, t);
                if c.flag(p::TRANSFORM_COMPONENT_CONSTRAINT_OFFSET, t) {
                    b.rotation += own;
                }
            }
        };
        let clamp = |b: &mut Parts| b.rotation = self.clamped(c, b.rotation, false);
        let Some(b) = self.copied_parts(c, i, a, copy, clamp) else {
            return;
        };
        let s = c.at(p::CONSTRAINT_STRENGTH, t);
        let out = Parts {
            rotation: a.rotation + turn(a.rotation, b.rotation) * s,
            ..a
        };
        self.world.insert(i, out.matrix());
    }

    fn scale(&mut self, c: &Constraint, i: usize) {
        let t = self.t;
        let a = Parts::of(self.world(i));
        let (own_x, own_y) = (self.own(i, |l| &l.scale_x), self.own(i, |l| &l.scale_y));
        let offset = c.flag(p::TRANSFORM_COMPONENT_CONSTRAINT_OFFSET, t);
        let copy = |b: &mut Parts, dest_local: bool| {
            if !c.flag(p::TRANSFORM_COMPONENT_CONSTRAINT_DOES_COPY, t) {
                b.scale_x = if dest_local { 1.0 } else { a.scale_x };
            } else {
                b.scale_x *= c.at(p::TRANSFORM_COMPONENT_CONSTRAINT_COPY_FACTOR, t);
                if offset {
                    b.scale_x *= own_x;
                }
            }
            if !c.flag(p::TRANSFORM_COMPONENT_CONSTRAINT_Y_DOES_COPY_Y, t) {
                b.scale_y = if dest_local { 1.0 } else { a.scale_y };
            } else {
                b.scale_y *= c.at(p::TRANSFORM_COMPONENT_CONSTRAINT_Y_COPY_FACTOR_Y, t);
                if offset {
                    b.scale_y *= own_y;
                }
            }
        };
        let clamp = |b: &mut Parts| {
            b.scale_x = self.clamped(c, b.scale_x, false);
            b.scale_y = self.clamped(c, b.scale_y, true);
        };
        let Some(b) = self.copied_parts(c, i, a, copy, clamp) else {
            return;
        };
        let s = c.at(p::CONSTRAINT_STRENGTH, t);
        let out = Parts {
            scale_x: a.scale_x * (1.0 - s) + b.scale_x * s,
            scale_y: a.scale_y * (1.0 - s) + b.scale_y * s,
            ..a
        };
        self.world.insert(i, out.matrix());
    }

    /// `TransformConstraint::constrainWorld`: every part moved toward
    /// `to` by the strength.
    fn toward(&mut self, i: usize, to: Matrix, s: f64) {
        let a = Parts::of(self.world(i));
        let b = Parts::of(to);
        let mix = |x: f64, y: f64| x * (1.0 - s) + y * s;
        let out = Parts {
            rotation: a.rotation % (2.0 * PI) + turn(a.rotation, b.rotation) * s,
            x: mix(a.x, b.x),
            y: mix(a.y, b.y),
            scale_x: mix(a.scale_x, b.scale_x),
            scale_y: mix(a.scale_y, b.scale_y),
            skew: mix(a.skew, b.skew),
        };
        self.world.insert(i, out.matrix());
    }

    fn transform(&mut self, c: &Constraint, i: usize) {
        let t = self.t;
        let Some(target) = c.target else { return };
        // A target's constraint bounds are empty but a text's or a layout's
        // (left at its origin here): the origin is the target's.
        let Some(mut b) = self.source(c, target, self.world(target)) else {
            return;
        };
        if c.flag(p::TRANSFORM_SPACE_CONSTRAINT_DEST_SPACE_VALUE, t) {
            b = self.parent_world(i) * b;
        }
        self.toward(i, b, c.at(p::CONSTRAINT_STRENGTH, t));
    }

    fn distance(&mut self, c: &Constraint, i: usize) {
        let t = self.t;
        let Some(target) = c.target else { return };
        let to = translation(self.world(target));
        let ours = translation(self.world(i));
        let away = sub(ours, to);
        let current = length(away);
        let distance = c.at(p::DISTANCE_CONSTRAINT_DISTANCE, t);
        match c.at(p::DISTANCE_CONSTRAINT_MODE_VALUE, t) as u64 {
            0 if current < distance => return,
            1 if current > distance => return,
            _ => {}
        }
        if current < 0.001 {
            return;
        }
        let k = distance / current;
        let at = Point::new(to.x + away.x * k, to.y + away.y * k);
        let s = c.at(p::CONSTRAINT_STRENGTH, t);
        let w = self.world.entry(i).or_insert(Matrix::IDENTITY);
        w.e = ours.x + (at.x - ours.x) * s;
        w.f = ours.y + (at.y - ours.y) * s;
    }

    fn follow_path(&mut self, c: &Constraint, i: usize, paths: &[(usize, Geometry)]) {
        let t = self.t;
        let Some(target) = c.target else { return };
        let s = c.at(p::CONSTRAINT_STRENGTH, t);
        let orient = c.flag(p::FOLLOW_PATH_CONSTRAINT_ORIENT, t);
        let mut b = self.world(target);
        if !paths.is_empty() {
            let mut outline = Path::default();
            for (pi, path) in paths {
                outline.extend(&geometry_path(path, t).transformed(self.world(*pi)));
            }
            // Rive's `PathMeasure::atPercentage`: wrapped, a whole turn
            // past 0 at the end.
            let distance = c.at(p::FOLLOW_PATH_CONSTRAINT_DISTANCE, t);
            let mut fraction = distance % 1.0;
            if fraction < 0.0 {
                fraction += 1.0;
            }
            if distance != 0.0 && fraction == 0.0 {
                fraction = 1.0;
            }
            let (position, degrees) = point_along(&outline, fraction).unwrap_or((Point::ZERO, 0.0));
            if orient {
                let angle = Parts::of(b).rotation;
                let r = angle % (2.0 * PI) + turn(angle, degrees.to_radians()) * s;
                b = Matrix::rotate(r.to_degrees());
            }
            let offset = if c.flag(p::FOLLOW_PATH_CONSTRAINT_OFFSET, t) {
                translation(self.local.get(&i).copied().unwrap_or(Matrix::IDENTITY))
            } else {
                Point::ZERO
            };
            b.e = position.x + offset.x;
            b.f = position.y + offset.y;
        }
        let Some(mut b) = self.source(c, target, b) else {
            self.world.insert(i, Parts::of(Matrix::IDENTITY).matrix());
            return;
        };
        if c.flag(p::TRANSFORM_SPACE_CONSTRAINT_DEST_SPACE_VALUE, t) {
            b = self.parent_world(i) * b;
        }
        let a = Parts::of(self.world(i));
        let mut out = Parts::of(b);
        if !orient {
            out.rotation = a.rotation % (2.0 * PI);
        }
        out.x = a.x * (1.0 - s) + out.x * s;
        out.y = a.y * (1.0 - s) + out.y * s;
        out.scale_x = a.scale_x;
        out.scale_y = a.scale_y;
        out.skew = a.skew;
        self.world.insert(i, out.matrix());
    }

    // ---- IK (`IKConstraint`) -------------------------------------------

    fn ik(&mut self, c: &Constraint, chain: &[usize]) {
        let t = self.t;
        let Some(target) = c.target else { return };
        let to = translation(self.world(target));
        let mut links: Vec<Link> = chain
            .iter()
            .map(|&bone| {
                let inverse = self.parent_world(bone).invert().unwrap_or(Matrix::IDENTITY);
                let local = inverse * self.world(bone);
                self.local.insert(bone, local);
                Link {
                    bone,
                    parent_world_inverse: inverse,
                    parts: Parts::of(local),
                    angle: 0.0,
                }
            })
            .collect();
        let invert = c.flag(p::IK_CONSTRAINT_INVERT_DIRECTION, t);
        match links.len() {
            0 => return,
            1 => self.solve1(&mut links, to),
            2 => self.solve2(&mut links, 0, 1, to, invert),
            n => {
                let last = n - 1;
                for k in 0..last {
                    self.solve2(&mut links, k, last, to, invert);
                    for link in &mut links[k + 1..last] {
                        link.parent_world_inverse = self
                            .parent_world(link.bone)
                            .invert()
                            .unwrap_or(Matrix::IDENTITY);
                    }
                }
            }
        }
        let s = c.at(p::CONSTRAINT_STRENGTH, t);
        if s != 1.0 {
            for link in &links {
                let from = link.parts.rotation % (2.0 * PI);
                let angle = from + turn(link.parts.rotation, link.angle) * s;
                self.constrain_rotation(link, angle);
            }
        }
    }

    fn solve1(&mut self, links: &mut [Link], to: Point) {
        let fk = &links[0];
        let at = translation(self.world(fk.bone));
        let r = atan2(fk.parent_world_inverse.apply_vector(sub(to, at)));
        self.constrain_rotation(fk, r);
        links[0].angle = r;
    }

    fn tip_world(&self, bone: usize) -> Point {
        let length = self
            .rig
            .locals
            .get(&bone)
            .and_then(|l| l.length.as_ref())
            .map_or(0.0, |l| l.at(self.t));
        self.world(bone).apply(Point::new(length, 0.0))
    }

    fn solve2(&mut self, links: &mut [Link], k1: usize, k2: usize, to: Point, invert: bool) {
        let (b1, b2) = (links[k1].bone, links[k2].bone);
        let first = k1 + 1;
        let iworld = links[k1].parent_world_inverse;
        let pa = iworld.apply(translation(self.world(b1)));
        let pc = iworld.apply(translation(self.world(links[first].bone)));
        let pb = iworld.apply(self.tip_world(b2));
        let pbt = iworld.apply(to);
        let (av, bv, cv) = (sub(pb, pc), sub(pc, pa), sub(pbt, pa));
        let (a, b, c) = (length(av), length(bv), length(cv));
        // Rive clamps with `std::min` and `std::max`, which turn NaN into 1.
        let clamp = |v: f64| if v.is_nan() { 1.0 } else { v.clamp(-1.0, 1.0) };
        let big_a = clamp((-a * a + b * b + c * c) / (2.0 * b * c)).acos();
        let big_c = clamp((a * a + b * b - c * c) / (2.0 * a * b)).acos();
        let correction = if self.rig.parent.get(b2).copied().flatten() != Some(b1) {
            let second = &links[k1 + 2];
            let pc = translation(self.world(links[first].bone));
            let pb = self.tip_world(b2);
            -atan2(second.parent_world_inverse.apply_vector(sub(pb, pc)))
        } else {
            0.0
        };
        let (r1, r2) = if invert {
            (atan2(cv) - big_a, -big_c + PI + correction)
        } else {
            (big_a + atan2(cv), big_c - PI + correction)
        };
        self.constrain_rotation(&links[k1], r1);
        self.constrain_rotation(&links[first], r2);
        if first != k2 {
            let local = self.local.get(&b2).copied().unwrap_or(Matrix::IDENTITY);
            self.world.insert(b2, self.parent_world(b2) * local);
        }
        links[k1].angle = r1;
        links[first].angle = r2;
    }

    fn constrain_rotation(&mut self, link: &Link, rotation: f64) {
        let local = Parts {
            rotation,
            ..link.parts
        }
        .matrix();
        self.local.insert(link.bone, local);
        self.world
            .insert(link.bone, self.parent_world(link.bone) * local);
    }
}
