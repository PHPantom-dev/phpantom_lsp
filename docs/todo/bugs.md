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

### B50. Removing the last member of a group import leaves the group behind

**Impact: Medium-High · Complexity: Low**

When every member of a group import is unused, `phpantom_lsp fix` turns
a valid file into one that no longer parses:

```php
use App\Models\{User, Post};   // both unused
```

becomes

```php
use App\Models\{
```

`extend_range_for_group_member`
(`src/code_actions/remove_unused_import.rs:368`) always returns an edit
scoped to the one member and its separating comma. Nothing checks
whether that member was the last one, so the `use App\Models\{` prefix
is never removed the way a single-import statement is. The function is
shared with the editor quick fix, so the same gap is reachable from
`remove_unused_import` and not only from the CLI.

The reason this went unnoticed is that the test covering it has never
run: `tests/integration/fix_cli.rs` landed without a matching
`mod fix_cli;` in `tests/integration/main.rs`, so none of its 18 tests
are compiled. Wiring the module in is the first half of the fix;
`removes_entire_group_import_when_all_unused` then fails on the output
above, and the other 17 pass.
