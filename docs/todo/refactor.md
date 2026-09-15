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

Filed by the Sprint 7 gate analysis of 2026-09-14 (fourth pass, sections
2 and 3, scoped to every `src/` and `tests/` file changed since 0.10.0).
Items are grouped so that each group touches a distinct set of files.

Section 2 (test placement) is clear: the integration-shaped tests that
lived in `src/` have moved to `tests/integration/`, and no test helper or
fixture is copy-pasted across three or more suites any more. Everything
below is section 3 (code duplication). R3 and the bulk of R4 are done;
R7 to R10 have not been started.

## R4. CLI output and external tools (remainder)

The worker loops, the per-file Mago commands, the workspace exit-code
tail, `WORKSPACE_TIMEOUT_FACTOR`, the full-line range, the summary boxes,
the findings table, the two bypassed helpers, and three of the four
worker pools are shared now. What is left:

- `src/analyse/output.rs`, `src/fix.rs`, `src/move_cli/output.rs`: three
  copies of the hand-written `"files"` JSON object loop (index/comma/
  newline bookkeeping around a per-item writer). `serde_json` is already
  a dependency, so the right fix is a `Serialize` struct per report
  rather than a shared string-builder; that changes the exact layout, so
  it needs the golden outputs updated in the same pass.
- `src/analyse/stages.rs`, `src/fix.rs`, `src/format_cli.rs`,
  `src/move_cli/run.rs`: four copies of the `report` dispatch (empty
  case, `GITHUB_ACTIONS` check alongside the table, per-format arm).
- `src/analyse/diagnose.rs` keeps its own worker pool. It takes a
  caller-supplied thread count and announces each file as it *starts* so
  a hang shows the in-flight set, neither of which
  `parallel::map_indexed` covers; give the helper an optional thread
  count and move the body over, or record why it stays.
- `src/analyse/diagnose.rs` bypasses `analysable_content_or` (116-126)
  and repeats the `Loaders` + `build_diagnostic_scopes` warm-up from
  `diagnostics/mod.rs:446-466`; use the shared helpers R5 adds.

Deliberately left alone, with the reason:

- `src/fix.rs::apply_text_edits` and
  `src/move_cli/run.rs::apply_text_edits` share only a five-line core.
  Their contracts genuinely differ: `fix` skips an edit whose range no
  longer matches, `move` fails the whole run. Unifying them means
  choosing one of those behaviours, which is a decision about the
  commands, not a refactor.

## R5. Diagnostics (remainder)

The shared `stop_at_inner_scopes!` macro, the Blade imbalance reporting
(now `src/diagnostics/blade_imbalance.rs`), and the
`analysable_content_or` bypass in `workspace.rs` are done. What is left:

- The `Loaders { … }` literal plus `build_diagnostic_scopes` call is
  written at `diagnostics/mod.rs:446`, `type_errors/mod.rs:370`,
  `return_type_errors.rs:583/733`, `property_type_errors.rs:345`,
  `match_type_errors.rs:82` (and in `analyse/diagnose.rs`, R4). Add a
  `Backend` helper that builds the loaders for a `FileContext`. Closure
  lifetimes make a factory that *returns* `Loaders` awkward, so hand
  them to a closure instead (`with_diagnostic_loaders(ctx, |loaders| …)`)
  or keep them in a struct that lends a `Loaders<'_>`.
- `src/diagnostics/return_type_errors.rs` and
  `src/diagnostics/property_type_errors.rs` share the collector prologue
  and the range-plus-`make_diagnostic` tail; extract them.
- `src/diagnostics/helpers.rs::find_matching_brace` re-implements
  `text_scan::find_matching_forward_bytes` without comment skipping
  (the file already imports the shared one); replace the two call
  sites and `find_enclosing_scope_end`'s inline copy. A `}` inside a
  `//` or `/* */` comment currently ends the match early, so this is a
  bug fix as well; it needs a changelog line.
- `make_diagnostic` bypasses: `syntax_errors.rs:77` and
  `implementation_errors.rs:132` build a fully substitutable literal;
  give `make_diagnostic` a tagged variant so `unused_variables.rs` (two
  identical sites, including the nine-line range preamble above each),
  `unused_imports.rs`, and `deprecated.rs` can use it;
  `class_name_mismatch.rs:57` and `namespace_mismatch.rs:52` are one
  helper.

## R6. Indexing, symbol map, Laravel extraction, inheritance (remainder)

