# PHPantom — Indexing

This document covers how PHPantom discovers, parses, and caches class
definitions across the workspace. The goal is to remain fast and
lightweight by default while offering progressively richer modes for
users who want exhaustive workspace intelligence.

Items are ordered by **impact** (descending), then **effort** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Effort** | **Low** (≤ 1 day), **Medium** (2-5 days), **Medium-High** (1-2 weeks), **High** (2-4 weeks), **Very High** (> 1 month) |

---

## Current state

PHPantom has three byte-level scanners (no AST) for early-stage file
discovery:

1. **composer-classmap** — parses Composer's `autoload_classmap.php`
   into an in-memory `HashMap<String, PathBuf>`.
2. **PSR-4 scanner** (`find_classes`) — walks PSR-4 directories from
   `composer.json` and extracts class FQNs with namespace compliance
   filtering.
3. **full-scan** (`find_symbols`) — walks files and extracts classes,
   standalone functions, `define()` constants, and top-level `const`
   declarations in a single pass.

These scanners serve three scenarios at startup:

| Scenario                                                  | Class discovery                                                                                    | Function & constant discovery                                                             |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| **Composer project** (classmap complete)                  | composer-classmap                                                                                  | `autoload_files.php` byte-level scan + lazy parse                                         |
| **Composer project** (classmap missing/incomplete)        | PSR-4 scanner + vendor packages                                                                    | `autoload_files.php` byte-level scan + lazy parse                                         |
| **No `composer.json`**                                    | full-scan on all workspace files                                                                   | full-scan on all workspace files                                                          |
| **Monorepo** (no root `composer.json`, subprojects found) | Per-subproject: composer-classmap or PSR-4 + vendor packages. Loose files: full-scan with skip set | Per-subproject: `autoload_files.php` byte-level scan + lazy parse. Loose files: full-scan |

The "no `composer.json`" path is fully lightweight: `find_symbols`
populates classmap, `autoload_function_index`, and
`autoload_constant_index` in one pass, and lazy `update_ast` on first
access provides complete `FunctionInfo`/`DefineInfo`. All directory
walkers (full-scan, PSR-4 scanner, vendor package scanner, and
go-to-implementation file collector) use the `ignore` crate for
gitignore-aware traversal instead of hardcoded directory name
filtering. Hidden directories are skipped automatically.

The monorepo path activates when there is no root `composer.json` but
`discover_subproject_roots` finds subdirectories with their own
`composer.json` files. Each subproject is processed through the full
Composer pipeline (PSR-4, classmap, vendor packages, autoload files)
and results are merged into the shared backend state. Loose PHP files
outside subproject trees are picked up by the full-scan walker with a
skip set that prevents double-scanning subproject directories. See
the ARCHITECTURE.md Composer Integration section for full details.

Find References parses files in parallel via `std::thread::scope`.
Go-to-Implementation walks classmap files sequentially.

---

## Strategy modes

Four indexing strategies, selectable via `.phpantom.toml`:

```toml
[indexing]
# "full"     (default) - background-parse all project files for rich intelligence
# "composer"           - merged classmap + self-scan
# "self"               - always self-scan, ignore composer classmap
# "none"               - no proactive scanning
strategy = "full"
```

### `"full"` (default)

Background-parse user PHP files for rich intelligence after discovery.
This is the zero-config experience and populates the symbol/reference
indexes used by workspace-wide navigation.

### `"composer"`

Merged classmap + self-scan. Load Composer's classmap (if it exists)
as a skip set, then self-scan all PSR-4 and vendor directories for
anything the classmap missed. Whatever the classmap already covers is
a free performance win; whatever it's missing, we find ourselves. No
completeness heuristic needed.

### `"self"`

Always build the classmap ourselves. Ignores `autoload_classmap.php`
entirely. Equivalent to the merged approach with an empty skip set.
For users who prefer PHPantom's own scanner or who are actively
editing `composer.json` dependencies.

### `"full"`

