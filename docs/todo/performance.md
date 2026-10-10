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

## P78. A `switch` over thousands of distinct literals is still quadratic

**Impact: Low-Medium · Complexity: Medium**

A `switch` that assigns a different literal in each case is fast at the
sizes real lookup tables reach, but each case's join still costs time in
proportion to the cases before it. `benches/scaling.py`'s
`switch_literals` shape, on a release build:

| Cases | analyse wall clock |
| ----- | ------------------ |
| 2,000 | 0.45 s             |
| 4,000 | 1.6 s              |
| 8,000 | 6.3 s              |

About a fifth of the samples are in `decode_php_string_literal`, decoding
the same literals again at each join, and most of the rest are in
`dedup_types` and `is_subtype_of` comparing the growing union against
itself.

**Where to look:** `dedup_types` and `hash_for_dedup` in
`php_type/normalize.rs`, `decode_php_string_literal` in `text_scan.rs`
and its callers in `php_type/subtype.rs`.

---

## P79. Writes under the same dynamic key in separate `if`s are quadratic

**Impact: Low-Medium · Complexity: Medium**

N `if`s each writing a different shape to `$data[$id]` cost time in
proportion to the square of N. `benches/scaling.py`'s
`conditional_dynamic_writes` shape, on a release build, takes 1.6 s at
2,000 writes and 6.6 s at 4,000. The array's value type is a union that
gains one shape per `if`, and every join builds that union again from
scratch: the samples are in `dedup_types`, `hash_for_dedup` and the shape
index `join_runtime_value_types` builds to find which members could absorb
which, each of them a pass over all the shapes so far. Joining one new
member into a union that is already normalised would make each join cost
only what it adds.

That cuts the constant, but the union still grows with every write, so
only bounding it removes the growth. PHPStan turns a union of more than
256 constant-array values into one general `non-empty-array<K, V>`
(`TypeCombinator::optimizeConstantArrays`). Doing the same would change
what hover shows past that size, so it needs a decision first.

**Where to look:** `join_runtime_value_types` and `absorb_subsumed_shapes`
in `php_type/normalize.rs`, reached from a join through
`ResolvedType::collapse_redundant_runtime_literals` in
`types/resolved_type.rs`.

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
