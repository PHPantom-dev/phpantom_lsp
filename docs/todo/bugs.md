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

### B324. Two anonymous classes at the same offset share one resolution

**Impact: Medium · Complexity: Medium**

`parser/anonymous.rs` names an anonymous class `__anonymous@<offset>`
from the offset of its opening brace *within its own file*, and
`resolution.rs` deliberately keeps those synthetic names out of the
workspace declaration index. So the name is unique per file and nothing
makes it unique across the workspace.

The resolved-class cache is keyed by fully-qualified name, and
`resolve_class_fully_inner` cannot reload an anonymous class from the
class loader (it is not in the index), so it falls back to the
`ClassInfo` it was handed and caches the result under that name. Two
files whose anonymous class sits at the same byte offset in the same
namespace therefore collide: whichever resolves first answers for both.

Boilerplate-identical files make this ordinary rather than exotic. A
Laravel anonymous migration is `return new class extends Migration {`
after a fixed `<?php` header and three `use` lines, so a project's
migrations all put the brace at the same offset. Reproduced with two
global-namespace files that differ only in the method their anonymous
class declares: completing on the second file's instance offers the
first file's method and not its own.

The name has to carry the file as well as the offset, or the cache key
does. Changing the synthetic name is the smaller change but it is
user-visible (it reaches hover and the outline), so the FQN it is built
from and the places that match on the `__anonymous@` prefix both have to
be checked.

**Where to look:** `AnonymousClassWalker` in `parser/anonymous.rs`, the
`__anonymous@` filters in `resolution.rs` and `parser/ast_update.rs`, and
the `cache_key` in `resolve_class_fully_inner`
(`virtual_members/resolve.rs`).

## Array types

No outstanding items.

## Docblock handling

No outstanding items.

## Miscellaneous

No outstanding items.
