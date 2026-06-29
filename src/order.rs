//! Order reorderable items so dependencies come first. Cycles (mutual
//! recursion) collapse into groups emitted together inside `// mutual` markers.

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};

pub struct Ordering {
    /// New order: a flat list of reorderable item indices.
    pub order: Vec<usize>,
    /// Component id for each item index.
    pub comp_of: Vec<usize>,
    /// For each item index, whether it lives in a mutual group (component > 1).
    pub in_mutual: Vec<bool>,
    /// Component id -> members in their emitted order (only multi-member comps).
    pub mutual_groups: Vec<Vec<usize>>,
}

/// `deps[a]` = items that `a` depends on (must precede `a`).
/// `names[i]` = a sort key used for alphabetical ordering inside mutual groups.
pub fn compute(deps: &[Vec<usize>], names: &[String], alphabetical: bool) -> Ordering {
    let n = deps.len();
    let (comp_of, comps) = tarjan(n, deps);
    let ncomp = comps.len();

    // Condensation edges: provider-comp -> user-comp (b precedes a when a deps b).
    let mut edges: BTreeSet<(usize, usize)> = BTreeSet::new();
    for a in 0..n {
        for &b in &deps[a] {
            let (ca, cb) = (comp_of[a], comp_of[b]);
            if ca != cb {
                edges.insert((cb, ca));
            }
        }
    }
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); ncomp];
    let mut indeg = vec![0usize; ncomp];
    for &(from, to) in &edges {
        succ[from].push(to);
        indeg[to] += 1;
    }

    // Tie-break key per component = smallest original index among members.
    let min_orig: Vec<usize> = comps
        .iter()
        .map(|m| *m.iter().min().unwrap_or(&0))
        .collect();

    // Kahn's algorithm; among ready comps prefer the smallest original index.
    let mut heap: BinaryHeap<Reverse<(usize, usize)>> = BinaryHeap::new();
    for c in 0..ncomp {
        if indeg[c] == 0 {
            heap.push(Reverse((min_orig[c], c)));
        }
    }
    let mut comp_order: Vec<usize> = Vec::with_capacity(ncomp);
    while let Some(Reverse((_, c))) = heap.pop() {
        comp_order.push(c);
        for &to in &succ[c] {
            indeg[to] -= 1;
            if indeg[to] == 0 {
                heap.push(Reverse((min_orig[to], to)));
            }
        }
    }
    // Safety net: if a cycle in the condensation somehow remained, append the rest.
    if comp_order.len() < ncomp {
        for c in 0..ncomp {
            if !comp_order.contains(&c) {
                comp_order.push(c);
            }
        }
    }

    // Flatten components into the final item order, ordering members within each.
    let mut order = Vec::with_capacity(n);
    let mut in_mutual = vec![false; n];
    let mut mutual_groups: Vec<Vec<usize>> = Vec::new();
    for &c in &comp_order {
        let mut members = comps[c].clone();
        if members.len() > 1 {
            if alphabetical {
                members.sort_by(|&x, &y| names[x].cmp(&names[y]).then(x.cmp(&y)));
            } else {
                members.sort();
            }
            for &m in &members {
                in_mutual[m] = true;
            }
            mutual_groups.push(members.clone());
        }
        order.extend(members);
    }

    Ordering {
        order,
        comp_of,
        in_mutual,
        mutual_groups,
    }
}

/// Tarjan's SCC. Returns (component-id per node, components as member lists).
fn tarjan(n: usize, deps: &[Vec<usize>]) -> (Vec<usize>, Vec<Vec<usize>>) {
    struct State<'a> {
        deps: &'a [Vec<usize>],
        index: usize,
        idx: Vec<Option<usize>>,
        low: Vec<usize>,
        on: Vec<bool>,
        stack: Vec<usize>,
        comp_of: Vec<usize>,
        comps: Vec<Vec<usize>>,
    }
    impl<'a> State<'a> {
        fn connect(&mut self, v: usize) {
            self.idx[v] = Some(self.index);
            self.low[v] = self.index;
            self.index += 1;
            self.stack.push(v);
            self.on[v] = true;
            for &w in &self.deps[v] {
                match self.idx[w] {
                    None => {
                        self.connect(w);
                        self.low[v] = self.low[v].min(self.low[w]);
                    }
                    Some(wi) => {
                        if self.on[w] {
                            self.low[v] = self.low[v].min(wi);
                        }
                    }
                }
            }
            if self.low[v] == self.idx[v].unwrap() {
                let cid = self.comps.len();
                let mut members = Vec::new();
                loop {
                    let w = self.stack.pop().unwrap();
                    self.on[w] = false;
                    self.comp_of[w] = cid;
                    members.push(w);
                    if w == v {
                        break;
                    }
                }
                self.comps.push(members);
            }
        }
    }

    let mut st = State {
        deps,
        index: 0,
        idx: vec![None; n],
        low: vec![0; n],
        on: vec![false; n],
        stack: Vec::new(),
        comp_of: vec![0; n],
        comps: Vec::new(),
    };
    for v in 0..n {
        if st.idx[v].is_none() {
            st.connect(v);
        }
    }
    (st.comp_of, st.comps)
}
