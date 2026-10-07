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

**Current state:** `translate_workspace_edit` (`src/blade/translate.rs`)
runs at the end of `handle_code_action` and `resolve_code_action`, moving
edits back into the template and dropping the ones that land in the
prologue. `use_block_for` (`src/completion/use_edit.rs`) hands a template
its own `@use` block (`src/blade/use_block.rs`), which "Import class" and
"Import all missing classes" already use, and a batch of imports keeps each
directive's line break (`UseBlockInfo::drop_repeated_separator`).
`replace_fqcn.rs` still plans against `analyze_use_block_in(content,
block)`, the virtual PHP, and needs `use_block_for(uri, content, block)`.
The server-side gate in `code_action` (`src/server.rs`) still answers
nothing for a template.

**What the Sprint 8 gate analysis found the actions need** (each point is
a way a translated edit corrupts or misplaces template text):

- **Request in virtual coordinates, diagnostics in the template's.**
  Translate the request range into the virtual PHP before the collectors
  run, but leave `context.diagnostics` alone: the diagnostic collectors
  already report a template's diagnostics in its own coordinates, and the
  editor matches an action's `diagnostics` against what it was sent.
  Collectors that compare a diagnostic's range with a virtual one must
  translate the diagnostic's range: `remove_unused_import.rs` (the overlap
  test, and the line it deletes on resolve) and `insert_translation_key.rs`.
  `create_missing_view.rs` builds its attached diagnostic from a virtual
  range and has to build it in the template's coordinates
  (`Backend::offset_range_to_lsp_range`).
- **All or nothing.** Dropping a prologue edit and keeping the rest leaves
  a refactoring half applied (an extracted constant whose declaration was
  dropped, a replaced call whose import was). A replacement has to cover
  text the template wrote: the virtual text under it equals the template
  text under the translated range (a CRLF template's line breaks come out
  bare in the virtual PHP). An insertion has to sit where the trip back to
  the virtual PHP returns to the same position, and not straight after a
  Blade token (a statement inserted at `@php(`'s argument splits the
  directive from it). An edit in the prologue or in the wrapper's closing
  lines rejects the whole action. Import edits are the one kind written in
  Blade rather than PHP, at the positions `use_block.rs` chooses, and are
  checked against those positions instead (see B550 for where the first
  import belongs).
- **Resolve before offering.** A deferred action can resolve to nothing in
  a template (extract variable inside `{{ }}`, whose statement starts in
  generated code; inline variable on a name only the prologue assigns), so
  a template's deferred actions are resolved before the list goes out.
- **Spans the template did not write.** Every template's prologue declares
  `$errors` with `\Illuminate\Support\ViewErrorBag`, and a component tag
  lowers to `new \App\View\Components\Alert(...)`; both are qualified class
  references in the symbol map. "Import all qualified symbols" and "Import
  all missing classes" have to skip a span whose text is not the template's
  own, or they import names the template never mentions.
- **Actions to filter out.** PHPStan and Mago fixes are planned in the raw
  file's coordinates (their diagnostics come from tools that read the file
  on disk), and several insert comment lines that would land in HTML.
  Fix namespace and fix class name are about class files. Extract function
  places the new function after the enclosing one, which for template code
  is the wrapper the preprocessor closes after the last line.
- **A stale lowering.** `did_change` parses in the background, so the
  virtual PHP can lag the buffer by a keystroke. Record the template length
  a source map was built from (as `SymbolMap::matches_source` does) and
  offer nothing for a template whose buffer no longer matches it.

Cover each through `code_actions_via_server` (`tests/integration/common/
mod.rs`), which goes through the gate; include a template whose first line
opens a block, an `@php(...)` one-liner, a component tag next to a
qualified class name, a Livewire template (whose body is a method of a
synthesized subclass), and a CRLF template.

**Deliverable:** Code actions are re-enabled for `.blade.php` files.
