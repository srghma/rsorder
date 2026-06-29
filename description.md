write a rust project that will take path to rust file/s (glob)

for each file it will parse type, structures, consts, functions

it will reorder them to not allow e.g. to use function/structure before it is defined

if it is not possible before of mutual recursion - it will wrap them into // mutual start // mutual end comments (similar to lean) to signify mutuality

inside of mutual - it will reorder alphabetically (if --same-level-alphabetically) or preserve original order if nothing

it will also print summary in the end

if --mermaid-before/end - will stdout graph of dependencies before and after

if --mermaid-diff then it will print 2 column table , on left - before on right after

something like

a | b
b | c
c | a

and the arrows btw 2 col show what moved where


allow to put special comments

// TO REORDER


// TO REORDER END

if have at least one such - then cli will reorder declarations only inside (output that this mode was selected bc ....)

1. instead of --same-level-alphabetically  we need to have --same-level-outside-of-mutual--alphabetically and --same-level-inside-of-mutual--alphabetically (if is not here should use "original" ordering mode)

the

// TO REORDER


// TO REORDER END

should support their own local orderings too that will override global using e.g.

// TO REORDER same-level-inside-of-mutual--alphabetically same-level-outside-of-mutual--original
// TO REORDER END


1. You print this table

before/after order:
  BEFORE                                     AFTER
  ---------------------------------------------------------------------------------

dont print

instead remove --mermaid-diff too and replace with --write-html-before-after-diff-table

will write file /tmp/$current-session-tmp-dir/$rustfilefilenamepath-before-after.html

where it will render table and arrows btw rows what moved where


also it will open file in browser automatically using xdg-open

1. remove --mermaid-before/after
replacewith --mermaid-write-before/after


will write  /tmp/$current-session-tmp-dir/$rustfilefilenamepath-before/after.mermaid

if in path have mmdr

then it will render this mermaids into svg like


```
mmdr -i diagram.mmd -o output.svg -e svg


and open using xdg-open automatically


5. dont stdout anything except of summary and etc

use colorization if output is tty or there is no --no-color option

6. fix

 ~/projects/rsorder   main ±✚  cargo build
warning: field `pinned` is never read
  --> src/model.rs:61:9
   |
48 | pub struct Item {
   |            ---- field in this struct
...
61 |     pub pinned: bool,
   |         ^^^^^^
   |
   = note: `Item` has derived impls for the traits `Clone` and `Debug`, but these are intentionally ignored during dead code analysis
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: `rsorder` (bin "rsorder") generated 1 warning
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.17s


7. use functional style, tokio etc, refactor

8. add e2e golden tests

---------------


1. disallow calling rsorder program without command (should be no default)

2. when I do

~/projects/lean4  ⇅ rust-rewrite ✚  rsorder check src/rust/runtime/src/**/*.rs
=== src/rust/runtime/src/leanh_extra.rs ===
dry run — pass --write to apply
summary:
   items:            168 (const=14, fn=147, macro-call=1, static=1, struct=3, type=2, use=6)
   dependency edges: 188
   items moved:      22 / 168
   mutual groups:    0

=== src/rust/runtime/src/lib.rs ===
dry run — pass --write to apply
summary:
   items:            3 (mod=3, use=1)
   dependency edges: 0
   items moved:      0 / 3
   mutual groups:    0

totals
   files:         2
   changed:       2
   items ordered: 171
   items moved:   22
   mutual groups: 0


it should not show items moved

it should show list of declarations which are used before being declared

how? You parse

decl1 body
decl2 body

then make a graph of dependencies. then You find out that e.g. decl2 is used in body of decl1 -> violation. notice that no reordering is used (alphabetic, original, topologicla) . if at least one violation then return code 1.

3.

         --same-level-inside-of-mutual--alphabetically
         --same-level-outside-of-mutual--alphabetically

         change to

         --sorting-non-mutual={ alphabetical, topological, original }
         --sorting-inside-mutual={ alphabetical, original }

same if declared inside of comments block . original is default

4. add tests for ordering

use golden tests, test all 3 orderings

also during testing should golden test the json of tree of dependencies . it should be Tree DeclId , but inside of this tree some nodes can be a mutual block, which is just Array DeclId

5. the input of golden tests . e.g. const WHOLE_BASIC: &str = r#"use std::collections::HashMap; should be read from file too