Background-parse every PHP file in the project. Uses Composer data to
guide file discovery when available, falls back to scanning all PHP
files in the workspace when it is not. Populates the uri_classes_index,
symbol_maps, and all derived indices. Enables workspace symbols, fast
find-references without on-demand scanning, and rich hover on
completion items. Memory usage grows proportionally to project size.

### `"none"`

No proactive file scanning. Still uses Composer's classmap if present,
still resolves classes on demand when the user triggers completion or
hover, still has embedded stubs. The only difference from `"composer"`
is that it never self-scans to fill gaps.

---

## X2. Parallel file processing

**Goal:** Speed up workspace-wide operations (find references,
go-to-implementation, self-scan, diagnostics) by processing files in
parallel with priority awareness.

All prerequisites (`RwLock`, `Arc<String>`, `Arc<SymbolMap>`) are
complete.

### Current state (partial)

`ensure_workspace_indexed` (used by find references) now parses files
in parallel via two helpers in `references/mod.rs`:

- **`parse_files_parallel`** — takes `(uri, Option<content>)` pairs,
  loads content via `get_file_content` when not provided, splits work
  into chunks, and parses each chunk in a separate OS thread.
- **`parse_paths_parallel`** — takes `(uri, PathBuf)` pairs, reads
  files from disk and parses them in parallel.

Both use `std::thread::scope` for structured concurrency (all threads
join before the function returns). The thread count is capped at
`std::thread::available_parallelism()` (typically the number of CPU
cores). Batches of 2 or fewer files skip threading overhead.

Transient entry eviction after GTI and find references has been
removed. Parsed files stay cached in `uri_classes_index`, `symbol_maps`,
`use_map`, and `namespace_map` so that subsequent operations benefit
from the work already done. This trades a small amount of memory for
faster repeat queries and simpler code.

**Self-scan classmap building** (`scan_psr4_directories`,
`scan_directories`, `scan_vendor_packages`,
`scan_workspace_fallback_full`) now uses a two-phase approach:
directory walks collect file paths first (single-threaded), then files
are read and scanned in parallel batches via `std::thread::scope`.
Three parallel helpers in `classmap_scanner.rs` cover the three scan
modes: `scan_files_parallel_classes` (plain classmap),
`scan_files_parallel_psr4` (PSR-4 with FQN filtering), and
`scan_files_parallel_full` (classes + functions + constants). Small
batches (≤ 4 files) skip threading overhead.

The byte-level PHP scanner (`find_classes`, `find_symbols`) uses
`memchr` SIMD acceleration to skip line comments, block comments,
single-quoted strings, double-quoted strings, and heredocs/nowdocs
instead of scanning byte-by-byte. This reduces per-file scanning time
for files with large docblocks or string literals.

### Remaining work

The following are deferred to a later sprint:

- **Priority-aware scheduling.** Interactive requests (completion,
  hover, go-to-definition) should preempt batch work. Currently all
  threads run at equal priority.
- **Parallel classmap scanning in `find_implementors`.** Phase 3 of
  `find_implementors` reads and parses many classmap files
  sequentially. Parallelizing this requires care because it
  interleaves reads and writes through `class_loader` callbacks.
- **`memmap2` for file reads.** Avoids copying file contents into
  userspace when the OS page cache already has them.
- **Parallel autoload file scanning.** The `scan_autoload_files` work
  queue is inherently sequential due to `require_once` chain
  following, but the initial batch of files could be processed in
  parallel before following chains.

### Why not rayon?

`rayon` is the obvious choice for "process N files in parallel" and
Libretto uses it successfully. But it runs its own thread pool
separate from tokio's runtime. When rayon saturates all cores on a
batch scan, tokio's async tasks (completion, hover, signature help)
get starved for CPU time. There is no clean way to pause a rayon
batch when a high-priority LSP request arrives.

### Why the classmap is not a prerequisite

