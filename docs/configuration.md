# Configuration Reference

PHPantom works best with Composer projects. It reads `composer.json` to discover autoload directories and vendor packages, so completions and go-to-definition only surface classes that your autoloader can actually load. Projects without `composer.json` fall back to scanning every PHP file in the workspace.

## `.phpantom.toml`

PHPantom supports an optional per-project configuration file. To
generate a starter config file:

```bash
phpantom_lsp init
```

This creates a minimal `.phpantom.toml` with a JSON schema directive.
Editors with TOML schema support (Zed, VS Code + Even Better TOML,
Neovim) provide autocomplete and hover documentation for every option
via the schema. Only add settings you want to override -- when absent,
all settings use their defaults.

### Global config

Settings you want in every project belong in the global config rather
than in a `.phpantom.toml` per repository:

```bash
phpantom_lsp init --global
```

It lives at `$XDG_CONFIG_HOME/phpantom_lsp/.phpantom.toml` (typically
`~/.config/phpantom_lsp/.phpantom.toml` on Linux and macOS alike, and
`%APPDATA%\phpantom_lsp\.phpantom.toml` on Windows), takes exactly the
same keys as a project config, and is read first. macOS follows the XDG
path rather than `~/Library/Application Support`, which is where a
command-line tool's config is expected to be, and keeps the path the
same across a machine you use both platforms on. A project config is
then merged over it key by key, not wholesale, so a project only has to
spell out the settings where it differs from your defaults: with
`workspace = true` and `extra-arguments = true` set globally, a project
that sets only `workspace = false` still gets `extra-arguments`.

Both the global config and a project's own `.phpantom.toml` are
watched, and most settings take effect within a couple of seconds of
saving either file. The exceptions are settings that shape the initial
workspace scan, such as the PHP version and indexing strategy below --
those still need a restart to fully apply.

