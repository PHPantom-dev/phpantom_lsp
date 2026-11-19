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

No outstanding items.
