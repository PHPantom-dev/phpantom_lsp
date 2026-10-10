# PHPantom — Performance

Internal performance improvements that reduce latency, memory usage,
and lock contention on the hot paths. These items are sequenced so
that structural fixes land before features that would amplify the
underlying costs (parallel file processing, full background indexing).

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

An item belongs here only when it saves at least 7% memory or 4% time
on a realistic use case, or 8% for a High or Very High complexity
change. Trivial fixes are batched and made without being weighed
against that bar.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## P70. Diagnostics on a long file find each access's context by scanning

**Impact: Medium · Complexity: Medium**

Several diagnostic collectors work out the context of a member access or
call by scanning for it, so each access costs time in proportion to the
length of the file and the whole pass costs its square. The forward walk
over the same file no longer does. On a release build, a generated
top-level script of repeated 11-line blocks (an `if`/`elseif`/`else`, a
`switch`, a `try`, a `while`, a closure and a few calls each) takes:

| Lines  | analyse wall clock |
| ------ | ------------------ |
| 5,500  | 0.7 s              |
| 11,000 | 2.1 s              |
| 22,000 | 8.1 s              |

with or without a `namespace` declaration. Legacy codebases do carry
files this long, and every edit to one re-runs the whole pass. Well over
half of the samples are in these lookups rather than in the walk:

- `class_context_placeholder` (`class_lookup.rs`) calls
  `text_scan::namespace_at_offset`, which searches the text backwards from
  the access for a `namespace` keyword. In a namespaced file the
  declaration sits near the top, so the search is just as long. It runs
  once per call from `collect_argument_type_diagnostics`, the return-type
  checks, the deprecated collector and the unknown-member collector
  (about 30%). The namespace blocks are already recorded per file:
  `Backend::namespace_at_offset` (`backend/file_access.rs`) answers from
  them, but this path only has the content and an offset.
- `SubjectCacheKey::build` (`diagnostics/subject_cache.rs`) calls
  `SymbolMap::find_enclosing_scope` and `find_narrowing_block`, which test
  every scope and every narrowing block in the file, plus
  `active_var_def_offset`, which adds another scope scan and a linear
  `find_var_definition` over every variable definition (about 15%).
- String comparisons inside the unknown-member and deprecated collectors
  themselves, not yet traced to a single call (about 12%).

The namespace blocks, scopes and narrowing blocks of a file are all known
once it is parsed, so each lookup can be a binary search over ranges
recorded at that point.

**Where to look:** `namespace_at_offset` in `text_scan.rs` and its
callers, `find_enclosing_scope` and `find_narrowing_block` in
`symbol_map/mod.rs`, and `SubjectCacheKey::build`.

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
`collect_slow_diagnostics_observed`, which first activates the shared
chain resolution cache, the type-engine caches, the forward-walked
diagnostic scope cache and the pass's line table, then runs the
collectors in an order chosen so later ones read what earlier ones
cached. The benchmark activates none of the shared ones (the deprecated
and unknown-member collectors open only their own) and runs
`deprecated_usage` first, cold, where production runs it about halfway
through against warm caches.

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

## P53. Diagnostics and type-hint resolution deep-copy classes they only read

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

`resolve_named_type` (`type_engine/types/resolution.rs`) has the same
habit on every type-hint lookup. It takes the class out of its `Arc` with
`Arc::unwrap_or_clone`, which deep-copies whenever the class is shared
(a cached class always is), and wraps the copy in a fresh `Arc` even when
nothing changed it. Only the Eloquent collection swap and the generic
path modify the class.

Hold `Arc<ClassInfo>` through the collector instead; both producers
already have one, and deref coercion covers the read sites. In
`resolve_named_type`, keep the `Arc` and copy only on the paths that
modify the class.

**Where to look:** `collect_deprecated_diagnostics` and
`resolve_variable_subject` in `diagnostics/deprecated.rs`, plus the
`var_type_cache` declaration at the top of the collector;
`resolve_named_type` in `type_engine/types/resolution.rs`.

---

## P51. CI checks for how cost grows with input size

**Impact: Medium · Complexity: Low-Medium**

CI already catches regressions on fixed inputs: `benchmark-pr` fails a
pull request when the `completion` bench regresses past 130% of its
history, and `memory-benchmark-pr` does the same for
`benches/memory_usage.py`. Two gaps remain:

- Only `--bench completion` runs. `references`, `laravel_completion` and
  `custom_builder` are registered in `Cargo.toml` but never run.
- Nothing checks how cost grows with the size of an input. Every
  performance problem users have hit so far was a growth problem on an
  input of an unusual shape (thousands of calls to undefined functions,
  hundreds of Blade views without a DocBlock, a `switch` assigning
  hundreds of distinct literals, and the open item P70), and a
  benchmark over fixed fixtures cannot see one until a project that has
  the shape reports it.

Add a scaling check: generate each known pathological shape (a long
top-level script, a `switch` over N distinct literals, N conditional
array writes, N calls to undefined functions, a long method chain) at N
and 2N, analyse both, and fail when doubling the input more than about
2.5 times the time. A ratio is independent of the machine's speed, so the
check can run on a CI runner, and each shape fixed later joins the set.

**Where to look:** `.github/workflows/ci.yml`'s `benchmark`/
`benchmark-pr` jobs; `benches/` for the registered benches.

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
   with changed regions.
5. Merge shifted cached diagnostics with freshly-computed ones.

