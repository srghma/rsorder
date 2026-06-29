//! rsorder CLI: reorder Rust items definition-before-use, Lean-style mutual
//! blocks, scoped `// TO REORDER` regions, with HTML/Mermaid views.

use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use rsorder::order::{InsideMutualTie, NonMutualTie, OrderOpts};
use rsorder::{Outcome, check, render, reorder};

#[derive(Clone)]
struct Cli {
    mode: Mode,
    patterns: Vec<String>,
    inside: InsideMutualTie,
    outside: NonMutualTie,
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

fn parse_cli() -> Cli {
    let args = RawCli::parse();
    let (mode, opts) = match args.command {
        Command::Reorder(opts) => (Mode::Reorder, opts),
        Command::Check(opts) => (Mode::Check, opts),
    };
    Cli {
        mode,
        patterns: opts.patterns,
        inside: opts.sorting_inside_mutual.into(),
        outside: opts.sorting_non_mutual.into(),
        mermaid_before: opts.mermaid_before,
        mermaid_after: opts.mermaid_after,
        html_diff: opts.html_diff,
        write: opts.write,
        stdout: opts.stdout,
        color: !opts.no_color && std::io::stdout().is_terminal(),
    }
}

#[derive(Parser)]
#[command(about = "Reorder Rust items definition-before-use, wrapping mutual cycles")]
struct RawCli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Rewrites or dry-runs reordered source.
    Reorder(CommandOpts),
    /// Reports and exits nonzero when an item uses a later non-mutual item.
    Check(CommandOpts),
}

#[derive(Args)]
struct CommandOpts {
    /// Sort independent items/components outside mutual blocks.
    #[arg(long, value_enum, default_value_t = NonMutualSorting::Original)]
    sorting_non_mutual: NonMutualSorting,
    /// Sort members inside a mutual block.
    #[arg(long, value_enum, default_value_t = InsideMutualSorting::Original)]
    sorting_inside_mutual: InsideMutualSorting,
    /// Write <file>-before.mermaid under a per-run temp dir.
    #[arg(long = "mermaid-write-before")]
    mermaid_before: bool,
    /// Write <file>-after.mermaid under a per-run temp dir.
    #[arg(long = "mermaid-write-after")]
    mermaid_after: bool,
    /// Write <file>-before-after.html under a per-run temp dir.
    #[arg(long = "write-html-before-after-diff-table")]
    html_diff: bool,
    /// Rewrite the .rs files in place.
    #[arg(short, long)]
    write: bool,
    /// Also print reordered contents to stdout.
    #[arg(long)]
    stdout: bool,
    /// Disable ANSI colors.
    #[arg(long)]
    no_color: bool,
    /// Glob pattern(s) for .rs files.
    #[arg(required = true, num_args = 1..)]
    patterns: Vec<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum NonMutualSorting {
    Original,
    Alphabetical,
    Topological,
}

impl std::fmt::Display for NonMutualSorting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.to_possible_value().unwrap().get_name().fmt(f)
    }
}

impl From<NonMutualSorting> for NonMutualTie {
    fn from(value: NonMutualSorting) -> Self {
        match value {
            NonMutualSorting::Original => NonMutualTie::Original,
            NonMutualSorting::Alphabetical => NonMutualTie::Alphabetical,
            NonMutualSorting::Topological => NonMutualTie::Topological,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum InsideMutualSorting {
    Original,
    Alphabetical,
}

impl std::fmt::Display for InsideMutualSorting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.to_possible_value().unwrap().get_name().fmt(f)
    }
}

impl From<InsideMutualSorting> for InsideMutualTie {
    fn from(value: InsideMutualSorting) -> Self {
        match value {
            InsideMutualSorting::Original => InsideMutualTie::Original,
            InsideMutualSorting::Alphabetical => InsideMutualTie::Alphabetical,
        }
    }
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
    fn bold(&self, s: &str) -> String {
        self.w(s, "1")
    }
    fn green(&self, s: &str) -> String {
        self.w(s, "32")
    }
    fn yellow(&self, s: &str) -> String {
        self.w(s, "33")
    }
    fn cyan(&self, s: &str) -> String {
        self.w(s, "36")
    }
    fn dim(&self, s: &str) -> String {
        self.w(s, "2")
    }
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
        inside: cli.inside,
        outside: cli.outside,
    };

    let mut set = tokio::task::JoinSet::new();
    for path in files.clone() {
        let cli = cli.clone();
        let tmp_dir = tmp_dir.clone();
        set.spawn(async move { process_file(path, cli, opts, tmp_dir, mmdr, xdg, paint).await });
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
    let outcome: Result<Outcome, String> =
        tokio::task::spawn_blocking(move || reorder(&src, opts).map_err(|e| e.to_string()))
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
    lines.push(format!(
        "  items:            {} ({kinds})",
        plan.nodes.len()
    ));
    lines.push(format!("  dependency edges: {}", plan.edges));
    lines.push(format!(
        "  items moved:      {} / {}",
        plan.moved,
        plan.nodes.len()
    ));
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
        lines.push(format!(
            "    mutual #{:<2} ({}): {members}",
            gi + 1,
            g.len()
        ));
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
                    "  {} uses later {} (line {} before line {}, item #{} before #{})",
                    v.user,
                    v.dependency,
                    v.user_line,
                    v.dependency_line,
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
        .args([
            "-i",
            &mermaid.to_string_lossy(),
            "-o",
            &svg.to_string_lossy(),
            "-e",
            "svg",
        ])
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
