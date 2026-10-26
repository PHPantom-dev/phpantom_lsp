# PHPantom — External Stubs

This document covers how PHPantom can support external PHP stub files
beyond the built-in phpstorm-stubs embedded in the binary. External
stubs let users get type information for PHP extensions, framework
helpers, and IDE-specific annotations that the bundled stubs don't
cover or that the user wants to override.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## Current state

PHPantom embeds JetBrains phpstorm-stubs at compile time via
`build.rs`. The stubs are baked into the binary as static string
arrays and indexed by class, function, and constant name. Stub files
are parsed lazily on first access and cached under `phpantom-stub://`
URIs, and `src/stub_patches/` corrects or enriches individual
declarations after parsing (templates on the SPL iterator wrappers,
precise return types for many functions).

Built-in names otherwise resolve to the embedded stubs. In
`find_or_load_class`, Phase 0.5 sends any global-namespace class the
embedded stubs know straight to the stub, ahead of the classmap and
PSR-4, so a conditional polyfill in vendor cannot shadow it. Likewise,
an unguarded redeclaration of a built-in function in a project or
vendor file is skipped at parse time, because per-function signature
packages (`phpstan/php-8-stubs` and similar) are less precise than the
embedded stubs plus patches. Those guards are about incidental
redeclarations in ordinary project and vendor code (polyfills,
signature packages pulled in as dependencies).

A deliberate stub source is different: it is how a user overrides
internals, so it wins. Three exist today, the `[stubs] paths` of
`.phpantom.toml`, the stub packages Composer installed, and the stub
directory an editor extension passes as
`initializationOptions.stubs.path` (`src/indexing/stub_paths.rs`). All
are scanned into one `ExternalStubIndex`, the `[stubs]` paths first,
then the Composer packages, and the editor's directory last, first
declaration wins, with a directory holding `PhpStormStubsMap.php` read
from the map instead of scanned. The
class, function and constant lookups consult that index before anything
else (`src/resolution/external_stubs.rs`), so a stub declaration wins
over project code, vendor code, and the embedded stubs alike, and the
embedded copy of an overridden built-in is never read. A stub file is
parsed under its own `file://` URI (so go-to-definition lands in it and
the watcher's re-parse covers it), is registered as library code (so it
is never analysed, renamed into, or searched for references), and gets
no `src/stub_patches/` corrections unless it is a phpstorm-stubs
checkout, which has the embedded copy's gaps. The index is rebuilt
whole, dropping everything parsed from the old and new stub files,
whenever the paths change, a file under one changes, or `composer.lock`
changes.

A Composer package is a stub package when its name is
`jetbrains/phpstorm-stubs`, matches `php-stubs/*` or `*-stubs`, or its
`type` is `phpstorm-stubs` or `php-stubs` (`src/composer/installed.rs`),
except `phpstan/php-8-stubs` (signature-only declarations of every
built-in function, less precise than the embedded stubs) and
`php-stubs/generator` (a tool). Each Composer project of the workspace,
monorepo subprojects included, contributes its own. An installed
`jetbrains/phpstorm-stubs` is therefore what go-to-definition on a
built-in lands in.

What is still missing:

- **Version lag** for anyone not pointing a stub path at newer stubs:
  the embedded stubs are pinned to whatever version of phpstorm-stubs
  was installed when the binary was built.

---

## Where stubs fit among the indexing settings

PHPStan, Psalm, PhpStorm, and Intelephense all give a path one of the
same three roles, plus a way to skip it:

| Role | PHPStan | Psalm | PhpStorm | Intelephense |
| ---- | ------- | ----- | -------- | ------------ |
| Project code: indexed and reported on | `paths` | `<projectFiles>` | content roots | the workspace |
| Library code: indexed, not reported on or edited | `scanFiles`, `scanDirectories` | `<extraFiles>` | Include Path | `environment.includePaths` |
| Overrides: declarations that win | `stubFiles` | `<stubs>` | custom stubs path | `stubs` |
| Skipped | `excludePaths` | `<ignoreFiles>` | Excluded | `files.exclude` |

