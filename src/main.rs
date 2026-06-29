//! rsorder CLI: reorder Rust items definition-before-use, Lean-style mutual
//! blocks, scoped `// TO REORDER` regions, with HTML/Mermaid views.

use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use rsorder::order::{OrderOpts, Tie};
use rsorder::{check, render, reorder, Outcome};

#[derive(Clone)]
struct Cli {
    mode: Mode,
    patterns: Vec<String>,
    inside_alpha: bool,
    outside_topological: bool,
    outside_alpha: bool,
    mermaid_before: bool,
    mermaid_after: bool,
    html_diff: bool,
    write: bool,
    stdout: bool,
    color: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Reorder,
    Check,
}

const USAGE: &str = "\
rsorder - reorder Rust items definition-before-use, wrap mutual cycles

USAGE:
    rsorder reorder [OPTIONS] <GLOB>...
    rsorder check [OPTIONS] <GLOB>...
    rsorder [OPTIONS] <GLOB>...

ORDERING:
        --same-level-inside-of-mutual--alphabetically
        --same-level-outside-of-mutual--alphabetically
        --same-level-outside-of-mutual--topological
            Set the global ordering policy; a `// TO REORDER <opts>` region
            header overrides it. The default is original order for ties.

MODES:
    reorder is the default and rewrites/dry-runs reordered source.
    check reports and exits nonzero when an item uses a later non-mutual item.

SCOPED MODE:
    If a file contains at least one `// TO REORDER` ... `// TO REORDER END`
    region, ONLY the declarations inside those regions are reordered.

OUTPUTS (written under a per-run temp dir, opened with xdg-open):
        --mermaid-write-before     write <file>-before.mermaid (+ svg via mmdr)
        --mermaid-write-after      write <file>-after.mermaid  (+ svg via mmdr)
        --write-html-before-after-diff-table
                                   write <file>-before-after.html and open it
    -w, --write                    rewrite the .rs files in place
        --stdout                   also print reordered contents to stdout
        --no-color                 disable ANSI colors
    -h, --help                     show this help";

fn parse_cli() -> Cli {
    let mut args = std::env::args().skip(1).peekable();
    let mode = match args.peek().map(String::as_str) {
        Some("reorder") => {
            let _ = args.next();
            Mode::Reorder
        }
        Some("check") => {
            let _ = args.next();
            Mode::Check
        }
        _ => Mode::Reorder,
    };
    let mut c = Cli {
        mode,
        patterns: vec![],
        inside_alpha: false,
        outside_topological: false,
        outside_alpha: false,
        mermaid_before: false,
        mermaid_after: false,
        html_diff: false,
        write: false,
        stdout: false,
        color: std::io::stdout().is_terminal(),
    };
    for a in args {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--same-level-inside-of-mutual--alphabetically" => c.inside_alpha = true,
            "--same-level-outside-of-mutual--alphabetically" => c.outside_alpha = true,
            "--same-level-outside-of-mutual--topological" => c.outside_topological = true,
            "--mermaid-write-before" => c.mermaid_before = true,
            "--mermaid-write-after" => c.mermaid_after = true,
            "--write-html-before-after-diff-table" => c.html_diff = true,
            "-w" | "--write" => c.write = true,
            "--stdout" => c.stdout = true,
            "--no-color" => c.color = false,
            s if s.starts_with('-') => {
                eprintln!("unknown option: {s}\n\n{USAGE}");
                std::process::exit(2);
            }
            s => c.patterns.push(s.to_string()),
        }
    }
    if c.patterns.is_empty() {
        eprintln!("error: at least one glob pattern is required\n\n{USAGE}");
        std::process::exit(2);
    }
    if c.outside_alpha && c.outside_topological {
        eprintln!(
            "error: choose only one outside-mutual ordering mode\n\n{USAGE}"
        );
        std::process::exit(2);
    }
    c
}

