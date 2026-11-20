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

### B758. A truthiness check leaves a `mixed` subject unrefined

**Impact: Low-Medium · Complexity: Medium**

```php
class C {
    /** @return array */
    public static function reduceOrder($array, $a) {
        $array = $array ?: [];
        if (!empty($a['children'])) {
            // reported: Argument 2 ($arrays) expects array, got array|null
            $array = array_merge($array, array_reduce($a['children'], [static::class, 'reduceOrder']));
        }
        return $array;
    }
}
```

`!empty($a['children'])` (and `if ($x)`) keeps a `mixed` subject as plain
`mixed`, so nothing after the check knows the value is truthy. Handed to a
parameter declared `array`, a truthy value can only be a non-empty array,
which is what lets `array_reduce()` drop the `null` it returns for an empty
one. PHPStan narrows such a subject to `mixed~(0|0.0|''|'0'|array{}|false|null)`;
we have `non-empty-mixed` for the same idea but never produce it. Two
pieces: the truthy narrowing of `mixed` (`PhpType::truthy_type` and the
`!empty()` arm in `forward_walk/cond_narrowing/null_narrowing.rs`), and a
conditional return that decides `$array is non-empty-array` against the
argument held to the parameter's native type
(`type_engine/type_ops/conditional.rs`). An untyped parameter (`$c` with no
hint) gets no type at all after `!empty($c)`, which is the same gap.
Drupal's `FieldUiTable::reduceOrder()` hits this.

## Array types

No outstanding items.

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
