# PHPantom — Performance

Internal performance improvements that reduce latency, memory usage,
and lock contention on the hot paths. These items are sequenced so
that structural fixes land before features that would amplify the
underlying costs (parallel file processing, full background indexing).

Items are ordered by **impact** (descending), then **effort** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Effort** | **Low** (≤ 1 day), **Medium** (2-5 days), **Medium-High** (1-2 weeks), **High** (2-4 weeks), **Very High** (> 1 month) |

---

## P3. Parallel pre-filter in `find_implementors`

**Impact: Medium · Effort: Medium**

`find_implementors` Phase 3 reads every unloaded classmap file
sequentially: `fs::read_to_string`, string pre-filter for the target
name, then `parse_and_cache_file`. On a project with thousands of
vendor classes, this loop is dominated by I/O latency. The string
pre-filter rejects most files (the target name appears in very few),
so the vast majority of reads are wasted.

### Fix

Split Phase 3 into two sub-phases:

1. **Parallel pre-filter.** Collect the candidate paths into a
   `Vec<PathBuf>`, then use `std::thread::scope` to read files and
   run the `raw.contains(target_short)` check in parallel. Return
   only the paths that pass the filter along with their content.

2. **Sequential parse.** For the (few) files that pass, call
   `parse_and_cache_file` sequentially. This step mutates `uri_classes_index`
   and calls `class_loader`, which may re-lock shared state.

The same pattern applies to Phase 5 (PSR-4 directory walk for files
not in the classmap). The pre-filter I/O is the bottleneck; the
parse step processes very few files and is fast.

### Trade-off

Thread spawning overhead is only worthwhile when the candidate set
is large. Skip parallelism when the candidate count is below a
threshold (e.g. 8 files).

---

## P9. `resolved_class_cache` generic-arg specialisation

**Impact: Medium · Effort: Medium**

The resolved-class cache is keyed by `(FQN, Vec<String>)`. Every
distinct generic instantiation of the same class (e.g.
`Builder<User>`, `Builder<Order>`, `Builder<Product>`) triggers a
full `resolve_class_fully` call, even though the base resolution
(inheritance merging, trait merging, virtual member injection) is
identical. Only the final generic substitution differs.

In a Laravel codebase with hundreds of Eloquent models, this means
`Builder` is fully resolved hundreds of times, once per model.

### Fix

Cache the base-resolved class (before generic substitution)
separately, keyed by FQN alone. When a generic instantiation is
requested, look up the base-resolved class and apply
`apply_substitution` on top. The substitution step is cheap
(tree walk) compared to the full resolution (inheritance walking,
trait merging, virtual member providers).

This requires splitting `resolve_class_fully` into two stages:
base resolution (cached by FQN) and generic specialisation (cached
by `(FQN, Vec<String>)` as today, but with a much cheaper miss
path).

---

## P11. Uncached base-resolution in `build_scope_methods_for_builder`

**Impact: Low-Medium · Effort: Low**

`build_scope_methods_for_builder` calls
`resolve_class_with_inheritance` (base resolution) for the model
class. This is not covered by the thread-local resolved-class
cache, which stores fully-resolved classes (after virtual member
injection), not base-resolved ones.

Every time an Eloquent `Builder<Model>` is resolved with scope
injection, the model is base-resolved from scratch. With many
Builder instantiations in a single file this adds up.

### Fix

Either introduce a separate base-resolution cache (keyed by FQN),
or restructure so `build_scope_methods_for_builder` accepts the
already-resolved model class from the caller (which may already
have it from the resolved-class cache).

---

## P14. Eager docblock parsing into structured fields

**Impact: Medium · Effort: Medium**

> **Note.** This item needs refinement when we work on it. The
> codebase and feature set may change significantly before then.

Currently `ClassInfo::class_docblock` stores the raw docblock
string. Every consumer that needs virtual members (`@method`,
`@property`, `@property-read`, `@property-write`) re-parses the
raw text via `PHPDocProvider`. Hover, completion, and diagnostics
all trigger this independently.

Parse the class-level docblock once during extraction and store the
structured results directly on ClassInfo:

- A list of parsed `@method` signatures (name, parameters, return
  type, static flag, description).
- A list of parsed `@property` / `@property-read` /
  `@property-write` entries (name, type, access mode, description).

