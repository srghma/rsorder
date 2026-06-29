//! rsorder — reorder top-level Rust items so definitions precede uses,
//! Lean-style `// mutual start/end` blocks for cycles, with Mermaid views.

mod analyze;
mod mermaid;
mod model;
mod order;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default)]
struct Cli {
    patterns: Vec<String>,
    same_level_alphabetically: bool,
    mermaid_before: bool,
    mermaid_after: bool,
    mermaid_diff: bool,
    write: bool,
    stdout: bool,
}

const USAGE: &str = "\
rsorder - reorder Rust items definition-before-use, wrap mutual cycles, show graphs

USAGE:
    rsorder [OPTIONS] <GLOB>...

ARGS:
    <GLOB>...   One or more glob patterns matching .rs files (e.g. 'src/**/*.rs')

OPTIONS:
        --same-level-alphabetically   Sort items alphabetically inside a mutual
                                       group (default: preserve original order)
        --mermaid-before              Print Mermaid dependency graph (original order)
        --mermaid-after               Print Mermaid dependency graph (reordered)
        --mermaid-diff                Print before/after movement table + Mermaid
    -w, --write                       Rewrite files in place (default: dry run)
        --stdout                      Print reordered contents to stdout
    -h, --help                        Show this help";

fn parse_cli() -> Cli {
    let mut cli = Cli::default();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--same-level-alphabetically" => cli.same_level_alphabetically = true,
            "--mermaid-before" => cli.mermaid_before = true,
            "--mermaid-after" | "--mermaid-end" => cli.mermaid_after = true,
            "--mermaid-diff" => cli.mermaid_diff = true,
            "-w" | "--write" => cli.write = true,
            "--stdout" => cli.stdout = true,
            other if other.starts_with('-') => {
                eprintln!("unknown option: {other}\n\n{USAGE}");
                std::process::exit(2);
            }
            other => cli.patterns.push(other.to_string()),
        }
    }
    if cli.patterns.is_empty() {
        eprintln!("error: at least one glob pattern is required\n\n{USAGE}");
        std::process::exit(2);
    }
    cli
}

fn main() {
    let cli = parse_cli();

    let mut files: BTreeSet<PathBuf> = BTreeSet::new();
    for pat in &cli.patterns {
        match glob::glob(pat) {
            Ok(paths) => {
                for entry in paths.flatten() {
                    if entry.extension().map(|e| e == "rs").unwrap_or(false) && entry.is_file() {
                        files.insert(entry);
                    }
                }
            }
            Err(e) => eprintln!("bad glob pattern {pat:?}: {e}"),
        }
    }

    if files.is_empty() {
        eprintln!("no .rs files matched the given pattern(s)");
        std::process::exit(1);
    }

    let mut total_items = 0usize;
    let mut total_moved = 0usize;
    let mut total_mutual = 0usize;
    let mut total_changed = 0usize;

    for path in &files {
        let src = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: cannot read: {e}", path.display());
                continue;
            }
        };
        let model = match analyze::parse(&src) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{}: parse error: {e}", path.display());
                continue;
            }
        };

        let deps: Vec<Vec<usize>> = model.items.iter().map(|i| i.deps.clone()).collect();
        let names: Vec<String> = model
            .items
            .iter()
            .map(|i| i.name.clone().unwrap_or_else(|| i.display.clone()))
            .collect();
        let ord = order::compute(&deps, &names, cli.same_level_alphabetically);

        let new_src = emit(&model, &ord);
        let changed = new_src != src;

        println!("=== {} ===", path.display());

        if cli.mermaid_before {
            println!("\n```mermaid");
            print!("{}", mermaid::before(&model));
            println!("```");
        }
        if cli.mermaid_after {
            println!("\n```mermaid");
            print!("{}", mermaid::after(&model, &ord));
            println!("```");
        }
        if cli.mermaid_diff {
            println!("\nbefore/after order:");
            print!("{}", mermaid::diff_table(&model, &ord));
            println!("\n```mermaid");
            print!("{}", mermaid::diff(&model, &ord));
            println!("```");
        }

        if cli.stdout {
            println!("\n----- reordered -----");
            print!("{new_src}");
            if !new_src.ends_with('\n') {
                println!();
            }
            println!("----- end -----");
        }

        if cli.write {
            if changed {
                if let Err(e) = fs::write(path, &new_src) {
                    eprintln!("{}: cannot write: {e}", path.display());
                } else {
                    println!("\n(written in place)");
                }
            } else {
                println!("\n(no change)");
            }
        } else if !changed {
            println!("\n(already in dependency order - no change)");
        } else {
            println!("\n(dry run - pass --write to apply)");
        }

        let moved = ord
            .order
            .iter()
            .enumerate()
            .filter(|(row, &idx)| *row != idx)
            .count();
        print_summary(&model, &ord, moved);

        total_items += model.items.len();
        total_moved += moved;
        total_mutual += ord.mutual_groups.len();
        if changed {
            total_changed += 1;
        }
        println!();
    }

    if files.len() > 1 {
        println!("=== totals ===");
        println!("  files:          {}", files.len());
        println!("  changed:        {total_changed}");
        println!("  items ordered:  {total_items}");
        println!("  items moved:    {total_moved}");
        println!("  mutual groups:  {total_mutual}");
    }
}