`preload.rs`'s two parallel parse drivers are one generic function now.
What is left:

- `src/classmap_scanner/discovery.rs`: `scan_files_parallel_classes` /
  `_psr4` / `_full` share the parallel skeleton, and `_full` writes the
  classmap-merge block twice (735-760, 835-861). `fqcn_short_name` is
  `util::short_name`.
- `src/symbol_map/extraction/laravel.rs`: `string_literal_content` is
  `virtual_members::laravel::helpers::extract_string_literal` (also
  copied in `gates.rs:375` and `morph_map.rs:265`); four span pushers
  inline the same offset arithmetic (`push_morph_alias_span`,
  `push_gate_ability_span`, `push_can_middleware_span`,
  `names_a_render_each_template`). The string-or-array argument
  dispatcher is byte-identical at 984, 1180, 1309; the two config-array
  walkers (394, 427) differ by a flag; the four `Statement::If` arms
  (here at 1799, `helpers.rs:1228`, `database_schema.rs:1207/1395`)
  should use `route_names::for_each_nested_statement`.
- `src/virtual_members/laravel/provider_resources.rs`:
  `is_app_container_expr` duplicates
  `symbol_map::extraction::laravel::is_laravel_container_expr`;
  `chain_roots_at_route` duplicates `chain_roots_at_facade`; the three
  `load*From` branches (626-658) are one table.
- `src/virtual_members/laravel/mod.rs::find_class_in` is
  `class_lookup::find_class_by_name`.
- `src/virtual_members/laravel/model_extraction.rs`:
  `extract_collected_by_attribute` should delegate to
  `extract_class_constant_attribute` like its siblings;
  `extract_custom_builder` / `extract_custom_collection` and
  `parse_casts_array` / `parse_string_list` share their scaffolding.
- `src/virtual_members/laravel/macros.rs`: the Class/AnonymousClass/
  Trait/Enum member-body arms at 798 and 1242 should use
  `route_names::visit_class_member_bodies`.
- `src/inheritance/mod.rs` and `src/inheritance/traits.rs`: the
  template-bound fallback block is copied three times (243, 483,
  `traits.rs:92`); the parent-walk visibility/dedup/push trio in
  `traits.rs:157-186` is a stripped copy of `mod.rs:316-443`.

## R7. Forward walker

- `src/type_engine/variable/forward_walk/control_flow.rs`:
  `process_if_statement_body` (93-390) and `process_if_colon_body`
  (394-644) are the same algorithm over two body types. Share the merge
  half. The catch-variable binding in `process_try` is written twice
  (2733, 2777). `receiver_class_names` here has the same name and
  signature as the one in `assignment.rs` with different behaviour;
  rename one.
- `src/type_engine/variable/forward_walk/assignment.rs`:
  `seed_pass_by_ref_primitives` has identical `Call::Method` and
  `Call::NullSafeMethod` arms (2807-2896) and re-implements the callee
  prologue that `resolution.rs::try_resolve_*_params` already provides;
  the compound-assignment operator ladder appears at 1902 and 1980;
  five hand-rolled `VarResolutionCtx` literals (2252, 2620, 3110, 3341,
  3490) plus the 16-site scope-snapshot closure should be one
  `ForwardWalkCtx` method; drop the pass-through wrappers
  `function_param_has_invocation_tag`, `parse_all_inline_var_docblocks`,
  `parse_all_var_docblock_annotations`; `unwrap_parens` is also in
  `code_actions/simplify_null.rs:744`.
- `src/type_engine/variable/forward_walk/cond_narrowing/scope_edits.rs`:
  the `refine_*_in_scope` / `strip_*_from_scope` family shares one
  map-and-set skeleton; `holds_null` is `scope_state::type_admits_null`.
- `src/type_engine/variable/forward_walk/cond_narrowing/instanceof.rs`:
  the negated-instanceof exclusion block appears three times in
  `commit_chain_instanceof`.
- `src/type_engine/variable/forward_walk/diagnostic_walk.rs`:
  `Array` / `LegacyArray` arms (260-287) and the same pair in
  `assignment.rs:531-545` should use `array_element_value`.
- `resolve_type_to_resolved_types` / `resolve_from_authoritative_type`
  exist, yet the `type_hint_to_classes_typed` then
  `from_classes_with_hint` else `from_type_string` idiom is hand-rolled
  ~20 times across `rhs_resolution/calls.rs`, `control_flow.rs`,
  `assignment.rs`. Use the helper where the fallback is
  `from_type_string`.
