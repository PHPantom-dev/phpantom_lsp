# PHPantom — Performance

Internal performance improvements that reduce latency, memory usage,
and lock contention on the hot paths. These items are sequenced so
that structural fixes land before features that would amplify the
underlying costs (parallel file processing, full background indexing).

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## P3. Parallel pre-filter in `find_implementors`

**Impact: Medium · Complexity: Medium-High**

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

Note that once the full workspace index is ready,
`find_implementors` answers from Phase 1 alone — Phases 3 and 5
only run during the startup window before indexing completes, which
narrows how often this cost is paid.

### Trade-off

Thread spawning overhead is only worthwhile when the candidate set
is large. Skip parallelism when the candidate count is below a
threshold (e.g. 8 files).

---

## P15. Two-phase stub index construction (eliminate `RwLock` on stub maps)

**Impact: Low · Complexity: Medium-High**

The three stub indexes (`stub_index`, `stub_function_index`,
`stub_constant_index`) are write-once-read-many maps. They are
populated at construction time from the compiled-in phpstorm-stubs
arrays, then filtered once in `set_php_version` (called during
`initialized`) to evict entries with `@removed X.Y` tags. After
that single mutation they are never written again.

Because the PHP version is not known at construction time (it comes
from `composer.json` / `.phpantom.toml`, read during `initialized`),
the maps are currently wrapped in `parking_lot::RwLock` so that
`set_php_version` can call `.write().retain(…)`. The maps are now
`Arc<RwLock<…>>`, shared across worker and request clones instead
of deep-copied per thread, so only the read-lock cost remains.
Every read — ~24 call sites across completion, resolution,
diagnostics, hover, and definition — acquires a shared read lock. On the
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

**Impact: High · Complexity: Very High**

The ~530 phpstorm-stubs PHP files are embedded as raw source via
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

**Impact: Medium · Complexity: High**

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

The resolved names are now consumed by many features through
`resolve_name_at()` (rename, references, semantic tokens,
go-to-definition, type hierarchy, highlight) and directly by
function resolution and deprecated diagnostics — but all of those
are on-demand request handlers. Nothing on the didChange or
completion hot path requires this data to be computed eagerly, so
lazy per-file-version resolution remains viable; it just has more
consumers to invalidate correctly than when this was filed.

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

**Impact: Medium · Complexity: High**

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
   `HashMap<(Atom, Atom), bool>` that caches the result of
   "is type A a subtype of type B?" lookups. Clear the map at the
   start of each completion/hover/diagnostic request. Class names
   are interned now (`TypeKind::Named` carries an `Atom`, `ClassInfo`
   caches its FQN as an `Atom`), so the keys are `Copy` and
   identity-hashed. Whole types are interned too, so a `(PhpType,
   PhpType)` key would work as well and hash just as cheaply.

2. Add a `has_template_params: bool` flag (or equivalent) to
   `ClassInfo` or type representations. Set it during parsing when
   `@template` tags or generic syntax are present. Before running
   `apply_substitution`, check the flag and skip the substitution
   walk entirely when it is `false`. (Today the equivalent guarding
   is ad hoc emptiness checks on `template_params` and on the
   substitution map.)

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

# Instantaneous CPU utilisation over a run (% of one core, sampled
# every 0.5 s), for machines where perf is unavailable
# (kernel.perf_event_paranoid > 2):
./target/release/phpantom_lsp analyze --project-root <dir> --no-colour \
  >/dev/null 2>&1 & PID=$!; prev=0
while kill -0 $PID 2>/dev/null; do
  cur=$(awk '{print $14+$15}' /proc/$PID/stat 2>/dev/null) || break
  [ -n "$cur" ] && echo $(( (cur - prev) * 2 )); prev=$cur; sleep 0.5
done
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

**Impact: Medium · Complexity: Very High**

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
- A peer PHP LSP project persists its per-file index cache on disk
  keyed by `blake3(uri || content)`. It originally keyed on
  `mtime + size` and shipped a cache-staleness bug (a size-preserving
  edit within the same mtime second was missed) before switching to
  content hashing. Confirms the content-hash-as-authority choice
  above; never trust mtime for correctness, at most as a cheap
  pre-filter to skip hashing unchanged files.

---

## P21. Offset-shifting for cached diagnostics on partial edits

**Impact: Medium · Complexity: Very High**

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

## P30. Evaluate migrating parse/resolve/docblock pipeline to `mago-hir`

**Impact: Medium-High · Complexity: Very High**