PHPantom covers the same roles with three user-facing keys, and finds
everything else on its own:

- **`[indexing] exclude`** skips. A `!` only undoes the project's own
  patterns.
- **`[indexing] include`** brings back what the workspace scan skips by
  default (hidden, gitignored, the laravel-ide-helper files). Where the
  path lives picks the role: inside the workspace it is project code,
  outside it is library code, registered like a vendor directory. That
  is what stands in for a separate `scanDirectories` key. Included paths
  fill gaps and never win over a declaration found elsewhere.
- **`[stubs] paths`** is the only way to make a declaration win.

Composer stub packages, the editor's stub directory, sibling editor
folders, and Drupal's web root are detected or forwarded, never typed by
the user. Keep it that way: every new source of PHP should map onto one
of the three roles rather than add a fourth key with its own semantics.

---

## Stub sources (in priority order)

When the same symbol is defined by multiple sources, the first source
wins:

1. **`.phpantom.toml` `[stubs] paths`**, in the order written. An
   explicit choice for this project, so it wins over everything.
2. **Composer stub packages**, from the vendor directory of each
   Composer project, in `installed.json` order.
3. **The editor's stub directory** (`initializationOptions.stubs.path`).
   An extension's bundled stubs, below the project's own choices.
4. **Embedded stubs.** The final fallback, not in the stub index; every
   other source overrides them. When PHPantom is built without embedded
   stubs, this source is empty.

Sources 1 to 3 win over project and vendor code too. A stub declaration
replaces the whole declaration of its name: a stub class does not merge
its members into the real class (E7 needs that merge and does not have
it).

---

## Open questions

### How does this interact with the classmap?

External stub packages installed via Composer may appear in the
classmap (`autoload_classmap.php`). This is fine: the classmap feeds
`fqn_uri_index`, which Phase 1 of `find_or_load_class` consults, and any class
found there is parsed and cached normally. The stub index serves as a
parallel discovery path for stub packages that are `require-dev`
dependencies (which may not be in the classmap if the user ran
`composer install --no-dev` in production).

In practice, most stub packages declare their classes in
`autoload.classmap` in their own `composer.json`, so they do appear
in the generated classmap. The stub index provides a safety net and is
also needed for function and constant stubs (which the classmap does
not cover).

### What about `phpstan-extension-installer` and PHPStan config?

Some projects configure stub files through `phpstan.neon`:

```neon
parameters:
    stubFiles:
        - stubs/MyCustomStub.php
```

Reading PHPStan config is out of scope. PHPantom is not PHPStan and
does not derive its settings from other tools' configuration. If users
want PHPantom to see these stubs, they can add the path to
`[stubs] paths` in `.phpantom.toml`.

### Building without embedded stubs

The `build.rs` script already handles a missing `stubs/` directory
gracefully by generating empty arrays. If the automatic GitHub
fetch fails (e.g. no network access during the build), the binary
compiles and runs normally; it just has no built-in fallback for
PHP standard library symbols.

For this to work, stubs must come from another source. The most
reliable combinations:

- The editor extension passes its bundled stubs as
  `initializationOptions.stubs.path`.
- The user points at stubs via `.phpantom.toml` `[stubs] paths`.
- The user's project has `jetbrains/phpstorm-stubs` in Composer.

Any of these is sufficient. Without any external stubs and without
embedded stubs, built-in symbols would be invisible.

---

## Summary

| #   | Goal                                                      | Complexity  | Dependencies                                       |
| --- | --------------------------------------------------------- | ----------- | -------------------------------------------------- |
| E7  | Stub-based framework patches (replace Rust patch system)  | Medium-High | None                                               |

The priority order (`.phpantom.toml` > Composer > editor > embedded)
ensures the user's explicit choices always win. Most users never
touch `.phpantom.toml` and will get stubs through Composer (automatic)
or their editor extension (transparent). The toml paths exist for
overrides, non-Composer projects, and edge cases.

---

## E7. Stub-based framework patches

