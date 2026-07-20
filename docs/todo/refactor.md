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

## Redundant backwards text walkers

These functions in `src/completion/source/helpers.rs` scan backward with `rfind`
to find the most recent assignment to a variable. The forward walker's scope map
already tracks this information during its top-to-bottom pass.

### `extract_closure_return_type_from_assignment`

Uses `rfind("$fn = ")` backward from cursor, then parses closure return type
from raw text. The forward walker does NOT currently store callable return type
info — it stores the variable as plain `Closure` via `resolve_rhs_expression`.
Eliminating this backward walker requires teaching `resolve_rhs_expression` (or
`resolve_rhs_with_scope`) to produce a `Callable(params, return_type)` PhpType
for closure/arrow-function expressions.

### `extract_first_class_callable_return_type`

Uses `rfind("$fn = ")` backward, then resolves `Foo::bar(...)` callable return
type from text. Same situation: the forward walker stores plain `Closure` for
first-class callable assignments. Needs the same `Callable` type support.

### `extract_function_return_from_source`

Uses `rfind("/**")` backward to get `@return` type for functions not yet in
`global_functions`. This is a fallback for unindexed functions and is harder to
replace, but could be eliminated once all reachable functions are guaranteed to
be indexed before resolution runs.

---

## Move test blocks out of src/ files

**What to do.** ~18,000 lines of tests currently live inside `src/`
files, inflating them far past the 600-line threshold and drowning the
production code they sit next to. Two distinct moves:

1. **Backend-driven suites → `tests/integration/`.** These build a full
   `Backend` (`new_test_with_stubs`, `update_ast`) and are integration
   tests by the project's own definition. `deprecated.rs` and
   `type_errors.rs` already follow the correct convention (zero inline
   tests, suites in `tests/integration/diagnostics_*.rs`) — extend it to:
   - `src/diagnostics/unknown_members/tests.rs` (5,294 lines, 217
     Backend uses). A parallel suite already exists at
     `tests/integration/diagnostics_unknown_members.rs` (5,033 lines) —
     consolidate, checking for overlapping coverage while porting.
   - `src/diagnostics/undefined_variables.rs` (~1,563 test lines;
     `tests/integration/diagnostics_undefined_variables.rs` also
     already exists — same consolidation).
   - Inline blocks in `unknown_classes.rs` (~1,009), `argument_count.rs`
     (~962), `unused_variables.rs` (~792), `invalid_class_kind.rs`
     (~563), `implementation_errors.rs` (~493), `unknown_functions.rs`
     (~446), `syntax_errors.rs` (~143).
2. **Pure unit tests → sibling `_tests.rs` files via `#[path]`** (the
   convention already used by `inheritance_tests.rs`,
   `subject_expr_tests.rs`, `signature_help_tests.rs`,
   `resolution_tests.rs`, `class_completion_tests.rs`):
   - `src/php_type.rs` — 3,215 test lines (lines ~4522-7736), the single
     largest violation. Moving them halves the file.
   - `src/types.rs` (~1,017), `src/selection_range.rs` (~2,248, 72% of
     the file), `src/completion/phpdoc/generation.rs` (~1,200),
     `src/classmap_scanner.rs` (~1,000), `src/code_actions/`:
     `extract_function.rs` (~2,432), `extract_constant.rs` (~1,413),
     `import_class.rs` (~1,193), `extract_variable.rs` (~1,164),
     `update_docblock.rs` (~1,155), `phpstan/fix_return_type.rs` (~828).

**Why it matters.** The file-size and test-placement checklist sections
fail on these files every gate pass. The moves are mechanical and
zero-risk, and for `extract_constant.rs`, `extract_variable.rs`, and
`import_class.rs` they alone bring production code back under ~900
lines.

---

## Split `forward_walk.rs` (9,476 lines) along its section banners

**What to do.** `src/completion/variable/forward_walk.rs` is the largest
file in the crate and does at least seven jobs: hover/diagnostic scope
caches, whole-file diagnostic walking, callable parameter inference, the
core `ScopeState` data structure, the assignment engine, control-flow
processing with loop clamping, and a ~1,700-line narrowing subsystem.
The file already carries section banners that map onto submodules.
Convert it to a `forward_walk/` directory:

- `diagnostic_cache.rs` — `HOVER_SCOPE_CACHE`, `DIAGNOSTIC_SCOPE`,
  `BUILDING_SCOPES` thread-locals, RAII guards, snapshot recording.
