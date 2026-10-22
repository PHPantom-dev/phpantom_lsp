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

## P84. Class lookups re-apply the Laravel patches on every call

**Impact: Medium-High · Complexity: Medium**

`find_or_load_class_typed` (`resolution/classes.rs`) memoises the indexed
class but runs the auth, storage, macro and pivot refinements after the
memo, on every lookup. When one applies, it copies the whole `ClassInfo`
into a new `Arc`, which also defeats the pointer-identity shortcut in
`forward_walk/scope_state/merge.rs`.

Measured on the large Laravel app (debug `analyze`, 2026-10-19): 8.4M
lookups, 400k of them copying, spend 47.7 s of the run's 371 CPU-seconds
in the refinements (12.9%), 32.7 s of it in the copies alone. Memoise the
refined class keyed on the indexed class and a generation that moves with
the auth model, storage driver, macro and pivot registries, so a repeat
lookup hands back the same `Arc`.

## P85. One keystroke parses the open file three times

**Impact: Medium · Complexity: Medium-High**

`update_ast` (`parser/ast_update.rs`) parses the buffer and discards the
tree, then the fast diagnostics (`diagnostics/collect.rs`) and the slow
pass (`diagnostics/incremental/mod.rs`) each parse it again under their
own parse cache. A probe counted three parses per edit in every file it
tried.

Measured on the large Laravel app (debug, an edit inside one method, the
slow pass running partial): a parse costs 3.5 to 5 ms on 1,300 to
1,900-line services, where the whole edit cycle takes 95 to 185 ms, and
0.2 to 0.6 ms on service providers whose cycle takes 7 to 22 ms. Parsing
once would save 4 to 11% of the cycle. The Laravel refreshers `update_ast`
calls add at most one more parse on the providers tried (about 2.5% of the
cycle), not worth a change of their own. Keeping the parsed tree alive
between `didChange` and the diagnostic pass costs its parse memory, about
22 MB for a 73 KB file, for as long as the file is open.

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

### Memory

```sh
# Per-store deep-size audit printed at the end of `analyze`. The feature
# installs a counting allocator, so build it into its own target dir to
# keep the normal binary untouched.
cargo build --features mem-audit --target-dir target/memaudit
PHPANTOM_MEM_AUDIT=1 target/memaudit/debug/phpantom_lsp analyze \
  --project-root <dir> --no-colour
```

The audit walks every long-lived store with shared `Arc`s counted once,
then drops each store in turn and reports the exact bytes released. For
an editor session, `LspClient` in `benches/memory_usage.py` can drive the
server (initialize, wait for indexing, open, hover) while you sample
`VmRSS`. The two differ: a large Laravel app settled at 842 MB in the LSP
against a 1,016 MB peak for `analyze`.

Use heaptrack only on small projects (around 100 MB peak), and run it
inside `systemd-run --user --scope -p MemoryMax=8G`. Its bookkeeping
scales with the allocation count, and on a large Laravel app it exhausted
a 64 GB machine.

### Growth checks

A whole-project profile hides a cost that only grows on one unusual
input. Generate the input at N and 2N (a long top-level script, a
`switch` over N distinct literals, N conditional writes to one array)
and compare the two times: doubling N should roughly double the time.
