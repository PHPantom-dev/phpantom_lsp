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

### B1. The Pint formatting proxy inherits the server's working directory

**Impact: Medium · Complexity: Low**

`run_pint` in `src/formatting/external.rs` spawns
`pint --stdin-filename=<path>` without `current_dir`, so the child
inherits whatever directory the language server was started in. Pint
resolves `pint.json` from `getcwd()` (`Project::path()`), not by walking
up from the stdin filename, so in an editor that does not start the
server in the workspace root (Neovim launched from a subdirectory, a
multi-root workspace) the project's preset and rules are silently
ignored and the `laravel` preset is applied instead. The PHPStan and
PHPCS proxies already pass `current_dir(workspace_root)`; Pint should
too. This also gates the Blade path in
[BL16](blade.md#bl16-blade-aware-formatting): with the
`Pint/laravel_blade` rule enabled Pint resolves the project's
`node_modules` from the same directory.

Fix: pass the workspace root as the child's working directory in
`run_pint`, and add a test that a `pint.json` at the workspace root is
honoured when the server's cwd is elsewhere.