This has three benefits:

1. **Drop the raw string.** For heavily-annotated classes (Eloquent
   models, facades) the raw docblock can be hundreds of bytes.
   The structured representation may be comparable in size but is
   directly usable without re-parsing.

2. **Eliminate repeated parsing.** Virtual member resolution
   currently re-parses the same docblock text on every completion,
   hover, and diagnostic pass. Parsing once during extraction
   removes this redundant work.

3. **Simpler consumer code.** Consumers iterate structured fields
   instead of calling into the docblock parser. This removes the
   lazy-parse indirection and makes the data flow easier to follow.

The same principle applies to other docblock data that is currently
extracted from raw text at multiple read sites (descriptions, link
URLs, see references), though those are smaller wins.

---

## P15. Two-phase stub index construction (eliminate `RwLock` on stub maps)

**Impact: Low · Effort: Medium**

The three stub indexes (`stub_index`, `stub_function_index`,
`stub_constant_index`) are write-once-read-many maps. They are
populated at construction time from the compiled-in phpstorm-stubs
arrays, then filtered once in `set_php_version` (called during
`initialized`) to evict entries with `@removed X.Y` tags. After
that single mutation they are never written again.

Because the PHP version is not known at construction time (it comes
from `composer.json` / `.phpantom.toml`, read during `initialized`),
the maps are currently wrapped in `parking_lot::RwLock` so that
`set_php_version` can call `.write().retain(…)`. Every subsequent
read — ~24 call sites across completion, resolution, diagnostics,
hover, and definition — acquires a shared read lock. On the
uncontended path this is a single atomic CAS (~1-5 ns), so the
cost is negligible in practice, but it is architecturally wasteful
for data that never changes after startup.

### Ideal solution

Split `Backend` construction into two phases so that the stub maps
are plain `HashMap`s with zero synchronisation cost on reads:

1. **Phase 1 — skeleton construction.** Create the `Backend` with
   empty (or placeholder) stub maps. No `RwLock` needed because
   nothing reads them yet.

2. **Phase 2 — version-aware population.** In `initialized`, after
   detecting the PHP version, build the filtered maps (applying
   `is_stub_function_removed` / `is_stub_class_removed` during
   construction rather than via `retain`) and store them on the
   backend through a one-shot setter that consumes the maps by
   value.

The setter could use `std::sync::OnceLock<HashMap<…>>` (or simply
an `UnsafeCell` behind a "set-exactly-once" assertion) to make the
write safe without ongoing read-side cost. Alternatively, the
fields can stay as plain `HashMap` if the `Backend` struct is built
in `initialized` rather than `initialize` — moving construction
after the version is known.

### Prerequisites

This interacts with the test helpers (`new_test`,
`new_test_with_stubs`, etc.) which currently call
`set_php_version` in the constructor. They would need to accept
a `PhpVersion` parameter or build the filtered maps inline.

### When to implement

Low priority. The current `RwLock` overhead is unmeasurable in
practice (~10-20 ns per completion request). Worth revisiting if
the stub indexes grow significantly or if `Backend` construction
is restructured for other reasons.

---

## P16. Pre-parsed stub format (eliminate raw PHP embedding)

**Impact: High · Effort: Medium-High**

The 630 phpstorm-stubs PHP files are embedded as raw source via
`include_str!` (~9.8 MB in `.rodata`). This has three costs:

1. **Permanent RSS.** The 9.8 MB is memory-mapped into every
   process regardless of how many stubs are actually accessed.
   That is ~17% of the current 59 MB baseline and will become a
   larger relative share as vendor indexing grows the working set.

2. **Parse cost on first access.** Each stub is parsed with the
   full mago parser on first use (`parse_and_cache_content_versioned`).
   Large files like `intl.php` (296 KB) take several milliseconds.
   A Symfony project can trigger hundreds of stub parses as vendor
   classes extend built-in types.

3. **Duplicate data.** After parsing, the `Arc<ClassInfo>` lives in
   `uri_classes_index` and `fqn_index`, but the raw PHP source stays resident
   in `.rodata` forever. Both copies exist simultaneously.

### Indexing order: stubs → vendor → user

Background indexing will load data in dependency order:

1. **Stubs** (built-in PHP classes, functions, constants)
2. **Vendor** (Composer dependencies)
3. **User** (project source)

