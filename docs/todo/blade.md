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

## BL16. Blade-aware formatting

**Impact: Low-Medium · Complexity: High**

`src/formatting/` has no Blade awareness. `mago`'s formatter runs against
the virtual PHP buffer generated for `.blade.php` files, and its output
has no fixed relationship to the original directive/HTML structure —
there is no path today that safely reformats the original Blade
markup.

- Medium term: extend `src/formatting/`'s existing external-tool
  resolution (currently php-cs-fixer/Pint/phpcbf via Composer
  `require-dev`) to also detect a project-installed `blade-formatter`
  (npm, via `package.json`/`node_modules/.bin`) and proxy it over
  `--stdin`, matching how Pint is already invoked. See the feasibility
  research below for why this is a good fit despite `blade-formatter`
  not being a Composer tool.
- Long term: a Blade-native indentation model (directive nesting depth
  + HTML tag depth + embedded `@php`/`{{ }}` PHP formatting via
  `mago`) as the built-in fallback for projects without
  `blade-formatter` installed. This is the highest-effort item in the
  Blade backlog; most Blade projects already reach for a dedicated
  Blade formatter, which is why the external-tool path above should
  land first.

### Feasibility research: proxying `blade-formatter` vs. a native basic formatter

Investigated `blade-formatter` (the `shufo/blade-formatter` npm
package) as a possible thing to shell out to.

**Proxying it fits the existing external-tool pattern.**
`src/formatting/` already resolves php-cs-fixer/Pint/phpcbf by detecting
them in the project (`composer.json` `require-dev`, resolved via
Composer's bin-dir) and falls back to the built-in mago-formatter when
absent — the same "use the project's own tool if it's there, otherwise
built-in" shape the user wants here. `blade-formatter` is a reasonable
addition to that resolution chain, not something to reject outright.
The one wrinkle: it is an npm package, not a Composer one, so detection
needs its own path parallel to (not reusing) the `vendor/bin` resolver
— check `package.json` `devDependencies`/`dependencies` for
`blade-formatter` and resolve the binary via `node_modules/.bin/
blade-formatter`, with the same `.phpantom.toml` override/disable
shape (`blade-formatter = "..."` / `blade-formatter = ""`) as the
existing PHP tools. This also means Node must be present on the
machine for the external path to trigger at all; when it isn't, or the
package isn't installed, fall back to the built-in formatter exactly
like the PHP tools do today.

The two operational concerns from the initial pass (no result cache;
`--write` deletes-then-rewrites the target file) turn out not to be
blockers once invoked the way we already invoke Pint: it supports
`--stdin` (format code from stdin, formatted result on stdout), so we
would never let `blade-formatter` touch the file directly — we own the
write, the same way we already do for Pint via `--stdin-filename`. And
since we'd invoke it once per format request (never in a long-lived
watch mode), the lack of an internal cache is no different from how
php-cs-fixer/phpcs are already invoked fresh per request; there is no
extra cost specific to this tool. Net: prefer the project's own
`blade-formatter` install when present (best fidelity with what the
team already uses and reviews), built-in native formatter as the
fallback and long-term goal — matching the existing PHP formatter
precedent.

