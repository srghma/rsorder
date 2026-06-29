//! Parse a source string into a `FileModel`: verbatim preamble/postamble,
//! source-ordered items (with attached comments + dependency edges), and any
//! `// TO REORDER [opts]` ... `// TO REORDER END` regions. Nothing is dropped.

use std::collections::{BTreeSet, HashMap, HashSet};

use proc_macro2::TokenTree;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::model::{classify, Item};
use crate::order::Tie;

/// Per-region option overrides. `None` means "inherit the global setting".
#[derive(Debug, Clone, Copy, Default)]
pub struct RegionOpts {
    pub inside: Option<Tie>,
    pub outside: Option<Tie>,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub start_line: String,
    pub end_line: String,
    pub opts: RegionOpts,
    /// Contiguous range of `items` indices that fall inside the region.
    pub first: usize,
    pub last_excl: usize,
}

pub struct FileModel {
    pub preamble: String,
    pub postamble: String,
    pub items: Vec<Item>,
    pub regions: Vec<Region>,
}

struct Marker {
    line_start: usize,
    is_end: bool,
    raw: String,
    opts: RegionOpts,
}

pub fn parse(src: &str) -> syn::Result<FileModel> {
    let file = syn::parse_file(src)?;

    // --- raw items with byte spans, sorted by source position ---
    let mut raws: Vec<(syn::Item, usize, usize)> = file
        .items
        .iter()
        .map(|it| {
            let r = it.span().byte_range();
            (it.clone(), r.start, r.end)
        })
        .collect();
    raws.sort_by_key(|r| r.1);

    // --- scan TO REORDER markers (line-based) ---
    let markers = scan_markers(src);
    let regions_spans = pair_markers(&markers);

    // --- preamble / postamble / leads / floating ---
    let preamble = raws
        .first()
        .map(|f| strip_markers(&src[..f.1]))
        .unwrap_or_else(|| strip_markers(src));
    let postamble = raws
        .last()
        .map(|l| strip_markers(&src[l.2..]))
        .unwrap_or_default();

    let mut leads = vec![String::new(); raws.len()];
    let mut floats = vec![String::new(); raws.len()];
    for i in 1..raws.len() {
        let (attached, floating) = split_gap(&src[raws[i - 1].2..raws[i].1]);
        leads[i] = strip_markers(&attached);
        floats[i] = strip_markers(&floating);
    }

    // --- name map for dependency edges (named, non-pinned items) ---
    let classified: Vec<_> = raws.iter().map(|(it, _, _)| classify(it)).collect();
    let name_to_idx: HashMap<String, usize> = classified
        .iter()
        .enumerate()
        .filter(|(_, (_, _, _, pinned))| !pinned)
        .filter_map(|(i, (_, name, _, _))| name.clone().map(|n| (n, i)))
        .collect();
    let defined: HashSet<String> = name_to_idx.keys().cloned().collect();

    // --- assemble items with dependency edges ---
    let items: Vec<Item> = raws
        .iter()
        .enumerate()
        .map(|(i, (item, start, end))| {
            let (kind, name, display, pinned) = classified[i].clone();
            let deps: Vec<usize> = if pinned {
                Vec::new()
            } else {
                collect_refs(item, &defined)
                    .into_iter()
                    .filter_map(|n| name_to_idx.get(&n).copied())
                    .filter(|&t| t != i)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect()
            };
            Item {
                kind,
                name,
                display,
                lead: leads[i].clone(),
                pre_floating: floats[i].clone(),
                body: src[*start..*end].trim_end().to_string(),
                byte_start: *start,
                deps,
                pinned,
            }
        })
        .collect();

    // --- map marker byte-spans to contiguous item index ranges ---
    let regions = regions_spans
        .into_iter()
        .filter_map(|(s, e)| {
            let first = items.iter().position(|it| it.byte_start > s.line_start && it.byte_start < e.line_start)?;
            let last_excl = items
                .iter()
                .rposition(|it| it.byte_start > s.line_start && it.byte_start < e.line_start)
                .map(|p| p + 1)?;
            Some(Region {
                start_line: s.raw.clone(),
                end_line: e.raw.clone(),
                opts: s.opts,
                first,
                last_excl,
            })
        })
        .collect();

    Ok(FileModel { preamble, postamble, items, regions })
}

fn scan_markers(src: &str) -> Vec<Marker> {
    let mut out = Vec::new();
    let mut off = 0usize;
    for line in src.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("// TO REORDER") {
            let rest = rest.trim();
            let raw = line.trim_end_matches(['\n', '\r']).to_string();
            if rest == "END" {
                out.push(Marker { line_start: off, is_end: true, raw, opts: RegionOpts::default() });
            } else {
                out.push(Marker { line_start: off, is_end: false, raw, opts: parse_opts(rest) });
            }
        }
        off += line.len();
    }
    out
}

fn parse_opts(rest: &str) -> RegionOpts {
    rest.split_whitespace().fold(RegionOpts::default(), |mut o, tok| {
        match tok {
            "same-level-inside-of-mutual--alphabetically" => o.inside = Some(Tie::Alphabetical),
            "same-level-inside-of-mutual--original" => o.inside = Some(Tie::Original),
            "same-level-outside-of-mutual--alphabetically" => o.outside = Some(Tie::Alphabetical),
            "same-level-outside-of-mutual--original" => o.outside = Some(Tie::Original),
            _ => {}
        }
        o
    })
}

/// Pair start/end markers sequentially (non-nested), yielding (start, end).
fn pair_markers(markers: &[Marker]) -> Vec<(&Marker, &Marker)> {
    let mut stack: Vec<&Marker> = Vec::new();
    let mut regions = Vec::new();
    for m in markers {
        if m.is_end {
            if let Some(start) = stack.pop() {
                regions.push((start, m));
            }
        } else {
            stack.push(m);
        }
    }
    regions
}

/// Strip the tool's own marker lines so the transform is idempotent.
fn strip_markers(s: &str) -> String {
    s.lines()
        .filter(|l| {
            let t = l.trim();
            t != "// mutual start" && t != "// mutual end" && !t.starts_with("// TO REORDER")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Split an inter-item gap into (comments touching the next item, floating ones).
fn split_gap(gap: &str) -> (String, String) {
    let lines: Vec<&str> = gap.split('\n').collect();
    if lines.len() <= 1 {
        return (String::new(), String::new());
    }
    let between = &lines[..lines.len() - 1];
    let start = (0..between.len())
        .rev()
        .take_while(|&i| !between[i].trim().is_empty())
        .last()
        .unwrap_or(between.len());
    let attached = between[start..].join("\n").trim_end().to_string();
    let floating = between[..start]
        .iter()
        .filter(|l| !l.trim().is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    (attached, floating)
}

fn collect_refs(item: &syn::Item, defined: &HashSet<String>) -> HashSet<String> {
    let mut c = RefCollector { defined, found: HashSet::new() };
    c.visit_item(item);
    c.found
}

struct RefCollector<'a> {
    defined: &'a HashSet<String>,
    found: HashSet<String>,
}

impl<'a> RefCollector<'a> {
    fn scan(&mut self, ts: &proc_macro2::TokenStream) {
        ts.clone().into_iter().for_each(|tt| match tt {
            TokenTree::Ident(id) => {
                let s = id.to_string();
                if self.defined.contains(&s) {
                    self.found.insert(s);
                }
            }
            TokenTree::Group(g) => self.scan(&g.stream()),
            _ => {}
        });
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
        self.scan(&m.tokens);
    }
}