- `diagnostic_walk.rs` — `walk_body_for_diagnostics`,
  `walk_closures_in_*`, `walk_top_level_statements`,
  `analyze_function_body`.
- `callable_inference.rs` — the `infer_callable_params_from_*_fw`
  family, `find_callable_params_on_*_fw`, `seed_this`.
- `scope_state.rs` — `ScopeState`, `ForwardWalkCtx`, `merge_branch`,
  `simplify_class_hierarchy_unions`, `is_subclass_of`.
- `assignment.rs` — `process_assignment_expr`,
  `process_compound_assignment`, `process_destructuring_assignment`,
  `process_array_key_assignment`, `resolve_rhs_with_scope`, inline
  `@var` docblock handling, superglobal/pass-by-ref seeding. (Note: the
  banner over this region says "ternary/match(true) narrowing" but the
  code is the assignment engine — fix the banner while splitting.)
- `snapshot_narrowing.rs` — `&&`/`||` chain operands, match/ternary
  snapshot recording.
- `control_flow.rs` — `process_if`/`process_foreach`/`process_while`/
  `process_for`/`process_do_while`/`process_try`/`process_switch` plus
  the assignment-dependency-depth helpers.
- `narrowing.rs` — the ~24 `apply_*_narrowing` / `strip_*_from_scope`
  functions and isset/empty extractors.
- `closures.rs` — `try_enter_closure`, `try_enter_closure_expr`,
  `widen_literal`.
- `loop_control.rs` — `LOOP_DEPTH`, `enter_loop`/`leave_loop`,
  `clamp_iterations_for_depth`.

All four thread-locals are referenced only within this file, so they
migrate cleanly with their sections. This is a mechanical move-only
split — no behaviour change.

**Why it matters.** Every type-inference task touches this file, and at
9,476 lines with mislabeled sections it is the highest-friction file in
the codebase. It is also shared by diagnostics, completion, hover,
go-to-definition, and signature help, so navigability here de-risks
everything.

---

## Split the other resolution-pipeline giants

**What to do.** Same mechanical treatment for the three files that,
together with `forward_walk.rs`, form the shared resolution pipeline:

- **`src/completion/variable/rhs_resolution.rs` (4,456)** →
  `rhs_resolution/{dispatch, instantiation, array_access, calls,
  property_access}.rs`. `dispatch.rs` keeps
  `resolve_rhs_expression_inner` and `resolve_var_types`;
  `instantiation.rs` takes the `new`-expression/template-binding block
  (~960 lines); `calls.rs` takes function/method/static call return
  resolution (~1,300 lines); `property_access.rs` takes property access
  plus the `find_*_this_property_assignment*` scanners.
- **`src/completion/call_resolution.rs` (2,805)** — its three separate
  `impl Backend` blocks already partition it →
  `call_resolution/{target_cache, callable_target, return_types,
  template_subs, arg_type_resolution}.rs`. The two giant functions
  (`resolve_call_return_types_expr_with_hint` ~700 lines,
  `build_method_template_subs` ~450 lines) should additionally be
  decomposed by call kind while moving.
- **`src/completion/resolver.rs` (1,805)** → extract
  `resolver/context.rs` (the `Loaders`/`ResolutionCtx`/
  `VarResolutionCtx` types that every sibling module imports) and
  `resolver/property_narrowing.rs` (the `walk_property_narrowing_*`
  family).

**Why it matters.** These files implement the single shared type
pipeline that the project's conventions require all consumers to use.
Their size is the main obstacle to fixing bugs in it confidently.

---

## Deduplicate parallel helpers inside the resolution pipeline

**What to do.** The resolution files contain several
copies-for-another-code-path that should collapse onto one
implementation (do this after — or as part of — the splits above):

1. **Call return-type wrappers.** `resolve_rhs_method_call_inner` /
   `resolve_rhs_static_call` / `resolve_rhs_function_call`
   (`rhs_resolution.rs`) call into
   `Backend::resolve_method_return_types_with_args`
   (`call_resolution.rs`) but each re-implements the surrounding
   self/static substitution, union-owner expansion, and scalar
   fallbacks. Consolidate the pre/post logic into one shared entry
   point.
