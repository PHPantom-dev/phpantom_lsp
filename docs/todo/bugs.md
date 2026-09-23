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

## B333. A stale index parse can pin a template to its old scope

**Impact: Low · Complexity: Medium**

A workspace index worker lowers a closed template with the scope cached
in `blade_injected_vars` when it starts, and its batch is applied later
(`parse_ast_index_update_for_index` →
`apply_ast_index_parse_results_batch`). If call-site inference re-infers
the template in between (the detached did-open or did-save pass), writes
the new scope to the cache and re-parses, the worker's batch still lands
afterwards and republishes the lowering built from the old scope. The
published lowering is self-consistent (its virtual PHP, source map and
symbol map are published together), so positions are right, but the
template's variables keep the old types. The next re-inference computes
the same scope that is already cached, so it sees nothing to do and the
stale types stay until the template itself is re-parsed.

**Fix:** record the scope a lowering was built from in the lowering, and
drop a batch result for a template whose cached scope has moved on since
(or re-lower it against the current scope when the batch applies).
