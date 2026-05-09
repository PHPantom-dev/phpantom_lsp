# PHPantom — Refactoring

Technical debt and internal cleanup tasks. This document is the first
item in every sprint. The sprint cannot begin feature work until this
gate is clear.

> **Housekeeping:** When a task is completed, remove it from this
> document entirely. Do not strike through or mark as done.

## Sprint-opening gate process

Every sprint lists "Clear refactoring gate" as its first item,
linking here. When an agent starts a sprint, follow these steps
**in order**. No step may be skipped.

### Step 1. Resolve outstanding items

Read this document top to bottom. If there are any tasks listed in the
"Outstanding items" section at the bottom, complete every one of them.
Remove each task from this document as it is completed. After all tasks
are resolved, go to step 2.

If the "Outstanding items" section says "No outstanding items", go
directly to step 3.

### Step 2. Request a fresh session

After completing refactoring work, **stop and ask the user to start a
new session**. The analysis in step 3 must happen in a session where
no refactoring edits have been made. This prevents the analyst from
rubber-stamping work it just performed. Do not proceed to step 3 in
the same session where you completed step 1.

### Step 3. Analyze the codebase

This step produces a written analysis report. The report must be shown
to the user before any decision is made about the gate.

**Prerequisite:** You must be in a session where no refactoring edits
have been made (either a fresh session, or one where step 1 had no
work to do).

Run through **every section** of the analysis checklist below. For
each section, **actually read the relevant source files** using tools.
Do not rely on memory, summaries, or prior context. Open the files,
look at the code, and report what you find.

**Required output format.** For each checklist section, write:

1. **Which files you read** (list them by path).
2. **What you found** (specific observations with line numbers).
3. **Verdict: PASS or FAIL** with justification.

A section FAILs if it identifies work that should be done before the
sprint's feature tasks begin. A section PASSes only if you can point
to specific evidence (file sizes, grep results, code you read) that
confirms there is no problem.

"I didn't find anything" is not a PASS. "I read X, Y, and Z, checked
for A and B, and found no instances because [concrete reason]" is a
PASS.

After completing the full checklist:

- If **any section FAILed**: add concrete, actionable tasks to the
  "Outstanding items" section of this document. Each task must name
  the file(s) to change and describe what to do. Then go to step 1.
- If **all sections PASSed**: go to step 4.

### Step 4. Declare the gate clear

Remove the "Clear refactoring gate" row from the current sprint's
table in `docs/todo.md`. The sprint is now open for feature work.

This step may only be reached after step 3 produces an all-PASS
report. There is no shortcut.

---

## Analysis checklist

The checklist is scoped to the **current sprint's tasks**. Before
starting, read the sprint table in `docs/todo.md` and the linked
domain documents to understand which modules will be touched.

### 1. File size and module boundaries

- Identify the source files most likely to be touched by this
  sprint's tasks. Read each one. Report its line count.
- Any file over ~600 lines is a candidate for splitting. Look for
  natural seams: logically distinct groups of functions, multiple
  unrelated `impl` blocks, or a section that is already commented
  as a separate concern.
- Check whether any module is doing two jobs (e.g. parsing _and_
  resolution, or building _and_ formatting). If the sprint will add
  a third job to the same file, that file must be split now.
- Look for `mod.rs` files that have grown beyond a thin re-export
  layer. Logic that lives in `mod.rs` is harder to find and test.

**FAIL criteria:** A file that will be heavily modified during the
sprint exceeds 600 lines, or a module mixes unrelated concerns that
the sprint will make worse.

### 2. Test placement

- Check whether any `#[cfg(test)]` blocks exist inside `src/` files
  for the modules this sprint will touch. Inline tests are fine for
  pure unit tests on private helpers, but integration tests and
  anything that touches the `Backend` or multi-file resolution should
  live in `tests/`.
- Check whether the existing `tests/` files cover the modules the
  sprint will modify. List what coverage exists and what is missing.
- Look for test helper code duplicated across multiple test files.
  If the same fixture setup or assertion pattern appears more than
  twice, it belongs in `tests/common/mod.rs`.

**FAIL criteria:** Integration-level tests live in `src/`, or the
sprint will modify modules that have no test coverage at all, or the
same test helper is copy-pasted in three or more files.

### 3. Code duplication

- Grep for structurally similar functions across the modules the
  sprint will touch. Report what you searched for and what you found.
- Pay particular attention to: type string manipulation, AST node
  offset extraction, docblock text extraction, and `WorkspaceEdit`
  construction. These patterns tend to proliferate.
