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

From a broad sweep on 2026-10-19 of everything changed since 0.11.1,
read against the Sprint 8 tasks. The first group blocks a Sprint 8
task; the second is accumulated friction found on the way. Bugs the
sweep found are filed in `bugs.md` (B642 to B645), not here.

## Blocks a Sprint 8 task

### Split `src/config.rs`

2048 lines: about 1000 of config types and loading, plus a 1042-line
inline test module. X17 adds `extra_roots` to `ClientIndexingOptions`,
and E3 adds a `[stubs]` section, both to this file. Make it a
`src/config/` directory: `mod.rs` (`Config`, `merge_toml`,
`load_config_from`, the create/write helpers, `ConfigError`),
`diagnostics.rs` (`DiagnosticsConfig`, `IgnoreRule`), `tools.rs`
(`FormattingConfig`, `PhpStanConfig`, `MagoConfig`, `PhpcsConfig`,
`PhpmdConfig`), `indexing.rs` (`IndexingConfig`, `ClientIndexingOptions`,
`IndexingStrategy`), `laravel.rs` (the `Laravel*Config` types) and
`tests.rs` for the test module. Re-export from `mod.rs` so callers keep
`crate::config::X`.

Files: `src/config.rs` → `src/config/*.rs`.

### Split `apply_watched_file_changes` and move its tests out of `src/indexing/watch.rs`

`apply_watched_file_changes` (`src/indexing/watch.rs`) is a single
267-line function, and the file carries 777 lines of inline tests that
build a `Backend` 23 times. X17 has to add re-registration when the
extra roots change, which lands in this file. Break the function into
steps (classify the events, config and composer reloads, the re-index,
cache invalidation) and move the test module to
`src/indexing/watch_tests.rs`.

Files: `src/indexing/watch.rs`, `src/indexing/watch_tests.rs` (new).

### Split `find_implementors` into its phases

`Backend::find_implementors` (`src/definition/implementation.rs`) is a
316-line function with three commented phases (index lookup, collecting
unparsed sources, parsing sources that name a subtype). X17's "switch
every walker together" goes through go-to-implementation's walk. Give
each phase its own method, and move the 738-line inline test module to
`src/definition/implementation_tests.rs`.

Files: `src/definition/implementation.rs`,
`src/definition/implementation_tests.rs` (new).

### Route URI and path conversion through `util`

F20 replaces `url::Url` with a `fluent_uri`-based `Uri`. Non-test code
calls `Url::parse`, `Url::from_file_path`, `Url::from_directory_path` or
`.to_file_path()` directly at 95 sites, while `util::path_to_uri` and
`util::uri_to_path` already exist. The biggest offenders are
`definition/implementation.rs` (15), `virtual_members/laravel/config_keys.rs`
(9), `definition/resolve.rs` (8), `virtual_members/laravel/string_keys.rs`
(6) and `indexing/watch.rs` (6). Add the `Url`-returning variants that
are missing to `src/util.rs` and send every site through them, so F20
changes one module instead of ninety.

Files: `src/util.rs` plus the call sites (`grep -rnE 'Url::(parse|from_file_path|from_directory_path)|\.to_file_path\(\)' src`).

### Make the code-action collector list declarative

`handle_code_action` (`src/code_actions/mod.rs`) calls 30 collectors
one after another by hand. BL1 needs a per-collector policy for
templates (drop PHPStan and Mago fixes, resolve deferred actions before
offering, reject partially translated edits). Turn the sequence into a
static table of collectors, each with its template policy, so BL1 adds
a field instead of an `if is_blade_file` beside each call.

Files: `src/code_actions/mod.rs`.

## Accumulated friction

### Convert byte ranges through `byte_range_to_lsp_range`

About 25 places call `offset_to_position` twice to build a `Range` by
hand, where `byte_range_to_lsp_range` (`src/text_position.rs`) does it in
one cached step: `code_actions/helpers.rs` (two), `docblock_edit.rs`,
`convert_to_instance_variable.rs`, `use_line_deletion.rs` (two),
`phpstan/remove_unreachable.rs`, `phpstan/attributes.rs`,
`insert_translation_key.rs` (three), `change_visibility.rs` (two),
`generate_property_hooks.rs`, `generate_constructor.rs`, `mago.rs`,
`update_docblock/mod.rs`, `diagnostics/class_name_mismatch.rs` and
`definition/resolve.rs` (two). Delete the private copies too:
`byte_deletion_edit` and `line_start_offset` in `use_line_deletion.rs`
(use the helper and `text_position::line_start_byte_offset`). B640's
`remove_assert.rs` fix is the hand-rolled case that is also wrong.

Files: the ones listed, all under `src/`.

### One `@phpstan-ignore` identifier parser

The identifier list of an ignore comment is parsed in five places:
`src/phpstan_ignore.rs` and `src/diagnostics/stale.rs` (each with its own
depth-aware scan) and three naive copies in
`src/code_actions/phpstan/ignore.rs` (B644). Add one
`parse_ignore_ids(line)` to `phpstan_ignore.rs` that returns each id with
its span and the span of the whole list, and use it at all five sites.

Files: `src/phpstan_ignore.rs`, `src/diagnostics/stale.rs`,
`src/code_actions/phpstan/ignore.rs`.

### Merge "Add `#[Override]`" and "Add `#[ReturnTypeWillChange]`"

