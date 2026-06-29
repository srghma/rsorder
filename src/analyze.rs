//! Turn a source string into a `FileModel`: a verbatim preamble, pinned items,
//! reorderable items (each with attached comments + dependency edges), floating
//! comments, and a postamble. Nothing in the source is dropped.

use std::collections::{BTreeSet, HashMap, HashSet};

use proc_macro2::TokenTree;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::model::{classify, Item, ItemKind};

pub struct FileModel {
    /// Bytes before the first item (license header, inner attrs, etc.) — verbatim.
    pub preamble: String,
    /// Comment blocks that sat between items separated by blank lines.
    pub floating: Vec<String>,
    /// Pinned items (use / extern crate) kept in original order.
    pub pinned: Vec<Item>,
    /// Reorderable items in original source order (index == order id).
    pub items: Vec<Item>,
    /// Trailing bytes after the last item — verbatim.
    pub postamble: String,
}

struct Raw {
    kind: ItemKind,
    name: Option<String>,
    display: String,
    start: usize,
    end: usize,
    pinned: bool,
    item: syn::Item,
}

pub fn parse(src: &str) -> syn::Result<FileModel> {
    let file = syn::parse_file(src)?;

    // 1. Collect raw items with byte spans, in source order.
    let mut raws: Vec<Raw> = Vec::new();
    for item in &file.items {
        let (kind, name, display, pinned) = classify(item);
        let range = item.span().byte_range();
        raws.push(Raw {
            kind,
            name,
            display,
            start: range.start,
            end: range.end,
            pinned,
            item: item.clone(),
        });
    }
    raws.sort_by_key(|r| r.start);

    // 2. Preamble / postamble / per-item leading comments.
    let preamble = if let Some(first) = raws.first() {
        strip_markers(&src[..first.start])
    } else {
        strip_markers(src)
    };
    let postamble = if let Some(last) = raws.last() {
        strip_markers(&src[last.end..])
    } else {
        String::new()
    };

    let mut leads: Vec<String> = vec![String::new(); raws.len()];
    let mut floating: Vec<String> = Vec::new();
    for i in 1..raws.len() {
        let gap = &src[raws[i - 1].end..raws[i].start];
        let (attached, floats) = split_gap(gap);
        leads[i] = strip_markers(&attached);
        let floats = strip_markers(&floats);
        if !floats.trim().is_empty() {
            floating.push(floats);
        }
    }

    // 3. Build the name -> reorderable-index map first (needed for dep edges).
    let mut order_index: HashMap<String, usize> = HashMap::new();
    let mut next_order = 0usize;
    let mut order_of_raw: Vec<Option<usize>> = Vec::with_capacity(raws.len());
    for r in &raws {
        if r.pinned {
            order_of_raw.push(None);
        } else {
            let oid = next_order;
            next_order += 1;
            if let Some(n) = &r.name {
                order_index.insert(n.clone(), oid);
            }
            order_of_raw.push(Some(oid));
        }
    }
    let defined: HashSet<String> = order_index.keys().cloned().collect();

    // 4. Assemble items, computing dependency edges for reorderable ones.
    let mut pinned_items: Vec<Item> = Vec::new();
    let mut items: Vec<Item> = vec![
        Item {
            kind: ItemKind::Other,
            name: None,
            display: String::new(),
            lead: String::new(),
            body: String::new(),
            deps: Vec::new(),
            pinned: false,
        };
        next_order
    ];

    for (i, r) in raws.iter().enumerate() {
        let body = src[r.start..r.end].trim_end().to_string();
        let lead = leads[i].clone();
        if r.pinned {
            pinned_items.push(Item {
                kind: r.kind,
                name: r.name.clone(),
                display: r.display.clone(),
                lead,
                body,
                deps: Vec::new(),
                pinned: true,
            });
            continue;
        }
        let oid = order_of_raw[i].unwrap();
        let used = collect_refs(&r.item, &defined);
        let mut deps: BTreeSet<usize> = BTreeSet::new();
        for name in used {
            if let Some(&target) = order_index.get(&name) {
                if target != oid {
                    deps.insert(target);
                }
            }
        }
        items[oid] = Item {
            kind: r.kind,
            name: r.name.clone(),
            display: r.display.clone(),
            lead,
            body,
            deps: deps.into_iter().collect(),
            pinned: false,
        };
    }

    Ok(FileModel {
        preamble,
        floating,
        pinned: pinned_items,
        items,
        postamble,
    })
}

/// Remove the tool's own `// mutual start` / `// mutual end` marker lines so the
/// transformation is idempotent (markers are re-derived from the graph).
fn strip_markers(s: &str) -> String {
    s.lines()
        .filter(|l| {
            let t = l.trim();
            t != "// mutual start" && t != "// mutual end"
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Split an inter-item gap into (comments attached to the following item,
/// floating comments separated from it by a blank line).
fn split_gap(gap: &str) -> (String, String) {
    let lines: Vec<&str> = gap.split('\n').collect();
    if lines.len() <= 1 {
        return (String::new(), String::new());
    }
    // Drop the final element: it is the indentation on the item's own line.
    let between = &lines[..lines.len() - 1];
    // Trailing contiguous non-blank lines attach to the item.
    let mut start = between.len();
    while start > 0 && !between[start - 1].trim().is_empty() {
        start -= 1;
    }
    let attached = between[start..].join("\n").trim_end().to_string();
    let floats: Vec<&str> = between[..start]
        .iter()
        .filter(|l| !l.trim().is_empty())
        .copied()
        .collect();
    (attached, floats.join("\n"))
}

/// Walk an item and collect the set of defined names it references.
fn collect_refs(item: &syn::Item, defined: &HashSet<String>) -> HashSet<String> {
    let mut c = RefCollector {
        defined,
        found: HashSet::new(),
    };
    c.visit_item(item);
    c.found
}

struct RefCollector<'a> {
    defined: &'a HashSet<String>,
    found: HashSet<String>,
}

impl<'a> RefCollector<'a> {
    fn scan_tokens(&mut self, ts: &proc_macro2::TokenStream) {
        for tt in ts.clone() {
            match tt {
                TokenTree::Ident(id) => {
                    let s = id.to_string();
                    if self.defined.contains(&s) {
                        self.found.insert(s);
                    }
                }
                TokenTree::Group(g) => self.scan_tokens(&g.stream()),
                _ => {}
            }
        }
    }
}

impl<'a, 'ast> Visit<'ast> for RefCollector<'a> {
    fn visit_path(&mut self, p: &'ast syn::Path) {
        for seg in &p.segments {
            let id = seg.ident.to_string();
            if !matches!(id.as_str(), "crate" | "self" | "Self" | "super") && self.defined.contains(&id) {
                self.found.insert(id);
            }
        }
        visit::visit_path(self, p);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        for seg in &m.path.segments {
            let id = seg.ident.to_string();
            if self.defined.contains(&id) {
                self.found.insert(id);
            }
        }
        self.scan_tokens(&m.tokens);
    }
}