This ordering means every layer's parent types are already
resolved before it starts. Vendor classes that extend `ArrayAccess`,
`Iterator`, `JsonSerializable`, etc. find pre-populated
`fqn_index` entries instead of triggering on-demand stub parses.
User classes that extend vendor classes find those already indexed
too.

With the current raw-PHP stubs, the stubs phase itself involves
parsing ~530 PHP files through the full mago pipeline. In a
pre-parsed format, this phase becomes a single deserialization
step (~5-10 ms), making the stubs layer essentially free and
letting vendor indexing start immediately.

### Cascade cost during first-file-open

When the user opens a file before background indexing completes,
the completion/hover path walks type chains synchronously. A
typical Laravel file triggers a cascade like:

- Model → `find_or_load_class` → classmap → parse vendor PHP
- Model implements `ArrayAccess`, `JsonSerializable`, `Countable`,
  uses `Traversable`, `Iterator`, `Stringable`, etc.
- Each of these hits Phase 3 (stub lookup) → full mago parse of
  the stub file containing it
- Stub files contain multiple classes, so parsing `SPL/SPL.php`
  for `ArrayAccess` also parses `Iterator`, `Countable`,
  `SeekableIterator`, etc.

A realistic first-open cascade triggers 20-40 stub file parses,
costing 40-200 ms of CPU time on the critical path. With
pre-parsed stubs, each stub lookup becomes a `HashMap::get`
returning an `Arc<ClassInfo>` in nanoseconds, eliminating this
cost entirely.

### Solution

Parse all stubs at build time in `build.rs` (mago becomes a build
dependency) and serialize the extracted `ClassInfo`, `FunctionInfo`,
and constant data into a compact binary blob using postcard (or
bincode). Embed the blob via `include_bytes!`. At startup,
deserialize the blob and populate `fqn_index` directly.

**Version filtering.** Add `since: Option<PhpVersion>` and
`until: Option<PhpVersion>` fields to `MethodInfo`, `ParameterInfo`,
`FunctionInfo`, `ClassInfo`, and `ConstantInfo`. Embed one
"maximal" blob containing all version variants. After
deserialization, filter elements whose version range excludes the
target PHP version. This replaces both the current byte-level
`@removed` scanning at startup and the `is_available_for_version`
AST filtering at parse time.

**Serde on the type hierarchy.** Add `#[derive(Serialize, Deserialize)]`
to the core structs (`ClassInfo`, `MethodInfo`, `PropertyInfo`,
`ConstantInfo`, `FunctionInfo`, `ParameterInfo`, and their
supporting enums). `SharedVec<T>` needs a custom serde impl that
serializes as `Vec<T>` and deserializes into `SharedVec::from(vec)`.

**What gets removed:**

- The `STUB_FILES` array (raw PHP source embedding)
- The `phpantom-stub://` URI scheme and associated `uri_classes_index` entries
- The `parse_and_cache_content_versioned` path for stubs
- The `is_stub_function_removed` / `is_stub_class_removed` byte
  scanners (replaced by version fields on deserialized structs)
- The `set_php_version` retain-based eviction (replaced by
  post-deserialize filtering)

**Go-to-definition.** Stubs are in-memory-only; the IDE cannot
navigate to them anyway. No raw source needs to be preserved.

**Hover.** The extracted fields (`class_docblock`, `deprecation_message`,
`links`, `see_refs`, parameter type hints and names) are all
carried in the serialized structs. Hover quality is preserved.

### Estimated impact

- **Binary:** −9.8 MB raw PHP, +2-3 MB serialized blob = net −7 MB
- **RSS:** 9.8 MB `.rodata` no longer mapped; stubs loaded as
  heap-allocated structs filtered to the target PHP version
- **First-file-open:** 40-200 ms of stub parse time on the
  critical path eliminated; stub lookups drop to nanoseconds
- **Background indexing:** stubs phase drops from seconds (parsing
  530 PHP files) to <10 ms (deserializing one blob), letting
  vendor indexing start immediately
- **Vendor indexing cascade:** every vendor class that extends a
  built-in type no longer triggers a stub parse; the parent
  `ClassInfo` is already in `fqn_index`