2. **Callable-param inference.** The `*_fw`-suffixed family in
   `forward_walk.rs` parallels the logic in
   `completion/variable/closure_resolution.rs`. The suffix itself marks
   a copy; unify them.
3. **`$this`/`self`/`static` resolution.** ~32 call sites spread across
   `util.rs` (`is_self_or_static`, `resolve_class_keyword`),
   `call_resolution.rs` (`resolve_class_name_keyword`), `resolver.rs`
   (`resolve_static_owner_class`), and `forward_walk.rs` (`seed_this`),
   plus hand-rolled `== "$this"` checks. Back them with one helper
   module.
4. **Subclass checks.** `is_subclass_of` (`forward_walk.rs`),
   `is_type_subclass_of` and `is_valid_virtual_narrowing`
   (`call_resolution.rs`), and `util::is_subtype_of*` overlap; route
   through the `util`/`php_type` versions.
5. **Property-assignment scanning.** The
   `find_*_this_property_assignment*` family (`rhs_resolution.rs`) and
   the `walk_property_narrowing_*` family (`resolver.rs`) walk class
   members and statements with near-identical skeletons for different
   outputs. Share the traversal.
6. **Argument-text extraction.** `extract_argument_texts_fw` /
   `extract_first_arg_string_fw` (`forward_walk.rs`) vs
   `extract_first_arg_text` / `resolve_inline_arg_raw_type` /
   `resolve_arg_text_to_type` (`call_resolution.rs`) vs
   `resolve_arg_raw_type` (`resolution.rs`).

**Why it matters.** These duplications are exactly the "parallel type
resolution systems" the conventions forbid, just internal to the
pipeline: a narrowing or generics fix applied to one copy silently
misses the others.

---

## Split `php_type.rs` into a module directory

**What to do.** After its test block moves out (see above), the
remaining ~4,500 production lines of `src/php_type.rs` split along
existing free-function clusters into `php_type/`:

- `mod.rs` — `PhpType`/`ShapeEntry`/`CallableParam` definitions,
  convenience constructors, and the small `is_*` predicate/accessor
  methods.
- `parse.rs` — `parse()`, the AST-conversion free functions
  (`convert`, `flatten_union`, `flatten_intersection`, `evaluate_*`),
  and raw-string preprocessing (`replace_star_wildcards`,
  `strip_variance_annotations_from_type`, `normalize_keyword_casing`).
- `subtype.rs` — `is_subtype_of` (~270 lines), `equivalent`,
  `is_named_subtype`, `literal_is_subtype_of`, `normalize_alias`.
- `normalize.rs` — `simplified`, `distribute_intersection`,
  `dedup_types`, `simplify_bool_union`, `absorb_scalar_refinements`.
- `transform.rs` — `resolve_names`, `shorten`, `replace_self*`,
  `substitute`, class-name collection.
- `display.rs` — the three `Display` impls and `format_shape_key`.
- `keywords.rs` — the pure `*_name` classifier free functions.

Also extract from `src/types.rs`: `SharedVec<T>` (a generic container
with its own impl suite) into `types/shared_vec.rs`, and the
`ResolvedType` impl (~340 lines of resolution/narrowing/join logic,
not data modelling) into `types/resolved_type.rs`.

**Why it matters.** `php_type.rs` is the core type representation every
feature depends on; `impl PhpType` blocks can be spread across files in
the same module, making this a mechanical split.

---

## Shared AST walker for the hand-rolled traversals

**What to do.** At least six modules hand-roll the same giant
`match` over `Statement`/`Expression` variants, each independently
re-typing the `IfBody`/`ForeachBody`/`WhileBody`/`SwitchBody` recursion:
`symbol_map/extraction.rs` (28 statement / 101 expression matches),
`selection_range.rs` (33/34), the anonymous-class walker in
`parser/classes.rs` (33/36), `completion/types/narrowing.rs` (64
expression matches), and the six boolean/name-set detector walker pairs
in `diagnostics/undefined_variables.rs` (dynamic-vars, extract(),
compact(), get_defined_vars(), `@`-suppression, isset/empty guards)
plus the structural halves of `unused_variables.rs` and
`type_errors.rs`.

`mago-syntax` already ships a generated `Walker` trait
(`walker/mod.rs`, per-node `walk_in_*`/`walk_out_*` hooks with full
recursive traversal) that none of these use. Migrate incrementally,
starting where the payoff is largest and the risk lowest:

