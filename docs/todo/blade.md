# PHPantom — Blade

Known gaps and planned features in PHPantom's Laravel Blade template
support. For Eloquent model support see `laravel.md`. For the shipped
preprocessor pipeline and module layout see `ARCHITECTURE.md`.

The strategy, implemented in `src/blade/`: preprocess `.blade.php`
files (recognised by URI suffix, or by a `did_open` `languageId` of
`"blade"`) into valid virtual PHP, feed the virtual PHP through the
existing pipeline (parser, resolver, completion, definition), and map
response positions back to the original Blade file through a source
map. Every item below builds on that pipeline.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier, with a dependent item placed after its
dependency.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## Out of scope (and why)

| Item | Reason |
|------|--------|
| Editor-side Blade language registration | The tree-sitter grammar, `.blade.php` file association, and `languageId: "blade"` wiring belong to editor extensions, not the server. Zed's official PHP extension (which absorbed PHPantom's plain-PHP wiring; this repo no longer bundles `zed-extension/`) will not grow Blade support — that registration belongs to a third-party Blade extension instead, which already exists (`zed-laravel-blade`, MIT, unaffiliated) and lists PHPantom as one of several selectable language servers alongside Intelephense, PhpTools, and Phpactor. On VS Code, Blade extensions already set `languageId` to `"blade"`, so PHPantom's integration needs to register for both `"php"` and `"blade"`. Neovim's `lspconfig` can be configured to send `.blade.php` files with the correct `languageId`. |
| Rendering or booting to resolve templates | Consistent with `laravel.md`: we never run PHP or boot a Laravel application. |

---

## Philosophy

- **No application booting.** Consistent with `laravel.md`. We
  never run PHP or boot a Laravel application.
- **Signatures over call-site scanning.** A template's variable types
  come from its declared contract: the Bladestan-compatible chain in
  `src/blade/signature.rs` — `@bladestan-signature` docblock, first
  docblock before template code, `@props`/`@aware`, Blade's own
  component scope, the backing component class, and the layouts the
  template `@extends`. The template declares what it expects; call
  sites are then *validated* against that contract
  (`src/blade/contract.rs` and `src/diagnostics/blade_call_site.rs`),
  exactly as a function signature works. Inferring types *from* call
  sites inverts the contract and produces "true for one caller" types,
  so it is not the foundation — the shipped call-site inference
  fallback for unannotated projects is layered strictly below every
  declared source. Projects running Bladestan (a PHPStan extension for
  Blade template analysis) get the full contract model in both the
  editor and CI from the same annotations.
- **Discovery is just directory walks.** Walking the configured view
  roots and the directories the component namespaces live in
  (`src/blade/discovery.rs`) is the full extent of external Blade file
  discovery. Paths are converted to view names and component names via
  string transforms. A namespace no PSR-4 mapping of the project's own
  `composer.json` covers (a vendor package that registered a component
  namespace) is read off the class index instead, since there is no
  directory to walk for it.
- **PSR-4 says where a namespace lives, not what a component is
  called.** A mapping resolves `App\View\Components` to the directory
  to walk, and once we know an FQN (e.g. `App\View\Components\Alert`)
  the existing `find_or_load_class` pipeline reads its source. The
  names themselves always come from the file paths.
- **Graceful degradation.** Unknown directives become comments. Failed
  component resolution produces comments. The user always gets partial
  completions rather than a broken file. The preprocessor must never
  produce invalid PHP.

---


## BL1. Blade-aware code actions

**Impact: Medium · Complexity: Medium-High**

Code actions are currently disabled for `.blade.php` files because
text edits target virtual PHP coordinates and actions like "Import
class" insert `use` statements at the top of the file rather than
inside a `@php` / `<?php` block. Re-enable code actions with:

- Range translation (virtual PHP → Blade) for all text edits.
- Blade-aware code generation (e.g. insert `use` inside `@php`).
- Filtering out actions that don't make sense in Blade context.

