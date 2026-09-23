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

---

# Outstanding items

Filed by the Sprint 7 gate analysis of 2026-09-23 (round 4, scoped to
every `src/` file changed since 0.10.0, about 400 files). The bugs the
same pass found are filed separately in `bugs.md` (B326 to B332). Items
are grouped so that each one touches a distinct set of files. 1 and 2
are prerequisites for P64 and should go first.

## 1. Split `forward_walk/control_flow.rs` along its existing seams

`src/type_engine/variable/forward_walk/control_flow.rs` is 2,398 lines,
and P64 rewrites its branch handling. Two blocks in it are not control
flow at all:

- The `foreach` machinery, `process_foreach` through `bind_foreach_key`
  (about lines 1122 to 1819: `LoopSeedPoint`, `process_foreach`,
  `resolve_foreach_iterable_type`, `resolve_foreach_expr_via_subject`,
  `bind_foreach_value`, `is_non_empty_array_literal`,
  `extract_foreach_var_name`, `extract_foreach_destr_key`,
  `bind_foreach_key`). `forward_walk/foreach.rs` already calls itself
  "the `foreach` machinery" and holds the rest of it. Move these there.
- The assignment-dependency analysis that sizes the loop fixed point
  (about lines 706 to 1121: `assignment_map_depth`,
  `assignment_map_depth_with_updates`, `chain_depth`,
  `collect_assignment_deps`, `collect_expr_assignment_deps`,
  `collect_assignment_target_vars`, `collect_lhs_index_variables`,
  `destructuring_element_exprs`, `collect_rhs_variables`,
  `collect_arglist_variables`, `scope_has_changes`). It is pure AST
  analysis. Move it to a new `forward_walk/assignment_deps.rs`.

What stays is `if`/`while`/`for`/`do`/`try`/`switch`, around 1,300 lines.

## 2. Split `scope_state/merge.rs` before P64 changes the join

`src/type_engine/variable/forward_walk/scope_state/merge.rs` is 882
lines. P64 changes `merge_branch` so it joins only the entries a branch
wrote.

- Move the proof joins (`join_ruled_out`, `join_non_null_implications`,
  `join_implied_narrowings`, `same_implied_narrowings`,
  `implication_holds`, `is_definitely_null`, `is_definitely_non_null`,
  `same_trigger`, about lines 342 to 722) into `scope_state/proofs.rs`.
- Move `simplify_class_hierarchy_unions` and `is_subclass_of` (about
  723 to 882, called from `control_flow.rs`, `null_identity.rs`,
  `resolver/mod.rs` and `blade/backing_class.rs`) out of the merge file.
  `is_subclass_of` is a one-line wrapper over
  `class_lookup::is_subtype_of`, so its callers can call that directly.
- Pull the body of the `for (name, other_types) in &other.locals` loop in
  `merge_branch` (lines 109 to 261) into its own function that joins one
  key. P64 then calls it for the keys a branch touched, instead of
  rewriting a 150-line loop body in place.

## 3. Move the array-shape write helpers out of `variable/resolution.rs`

`src/type_engine/variable/resolution.rs` (2,784 lines) has a "Shape
mutation helpers" section (lines 1753 to 2454: `merge_nested_array_write`,
`merge_push_type`, `merge_keyed_type`, `merge_array_plus`,
`normalize_array_key_type`, …) that is pure `PhpType` shape work and
uses nothing else in the file. Its callers are
`forward_walk/array_assignment.rs`, `forward_walk/assignment.rs`,
`raw_type_inference.rs`, `array_func_rules.rs` and
`rhs_resolution/arithmetic.rs`. Move it to
`src/type_engine/variable/array_shape_writes.rs` and update those paths.

## 4. One class-exclusion helper in `cond_narrowing`

`src/type_engine/variable/forward_walk/cond_narrowing/apply.rs` repeats
one block four times in `apply_condition_narrowing_inverse_single` (about
lines 281, 316, 365 and 422). Each copy excludes classes from a
variable, calls `record_exclusion`, and marks the scope unreachable if
that emptied a variable that had types. `exclude_classes_in_scope` in
`cond_narrowing/instanceof.rs:10` does the same thing, minus the
unreachable marking. Give the helper an "emptied a typed variable" result,
make it `pub(super)`, and use it at all five sites. Decide explicitly
whether the `&&`-chain caller in `instanceof.rs` should mark
unreachable too, rather than leaving the two to drift.

## 5. Find the inherited constructor through `inheritance::ancestors`

`src/type_engine/call_resolution/return_types/call_arms.rs:838-870` and
`src/type_engine/variable/rhs_resolution/instantiation.rs:129-163` each
hand-roll a `for _ in 0..15` parent loop to find the nearest
`__construct`, cloning the parent name into a `String` at every step.
Replace both with
`inheritance::ancestors(cls, loader).find(|(_, p)| p.get_method("__construct").is_some())`.
(The `0..15` loop at `instantiation.rs:523` carries per-level
`@extends` substitutions and is not a candidate.)

