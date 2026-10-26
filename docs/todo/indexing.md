# PHPantom — Indexing

This document covers how PHPantom discovers, parses, and caches class
definitions across the workspace. The goal is to remain fast and
lightweight by default while offering progressively richer modes for
users who want exhaustive workspace intelligence.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

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
filtering. Hidden directories are skipped automatically, and so are the
laravel-ide-helper files. Paths listed under `[indexing] include` are
scanned on top of that walk, bypassing all three, for first-party PHP
in a dotted directory and for generated files that are gitignored as
build artefacts. An included path inside the workspace is project code;
one outside it is registered like a vendor directory, so it resolves
but is never analysed or edited.

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

Always build the classmap ourselves: scan every PHP file under the
workspace root, ignoring `autoload_classmap.php` and the PSR-4
mappings. Vendor packages are still scanned via `installed.json`.
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

`ensure_workspace_indexed` (`src/indexing/preload.rs`, used by find
references) parses files in parallel via
`parse_files_parallel_with_progress` (`(uri, Option<content>)` pairs)
and `parse_paths_parallel_with_progress` (`(uri, PathBuf)` pairs, read
from disk). Both run on the shared pull-index pool in `src/parallel.rs`
(`map_indexed`): workers claim the next file as they finish the last,
the pool is capped at `available_parallelism()`, and never spawns more
workers than there are files.

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
Three parallel helpers in `classmap_scanner/discovery.rs` cover the
three scan modes: `scan_files_parallel_classes` (plain classmap),
`scan_files_parallel_psr4` (PSR-4 with FQN filtering), and
`scan_files_parallel_full` (classes + functions + constants). Small
batches (≤ 4 files) skip threading overhead.

The byte-level PHP scanner (`find_classes`, `find_symbols`) uses
`memchr` SIMD acceleration to skip line comments, block comments,
single-quoted strings, double-quoted strings, and heredocs/nowdocs
instead of scanning byte-by-byte. This reduces per-file scanning time
for files with large docblocks or string literals.

`read_for_scan` (`classmap_scanner/mod.rs`) now reads file bytes
adaptively: files at or above 256 KiB are memory-mapped so the OS page
cache is shared without a heap copy, and smaller files (the vast
majority of source files) are read directly into a heap buffer, since
mapping a small file costs more in page-fault and lock overhead than
copying it. This closed the gap identified in earlier profiling, where
mapping every file regardless of size held a process-wide lock that
serialized concurrent readers.

### Remaining work

The following are deferred to a later sprint:

- **Priority-aware scheduling.** Interactive requests (completion,
  hover, go-to-definition) should preempt batch work. Currently all
  threads run at equal priority.