- The `self` / `static` / `parent` class-expression resolution is
  written seven times (`resolution.rs:2753/2778`, `assignment.rs:2062/
  2902/3252`, `rhs_resolution/property_access.rs:98/221`,
  `rhs_resolution/calls.rs:2514`) with divergent `parent` handling in
  `property_access.rs`. Add one AST-level helper.

## R8. Type engine core and `php_type`

- `src/type_engine/call_resolution/return_types.rs:1425-1535`: the
  `NewExpr` arm re-implements `build_method_template_subs`; call it with
  `__construct`. The four quote/depth scanners (`contains_top_level_concat`,
  `split_top_level_coalesce`, `split_top_level_elvis`,
  `operand_is_single`) share a 22-line skeleton; the operand-join block
  at 2476/2488 is duplicated; the `MethodReturnCtx` prologue is written
  three times (763, 832, 935); the self/parent hint-capture skeleton
  twice (736, 910).
- `src/type_engine/call_resolution/{target_cache,out_param}.rs`,
  `src/type_engine/resolver/context.rs`,
  `src/type_engine/variable/resolution.rs`: five identical thread-local
  memo RAII guards; one macro or generic type.
  `try_infer_body_return_type` and `infer_out_type` repeat the
  memo / depth-cap / visited protocol.
- `src/type_engine/call_resolution/callable_target.rs`: the template
  substitution and conditional-collapse block is duplicated between
  the instance and static resolvers (147-206, 312-363).
- `src/type_engine/call_resolution/arg_type_resolution.rs::extract_first_arg_text`
  is `split_text_args(..).next()` and is not quote-aware.
- `src/type_engine/types/conditional.rs`: `split_call_subject` is a
  verbatim copy of `subject_expr::split_call_subject_raw`; the
  undecided-branch closure appears three times (528, 584, 675); the
  one-class-or-union block twice (394, 460).
- `src/type_engine/resolver/mod.rs`: `ClassName` / `NewExpr` arms
  (513-542), `is_bare_variable` (860, 1608), and the method/static
  arms of `resolve_call_raw_return_type` / `declared_call_return_type`.
- `src/type_engine/resolver/property_narrowing.rs`: `IfBody::Statement`
  vs `ColonDelimited` walks (437-608), three identical loop-body
  dispatches (332-373), and the call-argument extraction block (708,
  950) that should use `narrowing::instanceof::argument_value`.
- `src/type_engine/types/narrowing/{assertions,resolve}.rs`: merge the
  `Method | NullSafeMethod` and `Property | NullSafeProperty` arms.
- `src/types/mod.rs::virtual_method` should delegate to
  `virtual_method_typed` like `virtual_property` does.
- `src/php_type/mod.rs`: `to_native_hint` should delegate to
  `to_native_hint_typed`; `extract_shape_key_type` should call
  `shape_entry`.
- `src/types/resolved_type.rs`: `restrict_union_to_classes` /
  `subtract_classes_from_union` share their tail; the keep-mask
  `retain` idiom is written five times (`normalize.rs:280/899/985`,
  `resolved_type.rs:576`).
- `src/php_type/parse.rs`: shape-field conversion (379, 407) and
  `flatten_union` / `flatten_intersection`.
- `src/php_type/transform.rs`: seven structural rebuild walks
  (`resolve_names`, `shorten`, `resolve_self_refs_bounded`,
  `conditionals_as_branch_unions`, `unevaluated_operators_as_bounds`,
  `replace_self_inner`, `substitute`) plus
  `conditional.rs::evaluate_nested_conditionals_text` rebuild every
  `TypeKind` arm by hand. Add `PhpType::map_children` and express the
  walks as leaf handlers over it.

## R9. Editor features

- `src/rename/class.rs`: `build_class_rename_edit` (203-382) is the
  degenerate case of `build_class_move_edit` (389-735); ~110 lines are
  identical.
- `src/rename/namespace.rs`: three prefix-match/rewrite/emit blocks
  (293, 350, 383) should use the file's own `moved_name`; the hand
  `use` line scanner at 322-406 should be
  `diagnostics::helpers::scan_use_statements`.
- Namespace insert position is computed in
  `code_actions/fix_namespace.rs:97`, `rename/class.rs:1500`, and
  `completion/use_edit.rs:298` with a `declare()` placement
  disagreement. One helper.
