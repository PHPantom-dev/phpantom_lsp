# zed-laravel — Roadmap

Ordered by priority. Constraint to keep in mind throughout: Zed
extensions declare capabilities (languages, grammars, language
servers, snippets) and run WASM-sandboxed — there is no API for
custom panels, tree views, or webviews. Features needing UI beyond
text belong in PHPantom itself (code lens, hover, completion), which
Zed renders natively.

## Ecosystem context (read before Z1)

Two existing extensions shape our scope:

- **`bajrangCoder/zed-laravel-blade`** (registry id `blade`) owns the
  Blade language definition: `tree-sitter-blade` grammar, config,
  injections, and — as of its current release — **already registers
  PHPantom as a Blade language server** with
  `language_ids = { Blade = "blade" }` and its own binary discovery.
  Its README lists PHPantom as a supported PHP LSP option.
- **`mike-bronner/zed-laravel`** (registry id `laravel`) is a mature
  community Laravel extension (~141k LOC LSP, tree-sitter + Salsa,
  live-DB features). It positions itself as a *companion* to a PHP
  LSP and currently recommends Intelephense. Its feature surface
  overlaps heavily with PHPantom's Laravel layer (Eloquent magic,
  string keys) but also covers workflow features we don't (quick
  actions, column/Blade-var rename, artisan runnables, DB-backed
  diagnostics). Coordination, not competition — see Z6.

Zed facts learned from their upstream review process, binding on our
design: language ownership is exclusive (one extension owns "Blade",
no augmenting another extension's language definition), a file
belongs to exactly one language, and the Zed reviewers push back on
extensions that duplicate an existing language definition.

## Z1. Extension scaffold

`extension.toml` + Rust WASM extension crate, following the current
Zed extension conventions. Publish target: the `zed-extensions`
registry. **The id `laravel` is taken** (mike-bronner) and `blade` is
taken (bajrangCoder); publish as `phpantom-laravel` (or fold the
remaining scope into the official PHP extension if the registry
maintainers prefer — ask in the submission PR).

## Z2. Blade language + PHPantom wiring — COVERED UPSTREAM, verify only

The original plan (register our own Blade language, wire PHPantom to
it) is superseded: `zed-laravel-blade` already provides the Blade
language *and* registers PHPantom against it with the `blade`
language id. Shipping our own Blade language would conflict with it
(exclusive ownership) and would be rejected in review as a
duplicate. Remaining work:

- Verify end-to-end: install `zed-laravel-blade` + PHPantom, open a
  `*.blade.php`, confirm PHPantom attaches with `languageId: "blade"`
  and the Blade preprocessor engages (completions/hover inside echo
  and directive expressions, HTML regions injected).
- Coordinate binary sharing: their extension downloads its own
  PHPantom binary; the official PHP extension does too. Check whether
  both resolve the same install (PATH-first lookup) so users don't get
  two copies, and PR a fix upstream if not.
- Contribute improvements upstream (injections, highlight queries,
  bracket pairs for `{{ }}` / `{!! !!}` / `{{-- --}}`) rather than
  forking. mike-bronner's repo has a detailed loss/gain comparison of
  Blade language definitions worth mining for that PR.

## Z4. Laravel runnables

Task templates keyed off tree-sitter captures:

- Run/rerun the artisan command a `Command` class defines (capture
  the `$signature` literal).
- Run this test / this file for PHPUnit and Pest test files.
- `artisan serve`, `migrate`, `queue:work` as workspace tasks when
  `artisan` exists at the workspace root.

Note: `mike-bronner/zed-laravel` does not ship runnables today; this
is uncontested space, but re-check before building.

## Z5. Snippets

Blade directive snippets (`@if/@endif`, `@foreach`, `@section`,
component skeletons) and common Laravel PHP snippets (relationship
methods with Larastan-style generics, casts, scopes — matching the
annotation style PHPantom resolves best, so the snippets teach the
well-typed patterns).

## Z6. Upstream coordination

- The `zed-extension/` → official `zed-extensions/php` merge is
  complete; this extension depends on the official PHP extension for
  plain PHP and only layers Laravel workflow bits on top. Position
  check: the official extension bundles **four** language servers
  (PhpTools got the top listing, plus Intelephense, Phpactor, and
  PHPantom). PHPantom is one option, **not** the default. The update
  shipped without a config migration, so some existing users with
  explicit `language_servers` lists ended up with PHPantom active
  without choosing it. Never claim default status in outreach or
  docs.
- **`zed-laravel-blade`**: upstream any Blade language improvements
  (see Z2). Their README already lists PHPantom; keep that entry
  accurate as our Blade support evolves.
- **`mike-bronner/zed-laravel`**: coordination issue posted 2026-07-31
  (introduced PHPantom, noted the overlap, offered to write a
  coexistence doc, left the dedup mechanism open to their preference).
  Awaiting a response; once they weigh in, follow through with
  whichever of these apply:
  - PR their README: the companion list and editor table describe the
    official PHP extension as "(Intelephense)"; it now bundles four
    language servers, so the parenthetical is stale independent of
    any politics. The ask is to name PHPantom among the options, not
    to claim a default we don't have.
  - Write the coexistence doc mirroring their
    `docs/tuning-intelephense.md` (what to disable on each side to
    avoid duplicate hovers/completions/diagnostics when running
    laravel-lsp alongside PHPantom).
  - Propose a dedup mechanism: a laravel-lsp setting (or detection)
    to suppress its Eloquent-magic answers when PHPantom is attached,
    and symmetrically a PHPantom setting to defer string-key quick
    actions to them. Users pick, defaults stay sane.
  - Mutual recommendation once coexistence is clean.
- We are porting portions of their MIT test suite as behavior specs
  (see `docs/todo/test-porting.md` Phase 7). File any divergences we
  find as issues upstream — cross-pollination is the goodwill basis
  for the collaboration ask.
