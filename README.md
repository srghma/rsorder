# rsorder

Reorder top-level items in Rust source files so **definitions come before their
uses** (Lean-style), wrapping genuine cycles (mutual recursion) in
`// mutual start` / `// mutual end` markers. Prints a colored summary and can
emit standalone HTML / Mermaid views of the dependency graph and the movement.

Built as a library (`rsorder`) plus an async (`tokio`) CLI; files are processed
concurrently and the pure transform runs on a blocking worker.

## Build

```sh
cargo build --release      # target/release/rsorder
cargo test                 # unit + golden e2e tests
```

## Usage

```
rsorder [OPTIONS] <GLOB>...
```

### Ordering policy

Two independent tie-break controls (each defaults to **original order**):

* `--same-level-inside-of-mutual--alphabetically` — sort members *inside* a
  mutual block alphabetically.
* `--same-level-outside-of-mutual--alphabetically` — sort independent
  items/components *outside* mutual blocks alphabetically.

### Scoped mode — `// TO REORDER` regions

If a file contains at least one region:

```rust
// TO REORDER
... items ...
// TO REORDER END
```

then **only the declarations inside the regions are reordered**; everything
else keeps its exact position. The CLI prints that scoped mode was selected and
how many regions were found.

A region header may override the global policy for that region only, using the
same token names as the flags (without the leading `--`):

```rust
// TO REORDER same-level-inside-of-mutual--alphabetically same-level-outside-of-mutual--original
... items ...
// TO REORDER END
```

Any option not named in the header is inherited from the CLI.

### Outputs

Side outputs are written under a per-run temp dir
(`/tmp/rsorder-<pid>/`) named after the source file, and opened with `xdg-open`
when available:

* `--write-html-before-after-diff-table` → `<file>-before-after.html`: a
  two-column BEFORE/AFTER diagram with arrows linking each item's old row to its
  new row (red = moved). Opened automatically.
* `--mermaid-write-before` → `<file>-before.mermaid` (dependency graph, source
  order).
* `--mermaid-write-after` → `<file>-after.mermaid` (reordered layout; mutual
  groups become `subgraph`s). If `mmdr` is on `PATH`, each `.mermaid` is rendered
  with `mmdr -i in.mermaid -o out.svg -e svg` and the SVG is opened.

### Applying / output

* `-w`, `--write` — rewrite the `.rs` files in place (default is a dry run).
* `--stdout` — also print the reordered contents.
* `--no-color` — disable ANSI colors (colors are on automatically when stdout is
  a TTY).
* `-h`, `--help`.

Apart from `--stdout`, the program prints only the per-file summary, status, and
the paths of any files it wrote.

## Examples

```sh
# Dry run, alphabetical everywhere, see the result
rsorder src/lib.rs --stdout \
  --same-level-outside-of-mutual--alphabetically \
  --same-level-inside-of-mutual--alphabetically

# Apply across a tree
rsorder 'src/**/*.rs' --write

# Visualize movement + dependency graphs
rsorder src/lib.rs --write-html-before-after-diff-table \
  --mermaid-write-before --mermaid-write-after
```

## Ordering rules

* `use` / `extern crate` (and `extern { }`) are pinned at the top in whole-file
  mode; in scoped mode nothing outside a region is moved at all.
* A self-recursive function is **not** a mutual block; only true cycles between
  two or more items are.
* `macro_rules!` definitions are dependencies of their call sites, so a macro is
  ordered before its invocations (which Rust requires).
* Independent items keep original relative order unless the relevant
  `--...--alphabetically` flag (or region override) is set.

## Preservation guarantees

* Item bytes (attributes, doc comments, bodies, non-ASCII) are sliced verbatim
  and never rewritten.
* The license header / inner attributes above the first item are kept verbatim.
* A comment directly above an item travels with it.
* The tool strips its own `// mutual start/end` markers on input, so it is
  **idempotent**; `// TO REORDER` region markers are preserved.

### Known limitations

* Only top-level items are reordered (`mod { … }` bodies are opaque).
* A stand-alone comment placed immediately above a `// TO REORDER` marker may
  attach to the region's first item.
* Blank runs between items are normalized to a single blank line.
* Dependency detection is file-local and textual (a safe over-approximation).

## Layout

| file             | purpose                                                  |
|------------------|----------------------------------------------------------|
| `src/lib.rs`     | orchestration, plan, emit (whole-file + scoped)          |
| `src/analyze.rs` | parse, byte-span slicing, comments, regions, dep edges   |
| `src/order.rs`   | Tarjan SCC + configurable stable topological order       |
| `src/model.rs`   | item model + classification                              |
| `src/render.rs`  | Mermaid before/after + HTML movement diagram             |
| `src/main.rs`    | async CLI, color, file walking, openers, summary         |
| `tests/e2e.rs`   | golden + idempotence + behavior tests                    |