**Deliverable:** Code actions are re-enabled for `.blade.php` files.

---

## BL17. `format --check` CLI subcommand for CI

**Impact: Low-Medium · Complexity: Medium**

Once PHPantom's resolved formatting strategy (the built-in PHP
formatter, the built-in Blade reindenter, or an external tool) is
trustworthy enough to enforce, projects want a
non-editor way to verify a PR ran it, the same role `blade-formatter
-c`/`--check-formatted` plays today. `main.rs` currently only exposes
`analyse` and `fix` as CLI subcommands; there is no way to invoke
`textDocument/formatting`'s resolution logic (`src/formatting/`) outside
the LSP connection at all.

- Add a `format` subcommand (`phpantom_lsp format --project-root
  <DIR> [--check]`) that walks project PHP/Blade files, runs the same
  `resolve_strategy` external-tool-or-built-in logic `src/formatting/`
  already uses, and either writes the formatted result back or (with
  `--check`) exits non-zero and lists files that would change, without
  writing them — mirroring `blade-formatter -c -d` and `phpcs
  --dry-run`/`php-cs-fixer --dry-run` conventions projects already use
  in CI.
- This depends on the built-in Blade reindenter
  (`src/formatting/blade/`) being fast/correct enough that a maintainer
  would want it enforced in CI; do not build the CLI surface before that
  bar is met, since the CLI is only useful once there's a trustworthy
  formatter behind it (or a detected Pint/`php-cs-fixer` external tool,
  which `--check` should honour identically to `format` without
  `--check`).

### Tests

- `format --check` on an already-formatted project exits 0 with no
  output.
- `format --check` on a project with an unformatted `.blade.php` file
  exits non-zero and names the file.
- `format` (no `--check`) rewrites the file in place and a second run
  is a no-op.

---

## BL18. Format the PHP embedded in a Blade template

**Impact: Low-Medium · Complexity: Medium-High**

The built-in Blade formatter (`src/formatting/blade/reindent.rs`)
changes leading whitespace only, so the PHP inside a template keeps
whatever spacing the author typed: `{{$name}}` stays `{{$name}}`,
`@if($a&&$b)` stays as written, and an `@php` block is shifted but not
formatted. Pint's Blade rule formats those fragments with its PHP fixers
(`PhpBlockFormatting` over `@php` blocks, `<?php` islands, directive
arguments, and echoes); the built-in formatter should do the same through
the embedded `mago` formatter, on isolated snippets, the way diagnostics
isolate virtual-PHP buffers rather than through the preprocessor's
lowering.

- `@php … @endphp` bodies and `<?php … ?>` islands are statement lists:
  format them as a file body and let the reindenter shift the result to
  the block's level, which it already does for any body.
- `{{ }}`, `{!! !!}`, and directive arguments are single expressions:
  format each as an expression statement and drop the trailing `;`. A
  fragment that spans lines has to keep its line count, or the
  reindenter's per-line model has to learn to re-derive it.
- Spacing that is Blade's rather than PHP's belongs to the same pass:
  `@if(` to `@if (`, `{{$x}}` to `{{ $x }}`, `/>` spacing. The reflow
  tools all do it, and the directive and echo scanners make it cheap.
- Opt-in, behind a `[formatting]` key, since it changes line content
  and the reindenter's contract today is that it never does. It must
  not run inside `@verbatim`, a comment, a string, `<script>`,
  `<style>`, or `<pre>`.
- Never touch an Alpine or Livewire attribute value: it is JavaScript.

### Tests

- `{{$name}}` becomes `{{ $name }}`, `@if($a&&$b)` becomes
  `@if ($a && $b)`, and an `@php` block is formatted as PHP and lands at
  the block's indentation.
- A fragment that does not parse is left as written and the rest of the
  template still formats.
- Nothing inside `@verbatim`, `<script>`, `<style>`, or a string
  changes.
- Formatting stays idempotent over the corpora the reindenter's tests
  use.
