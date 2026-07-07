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

## P5. `memmap2` for file reads during scanning

**Impact: Low-Medium · Effort: Low**

All file-scanning paths (`scan_files_parallel_classes`,
`scan_files_parallel_psr4`, `scan_files_parallel_full`, and the
`find_implementors` pre-filter) use `std::fs::read(path)` which
copies the entire file into a heap-allocated `Vec<u8>`. When the OS
page cache already has the file mapped, `memmap2` can provide a
read-only view of the file's pages without any copy.

### Fix

Add `memmap2` as a dependency. In the parallel scan helpers, replace
`std::fs::read(path)` with `unsafe { Mmap::map(&file) }`. The
`find_classes` and `find_symbols` scanners already accept `&[u8]`,
so the change is confined to the call sites.

### Safety

Memory-mapped reads are `unsafe` because another process could
truncate the file while the map is live, causing a SIGBUS. In
practice this does not happen during LSP initialization (the user is
not deleting PHP files while the editor starts). A fallback to
`fs::read` on map failure handles edge cases.

### When to implement

Profile first. On Linux with a warm page cache the difference
between `read` and `mmap` is small for files under ~100 KB (which
covers most PHP files). The benefit is more pronounced on macOS
where `read` involves an extra kernel-to-userspace copy. If
profiling shows that file I/O is no longer the bottleneck after
parallelisation, this item can be dropped.

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
  `references/psalm/src/Psalm/Internal/Provider/ClassLikeStorageCacheProvider.php`
- Psalm: `FileStorageCacheProvider` for the content-hash invalidation
  pattern

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
  `references/psalm/src/Psalm/Internal/Diff/`
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

Related: X4 (full background indexing) plans a dedicated
workspace-symbol index; this fix is independent and worth taking
early since it is a few lines.

---

## P24. Per-file maps that survive `did_close` grow for the whole session

**Impact: Low · Effort: Low**

Two session-lifetime leaks found while auditing map hygiene:

1. **`parse_errors` is never pruned.** `clear_file_maps`
   (`src/util.rs:1757-1772`) removes `uri_classes_index`,
   `symbol_maps`, `file_imports`, `resolved_names`, and
   `file_namespaces`, but not `parse_errors`, and no other path
   removes entries either (`did_close` and `reindex_files_batch`
   both delegate to `clear_file_maps`). Every file ever opened
   (or deleted from disk) keeps its last parse-error vector in
   memory until restart.

2. **`member_completion_cache` has no size bound.** It is cleared
   wholesale on signature-changing edits
   (`src/parser/ast_update.rs:735`) and on watched-file changes,
   but during read-heavy browsing (no edits) it accumulates one
   `Vec<CompletionItem>` per distinct completion target — for
   Eloquent models those vectors contain hundreds of fully-built
   items each.

### Fix

Add `self.parse_errors.write().remove(uri)` to `clear_file_maps`.
For the completion cache, a simple cap (e.g. clear when the map
exceeds N entries) is enough — the cache exists to serve
keystroke bursts on one target, so losing cold entries is free.
While in there, audit the `diag_last_*` / `diag_result_ids` /
external-tool diagnostic caches for the same keep-after-close
pattern (they hold per-file diagnostic vectors).

---

# Remaining anti-pattern fixes

Most remaining depth-cap issues are addressed by ER5 (class
resolution). The forward walker loop iteration was addressed by the
assignment-depth-bounded strategy. The items below are independent
fixes that do not depend on either.
