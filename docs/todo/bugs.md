# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

Each entry below carries an **Impact · Complexity** rating using the same
scale defined in [`docs/todo.md`](../todo.md), but a bug's row lives
**here only** — do not add or link a bug entry to `docs/todo.md`'s sprint
or backlog tables. This file is its own list, not a domain document
sprint items draw from: whenever it holds anything, that is actively
addressed, independently of sprint planning.

Bugs land here from wherever they surface: found while working on another
task, or sweeps of the sample projects under `projects/`. Entries are
grouped by the mechanism that has to change, not by the symptom that
surfaced: one entry is one root cause, however many shapes it shows up in.

## Crashes

No outstanding items.

## Type comparison

No outstanding items.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

No outstanding items.

## Arithmetic

No outstanding items.

## Symbol resolution

No outstanding items.

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

### B544. Blade directives are recognized whether or not the installed Laravel has them

**Impact: Medium · Complexity: Medium**

```blade
<script type="application/ld+json">
    {
        "@context": "https://schema.org",
        "@type": "BreadcrumbList"
    }
</script>
```

On Laravel 8 this is plain text: Blade only compiles an `@name` it has a
`compileName()` method (or a registered custom directive) for, and
`@context` arrived with `CompilesContexts` in Laravel 11. The preprocessor
lowers every name in the fixed list in `blade/directives.rs` regardless of
version, so `"@context":` opens an `if` block and the rest of the JSON-LD
reports a cascade of syntax errors (around 200 in one Laravel 8 project,
from schema.org snippets alone). The recognized set should come from the
installed compiler, by reading which `compile*` methods
`Illuminate\View\Compilers\BladeCompiler` and its `Concerns` traits
actually define, the way the alias tables are read from the installed
framework, with the fixed list as the fallback when no framework is
installed.

## Templates

No outstanding items.

## Miscellaneous

No outstanding items.
