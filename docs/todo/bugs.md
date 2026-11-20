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

### B757. On Windows, a route file reached through an include ignores its open buffer

**Impact: Medium · Complexity: Low-Medium**

`routes/web_backstage.php` is loaded only through
`Route::name('admin.')->group(base_path('routes/web_backstage.php'))` in
`bootstrap/app.php`. On Windows, adding a route to it in the editor
without saving does not register the route: `route('admin.patient.index')`
stays reported as an unknown route until the file is saved. The same
happens after applying "Create missing route" to such a file, since its
edit is unsaved. Linux is unaffected. Two tests fail on the Windows CI
runner:

- `laravel_route_names::an_unsaved_edit_to_a_file_a_group_loads_is_seen`
  (`["Unknown route 'admin.patient.index'"]`)
- `code_action_create_missing_route::create_missing_route_strips_the_prefix_its_target_file_is_loaded_under`
  (the diagnostic is still there after the fix is opened as a buffer)

`open_included_file()` canonicalizes the include target with
`dunce::canonicalize`, and `read_route_source()` turns that path back into
a URI with `path_to_uri()` and looks it up in `open_files` by exact
string. The buffer is keyed by the URI the client sent, and on Windows the
two spell the same file differently (a temp dir under an 8.3 short name
such as `RUNNER~1` expands when canonicalized, and the drive letter's case
and encoding can differ), so the lookup misses and the file is read from
disk. The lookup should compare files by canonical path, the way
`RouteDiscovery::sources` already does, rather than by the string of a
rebuilt URI. Check the other `open_files.get(&path_to_uri(..))` callers
for the same mismatch.

**Where to look:**
`src/virtual_members/laravel/route_names/traversal.rs`
(`read_route_source`, `open_included_file`).

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
