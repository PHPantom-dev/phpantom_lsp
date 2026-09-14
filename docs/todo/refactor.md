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

Filed by the Sprint 7 gate analysis of 2026-09-14, scoped to the 264
`src/` files the sprint changed rather than to BL1's modules alone.

## R1. Split `preprocess_with_vars` out of the Blade preprocessor's `mod.rs`

**What to do.** `preprocess_with_vars` is a single 1,109-line function
(lines 247-1356) and `mod.rs` is 1,498 lines in a directory that already
holds `component_call.rs`. Lift the per-construct handling out of the
match into sibling modules and leave `mod.rs` as the driver plus its
re-exports: the `@`-directive arm, the `{{ }}`/`{!! !!}`/`@{{ }}` echo
arms, the component-tag arm, and the `<?php`/`@php` island arm each read
as a separate concern already, and the small helpers at the bottom
(`flush_buffer`, `push_comment_text`, `build_use_statement`,
`build_inject_statement`) go with whichever arm owns them.

**Which files to change.** `src/blade/preprocessor/mod.rs`, new siblings
under `src/blade/preprocessor/`.

**Why it matters for the sprint.** This is the sprint's highest-churn
file (3,722 lines changed) and the one BL1 has to reason about when it
decides which code actions make sense in a template and where their
edits land. A function this long cannot be read against the source map
it emits.

## R2. Move the Laravel self-scan subsystem out of `server.rs`

**What to do.** `src/server.rs` is 3,871 lines doing two jobs: the
`LanguageServer` trait dispatch (lines 104-2000) and, from line 2466 to
3,790, a Laravel index-building subsystem (`build_laravel_macro_index`,
`build_laravel_command_index`, `build_laravel_morph_map_index`,
`build_laravel_gate_index`, `build_laravel_date_class`,
`build_provider_resources`, `scan_provider_resources`,
`publish_provider_resources`, `refresh_laravel_*`,
`infer_laravel_macro_return_types`, `infer_storage_driver_return_types`,
`reload_laravel_schema_index`, `update_laravel_migrations` and their
private helpers). Move that block to its own module next to the code it
serves. The generic request plumbing above it (`with_file_content`,
`handle_with_position`, `handle_with_uri`, `coalesced_whole_file`, and
the coalescing tests) belongs with `src/backend/`, not with the Laravel
scan.

**Which files to change.** `src/server.rs`, new
`src/backend/laravel_scan.rs` (or `src/virtual_members/laravel/scan.rs`),
`src/backend/mod.rs`, `src/lib.rs`.

**Why it matters for the sprint.** BL1 edits the `code_action` gate in
this file, and the sprint already put 1,086 lines of churn through it.
Adding Blade gating to a file that also owns Laravel provider scanning
makes the LSP surface harder to find every time.

## R3. Split `cond_narrowing/mod.rs`

**What to do.** The directory already has six sibling modules
(`assertions.rs`, `emptiness.rs`, `instanceof.rs`, `key_types.rs`,
`null_identity.rs`, `property_checks.rs`) and `mod.rs` still holds 4,120
lines. Split along the seams the file already has: the `extract_*`
condition predicates (lines 2636-3100), the `preg_match` group
(`apply_preg_match_narrowing`, `record_preg_outcome`,
`preg_outcome_alias`), the `in_array` group
(`apply_in_array_narrowing`, `narrow_value_by_element`,
`resolve_in_array_element_type_fw`), and
`apply_phpstan_assert_condition_narrowing` with its helpers. `mod.rs`
keeps `apply_condition_narrowing`, its inverse, and the re-exports.

**Which files to change.**
`src/type_engine/variable/forward_walk/cond_narrowing/mod.rs`, new
siblings in the same directory.

**Why it matters for the sprint.** Second-highest churn of the sprint
(2,274 lines changed) and the shared narrowing path every feature reads
through. It is the file a future correctness fix lands in, and it is
currently the one place in the tree where a `mod.rs` outweighs all its
siblings combined.

## R4. Break up `analyse::run::run`

**What to do.** `run()` is one 505-line function (lines 31-536) that
parses options, discovers the project, builds the index, spawns the
diagnostic workers, collects results, and reports. `fix::run`
(`src/fix.rs`, 223 lines) now shares its parse phase, so the shared
stages should be named functions both call rather than a block one of
them owns.

**Which files to change.** `src/analyse/run.rs`, `src/fix.rs`.

**Why it matters for the sprint.** `analyze` and `fix` grew a shared
parse phase this sprint and `format` was added beside them; the next
change to any of the three has to be read against a function that does
six things.

