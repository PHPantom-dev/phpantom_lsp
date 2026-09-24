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

## Laravel

## B342. Published package views under `resources/views/vendor` are ignored

**Impact: Low-Medium · Complexity: Medium**

`loadViewsFrom($path, 'widgets')` registers `resources/views/vendor/widgets`
*ahead of* the package's own directory, so a published copy is what renders
and a view only the published directory holds is still `widgets::name`.
Neither `scan_view_names` (`src/blade/discovery.rs`) nor
`resolve_view_definitions` (`src/virtual_members/laravel/view_names.rs`)
looks there.

**Tests:** `laravel_view_names::a_published_copy_of_a_package_view_wins_over_the_original`,
`a_view_only_the_published_directory_holds_is_known`, and
`a_published_package_template_is_typed_by_its_namespaced_call_site`.

## B358. A string key argument is read from its raw source, not its value

**Impact: Low · Complexity: Low**

`config('app.it\'s')` names the key `it's`, but `push_laravel_string_span`
(`src/symbol_map/extraction/laravel.rs`) slices the literal's source text, so
the call site records `app.it\'s` and the unknown-key diagnostic flags a key
the config declares. The same helper feeds every Laravel string kind (routes,
views, translations), so any key spelled with an escape sequence is misread.
The key should come from the literal's unescaped `value`; the span stays on
the source text.

## Blade

No outstanding items.

## Miscellaneous

No outstanding items.