1. The six detector pairs in `undefined_variables.rs` — each becomes a
   ~10-line visitor, removing ~1,400 lines of traversal boilerplate.
2. The statement-dispatch halves of `unused_variables.rs` and
   `type_errors.rs`.
3. The anonymous-class walker in `parser/classes.rs` (~1,000 lines).
4. `symbol_map/extraction.rs` and `selection_range.rs` as follow-ups.

The forward walker is explicitly out of scope here (its traversal is
interleaved with scope-state mutation; see the split item above).

**Why it matters.** Every new mago AST node variant (new PHP syntax)
currently needs matching arms added in six places; missing one produces
silent blind spots in exactly one feature. One traversal, many small
visitors is the structure all three reference projects use.

---

## `diagnostics/mod.rs` is a grab-bag around a thin orchestrator

**What to do.** The actual orchestrator
(`collect_slow_diagnostics`, ~85 lines) is fine, but it is buried in
2,928 lines of unrelated logic. Carve out of `src/diagnostics/mod.rs`:

- `external/{phpstan,phpcs,mago}.rs` — the four `schedule_*`
  subprocess spawn/debounce/parse pipelines (~700 lines, self-contained).
- `stale.rs` — `is_stale_phpstan_diagnostic` plus its helpers
  (~335 lines reconciling cached external diagnostics with edits).
- `suppression.rs` — `filter_suppressed`,
  `suppress_imprecise_overlaps`, `is_full_line_range` (~230 lines of
  post-processing policy).

While in there, introduce a shared `FileDiagnosticContext` built once
in `collect_slow_diagnostics` and passed to collectors — seven
symbol-span collectors (`unknown_classes.rs`, `unknown_functions.rs`,
`deprecated.rs`, `implementation_errors.rs`, `invalid_class_kind.rs`,
`unknown_members/mod.rs`, `unused_imports.rs`) currently open with a
near-verbatim 15-20 line lock-gathering preamble (symbol map, resolved
names, use map, namespace, local classes), and a shared snapshot also
guarantees they observe consistent state.

Also split `src/diagnostics/type_errors.rs`: `is_type_compatible` is a
single ~607-line function (plus four helper predicates) implementing
the diagnostic-policy layer over `util::is_subtype_of_typed`. Move it
to `type_errors/compatibility.rs`, and audit whether some of its
"MAYBE escape hatches" (IntRange, union handling, Generic/Traversable)
belong in `is_subtype_of_typed` where all callers would benefit.

**Why it matters.** Diagnostics is the most actively developed area;
new collectors keep landing in a module whose entry file is 75%
unrelated scheduling and filtering code.

---

## `util.rs` mixes eleven unrelated concerns

**What to do.** Break `src/util.rs` (2,613 lines, 87 functions) into
cohesive modules, and move single-consumer helpers to their only
consumer:

- `text_position.rs` — the most reused cluster: `LineIndex`,
  `offset_to_position`, `position_to_byte_offset`,
  `byte_range_to_lsp_range`, UTF-16 column conversion.
- `php_text.rs` — string scanning (`unquote_php_string`,
  `find_matching_forward`/`_backward`, `find_semicolon_balanced`,
  `collapse_continuation_lines`).
- `class_lookup.rs` — `find_class_by_name`, `find_class_at_offset`,
  `is_subtype_of*`, `is_self_or_static`, `resolve_class_keyword`
  (~360 lines).
- `process.rs` — `CommandOutput`, `run_command_with_timeout`.
- Move to their sole consumers: `log` + `progress_*` (only
  `server.rs`), `has_unclosed_delimiters` (docblock area),
  `find_identical_occurrences` (`code_actions`),
  `contains_php_attribute` + `find_brace_match_line`
  (`code_actions/phpstan`), `collect_php_files_gitignore` (workspace
  indexing).
- The `impl Backend` file-content/context accessors (`get_file_content`,
  `file_context*`, `namespace_at_offset`, `clear_file_maps`, …) are
  Backend behaviour, not utilities — move next to `lib.rs` (e.g.
  `backend/file_access.rs`).

**Why it matters.** "Put it in util" is how grab-bags grow; the
position-conversion cluster in particular is used by every feature and
deserves a findable home.

---

## `server.rs` carries a ~950-line workspace-init block

