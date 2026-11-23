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

### B759. A nullsafe call's `null` sticks to the plain call that repeats it

**Impact: Low · Complexity: Medium**

```php
function run(?Maker $m): void {
    if ($m?->make(1) !== '') {}
    take($m->make(1)); // reported: expects string, got null|string
}
```

`expr_to_subject_key` keys `$m?->make(1)` and `$m->make(1)` alike, and a
call key is seeded from the expression it was written as, so the entry
carries the `null` the nullsafe operator adds when `$m` is null. A later
plain `$m->make(1)` only runs with `$m` set, yet reads that entry. The same
holds for an argument-less call whose conditional return type is seeded
from its expression. Sharing the key is what lets
`if ($m?->get() !== null) { $m->get()->x(); }` narrow, so the fix is to
seed the key with what the call returns once its receiver is known to be
set, not to split the keys (`forward_walk/cond_narrowing/seeding.rs`,
`seed_call_subject` and `seed_synthetic_key`).

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