`src/code_actions/phpstan/add_override.rs` and
`add_return_type_will_change.rs` are about 90 lines of the same
collect/resolve code, differing in the identifier, the attribute name
and the import policy, and they have already drifted (only one has a
staleness check). `extract_method_name` is in both, and the same parsing
is inlined in `remove_override.rs`. Write one "add attribute for
identifier" collector and resolver parameterised by those three things,
and one shared `phpstan_member_name(message)`.

Files: `src/code_actions/phpstan/add_override.rs`,
`add_return_type_will_change.rs`, `remove_override.rs`.

### A `deferred_quickfix` helper for PHPStan actions

Ten deferred PHPStan quick fixes each build the
`diagnostic_message`/`diagnostic_line`/`diagnostic_code` JSON, call
`make_code_action_data` and assemble a `QUICKFIX` `CodeAction` with no
edit, about 20 lines each: `fix_phpdoc_type.rs`, `fix_prefixed_class.rs`,
`ignore.rs`, `add_override.rs`, `remove_throws.rs`, `add_throws.rs`,
`new_static.rs` (three) and `add_return_type_will_change.rs`. Add
`deferred_quickfix(title, kind, uri, diag, extra)` to
`src/code_actions/helpers.rs` beside `rewrite_action`.

Files: `src/code_actions/helpers.rs`, `src/code_actions/phpstan/*.rs`.

### One docblock renderer for generated code

New docblocks are assembled by hand in `generate_getter_setter.rs` (two),
`extract_interface.rs` (two), `extract_function/codegen.rs` and
`docblock_edit.rs` (two). Add `render_docblock(indent, lines, eol)` to
`src/code_actions/docblock_edit.rs` and route all of them through it.
Taking `eol` there is most of B634 for these actions.

Files: those under `src/code_actions/`.

### Replace hand-built LSP params in the integration tests

`tests/integration/` builds `DidOpenTextDocumentParams { … }` by hand
1445 times across 77 files and `TextDocumentPositionParams { … }` 1459
times across 102, although `common/mod.rs` has `open_php`,
`complete_at`, `hover_at`, `goto_definition_at` and friends. The worst
are `completion_variables.rs` (291), `completion_generics.rs` (111),
`completion_array_shapes.rs` (89), `definition_members.rs` (85) and
`definition_variables.rs` (63). Convert file by file; it is mechanical
but large, so do it in the main session in batches of files, not as one
rewrite.

Files: `tests/integration/*.rs`.

### Report failures the user can act on

- `composer/mod.rs`'s `read_composer_package` turns a missing, unreadable
  and malformed `composer.json` into the same `None` (16 callers), so a
  syntax error silently drops PSR-4 mappings and the PHP version. Return
  a `Result` and have startup (`backend/startup.rs`) send a warning
  through `self.log` for a parse error.
- A `.phpantom.toml` that fails to reload (`indexing/watch.rs`
  `reload_config`), a phar that fails to read or parse
  (`indexing/phar.rs`) and a schema or migration reload failure
  (`backend/laravel/schema.rs`) only reach `tracing::warn!`, which the
  default filter hides. Send them through `self.log` the way the initial
  config load does.
- `composer/installed.rs` and `composer/autoload_files.rs` treat a
  corrupt file like a missing one; log the parse error.

Files: as listed.

### Shorten lock scopes that span file IO or long walks

- `references/variables.rs` holds the `symbol_maps` read guard for the
  whole reference walk (which also takes `uri_classes_index`); clone the
  map's `Arc` and drop the guard.
- `virtual_members/laravel/config_keys.rs`, `route_names.rs`
  (`enumerate_all_routes`) and `backend/laravel/mod.rs` read files while
  holding `laravel_provider_resources` or `vendor_dir_paths`; copy the
  path list first.
- `backend/laravel/schema.rs` holds the `schema_index` write guard
  across migration reads and `clear_resolved_class_cache()`; read first,
  then swap the index in.

Files: as listed.

### Measure the parse and copy costs the sweep found

None of these has been measured, so check each against the performance
bar before doing it, and file the ones that clear it in
`performance.md`:

- One keystroke parses an open file three or more times: `update_ast`
  (`parser/ast_update.rs`) discards its tree, then fast diagnostics
  (`diagnostics/collect.rs`) and slow diagnostics
  (`diagnostics/incremental/mod.rs`) each parse under their own cache,
  and every request parses once more.
- The Laravel refreshers `update_ast` calls (macros, morph map, gates,
  provider resources, commands) each parse the same content again, up to
  six times for a service provider.
- `find_or_load_class_typed` (`resolution/classes.rs`) re-applies the
  auth, storage, macro and pivot patches on every lookup, copying the
  whole `ClassInfo` and handing out a new `Arc` each time, which also
  defeats the pointer-identity shortcut in
  `forward_walk/scope_state/merge.rs`. A memo keyed on the class and an
  index generation would fix it.
- The per-thread parse arena (`parser/ast_update.rs`) keeps the peak size
  of the largest file each worker ever parsed, and `Atom` (`ustr`) never
  frees, so every partial identifier interned mid-edit stays for the
  session.

### Split the largest Laravel modules

Not touched by Sprint 8, but each does several jobs:
`virtual_members/laravel/database_schema.rs` (1826 lines of code: file
discovery, SQL dump parsing, and migration interpretation, which should
be three files), `virtual_members/laravel/route_names.rs` (1958:
resource registration, macro and include traversal, and route discovery)
and `virtual_members/laravel/model_extraction.rs` (1425).
`symbol_map/mod.rs` (1202) holds the `SymbolMap` type and its whole
`impl`; move those to `symbol_map/map.rs` and keep `mod.rs` a re-export
layer.

Files: as listed.
