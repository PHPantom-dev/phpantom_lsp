# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

Each entry below carries an **Impact · Complexity** rating using the same
scale defined in [`docs/todo.md`](../todo.md); that table is also where
each bug's row lives in the current sprint/backlog.

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

No outstanding items.

## Arithmetic

No outstanding items.

## Symbol resolution

No outstanding items.

## Array types

No outstanding items.

## Docblock handling

No outstanding items.

## Miscellaneous

## B334. The Blade view roots never notice a `resources/views` created later

**Impact: Low · Complexity: Low**

`laravel_view_roots` (`src/blade/view_paths.rs`) keeps only the configured
directories that exist when it is first built, caches an empty list when
the workspace root is not yet known, and is reset only when
`config/view.php` changes (`LaravelStringKeyCache::invalidate_for_uri` in
`src/lib.rs`). A project whose `resources/views` directory is created after
the first Blade request, or whose roots were computed before `initialize`
set the workspace root, resolves no view names until `config/view.php` is
edited. The provider-resource republish does not reset the roots either.

**Fix:** reset `view_roots` when a directory a configured root names
appears (the watcher already reports created files under it), and do not
cache the empty answer taken before the workspace root is known.