**What to do.** `src/server.rs` (2,821 lines) contains an
`impl Backend` block of workspace initialization and indexing that has
nothing to do with LSP dispatch: `init_single_project`,
`init_monorepo`, `init_no_composer`, `add_vendor_dir`,
`apply_watched_file_changes`, `rescan_composer_indexes`,
`scan_autoload_files`, `preload_autoload_files`, `scan_phar_archive`,
`build_self_scan_composer`, `populate_autoload_indices`. Move it to a
dedicated module (e.g. `src/workspace_init.rs` or `src/indexing/`).
`warm_laravel_completion_cache` belongs in `virtual_members/laravel/`.
Also move the pull-diagnostics resultId-cache logic embedded in the
`diagnostic` and `workspace_diagnostic` handlers into `diagnostics/`,
leaving the handlers as thin delegations like the rest.

Relatedly, `references/mod.rs` hosts `ensure_workspace_indexed`,
`parse_files_parallel`, and `parse_paths_parallel` — workspace
indexing, not reference finding. They should land in the same new
module.

**Why it matters.** Full background indexing has already shipped on
top of init logic scattered across `server.rs` and
`references/mod.rs`; leaving that logic unconsolidated guarantees more
sprawl as the feature continues to grow.

---

## Group `Backend`'s 67 fields into sub-systems

**What to do.** `struct Backend` in `src/lib.rs` has 67 fields that
cluster into implicit sub-systems. Highest value first:

1. **`ExternalToolWorker` struct.** The four external tools each add an
   identical `*_notify` / `*_pending_uri` / `*_last_diags` field triple
   (phpstan, phpcs, mago_lint, mago_analyze). One reusable struct
   removes twelve fields and makes adding the next tool (D10, PHPMD,
   already scheduled) a one-field change.
2. Diagnostic state (`diag_version`, `diag_notify`, `diag_pending_uris`,
   `diag_last_*`, `diag_result_ids`, `diag_suppressed`) →
   `DiagnosticState`.
3. Symbol/class indexes (`uri_classes_index`, `fqn_class_index`,
   `fqn_uri_index`, `gti_index`, `method_store`, `global_functions`,
   …) → `SymbolIndex`.
4. Workspace config (`workspace_root`, `psr4_mappings`, `vendor_*`,
   `php_version`, `config`) → `WorkspaceConfig`.

**Why it matters.** Item 1 directly de-risks the scheduled D10 task;
the rest makes the Backend's state graph legible and shrinks the
constructor.

---

## Split the logic-heavy `mod.rs` files

**What to do.** Per the project rule that `mod.rs` is a thin re-export
layer, split:

- **`src/references/mod.rs` (2,175)** → per-symbol-kind finders:
  `variables.rs`, `classes.rs`, `members.rs` (including the
  member-hierarchy resolution helpers), `functions.rs`, and a
  `dispatch.rs` for the entry points. (Workspace-indexing functions
  move out entirely — see the server.rs item.)
- **`src/hover/mod.rs` (1,885)** → `member.rs`, `variable.rs`,
  `class.rs`, `see_refs.rs`, `templates.rs`, `constants.rs`; the
  `formatting` submodule already models the pattern.
- **`src/scope_collector/mod.rs` (1,853)** → `scope_map.rs` (the query
  API), `collector.rs` (the recursive walker, ~1,050 lines),
  `build.rs` (the `collect_scope*` constructors).
- **`src/rename/mod.rs` (1,266)** → `class.rs`, `namespace.rs` (~450
  lines of namespace-rename edit building), `prepare.rs`.
- **`src/virtual_members/laravel/mod.rs` (920)** → move
  `resolve_laravel_string_key` + reference finding to
  `string_keys.rs`, and the builder-scope injection cluster
  (`try_inject_builder_scopes`, `inject_scopes_and_model_methods`, …)
  to `builder_injection.rs`, keeping only the provider impl and
  re-exports.
- **`src/analyse.rs` (1,194)** → `run.rs` (driver + file discovery)
  and `output.rs` (table/JSON/GitHub-annotation printers).

**Why it matters.** Logic in `mod.rs` is harder to find and grep for;
each of these is a mechanical move-only split.

---

## Split the remaining oversized single-concern files

**What to do.** After the test-block moves, these production bodies
remain over the threshold and have documented seams. Split each along
the seams (mechanical moves; do opportunistically, one per touch):