The classmap is a convenience for O(1) class lookup and class name
completion. But most resolution already works on demand via PSR-4
(derive path from namespace, check if file exists). Class name
completion is a minor subset of what users actually trigger. This
means classmap generation can run at normal priority without blocking
the user. They can start writing code immediately while the classmap
builds in the background.

---

## X3. Completion item detail on demand

**Goal:** Show type signatures, docblock descriptions, and
deprecation info in completion item hover without parsing every
possible class up front.

### Current limitation

When completion shows `SomeClass::doThing()`, hovering over that item
in the completion menu shows nothing because we haven't parsed
`SomeClass`'s file yet. Parsing it on demand would be fine for one
item, but the editor may request resolve for dozens of items as the
user scrolls.

### Approach: "what's already discovered"

Use `completionItem/resolve` to populate `detail` and
`documentation` fields. If the class is already in the uri_classes_index (parsed
during a prior resolution), return the full signature and docblock.
If not, return just the item label with no extra detail.

In `"full"` mode, everything is already parsed, so every completion
item gets rich hover for free. In `"composer"` / `"self"` mode, items
that happen to have been resolved earlier in the session get rich
detail; others don't. This is a graceful degradation that never blocks
the completion response.

### Future: speculative background parsing

When a completion list is generated, queue the unresolved classes for
background parsing at low priority. If the user lingers on the
completion menu, resolved items will progressively gain detail. This
is a nice-to-have, not a requirement.

---

## X4. Full background indexing

**Goal:** Parse every PHP file in the project in the background,
enabling workspace symbols, fast find-references without on-demand
scanning, and complete completion item detail.

**Prerequisites (from [performance.md](performance.md)):**

- **`Arc<ClassInfo>` (P1).** Full indexing stores a `ClassInfo` for
  every class in the project. Without `Arc`, every resolution clones
  the entire struct out of the map. With `Arc`, retrieval is a
  reference-count increment. This is the difference between full
  indexing using ~200 MB vs. ~500 MB for a large project.

### Trigger

By default. Users can opt out with `strategy = "composer"`, `"self"`, or
`"none"` in `.phpantom.toml`.

### Design: self + second pass

Full mode is not a separate discovery system. It works exactly like
`"self"` mode (self-scan) and then schedules a second pass:

1. **First pass (same as self):** Build the classmap via byte-level
   scanning. This completes in about a second and gives us class
   name completion and O(1) file lookup.
2. **Second pass:** Iterate every file path in the now-populated
   in-memory classmap and call `update_ast` on each one at Low
   priority. This populates uri_classes_index, symbol_maps, fqn_uri_index,
   global_functions, and global_defines.

No new file discovery logic is needed. The classmap from the first
pass already contains every relevant file path. The second pass just
enriches it.

When `composer.json` does not exist (e.g. the user opened a monorepo
root or a non-Composer project), the first pass falls back to walking
all PHP files under the workspace root, so the second pass still has
a complete file list to work from.

### Progressive enrichment

The user experiences three stages:

1. **Immediate:** LSP requests are up and running. Completion, hover,
   and go-to-definition work via on-demand resolution and stubs.
2. **Seconds:** Classmap is ready. Class name completion covers the
   full project. Cross-file resolution is O(1).
3. **Under a minute:** Full AST parse complete. Workspace symbols,
   fast find-references (no on-demand scanning), rich hover on
   completion items.

Each stage improves on the last without blocking the previous one.

### Behaviour

1. Respect the priority system from X2: pause the second pass
   when higher-priority work arrives.
2. Process user code first, then vendor.
3. Report progress via `$/progress` tokens so the editor can show
   "Indexing: 1,234 / 5,678 files". The `$/progress` infrastructure
   (token creation, begin/report/end helpers) is already in place and
   used during workspace initialization. The second pass just needs
   to call `progress_report` as it processes each file.

### Memory

Currently we store `ClassInfo`, `FunctionInfo`, and `SymbolMap`
structs that are not as lean as they could be. For a 21K-file
codebase, full indexing will use meaningful RAM. Since full indexing is
the default, we should profile and trim struct sizes over time. The aim
is to stay under 512 MB for a full project.

