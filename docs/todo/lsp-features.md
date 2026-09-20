# PHPantom — LSP Features

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## F2. Partial result streaming via `$/progress`

**Impact: Medium · Complexity: Medium-High**

The LSP spec (3.17) allows requests that return arrays — such as
`textDocument/implementation`, `textDocument/references`,
`workspace/symbol`, and even `textDocument/completion` — to stream
incremental batches of results via `$/progress` notifications when both
sides negotiate a `partialResultToken`. The final RPC response then
carries `null` (all items were already sent through progress).

This would let PHPantom deliver the _first_ useful results almost
instantly instead of blocking until every source has been scanned.

### Streaming between existing phases

`find_implementors` already runs five sequential phases (see
`docs/ARCHITECTURE.md` § Go-to-Implementation):

1. **Phase 1 — uri_classes_index** (already-parsed classes in memory) — essentially
   free. Flush results immediately.
2. **Phase 2 — fqn_uri_index** (FQN → URI entries not yet in uri_classes_index) —
   loads individual files. Flush after each batch.
3. **Phase 3 — classmap files** (Composer classmap, user + vendor mixed)
   — iterates unique file paths, applies string pre-filter, parses
   matches. This is the widest phase and the best candidate for
   within-phase streaming (see below).
4. **Phase 4 — embedded stubs** (string pre-filter → lazy parse) — flush
   after stubs are checked.
5. **Phase 5 — PSR-4 directory walk** (user code only, catches files not
   in the classmap) — disk I/O + parse per file, good candidate for
   per-file streaming.

Each phase boundary is a natural point to flush a `$/progress` batch,
so the editor starts populating the results list while heavier phases
are still running.

### Prioritising user code within Phase 3

Phase 3 iterates the Composer classmap, which contains both user and
vendor entries. Currently they are processed in arbitrary order. A
simple optimisation: partition classmap file paths into user paths
(under PSR-4 roots from `composer.json` `autoload` / `autoload-dev`)
and vendor paths (everything else, typically under `vendor/`), then
process user paths first. This way the results most relevant to the
developer arrive before vendor matches, even within a single phase.

### Granularity options

- **Per-phase batches** (simplest) — one `$/progress` notification at
  each of the five phase boundaries listed above.
- **Per-file streaming** — within Phases 3 and 5, emit results as each
  file is parsed from disk instead of waiting for the entire phase to
  finish. Phase 3 can iterate hundreds of classmap files and Phase 5
  recursively walks PSR-4 directories, so per-file flushing would
  significantly improve perceived latency for large projects.
- **Adaptive batching** — collect results for a short window (e.g. 50 ms)
  then flush, balancing notification overhead against latency.

### Applicable requests

| Request                       | Benefit                                                                         |
| ----------------------------- | ------------------------------------------------------------------------------- |
| `textDocument/implementation` | Already scans five phases; each phase's matches can be streamed                 |
| `textDocument/references`     | Will need full-project scanning; streaming is essential                         |
| `workspace/symbol`            | Searches every known class/function; early batches feel instant                 |
| `textDocument/completion`     | Less critical (usually fast), but long chains through vendor code could benefit |

### Implementation sketch

1. Check whether the client sent a `partialResultToken` in the request
   params.
2. If yes, create a `$/progress` sender. After each scan phase (or
   per-file, depending on granularity), send a
   `ProgressParams { token, value: [items...] }` notification.
3. Return `null` as the final response.
4. If no token was provided, fall back to the current behaviour: collect
   everything, return once.

---

## F7. Evaluatable expression support (DAP integration)

**Impact: Low-Medium · Complexity: Low**

Implement `textDocument/evaluatableExpression` so debuggers (Xdebug
via DAP) can evaluate expressions under the cursor during a debug
session. Given a cursor position, the handler returns the expression
text and range that the debugger should evaluate in the running PHP
process.

### Supported expression kinds

- **Variables**: `$var` — return the variable name and its span.
- **Property access**: `$obj->prop`, `$this->prop` — return the full
  member access expression.
- **Array access**: `$arr[0]`, `$arr['key']` — return the full
  subscript expression including brackets.
