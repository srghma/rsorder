//! Library core for rsorder: parse, decide ordering mode, reorder, and emit.
//! Everything returned is plain owned data (Send) so callers can run the
//! transform on a worker thread.

pub mod analyze;
pub mod model;
pub mod order;
pub mod render;

use std::collections::HashMap;

use order::{order_scope, OrderOpts};

/// A reorderable node, as seen by the renderers (compact index space).
#[derive(Debug, Clone)]
pub struct Node {
    pub display: String,
    pub name: Option<String>,
}

/// Plain, Send-able description of what happened, for rendering & summaries.
#[derive(Debug, Clone)]
pub struct Plan {
    pub scoped: bool,
    pub region_count: usize,
    pub nodes: Vec<Node>,
    /// Dependency edges in compact index space (`deps[c]` precede `c`).
    pub deps: Vec<Vec<usize>>,
    /// Compact indices in original source order (identity 0..R).
    pub before: Vec<usize>,
    /// Compact indices in final emit order.
    pub after: Vec<usize>,
    /// Per compact index: mutual group id, or -1.
    pub group_of: Vec<i64>,
    /// Mutual groups (compact indices), each length >= 2.
    pub mutual_groups: Vec<Vec<usize>>,
    pub kind_counts: Vec<(String, usize)>,
    pub edges: usize,
    pub moved: usize,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub new_src: String,
    pub plan: Plan,
}

#[derive(Debug, Clone)]
pub struct CheckViolation {
    pub user: String,
    pub user_index: usize,
    pub dependency: String,
    pub dependency_index: usize,
}

#[derive(Debug, Clone)]
pub struct CheckReport {
    pub violations: Vec<CheckViolation>,
}

impl CheckReport {
    pub fn is_ok(&self) -> bool { self.violations.is_empty() }
}

/// Validate that every non-mutual dependency is declared before the item that
/// uses it. Dependencies within the same multi-item SCC are treated as mutual.
pub fn check(src: &str) -> syn::Result<CheckReport> {
    let model = analyze::parse(src)?;
    let items = &model.items;
    let names: Vec<String> = items
        .iter()
        .map(|i| i.name.clone().unwrap_or_else(|| i.display.clone()))
        .collect();
    let deps_g: Vec<Vec<usize>> = items.iter().map(|i| i.deps.clone()).collect();
    let reorderable: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| !it.pinned)
        .map(|(i, _)| i)
        .collect();
    let (_, groups) = order_scope(
        &reorderable,
        &deps_g,
        &names,
        OrderOpts { inside: order::Tie::Original, outside: order::Tie::Original },
    );
    let mut group_of = HashMap::new();
    for (gid, group) in groups.iter().enumerate() {
        for &g in group {
            group_of.insert(g, gid);
        }
    }

    let mut violations = Vec::new();
    for &user in &reorderable {
        for &dep in &deps_g[user] {
            let same_mutual_group =
                group_of.get(&dep) == group_of.get(&user) && group_of.contains_key(&dep);
            if dep < user || same_mutual_group {
                continue;
            }
            violations.push(CheckViolation {
                user: names[user].clone(),
                user_index: user,
                dependency: names[dep].clone(),
                dependency_index: dep,
            });
        }
    }
    Ok(CheckReport { violations })
}