- **Build time:** clean builds gain 10-30 s for the mago parse
  step; incremental builds unaffected (`write_if_changed` caching)

### Prerequisites

- `serde` derive on the core type hierarchy (already in `Cargo.toml`)
- `build.rs` already downloads stubs and generates code; extending
  it to parse PHP is incremental
- Interacts with P15 (stub index `RwLock` elimination): if stubs
  are deserialized eagerly, the two-phase construction in P15
  becomes the natural approach

### When to implement

High priority. This is a prerequisite for efficient stubs → vendor
→ user indexing. The 9.8 MB static cost is already meaningful and
will become the dominant fixed overhead once vendor indexing is
deferred. Implementing this before full vendor indexing lands
avoids hitting the memory ceiling and ensures the stubs layer is
essentially free for both eager and deferred indexing paths.

---

## P17. `mago-names` resolution on the parse hot path

**Impact: Medium · Effort: Low**

The `mago-names` name resolver runs synchronously inside
`update_ast_inner`, adding a full AST walk plus an owned `HashMap`
copy on every `didChange` event. Measured regression from `6a0737a`
("Migrate to use mago-names"):

| Benchmark        | Before | After | Δ    |
| ---------------- | ------ | ----- | ---- |
| with_narrowing   | 12 ms  | 15 ms | +25% |
| 5_methods_chain  | 8 ms   | 10 ms | +25% |
| carbon_class     | 250 ms | 340 ms | +36% |
| large_file       | 150 ms | 210 ms | +40% |

The resolved names are currently consumed only by diagnostics (which
run asynchronously) and `FileContext::resolve_name_at()`. Nothing on
the completion hot path requires this data to be computed eagerly.

### Fix

Defer name resolution out of `update_ast_inner`. Options:

- **Lazy resolution:** compute `OwnedResolvedNames` on first access
  per file version, invalidate on the next `update_ast`. Moves the
  cost off the typing hot path entirely.
- **Diagnostic-worker resolution:** run the resolver in the
  diagnostic worker clone of `Backend`, since diagnostics are the
  primary consumer.

### When to implement

Low priority. The `mago-names` migration is complete, but the
`use_map` is still used by several consumers. Further refactoring
(migrating more consumers to byte-offset lookups, eventually
removing `use_map`) will change the access patterns. Optimizing
now would likely be reworked. Revisit once `use_map` usage is
significantly reduced.

---

## P18. Subtype result caching

**Impact: Medium · Effort: Low**

PHPStan caches subtype check results (`isSuperTypeOf()`) in a static
`HashMap` keyed by type description strings. This avoids redundant
class hierarchy walks when the same type pair is checked multiple
times during a single request. PHPantom resolves class hierarchies
repeatedly during completion (checking if a method override is
covariant, checking if a class implements an interface, etc.). A
per-request `HashMap<(String, String), bool>` cache for subtype
results would reduce redundant hierarchy walks.

PHPStan also uses a `hasTemplateOrLateResolvableType()` fast-path
to skip expensive type traversal when a type has no template
parameters. PHPantom could add a similar flag to its type
representations to short-circuit template substitution on simple
types. Most types in a typical codebase are concrete (no generics),
so this fast-path would apply to the majority of checks.

### Fix

1. Add a thread-local or per-request
   `HashMap<(String, String), bool>` that caches the result of
   "is type A a subtype of type B?" lookups. Clear the map at the
   start of each completion/hover/diagnostic request.

2. Add a `has_template_params: bool` flag (or equivalent) to
   `ClassInfo` or type representations. Set it during parsing when
   `@template` tags or generic syntax are present. Before running
   `apply_substitution`, check the flag and skip the substitution
   walk entirely when it is `false`.

3. Intern class name strings. PHPantom creates many copies of the
   same class name (e.g. `"Illuminate\\Database\\Eloquent\\Builder"`)
   across `ClassInfo`, type strings, and lookup keys. Mago already
   uses `Atom` (an interned string type) in its crates, and names
   flowing through `mago-names` / `mago-syntax` are already atoms.
   Using `Atom` or `Arc<str>` for class names in PHPantom's own
   data structures would reduce memory and make the subtype cache
   keys cheaper to hash and compare. Now that `PhpType` is the
   structured type representation throughout the codebase, interning
   the name strings inside each `PhpType` node (replacing owned
   `String` with `Atom` or `Arc<str>`) is a natural next step.

