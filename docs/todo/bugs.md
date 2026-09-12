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

### B51. Multi-line group imports are never flagged as unused

**Impact: Medium · Complexity: Medium**

`find_use_statement_range` (`src/diagnostics/unused_imports.rs:415`)
only recognizes a `use` statement when the same source line contains
both `use ` and `;`:

```php
use App\Models\{
    User,
    Post,
};
```

The opening line (`use App\Models\{`) has no `;`, so this check never
matches and no unused-import diagnostic is ever produced for any member
of a multi-line group, even when every member is unused. The same
line-bound assumption also appears in `is_group_import_match` and
`find_group_member_range` in the same file, which likewise expect the
whole group to fit on one line. Single-line group imports
(`use App\Models\{User, Post};`) are unaffected.
