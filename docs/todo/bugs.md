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

## Conditional return types

No outstanding items.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

No outstanding items.

## Array types

### B752. A required shape key removed by `unset()` is not reported

**Impact: Low-Medium · Complexity: Medium**

```php
/** @param array{foo: int} $shape */
function f(array $shape): void {
    unset($shape['foo']);
    takesFoo($shape); // expects array{foo: int}; nothing reported
}
```

The variable resolves to `array{}` correctly, but a missing required key
is only reported for an array literal at the call site
(`enumerates_all_keys` in `src/diagnostics/type_errors/arguments.rs`).
A shape held in a variable is treated as open, since the engine may not
have seen every key assigned. An explicit `unset()` of a key the
declared shape requires is not that kind of uncertainty: the key is
provably gone. The same applies to `$e = []; takesFoo($e);`, where the
value was a literal one assignment ago.

The shape needs to record whether its key set is known to be complete
(a literal, a sealed declared type, or one of those minus `unset` keys)
rather than the call site guessing from the argument's syntax.
(php-typing-conformance: `arrays_sealed_shape_mutation`.)

**Where to look:** the `unset` handling in the forward walker,
`missing_required_shape_keys` and `enumerates_all_keys` in
`src/diagnostics/type_errors/arguments.rs`.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Framework names in strings

No outstanding items.

## Name resolution

No outstanding items.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
