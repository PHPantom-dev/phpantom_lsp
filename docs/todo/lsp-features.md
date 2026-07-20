# PHPantom — LSP Features

Items are ordered by **impact** (descending), then **effort** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Effort** | **Low** (≤ 1 day), **Medium** (2-5 days), **Medium-High** (1-2 weeks), **High** (2-4 weeks), **Very High** (> 1 month) |

---

## F2. Partial result streaming via `$/progress`

**Impact: Medium · Effort: Medium-High**

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

## F3. Incremental text sync

**Impact: Low-Medium · Effort: Medium**

PHPantom uses `TextDocumentSyncKind::FULL`, meaning every
`textDocument/didChange` notification sends the entire file content.
Switching to `TextDocumentSyncKind::INCREMENTAL` means the client sends
only the changed range (line/column start, line/column end, replacement
text), reducing IPC bandwidth for large files.

The practical benefit is bounded: Mago requires a full re-parse of the
file regardless of how the change was received, so the saving is purely
in the data transferred over the IPC channel. For files under ~1000
lines this is negligible. For very large files (5000+ lines, common in
legacy PHP), sending 200KB on every keystroke can become noticeable.

**Implementation:**

1. **Change the capability** — set `text_document_sync` to
   `TextDocumentSyncKind::INCREMENTAL` in `ServerCapabilities`.

2. **Apply diffs** — in the `did_change` handler, apply each
   `TextDocumentContentChangeEvent` to the stored file content string.
   The events contain a `range` (start/end position) and `text`
   (replacement). Convert positions to byte offsets and splice.

3. **Re-parse** — after applying all change events, re-parse the full
   file with Mago as today. No incremental parsing needed initially.

**Relationship with partial result streaming (F2):** These two features
address different performance axes. Incremental text sync reduces the
cost of _inbound_ data (client to server per keystroke). Partial result
streaming (F2) reduces the _perceived latency_ of _outbound_ results
(server to client for large result sets). They are independent and can
be implemented in either order, but if both are planned, incremental
text sync is lower priority because full-file sync is rarely the
bottleneck in practice. Partial result streaming has a more immediate
user-visible impact for go-to-implementation, find references, and
workspace symbols on large codebases.

---



## F5. Call hierarchy

**Impact: Medium · Effort: Medium**

Implement `callHierarchy/incomingCalls` and
`callHierarchy/outgoingCalls` to answer "who calls this function?" and
"what does this function call?"

### Incoming calls (who calls this)

Given a function or method, find all call sites across the project.
This is conceptually similar to Find References but filtered to call
expressions and structured as a tree (each caller is itself a callable
with a location).

The existing Find References infrastructure
(`find_references_in_file`, cross-file scanning) provides the core
search. The call hierarchy handler wraps the results into
`CallHierarchyIncomingCall` items, grouping by containing function.

### Outgoing calls (what does this call)

Given a function or method, walk its AST body and collect all call
expressions (function calls, method calls, static calls, `new`
expressions). Resolve each callee to its declaration location.

This is a single-file AST walk with cross-file resolution for each
callee, similar to what go-to-definition already does.

### Prepare

`callHierarchy/prepare` returns a `CallHierarchyItem` for the symbol
at the cursor. This is straightforward: resolve the symbol, return its
name, kind, URI, range, and selection range.

### Dependencies

Call hierarchy benefits significantly from a full project index.
Without an index, incoming calls can only be found via the existing
classmap + PSR-4 scan approach (same as Find References). Now that
full background indexing is available, the lookup can become a
simple index query instead of relying on the scan-based approach that
Find References uses on its own.

**References:**
- php-lsp: `src/navigation/call_hierarchy.rs` in its own repo — a
  working Rust implementation (prepare/incoming/outgoing, cross-file)
  built on the same "wrap Find References + walk the body" shape
  described above. Their wire-protocol tests (`tests/`, call-hierarchy
  cases in the Symfony suite) show the expected item/range semantics
  editors rely on.
- Phpactor: call hierarchy via its references index.

## F7. Evaluatable expression support (DAP integration)

**Impact: Low-Medium · Effort: Low**

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

## F8. Test ↔ implementation navigation via `@covers`

**Impact: Low · Effort: Medium**

Provide bidirectional navigation between a test class and the class it
tests, using PHPUnit's `@covers` / `@coversClass` / `#[CoversClass]`
annotations as the linking mechanism.

### Why not path-based mapping