## 6. Stop `find_declaring_interface` visiting a parent twice per level

`src/inheritance/ancestry.rs:123-143` recurses into every entry of
`interfaces` and then into `parent_class`. For an interface the parser
stores the first `extends` in both, so a failed search down a
single-extends chain of depth d loads 2^d classes (bounded only by
`MAX_INHERITANCE_DEPTH`). A miss is the normal case on the code-lens
path (`code_lens.rs:557`), and the function also serves
`definition/member/declaring.rs:68` and `semantic_export/calls.rs:208`.
Skip `parent_class` when it is already in `interfaces`, or track visited
names as `collect_supertypes` does.

## 7. One member lookup for the member diagnostics

`declared_member` in `src/diagnostics/member_visibility.rs:535-573`,
and `member_exists` (1018-1063) and `member_is_public` (967-1008) in
`src/diagnostics/unknown_members/mod.rs`, each implement the same lookup:
a case-insensitive method match, then constant before static property
(with the `$` spelling check), then instance property. `display_class_name`
is duplicated verbatim (`unknown_members/mod.rs:1110`,
`member_visibility.rs:668`). Move `declared_member`, `display_class_name`
and the member helpers after line 942 of `unknown_members/mod.rs` into a new
`src/diagnostics/member_lookup.rs`, and express `member_exists` and
`member_is_public` over `declared_member`. That also drops the
per-method `to_ascii_lowercase()` and per-property `format!` allocations
on this hot diagnostic path.

## 8. One candidate-file loop for Find References

The per-kind searches in `src/references/` repeat one skeleton: take the
candidate snapshot, open a progress window, then for each file
`Url::parse`, build a `CandidateFile`, pre-check spans, scan them, push
locations, and finally sort. The copies are `classes.rs:65`
(`find_class_references`), `classes.rs:184` (`find_constructor_references`),
`functions.rs:16` (`find_function_references`), `functions.rs:128`
(`find_constant_references`), `members.rs:125`
(`find_laravel_macro_references`) and `members.rs:943`
(`find_member_references`). They have already drifted:
`find_constant_references` never opens a scan window, so its progress
never advances. `functions.rs:35-55` also re-implements
`SpanFqnResolver::fqn` from `classes.rs:41-57`. Add one helper in
`references/mod.rs` that owns the loop and progress and takes a per-file
closure, and move all six onto it. The lens batch path in
`members.rs:474-628` already scans with `parallel::map_indexed`. Decide
whether the helper does the same, and if so the other searches gain it
for free.

## 9. `analyze` walks files through `workspace_walk_builder`

`collect_php_files` in `src/analyse/run.rs:245-290` rebuilds the
`WalkBuilder` that `classmap_scanner::workspace_walk_builder`
(`classmap_scanner/filters.rs:40`) provides, and has drifted from it. It
prunes vendor by calling `canonicalize()` on every directory, and it
never calls `claims.cover(skip_dirs)`, so a symlink into vendor is walked
by `analyze` but not by the indexer. Build the walk from
`workspace_walk_builder` and keep only the `crop` filter local.

## 10. Share the Laravel string-key caches instead of cloning them

- `cached_config_trees` (`src/virtual_members/laravel/config_values.rs:487`)
  returns the whole `Vec<(String, ConfigNode)>` by value, and
  `resolve_config_type` is wired into the shared loaders
  (`resolution.rs:1656`). So every `config('x')` resolved anywhere
  deep-clones every config tree, which now includes the framework
  defaults. Store `config_trees` as an `Arc` (`src/lib.rs:392`,
  `config_values.rs`, `storage.rs:162`), as `trans_key_shapes`, `routes`
  and `blade_discovery` already are.
- `config_keys`, `view_names` and `trans_keys` are cloned whole on each
  read, and `src/diagnostics/laravel_string_keys.rs:285-291` copies them
  into a fresh `HashSet<String>` for every file's diagnostic pass. Store
  them shared (`Arc<HashSet<String>>` or similar).

## 11. Split `rename/namespace.rs` the way `rename/class/` is split

`src/rename/namespace.rs` grew from 602 to 807 lines and does two jobs:
the text-edit plan (lines 26 to 460) and the PSR-4 directory-move plan
(`namespace_merge_conflict`, `build_namespace_psr4_rename_ops`,
`namespace_psr4_root_conflict`, `namespace_source_dirs`,
`collect_merge_move_ops`, from line 462 on). Class rename keeps its
equivalent in `rename/class/layout.rs`. Make it
`rename/namespace/{mod,layout}.rs`.

## 12. Split `stub_patches.rs` into function and class patches

`src/stub_patches.rs` grew from 1,661 to 2,118 lines, and has two
independent halves: function patches (about 198 to 1000) and class
patches (about 1020 to 1634), followed by about 480 lines of inline tests.
Make it `stub_patches/{mod,functions,classes}.rs`, and move the tests to a
`#[path]` test file as the other large modules do.