| File | Prod lines | Split |
| --- | --- | --- |
| `symbol_map/extraction.rs` | 3,603 | `statements.rs`, `class_like.rs`, `expressions.rs` (the 955-line `extract_from_expression` also needs decomposing by expression category), `subject_text.rs`, `laravel.rs`, `keywords.rs` |
| `parser/classes.rs` | ~2,760 | `laravel_model.rs` (casts/attributes/dates/relationship extraction, ~700 lines), `attributes.rs`, `anonymous.rs` (the anonymous-class walker, ~1,000 lines) |
| `code_actions/extract_function.rs` | ~2,496 | `context.rs`, `scope.rs`, `naming.rs`, `codegen.rs`, `returns.rs` (its own section banners) |
| `diagnostics/undefined_variables.rs` | ~2,075 | `feature_guards.rs`, `offset_guards.rs` (or collapse via the shared walker item) |
| `code_actions/phpstan/fix_return_type.rs` | ~2,080 | `inference.rs`, `edits.rs`, `message_parse.rs` |
| `completion/phpdoc/generation.rs` | ~1,900 | `trigger.rs`, `parse_decl.rs`, `build.rs` |
| `completion/handler.rs` | 1,877 | per-strategy: `member_access.rs`, `named_args.rs`, `class_constant.rs`, `phpdoc.rs`, `patching.rs` |
| `classmap_scanner.rs` | ~1,850 | `lexer.rs` (the intentional SIMD byte-lexer fast path — isolate, don't remove), `discovery.rs` |
| `completion/context/class_completion.rs` | ~1,900 | `context_detect.rs`, `heuristics.rs`, `attributes.rs`, `ctor.rs` (416-line `ctor_params_for`) |
| `completion/source/throws_analysis.rs` | ~1,740 | `scanning.rs`, `catch.rs`, `cross_file.rs` |
| `completion/types/narrowing.rs` | 1,720 | `resolve.rs` (452-line `resolve_extraction_to_fqn`), `instanceof.rs`, `assertions.rs`, `guards.rs` |
| `inheritance.rs` | ~1,550 | `enrichment.rs`, `traits.rs`, `generics.rs` |

Two dedup notes attached to this table:

- `extraction.rs::expr_to_subject_text` (202 lines matching ~30
  expression variants) duplicates
  `subject_expr.rs::SubjectExpr::to_subject_text`. Unify: build a
  `SubjectExpr` and render it, instead of a second serializer.
- `parser/classes.rs`'s Laravel-model extraction and
  `inheritance.rs`'s factory heuristics (`is_has_factory_trait`,
  `is_factory_class`) are framework logic living in generic modules;
  the splits should land them near `virtual_members/laravel/`.

**Why it matters.** These are all files the size checklist flags every
gate pass; recording the seams here makes each split a bounded task
instead of a re-analysis.

---

## Code actions: shared edit-building helpers

**What to do.** Complementary to (and independent of) A34 in the
backlog:

1. **`single_file_edit` helper.** `WorkspaceEdit` for one file is
   open-coded in ~35 files (~50 `document_changes: None` literals,
   8-12 lines each). Add
   `single_file_edit(uri, edits) -> WorkspaceEdit` /
   `single_edit(uri, range, text)` to `code_actions/mod.rs` and adopt.
2. **Adopt `util::byte_range_to_lsp_range`.** The helper exists but is
   used in code actions only by `phpstan/add_throws.rs`; ~29 other
   files hand-build `Range { start: offset_to_position(..), end: .. }`
   (183 `offset_to_position` calls).
3. **Merge the verbatim duplicate `find_method_insertion_point`** in
   `phpstan/add_override.rs` and
   `phpstan/add_return_type_will_change.rs` (~60 identical lines) into
   `phpstan/mod.rs`.
4. **Consolidate indent helpers.** Nine near-identical
   line-indent-at-offset extractors and two copies of
   `detect_indent_unit` across `extract_function.rs`,
   `generate_property_hooks.rs`, `update_docblock.rs`,
   `convert_switch_to_match.rs`, `extract_constant.rs`,
   `phpstan/new_static.rs`, `implement_methods.rs`. Provide
   `indent_of_line_at` / `indent_unit` next to
   `detect_indent_from_members`.
5. **Consolidate naming helpers.** `to_camel_case`, `snake_to_camel`,
   `to_pascal_case`, `string_to_screaming_snake`, `capitalise`, and
   two `deduplicate_name` implementations across the extract handlers
   → one `code_actions/naming.rs`.