Pattern-based approaches (e.g. `src/Foo.php` → `tests/FooTest.php`)
assume a project follows a specific directory convention. Many projects
don't: tests may live under `tests/Feature/`, `tests/Functional/`,
or in a completely separate directory structure. The `@covers` tag is
an explicit, project-layout-independent link that works for any
structure.

### From test → subject

When the cursor is in a test class, look for:
- `@covers \App\Service\UserService` (docblock on class or method)
- `@coversClass(\App\Service\UserService::class)` (PHPUnit 10+)
- `#[CoversClass(UserService::class)]` (PHP 8 attribute, PHPUnit 10+)

Resolve the referenced class name via the standard class loader and
navigate to its definition. This can be exposed as a code lens
("Go to subject") or a code action, or both.

### From subject → test

Given a class, find test classes that reference it in `@covers` /
`@coversClass` / `#[CoversClass]`. This requires scanning test files
for the annotation. Two approaches:

- **Lazy scan**: When the user invokes "find tests" on a class, scan
  files matching `*Test.php` in the project for `@covers` / `#[CoversClass]`
  referencing the current class FQN. This is O(n) in test file count
  but test directories are typically small.
- **Indexed**: Now that full background indexing is available,
  `@covers` annotations can be indexed during the indexing pass and
  looked up in O(1).

The lazy approach is fine for most projects. Test directories rarely
exceed a few hundred files, and a simple `memchr`-based string
pre-filter on the class name before parsing keeps it fast.

### Exposure

- **Code lens** on test classes: "Subject: UserService" (clickable,
  navigates to the subject class).
- **Code lens** on subject classes: "Tests: UserServiceTest" (clickable,
  navigates to the test).
- **Code action**: "Go to test" / "Go to subject" when the cursor is
  on the class name.

### Dependencies

No hard dependencies. Works with the existing class loader for the
test → subject direction. The subject → test direction benefits from,
but does not require, the full background indexing that is now
available.

---

## F11. VS Code extension

| Field      | Value                    |
| ---------- | ------------------------ |
| **Impact** | High                     |
| **Effort** | Medium (2-5 days)        |

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
| **Effort** | Medium (2-5 days)        |

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
| **Effort** | Low (≤ 1 day)            |

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
| **Effort** | Low (≤ 1 day)            |

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

**Impact: Low-Medium · Effort: Low**

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

**Impact: Low · Effort: Low**

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

## F17. Class move with reference update

**Impact: Medium · Effort: Medium-High**

Move a class file to a new location and update all references across
the project (namespace declaration, `use` statements, FQN references).
PHPantom currently supports file rename on class rename but not the
full move-with-reference-update workflow.

The operation needs to:

1. Accept a source file and a destination path.
2. Compute the new namespace from the destination path using the
   PSR-4 autoload map.
3. Update the namespace declaration in the moved file.
4. Find all references to the class across the project (use
   statements, FQN occurrences, docblock type strings).
5. Rewrite each reference to use the new FQN, or update the `use`
   statement and leave short names unchanged.

Once the core move operation exists, also wire it to
`workspace/willRenameFiles` (declared via server capabilities
`workspace.fileOperations.willRename`): when the user renames or
moves a PHP file in the editor's file tree, return a `WorkspaceEdit`
that updates the namespace declaration and all `use` imports across
the workspace. This is the same machinery as steps 2-5, just
triggered by the editor instead of a code action. The companion
`workspace/willCreateFiles` can then insert a PSR-4-derived
`namespace` + class stub into newly created files (overlaps with
F18's namespace computation).

**References:**
- Phpactor: `MoveClass` refactoring in the class-mover package.
- php-lsp: `handle_will_rename_files` in
  `src/backend/handlers/workspace.rs` in its own repo (updates `use`
  imports workspace-wide on file rename) and their `willCreateFiles`
  PSR-4 stub insertion.

## F18. Fix namespace/class name from PSR-4

**Impact: Medium · Effort: Low**

When a class's namespace or name does not match its file path per
PSR-4 mapping, offer a code action (or command) to fix the namespace
and/or class name. The inverse direction (rename file on class rename)
is already supported.

The code action should:

1. Resolve the expected namespace and class name from the file path
   using the PSR-4 autoload map in `composer.json`.
2. If the current namespace differs, offer "Fix namespace to
   `App\Models\Foo`".
3. If the class name differs from the filename, offer "Fix class name
   to `Foo`".

**References:**
- Phpactor: `FixNamespaceClassName` code action.


---

## F19. Connect to a remote/TCP language server (VS Code extension)

**Impact: Low · Effort: Low-Medium**

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
