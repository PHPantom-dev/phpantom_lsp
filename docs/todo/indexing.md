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

Background-parse every user PHP file in the workspace after discovery.
Uses Composer data to guide file discovery when available, falls back
to scanning all PHP files in the workspace when it is not. Populates
the uri_classes_index, symbol_maps, the cross-file reference index,
and all derived indices. Enables workspace symbols, fast
find-references without on-demand scanning, and rich hover on
completion items. Vendor files are not background-parsed; they are
still resolved lazily on demand. Memory usage grows proportionally to
project size. This is the zero-config experience.

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

## X6. Disk cache (evaluate later)

**Goal:** Persist the full index to disk so that restarts don't
require a full rescan.

### When to consider

Only if full background indexing is slow enough on cold start that
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
bucket (classmap/stubs, tier 2) with lower priority. Now that full
indexing is the default (parsing all files at startup), every class
appears equally "loaded" and the tier distinction has collapsed. The
same-namespace tier now contains every class in the namespace, not
just the ones the developer recently touched.

**When to implement:** Eager/full indexing is now the default, so the
tier distinction has already collapsed as described above — this is
ready to implement.

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

## X10. Interactive requests block on the workspace index lock during initial indexing

**Impact: Medium · Effort: Medium**

The full background index holds `workspace_index_lock` for its entire
run. Any request that reaches `ensure_workspace_indexed` during that
window parks on the lock until the whole workspace parse finishes
(tens of seconds on large projects):

- **Laravel string-key completion** (`config('`, `route('`, `view('`,
  `__('`): the key enumerations call `user_file_symbol_maps()`, so
  the first such completion during startup stalls instead of
  returning what is already parsed. Same for the invalid-string-key
  diagnostics pass on open files.
- **Find References / Rename**: these need complete data, so waiting
  is semantically right, but the user gets no signal that the wait is
  the index (the request's own progress token stays at "Resolving…").
- **Go to Implementation** (full strategy, index not yet ready):
  blocks the same way.

Core completion, hover, and go-to-definition are unaffected (they use
lazy per-class loading and never touch the index lock), which is the
main responsiveness guarantee to preserve.

### Direction

Completeness-critical consumers (references, rename, GTI) should keep
waiting but report "waiting for workspace index" through their
progress token. Best-effort consumers (string-key completion, the
string-key diagnostics pass) should not call
`ensure_workspace_indexed` while `full_index_in_progress` is set;
they should serve partial results from the current `symbol_maps`
snapshot and rely on the post-index `workspace/diagnostic/refresh`
to correct anything that was missing. Additionally, the config /
view / translation enumerations are directory-scoped (`config/`,
`resources/views/`, `lang/`) and could be fed by targeted scans that
do not depend on the full workspace parse at all.

Related but separate: the deferred X2 priority-aware scheduling
covers the CPU-contention side (index workers saturating all cores);
this item covers the lock-blocking side.

---

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