**How it actually formats, and what that implies for a native
implementation.** It is not AST-based. The pipeline
(`formatContentPipeline.ts`) runs ~30 regex-based string processors in
a pre-process/post-process sandwich around two off-the-shelf
formatters: `js-beautify` for the HTML "shell" and
`@prettier/plugin-php` for isolated PHP/Blade-brace expressions. Content
that would confuse those formatters (raw `@php` blocks, `<script>`,
`<style>`, comments, Alpine.js `x-data`/`x-init` attributes, component
props) is regex-extracted to placeholder tokens before beautifying and
spliced back in afterward. Directive-nesting indent is a *separate*
pass (`formatter.ts`'s `processTokenizeResult`/`processKeyword`):
it tokenizes each line with a bundled Blade TextMate grammar
(`syntaxes/blade.tmLanguage.json`, run through `vscode-textmate` +
`vscode-oniguruma`/WASM) purely to classify which tokens are Blade
keywords, then walks a hardcoded stack of directive-start/-end/-else
token lists (`indent.ts`) to raise/lower indent level per line, with
special-cased exceptions (`@case` inside `@switch` dedents one extra
level, `@break` inside `@if` doesn't indent, `@section`/`@push`/`@slot`
are self-closing when given a second argument, `@hasSection` is
"unbalanced" and never closes). It is a large surface of hand-tuned
edge cases, not a formal grammar.

The directive-nesting indent pass is the one part of this design that
translates cleanly to PHPantom, and we are better positioned to do it
than blade-formatter was. Unlike blade-formatter (which had to pull in
a TextMate grammar + oniguruma WASM just to find directive tokens on a
line), we already scan raw Blade source: `blade::balance::directives`
yields every directive with its span and argument list,
`blade::balance::BLOCKS` already pairs each opener with the closers
Blade accepts for it (the same table blade-formatter hardcodes in
`indent.ts`), `blade::signature::inert_regions` masks comments,
`@verbatim`, and `@php` blocks, and `blade::component_tags::tag_spans`
/ `lex_tag_attributes` read tags. Build on those. Do not build on the
preprocessor: `preprocess_with_vars` is one 1,100-line lowering that
emits PHP as it scans, not a tokenizer, and it is not to be refactored
for this. What's missing is (a) marking the else-like directives
(`@else`, `@elseif`, `@empty` inside `@forelse`, `@case`/`@default`)
that dedent without closing, on top of the open/close pairing `BLOCKS`
gives, and (b) an HTML tag-depth counter for plain elements
(open/close/void/self-closing) interleaved with directive depth on the
same line — new code, but a single self-contained pass, not a full HTML
parser. Embedded `@php`/`{{ }}` expression
formatting can reuse `mago`'s formatter on isolated snippets the same
way diagnostics already isolate virtual-PHP buffers, rather than
needing a second PHP formatter dependency like blade-formatter does.

**Recommended scope for the native fallback formatter:** directive-
nesting indent + HTML tag indent only, reindenting existing lines
without rewriting their content (no attribute sorting, no line-wrap/
wrapping of long tags, no quote-style normalization, no Tailwind class
sorting). That covers the visible majority of blade-formatter's example
output (consistent indentation) while skipping almost all of its
~30-processor edge-case surface, which exists to handle content
rewriting we are choosing not to do. This is still a real chunk of work
(a new directive-classification table + an HTML depth scanner + the
`mago`-snippet-reformat glue), but is meaningfully smaller than full
parity with `blade-formatter`, does not require adding a
TextMate/oniguruma dependency the way blade-formatter's own approach
does, and is worth having on its own merits: it is the only
option for projects that don't have `blade-formatter` installed, and
if it ends up faster and at least as correct as `blade-formatter` (a
real possibility, given we skip the regex/placeholder round-tripping
entirely), it can become the default rather than staying a fallback.
See BL17 below for exposing it as a standalone CI check once it
reaches that bar.

### Tests

- `formatting` on a `.blade.php` file returns no edits (short-term
  behaviour) rather than corrupting the file, until the long-term
  model lands.

---

## BL17. `format --check` CLI subcommand for CI

**Impact: Low-Medium · Complexity: Medium** (depends on BL16)

Filed while researching BL16: once the native Blade formatter (and,
more generally, PHPantom's resolved formatting strategy — external
tool or built-in) is trustworthy enough to enforce, projects want a
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
- This depends on the native Blade formatter model (BL16 long term)
  existing and being fast/correct enough that a maintainer would want
  it enforced in CI; do not build the CLI surface before that bar is
  met, since the CLI is only useful once there's a trustworthy
  formatter behind it (or a detected `blade-formatter`/`php-cs-fixer`
  external tool, which `--check` should honour identically to
  `format` without `--check`).

### Tests

- `format --check` on an already-formatted project exits 0 with no
  output.
- `format --check` on a project with an unformatted `.blade.php` file
  exits non-zero and names the file.
- `format` (no `--check`) rewrites the file in place and a second run
  is a no-op.