---

## Appendix: Profiling

### Commands

```sh
# Record (Ctrl-C after ~60s):
perf record -g --call-graph dwarf -- \
  ./target/release/phpantom_lsp analyze \
  src/core/Purchase/Services/PurchaseFileService.php

# Text report (top functions):
perf report --stdio --no-children | head -80

# Flamegraph (requires the `flamegraph` crate or perf-tools):
perf script | flamegraph > /tmp/phpantom.svg
```

### Pathological test file

`PurchaseFileService.php` (~700-line Eloquent-heavy service with
~55 imports) is the most expensive single file encountered so far.
The per-collector timing is controlled by a `>= 2s` threshold in
`src/analyse.rs` Phase 2 (search for `⏱`). It prints a breakdown
like:

```
⏱  63.2s  src/core/Purchase/Services/PurchaseFileService.php
  [fast=1ms cls=40ms mem=23696ms fn=12ms unres=16781ms arg=22568ms impl=0ms depr=54ms]
```

---

## P20. Content-hash gated resolution cache persistence

**Impact: Medium · Effort: Medium**

The resolved-class cache (`resolved_class_cache`) is ephemeral — it
lives only for the duration of the process. On LSP restart or cold
start, all class resolution (inheritance merging, virtual members,
template substitution) is re-computed from scratch even when files
haven't changed.

**Fix:** Persist resolved `ClassInfo` entries to a project-local cache
directory, keyed by `xxh128(file_contents)`. On startup, walk the
project, compare content hashes, and load cached entries for unchanged
files. Only re-resolve classes whose source files (or dependency files)
have changed.

Psalm implements exactly this pattern with three cache layers:
- Parser cache (serialized AST, keyed by file content hash)
- File storage cache (classes-in-file, functions, constants)
- ClassLike storage cache (methods, properties, template types,
  parent chains — keyed by `xxh128(file_contents)`)

Each layer checks the hash on load and discards stale entries. Schema
versioning (tracking `filemtime` of the storage struct source files)
auto-invalidates all caches when internal types change.

**Design:**

1. Use `bincode` serialization (already evaluated in X6) for
   `ClassInfo` entries.
2. Key: `(fqn, content_hash)` → serialized `ClassInfo`.
3. On startup: load cache entries where content hash matches current
   file. Skip resolution for those classes entirely.
4. On file change: evict entries for the changed file AND entries
   whose classes depend on changed members (using the existing
   dependency tracking from ER4).
5. Schema version: embed a version constant derived from `ClassInfo`
   struct layout. Invalidate entire cache on version mismatch.

**Relationship to X6:** X6 (disk cache) is the broader evaluation of
whether disk caching is worthwhile. P20 is the specific application
to resolved-class storage, which is the most expensive thing to
recompute. P20 can ship independently as a targeted optimization
even if the broader X6 evaluation concludes that full disk caching
isn't needed.

**References:**
- Psalm: `ClassLikeStorageCacheProvider` in
  `Psalm\Internal\Provider\ClassLikeStorageCacheProvider`
- Psalm: `FileStorageCacheProvider` for the content-hash invalidation
  pattern
- php-lsp: persists `FileIndex` entries under `~/.cache/php-lsp/`
  keyed by `blake3(uri || content)`. They originally keyed on
  `mtime + size` and shipped a cache-staleness bug (a size-preserving
  edit within the same mtime second was missed) before switching to
  content hashing. Confirms the content-hash-as-authority choice
  above; never trust mtime for correctness, at most as a cheap
  pre-filter to skip hashing unchanged files.
- php-lsp: `docs/salsa-migration.md` in its own repo (Phases K1-K4)
  documents their cache-size cap, reset-on-overflow, and
  LRU-by-mtime eviction plans — useful pitfall list if P20 ships.

---

## P21. Offset-shifting for cached diagnostics on partial edits

**Impact: Medium · Effort: Medium**

When a user edits one method in a file, PHPantom currently re-runs
diagnostics on the entire file. For large files (500+ lines), this
is wasteful — diagnostics in unchanged regions are still valid, just
at shifted byte offsets.

