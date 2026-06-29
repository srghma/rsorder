//! Ordering of a set of items so dependencies precede uses; cycles (mutual
//! recursion) collapse into groups. Tie-breaks are configurable.

use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tie {
    /// Preserve original source order among items at the same level.
    #[default]
    Original,
    /// Sort alphabetically by name among items at the same level.
    Alphabetical,
    /// Walk source items and emit each component's dependencies first.
    Topological,
}

#[derive(Debug, Clone, Copy)]
pub struct OrderOpts {
    /// Tie-break for members within a mutual group.
    pub inside: Tie,
    /// Tie-break among independent items/components outside mutual groups.
    pub outside: Tie,
}

/// Order `members` (global item indices) by their dependencies.
/// `deps[g]` and `names[g]` are indexed by global id; only edges that stay
/// within `members` are considered. Returns the flattened order (globals) and
/// the multi-member mutual groups (globals).
pub fn order_scope(
    members: &[usize],
    deps: &[Vec<usize>],
    names: &[String],
    opts: OrderOpts,
) -> (Vec<usize>, Vec<Vec<usize>>) {
    let mut members: Vec<usize> = members.to_vec();
    members.sort_unstable();
    let n = members.len();
    let local_of: std::collections::HashMap<usize, usize> =
        members.iter().enumerate().map(|(l, &g)| (g, l)).collect();

    // Local adjacency restricted to the member set.
    let adj: Vec<Vec<usize>> = members
        .iter()
        .map(|&g| {
            deps[g]
                .iter()
                .filter_map(|d| local_of.get(d).copied())
                .collect()
        })
        .collect();

    let (comp_of, comps) = tarjan(n, &adj);

    // Condensation edges provider-comp -> user-comp.
    let edges: BTreeSet<(usize, usize)> = (0..n)
        .flat_map(|a| adj[a].iter().map(move |&b| (a, b)))
        .filter(|(a, b)| comp_of[*a] != comp_of[*b])
        .map(|(a, b)| (comp_of[b], comp_of[a]))
        .collect();

    let ncomp = comps.len();
    let mut succ = vec![Vec::new(); ncomp];
    let mut indeg = vec![0usize; ncomp];
    for &(from, to) in &edges {
        succ[from].push(to);
        indeg[to] += 1;
    }

    let comp_order = match opts.outside {
        Tie::Topological => dependency_first_order(ncomp, &comps, &members, &edges),
        Tie::Original | Tie::Alphabetical => {
            let key = |c: usize| -> (String, usize) {
                let min_g = comps[c].iter().map(|&l| members[l]).min().unwrap();
                let min_name = comps[c]
                    .iter()
                    .map(|&l| names[members[l]].clone())
                    .min()
                    .unwrap();
                (min_name, min_g)
            };
            let cmp = |a: usize, b: usize| {
                let (an, ag) = key(a);
                let (bn, bg) = key(b);
                match opts.outside {
                    Tie::Alphabetical => an.cmp(&bn).then(ag.cmp(&bg)),
                    Tie::Original | Tie::Topological => ag.cmp(&bg),
                }
            };

            // Kahn with an explicit min-selection so the tie-break is fully controlled.
            let mut ready: Vec<usize> = (0..ncomp).filter(|&c| indeg[c] == 0).collect();
            let mut comp_order = Vec::with_capacity(ncomp);
            while !ready.is_empty() {
                let pick = (0..ready.len())
                    .min_by(|&i, &j| cmp(ready[i], ready[j]))
                    .unwrap();
                let c = ready.swap_remove(pick);
                comp_order.push(c);
                for &to in &succ[c] {
                    indeg[to] -= 1;
                    if indeg[to] == 0 {
                        ready.push(to);
                    }
                }
            }
            let seen: std::collections::HashSet<usize> = comp_order.iter().copied().collect();
            comp_order.extend((0..ncomp).filter(|c| !seen.contains(c)));
            comp_order
        }
    };

    let mut order = Vec::with_capacity(n);
    let mut groups = Vec::new();
    for &c in &comp_order {
        let mut mem: Vec<usize> = comps[c].iter().map(|&l| members[l]).collect();
        match opts.inside {
            Tie::Alphabetical => mem.sort_by(|&x, &y| names[x].cmp(&names[y]).then(x.cmp(&y))),
            Tie::Original | Tie::Topological => mem.sort_unstable(),
        }
        if mem.len() > 1 {
            groups.push(mem.clone());
        }
        order.extend(mem);
    }
    (order, groups)
}

fn dependency_first_order(
    ncomp: usize,
    comps: &[Vec<usize>],
    members: &[usize],
    edges: &BTreeSet<(usize, usize)>,
) -> Vec<usize> {
    let mut deps = vec![Vec::new(); ncomp];
    for &(dep, user) in edges {
        deps[user].push(dep);
    }
    for ds in &mut deps {
        ds.sort_by_key(|&c| comps[c].iter().map(|&l| members[l]).min().unwrap());
    }

    let mut starts: Vec<usize> = (0..ncomp).collect();
    starts.sort_by_key(|&c| comps[c].iter().map(|&l| members[l]).min().unwrap());

    let mut seen = vec![false; ncomp];
    let mut order = Vec::with_capacity(ncomp);
    fn visit(c: usize, deps: &[Vec<usize>], seen: &mut [bool], order: &mut Vec<usize>) {
        if seen[c] {
            return;
        }
        seen[c] = true;
        for &d in &deps[c] {
            visit(d, deps, seen, order);
        }
        order.push(c);
    }
    for c in starts {
        visit(c, &deps, &mut seen, &mut order);
    }
    order
}

/// Tarjan's SCC over a local 0..n graph. Returns (comp-of-node, components).
fn tarjan(n: usize, adj: &[Vec<usize>]) -> (Vec<usize>, Vec<Vec<usize>>) {
    struct S<'a> {
        adj: &'a [Vec<usize>],
        idx: usize,
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on: Vec<bool>,
        stack: Vec<usize>,
        comp_of: Vec<usize>,
        comps: Vec<Vec<usize>>,
    }
    impl<'a> S<'a> {
        fn go(&mut self, v: usize) {
            self.index[v] = Some(self.idx);
            self.low[v] = self.idx;
            self.idx += 1;
            self.stack.push(v);
            self.on[v] = true;
            for &w in &self.adj[v] {
                match self.index[w] {
                    None => {
                        self.go(w);
                        self.low[v] = self.low[v].min(self.low[w]);
                    }
                    Some(wi) if self.on[w] => self.low[v] = self.low[v].min(wi),
                    _ => {}
                }
            }
            if self.low[v] == self.index[v].unwrap() {
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
    let mut s = S {
        adj,
        idx: 0,
        index: vec![None; n],
        low: vec![0; n],
        on: vec![false; n],
        stack: Vec::new(),
        comp_of: vec![0; n],
        comps: Vec::new(),
    };
    (0..n).for_each(|v| {
        if s.index[v].is_none() {
            s.go(v);
        }
    });
    (s.comp_of, s.comps)
}
