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

## P91. Parsing and merging of the full index take turns

**Impact: Medium-High · Complexity: Medium-High**

On `projects/drupal` (15,900 PHP files, 32 cores, release build,
2026-11-09) the server takes 2.4 s from `initialize` to idle, 1.7 s of it
in the full index. PHPlumb parses every file of the same checkout fully,
with its member index, in 0.49 s. Our parse is heavier per file, but that
is not what decides the wall time: the parse workers keep only 4 to 6 of
the 32 cores busy, while one tokio thread runs at 100% from the start of
the full index to its end.

Drupal core counts as installed-package code, so `parse_batch_parallel`
parses it `DORMANT_CHUNK` (1,024) files at a time and publishes each chunk
through `apply_ast_index_parse_results_batch` before the next one starts.
That merge is serial, and the workers wait for it. Inside it,
`reindex_references_for_symbol_maps_batch` builds every file's reference
entries (`reference_entries_for_symbol_map`) on the merging thread, then
inserts them into `std` `HashMap`s, so about a quarter of that thread's
samples are SipHash (`hash_one::<&ReferenceIndexKey>`, `Arc<str>` keys).
The debug build shows the same shape: 10.2 s of full index, 7.0 s of it
for the 10,893 core files.

Three changes, measured one at a time:

- Build each file's reference entries in the parse worker, next to its
  symbol map, so the merge only inserts.
- Give the reference index (`by_key`, `uri_keys`, the per-key URI maps) a
  fast non-cryptographic hasher.
- Parse the next chunk while the previous one merges, keeping at most two
  chunks of parses alive so the memory bound `DORMANT_CHUNK` exists for
  still holds.

The serial tail should shrink to the inserts alone, which would put the
full index under 1 s on this checkout. Every project with installed
packages under the workspace walk (Drupal core, WordPress core) goes
through the same chunks, and every project's own files go through the same
merge. Record a frame-pointer profile first (see the appendix) to confirm
how the rest of the merging thread's time splits, since the flat profile
spreads it thinly.

**Where to look:** `parse_batch_parallel` and `parse_and_publish` in
`src/indexing/preload.rs`, `apply_ast_index_parse_results_batch` in
`src/parser/ast_update.rs`, `reindex_references_for_symbol_maps_batch` in
`src/reference_index.rs`.

---

## P92. Startup's file walks and resource indexing run on one core

**Impact: Medium · Complexity: Medium**

On the same checkout and build as P91, about 0.5 s of the 2.4 s to idle
runs on a single core while the other 31 wait:

- `scan_drupal_directories` walks the web root with a sequential `ignore`
  walker and a `path.is_file()` stat per entry, before the classmap
  scanner's parallel lexing starts.
- `visit_workspace_files_gitignore`, the full index's discovery walk, is
  sequential too and stats every entry. It takes 214 ms in release and
  737 ms in debug, and it walks most of the web root a second time.
- `index_resource_paths_batch` reads and scans the 4,494 YAML and Twig
  files the walk found one after another.

The classmap scanner already has a parallel walk (`walk_roots`:
`build_parallel`, work stealing, sorted output for a deterministic
first-wins merge). Move both walks onto that pattern and use the entry's
own file type rather than a fresh stat. The full index could also take the
files the Drupal walk found instead of walking them again. Read and scan the
resource files on parallel workers and publish them in one merge, as the PHP
files are. PHPlumb's discovery is a single parallel walk that canonicalizes
only paths reached through a symlink, which is what its 0.49 s includes.

**Where to look:** `scan_drupal_directories` in
`src/classmap_scanner/discovery.rs`, `visit_workspace_files_gitignore` and
`walk_roots` in `src/classmap_scanner/walk.rs`,
`index_resource_paths_batch` in `src/resource_navigation.rs`.

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

## P90. A long `if`/`elseif` chain grows slightly faster than its length

**Impact: Low-Medium · Complexity: Medium**

The `elseif_literals` shape in `benches/scaling.py` (one function, an
`if`/`elseif` chain of N arms each assigning a different string literal
to the same variable) doubles its time by 2.15 to 2.4 when N doubles
(release build, 2026-11-07), and the ratio rises with N: 2.15 to 2.39 at
N = 4,000, 2.27 to 2.41 at N = 8,000. The same lookup written as a
`switch` (`switch_literals`) stays near 2.1 at every size, as do the
other enforced shapes. A debug build shows a flat 2.1 for both, so the
extra cost is small next to the linear work and only shows once release
optimisation shrinks that.

It matters mostly because it sits close to the check's 2.5 limit: a
slower CI runner pushes it to 2.42 to 2.52, which fails the scaling job
on unrelated pull requests. Raising N or retrying does not help, since
the ratio grows with N.

Not profiled yet. The likely suspect is the join after each arm: the
variable's type is a union that gains one literal per arm, so a join
whose cost is linear in the union's size makes the chain quadratic,
where the `switch` path may merge its arms once. Profile a single-file
`analyze` of the shape at N = 8,000 to confirm before changing anything.

**Where to look:** the `if`/`elseif` handling and `ScopeState::merge_branch`
in `src/type_engine/variable/forward_walk/`, against how `switch` arms
are merged.

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