**Fix:** After a file edit, compute a line-level diff (Myers algorithm)
to produce byte-offset shift deltas. Apply the deltas to cached
diagnostics in unchanged regions. Only re-diagnose methods whose
byte ranges overlap with the edited region.

Psalm implements this with:
1. `FileDiffer` — Myers line-level diff producing byte-offset ranges
2. `FileStatementsDiffer` — AST-level statement diff classifying
   statements as keep/keep_signature/add_or_delete
3. `shiftFileOffsets()` — shifts surviving diagnostics/references by
   the offset delta, removes those in deleted ranges

**Design:**

1. On `didChange`, compute a line diff between old and new content.
2. Produce a `diff_map: Vec<(old_start, old_end, offset_delta)>`.
3. Walk cached diagnostics for this file:
   - If diagnostic span falls in a deleted range → remove it.
   - If diagnostic span is after the edit → shift by delta.
   - If diagnostic span is before the edit → keep as-is.
4. Re-run diagnostics only for methods/functions whose spans overlap
   with changed regions (use the member-level AST diff from ER4's
   incremental repopulation).
5. Merge shifted cached diagnostics with freshly-computed ones.

**Prerequisites:** The incremental repopulation (ER4) already
identifies which members changed. This task extends that to the
diagnostic layer.

**References:**
- Psalm: `FileDiffer` and `FileStatementsDiffer` in
  `Psalm\Internal\Diff`
- Psalm: `Analyzer::shiftFileOffsets()` for the offset-shifting logic

---

## P22. Signature change re-queues slow diagnostics for every open file

**Impact: Medium-High · Effort: Medium**

When `update_ast` detects that a class signature changed,
`schedule_diagnostics_for_open_files` (`src/diagnostics/mod.rs:935`,
called from `src/server.rs:589` and `:626`) queues **all** open
files (minus the edited one) for a full slow-diagnostic pass —
unknown classes, unknown members, argument checks. The per-file
cost of that pass is the most expensive thing the server does (see
the Appendix: tens of seconds on pathological files, hundreds of
ms on ordinary ones).

A user with 20 tabs open who adds a method to a class therefore
pays 20 full-file analysis passes per signature-changing edit
burst. Debouncing coalesces keystrokes, but the work is still
O(open files) regardless of whether those files reference the
edited class at all. During the burst the diagnostic worker
saturates blocking threads that completion/hover also need.

### Fix

Queue only files that can observe the change. The resolved-class
cache already maintains a dependency index for transitive
eviction (`evict_fqn`), and ER4 tracks which members changed.
Build a reverse map (FQN → open files that reference it) — either
from each file's `resolved_names` (which byte-offset FQN lookups
already exist for) or by recording, during each diagnostic pass,
which FQNs the pass touched. On signature change, queue only the
dependent files. Falls back to all-open-files when the dependency
data is missing (e.g. right after startup).

Synergy: P21 (offset-shifting) reduces the cost of re-diagnosing
the *edited* file; this item reduces the *count* of other files
re-diagnosed. Together they make the slow pass proportional to
the blast radius of an edit.

---

## P23. `workspace/symbol` allocates a lowercase copy of every symbol name per request

**Impact: Low-Medium · Effort: Low**

`match_tier` (`src/workspace_symbols.rs:64-72`) calls
`name.to_lowercase()` on every candidate symbol, and each symbol
is tested against up to two or three name forms (FQN, short name,
display name — call sites at lines 144, 232, 318, 401, 471). A
`workspace/symbol` request walks every class, method, property,
constant, and function in `uri_classes_index` and
`global_functions`, so each keystroke in the editor's symbol
picker performs O(total symbols × name length) heap allocations
just for case folding, then throws them away.

### Fix

Match case-insensitively without allocating: a byte-wise
`eq_ignore_ascii_case`-style prefix/substring scan (PHP
identifiers are ASCII; a non-ASCII fallback can keep the old
path), or store a pre-lowercased name alongside each symbol if
the tiering logic needs real substring search. Note B25
(case-insensitive index keys) will make lowercased names
available on the index side anyway — implementing that first
makes this nearly free.

Related: full background indexing already populates a dedicated
workspace-symbol index; this fix is independent and worth taking
regardless since it is a few lines.

---

## P25. `type_mismatch_argument` / `argument_count_mismatch` slow on large single files

**Impact: Medium · Effort: Medium**