The performance prerequisites above (P1 `Arc<ClassInfo>`,
`Arc<String>`, `Arc<SymbolMap>`) directly reduce memory usage by
sharing data across the uri_classes_index, caches, and snapshot copies instead
of deep-cloning each. These should be measured before and after to
validate the 512 MB target.

### Workspace symbols

With the full index populated, `workspace/symbol` becomes a simple
filter over the uri_classes_index and global_functions maps. No additional
infrastructure needed.

When full indexing is disabled, workspace symbols still works but only returns results
from already-parsed files (opened files, on-demand resolutions, stubs).
Complete coverage requires the default `strategy = "full"`.

---

## X5. Granular progress reporting

**Impact: Medium · Effort: Medium**

All progress indicators in PHPantom currently report coarse
milestone percentages (e.g. 10%, 20%, 70%) or just begin/end with
no intermediate updates. On large codebases the user sees the bar
jump from 0% to "done" with no feedback in between. This affects
three areas:

1. **Workspace indexing.** `init_single_project` reports four
   hardcoded milestones (10/20/70/100). The actual scanning work
   (`build_self_scan_composer`, `scan_autoload_files`) runs between
   those milestones with no per-file reporting.
2. **Monorepo indexing.** `init_monorepo` divides 10..80 across
   subprojects, which gives per-subproject granularity. But within
   each subproject the scanning is opaque. The loose-file scan
   (80..95) has no file-level reporting either. When new subprojects
   are discovered during a directory walk, the total should grow
   dynamically so the percentage reflects actual progress.
3. **Go to Implementation and Find References.** These report
   begin/end only. The underlying scans (`find_implementors`,
   `find_member_references`, `find_class_references`) iterate over
   classmap files, uri_classes_index entries, and PSR-4 directories, all of
   which have known or discoverable totals.

### Design

Introduce a lightweight progress callback that the scanning
functions accept optionally:

```rust
/// Progress callback: (completed, total, phase_label).
/// `total` may increase during scanning as new files are
/// discovered (e.g. PSR-4 directory walk).
type ProgressFn = dyn Fn(u32, u32, &str) + Send + Sync;
```

The caller (the async handler) creates the callback, which
captures the progress token and a `last_report` timestamp. The
callback checks `Instant::elapsed` and only sends a
`WorkDoneProgressReport` when at least 100 ms have passed since
the last report, avoiding notification spam.

### Indexing

- `build_self_scan_composer` and the parallel scan helpers
  (`scan_files_parallel_classes`, `scan_files_parallel_full`)
  accept an optional `&ProgressFn`. The total is the number of
  files to scan (known up front from the directory walk). Each
  file processed increments the completed count.
- `scan_autoload_files` reports per-file progress through the
  same callback.
- For monorepo mode, the per-subproject percentage range
  (currently 10..80) can be subdivided by file count within
  each subproject. If the loose-file scan discovers additional
  files beyond the initial estimate, the total grows and the
  percentage recalculates.

### Go to Implementation

`find_implementors` has five phases with different totals:
Phase 0 (fqn_class_index), Phase 1 (fqn_uri_index), Phase 2 (PSR-4
files), Phase 4 (stubs), Phase 5 (PSR-4 walk). The total for
each phase is known before iteration begins. Report progress
within each phase and allocate percentage ranges across phases
proportional to expected cost (Phase 3 dominates).

### Find References

`find_member_references`, `find_class_references`, and
`find_function_references` iterate over `user_file_symbol_maps()`.
The snapshot size is known up front. Report per-file progress
with 100 ms throttling.

### Threading considerations

The progress callback must work from both the async runtime and
`spawn_blocking` threads. Since the callback captures a
`tokio::sync::mpsc::Sender` or uses `std::sync::Mutex` for the
timestamp and token, it can be invoked from any thread. The
actual notification send happens on the async side (the receiver
drains the channel and calls `progress_report`).

