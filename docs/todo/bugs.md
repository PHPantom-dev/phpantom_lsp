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

#### B1. A closure parameter hint matches an inferred generic by short name

**Impact: Low-Medium · Complexity: Low**

When a closure is passed where the callee declares a callable signature,
`inferred_type_is_more_specific` lets the inferred generic type replace
a bare class hint if the two share a short name. A hint that names an
unrelated class with the same short name is overwritten:

```php
namespace App;

class Collection { public function own(): void {} }

/** @param callable(\Illuminate\Support\Collection<int, Loaf>): void $cb */
function takesCollection(callable $cb): void {}

takesCollection(function (Collection $c) {
    $c->own();   // $c is typed Illuminate\Support\Collection<int, Loaf>
});
```

The parameter is `App\Collection` at runtime (PHP enforces the hint), so
completion offers the wrong class's members and diagnostics check against
it. The argument-level diagnostic that reports the callable mismatch is
correct; only the type inside the closure is wrong.

**Where to change:** the short-name comparison in
`inferred_type_is_more_specific`
(`src/type_engine/variable/closure_resolution.rs`). The hint is already
resolved to a FQN, so the subtype check below it covers the legitimate
cases (`Collection` imported as `Illuminate\Support\Collection`) on its
own.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

No outstanding items.

## Array types

No outstanding items.

## Laravel

#### B2. Relation callbacks on a relation receiver lose the related model

**Impact: Medium · Complexity: Medium**

Relation-query callbacks are typed from the receiver's model, but a
relation receiver is not recognised as carrying one. Both calls below
reach the callback with the declared fallback type rather than the
related model:

```php
class Team extends Model {
    /** @return HasMany<Stock, $this> */
    public function stocks(): HasMany { return $this->hasMany(Stock::class); }
}
class Stock extends Model {
    /** @return BelongsTo<Warehouse, $this> */
    public function warehouse(): BelongsTo { return $this->belongsTo(Warehouse::class); }
}

$team->stocks()->whereHas('warehouse', fn ($q) => $q);  // Builder<Model>, expected Builder<Warehouse>
$team->stocks()->with('warehouse', fn ($r) => $r);      // Relation<mixed, mixed, mixed>, expected BelongsTo<Warehouse, Stock>
```

Both forms run at runtime: `Relation` forwards `whereHas()` and `with()`
to its query, whose model is the relation's related model.

**Where to change:** `find_models_from_receivers` in
`src/type_engine/variable/closure_resolution.rs` reads a model from a
builder's generic argument, `getModel()` or builder methods. A
`Relation<TRelated, …>` receiver should yield `TRelated` through the
same ancestor projection custom relation subclasses already use.

#### B3. Builder methods called through a relation drop the relation

**Impact: Low-Medium · Complexity: Medium**

`Relation::__call` forwards unknown methods to the related query and
returns the relation itself when the forwarded call returns the builder.
Custom builder methods do not follow this:

```php
#[UseEloquentBuilder(LoafBuilder::class)]
class Loaf extends Model {}   // LoafBuilder::stale() returns $this

$bakery->baguettes()->stale();         // no type, expected HasMany<Loaf, Bakery>
$bakery->baguettes()->where('a', 1);   // HasMany<Loaf, Bakery>, correct

Bakery::withWhereHas('baguettes', function ($q) {
    $q->stale();   // LoafBuilder<Loaf>, expected LoafBuilder<Loaf>|HasMany<Loaf, Bakery>
});
```

`where()` works because the stubs declare it on `Relation`. Methods that
only exist on the related model's custom builder (scopes, builder
methods) are not forwarded at all on a plain relation, and on the
`withWhereHas()` builder/relation union they narrow to the builder arm.

**Where to change:** the `Relation` member forwarding in
`virtual_members/laravel/` should include the related model's custom
builder methods, rewriting a builder-typed return to the relation the
way `transform_flags::FORWARDS_CALLS_SELF` already does for
`ForwardsCalls` decorators.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Miscellaneous

No outstanding items.