- **Static property access**: `Foo::$bar` — return the full expression.
- **Parameters**: function/method parameters at declaration sites.

### Why this is cheap

The symbol map already identifies all of these constructs with precise
byte ranges. The handler is a thin layer: look up the `SymbolSpan` at
the cursor position, check that it's a variable, member access, or
subscript expression, and return the source text and range. No type
resolution needed.

### What this enables

When a user is debugging PHP with Xdebug and hovers over `$user->name`
in their editor, the editor asks the LSP "what expression is here?"
and forwards it to the debug adapter for evaluation. Without this
handler, the editor falls back to selecting the word under the cursor,
which gives `name` instead of `$user->name` — useless for the
debugger.

---

## F11. VS Code extension

| Field      | Value                    |
| ---------- | ------------------------ |
| **Impact** | High                     |
| **Complexity** | Medium-High          |

Create a VS Code extension that bundles PHPantom and publishes it to
the VS Code Marketplace.

### Approach

Fork the [vscode-intelephense](https://github.com/bmewburn/vscode-intelephense)
client extension (MIT-licensed). Intelephense is the #1 PHP extension
in the VS Code Marketplace, so its `package.json` represents what
PHP developers expect from an extension: the settings schema,
activation events, file associations, categories, and contribution
points are battle-tested. Starting from this base means we do not
accidentally omit something users take for granted.

Strip the proprietary Intelephense server dependency (`intelephense`
npm package) and replace it with PHPantom binary management. The
extension is a thin TypeScript wrapper around `vscode-languageclient`
that spawns `phpantom_lsp` over stdio.

**Cleanup process:** After forking, compare the result against a
fresh VS Code extension scaffold (`yo code` generator) to identify
and remove Intelephense-specific legacy that does not apply to
PHPantom (licence key commands, telemetry integration, Node.js
runtime configuration, premium feature gating). The goal is a clean
extension that inherits the right UX expectations without carrying
over implementation baggage.

### Scope

1. **Binary distribution.** Bundle or auto-download the correct
   pre-built binary for each platform (linux-x64, linux-arm64,
   darwin-x64, darwin-arm64, win-x64). Use GitHub Releases as the
   download source.
2. **Settings surface.** Expose PHPantom's `.phpantom.toml` settings
   as VS Code settings (PHP version, diagnostics toggles, indexing
   strategy).
3. **Status bar.** Show indexing progress and server status.
4. **Marketplace listing.** Icon, description, screenshots,
   categories, keywords.
5. **CI.** GitHub Actions workflow to build, test, and publish the
   extension on release.

### Code signing

macOS and Windows builds must be signed so the OS
stops flagging PHPantom as malware. This is a prerequisite for the
VS Code extension (users will not trust an extension that triggers
Gatekeeper or SmartScreen warnings).

- **macOS:** Apple Developer ID certificate, `codesign`, and
  `notarytool` in the release CI workflow.
- **Windows:** Authenticode certificate (or Azure Trusted Signing)
  and `signtool` in the release CI workflow.

---

## F12. IntelliJ / PHPStorm plugin

| Field      | Value                    |
| ---------- | ------------------------ |
| **Impact** | High                     |
| **Complexity** | Medium-High          |

Create an IntelliJ plugin that depends on
[LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) and
bundles PHPantom. Publish it to the JetBrains Marketplace. Works in
all IntelliJ-based IDEs (PHPStorm, IntelliJ IDEA, WebStorm, etc.).

### Approach

Fork [clojure-lsp-intellij](https://github.com/clojure-lsp/clojure-lsp-intellij)
(MIT-licensed). It is a Kotlin/Gradle plugin that registers a
language server via lsp4ij's `com.redhat.devtools.lsp4ij.server`
extension point. Strip the Clojure-specific parts and replace them
with PHPantom:

- Register PHPantom as the language server in `plugin.xml`.
- Map the `PHP` language and file type via
  `com.redhat.devtools.lsp4ij.languageMapping`.
- Bundle or auto-download the PHPantom binary.
- Add a settings page for the binary path and any PHPantom-specific
  options.

### Scope

1. **`plugin.xml` registration.** Server definition, language
   mapping, file type mapping (`.php`, `.phtml`, `.inc`).
2. **Binary management.** Auto-download from GitHub Releases on
   first run, with a manual path override in settings.
3. **Settings UI.** Binary path, PHP version override, diagnostic
   toggles.
4. **JetBrains Marketplace listing.** Icon, description, plugin
   compatibility range (2024.2+, matching lsp4ij's requirement).
5. **CI.** GitHub Actions workflow using `gradlew buildPlugin` and
   `gradlew publishPlugin`.

### Why not use the built-in IntelliJ LSP API

IntelliJ's native LSP support (since 2023.2) is only available in
Ultimate editions and is still limited in capability. LSP4IJ is free,
works in all editions (including Community), and supports a broader
set of LSP features. Using lsp4ij also means the plugin works in
IntelliJ IDEA (for PHP projects opened there) and other JetBrains
IDEs, not just PHPStorm.

---

## F13. Homebrew formula

| Field      | Value                    |
| ---------- | ------------------------ |
| **Impact** | Medium                   |
| **Complexity** | Low                  |

Create a Homebrew formula for PHPantom so users on macOS and Linux
can install it with `brew install phpantom_lsp`.

### Approach

Submit a PR to [homebrew-core](https://github.com/Homebrew/homebrew-core)
with a formula that downloads the pre-built binary from GitHub
Releases for the current platform. Alternatively, the formula can
build from source using `cargo install` if the Homebrew reviewers
prefer source builds (common for Rust projects).

### Formula contents

- **Homepage:** `https://github.com/PHPantom-dev/phpantom_lsp`
- **Source:** GitHub Releases tarball or `cargo install` from crates.io.
- **Binary:** `phpantom_lsp`
- **Test block:** `system bin/"phpantom_lsp", "--version"`

### Why this matters

A Homebrew formula is a prerequisite for upstream PRs to editors like
Helix, which prefer that language servers be installable via a
package manager. It also simplifies the VS Code extension's binary
management on macOS (detect Homebrew-installed binary before
downloading).

---

## F14. Helix upstream PR

| Field      | Value                    |
| ---------- | ------------------------ |
| **Impact** | Low-Medium               |
| **Complexity** | Low                  |

**Depends on:** F13 (Homebrew formula).

Submit a PR to the [Helix editor](https://github.com/helix-editor/helix)
adding `phpantom_lsp` as a language server option in the default
`languages.toml`.

### Change

Add a `phpantom` server definition and include it in the `php`
language entry (alongside `intelephense`):

```toml
[language-server.phpantom]
command = "phpantom_lsp"

# In the [[language]] entry for php, add "phpantom" to language-servers.
```

### Prerequisites

- F13 (Homebrew formula) should be merged so Helix maintainers can
  point users at `brew install phpantom_lsp`.
- Helix maintainers may want a brief README section documenting the
  server and its feature set.

## F15. Go-to-declaration

**Impact: Low-Medium · Complexity: Low**

Implement `textDocument/declaration` to jump from a concrete method to
its abstract or interface prototype, complementing the existing
go-to-definition (which jumps to the concrete implementation) and
go-to-implementation (which jumps from an interface to concrete classes).

### Behaviour

When the cursor is on a method call or method name:

1. Search for an **interface or abstract class** that declares a method
   with the same name and is in the inheritance chain of the resolved
   class.
2. If found, jump to the interface/abstract method declaration.
3. If no abstract prototype exists, fall back to the same result as
   go-to-definition.

### Implementation

The existing `resolve_implementation` already does reverse lookups
(concrete → prototype) via `resolve_reverse_implementation`. The
declaration handler can reuse this: for `MemberAccess` and
`MemberDeclaration` symbols, call the reverse-implementation resolver
first. For class-level symbols, declaration and definition are the
same.

Register `declaration_provider` in `server.rs` and wire it to a thin
handler that delegates to the existing infrastructure.

## F16. On-type `}` brace de-indent

**Impact: Low · Complexity: Low**

Extend the existing on-type formatting handler (currently triggered on
`\n` for docblock generation) to also trigger on `}`, automatically
de-indenting the closing brace to match its opening `{`.

### Behaviour

When the user types `}`:

1. From the `}` position, scan backward through the document text to
   find the matching `{` (tracking brace depth, skipping strings and
   comments).
2. Read the indentation of the line containing the matching `{`.
3. If the `}` line has more indentation than the `{` line, return a
   `TextEdit` that replaces the leading whitespace on the `}` line
   with the `{` line's indentation.

This is a pure text-based operation — no AST needed. Register `}` as
an additional `on_type_formatting_trigger_character` alongside the
existing `\n`.

## F17. Wire class move to `workspace/willRenameFiles`

**Impact: Medium · Complexity: Medium**

Renaming a class's FQN via `textDocument/rename` already moves the file
and rewrites references across the project: renaming a class's
declaration accepts the full FQCN so it can move between namespaces in
one step, and renaming a namespace segment rewrites every affected
`namespace` declaration, `use` statement, and FQN reference while
moving the PSR-4 directories to match (see `build_class_move_edit` in
`src/rename/class.rs` and `build_namespace_rename_edit` in
`src/rename/namespace.rs`). What's still missing is the editor-triggered
path: when the user renames or moves a PHP file in the editor's file
tree (rather than through the LSP rename command), nothing updates the
file's `namespace` declaration or the workspace's `use` imports.

Wire the existing move logic to `workspace/willRenameFiles` (declared
via server capabilities `workspace.fileOperations.willRename`): on a
file-tree rename/move, recompute the namespace from the destination
path using the PSR-4 autoload map, and reuse the same reference-rewrite
machinery to produce the `WorkspaceEdit`. The companion
`workspace/willCreateFiles` can then insert a PSR-4-derived `namespace`
+ class stub into newly created files.

**References:**
- Phpactor: `MoveClass` refactoring in the class-mover package.

---

## F19. Connect to a remote/TCP language server (VS Code extension)

**Impact: Low · Complexity: Medium**

This task is for the VS Code extension package, not the `phpantom_lsp`
server itself. The server can already speak LSP over a TCP socket; the
gap is purely on the client side, where the editor extensions only ever
spawn a local binary over stdio. Expose an option in the extension
(mirroring Phpactor's `remote.enabled` / `remote.host` / `remote.port`)
to connect to an already-running server instead of spawning one. This
covers running the server inside a container or on a remote host while
editing locally.

### Scope

This is a client-side change in the editor extensions, not the server.
In the VS Code extension, add `phpantom.remote.enabled`, `.host`, and
`.port` settings; when enabled, build the language client from a socket
transport rather than a spawned process. Remote mode is a single shared
endpoint, so it bypasses the per-folder server model and uses one
client that matches all PHP documents (the same exception Phpactor's
extension makes).

### Caveats

A remote server has its own filesystem view, so `rootUri` / workspace
paths must line up with the paths the server sees (or be remapped).
Auto-download, version checks, and the per-folder rooting do not apply
in remote mode.

## F20. Migrate to the maintained `tower-lsp` fork

**Impact: Low-Medium · Complexity: Very High**

`tower-lsp` 0.20 (our current dependency) is the last release of the
original crate; it's unmaintained upstream. A maintained fork exists
as `tower-lsp-server` (types crate `ls-types`), actively developed as
of 2026. Because it's a rename rather than a version bump of the same
crate, `cargo update`/routine dependency audits will not surface this
on their own — nothing shows up as "outdated" since no new `tower-lsp`
version is being withheld. It has to be picked up as a deliberate
migration.

**Does not unblock A16 or F21.** Checked directly against `ls-types`
`main` and upstream `lsp-types` 0.97.0 source (not just docs): neither
crate implements `SnippetTextEdit`/`StringValue` (tracked upstream at
[gluon-lang/lsp-types#310](https://github.com/gluon-lang/lsp-types/issues/310),
still open) or a static `type_hierarchy_provider` field on
`ServerCapabilities` (tracked at
[gluon-lang/lsp-types#298](https://github.com/gluon-lang/lsp-types/issues/298)
and [tower-lsp-community/ls-types#38](https://github.com/tower-lsp-community/ls-types/issues/38),
both open). `ls-types` also removed its generic `"proposed"` 3.18
feature flag in 0.0.4 ("only applied to a handful of v3.18 items"), so
there is no version bump or feature flag on our side that grants either
type today. Both are real 3.18-spec (`@proposed`) features, just not
yet implemented in any Rust LSP-types crate. The remaining motivation
for this migration is staying on an actively maintained crate — bug
and security fixes, and a path to 3.18 support once upstream catches
up — not unblocking a specific feature now. Re-check A16 and F21 for
upstream progress before assuming this migration alone resolves them.

**The real complexity driver:** `ls-types`'s `Uri` is a newtype over
`fluent_uri::Uri<String>`, not `url::Url`. Our code uses `Url`
(re-exported from `lsp_types`) directly in roughly 90 files across
nearly every module — path manipulation, `to_file_path`/
`from_file_path`, `.path()`, `.join()`, and more — and `fluent_uri`'s
API does not mirror `url::Url`'s. This is a project-wide port of the
document-URI type, not a mechanical import rename. Scope it file by
file before committing to a single PR; it may need a preparatory
abstraction (e.g. isolate URI construction/parsing behind a narrow
internal helper) to keep the blast radius reviewable, and likely
warrants breaking into more than one PR despite the "one task per PR"
convention — raise that with the maintainer before starting.

**What to also check:** the fork's public API surface relative to
`tower_lsp::LspService`/`tower_lsp::lsp_types` (import paths, trait
signatures) to scope the mechanical rename across every file that does
`use tower_lsp::...` (grep `tower_lsp::` for the full list:
`src/lsp_dispatch.rs`, `src/inlay_hints.rs`, `src/document_symbols.rs`,
`src/folding.rs`, `src/phpcs.rs`, `src/fix.rs`,
`src/selection_range.rs`, `src/text_position.rs`, and others), plus the
wire-protocol test harness described in `test-porting.md` Phase 6B if
that gets ported around the same time.

**Where to look:** `Cargo.toml`'s `tower-lsp = { version = "0.20", features = ["proposed"] }`.

## F21. Static `typeHierarchyProvider` advertisement (depends on F20)

**Impact: Low-Medium · Complexity: Low**

Type hierarchy (`textDocument/prepareTypeHierarchy`,
`typeHierarchy/supertypes`, `typeHierarchy/subtypes`) is fully
implemented (`src/type_hierarchy.rs`) and registered dynamically via
`client/registerCapability` in `initialized` (`server.rs`,
`type_hierarchy_registration()`), gated on the client declaring
`textDocument.typeHierarchy.dynamicRegistration: true`. This works for
every client that supports dynamic registration, but there is no
static fallback: `lsp-types` 0.94.1 (pinned by `tower-lsp` 0.20, see
F20) has no `type_hierarchy_provider` field on `ServerCapabilities`, so
a client that supports type hierarchy without dynamic registration —
or any tool that inspects only the `initialize` response's static
capabilities, such as a feature-conformance probe — sees no type
hierarchy support at all, even though the feature works end-to-end for
a real editor that does the dynamic-registration round trip.

The field is still missing after F20, not just before it: neither
upstream `lsp-types` nor the maintained `tower-lsp-server`/`ls-types`
fork that F20 migrates to has added `type_hierarchy_provider` yet
(tracked at
[gluon-lang/lsp-types#298](https://github.com/gluon-lang/lsp-types/issues/298)
and [tower-lsp-community/ls-types#38](https://github.com/tower-lsp-community/ls-types/issues/38),
both open) — so F20 landing is necessary but not sufficient here; this
also needs the upstream crate to add the field. Once both have
happened, add static advertisement (`Boolean(true)` or
`TypeHierarchyOptions`) in `initialize`'s `ServerCapabilities`,
conditional on the client *not* declaring `dynamicRegistration: true`
for type hierarchy (avoid double-registering: send either the static
capability or the dynamic registration, not both, per the client's
declared support). Also
verify whether the same version bump exposes `diagnostic` client
capabilities more precisely — pull diagnostics (`diagnostic_provider`
in `server.rs`) is unaffected by this gap (it's already advertised
correctly whenever the client declares `textDocument.diagnostic`,
verified by probing `initialize` directly with that capability set),
but is worth a quick re-check after the migration in case the newer
`lsp-types` changes the shape of that capability struct.

**Where to look:** `src/server.rs` (`initialize`, `type_hierarchy_registration`),
`src/type_hierarchy.rs`.

---

## F22. Merge a namespace onto one that shares a class name

**Impact: Low-Medium · Complexity: Medium-High**

Renaming `App\Internal` to an `App\Support` that already exists now
merges: the files move into the existing directory one at a time
instead of the source directory being renamed on top of the
destination. Where the two namespaces declare the same class name
(`App\Internal\Helper` and `App\Support\Helper` both exist), the whole
rename is refused with a message naming the clash, because the merge
has no well-defined answer for that name — see
`namespace_merge_conflict` in `src/rename/namespace.rs`.

Refusing is the safe answer, not the desirable one: a user merging
twenty files should not be blocked by one clash. What is missing is a
partial merge that moves everything except the clashing names and
leaves those behind *intact*. "Intact" is the hard part and the reason
this is deferred rather than done:

- The clashing class's file must keep its own `namespace` declaration,
  so that file's namespace-declaration edit has to be suppressed while
  its siblings' are kept. Today the namespace rename rewrites by
  string prefix over raw text with no per-class granularity.
- Every reference to the clashing FQN (`use App\Internal\Helper;`,
  `\App\Internal\Helper`, and unqualified `Helper` inside the old
  namespace) must be left pointing at the old name. Rewriting them is
  what makes a naive partial merge worse than a refusal: the reference
  silently resolves to the *other* class.
- A file holding both a clashing class and a non-clashing one cannot
  half-move, so it has to be reported as a clash too.
- The classes that do move out of the old namespace stop being
  namespace-siblings of the ones left behind, so the files left behind
  need `use` imports added for them — the same rule
  `build_class_move_edit` already applies for a single class move.

The refusal message should then list only the names that could not
move, and the response should still carry the edits for everything
that could.

**Where to look:** `src/rename/namespace.rs`
(`namespace_merge_conflict`, `build_namespace_prefix_rename_edit`,
`collect_merge_move_ops`), `src/rename/class.rs` (the import-adding
rule to mirror).

## F23. Rename a class through its YAML/XML occurrences

**Impact: Medium · Complexity: Medium**

A fully-qualified class name in a YAML or XML file is found by Find
References and counted by the declaration CodeLens, but rename leaves
it alone: `find_references_inner` drops resource locations in
`ReferenceSearchMode::Rename`. Renaming the class therefore leaves the
config pointing at a name that no longer exists, and `move` reports the
leftover through its residual scan rather than fixing it.

The blocker is that the text at such an occurrence is not always the
PHP spelling of the name. A YAML double-quoted scalar writes
`"App\\Handler\\Run"`, so the span covers doubled separators; an XML
attribute may carry entity references. Rename plans a single
replacement string per location and verifies each range spells a whole
PHP name token before emitting anything, so an escaped occurrence both
fails verification (dropping the *entire* rename, including its PHP
edits, which is what the current exclusion prevents) and would be
rewritten with single separators, corrupting the document's quoting.

What is needed is a per-occurrence replacement: the scanner already
knows the raw text it normalised, so it can record how the name was
escaped and let the rename re-escape the replacement the same way,
with verification asking for "a name token in this document's
escaping" rather than a bare PHP one.

**Where to look:** `src/resource_navigation.rs` (`scan_symbols`,
`normalize_fqn`), `src/references/dispatch.rs`
(`find_references_inner`), `src/rename/validate.rs`
(`Expected`, `is_name_token`), `src/rename/class.rs`
(`build_class_move_edit`).