- If two code action handlers share a non-trivial pattern (e.g. "find
  the token at the cursor, determine its span, build an edit"), check
  whether a shared helper already exists or should be created before
  the sprint adds a third copy.

**FAIL criteria:** Two or more places implement the same non-trivial
logic (>10 lines of structurally similar code), and the sprint will
add another copy or modify one of the existing copies.

### 4. Performance and memory

- Look for any place where the full file AST is re-parsed inside a
  hot path (completion, hover, diagnostics) in the modules the sprint
  will touch. Re-parsing should happen at most once per request.
- Look for unbounded clones of `ClassInfo`, `MethodInfo`, or other
  large structs inside loops. These should be references or
  `Arc`-wrapped.
- Check whether any new data structures added in the previous sprint
  are stored per-file but never evicted. Unbounded growth in
  `DashMap` entries is a memory leak.
- Look for `Vec::contains` or `Vec::iter().find()` used as a set
  membership check on collections that could grow with the number of
  files. These should be `HashSet` or `DashSet`.

**FAIL criteria:** A hot path re-parses when it does not need to,
large structs are cloned in a loop, or a per-file data structure has
no eviction path.

### 5. Fragility and error handling

- Look for `unwrap()` and `expect()` calls in request-handling code
  paths (anything reachable from `server.rs`) in the modules the
  sprint will touch. A panic in a request handler crashes the language
  server. These should be `?` or explicit early returns.
- Check whether the sprint's target modules propagate errors up or
  silently swallow them with `let _ = ...` or empty `Err(_) => {}`
  arms. Silent failures produce confusing user-visible behaviour.
- Look for code that assumes a particular UTF-8 byte offset is a
  valid char boundary without checking. This is a common source of
  panics when files contain multibyte characters.
- Check whether any `Arc<RwLock<...>>` or `Arc<Mutex<...>>` is held
  across an `await` point or across a call that re-acquires the same
  lock. These cause deadlocks or unnecessary blocking.

**FAIL criteria:** `unwrap()`/`expect()` in a request handler, errors
silently swallowed in code the sprint will build on, or a lock held
across an await point.

### 6. Sprint-specific concerns

Read each feature task in the sprint and ask these questions. Answer
each one explicitly in the report:

- Will any task require touching a module that is already large or
  doing too many things? If so, it must be split now.
- Will any task duplicate logic that already exists elsewhere? If so,
  the shared helper must be extracted first.
- Will any task add a new data structure that needs an eviction path?
  The eviction must be planned before writing the feature.
- Will any task generate `WorkspaceEdit` responses? Check that the
  existing edit-building helpers (if any) are adequate, or that a new
  shared helper should be written before the first action is
  implemented.

**FAIL criteria:** Any "yes" answer to the above questions where the
prerequisite work has not already been done.

---

## What belongs here

Only add items that would actively hinder the upcoming sprint's work
or that have accumulated enough friction to justify a focused cleanup
pass. Small fixes that can be done inline during feature work should
just be done inline. Items do not need to be scoped to the sprint's
feature area, but they should be completable in reasonable time (not
multi-week rewrites that would stall the sprint indefinitely).

Each item must include:

- **What to do** (concrete action, not "consider refactoring X").
- **Which files to change** (list specific paths).
- **Why it matters for the sprint** (which task it unblocks or
  de-risks).

---

# Outstanding items

## Intentional overlap (reference, not actionable)

These are parallel systems that exist for valid reasons. Documented here so
they are factored into future design decisions.

### `uri_classes_index` vs `fqn_class_index` vs `fqn_uri_index`

Three maps storing overlapping class data:
- `uri_classes_index`: URI → `Vec<Arc<ClassInfo>>` (needed for "all classes in this file")
- `fqn_class_index`: FQN → `Arc<ClassInfo>` (O(1) FQN lookup)
- `fqn_uri_index`: FQN → URI string (survives `didClose`, enables re-loading)

Data is shared via `Arc`. Each serves a distinct query pattern.

### PathBuf ↔ URI round-tripping

The classmap scanner produces `PathBuf`, converts to URI strings for
`fqn_uri_index`, then at lookup time parses back to `PathBuf` for file reading.
A minor inefficiency, not a parallel system.

### `find_class_in_ast_map` linear scan fallback

`src/util.rs` L1436–1447: After the O(1) `fqn_class_index` lookup fails, falls
back to linear scan of `uri_classes_index`. Covers race conditions during initial
indexing. Legitimate safety net.

---

## Redundant backwards text walkers

These functions in `src/completion/source/helpers.rs` scan backward with `rfind`
to find the most recent assignment to a variable. The forward walker's scope map
already tracks this information during its top-to-bottom pass.

### `extract_closure_return_type_from_assignment`

Uses `rfind("$fn = ")` backward from cursor, then parses closure return type
from raw text. The forward walker already processes closure assignments and
extracts return types from the AST node's type hint. Callers in
`call_resolution.rs` (L1272) and `rhs_resolution.rs` (L2695) should read the
variable's type from the scope map instead.

### `extract_first_class_callable_return_type`

Uses `rfind("$fn = ")` backward, then resolves `Foo::bar(...)` callable return
type from text. The forward walker already handles first-class callable
assignments. Callers in `call_resolution.rs` (L1292) and `rhs_resolution.rs`
(L2718) should use the scope map.

### `extract_function_return_from_source`

Uses `rfind("/**")` backward to get `@return` type for functions not yet in
`global_functions`. This is a fallback for unindexed functions and is harder to
replace, but could be eliminated once all reachable functions are guaranteed to
be indexed before resolution runs.

---

## SymbolMap data re-extracted from AST

The SymbolMap is built once per file in `update_ast_inner` and stores symbol
spans, variable definitions, scope boundaries, and call sites. Several features
re-parse the AST to extract information the SymbolMap already has.

### Variable definition lookup

The SymbolMap has `var_defs` with `find_var_definition()`, but
`definition/variable/mod.rs` ignores it and does its own full AST walk via
`find_variable_definition_in_program()`. The SymbolMap's `var_defs` are used
elsewhere (hover, forward walk) but the go-to-definition feature duplicates
the work.

### Variables-in-scope collection

`collect_variables_in_scope` in `completion.rs` re-parses the AST to find all
variables visible at the cursor. The SymbolMap's `var_defs` already has every
variable definition site with scope boundaries; this could be a filtered query
on `var_defs` instead of a full AST walk.

### Scope boundary detection

The SymbolMap stores `scopes` (function/closure boundaries) and provides
`find_enclosing_scope()`. Yet several code action helpers and diagnostic modules
manually walk the AST to find the enclosing function/method body (e.g.
`build_scope_map` in extract variable, inline variable, undefined variable
diagnostics).