## R5. Give the integration suite one `complete_at` helper

**What to do.** 37 files under `tests/integration/` each define their
own `async fn complete_at`, in three signature variants
(`&Backend` vs `&phpantom_lsp::Backend`, with and without a `text`
argument, one taking a `Position`). `tests/integration/common/mod.rs`
has no completion helper at all. Add one there and delete the copies.

**Which files to change.** `tests/integration/common/mod.rs` and the 37
`tests/integration/*.rs` files that define the helper.

**Why it matters for the sprint.** Eight of those files were edited this
sprint, and a completion test for a Blade template has to be written
against whichever variant its file happens to carry. This is a
project-wide rewrite: run it as the sole active agent or in batches of
non-overlapping files.

## R6. Move the rename suite to `tests/integration/`

**What to do.** `src/rename/tests.rs` is 6,363 lines, the largest file
in the tree, and every test in it drives a real `Backend` through
`did_open` + `prepare_rename`/`rename` (148 `Backend` references).
`tests/integration/` has no `rename*.rs` at all. Move it out, splitting
by subject the way the code-action tests are split (class, namespace,
member, variable, Laravel). The private API it reaches for is small:
`crate::composer::Psr`, `crate::test_fixtures::apply_edits`,
`extract_macro_registrations`, `backend.laravel_macros`, `backend.psr`,
`parse_and_cache_content`, `update_ast`, `supports_file_rename` -
`tests/integration/common/mod.rs` already exposes equivalents for most
of these, and `laravel_references.rs` already seeds the macro index from
outside the crate.

`src/references/tests.rs` (3,028 lines) stays where it is: it reaches
into `indexing::preload`, `progress::ScanProgress`,
`reference_index::ReferenceIndexKey`, and `workspace_index_lock`, which
are white-box tests of the indexing machinery rather than LSP-level
tests, and moving them would mean making that machinery public.

**Which files to change.** `src/rename/tests.rs` (delete), new
`tests/integration/rename_*.rs`, `src/rename/mod.rs`,
`tests/integration/common/mod.rs`.

**Why it matters for the sprint.** The 2026-09-12 pass moved
`import_class_tests.rs` out for exactly this reason; this is the same
problem an order of magnitude larger, in a module the sprint changed
(1,432 lines of churn) and that B51 is about to change again.

## R7. Consolidate the `use`-statement scanners

**What to do.** Four scanners read a file's `use` statements and they no
longer agree: `compute_use_line_ranges` (`src/diagnostics/helpers.rs`)
and `find_use_statement_range` (`src/diagnostics/unused_imports.rs`)
read a `use` as a whole statement and understand group imports;
`find_use_line_range` (`src/rename/class.rs`) is line-based and
understands neither; `analyze_use_block`
(`src/completion/use_edit.rs`) reads the block to decide where a new
import goes. Fold the rename path onto the shared statement scanner and
keep `analyze_use_block` as the one insertion-point reader, documenting
at each what it answers.

**Which files to change.** `src/rename/class.rs`,
`src/diagnostics/helpers.rs`, `src/diagnostics/unused_imports.rs`,
`src/completion/use_edit.rs`.

**Why it matters for the sprint.** The divergence is already a
user-visible bug: B51 (`docs/todo/bugs.md`), where renaming a class
imported through `use App\Models\{User, Post};` leaves the import
behind. The sprint taught one copy about group and multi-line imports
and left the other on line matching. Delete the B51 entry when this
lands.

## R8. Stop deep-cloning a template's virtual PHP on every request

**What to do.** `blade_virtual_content` is
`Arc<RwLock<HashMap<String, String>>>` and `blade_virtual_php`
(`src/lib.rs`) hands out `.cloned()`, so `analysable_content`,
`analysable_content_at`, and `analysable_content_or`
(`src/backend/file_access.rs`) copy the whole virtual PHP string on
every completion, hover, go-to-definition, code-action, and diagnostic
request against a template. PHP files have `get_file_content_arc` for
exactly this; templates have no equivalent. Store `Arc<String>` in the
map and add the `Arc` accessors beside the cloning ones.

**Which files to change.** `src/lib.rs`, `src/backend/file_access.rs`,
`src/parser/ast_update.rs` (`record_blade_virtual_php`),
`src/server.rs`, `src/completion/handler/mod.rs`,
`src/code_actions/mod.rs`, `src/diagnostics/mod.rs`, `src/fix.rs`.

**Why it matters for the sprint.** BL1 adds `code_actions/mod.rs` to the
list of per-keystroke consumers, on files (Blade templates) that are
already the slowest path in the server.
