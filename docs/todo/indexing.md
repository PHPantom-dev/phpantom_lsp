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
  path would filter out the package's own implementations).
  Parallelizing it requires care because it interleaves reads and
  writes through `class_loader` callbacks.
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
to skip hashing files that look unchanged. A peer PHP LSP project
shipped this exact bug: its cache was keyed on `mtime + size`, so a
size-preserving edit within the same mtime second served a stale
index entry. It later switched to `blake3(uri || content)`.

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

## X14. Ask Zed to expose `file_scan_exclusions` and `file_types` to extensions

**Impact: Low · Complexity: Low**

An upstream request, not a code change here. Zed's extension API serves
extensions only the `language`, `lsp`, and `context_servers` settings
categories (the `category` match in `get_settings`, `extension_host`
crate), so the PHP extension cannot read the editor's own
`file_scan_exclusions` or `file_types` and forward them the way the VS
Code extension forwards `files.exclude` and `files.associations`. Zed
users therefore have to restate those two lists under
`lsp.phpantom.initialization_options`, which
[`editor-setup.md`](../editor-setup.md) documents as the workaround.

File the request against `zed-industries/zed` for read access to those
two settings, then link the issue from that section so users can track
it. When it lands, the forwarding itself is a small change in Zed's
official PHP extension, and the manual step in the docs goes away.

## X17. Index the workspace's other folders

**Impact: Medium · Complexity: Medium**

A multi-root workspace is the one case where the editor already knows
about PHP source outside a server's root and can say so without the user
configuring anything, the same way `files.exclude` and
`files.associations` already reach the server. The VS Code extension
starts one server per workspace folder, each scoped to that folder, so a
project whose shared library sits in a sibling folder resolves nothing
across the boundary: the library's classes are missing from completion,
go-to-definition, and find-references in the project that uses them.
Intelephense splits a multi-root workspace the same way and asks the user
to name the sibling explicitly; the folder list is already in front of
the extension, so nothing needs naming.

Forwarded through `ClientIndexingOptions` beside `exclude` and
`extensions`, as a list of absolute directories:

```json
{ "indexing": { "include_paths": ["/home/me/work/shared-lib"] } }
```

`collectIndexFilters` fills it from `vscode.workspace.workspaceFolders`,
dropping the client's own folder, and the extension re-sends on
`onDidChangeWorkspaceFolders` the way it already re-sends on a
`files.exclude` change. Because it travels inside the block the extension
builds in one place, it cannot collide with the `phpantom` section VS
Code's `synchronize` pushes: that blob has no `indexing` key, so
`ClientIndexingOptions::from_client_settings` ignores it and the
whole-value replacement in `set_client_indexing_options` stays safe.

The shape stays generic, so a Zed or Neovim user can hand-write the same
block for a directory their editor has no concept of.

**Not in scope: a PHPantom setting for arbitrary directories.** VS Code
has no general setting that names extra source roots, so anything beyond
the folder list means a `phpantom.*` setting the user types, which brings
its own questions (variable and `~` expansion, a matching
`.phpantom.toml` key so `analyze` sees what the editor sees, whether a
named path overrides `exclude`, out-of-workspace watchers, and a document
selector that covers a folder no client owns). A sibling folder needs
none of that. File it separately if users ask for it.

**Append to the existing roots slice.** `walk_roots` already takes a
roots slice and puts every root in a single `ignore` walk, which is what
compiles each shared ancestor `.gitignore` once instead of once per root.
Include roots belong in that slice, not in a walk of their own. Its
attribute-by-depth accounting holds for a root whose files were reached
through a followed link, pinned by
`walk_roots_attributes_a_followed_link_to_the_root_that_reached_it`.

Putting them in that slice also gets the duplicate protection for free:
`LinkClaims` claims every root up front, so a symlink pointing at an
include folder loses to the include folder's own walk rather than
indexing the tree a second time under the link's spelling.

**Two folders can still overlap.** The guard above only sees symlinks,
and VS Code lets a workspace hold both a folder and its parent. An
include root that is a parent, child, or exact duplicate of another one
(or of the server's own root) is walked twice, because nothing crossed a
link to get there. Canonicalize the include roots when they are
registered and drop the ones contained in another, the same containment
test `LinkClaims::claim` already does.

**Switch every walker together.** `collect_php_files_gitignore`,
`util::collect_php_files`, and `analyse::collect_php_files` each take a
single root, so only `walk_roots` is multi-root today. Leaving the serial
three behind would index classes from an include folder while
find-references, rename, and go-to-implementation never see them, which
is the half-wired state the symlink change avoided by moving all four
walkers at once.

**Fold the "did the inputs widen?" check into one place.** Two entry
points reconcile a live change, `reload_config` and
`did_change_configuration`, and both compare the compiled filters.
Include roots make a second input and a second bespoke comparison at each
site. Replace them with one snapshot of everything that decides what a
walk finds, so the next input is added once.

**Confirm the watchers before building any.** VS Code limits a
string-pattern watcher to paths inside the workspace, and a sibling
folder is inside it, so the `**/*.php` watchers every session registers
should already report a change written in one. Verify that and stop
there. The relative-pattern-per-base machinery followed links need
(`watchable_followed_links`, the per-base list in `WatchedFileInputs`)
exists for trees that sit outside the workspace altogether, which these
do not, and server-side registration for such a path is reported not to
deliver events at all
([vscode-languageserver-node#1783](https://github.com/microsoft/vscode-languageserver-node/issues/1783)).

**Resolution only: no diagnostics, no edits.** The sibling folder has its
own server publishing its own diagnostics, so a second pass from this one
would duplicate every message the user sees on those files. Index an
include root, resolve into it, and leave publishing and workspace-wide
edits (rename, fix) to the server that owns the folder. That also keeps
the extension's per-folder `documentSelector` as it is: a file opened
from an include root is served by its own folder's client.

## X13. Decide how workspace-wide edits treat excluded files

**Impact: Low-Medium · Complexity: Medium**

`collect_php_files_gitignore` feeds find-references and namespace
rename, so an excluded tree is invisible to both: a rename can leave
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