// ----- tiny ANSI palette -----
#[derive(Clone, Copy)]
struct Paint {
    on: bool,
}
impl Paint {
    fn w(&self, s: &str, code: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn bold(&self, s: &str) -> String { self.w(s, "1") }
    fn green(&self, s: &str) -> String { self.w(s, "32") }
    fn yellow(&self, s: &str) -> String { self.w(s, "33") }
    fn cyan(&self, s: &str) -> String { self.w(s, "36") }
    fn dim(&self, s: &str) -> String { self.w(s, "2") }
}

struct FileReport {
    lines: Vec<String>,
    moved: usize,
    mutual: usize,
    items: usize,
    changed: bool,
    failed: bool,
}

#[tokio::main]
async fn main() {
    let cli = parse_cli();
    let paint = Paint { on: cli.color };

    let files: BTreeSet<PathBuf> = cli
        .patterns
        .iter()
        .filter_map(|p| glob::glob(p).ok())
        .flat_map(|paths| paths.flatten())
        .filter(|p| p.is_file() && p.extension().map(|e| e == "rs").unwrap_or(false))
        .collect();

    if files.is_empty() {
        eprintln!("no .rs files matched the given pattern(s)");
        std::process::exit(1);
    }

    let tmp_dir = std::env::temp_dir().join(format!("rsorder-{}", std::process::id()));
    let _ = tokio::fs::create_dir_all(&tmp_dir).await;
    let mmdr = in_path("mmdr");
    let xdg = in_path("xdg-open");

    let opts = OrderOpts {
        inside: if cli.inside_alpha { Tie::Alphabetical } else { Tie::Original },
        outside: if cli.outside_alpha {
            Tie::Alphabetical
        } else if cli.outside_topological {
            Tie::Topological
        } else {
            Tie::Original
        },
    };

    let mut set = tokio::task::JoinSet::new();
    for path in files.clone() {
        let cli = cli.clone();
        let tmp_dir = tmp_dir.clone();
        set.spawn(async move {
            process_file(path, cli, opts, tmp_dir, mmdr, xdg, paint).await
        });
    }

    let mut reports: Vec<FileReport> = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(Some(rep)) = joined {
            reports.push(rep);
        }
    }
    reports.sort_by(|a, b| a.lines.first().cmp(&b.lines.first()));

    for rep in &reports {
        for l in &rep.lines {
            println!("{l}");
        }
        println!();
    }

    if reports.len() > 1 {
        let changed = reports.iter().filter(|r| r.changed).count();
        let moved: usize = reports.iter().map(|r| r.moved).sum();
        let mutual: usize = reports.iter().map(|r| r.mutual).sum();
        let items: usize = reports.iter().map(|r| r.items).sum();
        println!("{}", paint.bold("totals"));
        println!("  files:         {}", reports.len());
        println!("  changed:       {changed}");
        println!("  items ordered: {items}");
        println!("  items moved:   {moved}");
        println!("  mutual groups: {mutual}");
    }

    if reports.iter().any(|r| r.failed) {
        std::process::exit(1);
    }
}

