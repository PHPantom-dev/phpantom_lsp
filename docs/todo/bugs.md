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

### B568. A passing strict `in_array()` against `object` elements drops the needle's classes

**Impact: Low · Complexity: Low-Medium**

```php
/** @param list<object> $handlers */
function f(Foo|Bar|string $x, array $handlers): void {
    if (in_array($x, $handlers, true)) {
        $x->run(); // `$x` reads `string`, though the object it equals can be a `Foo` or a `Bar`
    }
}
```

In the branch where the check held, `apply_in_array_narrowing`
(`cond_narrowing/in_array.rs`) narrows the needle's class layer through
`apply_instanceof_inclusion`, which keeps only the classes the element type
resolves to. An element that names no class it can load, such as `object`
or a class that is not found, resolves to none, so every class goes and
only the needle's scalar alternatives are left. The branch already skips an
element that could be anything (`mixed`), but `object` can be any object as
well, and a class that cannot be loaded may be an ancestor of the needle's.

**Fix:** Narrow the class layer only when every element alternative that
can hold an object names a class that loads, and leave the needle's classes
alone otherwise.

## Arithmetic

No outstanding items.

## Symbol resolution

No outstanding items.

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Miscellaneous

