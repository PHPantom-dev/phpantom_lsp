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

## B367. A class name in a file with several braced namespaces resolves against the first one

**Impact: Low-Medium · Complexity: Medium**

In a file that declares more than one `namespace Foo { … }` block, a
member access's receiver written as a bare class name is resolved
against the file's *first* namespace rather than the block it sits in.
With `namespace Other { … }` followed by `namespace App { class Author
{ public static function make() {} } function show() { Author::make(); } }`,
the receiver of `Author::make()` resolves to `Other\Author`, so Find
References on `make()` finds nothing. The reference search
(`src/references/receivers.rs`, `member_scope.rs`) resolves receivers
through `file_context`, whose `namespace` is the first one the file
declares, rather than `file_context_at`, which picks the namespace block
that contains the access.

## Array types

No outstanding items.

## Docblock handling

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Miscellaneous

No outstanding items.
