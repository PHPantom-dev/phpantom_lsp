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

## B360. References, reference lenses, and rename miss an Eloquent magic member's uses

**Impact: Medium · Complexity: Medium-High**

A scope, accessor, or mutator is declared under one name and used under
another: `scopeActive()` is called as `active()`, `getFullNameAttribute()`
and an `Attribute`-returning `fullName()` are read as `->full_name`,
`setLogoAttribute()` is written as `->logo = …`. Go-to-definition follows
a use back to its declaration (`src/definition/member/`), but nothing under
`src/references/` maps the declaration forward, so find-references on the
declaring method finds only direct calls to it. The member reference
lens (`indexed_member_reference_count` in `src/code_lens.rs`) counts the
same way, so live scopes and accessors read "0 references", and a
rename of the declaration strands every use. Relationship methods have
the same shape through their `->posts` property reads. Pinned by the
four ignored `definition_laravel::references_on_*` tests.

## Blade

No outstanding items.

## Miscellaneous

No outstanding items.
