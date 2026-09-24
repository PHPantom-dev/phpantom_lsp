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
