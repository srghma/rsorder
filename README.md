# rsorder

Reorder top-level items in Rust source files so that **definitions come before
their uses** (Lean-style), wrapping any genuine cycles (mutual recursion) in
`// mutual start` / `// mutual end` markers. It also prints a summary and can
emit Mermaid dependency graphs.

It parses each file with [`syn`], builds a dependency graph over the named
top-level items (`fn`, `struct`, `enum`, `union`, `trait`, `const`, `static`,
`type`, `mod`, `macro_rules!`, `impl`, item-level macro invocations), finds the
strongly-connected components (Tarjan), and emits the condensation in
topological order so every item appears after the things it references.

## Build

```sh
cargo build --release
# binary at target/release/rsorder
```

## Usage

```
rsorder [OPTIONS] <GLOB>...

ARGS:
    <GLOB>...   One or more glob patterns matching .rs files (e.g. 'src/**/*.rs')

OPTIONS:
        --same-level-alphabetically   Sort items alphabetically inside a mutual
                                       group (default: preserve original order)
        --mermaid-before              Print Mermaid dependency graph (original order)
        --mermaid-after               Print Mermaid dependency graph (reordered,
                                       with mutual groups as subgraphs)
        --mermaid-diff                Print a before|after table + a Mermaid diff
                                       whose crossing arrows show what moved
    -w, --write                       Rewrite files in place (default: dry run)
        --stdout                      Print the reordered contents to stdout
    -h, --help                        Show help
```

By default nothing is written — it's a dry run that reports what would change.
Pass `--write` to apply, or `--stdout` to see the result.

### Examples

```sh
# See the reordered file without touching disk
rsorder sample/demo.rs --stdout

# Apply across a tree, alphabetising inside mutual blocks
rsorder 'src/**/*.rs' --write --same-level-alphabetically

# Show dependency graphs and the movement diff
rsorder sample/demo.rs --mermaid-before --mermaid-after --mermaid-diff
```

## What the output looks like

A cycle is wrapped exactly like Lean's `mutual ... end`:

```rust
// mutual start
fn ping(n: u32) -> u32 { if n == 0 { 0 } else { pong(n - 1) } }

fn pong(n: u32) -> u32 { if n == 0 { 1 } else { ping(n - 1) } }
// mutual end
```

Inside a mutual block the members keep their original relative order, unless
`--same-level-alphabetically` is given, in which case they are sorted by name.

`--mermaid-diff` prints the two-column view you asked for plus a Mermaid diagram
that links each item's old slot to its new slot (crossing lines = movement):

```
  BEFORE          AFTER
  ------------------------------
  fn run       => const SCALE
  const SCALE  => struct Helper
  ...
```

## Ordering rules

* `use` / `extern crate` (and `extern { }` blocks) are *pinned* at the top in
  their original order — reordering imports is rarely wanted and can change
  macro/`#[macro_use]` semantics.
* Everything else is ordered dependencies-first. Independent items keep their
  original relative order (stable), so diffs stay small.
* A self-recursive function is **not** a mutual block; only true cycles between
  two or more items are.
* `macro_rules!` definitions are treated as dependencies of the code that
  invokes them, so a macro ends up before its call sites (which Rust requires).

## How dependencies are detected

For each item, every path segment and every identifier appearing inside macro
invocations is matched against the set of names defined in the file. This is a
deliberate over-approximation: it never misses a real reference, and at worst
adds a spurious edge if a local happens to share a top-level name. Field and
method names are not treated as references.

## Preservation guarantees

* The original bytes of every item (attributes, doc comments, bodies — including
  non-ASCII) are sliced verbatim from the source and never rewritten.
* The license header / inner attributes above the first item are kept verbatim.
* A comment directly above an item (no blank line between) travels with it.
* The tool's own `// mutual start/end` markers are stripped on input, so running
  it repeatedly is **idempotent**.

### Known limitations

* Only top-level items are reordered; items nested inside an inline `mod { ... }`
  are treated as one opaque block.
* Stand-alone comments separated from any item by blank lines are kept (never
  dropped) but are gathered just after the imports rather than at their exact
  original line.
* Blank runs between items are normalised to a single blank line.
* Name resolution is file-local and textual; it does not perform full path or
  hygiene resolution.

## Layout

| file             | purpose                                             |
|------------------|-----------------------------------------------------|
| `src/main.rs`    | CLI, file walking, emit, summary                    |
| `src/analyze.rs` | parse, byte-span slicing, comment attachment, edges |
| `src/order.rs`   | Tarjan SCC + stable topological ordering            |
| `src/model.rs`   | item model + classification                         |
| `src/mermaid.rs` | before / after / diff renderings                    |

[`syn`]: https://docs.rs/syn