async fn process_file(
    path: PathBuf,
    cli: Cli,
    opts: OrderOpts,
    tmp_dir: PathBuf,
    mmdr: bool,
    xdg: bool,
    paint: Paint,
) -> Option<FileReport> {
    let src = tokio::fs::read_to_string(&path).await.ok()?;
    if cli.mode == Mode::Check {
        return process_check(path, src, paint).await;
    }

    // CPU-bound transform off the async runtime.
    let outcome: Result<Outcome, String> = tokio::task::spawn_blocking(move || {
        reorder(&src, opts).map_err(|e| e.to_string())
    })
    .await
    .ok()?;

    let mut lines = vec![paint.bold(&format!("=== {} ===", path.display()))];

    let outcome = match outcome {
        Ok(o) => o,
        Err(e) => {
            lines.push(paint.yellow(&format!("parse error: {e}")));
            return Some(FileReport {
                lines,
                moved: 0,
                mutual: 0,
                items: 0,
                changed: false,
                failed: true,
            });
        }
    };

    let original = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    let changed = outcome.new_src != original;
    let plan = &outcome.plan;

    if plan.scoped {
        lines.push(paint.cyan(&format!(
            "scoped mode: found {} `// TO REORDER` region(s) — only their contents are reordered",
            plan.region_count
        )));
    }

    // ----- side outputs -----
    let stem = flat_name(&path);
    let mut openers: Vec<PathBuf> = Vec::new();

    if cli.mermaid_before {
        let f = tmp_dir.join(format!("{stem}-before.mermaid"));
        let _ = tokio::fs::write(&f, render::mermaid_before(plan)).await;
        lines.push(paint.dim(&format!("wrote {}", f.display())));
        if let Some(svg) = render_mermaid(&f, mmdr).await {
            openers.push(svg);
        }
    }
    if cli.mermaid_after {
        let f = tmp_dir.join(format!("{stem}-after.mermaid"));
        let _ = tokio::fs::write(&f, render::mermaid_after(plan)).await;
        lines.push(paint.dim(&format!("wrote {}", f.display())));
        if let Some(svg) = render_mermaid(&f, mmdr).await {
            openers.push(svg);
        }
    }
    if cli.html_diff {
        let f = tmp_dir.join(format!("{stem}-before-after.html"));
        let html = render::html_diff(plan, &path.display().to_string());
        let _ = tokio::fs::write(&f, html).await;
        lines.push(paint.dim(&format!("wrote {}", f.display())));
        openers.push(f);
    }
    for f in &openers {
        open(f, xdg).await;
    }

    // ----- write / stdout -----
    if cli.write {
        if changed {
            match tokio::fs::write(&path, &outcome.new_src).await {
                Ok(_) => lines.push(paint.green("written in place")),
                Err(e) => lines.push(paint.yellow(&format!("cannot write: {e}"))),
            }
        } else {
            lines.push(paint.dim("no change"));
        }
    } else if changed {
        lines.push(paint.dim("dry run — pass --write to apply"));
    } else {
        lines.push(paint.dim("already in dependency order — no change"));
    }
    if cli.stdout {
        lines.push("----- reordered -----".to_string());
        lines.push(outcome.new_src.trim_end().to_string());
        lines.push("----- end -----".to_string());
    }

    // ----- summary -----
    lines.push(paint.bold("summary:"));
    let kinds = plan
        .kind_counts
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(format!("  items:            {} ({kinds})", plan.nodes.len()));
    lines.push(format!("  dependency edges: {}", plan.edges));
    lines.push(format!("  items moved:      {} / {}", plan.moved, plan.nodes.len()));
    lines.push(format!("  mutual groups:    {}", plan.mutual_groups.len()));
    for (gi, g) in plan.mutual_groups.iter().enumerate() {
        let members = g
            .iter()
            .map(|&c| {
                plan.nodes[c]
                    .name
                    .clone()
                    .unwrap_or_else(|| plan.nodes[c].display.clone())
            })
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("    mutual #{:<2} ({}): {members}", gi + 1, g.len()));
    }

    Some(FileReport {
        lines,
        moved: plan.moved,
        mutual: plan.mutual_groups.len(),
        items: plan.nodes.len(),
        changed,
        failed: false,
    })
}

async fn process_check(path: PathBuf, src: String, paint: Paint) -> Option<FileReport> {
    let report = tokio::task::spawn_blocking(move || check(&src).map_err(|e| e.to_string()))
        .await
        .ok()?;
    let mut lines = vec![paint.bold(&format!("=== {} ===", path.display()))];
    match report {
        Ok(report) if report.is_ok() => {
            lines.push(paint.green("ok"));
            Some(FileReport {
                lines,
                moved: 0,
                mutual: 0,
                items: 0,
                changed: false,
                failed: false,
            })
        }
        Ok(report) => {
            lines.push(paint.yellow(&format!(
                "{} before-definition use(s) found:",
                report.violations.len()
            )));
            for v in report.violations {
                lines.push(format!(
                    "  {} uses later {} (item #{} before #{})",
                    v.user,
                    v.dependency,
                    v.user_index + 1,
                    v.dependency_index + 1
                ));
            }
            Some(FileReport {
                lines,
                moved: 0,
                mutual: 0,
                items: 0,
                changed: false,
                failed: true,
            })
        }
        Err(e) => {
            lines.push(paint.yellow(&format!("parse error: {e}")));
            Some(FileReport {
                lines,
                moved: 0,
                mutual: 0,
                items: 0,
                changed: false,
                failed: true,
            })
        }
    }
}


/// Render a .mermaid file to .svg with `mmdr` if available; return the svg path.
async fn render_mermaid(mermaid: &Path, mmdr: bool) -> Option<PathBuf> {
    if !mmdr {
        return None;
    }
    let svg = mermaid.with_extension("svg");
    let ok = tokio::process::Command::new("mmdr")
        .args(["-i", &mermaid.to_string_lossy(), "-o", &svg.to_string_lossy(), "-e", "svg"])
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);
    ok.then_some(svg)
}

async fn open(path: &Path, xdg: bool) {
    if !xdg {
        return;
    }
    let _ = tokio::process::Command::new("xdg-open").arg(path).spawn();
}

fn in_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

/// Flatten a path into a single safe file-name stem.
fn flat_name(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(['.', '/', '\\'])
        .replace(['/', '\\'], "_")
}
