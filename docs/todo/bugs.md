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

## B332. A commented-out `@use` counts as a template import

**Impact: Low · Complexity: Low**

`use_directive_arguments` (`blade/use_directive.rs:16`) finds `@use` with
a plain `find`. It neither masks Blade comments nor checks a word
boundary before the `@`. So `{{-- @use('App\Foo') --}}` is taken as a
real import by `analyze_template_use_block` (which BL1's import
insertion builds on) and rewritten by the template rename in
`rename/blade.rs:32`, while the preprocessor ignores it.

**Fix:** mask with `signature::inert_regions` and require a directive
boundary (`directives::directive_head`), as the other Blade scanners do.
