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

### B747. "Create route" ignores the name prefix the target routes file is loaded under

**Impact: Low · Complexity: Low-Medium**

The "Create route" quick fix on an unknown `route('…')` name appends a
`->name('…')` registration with the full name to `routes/web.php` or
`routes/api.php`. When the app loads that file under a named group, the
group's prefix is applied a second time:

```php
// bootstrap/app.php
->withRouting(then: function () {
    Route::name('api.')->group(base_path('routes/api.php'));
})

// app/Http/Controllers/UserController.php
route('api.users.index'); // quick fix appends ->name('api.users.index')
                          // to routes/api.php, registering api.api.users.index
```

The unknown-route diagnostic stays, and the route the fix wrote has the
wrong name. Route discovery already reads such a file with its group's
prefix (`enumerate_all_routes` walks `bootstrap/app.php` first), so the
fix can ask the route table which prefix the target file is loaded under:
strip it from the generated name when the requested name starts with it,
and pick the other file (or decline) when it does not.

## Miscellaneous

### B746. `self` inside a trait means the trait alone outside parameter hints

**Impact: Low · Complexity: Medium**

A parameter typed `self` inside a trait, and `$this`, both see the class
the trait's `@phpstan-require-extends` or `@phpstan-require-implements`
names. Every other place `self` is written in the trait still resolves to
the trait alone, so the required class's members are unknown there:

```php
abstract class Model {
    public function getTable(): string { return ''; }
}

/** @phpstan-require-extends Model */
trait Stamps {
    public ?self $parent = null;

    public function fluent(): self { return $this; }

    public function run(self $model): void {
        $model->fluent()->getTable(); // Method Stamps::getTable() not found
        $model->parent?->getTable();  // Method Stamps::getTable() not found
        $this->fluent()->              // offers only the trait's methods

        /** @var self $m */
        $m = make();
        $m->getTable();               // Method Stamps::getTable() not found
    }
}
```

The bounds are added where `$this` and parameters are seeded
(`trait_context::extend_this_with_trait_bounds`), not where `self` is
resolved, so return types, property types and `@var` tags miss them.
Resolving `self` in a trait to the trait plus its bounds in one place
would cover all of them, including a `self|Foo` parameter, which the
parameter seeding leaves to the trait alone.