/// Render the new file from the model + ordering. Lossless except for blank-line
/// normalisation and possible relocation of stand-alone comments.
fn emit(model: &analyze::FileModel, ord: &order::Ordering) -> String {
    let mut segments: Vec<String> = Vec::new();

    let pre = model.preamble.trim_end();
    if !pre.is_empty() {
        segments.push(pre.to_string());
    }

    for p in &model.pinned {
        segments.push(block(&p.lead, &p.body));
    }

    for f in &model.floating {
        let t = f.trim();
        if !t.is_empty() {
            segments.push(t.to_string());
        }
    }

    let items = &model.items;
    let mut i = 0;
    while i < ord.order.len() {
        let idx = ord.order[i];
        if ord.in_mutual[idx] {
            let comp = ord.comp_of[idx];
            let mut group = String::from("// mutual start\n");
            let mut first = true;
            while i < ord.order.len()
                && ord.in_mutual[ord.order[i]]
                && ord.comp_of[ord.order[i]] == comp
            {
                let m = ord.order[i];
                if !first {
                    group.push_str("\n\n");
                }
                first = false;
                group.push_str(&block(&items[m].lead, &items[m].body));
                i += 1;
            }
            group.push_str("\n// mutual end");
            segments.push(group);
        } else {
            segments.push(block(&items[idx].lead, &items[idx].body));
            i += 1;
        }
    }

    let post = model.postamble.trim();
    if !post.is_empty() {
        segments.push(post.to_string());
    }

    let mut out = segments.join("\n\n");
    out.push('\n');
    out
}

fn block(lead: &str, body: &str) -> String {
    if lead.trim().is_empty() {
        body.to_string()
    } else {
        format!("{}\n{}", lead.trim_end(), body)
    }
}

fn print_summary(model: &analyze::FileModel, ord: &order::Ordering, moved: usize) {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for it in &model.items {
        *counts.entry(it.kind.label()).or_default() += 1;
    }
    for p in &model.pinned {
        *counts.entry(p.kind.label()).or_default() += 1;
    }
    let edges: usize = model.items.iter().map(|i| i.deps.len()).sum();

    println!("\nsummary:");
    let kinds: Vec<String> = counts.iter().map(|(k, v)| format!("{k}={v}")).collect();
    println!("  items:            {} ({})", model.items.len(), kinds.join(", "));
    println!("  dependency edges: {edges}");
    println!("  items moved:      {moved} / {}", model.items.len());
    println!("  mutual groups:    {}", ord.mutual_groups.len());
    for (gi, g) in ord.mutual_groups.iter().enumerate() {
        let members: Vec<&str> = g
            .iter()
            .map(|&m| {
                model.items[m]
                    .name
                    .as_deref()
                    .unwrap_or(model.items[m].display.as_str())
            })
            .collect();
        println!("    mutual #{:<2} ({}): {}", gi + 1, g.len(), members.join(", "));
    }
}