/// Reorder a single source string. `global` is the CLI-level ordering policy;
/// `// TO REORDER` regions may override it locally.
pub fn reorder(src: &str, global: OrderOpts) -> syn::Result<Outcome> {
    let model = analyze::parse(src)?;
    let items = &model.items;

    let names: Vec<String> = items
        .iter()
        .map(|i| i.name.clone().unwrap_or_else(|| i.display.clone()))
        .collect();
    let deps_g: Vec<Vec<usize>> = items.iter().map(|i| i.deps.clone()).collect();

    // Compact index space over reorderable items, in source order.
    let reorderable: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| !it.pinned)
        .map(|(i, _)| i)
        .collect();
    let g2c: HashMap<usize, usize> = reorderable.iter().enumerate().map(|(c, &g)| (g, c)).collect();

    // group_of keyed by global id, built from each scope's mutual groups.
    let mut group_of_g: HashMap<usize, i64> = HashMap::new();
    let mut mutual_groups_g: Vec<Vec<usize>> = Vec::new();
    let mut register_groups = |groups: Vec<Vec<usize>>| {
        for grp in groups {
            let id = mutual_groups_g.len() as i64;
            for &g in &grp {
                group_of_g.insert(g, id);
            }
            mutual_groups_g.push(grp);
        }
    };

    let mut after_g: Vec<usize> = Vec::new();
    let new_src;

    if model.regions.is_empty() {
        // ---- whole-file mode ----
        let (order, groups) = order_scope(&reorderable, &deps_g, &names, global);
        register_groups(groups);

        let mut segs: Vec<String> = Vec::new();
        push_nonempty(&mut segs, model.preamble.trim_end());
        for (i, it) in items.iter().enumerate() {
            if it.pinned {
                segs.push(block(items, i, true));
            }
        }
        let floating: String = reorderable
            .iter()
            .map(|&g| items[g].pre_floating.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        push_nonempty(&mut segs, &floating);
        segs.extend(emit_ordered(&order, &group_of_g, items, false, &g2c, &mut after_g));
        push_nonempty(&mut segs, model.postamble.trim());
        new_src = join(segs);
    } else {
        // ---- scoped mode: reorder only inside regions, everything else fixed ----
        let region_at: HashMap<usize, usize> =
            model.regions.iter().enumerate().map(|(ri, r)| (r.first, ri)).collect();
        let region_orders: Vec<(Vec<usize>, Vec<Vec<usize>>)> = model
            .regions
            .iter()
            .map(|r| {
                let opts = OrderOpts {
                    inside: r.opts.inside.unwrap_or(global.inside),
                    outside: r.opts.outside.unwrap_or(global.outside),
                };
                let members: Vec<usize> = (r.first..r.last_excl).collect();
                order_scope(&members, &deps_g, &names, opts)
            })
            .collect();
        for (_, groups) in &region_orders {
            register_groups(groups.clone());
        }

        let mut segs: Vec<String> = Vec::new();
        push_nonempty(&mut segs, model.preamble.trim_end());
        let mut i = 0;
        while i < items.len() {
            if let Some(&ri) = region_at.get(&i) {
                let r = &model.regions[ri];
                segs.push(r.start_line.clone());
                let (order, _) = &region_orders[ri];
                segs.extend(emit_ordered(order, &group_of_g, items, true, &g2c, &mut after_g));
                segs.push(r.end_line.clone());
                i = r.last_excl;
            } else {
                let it = &items[i];
                segs.push(block(items, i, true));
                if let Some(&c) = g2c.get(&i) {
                    after_g.push(reorderable[c]); // record global; mapped below
                }
                let _ = it;
                i += 1;
            }
        }
        push_nonempty(&mut segs, model.postamble.trim());
        new_src = join(segs);
    }

    // Build the compact plan.
    let nodes: Vec<Node> = reorderable
        .iter()
        .map(|&g| Node { display: items[g].display.clone(), name: items[g].name.clone() })
        .collect();
    let deps: Vec<Vec<usize>> = reorderable
        .iter()
        .map(|&g| deps_g[g].iter().filter_map(|d| g2c.get(d).copied()).collect())
        .collect();
    let before: Vec<usize> = (0..reorderable.len()).collect();
    let after: Vec<usize> = after_g.iter().map(|g| g2c[g]).collect();
    let group_of: Vec<i64> = reorderable
        .iter()
        .map(|&g| group_of_g.get(&g).copied().unwrap_or(-1))
        .collect();
    let mutual_groups: Vec<Vec<usize>> = mutual_groups_g
        .iter()
        .map(|grp| grp.iter().map(|g| g2c[g]).collect())
        .collect();
    let kind_counts = kind_counts(items);
    let edges = deps.iter().map(Vec::len).sum();
    let pos_after: HashMap<usize, usize> = after.iter().enumerate().map(|(p, &c)| (c, p)).collect();
    let moved = before.iter().filter(|&&c| pos_after.get(&c) != Some(&c)).count();

    Ok(Outcome {
        new_src,
        plan: Plan {
            scoped: !model.regions.is_empty(),
            region_count: model.regions.len(),
            nodes,
            deps,
            before,
            after,
            group_of,
            mutual_groups,
            kind_counts,
            edges,
            moved,
        },
    })
}

fn kind_counts(items: &[model::Item]) -> Vec<(String, usize)> {
    let mut m: std::collections::BTreeMap<&'static str, usize> = Default::default();
    for it in items {
        *m.entry(it.kind.label()).or_default() += 1;
    }
    m.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn push_nonempty(segs: &mut Vec<String>, s: &str) {
    if !s.trim().is_empty() {
        segs.push(s.to_string());
    }
}

fn join(segs: Vec<String>) -> String {
    let mut out = segs.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
    out.push('\n');
    out
}

fn block(items: &[model::Item], i: usize, include_floating: bool) -> String {
    let it = &items[i];
    let mut parts: Vec<String> = Vec::new();
    if include_floating && !it.pre_floating.trim().is_empty() {
        parts.push(it.pre_floating.trim_end().to_string());
    }
    if !it.lead.trim().is_empty() {
        parts.push(it.lead.trim_end().to_string());
    }
    parts.push(it.body.clone());
    parts.join("\n")
}

/// Emit an ordered run of items, wrapping contiguous mutual groups in markers
/// and recording the emitted compact order into `after_g` (as globals).
fn emit_ordered(
    order: &[usize],
    group_of_g: &HashMap<usize, i64>,
    items: &[model::Item],
    include_floating: bool,
    _g2c: &HashMap<usize, usize>,
    after_g: &mut Vec<usize>,
) -> Vec<String> {
    let mut segs = Vec::new();
    let mut i = 0;
    while i < order.len() {
        let g = order[i];
        let gid = group_of_g.get(&g).copied().unwrap_or(-1);
        if gid >= 0 {
            let mut group = String::from("// mutual start\n");
            let mut first = true;
            while i < order.len() && group_of_g.get(&order[i]).copied().unwrap_or(-1) == gid {
                if !first {
                    group.push_str("\n\n");
                }
                first = false;
                group.push_str(&block(items, order[i], include_floating));
                after_g.push(order[i]);
                i += 1;
            }
            group.push_str("\n// mutual end");
            segs.push(group);
        } else {
            segs.push(block(items, g, include_floating));
            after_g.push(g);
            i += 1;
        }
    }
    segs
}
