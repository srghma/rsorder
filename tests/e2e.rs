//! End-to-end golden tests — self-contained (inputs embedded inline).
//!
//! Each case reorders an embedded input with a fixed policy and compares
//! against `tests/golden/<name>.rs`. The expected snapshot is created
//! automatically on first run (or regenerated with `BLESS=1 cargo test`).
//! The transform must also be idempotent.

use std::fs;
use std::path::PathBuf;

use rsorder::order::{OrderOpts, Tie};
use rsorder::reorder;

const WHOLE_BASIC: &str = r#"use std::collections::HashMap;

pub fn run(c: Config) -> u32 { Helper::new().apply(c.value) + ping(3) }

const SCALE: u32 = 10;

pub struct Helper { cache: HashMap<u32, u32> }

impl Helper {
    fn new() -> Helper { Helper { cache: HashMap::new() } }
    fn apply(&self, v: u32) -> u32 { v * SCALE }
}

pub struct Config { pub value: u32 }

fn ping(n: u32) -> u32 { if n == 0 { 0 } else { pong(n - 1) } }
fn pong(n: u32) -> u32 { if n == 0 { 1 } else { ping(n - 1) } }
"#;

const SCOPED_REGIONS: &str = r#"use std::fmt;

pub fn keep_first() -> u32 { keep_second() }
pub fn keep_second() -> u32 { 0 }

// TO REORDER same-level-outside-of-mutual--alphabetically
pub fn zeta() -> u32 { alpha() }
pub fn alpha() -> u32 { 1 }
const BETA: u32 = 2;
// TO REORDER END

// TO REORDER
fn user() -> u32 { even(4) }
fn even(n: u32) -> u32 { if n == 0 { 1 } else { odd(n - 1) } }
fn odd(n: u32) -> u32 { if n == 0 { 0 } else { even(n - 1) } }
// TO REORDER END

pub fn keep_last() -> u32 { keep_first() }
"#;

struct Case {
    name: &'static str,
    input: &'static str,
    opts: OrderOpts,
}

fn cases() -> Vec<Case> {
    let orig = OrderOpts { inside: Tie::Original, outside: Tie::Original };
    let alpha = OrderOpts { inside: Tie::Alphabetical, outside: Tie::Alphabetical };
    vec![
        Case { name: "whole_basic", input: WHOLE_BASIC, opts: orig },
        Case { name: "whole_alpha_both", input: WHOLE_BASIC, opts: alpha },
        Case { name: "scoped_regions", input: SCOPED_REGIONS, opts: orig },
    ]
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

#[test]
fn golden() {
    let bless = std::env::var("BLESS").is_ok();
    let dir = golden_dir();
    fs::create_dir_all(&dir).unwrap();
    let mut failures = Vec::new();

    for case in cases() {
        let out = reorder(case.input, case.opts)
            .unwrap_or_else(|e| panic!("reorder {}: {e}", case.name))
            .new_src;

        // Idempotence: reordering the output again is a no-op.
        let twice = reorder(&out, case.opts).unwrap().new_src;
        assert_eq!(out, twice, "case `{}` is not idempotent", case.name);

        let path = dir.join(format!("{}.rs", case.name));
        match fs::read_to_string(&path) {
            Ok(expected) if !bless => {
                if out != expected {
                    failures.push(case.name);
                    eprintln!("--- mismatch for `{}` ---\n{out}", case.name);
                }
            }
            // Missing snapshot or BLESS: write it and pass.
            _ => fs::write(&path, &out).unwrap(),
        }
    }

    assert!(failures.is_empty(), "golden mismatches: {failures:?} (run BLESS=1 to update)");
}

/// Outside items stay fixed and regions are detected in scoped mode.
#[test]
fn scoped_keeps_outside_fixed() {
    let out = reorder(SCOPED_REGIONS, OrderOpts { inside: Tie::Original, outside: Tie::Original }).unwrap();
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
    let out = reorder(src, OrderOpts { inside: Tie::Original, outside: Tie::Original }).unwrap();
    assert_eq!(out.plan.mutual_groups.len(), 1);
    assert_eq!(out.plan.mutual_groups[0].len(), 2);
    assert!(out.new_src.contains("// mutual start"));
}