- **Parallel classmap scanning in `find_implementors`.** Once the
  workspace index is ready (the default `"full"` strategy after
  startup), `find_implementors` answers entirely from the `gti_index`
  reverse-inheritance index and never reaches the sequential classmap
  scan below. Phase 3 (reading and parsing classmap files one at a
  time) only still runs for `"composer"`/`"self"`/`"none"` strategies,
  for a request that arrives before the background index finishes, or
  for a target that lives under `/vendor/` (where the index-ready fast
  path would filter out the package's own implementations). The
  candidate files are then parsed one at a time (`parse_and_cache_file`
  in a loop in `src/definition/implementation.rs`). Parallelizing it
  requires care because it interleaves reads and writes through
  `class_loader` callbacks.
- **Parallel autoload file scanning.** The `scan_autoload_files` work
  queue (`src/indexing/scan.rs`) is inherently sequential due to `require_once` chain
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

## X7. Recency tracking

**Impact: Medium · Complexity: Medium-High**

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

## X12. Say when an exclude hid the class a diagnostic names

**Impact: Low-Medium · Complexity: Medium-High**

An over-broad `[indexing] exclude` makes real classes unresolvable,
and the resulting `Class 'App\Generated\Foo' not found` is
indistinguishable from a typo: the user's own configuration caused
it and nothing says so, which reads as a PHPantom bug. When class
resolution fails, the FQN maps to a PSR-4 path, and that file exists
on disk but `is_excluded_path` matches it, extend the message with
the cause ("defined in `src/Generated/Foo.php`, which `[indexing]
exclude` skips"). The PSR-4 mapping makes the probe a single stat
plus an already-compiled glob match on the failure path only;
classmap-only projects get no probe rather than a disk walk.

## X16. Composer's own class lists bypass `[indexing] exclude`

**Impact: Low-Medium · Complexity: Low**

Every workspace walker honours `exclude`, but three sources write
straight into `fqn_uri_index` without consulting it: the Composer
classmap (`parse_autoload_classmap`, used under `strategy = "composer"`
and `"none"`), the PSR-0 map (`parse_autoload_namespaces`), and the
bootstrap classes `vendor/composer` requires before any autoloader
exists. A class any of those name stays resolvable from a path the user
excluded, so `exclude` means one thing for a file a walk found and
another for a file a list named.

It also costs the mid-session reconciliation some precision. The
eviction pass applies only the *change* (a file both the old and the new
filters exclude is left alone) exactly so an unrelated settings edit
cannot drop these entries, which leaves one asymmetry: excluding such a
path mid-session evicts it, while a restart would index it again.
Filtering the three where they merge into the index makes one rule out
of two and removes the asymmetry; the cost is one already-compiled glob
match per entry, skipped outright when no exclude is configured.

## X14. Forward Zed's `file_scan_exclusions` and `file_types`

**Impact: Low · Complexity: Low**

Waiting on upstream review. Zed's extension API serves extensions only
the `language`, `lsp`, and `context_servers` settings categories, so the
PHP extension cannot read the editor's own `file_scan_exclusions` or
`file_types` and forward them the way the VS Code extension forwards
`files.exclude` and `files.associations`. Zed users therefore have to
restate those two lists under `lsp.phpantom.initialization_options`,
which [`editor-setup.md`](../editor-setup.md) documents as the
workaround.

[zed-industries/zed#66121](https://github.com/zed-industries/zed/pull/66121)
adds `WorktreeSettings::for_worktree` (`file_scan_exclusions` and
`file_scan_inclusions` as their source globs) and
`FileTypeSettings::for_worktree` (the effective `file_types` map) to
extension API 0.8.0, and the Zed section of `editor-setup.md` links it.
Zed is slow to review, so nothing here is scheduled until it merges.
What follows it:

- **A pull request to `zed-extensions/php`**, once a Zed release ships
  the API. Bump `zed_extension_api`, read both settings in
  `language_server_initialization_options`, and merge them into
  `indexing.exclude` and `indexing.extensions` without overwriting what
  the user set there. An exclusion that an inclusion overrides must not
  be forwarded. A `file_types` entry for `PHP` contributes only when it
  is a bare `*.ext` glob, the same rule the VS Code extension applies to
  `files.associations`.
- **Drop the manual step** from the Zed section of `editor-setup.md`
  once that extension release is out.

## X18. Watch `[indexing] include` and stub paths outside the workspace

**Impact: Low-Medium · Complexity: Medium**

An include path outside the workspace is indexed at startup and then
never refreshed. The client only watches its workspace folders, so an
edit to a shared library at `/opt/company/lib` leaves stale symbols, and
a class added there stays unknown, until the editor restarts. Paths
inside the workspace are covered by the `**/*.php` watchers every
session registers, and the exclude exemption `IndexFilters` gives a
named path keeps their events from being dropped.

Followed symlinks have the same problem and already solve it:
`build_watched_file_registration` adds a
[`RelativePattern`](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#relativePattern)
watcher based at each link. Add the outside include paths to that list,
then check against VS Code that a server-registered relative pattern
outside the workspace actually delivers events
([vscode-languageserver-node#1783](https://github.com/microsoft/vscode-languageserver-node/issues/1783)
reports that it does not). If it does not, fall back to a server-side
`notify` watcher for these few named paths.

Stub paths outside the workspace (`[stubs] paths` and the editor's stub
directory) have the same gap and take the same watchers. An event under
one already sets `stubs_changed` in `classify_watched_changes`, which
rebuilds the stub index, so only the registration is missing.

## X13. Decide how workspace-wide edits treat excluded files

**Impact: Low-Medium · Complexity: Medium**

Find-references walks via `collect_workspace_index_files_gitignore` and
namespace rename via `collect_php_files_gitignore`; both apply the
index filters, so an excluded tree is invisible to both: a rename can leave
excluded code calling a name that no longer exists, with no warning.
That is consistent with what an exclude means, but sharper than
"background discovery" suggests. Options: (a) keep the behaviour and
document it under the `exclude` setting; (b) let correctness
operations (rename, find-references) ignore `[indexing] exclude` on
the grounds that the setting is an index-size and startup-time tool,
not a statement that the code does not exist; (c) keep the exclusion
but include a warning in the rename response when excludes are
configured. Maintainer's call on the semantics; whichever way it
goes, the chosen behaviour needs a test and a sentence in
[`configuration.md`](../configuration.md).
