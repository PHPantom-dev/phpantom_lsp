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
feature area.    

Each item must include:

- **What to do** (concrete action, not "consider refactoring X").
- **Which files to change** (list specific paths).

---

# Outstanding items

## Blocks a Sprint 9 task

### Split `src/reference_index.rs` by concern

P91 rewrites how a file's reference entries are built, moving the work
from the merging thread into the parse workers. The file is 1,185 lines
of production code (plus 620 lines of inline tests) doing three jobs:
the index itself (`ReferenceIndexKey`, `ReferenceIndexInner`, the
candidate and count queries, `reindex_references_for_symbol_maps_batch`
and eviction), the per-file entry builder
(`reference_entries_for_symbol_map`, `reference_keys_for_span`,
`names_global_fallback`, `resolved_name_at`, the `*_fqn_at` helpers, and
the free `class_keys`/`function_keys`/`constant_keys`/`symbol_name_keys`),
and the receiver-resolution cache (`ResolvedMemberAccess`,
`ResolvedMemberFile`, and the `resolved_member_file`,
`cache_resolved_member_file`, `clear_resolved_member_files`,
`retain_resolved_member_files` methods).

Turn it into a directory module: `mod.rs` keeps the index and its
queries, `entries.rs` takes the entry builder, `resolved_members.rs`
takes the receiver-resolution cache, and the inline tests move to a
sibling `tests.rs` behind `#[cfg(test)] #[path]`, the way
`classmap_scanner/discovery_tests.rs` is laid out. Move code only;
change no behaviour.

Files: `src/reference_index.rs` → `src/reference_index/{mod.rs,
entries.rs, resolved_members.rs, tests.rs}`.

### Move batch parsing out of `src/indexing/preload.rs`

P91 rewrites `parse_batch_parallel` and `parse_and_publish` (overlapping
a chunk's parse with the previous chunk's merge), and P92 rewrites the
file discovery in `ensure_workspace_indexed_with_progress_mode`. The
file is 755 lines and holds both the workspace-index orchestration and
the batch-parsing engine, and that one function is 280 lines.

Move the batch parser (`parse_files_parallel_with_progress`,
`parse_paths_parallel_with_progress`, `parse_paths_parallel_awake`,
`parse_batch_parallel`, `parse_and_publish`, `publish_parsed`,
`DORMANT_CHUNK`, `ParseCounter`, `largest_first_work_order`, and the
`report_weighted_*` progress helpers) into `src/indexing/parse_batch.rs`.
Then extract Phase 1 and Phase 2 discovery (from the `existing_uris`
set through building `phase2_work` and `resource_work`) out of
`ensure_workspace_indexed_with_progress_mode` into a function that
returns the three work lists, so P92 changes discovery without touching
the parse and publish sequence. Move code only; change no behaviour.

Files: `src/indexing/preload.rs`, `src/indexing/parse_batch.rs` (new),
`src/indexing/mod.rs`.

### Extract the parallel walk from `walk_roots`

P92 moves `visit_workspace_files_gitignore` and `scan_drupal_directories`
onto a parallel walk. `walk_roots` in `src/classmap_scanner/walk.rs` is
the only one today, and its parallel machinery (`build_parallel` over
`available_cores()`, a channel per worker, collecting, sorting for a
deterministic order) is tied to PSR-4 root attribution and the PHP-file
filter. Moving the other two walks onto it as written would add two more
copies of that machinery.

Extract a helper that runs a `WalkBuilder` in parallel, hands each
entry to a caller-supplied classifier (`Fn(&ignore::DirEntry) ->
Option<T>`), and returns the results sorted. Make `walk_roots` its first
caller. The classifier receives the entry, so it can use
`entry.file_type()` rather than stat the path again. Change no
behaviour.

Files: `src/classmap_scanner/walk.rs`.

## General

### Gate Laravel's type-confirmed sites at extraction

`Frameworks` (`src/symbol_map/extraction/mod.rs`) has a flag for
Symfony, the service container, Drupal, Twig, Doctrine, Yii and Tempest,
and their extraction records a site for the type engine to confirm only
in a project that uses the framework. It has no Laravel flag, so the
Laravel sites are recorded in every project, and each consumer decides
for itself whether to look at them:

- `record_view_receiver_sites` (`laravel/method_calls.rs`) records a
  view-factory or mailable site for any `->make('x')`, `->first('x')`,
  `->exists('x')`, `->view('x')` and the like, in any project.
  `typed_receiver_spans_for` (`src/blade/typed_receiver.rs`) resolves
  each such receiver's type the first time the file is read, with no
  Laravel check. In a Symfony or Drupal project that is a receiver
  resolution per container `make()` or repository `first()` call, which
  confirms nothing.
- Morph column sites (`src/symbol_map/morph_columns.rs`) are recorded
  for every `$x->prop === 'literal'`. `morph_column_spans_for`
  (`src/virtual_members/laravel/typed_morph_columns.rs`) skips them
  outside Laravel at confirmation, and
  `reference_entries_for_symbol_map` (`src/reference_index.rs`) skips
  them again with its own `is_laravel` check.
- Any new Laravel site added the same way (connection names from
  `->connection('x')`, `->onConnection('x')` and `$connection`) is
  another copy of the pattern.

Add `laravel` to `Frameworks`, set from
`resolved_class_cache.laravel().is_enabled()` where the index builds it
(`src/parser/ast_update.rs`) and `true` in `Frameworks::ALL`. Gate the
recording of view-receiver and morph column sites on it. Then remove the
checks this makes redundant: the Laravel test in
`morph_column_spans_for` and the `is_laravel` filtering of morph sites in
`reference_entries_for_symbol_map`. Before you remove them, check that a
project which turns into a Laravel project (a `composer.json` change)
re-extracts its files the way a change to the Symfony flags does. If it
does not, the checks at confirmation are what keeps that case working,
and they stay. Set the flag at the other places that build `Frameworks`
(`src/symfony/route_world.rs`, `src/symfony/bundles.rs`,
`src/twig/callables.rs`, `src/yii/definitions.rs`,
`src/wordpress/hook_sources.rs`, `src/diagnostics/dql_syntax.rs`) to
what each of them needs.

Files: `src/symbol_map/extraction/mod.rs`,
`src/symbol_map/extraction/laravel/method_calls.rs`,
`src/symbol_map/morph_columns.rs`, `src/parser/ast_update.rs`,
`src/virtual_members/laravel/typed_morph_columns.rs`,
`src/reference_index.rs`, and the `Frameworks` builders listed above.