On very large PHP files with thousands of call sites, the
`type_mismatch_argument` and `argument_count_mismatch` diagnostic
passes dominate the per-file time. On PDepend's
`src/Source/Language/PHP/AbstractPHPParser.php` a single file spent
~11.7s in `type_mismatch_argument` and ~2.9s in
`argument_count_mismatch`:

```
⚠ slow file (15.6s): src/Source/Language/PHP/AbstractPHPParser.php
  scope=1.9s, fast=0.0s, unknown_class=0.0s, unknown_member=0.1s,
  unknown_function=0.0s, argument_count_mismatch=2.9s,
  type_mismatch_argument=11.6s, deprecated_usage=0.1s,
  unknown_variable=0.8s, invalid_class_kind=0.0s
```

Both passes walk every call site and call
`resolve_callable_target_with_args` / `resolve_callable_target`
per site. Variable-based calls and calls with argument text are
resolved fresh at every site (only zero-arg static/function calls
are cached), so a file with thousands of `$obj->method($args)`
call sites re-resolves the receiver type once per site. Look at
memoizing receiver-type resolution per (expression, offset) within
a file, or sharing the forward-walk scope snapshot across both
passes so the receiver type is computed once.

---

## P27. `object`/`?object` call-return check re-resolves the subject a second time

**Impact: Medium · Effort: Low**

`resolve_subject_outcome` resolves the whole subject up front (the
`resolve_target_classes(subject, …)` call at the top). When that
yields no concrete class, the `object`/`?object` escape-hatch branch
then calls `resolve_call_raw_return_type(callee, "", ctx)` purely to
peek at `is_object()`, and that helper re-resolves the callee's base
chain from scratch. So every method/function call whose result has
no concrete class pays a second full return-type resolution.

This is exactly the shape of the `diagnostics/fixture/lots_of_missing_methods`
benchmark (many calls whose results have missing members), and it
regressed that fixture by ~29% (54ms → 70ms) when the branch was
added, with a matching drift on hover and go-to-definition, which
share this path.

### Fix

Fold the `object` detection into the primary resolution pass instead
of running a separate re-resolution: when `resolve_target_classes`
is about to return empty for a call subject, check whether the raw
return type was `object`/`?object` at that point (the base chain is
already resolved there) and emit the synthetic `stdClass` from the
same pass. Do not pass `""` for the argument text on a second call;
reuse the resolution already performed.

---

## P28. `process_assert_narrowing` clones the top-level scope once per variable for every statement

**Impact: Low-Medium · Effort: Low**

The forward walker's assert-narrowing step runs for every statement
and clones `ctx.top_level_scope` once per variable currently in
scope, even when the statement is not a call expression and can
therefore never be an `assert()` / custom type-guard call. On
methods with many locals and many statements the clones add up to a
measurable share of the walk.

Add an early-out that skips the per-variable loop when the statement
cannot be an assertion (not a call expression). This was confirmed
as an independently worthwhile micro-fix while investigating the
PclZip return-type-inference hang, but was reverted at the time
because it did not address that hang's dominant cost (uncached
body-return inference, since fixed by memoization).

---

## P29. Migrate to `mago-phpdoc-syntax`

**Impact: Medium · Effort: Medium**

As of the 1.42/1.43 mago releases, `mago-docblock` and
`mago-type-syntax` are both deprecated and frozen at 1.42.0. Upstream
folded them into a single unified crate, `mago-phpdoc-syntax`, which
parses docblock comments and their embedded type and constant
expressions in one pass and is meant to replace both. The deprecation
notices point at `mago_phpdoc_syntax::PHPDocParser` (for
`parse_phpdoc_with_span`) and `mago_phpdoc_syntax::parse_type` (for
`parse_str`).

We currently call the deprecated functions at four sites, each wrapped
in `#[allow(deprecated)]`:

- `src/docblock/parser.rs` — `mago_docblock::parse_phpdoc_with_span`
- `src/php_type.rs` — `mago_type_syntax::parse_str` (type-string parse
  and the round-trip test helper)
- `src/symbol_map/docblock.rs` — `mago_type_syntax::parse_str`