6. **`find_docblock_above_line` helper.** At least three independent
   copies (`phpstan/remove_throws.rs`, `phpstan/add_throws.rs`,
   `phpstan/add_iterable_type.rs`) locate the `/** */` block above a
   line; `update_docblock.rs` additionally owns a private docblock
   line-model (`parse_docblock_lines`/`rebuild_docblock`) that the
   other handlers re-approximate. Extract a shared
   `code_actions/docblock_edit.rs`.
7. **Relocate PHPStan-specific logic out of `code_actions/mod.rs`**
   (`expand_sibling_checked_exception_diags`,
   `clear_phpstan_diagnostics_after_resolve`) into `phpstan/mod.rs`.

**Why it matters.** Several new code actions are scheduled (A40, A41,
H4-H24, FX rules); each currently starts by copying this plumbing from
a neighbour, adding another divergent copy per action.

---

## `fix_return_type` re-implements expression type inference

**What to do.** `src/code_actions/phpstan/fix_return_type.rs` contains
a parallel, text-based type inferencer: `infer_return_type` scans body
lines as strings, and `infer_type_from_literal` /
`infer_array_literal_type` / `split_array_elements` /
`find_top_level_arrow` re-derive array-literal and `new ClassName`
types from source text. Only the `$variable` case defers to the shared
pipeline. This is the "lightweight parallel resolver" the conventions
forbid. Route the array/`new`/expression cases through
`resolve_rhs_expression` (the AST is already parsed and cached in both
`handle_code_action` and `resolve_code_action`, so there is no
performance excuse). Audit `phpstan/add_iterable_type.rs::
infer_iterable_element_type` for the same pattern while there.

**Why it matters.** Direct violation of the single-pipeline rule:
every inference improvement (shapes, generics, literals) silently
misses this code action, and its answers can contradict hover.

---

## Shared PHP byte-scanning primitives

**What to do.** Four-plus modules maintain independent
"skip string / comment / heredoc while scanning bytes" primitives:
`classmap_scanner.rs` (inside `find_classes`/`find_symbols`),
`completion/source/throws_analysis.rs` (`skip_string_forward`,
`skip_line_comment`, `skip_block_comment`,
`find_matching_delimiter_forward`), `completion/named_args.rs`
(`skip_string_backward`), and
`completion/context/type_hint_completion.rs`
(`skip_string_literal_backward`). Consolidate into one shared module
(e.g. `src/text_scan.rs`) with forward and backward variants.

**Why it matters.** Each copy must independently track PHP lexical
quirks (heredoc flavours, escaping); they already differ in what they
handle, which means position-dependent bugs that reproduce in one
feature but not another.

---

## Error-handling hardening on request paths

**What to do.** Small, targeted fixes from the fragility audit (the
handlers are otherwise clean — no unguarded unwraps found beyond
these):

- `server.rs` `handle_with_position`: `f(content, pos.unwrap())`
  relies on a non-local invariant that callers pass `Some`; replace
  with an early return.
- `references/mod.rs` has two `Err(_) => return locations` arms that
  silently return partial find-references results, and
  `rename/mod.rs` has two `Err(_) => continue` arms that silently drop
  edits for files that fail to parse during a rename. A rename that
  silently omits edits is a correctness hazard: at minimum `log` the
  failure; consider surfacing a partial-result warning for rename.
- `rename/mod.rs`'s `strip_prefix(..).unwrap()` pairs are guarded but
  brittle — convert to `if let Some(rest)` while touching the file.

**Why it matters.** These are the paths where a silent failure looks
like "the LSP found nothing", which users report as broken features
with nothing in the logs to go on.

---

## Small cleanups

- `src/docblock/types.rs` is now a pure re-export shim (29 lines);
  delete it and point imports at `type_strings`/`shapes` directly.
- `code_actions/extract_function.rs::clean_type_for_signature` is a
  `#[cfg(test)]`-only string shim over
  `PhpType::parse(...).to_native_hint()`; port its tests to the typed
  version and drop it.
- `completion/context/catch_completion.rs` and
  `completion/source/throws_analysis.rs` hand-split unions on `'|'`
  and build `PhpType::Named` per part; route through `PhpType::parse`
  union handling where the input permits. (These are the last remnants
  of string-based type manipulation — the `PhpType` migration is
  otherwise complete.)