**Prerequisites:** a member-level diff. The edit path only compares
class signatures today (`ast_update.rs`), so which members changed has
to be worked out first.

**References:**
- Psalm: `FileDiffer` and `FileStatementsDiffer` in
  `Psalm\Internal\Diff`
- Psalm: `Analyzer::shiftFileOffsets()` for the offset-shifting logic

---

## P73. A lazily loaded file is parsed three times

**Impact: Low-Medium · Complexity: Low**

`parse_and_cache_content_versioned` (`resolution.rs`) builds a loaded
file's classes from three helpers, `parse_use_statements`,
`parse_namespace` and `parse_php_classes_by_block`, and each runs its own
mago parse of the same content (and copies it), because no parse cache
is installed for the file. Every embedded stub and every vendor or
project file loaded on demand is parsed three times where once would do.

Installing a parse cache for the content around the three calls removes
the extra two. On a release build that takes about 2% off an `analyze`
of a mid-sized Laravel project; paths dominated by on-demand loading
(startup population, the go-to-implementation scan) spend a larger share of their time
parsing. `parse_php_classes_by_block` already works out the use map and
namespace internally, so the other two helpers could also read them from
there.

**Where to look:** `parse_and_cache_content_versioned` in
`resolution.rs`; `with_parse_cache` in `parser/mod.rs`.

---

## P74. Every class lookup by name re-parses and lowercases the name

**Impact: Low-Medium · Complexity: Low**

`find_or_load_class(&str)` (`resolution.rs`) runs `PhpType::parse` on the
name before looking it up, so that callers may pass `?Foo` or
`Collection<int, User>`. Most callers pass a bare class name, and
`try_parse` has no fast path for one: each lookup runs the full PHPDoc
type parser, hyphenated-keyword rewriting included, before
`class_loader_memo` is consulted. 119 call sites use the `&str` form.
Separately, `is_scalar_name` and `is_keyword_type`
(`php_type/keywords.rs`) allocate a lowercase copy of the name on every
call, and `find_indexed_class` reaches them on every lookup, memo hits
included.

On a release `analyze` of a mid-sized Laravel project the PHPDoc type
parser is about 4% of CPU samples (part of it is this re-parse), and the
two keyword checks about 2%.

**Fix:** give `PhpType::try_parse` a fast path for input made only of
identifier characters and backslashes that is not a keyword, returning
the named type directly, and compare against the keyword lists with
`eq_ignore_ascii_case` (or a stack buffer) instead of allocating. An
earlier attempt at allocation-free `CiMap` lookups regressed slightly
under mimalloc, so confirm this one with an order-swapped A/B run.

**Where to look:** `find_or_load_class` in `resolution.rs`,
`PhpType::try_parse` in `php_type/parse.rs`, `is_scalar_name` and
`is_keyword_type` in `php_type/keywords.rs`.

---

## P66. Stub version filtering rescans a stub file once per symbol it declares

**Impact: Low-Medium · Complexity: Low-Medium**

`set_php_version` drops every stub symbol marked `@removed` at or before
the target version. For a file that mentions `@removed` at all,
`is_stub_function_removed` and its class and constant counterparts locate
each symbol with `source.find("function NAME(")` from the start of the
file, so a stub file declaring n symbols is scanned n times. 49 of the
537 stub files mention `@removed`, and they declare about 3,700 symbols
between them.

The server pays this once at startup and `analyze` once per run, on the
serial path before any worker starts. On a release build it is about
three quarters of an empty project's 0.07 s `analyze`, roughly 50 ms, or
about 4% of a 1.2 s `analyze` of a mid-sized Laravel project. Every test
that uses `new_test_with_full_stubs`, including each fixture the
assertType runner checks, pays it too, about 0.45 s each in a debug
build.

Scanning each such file once for its `@removed` docblocks and recording
the name of the declaration that follows each one would give a per-file
set of removed names, turning the filter into set lookups.

**Where to look:** `set_php_version` in `lib.rs` and the
`is_stub_*_removed` family in `stubs.rs`.

---

## P58. A member-completion cache hit copies the whole item list

**Impact: Low · Complexity: Low**

The member-completion cache exists so that each keystroke in
`$model->wh…` reuses the unfiltered member list instead of re-resolving
it. A hit clones the cached `Vec<CompletionItem>` wholesale, which for an
Eloquent model is several hundred items each carrying several `String`s
and an optional documentation block — and the prefix filter then throws
most of them away. The cache is capped, so this is CPU rather than a
leak, but it is paid on the keystroke the cache was added to make fast.

Store an `Arc<Vec<CompletionItem>>` and have the filter take a slice,
cloning only the items that survive.

**Where to look:** `member_completion_cache` and
`filter_member_completion_items` in
`completion/handler/member_access.rs`.

---

## Appendix: Profiling

### Commands

```sh
# Flat profile of a whole-project run. The release build has symbols but
# no debug info or frame pointers, so for call graphs build with
# RUSTFLAGS="-C force-frame-pointers=yes" and record with `-g`.
perf record -F 999 -- ./target/release/phpantom_lsp analyze \
  --project-root <dir> --no-colour

# Text report (top functions):
perf report --stdio --no-children --sort symbol | head -80

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

### Growth checks

A whole-project profile hides a cost that only grows on one unusual
input. Generate the input at N and 2N (a long top-level script, a
`switch` over N distinct literals, N conditional writes to one array)
and compare the two times: doubling N should roughly double the time.
