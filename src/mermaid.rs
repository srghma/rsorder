//! Mermaid renderings: dependency graph before/after, and a before/after
//! movement diff (two linked columns), plus a plain-text diff table.

use crate::analyze::FileModel;
use crate::order::Ordering;

fn esc(s: &str) -> String {
    s.replace('"', "'").replace('\n', " ")
}

/// Dependency graph (edges = "uses"), nodes declared in original source order.
pub fn before(model: &FileModel) -> String {
    let items = &model.items;
    let mut out = String::from("graph TD\n");
    for (i, it) in items.iter().enumerate() {
        out.push_str(&format!("  N{i}[\"{}\"]\n", esc(&it.display)));
    }
    for (i, it) in items.iter().enumerate() {
        for &d in &it.deps {
            out.push_str(&format!("  N{i} --> N{d}\n"));
        }
    }
    out
}

/// Dependency graph after reordering: same edges, nodes declared in the new
/// order, and mutual groups wrapped in labelled subgraphs.
pub fn after(model: &FileModel, ord: &Ordering) -> String {
    let items = &model.items;
    let mut out = String::from("graph TD\n");
    // Walk the flat new order, opening a subgraph when a mutual run begins.
    let mut i = 0;
    let mut grp = 0;
    while i < ord.order.len() {
        let idx = ord.order[i];
        if ord.in_mutual[idx] {
            let comp = ord.comp_of[idx];
            grp += 1;
            out.push_str(&format!("  subgraph mutual_{grp}[\"mutual\"]\n"));
            while i < ord.order.len() && ord.in_mutual[ord.order[i]] && ord.comp_of[ord.order[i]] == comp {
                let m = ord.order[i];
                out.push_str(&format!("    N{m}[\"{}\"]\n", esc(&items[m].display)));
                i += 1;
            }
            out.push_str("  end\n");
        } else {
            out.push_str(&format!("  N{idx}[\"{}\"]\n", esc(&items[idx].display)));
            i += 1;
        }
    }
    for (i, it) in items.iter().enumerate() {
        for &d in &it.deps {
            out.push_str(&format!("  N{i} --> N{d}\n"));
        }
    }
    out
}

/// Mermaid two-column movement diff: left column = before order, right = after,
/// dashed links connect each item to its new slot. Moved items are highlighted.
pub fn diff(model: &FileModel, ord: &Ordering) -> String {
    let items = &model.items;
    let n = items.len();
    // pos_in_new[orig_index] = row in the after column.
    let mut pos_in_new = vec![0usize; n];
    for (row, &idx) in ord.order.iter().enumerate() {
        pos_in_new[idx] = row;
    }

    let mut out = String::from("flowchart LR\n");
    out.push_str("  subgraph BEFORE\n    direction TB\n");
    for i in 0..n {
        out.push_str(&format!("    B{i}[\"{}\"]\n", esc(&items[i].display)));
    }
    for i in 0..n.saturating_sub(1) {
        out.push_str(&format!("    B{i} ~~~ B{}\n", i + 1));
    }
    out.push_str("  end\n");

    out.push_str("  subgraph AFTER\n    direction TB\n");
    for (row, &idx) in ord.order.iter().enumerate() {
        out.push_str(&format!("    A{row}[\"{}\"]\n", esc(&items[idx].display)));
    }
    for row in 0..n.saturating_sub(1) {
        out.push_str(&format!("    A{row} ~~~ A{}\n", row + 1));
    }
    out.push_str("  end\n");

    let mut moved_nodes: Vec<String> = Vec::new();
    for i in 0..n {
        let row = pos_in_new[i];
        out.push_str(&format!("  B{i} -.-> A{row}\n"));
        if row != i {
            moved_nodes.push(format!("B{i}"));
            moved_nodes.push(format!("A{row}"));
        }
    }
    if !moved_nodes.is_empty() {
        out.push_str("  classDef moved fill:#ffe7a3,stroke:#d08c00,color:#000;\n");
        out.push_str(&format!("  class {} moved;\n", moved_nodes.join(",")));
    }
    out
}

/// Plain-text two-column before/after table (the `a | b` view).
pub fn diff_table(model: &FileModel, ord: &Ordering) -> String {
    let items = &model.items;
    let n = items.len();
    let width = items.iter().map(|i| i.display.len()).max().unwrap_or(10).min(44);
    let mut out = String::new();
    out.push_str(&format!(
        "  {:<width$}     {:<width$}\n",
        "BEFORE", "AFTER",
        width = width
    ));
    out.push_str(&format!("  {}\n", "-".repeat(width * 2 + 5)));
    for row in 0..n {
        let left = &items[row].display;
        let right = &items[ord.order[row]].display;
        let arrow = if ord.order[row] != row { "=>" } else { "  " };
        out.push_str(&format!(
            "  {:<width$}  {arrow} {:<width$}\n",
            trunc(left, width),
            trunc(right, width),
            width = width
        ));
    }
    out
}

fn trunc(s: &str, w: usize) -> String {
    if s.len() <= w {
        s.to_string()
    } else {
        format!("{}…", &s[..w.saturating_sub(1)])
    }
}
