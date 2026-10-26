//! Draw rules as Rive's runtime applies them (`Artboard::sortDrawOrder`):
//! the drawables a rule holds moved before or after its active target's
//! drawable, the targets in the order their dependencies sort them.

use std::collections::{BTreeSet, HashMap};

/// An artboard's draw rules.
pub(crate) struct Rules {
    /// The rules each drawable follows: its own or its nearest ancestor's.
    pub(crate) of: HashMap<usize, usize>,
    /// The draw targets in the order they are applied, each with its
    /// drawable.
    pub(crate) targets: Vec<(usize, usize)>,
}

impl Rules {
    /// Sorts the targets of `rules` (each rules object with its targets and
    /// their drawables, in the order of the components holding them) as
    /// Rive's `DependencySorter` does: a target after the targets of the
    /// rules its drawable follows.
    pub(crate) fn new(of: HashMap<usize, usize>, rules: &[(usize, Vec<(usize, usize)>)]) -> Rules {
        const ROOT: usize = usize::MAX;
        let mut dependents: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut add = |from: usize, to: usize| {
            let list = dependents.entry(from).or_default();
            if !list.contains(&to) {
                list.push(to);
            }
        };
        let drawable: HashMap<usize, usize> =
            rules.iter().flat_map(|(_, t)| t.iter().copied()).collect();
        let targets_of: HashMap<usize, &Vec<(usize, usize)>> =
            rules.iter().map(|(r, t)| (*r, t)).collect();
        for (_, targets) in rules {
            for &(target, d) in targets {
                add(ROOT, target);
                if let Some(theirs) = of.get(&d).and_then(|r| targets_of.get(r)) {
                    for &(other, _) in theirs.iter() {
                        add(other, target);
                    }
                }
            }
        }
        // A depth-first sort; a cycle stops it, as it stops Rive's.
        fn visit(
            c: usize,
            dependents: &HashMap<usize, Vec<usize>>,
            temp: &mut BTreeSet<usize>,
            perm: &mut BTreeSet<usize>,
            order: &mut Vec<usize>,
        ) -> bool {
            if perm.contains(&c) {
                return true;
            }
            if !temp.insert(c) {
                return false;
            }
            for &d in dependents.get(&c).into_iter().flatten() {
                if !visit(d, dependents, temp, perm, order) {
                    return false;
                }
            }
            perm.insert(c);
            order.push(c);
            true
        }
        let mut order = Vec::new();
        visit(
            ROOT,
            &dependents,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
            &mut order,
        );
        order.reverse();
        let targets = order
            .into_iter()
            .skip(1)
            .filter_map(|t| Some((t, *drawable.get(&t)?)))
            .collect();
        Rules { of, targets }
    }

    /// `drawables` (in the file's order) in the order they are drawn, the
    /// bottom first, with each rules object's active target `active` says
    /// and each target placed after its drawable when `after` says.
    pub(crate) fn order(
        &self,
        drawables: &[usize],
        active: impl Fn(usize) -> Option<usize>,
        after: impl Fn(usize) -> bool,
    ) -> Vec<usize> {
        let mut prev: HashMap<usize, Option<usize>> = HashMap::new();
        let mut next: HashMap<usize, Option<usize>> = HashMap::new();
        // Each target's first and last drawable.
        let mut ends: HashMap<usize, (usize, usize)> = HashMap::new();
        let mut last: Option<usize> = None;
        let mut first: Option<usize> = None;
        let targets: BTreeSet<usize> = self.targets.iter().map(|(t, _)| *t).collect();
        for &d in drawables {
            let target = self
                .of
                .get(&d)
                .and_then(|&r| active(r))
                .filter(|t| targets.contains(t));
            if let Some(t) = target {
                match ends.get_mut(&t) {
                    None => {
                        ends.insert(t, (d, d));
                        prev.insert(d, None);
                        next.insert(d, None);
                    }
                    Some((_, end)) => {
                        next.insert(*end, Some(d));
                        prev.insert(d, Some(*end));
                        next.insert(d, None);
                        *end = d;
                    }
                }
            } else {
                prev.insert(d, last);
                next.insert(d, None);
                match last {
                    None => first = Some(d),
                    Some(l) => {
                        next.insert(l, Some(d));
                    }
                }
                last = Some(d);
            }
        }
        for &(t, target) in &self.targets {
            let Some(&(head, tail)) = ends.get(&t) else {
                continue;
            };
            if after(t) {
                if let Some(n) = next.get(&target).copied().flatten() {
                    prev.insert(n, Some(tail));
                    next.insert(tail, Some(n));
                }
                if Some(target) == last {
                    last = Some(tail);
                }
                next.insert(target, Some(head));
                prev.insert(head, Some(target));
            } else {
                if let Some(p) = prev.get(&target).copied().flatten() {
                    next.insert(p, Some(head));
                    prev.insert(head, Some(p));
                }
                if Some(target) == first {
                    first = Some(head);
                }
                prev.insert(target, Some(tail));
                next.insert(tail, Some(target));
            }
        }
        // Drawn from the last back to the first.
        let mut out = Vec::with_capacity(drawables.len());
        let mut at = last;
        while let Some(d) = at {
            if out.len() >= drawables.len() {
                break;
            }
            out.push(d);
            at = prev.get(&d).copied().flatten();
        }
        out
    }
}