**Impact: Medium · Complexity: Medium-High**

Replace the Rust-coded Laravel class patch system
(`virtual_members/laravel/patches.rs`) with plain PHP stub files that
override specific declarations. Instead of patching return types in
Rust after resolution, ship corrected stubs that the normal stub
loading pipeline picks up at a higher priority than the framework's
own declarations.

### Motivation

The current patch system (`apply_laravel_patches`) fixes framework
type inaccuracies by mutating resolved `ClassInfo` in Rust code.
This works but has drawbacks:

- **Contributor barrier.** Adding a patch for a new framework (Symfony,
  WordPress, Drupal) or fixing a Laravel type requires Rust knowledge.
- **Maintenance burden.** Framework updates may change signatures;
  keeping Rust code in sync is harder than updating a PHP stub file.
- **Not user-extensible.** Users cannot add their own patches for
  project-specific quirks without forking PHPantom.

Stub overrides solve all three. A PHP developer who understands the
framework can write a corrected stub, submit a PR, and never touch
Rust. Users can drop override stubs into their `.phpantom.toml`
`[stubs] paths` to fix types locally.

### What the stubs would look like

For the Conditionable `when()`/`unless()` patch, the override stub
would be:

```php
namespace Illuminate\Support\Traits;

trait Conditionable {
    /** @return $this */
    public function when(mixed $value = null, ?callable $callback = null, ?callable $default = null): mixed {}

    /** @return $this */
    public function unless(mixed $value = null, ?callable $callback = null, ?callable $default = null): mixed {}
}
```

For the Eloquent Builder `__call` patch:

```php
namespace Illuminate\Database\Eloquent;

class Builder {
    /** @return static */
    public function __call(string $method, array $parameters): mixed {}

    /** @return static */
    public static function __callStatic(string $method, array $parameters): mixed {}
}
```

These are standard PHP stub files. They declare only the members that
need overriding. The stub loading pipeline merges them at a higher
priority than the framework's own declarations, so the corrected
`@return` types win.

### Bundled override stubs

PHPantom would ship a set of override stubs for common frameworks,
organized by framework:

```
stubs/overrides/
├── laravel/
│   ├── Conditionable.stub.php
│   ├── EloquentBuilder.stub.php
│   └── ...
├── symfony/
│   └── ...
└── wordpress/
    └── ...
```

These would be embedded at build time (like phpstorm-stubs today) and
loaded into the stub index at a priority above vendor stubs but below
user `.phpantom.toml` paths. Framework detection (already done for
Laravel via `composer.json` inspection) controls which set is loaded.

### User-provided override stubs

Users can already place their own stubs in a `.phpantom.toml`
`[stubs] paths` directory. Since user paths have the highest priority,
they will override both bundled overrides and vendor stubs, which makes
the system fully extensible without code changes once step 1 of the
migration below lets a stub declare only the members it overrides.

### Migration path

1. Add a partial-class merge: an override stub declares only some
   members, so loading it has to replace those members on the real
   class and keep the rest. Today a stub path's class replaces the
   real class whole (`load_external_stub_class`).
2. Write override stubs for the patches in `patches.rs` that a plain
   declaration can express (return-type and generic fixes such as
   Builder `__call`, Conditionable `when`/`unless`, the paginate
   element type, the Cache facade generics).
3. Add a bundled-overrides source to the stub index, between vendor
   stubs and user stubs.
4. Remove the Rust patch functions those stubs replace. Several
   patches are not expressible as a stub and stay in Rust: metadata
   such as higher-order-proxy property tagging, mock return types that
   dispatch on a signature scan, and Conditionable applied to any class
   that uses it.
5. Document how to contribute framework override stubs (just PHP,
   no Rust needed).

The phpstorm-stubs patch system (`src/stub_patches/`) is an equally
good candidate for the same treatment once the mechanism exists.

### Scope of the Rust patch system until then

The current `patches.rs` module handles the known cases correctly
and is well-tested. It stays in place until the partial-class merge
above is ready. New patches can still be added
in Rust in the interim.
