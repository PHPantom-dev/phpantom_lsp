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

## P87. A run of conditional appends to one array is polynomial in its length

**Impact: Medium · Complexity: High**

```php
$catalog = [];
if ($user->hasPermission('access', 'catalog/category')) {
    $catalog[] = ['name' => …, 'href' => …, 'children' => []];
}
if ($user->hasPermission('access', 'catalog/product')) {
    $catalog[] = ['name' => …, 'href' => …, 'children' => []];
}
// … ninety of these in OpenCart's admin/controller/common/column_left.php
```

A synthetic file with N such blocks takes 0.01 s at N = 20, 0.57 s at
80 and 9.4 s at 160 (release `analyze`). OpenCart's `column_left.php`
alone takes 2.2 to 3.3 s, the whole tail of that project's analysis; the
rest of its 1,872 files finish in under 0.6 s. An editor opening the
file waits as long for its diagnostics.

Nearly all of it is in `ScopeState::merge_branch`. Each condition that
narrows something (a call key, `$s === 'x'`; a bare `bool` does not)
records an implied narrowing whose trigger is the holder's whole shape
at that point. Every later join re-asks each accumulated proof whether
it survives (`join_implied_narrowings` → `disjoint` → `shape_is_subshape`),
and those proofs never lapse, so a join costs O(N) proofs times an O(N)
shape comparison. `ResolvedType::drop_subsumed_entries` in `merge_local`
spends the rest comparing the same shapes. The shape comparison itself
allocates: `runtime_shape_keys` formats every positional key with
`to_string` and `KeyPositions::find` builds a `HashMap` per call.

Fix the growth, not the constant. Each old proof stays live because
the fall-through path still holds the shape its trigger names, so
lapsing them on reassignment alone is not enough; the join needs a way
to settle a proof once instead of re-proving it at every later join
(remember which proofs were already shown to survive against a holder
value that has not changed since). Making the shape comparison
allocation-free is worth doing on its own, but leaves the curve where
it is.

**Where to look:** `join_implied_narrowings` in
`src/type_engine/variable/forward_walk/scope_state/proofs.rs`,
`shape_is_subshape` in `src/php_type/subtype.rs`, `runtime_shape_keys`
in `src/php_type/shape_keys.rs`.

---

## P88. Reading a by-reference parameter's out type walks the callee once per `return`

**Impact: Medium · Complexity: Medium**

```php
function privParseOptions(&$p_options_list, $p_size, &$v_result_list, $v_requested_options = false)
{
    // ~340 lines, 25 `return` statements
}

$v_result = $this->privParseOptions($v_arg_list, $v_size, $v_options, [...]);
```

When a call passes a variable to a by-reference parameter, the walker
reads the callee's body for what it leaves there (`read_out_type` in
`src/type_engine/call_resolution/out_param.rs`). It does so by running a
separate `resolve_variable_types_in_program` query at every exit, each
`return` plus the closing brace, with the diagnostic scope cache
suspended, so each query walks the body from the top. A body with R
returns costs R full walks per out-parameter, and the walks themselves
reach other by-reference calls whose callees are read the same way (up
to `MAX_OUT_TYPE_DEPTH`).

WordPress core's bundled `class-pclzip.php` (5,700 lines, by-reference
array parameters on nearly every private method) takes 10.1 s on its own
in a debug `analyze`, the slowest file in core by a factor of two and
about 8% of the whole project's CPU time. The same file with the `&`
removed from its parameters takes 3.0 s. Every method on its own takes
0.1 to 0.4 s, so the cost is all in the cross-method readings. An editor
opening the file pays it on each diagnostic pass.

Read every exit in one walk: walk the body once and record the
parameter's type as the walk passes each `return` (the walker already
visits them in order), then join the recordings. That makes a reading
O(body) instead of O(returns × body), and the existing memo keeps it to
one reading per out-parameter per pass.

**Where to look:** `read_out_type` and `infer_out_type` in
`src/type_engine/call_resolution/out_param.rs`;
`resolve_variable_types_in_program` in
`src/type_engine/variable/resolution/`.

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
