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

### B748. Route files pulled in by another file are read from disk, not the editor

**Impact: Low · Complexity: Low-Medium**

Route discovery reads a route file a group includes
(`Route::name('api.')->group(base_path('routes/api.php'))` in
`bootstrap/app.php`, or `Route::group([], __DIR__ . '/sub.php')`) straight
from disk, while a top-level route file is read through the editor's
buffer. Unsaved edits to an included file are invisible until it is
saved:

```php
// bootstrap/app.php
->withRouting(then: function () {
    Route::name('api.')->group(base_path('routes/api.php'));
})

// routes/api.php, edited but not saved
Route::get('/users', …)->name('users.index');

// app/Http/Controllers/UserController.php
route('api.users.index'); // still reported as an unknown route
```

This also means applying the "Create route" quick fix to such a file
leaves the diagnostic in place until the file is saved. The include
traversal (`open_included_file`) needs to read through the same
buffer-aware path `walk_workspace_route_file` uses.

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
