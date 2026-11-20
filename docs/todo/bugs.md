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

### B757. `array_reduce()` with a callable-array callback returns `null`

**Impact: Low-Medium · Complexity: Low-Medium**

```php
class C {
    /** @return array */
    public static function reduceOrder($array, $a) {
        $array = $array ?: [];
        if (!empty($a['children'])) {
            // reported: Argument 2 ($arrays) expects array, got null
            $array = array_merge($array, array_reduce($a['children'], [static::class, 'reduceOrder']));
        }
        return $array;
    }
}
```

The callback `[static::class, 'reduceOrder']` returns `array`, but the
call resolves to plain `null`: `TCarry` binds nothing from a callable
array, so only the `null` of `TCarry|null` survives. Even once it binds,
the `null` stays, because the stub patch drops it only when `$initial` is
passed. An array the call already knows is non-empty (here through
`!empty()`) never reaches the empty-array case that returns `null`
either. Drupal's `FieldUiTable::reduceOrder()` hits this.

**Where to look:** `patch_array_reduce` in
`src/stub_patches/functions.rs`, and the `CallableReturnType` binding in
`src/type_engine/call_resolution/template_binding.rs`.

## Reachability

No outstanding items.

## Narrowing

No outstanding items.

## Array types

No outstanding items.

## Laravel

### B758. Constraint closures passed to a collection's `load()` are untyped

**Impact: Low-Medium · Complexity: Low-Medium**

```php
/** @var \Illuminate\Database\Eloquent\Collection<int, Post> $posts */
$posts->load(['comments' => fn ($query) => $query->with('author')]);
// $query is untyped: no completion after `$query->`, and
// go-to-definition on 'author' finds nothing.
```

A model's `load()` and `loadMissing()` type the closure as the relation,
like `with()`, but Eloquent's `Collection::load()` and `loadMissing()`
pass the same array to `with()` on the first model's query and are not
recognised. Two things stop them: the framework check only counts
declarations on `Builder`, `QueriesRelationships` and `Model`, and
`find_models_from_receivers` cannot read the model out of a
`Collection<int, Post>` receiver.

**Where to look:** `eager_callback_arguments`,
`declared_relation_method` and `find_models_from_receivers` in
`src/type_engine/variable/forward_walk/relation_callbacks.rs`.

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
