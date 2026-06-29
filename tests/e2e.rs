//! End-to-end golden tests.
//!
//! Each case reorders an input fixture with a fixed policy and compares source
//! and dependency JSON snapshots under `tests/golden/`. The expected snapshots
//! are created automatically on first run (or regenerated with
//! `BLESS=1 cargo test`). The transform must also be idempotent.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use rsorder::order::{OrderOpts, Tie};
use rsorder::{check, render, reorder};

struct Case {
    name: &'static str,
    input_file: &'static str,
    opts: OrderOpts,
}

fn cases() -> Vec<Case> {
    let orig = OrderOpts {
        inside: Tie::Original,
        outside: Tie::Original,
    };
    let alpha = OrderOpts {
        inside: Tie::Alphabetical,
        outside: Tie::Alphabetical,
    };
    let topo = OrderOpts {
        inside: Tie::Original,
        outside: Tie::Topological,
    };
    vec![
        Case {
            name: "whole_original",
            input_file: "whole_basic.rs",
            opts: orig,
        },
        Case {
            name: "whole_alphabetical",
            input_file: "whole_basic.rs",
            opts: alpha,
        },
        Case {
            name: "whole_topological",
            input_file: "whole_basic.rs",
            opts: topo,
        },
        Case {
            name: "scoped_regions",
            input_file: "scoped_regions.rs",
            opts: orig,
        },
    ]
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn input_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/input")
}

#[test]
fn golden() {
    let bless = std::env::var("BLESS").is_ok();
    let dir = golden_dir();
    let input_dir = input_dir();
    fs::create_dir_all(&dir).unwrap();
    let mut failures = Vec::new();

    for case in cases() {
        let input = fs::read_to_string(input_dir.join(case.input_file)).unwrap();
        let outcome =
            reorder(&input, case.opts).unwrap_or_else(|e| panic!("reorder {}: {e}", case.name));
        let out = outcome.new_src;
        let graph = render::dependency_tree_json(&outcome.plan);

        // Idempotence: reordering the output again is a no-op.
        let twice = reorder(&out, case.opts).unwrap().new_src;
        assert_eq!(out, twice, "case `{}` is not idempotent", case.name);

        let path = dir.join(format!("{}.rs", case.name));
        match fs::read_to_string(&path) {
            Ok(expected) if !bless => {
                if out != expected {
                    failures.push(format!("{}.rs", case.name));
                    eprintln!("--- mismatch for `{}` ---\n{out}", case.name);
                }
            }
            // Missing snapshot or BLESS: write it and pass.
            _ => fs::write(&path, &out).unwrap(),
        }

        let graph_path = dir.join(format!("{}.deps.json", case.name));
        match fs::read_to_string(&graph_path) {
            Ok(expected) if !bless => {
                if graph != expected {
                    failures.push(format!("{}.deps.json", case.name));
                    eprintln!(
                        "--- dependency JSON mismatch for `{}` ---\n{graph}",
                        case.name
                    );
                }
            }
            _ => fs::write(&graph_path, graph).unwrap(),
        }
    }

    assert!(
        failures.is_empty(),
        "golden mismatches: {failures:?} (run BLESS=1 to update)"
    );
}

/// Outside items stay fixed and regions are detected in scoped mode.
#[test]
fn scoped_keeps_outside_fixed() {
    let src = fs::read_to_string(input_dir().join("scoped_regions.rs")).unwrap();
    let out = reorder(
        &src,
        OrderOpts {
            inside: Tie::Original,
            outside: Tie::Original,
        },
    )
    .unwrap();
    assert!(out.plan.scoped);
    assert_eq!(out.plan.region_count, 2);
    let p1 = out.new_src.find("fn keep_first").unwrap();
    let p2 = out.new_src.find("fn keep_second").unwrap();
    let p3 = out.new_src.find("fn keep_last").unwrap();
    assert!(p1 < p2 && p2 < p3, "outside items must keep their order");
}

/// A real cycle becomes a mutual block; a self-recursive fn does not.
#[test]
fn mutual_detection() {
    let src = "fn a(){b();} fn b(){a();} fn c(){c();}";
    let out = reorder(
        src,
        OrderOpts {
            inside: Tie::Original,
            outside: Tie::Original,
        },
    )
    .unwrap();
    assert_eq!(out.plan.mutual_groups.len(), 1);
    assert_eq!(out.plan.mutual_groups[0].len(), 2);
    assert!(out.new_src.contains("// mutual start"));
}

#[test]
fn check_fails_on_later_non_mutual_dependency() {
    let bad = "fn user(){dep();}\nfn dep(){}";
    let report = check(bad).unwrap();
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].user, "user");
    assert_eq!(report.violations[0].dependency, "dep");

    let mutual = "fn a(){b();}\nfn b(){a();}";
    assert!(check(mutual).unwrap().is_ok());
}

#[test]
fn outside_topological_emits_dependency_chain_before_unrelated_items() {
    let src = "fn user(){dep();}\nfn unrelated(){}\nfn dep(){}";
    let out = reorder(
        src,
        OrderOpts {
            inside: Tie::Original,
            outside: Tie::Topological,
        },
    )
    .unwrap()
    .new_src;
    let dep = out.find("fn dep").unwrap();
    let user = out.find("fn user").unwrap();
    let unrelated = out.find("fn unrelated").unwrap();
    assert!(dep < user && user < unrelated, "{out}");
}

#[test]
fn cli_requires_command_and_check_reports_violations() {
    let bin = env!("CARGO_BIN_EXE_rsorder");
    let input = input_dir().join("whole_basic.rs");
    let bare = Command::new(bin).arg(&input).output().unwrap();
    assert!(!bare.status.success());
    assert!(String::from_utf8_lossy(&bare.stderr).contains("expected command"));

    let path = std::env::temp_dir().join(format!(
        "rsorder-check-{}-{}.rs",
        std::process::id(),
        "violation"
    ));
    fs::write(&path, "fn user(){dep();}\nfn dep(){}\n").unwrap();
    let check = Command::new(bin)
        .args(["check", "--no-color"])
        .arg(&path)
        .output()
        .unwrap();
    let _ = fs::remove_file(&path);

    assert!(!check.status.success());
    let stdout = String::from_utf8_lossy(&check.stdout);
    assert!(stdout.contains("user uses later dep"), "{stdout}");
    assert!(!stdout.contains("items moved"), "{stdout}");
    assert!(!stdout.contains("dry run"), "{stdout}");
}