/// One sequence holding each of `orders` as the entries shown in it: each
/// entry is a drawable and the orders it is drawn there in.
pub(crate) fn merge(orders: &[Vec<usize>]) -> Vec<(usize, BTreeSet<usize>)> {
    let mut merged: Vec<(usize, BTreeSet<usize>)> = Vec::new();
    for (k, order) in orders.iter().enumerate() {
        if k == 0 {
            merged = order.iter().map(|&d| (d, BTreeSet::from([0]))).collect();
            continue;
        }
        // The longest common subsequence keeps the entries already there.
        let (m, n) = (merged.len(), order.len());
        let mut lcs = vec![vec![0u32; n + 1]; m + 1];
        for i in (0..m).rev() {
            for j in (0..n).rev() {
                lcs[i][j] = if merged[i].0 == order[j] {
                    lcs[i + 1][j + 1] + 1
                } else {
                    lcs[i + 1][j].max(lcs[i][j + 1])
                };
            }
        }
        let mut out = Vec::with_capacity(m.max(n));
        let (mut i, mut j) = (0, 0);
        while i < m || j < n {
            if i < m && j < n && merged[i].0 == order[j] {
                let (d, mut shown) = merged[i].clone();
                shown.insert(k);
                out.push((d, shown));
                i += 1;
                j += 1;
            } else if j < n && (i == m || lcs[i][j + 1] >= lcs[i + 1][j]) {
                out.push((order[j], BTreeSet::from([k])));
                j += 1;
            } else {
                out.push(merged[i].clone());
                i += 1;
            }
        }
        merged = out;
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_orders_keep_each_order() {
        let orders = vec![vec![1, 2, 3, 4], vec![2, 1, 3, 4], vec![1, 2, 4, 3]];
        let merged = merge(&orders);
        for (k, order) in orders.iter().enumerate() {
            let shown: Vec<usize> = merged
                .iter()
                .filter(|(_, s)| s.contains(&k))
                .map(|(d, _)| *d)
                .collect();
            assert_eq!(&shown, order);
        }
        assert!(merged.len() <= 6, "{merged:?}");
    }

    #[test]
    fn rules_move_their_drawables_next_to_their_target() {
        // Drawables 1 to 4 (1 on top); 4 follows rules 10, whose target 11
        // puts it before (over) drawable 2.
        let rules = Rules::new(HashMap::from([(4, 10)]), &[(10, vec![(11, 2)])]);
        let before = rules.order(&[1, 2, 3, 4], |_| Some(11), |_| false);
        assert_eq!(before, vec![3, 2, 4, 1]);
        let after = rules.order(&[1, 2, 3, 4], |_| Some(11), |_| true);
        assert_eq!(after, vec![3, 4, 2, 1]);
        let none = rules.order(&[1, 2, 3, 4], |_| None, |_| true);
        assert_eq!(none, vec![4, 3, 2, 1]);
    }
}