`mago-hir` is an intermediate representation that lowers the CST plus
PHPDoc comments into a single flat,
fully-resolved tree in one pass: names are resolved
(`Local`/`Qualified`/`FullyQualified` + `imported` flag on every
identifier), docblock tags are parsed into structured annotations
(`@template`, `@extends`/`@implements` generics, `@mixin`,
`@method`/`@property`, `@param`/`@return`/`@throws`,
`@assert`/`@assert-if-true`/`@assert-if-false`, `@param-out`,
`@self-out`, type aliases), and types are parsed into a full
PHPStan/Psalm-grade type language (generics with resolved bounds,
conditional types, `key-of`/`value-of`, array/object shapes,
int-ranges, class-string variants, int-masks). `mago-phpdoc-syntax`
already supplies the docblock and type half of that; what `mago-hir`
adds on top is the resolved-name and single-tree lowering PHPantom
hand-rolls across `parser/` and `names.rs`.

The IR threads three generic "hole" parameters
(`IR<'arena, I, S, E>`, defaulting to `()`) through every node so a
later inference pass can fill in resolved type information at
item/statement/expression granularity without changing the tree
shape — this is the "groundwork for the upcoming rule-based checker"
azjezz described.

Confirmed by reading the source directly (docs.rs is only
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

**Do not start this now.** The crate is ~19k LOC of essentially
undocumented API with no consumers, and was described as under active
redesign ("final touches... in the next branch" per azjezz).

Note for anyone re-reading the history here: `mago-hir` did **not**
arrive in 1.44.0. It first shipped in 1.40.0 (2026-06-24) and had
already reached its current size by 1.43.0. The "brand new crate"
framing in the original writeup was wrong, which matters because it
means the API-settling clock below started earlier than assumed.

**Triggers to revisit — start only once at least two of these hold:**

- `mago-hir` has shipped unchanged (no breaking API changes) across
  at least 2-3 minor mago releases, indicating the API has settled.
- Upstream's own rule-based checker/analyzer ships on top of
  `mago-hir` and is in real use, proving the IR's "holes" mechanism
  works end-to-end for type inference, not just as a parse target.
- rustdoc coverage for `mago-hir` is substantially more complete
  (the 1.44.0 release is ~1% documented), or azjezz confirms the
  shape is stable enough to build against.

**Re-evaluated at mago 1.45.0 (2026-07-29): one of three triggers
holds, so this stays parked and the prototype below was not run.**

- *API settled — technically yes, but for the wrong reason.* The
  `mago-hir` sources are byte-identical across 1.43.0, 1.44.0 and
  1.45.0 (verified by unpacking the crates and diffing: zero changed
  lines). That is not an API that settled through use, it is one that
  has seen no development at all.
- *Upstream checker builds on it — no, and upstream picked something
  else.* `mago-analyzer`, `mago-linter` and `mago-codex` at 1.45.0
  all depend on `mago-syntax` plus `mago-phpdoc-syntax` directly, and
  none of them depends on `mago-hir`. `mago-hir` has zero reverse
  dependencies on crates.io. Six releases and a month after it
  appeared, the rule-based checker it was billed as groundwork for is
  being built on a different foundation. This is the load-bearing
  trigger: adopting the IR now would make PHPantom its first and only
  user, betting the parse layer on an inference "holes" mechanism that
  nothing has exercised end to end.
- *rustdoc coverage — no.* 84 doc-comment lines against 876 `pub`
  items in 1.45.0, still about the ~1% the original writeup found.

The cheap way to re-check is the second trigger on its own: if
anything in mago's own workspace starts depending on `mago-hir`, or
its crates.io reverse-dependency count moves off zero, that flips the
one trigger that carries real evidence and this becomes worth another
look.

**Re-checked at mago 1.46.0 (2026-08-13): unchanged.** `mago-hir` still
has zero reverse dependencies on crates.io, and docs.rs coverage is
still around 1%. Still parked.

**Before committing to a full migration, prototype first:** feed
`symbol_map` extraction (or `ClassInfo` construction) for a single
file from `IR` behind a flag, on a branch, and compare output against
the current extraction plus wall-clock time. Only proceed to a
broader migration if the prototype reproduces current behavior and
shows a real win; otherwise record the findings here and keep
waiting on the triggers above.

---

## P47. The resolved-class cache lock caps concurrent class resolution

**Impact: Medium · Complexity: Very High**

`ResolvedClassCache` is a single `RwLock<ResolvedCacheInner>`, and every
resolution takes its write lock several times: twice per
`resolve_class_fully_inner` call to mark and clear the cycle-break
in-flight marker, once to insert the finished class, and once per
transformed member that `intern_transformed_method` /
`intern_transformed_property` has to build (the read side of interning
takes the read lock even on a hit). Member interning dominates the
volume by far, since a merge produces one lookup per inherited or
synthesized member.

Measured with the eager-population worker pool
(`populate_from_sorted`) swept from 1 to 32 workers on a 16-core / 32-thread
SMT machine (Ryzen 9 5950X), release build, large Laravel projects: wall
time falls steeply to ~8 workers, flattens through 16, and then
*regresses* past that. On one project 2.1k classes went 0.62 s
(1 worker) → 0.16 s (8) → 0.27 s (32), i.e. 32 workers were worse than
4. Duplicated resolution is not the cause: the count of full resolutions
rises only 5.9 % from 1 to 32 workers.

Two confounds on this hardware make the raw sweep numbers hard to read
at face value. The 16→32 leg is SMT: past 16 threads two workers share
a physical core's execution resources rather than getting one each,
which degrades throughput on its own and independently amplifies any
lock contention (a spinning waiter now trashes the lock-holder's cache
lines on the same core instead of a separate one). More importantly,
the 5950X is dual-CCD — cores 0-7 and 8-15 (and their SMT siblings
16-23/24-31) sit behind two separate L3 caches (`lscpu -e=CPU,CORE,CACHE`
shows the split), and cache-line traffic for anything shared, including
this lock, crosses the Infinity Fabric between them at much higher
latency than a same-CCD hop. An unpinned process asking for ≤8 threads
tends to get packed onto one CCD by the scheduler; past 8 it necessarily
spills onto the second. Confirmed directly with `taskset`: 8 workers
pinned to `0-7` (one CCD, no SMT) averaged 1.79 s wall for the same
whole-project run that split 4+4 across both CCDs (`0,1,2,3,8,9,10,11`,
still 8 distinct physical cores, still no SMT) averaged 2.02-2.17 s —
15-20 % slower from CCD-crossing alone, with identical core count and
no hyperthreading involved. So the knee at 8 in the original unpinned
sweep is at least partly a topology artifact of this machine, not
solely "how much lock contention exists at that many workers" — the
same experiment on a single-CCD or single-die machine would likely
plateau at a different worker count. `MAX_POPULATE_WORKERS` is pinned
to 8 regardless: it is the largest value that stayed reliably within
one CCD's worth of cores in testing, so it also happens to dodge the
cross-CCD lock-traffic penalty as a side effect, not just diminishing
per-lock-acquisition returns.

Implication for the fix: fixing the lock removes the *within-CCD*
contention this section measured, and should let population usefully
raise `MAX_POPULATE_WORKERS` above 8. But raising it past one CCD's
core count re-introduces cross-CCD traffic for whatever of the lock
(sharded or not) is still shared — a plain `Vec<Mutex<Shard>>` does not
avoid that on its own. Re-run the `taskset` same-CCD-vs-split comparison
above after any lock-splitting change before raising the cap past 8, to
check how much of the remaining ceiling is the lock versus the CCD
boundary.

The same lock is on the diagnostic pass's hot path (see P35), so
splitting it should also help there. Directions, cheapest first:

1. **Take the in-flight set off the shared lock.** It is already keyed
   `(ThreadId, FQN)` purely to emulate a thread-local, so making it an
   actual thread-local `HashSet<Atom>` removes two write locks per
   resolution with no semantic change. Measured on its own this did
   *not* move the sweep, so it is a prerequisite rather than the fix,
   but it is nearly free.
2. **Shard the interning tables.** `substituted_methods` /
   `substituted_properties` are keyed by origin pointer and a
   fingerprint hash, so they shard cleanly (e.g. by low bits of the
   key) into independent locks without changing what gets shared. This
   is where the volume is.
3. **Separate the interning tables from the class map entirely.** The
   two have different access patterns (interning is
   write-heavy-then-read-heavy per merge; the class map is one insert
   per class) and only share a lock for convenience. Note the member
   sharing they provide is load-bearing for memory, so any change must
   keep cross-class sharing intact rather than falling back to
   per-thread tables.

Before implementing, confirm the attribution by counting lock
acquisitions per site during a population run: the sweep above proves
*a* lock is the ceiling but not which acquirer dominates.

---

## P35. Diagnostic passes reach only a fraction of available cores

**Impact: Medium-High · Complexity: Very High**

Measured on a 32-core machine against large Laravel projects (release
build): the `analyze` Phase 2 diagnostic pass spawns one worker per
core with atomic work-stealing but does not keep them busy. Three
offenders are fixed. The original one was every worker deep-cloning
the embedded stub class/function/constant indexes twice per file via
`clone_for_diagnostic_worker`; the indexes are Arc-shared now. The
second was the Laravel string-key enumerations
(`cached_route_names`/`cached_config_keys`/`cached_view_names`/
`cached_trans_keys`/`cached_config_trees`): each walks the workspace
from disk, and the plain check-then-fill cache stampeded, so all 32
workers missed the same empty slot at once and each repeated the same
gitignore-aware walk. They are guarded by
`LaravelStringKeyBuildLocks` now, which took Phase 2 on a large
Laravel project from 11.6 to 22.6 of 32 cores (1.91 s → 1.31 s) and
whole-run wall clock down ~15%.

The third was the name → class loader itself. `find_or_load_class_typed`
was called 3.68 M times in a 1.2 s Phase 2 over a few thousand distinct
types, and every call hashed the name case-insensitively for a read
lock on `class_not_found_cache` and another on `fqn_class_index`. That
cluster (`find_or_load_class_typed`, `find_class_in_uri_classes_index`,
`CiMap::get`, `sip::Hasher::write`, `RawRwLock::lock_shared_slow`,
kernel `osq_lock`) was ~22% of Phase 2 samples, and both lock symbols
have since dropped out of the profile entirely. `class_loader_memo`
memoises the loader per worker on the interned `PhpType` handle, keyed
by `SymbolIndex::id` and stamped with `class_lookup_generation` so an
answer is never staler than the two caches it derives from. Phase 2 fell
~25-32% (1.23 s → 0.93 s and 0.72 s → 0.49 s on the two largest Laravel
projects benchmarked) at ~26.8 → ~28.6 of 32 cores, with whole-run wall
clock down 8-12% and user CPU down 17-34% across three large Laravel
projects. Every smaller project benchmarked improved as well, RSS did
not move, and diagnostic output was byte-identical on all ten.

Sampling `/proc/<pid>/task/*/stat` is the fastest way to see the
remaining ceiling: workers in `S` rather than `R` are blocked, not
computing. What is left, re-profiled after the memo:

- Type strings are still parsed during the diagnostic pass rather than
  at index time: `TypeTokenStream::fill_buffer_slow`, `PhpType::parse`,
  `LocalArena::alloc_slice_copy` and `parse_primary_type` together are
  ~6.5% of Phase 2 samples. Class-level `@method` / `@property` tags
  are now parsed at extraction time; the remaining cost is method,
  parameter and return type strings.
- malloc/free/memmove remains diffuse. `is_scalar_name` and
  `is_keyword_type` (~2.3% between them) allocate a lowercase `String`
  per call and are reached from `base_name`, the subtype checks and the
  narrowing paths; the ~150 other `to_ascii_lowercase()` calls in
  `php_type/` do the same. A stack buffer plus a length pre-filter
  would make them allocation-free, but see the reverted attempt below
  before assuming that wins.
- The `PhpType` interner (`php_type::intern` + `intern::lookup`) is
  ~4.6% of Phase 2 samples and its 64 shards are now the largest
  remaining lock, though the memo removed enough traffic that
  `lock_shared_slow` no longer registers at all. An earlier count put
  the interner at ~13 M hits per pass with roughly a quarter falling
  through the shard read lock to the write path; that has not been
  re-counted since. A per-thread direct-mapped memo in front of the
  shard read (the same shape as `class_loader_memo`) is the obvious
  next attempt.
- `ensure_workspace_indexed_with_progress` still re-walks the whole
  workspace on every call, so the four surviving string-key
  enumerations do four full walks per diagnostic pass (down from ~44).
  The walk is deliberate — it is how PHP files created outside the
  editor get discovered — so removing it needs a change to that
  contract, not just another cache.
- `class_loader_memo`'s own hit rate is bounded by how often
  `note_class_lookup_change` fires: 1,169 times in Phase 2, once per
  lazily parsed vendor file, each retiring every worker's table. A
  build with invalidation removed (unsound, for sizing only) reached
  0.80 s against the 0.93 s shipped, so ~14% of the memo's prize is
  still on the table. Recovering it means either loading fewer vendor
  classes lazily (the reverted experiment below, whose calculus this
  changes) or splitting the generation so an additive insert only
  retires negative answers. The latter is not sound as stated: a
  positive answer reached through PSR-4 or a stub is matched by short
  name, so it is not always backed by an `fqn_class_index` entry under
  the name that was looked up, and a later first-time insert of that
  exact name can change it.

The LSP workspace diagnostics pass uses the same collectors and has the
same ceiling. Re-measure with `perf` (frame-pointer build) or the
CPU-sampling loop in the Appendix after any change.

**Tried and reverted (vendor classes in eager population):** an
earlier revision of this item claimed that projects keeping
substantial code in `vendor/` pay extra because eager population only
walks the indexed user files, leaving every vendor class to be parsed
and resolved lazily mid-diagnostic — serialising workers on the load
path, with cycle-break re-merges that dependency-first ordering would
have avoided. Seeding eager population with vendor classes was
implemented in two escalating variants and benchmarked (10-run,
order-swapped wall/user-CPU averages against the two largest Laravel
projects benchmarked): (1) expanding the toposort input with the
transitive inheritance closure of the user classes (parents, traits,
interfaces, mixins, generic arguments, loaded via
`find_or_load_class`), and (2) additionally seeding every `use`-import
target found in `fqn_uri_index`, with parallel frontier loading and
Kahn-levelled parallel resolution to keep Phase 1.5 off the critical
path. Neither moved wall clock on any project. Variant 1 cut lazy
Phase 2 resolutions only ~2% (950 → 931) because vendor ancestors
were already being resolved as nested resolutions during eager
population; variant 2 cut them to a third (931 → 276) but cost ~3%
more user CPU (duplicated nested provider resolutions across level
workers) and ~10 MB RSS for classes the pass never needed resolved.
The predicted cycle-break re-merges also failed to reproduce: Phase 2
hits 0 on one of them and 18 on the other, and all
18 are genuine dependency cycles (`Schedule` ↔
`PendingEventAttributes`, Spatie `Role` ↔ `Permission`) that
dependency-first ordering cannot avoid — the toposort has to break
them somewhere too. Conclusion: after the stampede fix, lazy vendor
class loading no longer serialises the diagnostic pass measurably;
the remaining ceiling is the clone/interner traffic above. Diagnostic
output was byte-identical in every configuration.

**Tried and reverted:** a single `perf` snapshot attributed ~4% of
Phase 2 time to `core::hash::sip::Hasher::write`, called from
`CiMap`/`CiSet` (`fqn_class_index`, `class_not_found_cache`) inside
`find_or_load_class` — the single hottest function in the profile.
Two independent fixes were tried: swapping `CiMap`/`CiSet`'s `HashMap`
from `std`'s default SipHash to a hand-rolled FxHash-style hasher, and
avoiding `fold()`'s per-lookup heap allocation with a stack buffer.
Both looked sound in isolation and passed all tests, but 10-run
wall-clock/user-CPU averages against a large Laravel project (order-swapped
to rule out warm-cache bias) showed a small, consistent *regression*
(~2% more user CPU) for the hasher swap alone, the allocation-avoidance
alone, and the two combined. Likely cause: `mimalloc` already makes
these transient allocations cheap, and the hand-rolled hasher's
sequential dependency chain (`rotate_left` → `xor` → `wrapping_mul` per
word) didn't beat `std`'s SipHash13 for these short keys on this
hardware. Lesson for next attempt: a single `perf --stdio` percentage
is not sufficient evidence — confirm with repeated, order-controlled
wall-clock measurement before committing a "hot function" fix; sampling
noise and inlining attribution can point at the wrong function. The
re-measurement did surface a more promising lead: `RawRwLock::lock_shared_slow`
rose from 3.78% to 4.36% of samples once the hashing/allocation cost
was removed, suggesting the read lock on `fqn_class_index` (contended
by 32 workers doing `find_or_load_class` concurrently) is closer to the
real ceiling than the hashing was.

---

## P48. Higher-order collection proxy injection repeats work

**Impact: Low · Complexity: Medium**

Grafting the item type's members onto a
`HigherOrderCollectionProxy<TKey, TValue, 'method', Collection>` runs
inside `resolve_class_fully_with_generics`, on the generic-substitution
path. Two avoidable costs sit there. Neither is a correctness problem
and both are linear rather than exponential, which is why they were left
when the feature landed; the outer `(FQN, generic_args)` cache absorbs
most of the repetition.

1. **The value type is resolved twice.** The framework annotates the
   proxy `@mixin \Illuminate\Support\Enumerable<TKey, TValue>` *and*
   `@mixin TValue`. The second is a template-parameter mixin, so the
   "Template-param mixin resolution" block in
   `type_engine/types/resolution.rs` resolves the value class and merges
   its members after `inject_higher_order_proxy_members` has already
   grafted the same members with their proxied types. The injected
   members win (`merge_virtual_members` keeps whichever arrived first),
   so the second pass is pure waste. Skipping the template-param mixin
   for a tagged proxy, or letting the injection mark `TValue` as already
   consumed, avoids it.

2. **Grafted members are not interned.** `inject_higher_order_proxy_members`
   builds each `MethodInfo`/`PropertyInfo` directly, where every other
   transform site in the codebase goes through
   `intern_transformed_method` / `intern_transformed_property` so that
   applying the same transform to the same origin shares one `Arc`. The
   proxied members of one item type are byte-identical across every
   proxy that wraps it with the same result shape, so the same model's
   members are re-allocated once per distinct `(proxied method, owning
   collection)` pair rather than shared.

**Where to look:** `virtual_members/laravel/higher_order_proxy.rs`,
`virtual_members/resolve.rs`, and the template-param mixin block in
`type_engine/types/resolution.rs`.

## P49. A very long method chain costs superlinear time to analyse

**Impact: Low · Complexity: Medium**

Resolving a receiver spine no longer recurses per link, so a fluent chain
of any length parses, hovers, and analyses without overflowing the stack.
The work is still superlinear in the chain's length, though: a 1000-link
chain takes roughly eight times as long to run diagnostics over as a
500-link one on a debug build, so a generated query builder or generated
API client long enough turns a diagnostic pass into a multi-second stall.

Two costs compound along the spine, each linear in the prefix and paid
once per link:

1. **Subject text is rebuilt per link.** `extract_call_expr` calls
   `expr_to_subject_text(method_call.object)` for every link, which
   renders the whole prefix, so the symbol map spends O(n²) bytes on one
   chain. Rendering the spine once and handing each link a slice of the
   result would make it linear.

2. **Chain cache keys are rebuilt per link.** `chain_cache_key` calls
   `SubjectExpr::to_subject_text`, which renders the whole prefix. A
   spine the chain cache answers at its outermost link only pays for one
   key, but a cold spine, or any resolution running without the cache
   active, needs a key per link and so renders O(n²) bytes. The keys of a
   spine are prefixes of one another, so one render plus per-link lengths
   would do.

**Where to look:** `symbol_map/extraction/expressions/calls.rs` for the
first, `chain_cache_key` in `type_engine/resolver/mod.rs` for the second.
Neither is a correctness problem, and hand-written code never reaches the
lengths where it shows.


---

## P56. Folding array shapes across branches costs superlinear time

**Impact: Low · Complexity: Medium**

`join_shapes` keeps a variable at one tracked shape no matter how many
branches write to it, which is what stops a merge from having to compare
a variant per branch pairwise. The fold itself is not free, though:
`join_shape_entries` builds a fresh `Vec<ShapeEntry>` and interns a new
shape on every merge, so a variable that gains a key per branch pays for
hashing a shape whose entry count grows with the branch count. The work
is quadratic in the number of conditional writes.

Measured on a release build, over a generated function assigning a
distinct array shape under each of N sequential `if`s:

| N writes | analyse wall clock |
| -------- | ------------------ |
| 400      | 0.30s              |
| 800      | 0.90s              |
| 1200     | 2.10s              |

Doubling the writes roughly triples the time, and the same file with the
shapes left un-merged (each write pushing its own alternative instead)
runs in half of that, so folding is the more expensive of the two
strategies at these sizes. It is still the right default: the variant-per-
branch alternative grows the *type* without bound, which costs every
later consumer rather than just the merge. What is missing is the cheap
exit that would make the fold linear in the common case:

1. **Skip the rebuild when nothing changes.** Most merges join a shape
   with one whose keys it already covers, and the result is the existing
   shape. Comparing entry lists before allocating would return the
   interned handle unchanged rather than rebuilding and re-hashing it.

2. **Fold in place along a chain of merges.** A run of merges against the
   same variable rebuilds the accumulator from scratch each time. Joining
   into a reusable buffer and interning once at the end of the run would
   drop the repeated hashing.

**Where to look:** `join_shapes`, `join_shape_entries`, and `join_values`
in `php_type/mod.rs`, and the shape-folding branch of `merge_scopes` in
`type_engine/variable/forward_walk/scope_state.rs`. Hand-written code
does not reach the sizes where this shows; generated code and long
procedural report builders do.

---

## P50. Cache the top-level scope for `global` keyword resolution

**Impact: Low-Medium · Complexity: High**

Every `resolve_variable_types` call on a file containing `global `
rebuilds the top-level scope by forward-walking every top-level
statement with `cursor_offset = u32::MAX`. This is done once per
variable query, so hovering three variables in the same file walks the
top-level three times.

Re-entry guards (added to fix #327) prevent the walk from recursing
unboundedly, but the repeated cost remains. The preferred shape is
pre-compute-and-cache: build the top-level scope once per file version
(keyed by content hash or pointer) and reuse it across queries within
the same request cycle.

**Where to look:** `resolve_variable_in_statements` in
`type_engine/variable/resolution.rs`, the `walk_top_level_for_globals`
call. A per-request cache (similar to the chain resolution cache in
`type_engine/resolver/context.rs`) would eliminate the redundant walks.

---

## P51. CI-gated scaling and memory invariants

**Impact: Medium · Complexity: Low-Medium**

`.github/workflows/ci.yml`'s `benchmark`/`benchmark-pr` jobs only run
the `completion` bench and publish it to the tracking dashboard; a
regression is visible after the fact (someone has to look at the
dashboard) rather than failing the PR. `references.rs` and
`laravel_completion.rs` under `benches/` exist but aren't wired into
CI at all, and none of our benchmarks assert an absolute ceiling —
only relative-to-history comparison.

Add CI-gated invariants that fail the build outright when crossed,
not just recorded for later inspection:

- **Per-edit republish scaling.** A synthetic workspace ingested at
  several sizes (e.g. 100 → 5000 files); assert republish-after-edit
  wall time stays flat (or sub-linear) as file count grows, catching
  an accidental O(n) or worse regression on the republish path before
  it ships.
- **Cold/warm start wall time.** Time to `indexReady` on a pinned
  fixture workspace (a vendored copy of a real Laravel/Symfony
  project, or one of the public corpora already used by `analyze`
  triage), gated on an absolute ceiling, both cold (no cache) and warm
  (repeat run).
- **Session RSS guard.** `benches/memory_usage.py` already measures
  resident memory on two workloads; run it in CI and fail if RSS
  exceeds a fixed ceiling instead of only exposing the script for
  manual use.

**Where to look:** `.github/workflows/ci.yml`'s `benchmark`/
`benchmark-pr` jobs; `benches/memory_usage.py`; `benches/references.rs`
and `benches/laravel_completion.rs` for benches that exist but aren't
CI-gated yet.

---

## P52. The diagnostic benchmarks measure a path no consumer takes

**Impact: Medium · Complexity: Low**

`bench_diagnostics_phpactor_fixtures` in `benches/completion.rs` calls
four collectors directly:

```rust
backend.collect_deprecated_diagnostics(&uri, content, &mut out);
backend.collect_unused_import_diagnostics(&uri, content, &mut out);
backend.collect_unknown_class_diagnostics(&uri, content, &mut out);
backend.collect_unknown_member_diagnostics(&uri, content, &mut out);
```

No consumer does this. Every real caller goes through
`collect_slow_diagnostics_observed`, which first activates the chain
resolution cache, the type-engine caches, and the forward-walked
diagnostic scope cache, then runs the collectors in an order chosen so
later ones read what earlier ones cached. The benchmark activates none
of them and runs `deprecated_usage` first, cold, where production runs
it last against warm caches.

The result is a tracked number that moves for reasons users never
experience. On the `method_chain` fixture the benchmark reports roughly
twice the time the production pass takes on the same file while running
a quarter of the collectors, and an optimisation to the cached path
shows up as a fraction of its real effect: making chain cache keys lazy
measured -25% on the production pass and -3.7% here, because without an
active cache the probe never hits early and every key is needed anyway.

Point the benchmark at `collect_slow_diagnostics`, so it measures what
an editor keystroke and an `analyze` run actually pay for. This resets
the tracked history for `diagnostics/fixture/*` once, which is worth it
for a number that tracks the real path. Keep a separate uncached case
only if there is a consumer that runs collectors without the guards.

**Where to look:** `bench_diagnostics_phpactor_fixtures` in
`benches/completion.rs`; `collect_slow_diagnostics_observed` in
`diagnostics/mod.rs` for the guards and the collector order.

---

## P53. The deprecated collector deep-copies a class per member access

**Impact: Medium · Complexity: Low**

`collect_deprecated_diagnostics` resolves each member access to a class
and then clones the whole `ClassInfo` out of the `Arc` it just got:

```rust
.and_then(|name| self.find_or_load_class(&name))
.map(|arc| ClassInfo::clone(&arc));
```

`resolve_variable_subject` does the same on its own path, and the
per-variable cache stores `Option<ClassInfo>` rather than
`Option<Arc<ClassInfo>>`, so its hits clone too. The class is only ever
read afterwards (`get_method`, `get_property`, and a `&ClassInfo`
argument to `resolve_class_fully_cached`), so every one of those copies
is wasted, and the cost scales with the class's member count: a file
whose accesses land on a large resolved class (an Eloquent `Builder`, a
facade's concrete binding) pays for a full copy of its methods,
properties, and constants once per access.

Hold `Arc<ClassInfo>` through the collector instead. Both producers
already have one, deref coercion covers the read sites, and the
`enclosing_class` clone a few lines below is the same pattern.

**Where to look:** `collect_deprecated_diagnostics` and
`resolve_variable_subject` in `diagnostics/deprecated.rs`, plus the
`var_type_cache` declaration at the top of the collector.

## P54. Property narrowing re-walks the whole body once per subject

**Impact: Low · Complexity: Medium**

Every `$this->prop` or `$h->getCall()` whose type the engine needs sends
`apply_property_narrowing` back over the enclosing body from its first
statement, looking for a check that refines that subject. Nothing
remembers the answer, so a body holding n such subjects walks itself n
times, and each walk resolves the expressions it passes, which resolves
subjects of their own.

`NARROWING_IN_PROGRESS` stops that from compounding without bound — a
walk no longer starts while another is running over the same source —
but the remaining growth is still superlinear: on the reproducer from
#385 a release build measures under 0.05 s at 20 guard/chain pairs,
0.1 s at 30, and 0.4 s at 60, so roughly quadratic in the number of
narrowed subjects. Real code stays well under those sizes, which is why
this is Low rather than a bug, but a generated file or a long legacy
method can reach them.

Memoising the walk would collapse it, and blocking nested walks is what
makes that straightforward: every walk that runs now runs with nothing
above it on the same source, so its answer no longer depends on which
other walks happened to be in flight. The result depends on the source,
the subject key, the cursor offset, and the classes handed in, so a
per-request map keyed by those four and cleared with the rest of the
request caches would turn the n walks into n lookups. The awkward part is
that the walk mutates `results` in place and reports intersections
through a separate flag, so the cached value has to carry both.

**Where to look:** `apply_property_narrowing` in
`type_engine/resolver/property_narrowing.rs`, and its three callers in
`type_engine/resolver/mod.rs` (`SubjectExpr::CallExpr` and the property
path) and `narrowed_by_rewalk` in
`type_engine/variable/rhs_resolution/mod.rs`.

---

## P55. Every edit re-reads every member-reference candidate file

**Impact: Medium-High · Complexity: Medium-High**

`reindex_references_for_symbol_maps_batch` and
`evict_reference_index_uri` both call
`MemberRefCounts::invalidate_locations_all`, so any reparse marks the
exact locations of *every* cached member declaration stale, not just the
ones the edited file contributed to. The count side is invalidated far
more selectively: `member_contributions` compares each file's old and
new per-member contribution and only the members whose contribution
actually changed become `count_stale`, which is what lets an edit that
touches no access recompute nothing.

Locations are blanket-invalidated because a receiver's type can change
without changing any indexed member name or count (the
`Order $value` → `Buyer $value` case), and a clickable lens must never
point at a range the edit moved. But the consequence is that the
declaration CodeLens path re-queues every public member of the open file
on every keystroke, and `member_declaration_references_batch` then calls
`reference_file_content_arc` for each candidate file. That falls through
to `get_file_content_arc`, which is an uncached
`std::fs::read_to_string` for any file not open in the editor. On a
large application a common member name (`handle`, `get`, `name`) is a
candidate in thousands of files, so a typing pause costs thousands of
file reads even though the expensive part, the per-file
`ResolvedMemberFile` semantic layer, is still warm and reused.

### Fix

Two independent halves, either of which helps on its own:

1. **Narrow the invalidation.** Split the blanket call into the two
   causes it currently conflates. Offsets only shift in the files that
   were reparsed, so a location cache entry needs invalidating when one
   of its own locations names a rebuilt URI. The receiver-type case
   belongs where the type change is already detected: `update_ast`
   clears `resolved_members` when `any_signature_changed ||
   any_function_changed`, and the location cache should be invalidated
   from the same branch. Check what that flag covers before relying on
   it — a docblock-only `@return` edit changes receiver types too, and
   serving a stale clickable location is worse than the current cost.

2. **Make the warm semantic layer self-sufficient.** `ResolvedMemberFile`
   already packs the resolved receiver atoms per symbol-span index; it
   does not carry the LSP `Range` for the access, which is the only
   other thing `scan_file` needs the file's text for. Storing the range
   alongside the targets (16 bytes per access) would let a warm
   candidate file be filtered without reading it at all.

**Where to look:** `invalidate_locations_all` and
`member_declaration_references` in `reference_counts.rs`,
`reindex_references_for_symbol_maps_batch` and `ResolvedMemberFile` in
`reference_index.rs`, `member_declaration_references_batch` in
`references/members.rs`, and `get_file_content_arc` in
`backend/file_access.rs`.
