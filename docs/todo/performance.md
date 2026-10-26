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

## P86. A Symfony app's generated `config/reference.php` is slow to parse

**Impact: Medium · Complexity: Medium**

Symfony 7.3 and later generate `config/reference.php` for the application:
one 100 KB class whose docblock declares some forty `@psalm-type` array
shapes, several hundred lines each, nested into one another. Parsing that
single file takes about 0.5 s in a debug build, roughly ten times what a
PHP file of its size costs, so something on the way from the docblock to
the type aliases is superlinear in the size of a shape.

Every Symfony project pays it once, when the workspace index parses the
file. On `projects/symfony-demo` (debug `analyze`, 2026-10-24) it is the
whole difference between 0.74 s and 1.23 s, since the workspace walk that
reads the service configuration now runs there too. Profile a single-file
`analyze` of the file to find where the time goes.

**Where to look:** `src/docblock/` (type-alias tags), the alias handling in
`src/parser/classes/`.

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
