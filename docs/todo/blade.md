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
has no fixed relationship to the original directive/HTML structure, so
`textDocument/formatting` currently returns no edits for a Blade file
(`server.rs` short-circuits on `is_blade_file`).

Three tools format Blade today; the feasibility notes below compare them.
The plan that falls out of the comparison:

1. **Short term: let the existing Pint proxy format Blade.** Pint 1.30.0
   (June 2026) formats `.blade.php` natively behind the
   `Pint/laravel_blade` rule (also reachable as the `--blade` flag), and
   it works over the `--stdin-filename` invocation
   `src/formatting/external.rs` already uses. Route a Blade file to Pint
   when the project's `pint.json` enables the rule, and pass `--blade`
   when `.phpantom.toml` asks for it explicitly (`[formatting]
   pint-blade = true`). Otherwise a Blade file must not go to Pint at
   all: Pint echoes an excluded file back unchanged with exit 0, which
   is indistinguishable from "already formatted", so the decision has
   to be made from the config, the same way a `phpcs.xml` or a
   `mago.toml` `[formatter]` table already steers `resolve_strategy`.
   Depends on [B1](bugs.md#b1-the-pint-formatting-proxy-inherits-the-servers-working-directory):
   Pint reads `pint.json` and, for Blade, the project's `node_modules`
   from its working directory, and the proxy does not set one today.
2. **Long term: a native reindent-only formatter as the built-in
   fallback**, modelled on `joelstein/blade-formatter`'s
   `IndentationFormatter` and built on the directive and tag scanners
   `src/blade/` already has (see "What we already have" below). Leading
   whitespace only: directive nesting, HTML/component tag nesting,
   multi-line tag attributes. No reflow, no attribute wrapping or
   sorting, no Tailwind class sorting, no quote normalisation. Embedded
   PHP (`@php` blocks, `{{ }}`, directive arguments) is a later step
   through `mago`'s formatter on isolated snippets, the same way
   diagnostics isolate virtual-PHP buffers.
3. **Explicit-path proxies only for the two standalone Blade
   formatters.** `shufo/blade-formatter` (npm, `--stdin`) and
   `joelstein/blade-formatter` (Composer, `vendor/bin/blade-format`,
   rewrites a path in place) can be supported as `.phpantom.toml`
   overrides if a project asks, but neither should be auto-detected.
   Pint now covers the "team already uses a Blade reflow formatter" case
   with a Composer-visible signal, and a detection path for an npm tool
   (Node present, `package.json`, `node_modules/.bin`) is a second
   resolver for a shrinking audience. If the Composer one is ever
   wired up, note that it decides what to do from the file *name*
   (`Envoy.blade.php`, `vendor/mail/…`, the `.blade.php` suffix), so
   the sibling temp file `write_sibling_temp_file` creates would need
   the real basename, not the `.phpantom-fmt-*.php` pattern.

### Feasibility research

**Pint formats Blade natively since 1.30.0.** The `Pint/laravel_blade`
fixer (`app/Fixers/LaravelBlade/Fixer.php`) hands every `.blade.php` to
`app/BladeFormatter.php`, which runs prettier with `prettier-plugin-blade`
and `prettier-plugin-tailwindcss` inside a long-lived Node worker
(`resources/js/worker.cjs`, newline-delimited JSON over stdin), wrapped in
eleven regex pre/post passes (mask Blade inside `<script>`/`<style>` and
Alpine `x-mask`, collapse short slots and single-attribute tags, re-join
dangling `>`, restore `@@` spacing). Prettier's own PHP formatting is
turned off (`bladePhpFormatting: "off"` in the bundled
`resources/prettier/prettierrc.json`); Pint's regular fixers format the
PHP fragments instead (`PhpBlockFormatting` over `@php` blocks, `<?php`
islands, directive arguments, and echoes, via `PhpFragmentFormatter`).
Output is prettier reflow: `printWidth` 120, `tabWidth` 4, single quotes,
`htmlWhitespaceSensitivity: css`, Tailwind classes always sorted. It
skips `Envoy.blade.php`, anything under `vendor/mail` or using an
`<x-mail::` component, and `resources/boost/guidelines/**`, all
whitespace-sensitive.

Verified against the `references/pint` checkout (v1.31.1, PHP 8.5,
Node 25):

- `pint --blade --stdin-filename=resources/views/welcome.blade.php` on a
  four-line sample reindented it, normalised `@if(true)` to `@if (true)`
  and `{{$name}}` to `{{ $name }}`, sorted `class="p-4 flex"` to
  `flex p-4`, and spaced `/>`; exit 0. Pint's own `StdinTest.php` covers
  the same path.
- The same input without `--blade` and without the rule in `pint.json`
  came back byte-identical with exit 0. `ConfigurationFactory` excludes
  `*.blade.php` from the finder whenever the rule is off, and the stdin
  path echoes an excluded file. This is why detection has to read the
  config rather than try Pint and see.
- `--blade` from a directory with no `node_modules` printed an
  explanation and exited 1 in 0.2 s without writing anything: the
  Laravel Prompts "install them now?" confirmation falls back to its
  default (no) when stdin is not a TTY, and `run_pint` already treats a
  non-zero exit as an error rather than as output. It does not hang, and
  it does not create a `package.json` (that only happens on an accepted
  prompt).
- Requirements on the machine: Node or Bun on `PATH` (Bun only when a
  `bun.lock` exists), and `prettier ^3.9.6`, `prettier-plugin-blade
  ^3.3.3`, `prettier-plugin-tailwindcss ^0.8.1` resolvable from the
  project root. Version drift is refused with a message, not silently
  accepted.

**The three tools side by side.**

| | `shufo/blade-formatter` 1.44 (npm) | Pint 1.30+ `--blade` | `joelstein/blade-formatter` 0.10 (Composer) |
| --- | --- | --- | --- |
| Engine | `js-beautify` for the HTML shell, `@prettier/plugin-php` for PHP snippets, a TextMate grammar (`vscode-textmate` + oniguruma WASM) to classify directive tokens, ~30 regex processors around them | prettier + `prettier-plugin-blade` + `prettier-plugin-tailwindcss` in a Node worker, Pint's PHP fixers for embedded PHP, 11 regex pre/post passes | One hand-written per-line indenter in PHP (706 lines, no dependencies); Pint for `@php`/Livewire SFC PHP; prettier for Tailwind sorting |
| What it changes | Reflow: wraps at `wrapLineLength`, re-lays-out attributes, normalises spacing, optional attribute and Tailwind sorting | Reflow at 120 columns, spacing normalisation, Tailwind sorting always on | Leading whitespace only; line breaks stay where the author put them |
| Runtime | Node | PHP + Node/Bun + three npm packages | PHP (Node only for Tailwind sorting) |
| Invocation | `--stdin` | `--stdin-filename`, cwd must be the project root | In place on a path, no stdin; its own VS Code extension writes a temp file and reads it back |
| Detection signal | `package.json` | `composer.json` `laravel/pint` plus `pint.json` `rules["Pint/laravel_blade"]` | `composer.json` `joelstein/blade-formatter` |
| Config file | `.bladeformatterrc.json` | `pint.json` (prettier config is bundled, not the project's) | `blade-formatter.json` |
| Disable regions | `{{-- blade-formatter-disable --}}` / `-enable`, `-disable-next-line` | none | `{{-- blade-formatter-disable --}}` / `-enable` |

Pint depends on prettier just as much as `blade-formatter` does; it only
hides it behind Composer. There is no dependency-free reflow formatter for
Blade, and there is not going to be one inside PHPantom either.

**Scope: reindent, not reflow.** `joelstein/blade-formatter`'s README
makes the case for this scope well: HTML whitespace can be meaningful,
Blade files are one deep tree so wrapping compounds at every level, and
line length and attributes-per-line are exactly the settings teams
disagree about. Reindenting existing lines is what every one of the
three tools agrees on and where their outputs coincide; it is also the
part that needs no external dependency and no per-language formatter
for the CSS/JS/PHP embedded in the file. Concretely, the native
formatter should:

- Reindent every line by directive depth and HTML/component tag depth,
  including continuation lines of a multi-line opening tag (attributes
  one level in, the closing `>`/`/>` back at the tag's level).
- Shift `<script>`, `<style>`, and `@php` bodies to the enclosing level
  while preserving their internal relative indentation (the
  "indent-preserve" mode `joelstein/blade-formatter` uses for `@php`),
  rather than formatting JavaScript or CSS.
- Leave `<pre>`, `<textarea>`, and `@verbatim` bodies byte-identical,
  honour `{{-- blade-formatter-disable --}}`/`-enable` regions, and skip
  whole files where indentation is output: `Envoy.blade.php`, Markdown
  mail (`<x-mail::`, `vendor/mail`), `resources/boost/guidelines/**`.
- Not touch anything inside a line. Spacing normalisation (`@if(` to
  `@if (`, `{{$x}}` to `{{ $x }}`) is cheap with our tokenizer and both
  reflow tools do it, but it is a separate, opt-in step, and it must not
  run inside `@verbatim`, strings, or `<script>` bodies.

Embedded PHP through `mago` comes after that, and only for regions the
preprocessor already isolates.

**Nobody else can format the non-Blade part of a Blade file.** LSP
formatting is per document: the client picks one server for a language
ID and sends it the whole file. There is no protocol for one server
formatting the HTML and another the directives, and editors that handle
embedded languages (VS Code's HTML server for CSS/JS in `<style>`/
`<script>`) do it inside a single server. Whichever formatter PHPantom
runs owns the whole `.blade.php`, so "just do the Blade bits" is not an
option; hence the shift-don't-format rule for `<script>`/`<style>` above.

**How `shufo/blade-formatter` works, and why its design does not
transfer.** It is not AST-based. `formatContentPipeline.ts` runs its
~30 regex string processors in a pre-process/post-process sandwich around
`js-beautify` (HTML) and `@prettier/plugin-php` (isolated PHP/Blade-brace
expressions): content that would confuse those formatters (raw `@php`
blocks, `<script>`, `<style>`, comments, Alpine `x-data`/`x-init`,
component props) is swapped for placeholder tokens before beautifying and
spliced back afterward. Directive-nesting indent is a separate pass
(`formatter.ts`'s `processTokenizeResult`/`processKeyword`): it tokenizes
each line with the bundled TextMate grammar purely to find Blade
keywords, then walks hardcoded directive-start/-end/-else lists
(`indent.ts`) with special cases (`@case` inside `@switch` dedents one
extra level, `@break` inside `@if` does not indent, `@section`/`@push`/
`@slot` are self-closing with a second argument, `@hasSection` never
closes). The directive lists are worth cross-checking our classification
table against; nothing else carries over.

**How `joelstein/blade-formatter` works, and why it is the model.**
`app/Formatters/IndentationFormatter.php` is a single forward pass over
lines with a handful of counters: indent level, a directive stack (so an
`@endif` aligns with its `@if` even when an HTML tag opened between them
was never closed inside the block), an HTML tag stack, brace depth for
multi-line Alpine/`@class` attribute values, a "multi-line tag" state
for attributes on their own lines, preserve and indent-preserve block
modes. Tables: void elements, inline elements (do not indent when
followed by text), opening directives with their closers (`@hasSection`
and `@sectionMissing` close with `@endif`), mid-block directives
(`@else`, `@elseif`), `@case`/`@default`. Its weak points are the ones
we can fix structurally: it recognises tags and directives with per-line
regexes and counts braces by character (Blade delimiters and quoted
strings are blanked first), where we can take directive and tag spans
from real scanners and know exactly which braces are inside a `{{ }}`.
Its 86 unit tests are the closest thing to a specification of the
reindent-only behaviour that exists.

**What we already have.** We already scan raw Blade source, so none of
this needs a new tokenizer or a TextMate dependency.
`blade::balance::directives` yields every directive with its span and
argument list, `blade::balance::BLOCKS` pairs each opener with the
closers Blade accepts for it (the same table `shufo/blade-formatter`
hardcodes in `indent.ts`), `blade::balance::walk`/`block_pairs` walk
that stream with a stack of open blocks and return each opener/closer
pair with spans, and `blade::signature::inert_regions` masks comments,
`@verbatim`, and `@php` blocks. That is the directive half of the depth
model. `blade::component_tags::tag_spans`/`tag_imbalances` and
`lex_tag_attributes` already read HTML and `<x-…>` tags with
void/self-closing handling for the unbalanced-tag diagnostic, which is
the tag half. Build on those. Do not build on the preprocessor:
`preprocess_with_vars` is one 1,100-line lowering that emits PHP as it
scans, not a tokenizer, and it is not to be refactored for this. What
is missing is (a) marking the else-like directives (`@else`, `@elseif`,
`@empty` inside `@forelse`, `@case`/`@default`) that dedent without
closing, on top of the open/close pairing `BLOCKS` gives, (b) a per-line
indent computation that consumes both span streams, including an HTML
tag-depth counter for plain elements interleaved with directive depth
on the same line, (c) the multi-line-attribute and brace-depth handling
inside a tag, and (d) the preserve/skip rules.

### Test corpora to port

All three projects are MIT, so their fixtures can be copied with an
attribution note in the fixture directory's README.

| Source | What | Size | How to use it |
| --- | --- | --- | --- |
| `joelstein/blade-formatter` `tests/Unit/IndentationFormatterTest.php` | Inline input/expected pairs for leading-whitespace-only reindentation: HTML nesting, void/self-closing, `x-`/`flux:` components, every block directive family, `@switch`/`@case`, `@hasSection`, inline vs block `@php`/`@section`, multi-line tags and Alpine attribute values, `<pre>`/`@verbatim` preservation, `<script>`/`<style>` shifting, dynamic `<{{ $tag }}>`, crossed HTML/Blade nesting | 86 cases | Port near-verbatim as unit tests of the native formatter. This is the golden set; the expectations match our scope exactly. `tests/Unit/ParserTest.php` (Livewire SFC split) if SFCs are handled. |
| `shufo/blade-formatter` `__tests__/formatter/*.ts` | 207 inline tests; `indent.test.ts`, `builtin-directives.test.ts` (31, every built-in block directive), `if-else`, `nested`, `unbalanced`, `custom-directives`, `case` fixture | ~60 indent-focused | Expected output also carries reflow effects (`@if (` spacing, `js-beautify` attribute layout), so port the *input* side and re-derive expectations, or use the expected side as input for idempotence. Cross-check `src/indent.ts`'s directive lists against our classification. |
| `shufo/blade-formatter` `__tests__/fixtures/` | 40 `x.blade.php` / `formatted.x.blade.php` pairs, 18 `snapshots/*.snapshot` (options / content / expected sections), `largefile.blade.php`, `syntax.error.blade.php` | 58 files | Property corpus: format twice is a no-op; only leading whitespace changes; blank lines preserved; `<pre>`/`@verbatim` bodies byte-identical; malformed input does not panic. `largefile` for a perf floor. |
| Pint `tests/Fixtures/blade-formatting/**` | `.blade.php` / `.blade.php.expected` pairs in 23 groups (alpine, attributes, comments, components, control-structures, directives, echoes, error-directives, escaping, html, ignorables, inline, kitchen-sink, minimal, normalization, not-operator, php, script, style, style-dynamic, tailwind, trailing, whitespace) | 183 pairs | The expected side is prettier reflow and is not golden for a reindenter. The input side is the best stress corpus available (`kitchen-sink/everything.blade.php` is deliberately messy and touches everything), and `ignorables/` enumerates the must-not-touch files. The pairs *are* golden for an integration test of the Pint proxy, gated on Node and the three packages being present, so not run in CI. |
| Pint `tests/Feature/StdinTest.php` | The `--stdin-filename` Blade case | 1 | Template for the Pint-proxy integration test and for the "excluded file echoes back unchanged" case. |

### Tests

- A `.blade.php` in a project whose `pint.json` enables
  `Pint/laravel_blade` is sent to Pint with the workspace root as the
  working directory; the same file in a project without the rule is not
  sent to Pint (native formatter, or no edits until it lands).
- `pint-blade = true` in `.phpantom.toml` adds `--blade`; a Pint exit
  code of 1 (missing Node dependencies) surfaces as a formatting error,
  never as an edit that replaces the document with Pint's message.
- Native formatter: the ported `IndentationFormatterTest` cases pass;
  formatting is idempotent over every corpus above; every output line
  equals its input line after trimming leading whitespace; blank lines,
  `<pre>`/`@verbatim` bodies, disabled regions, and the skip-listed
  files are byte-identical.
- `formatting` on a `.blade.php` file never corrupts the file, whichever
  strategy resolves.

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
