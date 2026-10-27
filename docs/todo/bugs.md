# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

Each entry below carries an **Impact · Complexity** rating using the same
scale defined in [`docs/todo.md`](../todo.md), but a bug's row lives
**here only** — do not add or link a bug entry to `docs/todo.md`'s sprint
or backlog tables. This file is its own list, not a domain document
sprint items draw from: whenever it holds anything, that is actively
addressed, independently of sprint planning.

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

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Name resolution

### B686. Another Composer project's own sources resolve in every project of the workspace

**Impact: Low · Complexity: Medium**

A file under `mobile/` can name a class from `platform/src/` and gets
completion, hover and no `unknown_class`, although `mobile/`'s
autoloader cannot load it. A file now only sees its own project's
`vendor/` (see `ProjectScope` in `src/resolution/project_scope.rs`), but
the PSR-4 sources of every project stay visible to all of them, since
`psr4_mappings` is one workspace-wide list.

Hiding them needs care: a path repository symlinked into a project's
`vendor/` is that project's dependency even though its files live
under another project's tree, and the workspace's loose files must stay
visible to everyone.

**Where to look:** `ProjectScope::sees`, and Phase 2 of
`find_or_load_class_inner` in `src/resolution/classes.rs`, which
resolves through the shared `psr4_mappings`.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