A simpler alternative: since GTI and Find References already run
on `spawn_blocking`, the callback can write to a shared
`Arc<AtomicU32>` for completed/total, and a separate
`tokio::spawn` task polls it every 100 ms and sends reports.
This avoids threading async senders into sync code.

---

## X6. Disk cache (evaluate later)

**Goal:** Persist the full index to disk so that restarts don't
require a full rescan.

### When to consider

Only if X4 background indexing is slow enough on cold start that
users complain. Given that:

- Mago can lint 45K files in 2 seconds.
- A regex classmap scan over 21K files should be sub-second.
- Full AST parsing of a few thousand user files should take single
  digit seconds.

...disk caching may never justify its complexity. The primary use
case would be memory savings (load from disk on demand instead of
holding everything in RAM), not startup speed.

### Format options

- `bincode` / `postcard`: simple, small dependency footprint, tolerant
  of struct changes (deserialization fails gracefully instead of
  reading garbage memory). The right default choice.
- SQLite: robust, queryable, but heavier than needed for a flat
  key-value store.

Zero-copy formats like `rkyv` are ruled out. They map serialized bytes
directly into memory as if they were the original structs, which means
any struct layout change between versions reads corrupt data. PHPantom's
internal types change frequently and will continue to do so. A cache
format that silently produces garbage after an update is worse than no
cache at all.

### Invalidation

Store file mtime + content hash per entry. On startup, walk the
directory, compare mtimes, re-parse only changed files. This is
Libretto's `IncrementalCache` approach and it works well.

The content hash must be the authority; mtime is only a pre-filter
to skip hashing files that look unchanged. php-lsp shipped this
exact bug: their cache was keyed on `mtime + size`, so a
size-preserving edit within the same mtime second served a stale
index entry. They later switched to `blake3(uri || content)`.

### Decision criteria

Implement disk caching only if:

1. Full-mode cold start exceeds 10 seconds on a representative large
   codebase, AND
2. The memory overhead of holding the full index exceeds the 512 MB
   target, or users on constrained systems report issues.

If neither condition is met, skip this phase entirely. Simpler is
better.

---

## X7. Recency tracking

**Impact: Medium · Effort: Medium**

The current lazy-loading design provides an implicit recency signal:
classes in `uri_classes_index` were loaded because the developer interacted with
their file during this session (hovered, navigated, completed). Source
tiers 0 (use-imported) and 1 (same-namespace) already capture this
for the current file's neighborhood. The `fqn_uri_index` source captures
cross-file interactions (go-to-definition, hover, or completion that
triggered a load).

This implicit signal works because unloaded classes are in a separate
bucket (classmap/stubs, tier 2) with lower priority. If the project
moves to eager indexing (X4, parsing all files at startup), every class
would appear equally "loaded" and the tier distinction would collapse.
The same-namespace tier would contain every class in the namespace, not
just the ones the developer recently touched.

**When to implement:** Only when eager loading (X4) ships. Until then,
the implicit signal is sufficient and no explicit tracking is needed.

**Design sketch:**

1. **Track accepted completions.** When the editor sends
   `completionItem/resolve` or the next `didChange` contains text
   matching a recently offered completion, record the FQN and a
   timestamp.

2. **Track navigation.** When go-to-definition or hover resolves a
   class, record the FQN.

3. **Score decay.** Use an exponential decay function so that a class
   used 5 minutes ago scores higher than one used 2 hours ago, but
   both score higher than one never interacted with.

