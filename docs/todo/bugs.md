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

### B744. A nullsafe chain compared greater than a value does not prove its receiver

**Impact: Low · Complexity: Low**

```php
if ($line->product?->supply_amount > 0) {
    Cart::add($line->product); // still ?Product
}
```

`null > $x` is false for every `$x`, so a true `$a?->b > $x` (or
`$x < $a?->b`) proves `$a` is not null, the same proof
`apply_nullsafe_receiver_narrowing` already draws from a truthy chain or an
identity check against a non-null value. Only the strict greater-than
direction carries it: `null >= 0` and `null < 1` are both true. The miss
only shows with `possibly-empty-mismatches` on, as a nullable argument
mismatch on the receiver.

## Array types

No outstanding items.

## Laravel

### B745. A singular relationship with `withDefault()` is typed nullable

**Impact: Low · Complexity: Medium**

```php
public function author(): BelongsTo
{
    return $this->belongsTo(User::class)->withDefault();
}
// $post->author is ?User, but withDefault() hands back an empty User
// instead of null
```

`build_property_type` makes every `hasOne`, `belongsTo`, `morphOne` and
`hasOneThrough` property nullable, following the `?TRelatedModel` result
Laravel declares on the relation class. `withDefault()` returns the same
relation type, so nothing in the return type tells the two apart; only the
method body does. The fact has to be read from the body at extraction (as
`config_related_models` is) and reach the model provider for relationship
methods declared in traits and parent classes too, so a per-class list on
`LaravelMetadata` alone is not enough. Larastan has the same gap. The
nullable type only shows on hover and with `possibly-empty-mismatches` on.

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
