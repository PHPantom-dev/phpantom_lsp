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

### B322. A method that implements or overrides a prototype gets no reference count

**Impact: Low-Medium · Complexity: Low**

`handle_code_lens` builds the reference lens for a method only when
`find_prototype` came back empty, so a method that implements an
interface or overrides a parent loses its count and keeps only the
`◆ Interface::method` navigation lens. Deleting the `implements` clause
makes the count reappear, which is how the reporter of github #412 found
it: the very methods a project most wants a usage count for, the ones
behind a contract, are the ones that never show one.

The two lenses answer different questions and both fit on the line, the
way a property carrying a count and a class carrying an implementation
count already coexist. The gate reads as a guard against a second
resolution pass rather than a deliberate layout choice, and the counts
are already answered from the reference index, so keeping both is not
the work the gate seems to be avoiding.

**Where to look:** `src/code_lens.rs` (`handle_code_lens`, the
`proto.is_none()` condition on `build_member_reference_lens`).