The full schema is at [`config-schema.json`](https://github.com/PHPantom-dev/phpantom_lsp/blob/main/config-schema.json).

### `[php]`

| Key       | Type   | Default                     | Description |
| --------- | ------ | --------------------------- | ----------- |
| `version` | string | Inferred from composer.json | Override the detected PHP version (e.g. `"8.3"`). |

### `[diagnostics]`

| Key                        | Type   | Default | Description |
| -------------------------- | ------ | ------- | ----------- |
| `unresolved-member-access` | bool   | `false` | Report `->`, `?->`, `::` where the subject is `mixed` or its type could not be worked out. Useful for type coverage, noisy on untyped codebases. |
| `extra-arguments`          | bool   | `false` | Report calls that pass more arguments than the function accepts. |
| `report-magic-properties`  | bool   | `false` | Report unknown property access on classes with `__get` when virtual properties are defined. Matches PHPStan's `reportMagicProperties`. |
| `downgrade-nullable-argument-mismatch` | bool | `false` | Downgrade `type_mismatch_argument` to a warning when the only reason an argument fails to satisfy the parameter is a stray `null` (every non-null member of the argument's type is already compatible). |
| `workspace`                | bool   | `false` | Compute diagnostics for the whole workspace in the background after startup, so problems appear for files you have not opened. Costs a project-wide sweep every session. Requires the default `full` indexing strategy. |
| `workspace-external`       | bool   | `true`  | Run configured external tools (PHPStan, PHPCS, Mago) once over the whole project after workspace diagnostics finish. Only takes effect when `workspace` is enabled. |

#### `[[diagnostics.ignore]]`

Rules that suppress matching diagnostics, similar to PHPStan's
`ignoreErrors`. Each rule may constrain by `message` (regex), `path`
(glob relative to workspace root), and/or `identifier` (diagnostic
code). A diagnostic is suppressed when it matches every constraint
present on a rule; omitted constraints match anything.

```toml
[[diagnostics.ignore]]
path = "tests/**"

[[diagnostics.ignore]]
identifier = "deprecated_usage"
message = "^Call to deprecated function some_legacy_helper\\(\\)"
```

### `[indexing]`

| Key          | Type     | Default  | Description |
| ------------ | -------- | -------- | ----------- |
| `strategy`   | string   | `"full"` | Class discovery strategy: `"full"`, `"composer"`, `"self"`, or `"none"`. See [Indexing Strategy](#indexing-strategy) below. |
| `exclude`    | string[] | `[]`     | Paths the workspace scanners skip, in gitignore syntax relative to the workspace root: a bare name matches at any depth, a pattern containing `/` anchors to the root, a trailing `/` restricts to directories, and a leading `!` re-includes. Applies to background discovery and to the directories `analyze` walks. A file you open in the editor, or name outright on the `analyze` command line, is always served. |
| `extensions` | string[] | `[]`     | Extra file extensions (without the dot) treated as PHP source during workspace discovery, e.g. `["module", "inc", "theme"]` for Drupal. `.php` is always included. |

```toml
[indexing]
exclude = ["generated", "web/sites/default/files"]
extensions = ["module", "install", "theme"]
```

### `[semantic_tokens]`

PHPantom defaults to `contextual` semantic tokens so editor syntax
highlighting remains in charge of ordinary PHP syntax.

| Key    | Type   | Default        | Description |
| ------ | ------ | -------------- | ----------- |
| `mode` | string | `"contextual"` | Semantic token mode: `"contextual"`, `"full"`, or `"off"`. |

| Mode | Behaviour |
| --- | --- |
| `"contextual"` | Emit only context-sensitive tokens that complement Tree-sitter/TextMate highlighting, such as parameters, PHPDoc template parameters, deprecated references, and static member accesses. |
| `"full"` | Emit the complete semantic token stream, including ordinary classes, variables, functions, methods, properties, comments, keywords, attributes, and Blade tokens. |
| `"off"` | Return no semantic tokens. |

### `[formatting]`

| Key            | Type    | Default | Description |
| -------------- | ------- | ------- | ----------- |
| `pint`         | string  | unset   | Command or path for Laravel Pint. Unset: auto-detect from `require-dev`. `""`: disable. |
| `pint-blade`   | boolean | unset   | Whether `.blade.php` files are formatted by Pint's `Pint/laravel_blade` rule. Unset: follow the workspace `pint.json`. `true`: send them to Pint with `--blade`. `false`: never; use the built-in Blade formatter. |
| `php-cs-fixer` | string  | unset   | Command or path for php-cs-fixer. Unset: auto-detect from `require-dev`. `""`: disable. |
| `phpcbf`       | string  | unset   | Command or path for phpcbf. Unset: auto-detect from `require-dev`. `""`: disable. |
| `timeout`      | integer | `10000` | Max runtime in milliseconds per external formatting tool. |

### `[phpstan]`

| Key            | Type    | Default  | Description |
| -------------- | ------- | -------- | ----------- |
| `command`      | string  | unset    | Command or path for PHPStan. Unset: auto-detect via `vendor/bin/phpstan` (only when the project has a PHPStan config file or `composer.json` requires `phpstan/phpstan`, or a Laravel-aware PHPStan extension such as `larastan/larastan` or `calebdw/phpstan-laravel`, directly) then `$PATH`. A Laravel application with neither such an extension nor a config file is left alone entirely, since plain PHPStan misreads the framework. `""`: disable. |
| `memory-limit` | string  | `"1G"`   | Memory limit passed to PHPStan via `--memory-limit`. |
| `timeout`      | integer | `60000`  | Max runtime in milliseconds before PHPStan is killed. |

### `[phpcs]`

| Key        | Type    | Default | Description |
| ---------- | ------- | ------- | ----------- |
| `command`  | string  | unset   | Command or path for PHPCS. Unset: auto-detect via `vendor/bin/phpcs` then `$PATH`. `""`: disable. |
| `standard` | string  | unset   | Coding standard to enforce (e.g. `"PSR12"`). Unset: PHPCS uses its own default detection. |
| `timeout`  | integer | `30000` | Max runtime in milliseconds before PHPCS is killed. |

### `[mago]`

Mago is only activated when `mago.toml` exists at the workspace root.

| Key               | Type    | Default | Description |
| ----------------- | ------- | ------- | ----------- |
| `command`         | string  | unset   | Command or path for Mago. Unset: auto-detect via `vendor/bin/mago` (only when `composer.json` requires `carthage-software/mago` directly) then `$PATH`. `""`: disable. |
| `lint`            | bool    | unset   | Proxy `mago lint` diagnostics. Unset: only when `mago.toml` has a `[linter]` table. |
| `analyze`         | bool    | unset   | Proxy `mago analyze` diagnostics. Unset: only when `mago.toml` has an `[analyzer]` table, and on Laravel only when it also wires up an extension. |
| `lint-timeout`    | integer | `30000` | Max runtime in milliseconds before `mago lint` is killed. |
| `analyze-timeout` | integer | `60000` | Max runtime in milliseconds before `mago analyze` is killed. |

Which of Mago's two diagnostic commands run follows the workspace `mago.toml`, since a project that uses Mago for one thing rarely wants the others. A `mago.toml` holding a `[formatter]` table and nothing else belongs to a project that formats with Mago and checks its code with something else, so neither `mago lint` nor `mago analyze` is proxied for it.

On a Laravel project, `mago analyze` additionally needs the `mago.toml` to wire up an extension, either an enabled `[extension-hosts.*]` entry or a namespaced plugin such as `plugins = ["acme/laravel"]`. Mago's analyser has no built-in Laravel support, so without one it cannot see through Eloquent or the facades and reports correct code in bulk. Mago's own plugins (`stdlib`, `psl`, `flow-php`, `psr-container`) do not count, since none of them supplies that knowledge. `mago lint` is unaffected, as its linter does have a Laravel integration.

Set `lint` or `analyze` explicitly to override all of this in either direction.

### `[laravel]`

#### `[laravel.schema]`

| Key       | Type     | Default              | Description |
| --------- | -------- | -------------------- | ----------- |
| `enabled` | bool     | `true`               | Enable Laravel schema dump scanning for Eloquent model property inference. |
| `paths`   | string[] | `["database/schema"]` | Schema dump files or directories to scan, relative to the workspace root. |

#### `[laravel.migrations]`

| Key       | Type     | Default | Description |
| --------- | -------- | ------- | ----------- |
| `enabled` | bool     | `true`  | Enable Laravel migration scanning for Eloquent model property inference. |
| `paths`   | string[] | unset   | Migration files or directories to scan. Defaults to non-vendor `database/migrations` directories. |

The file is optional. Unknown keys are silently ignored, so the file is forward-compatible.

## Editor-supplied file filters

Your editor already knows which folders it hides and which extensions it opens as PHP. PHPantom accepts the same two `[indexing]` lists from the editor over LSP, so that knowledge does not have to be mirrored into `.phpantom.toml` by hand:

```json
{ "indexing": { "exclude": ["generated"], "extensions": ["module"] } }
```

The server reads that shape from `initializationOptions` at startup and from `workspace/didChangeConfiguration` when you change your settings mid-session. It is also accepted namespaced under a `phpantom` key, which is how a client that pushes its whole settings tree sends it.

A notification that carries no `indexing` block leaves the filters as they are, since clients re-push their settings for reasons of their own. To clear the filters, send an `indexing` block with empty lists rather than omitting it.

A change made mid-session is reconciled against the index built under the previous filters, so neither direction needs a restart: classes under a path you just excluded leave the index, and files a removed exclude or an added extension brings back into scope are picked up by a fresh workspace scan a moment later. That scan is debounced, so a burst of settings changes costs one walk rather than one each. Editing `.phpantom.toml` is reconciled the same way. A file you have open in the editor is always served, whatever the filters say about it.

The values mean exactly what the [`[indexing]`](#indexing) keys of the same name mean: `exclude` is gitignore syntax relative to the workspace root, and `extensions` are extra file extensions (without the dot) treated as PHP source. Only those two keys are read; the indexing strategy stays a project decision.

Editor settings and `.phpantom.toml` are two layers of one filter set rather than one overriding the other, so:

- Both lists apply. A path either side excludes is excluded, and an extension either side names is indexed.
- Changing one never drops the other. Reloading `.phpantom.toml` keeps what the editor sent, and a settings change keeps what the file says.
- A `!` re-include in `.phpantom.toml` still wins over an exclude the editor sent, following the usual gitignore rule that the last matching pattern decides. This is how a project keeps one generated file indexed that its contributors happen to hide in their editors.

The interface is deliberately generic (a list of globs and a list of extensions, never an editor's own setting names), so any client can translate its native settings into it. See [Editor Setup](editor-setup.md) for what each editor does with it.

## Code Formatting

PHPantom ships a built-in PHP formatter (mago-formatter) that works out of the box, so `textDocument/formatting` requests are answered without any setup. The formatter is chosen per project in this order:

1. **Explicit config wins.** A tool path set under `[formatting]` in `.phpantom.toml` (`pint`, `php-cs-fixer`, or `phpcbf`) is always used. Setting a tool to `""` disables it.
2. **Composer `require-dev` wins over the built-in formatter.** If `composer.json` lists `laravel/pint`, `friendsofphp/php-cs-fixer`, or `squizlabs/php_codesniffer` in `require-dev`, PHPantom resolves the binary through Composer's bin-dir and runs it as a subprocess. A `phpcs.xml`, `.phpcs.xml`, `phpcs.xml.dist`, or `.phpcs.xml.dist` file at the workspace root certifies phpcbf the same way, so a project that only pulls `squizlabs/php_codesniffer` in transitively (e.g. through `slevomat/coding-standard`) is still detected. These tools discover their own project config (`pint.json`, `.php-cs-fixer.php`, `.phpcs.xml`, etc.) as they normally would.
3. **Otherwise, the built-in formatter is used.**

**Blade templates** are resolved on their own, since only Pint knows how to format one. A `.blade.php` file goes to Pint when the project's `pint.json` turns the `Pint/laravel_blade` rule on (or `pint-blade = true` asks for it, which also passes `--blade`), and otherwise to PHPantom's built-in Blade formatter. The built-in formatter reindents the template without changing the content of any line: indentation follows the nesting of directives, HTML and component tags, multi-line attribute lists and values, and brackets left open at the end of a line, using the editor's tab size and spaces-or-tabs setting. The bodies of `<script>`, `<style>`, and `@php` blocks are moved as a unit and keep their own layout; `<pre>`, `<textarea>`, and `@verbatim` contents are left exactly as written, as is anything between `{{-- blade-formatter-disable --}}` and `{{-- blade-formatter-enable --}}`. Templates whose indentation is output are never touched: Envoy task files, Markdown mail templates (under `resources/views/mail`, `resources/views/emails`, or `vendor/mail`, or using `<x-mail::` components), and Laravel Boost guidelines.

The built-in formatter defaults to the PER-CS 2.0 style. If a `mago.toml` is present at the workspace root, its `[formatter]` table is honoured instead, so PHPantom formats with the same preset and settings your project already uses with the Mago CLI:

```toml
# mago.toml
[formatter]
preset = "psr-12"
print-width = 100
use-tabs = false
```

For the full list of `[formatter]` options (presets, brace placement, blank-line handling, casing, and the rest), refer to the upstream Mago documentation: [Formatter configuration reference](https://mago.carthage.software/latest/en/tools/formatter/configuration-reference).

## Indexing Strategy

By default, PHPantom builds a full workspace index: it discovers PHP files, then background-parses user files to populate symbol maps and the reference candidate index. This gives complete cross-file references, implementation lookup, and workspace-wide navigation without per-feature scanning.

The `strategy` setting controls this behaviour:

| Strategy | Behaviour |
| --- | --- |
| `"full"` (default) | Scan PHP files, then background-parse user files to populate symbol and reference indexes. |
| `"composer"` | Use Composer's classmap when available, self-scan to fill gaps. Results stay closer to what `composer dump-autoload` knows about. |
| `"self"` | Ignore Composer's classmap entirely and scan every PHP file in the workspace. Discovers all classes regardless of autoloading. |
| `"none"` | Use only Composer's classmap with no fallback scanning. The most conservative option. |

Most projects should leave this at the default. Change it to `"composer"` or `"none"` only if you want a lighter or more Composer-constrained index.

## Troubleshooting

### Classes from other files are not found

PHPantom resolves cross-file classes through the full workspace index by default. If a class exists in your project but PHPantom reports it as unknown, the most common causes are:

1. **The file is excluded from the workspace walk.** Check ignored directories and `.gitignore` rules. If you explicitly set `strategy = "composer"` or `"none"`, classes outside Composer's autoload rules may be skipped.

2. **Composer's classmap is stale.** Run `composer dump-autoload` to regenerate it. PHPantom reads the classmap at startup.

3. **The class is in a directory not covered by `autoload` or `autoload-dev`.** Check that your `composer.json` PSR-4 mappings cover the directory where the class lives.
