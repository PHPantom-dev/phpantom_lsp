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

## 1. Fold the PHPStan and PHPCS workers onto the generic Mago worker

`diagnostics/external/mago.rs` already carries the generic form:
`mago_worker` takes the `ExternalToolWorker`, a label, a service
predicate, and a file runner, and both Mago commands are two-line
delegations to it. `phpstan_worker` (`diagnostics/external/phpstan.rs`)
and `phpcs_worker` (`diagnostics/external/phpcs.rs`) are hand-written
copies of the same loop, around eighty-five lines each, and the module
header in `diagnostics/external/mod.rs` already describes the shape as
shared. The four `schedule_*` functions are four copies of the same two
statements.

Generalise `mago_worker` into an `external_tool_worker` on `Backend`
whose per-tool parameters are the binary resolution (the only real
difference, plus the Mago-only `enabled_services` gate) and the runner,
move it to `diagnostics/external/mod.rs`, and reduce all four workers and
all four schedule functions to delegations.

**Files:** `src/diagnostics/external/mod.rs`,
`src/diagnostics/external/mago.rs`,
`src/diagnostics/external/phpstan.rs`,
`src/diagnostics/external/phpcs.rs`.

## 2. Convert offsets through `LineIndex` in code lens and inlay hints

`text_position.rs` documents `offset_to_position` as O(offset) and
`LineIndex` as the fix for converting many offsets from one piece of
content. `document_symbols.rs`, `folding.rs`, and `semantic_tokens.rs`
all build a `LineIndex` once per request. `code_lens.rs` and
`inlay_hints.rs` do not: they call `offset_to_position` once per member,
per lens, and per hint, and `code_lens.rs::line_indent` adds a second
backwards scan of the same prefix. Both are refreshed on every committed
change (`backend/documents.rs` asks for a code-lens and an inlay-hint
re-pull after each one), so a large file pays a full-content scan per
item on every keystroke.

Build the index once at the top of `handle_code_lens` and of the inlay
hint collection and thread it through the builders. Derive the indent
from the line start the index already knows rather than scanning
backwards for a newline.

**Files:** `src/code_lens.rs`, `src/inlay_hints.rs`.

## 3. Index `BladeBlockIndex` by parent instead of scanning every template

`blade/block_index.rs::descendants` walks "who extends what I have found
so far" by scanning every template in the project once per frontier
entry, and tracks the visited set in a `Vec<String>` searched with
`iter().any`. `is_rendered_by_a_template` scans every template's include
list, and it runs once per `@extends` hop inside `blade_render_scope`.
`blade_block_references` nests the two. In a real Laravel application
most pages extend one layout, so the visited set grows to roughly the
template count and the whole thing is quadratic — on the completion path
(`blade_block_name_candidates`), the diagnostics path
(`diagnostics/blade_sections.rs`), and find-references alike.

Give `Templates` a parent-to-children map and an included-view set built
alongside `by_view`/`view_by_uri` (and kept current in
`refresh_blade_block_index`), and make the visited set a `HashSet`.

**Files:** `src/blade/block_index.rs`.

## 4. Run `codeLens/resolve` through the shared request guards

Every other request reaches `Backend::with_file_content`, which installs
the chain-resolution cache, the type-engine resolvers, the per-request
parse cache, and the panic guard. `code_lens_resolve` in `server.rs`
calls `resolve_code_lens_item` directly, so a lens resolve runs with none
of them, and it fetches the file with `get_file_content`, which deep-copies
the whole buffer — whose own doc comment says to prefer the `Arc`
variant in hot paths. The editor sends one resolve per visible lens, so a
screenful repeats both costs.

`resolve_code_action` already does this correctly by installing the parse
cache and the resolver guard itself; give the lens resolve the same
treatment (or route it through a shared helper) and switch it to
`get_file_content_arc`.

**Files:** `src/code_lens.rs`, `src/server.rs`.

## 5. One reader for `vendor/composer/installed.json`

The Composer-1-array vs Composer-2-`{"packages": …}` dispatch is written
out three times, byte for byte: `composer.rs::extract_path_repo_psr4_mappings`,
`classmap_scanner/discovery.rs::vendor_package_roots`, and the workspace
scan further down the same file. A fourth, differently-spelled copy sits
in `virtual_members/laravel/macros.rs`.

Add a reader to `composer.rs` that returns the packages and the
`vendor/composer` directory, and route all four through it.

**Files:** `src/composer.rs`, `src/classmap_scanner/discovery.rs`,
`src/virtual_members/laravel/macros.rs`.

## 6. Give `VarResolutionCtx` a constructor

`VarResolutionCtx` has fifteen fields and twelve call sites write the
whole literal out, nine of whose fields are the same constant in every
one of them. Two of the sites are inside `type_engine/resolver/context.rs`
itself: `with_cursor_offset` and `with_match_arm_narrowing` each restate
all fifteen to change one. `cond_narrowing/apply.rs::build_var_ctx` is a
local extraction of exactly this, with a comment saying why, that nothing
outside that module can reach.

Add a constructor (or a `Default` plus struct-update syntax) beside the
existing `impl VarResolutionCtx`, rewrite the twelve sites to name only
the fields they actually choose, and make `build_var_ctx` a thin wrapper
over it.

**Files:** `src/type_engine/resolver/context.rs`, and the call sites in
`src/blade/{call_site_inference,shared_vars,typed_receiver}.rs`,
`src/type_engine/variable/resolution.rs`,
`src/type_engine/resolver/property_narrowing.rs`,
`src/diagnostics/{match_type_errors,return_type_errors,property_type_errors}.rs`,
`src/diagnostics/type_errors/mod.rs`,
`src/code_actions/phpstan/fix_return_type/inference.rs`.

## 7. Collapse the duplicated condition extractors in `cond_narrowing`

Four of the sentinel-comparison extractors in
`cond_narrowing/predicates.rs` share the same nine-line "compare against
the sentinel on either side, then take the other operand's subject key"
block, and `extract_isset_vars` / `extract_not_isset_vars` are
nineteen-line functions that differ by a single `!`. In
`cond_narrowing/null_narrowing.rs` the same six-line strip-null block
appears six times. `cond_narrowing/emptiness.rs` opens
`refine_non_empty_type` and `refine_empty_type` with the same eleven-line
union distribution.

`cond_narrowing/scope_edits.rs` is already the home for shared scope
edits and shows the pattern to follow. Extract a subject-key helper, a
sentinel-comparison helper parameterised by the sentinel predicate and
the polarity, an isset helper parameterised by the wanted polarity, the
strip-null block, and the union distribution. Keep the deliberate
asymmetry between the null-equality and non-null extractors: they cover
complementary polarities, not the same one.

**Files:** `src/type_engine/variable/forward_walk/cond_narrowing/predicates.rs`,
`.../null_narrowing.rs`, `.../emptiness.rs`, `.../scope_edits.rs`.

## 8. One `SUPERGLOBALS` list

`diagnostics/undefined_variables/mod.rs` and
`diagnostics/unused_variables.rs` each declare the same thirteen-entry
`SUPERGLOBALS` constant; only the doc comment differs. Adding a
superglobal currently means remembering both files.

Move it to `diagnostics/helpers.rs` and import it from both.

**Files:** `src/diagnostics/helpers.rs`,
`src/diagnostics/undefined_variables/mod.rs`,
`src/diagnostics/unused_variables.rs`.