- `src/code_actions/import_class.rs`: the unresolved-name check is
  written four times (98, 261, 593, 802) and the candidate-to-action
  loop twice (158, 306).
- `src/code_actions/{convert_to_instance_variable,inline_variable}.rs`:
  occurrence-replacement plus edit-sort (474, 721);
  `inline_variable.rs:139-201` three identical class-like arms should
  use `util::find_enclosing_method_block_in_members`.
- `src/code_actions/{extract_interface,implement_methods}.rs`: the
  innermost-class lookup is copied three times.
- `src/code_actions/docblock_edit.rs`: `find_docblock_above_line` and
  `docblock_start_above_offset` are two implementations; the former
  computes offsets with `len() + 1` (CRLF bug) instead of
  `text_position::line_start_byte_offset`.
- `src/docblock/tags.rs`: `@var` / `@param` token splitting (716,
  1178) and the two preceding-docblock finders (573, 1305).
- `src/parser/classes.rs` vs `src/parser/functions.rs`: the
  docblock-to-parameter merge (1173-1236 vs 317-408) is ~50 identical
  lines, and the method side lacks the positional `@param` fallback.
- `src/parser/classes.rs`: four ~50-line `ClassInfo` literals (264,
  378, 478, 626) with ~30 identical fields, plus the surrounding
  guard / destructure / offset boilerplate. `ClassInfo: Default`; use
  `..Default::default()` and a shared builder. `test_fixtures::make_class`
  is 45 lines of default values.
- `src/parser/ast_update.rs:562/612` and `src/parser/mod.rs:1302`: the
  class-like statement arm list is written three times.
- `src/hover/member.rs`: seven copies of the per-kind member match
  (`has_member`), three near-identical hover tails (575, 661, 731).
- `src/inlay_hints.rs`: `traits_have_{method,property,constant}` and
  `ancestor_has_{property,constant}` are one generic walk each.
- `src/completion/context/override_completion.rs`: `collect_from_traits`
  ×3, `collect_from_interface` ×2, `override_edit` block ×3, the
  property / constant item builders; `skip_quoted` is
  `text_scan::skip_string_forward`; seven copies of the backward
  identifier scan (also in `completion/handler/member_access.rs`).
- `src/completion/source/helpers.rs`: four copies of a balanced-paren
  scan (186, 210, 435, 655) that `text_scan::find_matching_forward`
  provides.
- `src/references/`: `sort_locations_for_references` exists and seven
  sites hand-roll it (two without `dedup`); `classes.rs` resolves a
  span name to an FQN three times (56, 96, 239).
- `src/definition/member/mod.rs`: the seven-deep fallback cascade is
  duplicated (263-349 vs 505-647), and the file-content / position /
  `point_location` block appears seven times, the Eloquent-array
  variant five times.
- `src/definition/member/file_lookup.rs`: the `@property` and `@method`
  scans (199, 234) differ only in that the second returns a byte column
  as UTF-16.
- `src/definition/implementation.rs`: four index-scan phases (806, 872,
  904, 987) share one body; `interface_extends_target` repeats its
  branch; `find_member_position_in_class` counts lines by hand instead
  of `LineIndex`.
- `src/backend/file_access.rs`: `file_context_at` should build on
  `file_context`.
- `src/call_hierarchy.rs`: `build_method_callable` /
  `build_function_callable`.
- `src/workspace_symbols.rs` and `src/document_symbols.rs`: four
  per-kind symbol builders each.
- `src/server.rs`: eight position-request handlers share a 9-line
  prologue/epilogue; `src/lsp_dispatch.rs` three 11-line wrappers.
- `src/hover/mod.rs`: Config / Trans arms of `hover_laravel_string_key`.
- `src/code_actions/replace_deprecated.rs`: function-call vs
  member-call arms (106, 178).

## R10. `ResolutionCtx` factory

The 13-field `ResolutionCtx` literal with identical fixed fields is
written in `hover/mod.rs:178`, `definition/member/mod.rs:156`,
`definition/type_definition.rs:90`, `definition/implementation.rs:534`,
`completion/handler/member_access.rs:67/95`, `backend/laravel_scan.rs`
(three sites), and ~15 more places outside the changed set. Add
`Backend::resolution_ctx_at(...)` and use it. Run this after R7 to R9
since it touches their files.