4. **Integration with sort key.** The recency score could replace the
   source tier dimension (since tier 0/1/2 distinctions become less
   meaningful with full indexing) or be added as a new dimension
   between affinity and demotion. The sort_text scheme is documented
   in [ARCHITECTURE.md § Class Name Sources and Priority](../ARCHITECTURE.md#class-name-sources-and-priority).

5. **Persistence.** The recency table can be in-memory only (reset on
   server restart). Cross-session persistence is a nice-to-have but
   not essential; the affinity table already provides a good cold-start
   ordering.

---

## X8. Inverted reference index for O(k) find-references

**Impact: Medium-High · Effort: Medium**

Find-references currently scans every span in every file's `SymbolMap`
— O(total\_spans\_in\_project) per request. For a 10K-file project
with ~500 spans/file that is ~5 million comparisons, each involving a
`resolved_names` lookup and string comparison. The result set is
typically tiny (tens of locations), so almost all work is wasted.

php-lsp builds a background reference index (Phase 3) that maps each
symbol's FQN to its reference locations, giving O(k) lookups where
k = number of actual references.

### Data structure

A new inverted index on `Backend`:

```
ref_index: HashMap<String, Vec<(String, u32, u32)>>
```

Key: target FQN (e.g. `App\Models\User`, `App\Models\User::save`).
Value: list of `(file_uri, span_start, span_end)` tuples.

### Maintenance

- **Bulk build.** `ensure_workspace_indexed` already parses every
  file and builds `SymbolMap` entries with resolved names. After that
  loop completes, a second pass over all symbol maps populates the
  inverted index. This is the same work find-references does today,
  but done once instead of on every request.

- **Incremental update.** `update_ast_inner` already rebuilds a
  file's `SymbolMap`. After replacing the symbol map entry, remove
  the old file's contributions from the inverted index and insert the
  new ones. Since entries are keyed by FQN, this is a scan of the
  old/new symbol map for the single changed file — negligible cost.

### Query

`find_class_references`, `find_member_references`, and
`find_function_references` look up the target FQN in the inverted
index. If the index is populated, return the stored locations
directly (converting span offsets to LSP ranges via the file content).
If the index is not yet built (workspace not indexed), fall back to
the current full-scan approach.

### Dependencies

Benefits from X4 (full background indexing) — if the workspace is
indexed eagerly, the inverted index is always warm. Without X4, the
index is built lazily on first find-references and kept warm
afterward.

### When to implement

After X4 ships (the index is most valuable when the workspace is
fully indexed). Can also land independently — the lazy-build fallback
makes it safe to ship without X4.




## X9. Honor editor file excludes and PHP associations during indexing

**Impact: Low-Medium · Effort: Medium**

This task spans both the server and the IDE plugins. The server side
teaches the directory walkers to honor a generic list of exclude globs
and extra PHP extensions. The client side (each editor extension) must
gather the editor's effective `files.exclude` / `files.associations`
and forward them to the server, since only the extension has access to
those editor settings.

The workspace scanners discover files by the `.php` extension and do
not consult any exclude list. Two pieces of information the editor
already has are ignored:

- **`files.exclude` (and a PHPantom-specific exclude glob).** Large
  generated/vendored directories that the user has hidden from the
  editor are still walked and parsed by the indexer. Skipping them
  would cut startup work and avoid indexing irrelevant symbols.
- **`files.associations`.** Files mapped to PHP under a non-`.php`
  extension (e.g. `.module`, `.inc`, `.theme` in Drupal) are not
  discovered by the byte-level scanners, so their classes/functions
  are missing from the index. Note that *open* associated files
  already work, because VS Code reports them with the `php` language
  id and the client's document selector matches on language id, not
  extension. Only background discovery is affected.

### Approach

The client passes the effective exclude globs and the set of
PHP-associated extensions to the server (via `initializationOptions`,
or by responding to `workspace/configuration` the way Intelephense's
middleware merges VS Code's native `files.exclude` /
`files.associations` into the server config). The directory walkers in
`classmap_scanner.rs` and `util.rs` consult the exclude globs before
descending, and treat the extra associated extensions as PHP when
collecting candidate files.

### Editor-agnostic note

Excludes and associations are editor concepts. Keep the server's
interface generic (a list of globs and a list of extensions) so any
client (VS Code, Zed) can supply them, rather than hard-coding
VS Code setting names in the server.