`mago-phpdoc-syntax` first shipped at 1.44.0, so this migration is
coupled to bumping the rest of the mago toolchain from 1.43.0 to
1.44.0 (mixing 1.43 and 1.44 would pull two incompatible copies of the
shared `mago-span` types). Do the version bump and the API migration
together, drop `mago-docblock` and `mago-type-syntax` from
`Cargo.toml`, and remove the `#[allow(deprecated)]` wrappers. The
unified single-pass parser should also be a small win on the docblock
parse hot path.

---

## P30. Evaluate migrating parse/resolve/docblock pipeline to `mago-hir`

**Impact: Medium-High · Effort: High**

Mago 1.44.0 introduces `mago-hir`, a new intermediate representation
that lowers the CST plus PHPDoc comments into a single flat,
fully-resolved tree in one pass: names are resolved
(`Local`/`Qualified`/`FullyQualified` + `imported` flag on every
identifier), docblock tags are parsed into structured annotations
(`@template`, `@extends`/`@implements` generics, `@mixin`,
`@method`/`@property`, `@param`/`@return`/`@throws`,
`@assert`/`@assert-if-true`/`@assert-if-false`, `@param-out`,
`@self-out`, type aliases), and types are parsed into a full
PHPStan/Psalm-grade type language (generics with resolved bounds,
conditional types, `key-of`/`value-of`, array/object shapes,
int-ranges, class-string variants, int-masks). This is exactly the
layer PHPantom currently hand-rolls across `parser/`, `names.rs`,
`docblock/`, and parts of `php_type.rs`.

The IR threads three generic "hole" parameters
(`IR<'arena, I, S, E>`, defaulting to `()`) through every node so a
later inference pass can fill in resolved type information at
item/statement/expression granularity without changing the tree
shape — this is the "groundwork for the upcoming rule-based checker"
azjezz described.

Confirmed by reading the 1.44.0 source directly (docs.rs is only
~1% documented, so don't rely on it): `mago-hir` depends only on
crates we already use (`mago-syntax`, `mago-syntax-core`,
`mago-phpdoc-syntax`, `mago-span`, `mago-database`,
`mago-allocator`) plus the small `mago-flags` crate. It does **not**
pull in `mago-codex`, `mago-analyzer`, or `mago-reflection`, so
adopting it would not introduce a second type-resolution engine
alongside our own (see the "no parallel type resolution systems"
rule in `CLAUDE.md`) — it would replace the raw-parsing layer that
currently *feeds* `ClassInfo` construction, not `ClassInfo` itself or
`resolve_rhs_expression`/`resolve_expression_type`.

Potential payoff if it holds up:

- Deletes a large share of our hand-rolled docblock tag parsing,
  type-string parsing, and name resolution, replacing it with a
  single upstream-maintained pass.
- A natural path to Blade support: lowering Blade's compiled-PHP
  approximation (see `examples/laravel`/Blade handling) to the same
  IR would let more code actions and diagnostics work uniformly on
  Blade files instead of only a subset, as flagged in the Discord
  discussion with azjezz.

**Do not start this now.** The crate is brand new (shipped in
1.44.0, not the 1.43.0 this project is currently on), ~19k LOC,
under active redesign ("final touches... in the next branch" per
azjezz), and its public API should be expected to move.

**Triggers to revisit — start only once at least two of these hold:**

- `mago-hir` has shipped unchanged (no breaking API changes) across
  at least 2-3 minor mago releases, indicating the API has settled.
- Upstream's own rule-based checker/analyzer ships on top of
  `mago-hir` and is in real use, proving the IR's "holes" mechanism
  works end-to-end for type inference, not just as a parse target.
- rustdoc coverage for `mago-hir` is substantially more complete
  (the 1.44.0 release is ~1% documented), or azjezz confirms the
  shape is stable enough to build against.

**Before committing to a full migration, prototype first:** feed
`symbol_map` extraction (or `ClassInfo` construction) for a single
file from `IR` behind a flag, on a branch, and compare output against
the current extraction plus wall-clock time. Only proceed to a
broader migration if the prototype reproduces current behavior and
shows a real win; otherwise record the findings here and keep
waiting on the triggers above.

---

# Remaining anti-pattern fixes

Most remaining depth-cap issues are addressed by ER5 (class
resolution). The forward walker loop iteration was addressed by the
assignment-depth-bounded strategy. The items below are independent
fixes that do not depend on either.
