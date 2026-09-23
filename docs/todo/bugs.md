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

## B326. A dead branch in another file hides diagnostics in the file being checked

**Impact: Medium · Complexity: Low**

A diagnostic pass collects the byte ranges of branches a decidable guard
rules out (`if (false)`, a negated `method_exists` on a literal
class and method), and drops every diagnostic of the file that starts
inside one (`diagnostics/mod.rs:368-387`). `record_unreachable_range`
(`type_engine/variable/forward_walk/reachability.rs:46`) records
whenever a collection is active. It does not check which file the
walker is in, and neither `suspend_diagnostic_scope` nor
`suspend_snapshot_recording` stops it, even though
`record_scope_snapshot` skips nested walks for exactly this reason
("their statement offsets can even come from a different file").
Return-type inference of an untyped method in another file walks that
file's body through the same `process_if` (`control_flow.rs:113-128`,
`318-331`). The dead ranges it records are offsets into the other file,
and they suppress whatever the checked file reports at the same byte
offsets.

**Reproducer shape:** file A calls `(new Helper)->make()->bogus()`, and
`Helper::make()` in file B has no return type and contains
`if (false) { … }` spanning bytes X..Y. Any diagnostic in A that starts
in X..Y disappears.

**Fix:** tie the recorded ranges to the walk that owns the pass, either
by keying them on the pass's content (as the snapshot map is keyed) or
by not recording while snapshot recording is suspended, after checking
that every cross-file walk runs suspended.

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

## B328. Closing a file drops its references from the reference-count lenses

**Impact: Medium · Complexity: Medium**

`on_did_close` (`backend/documents.rs:219`) calls `clear_file_maps`,
which removes the file's `symbol_maps` entry and its reference-index
entries (`backend/file_access.rs:374-381`). Once the initial workspace
index has completed, `ensure_workspace_index_ready_for_request`
(`indexing/preload.rs:212`) returns early and never re-parses the file.
The lens batch (`user_file_symbol_maps_for_reference_keys`) and
`indexed_reference_count` therefore stop seeing its references, and the
eviction marks the counts stale, so they are recomputed without them.
Open `B.php`, which calls `Foo::bar()`, close it, and the lens on
`Foo::bar` drops by one. Only an explicit Find References, which
refreshes the index, brings it back. No integration test closes a file.

**Fix:** on close, re-read a workspace file from disk and re-index it,
as `on_did_close` already does for resource documents
(`documents.rs:236-240`), instead of evicting it.

## B329. The reference-count worker keeps searching while the user types

**Impact: Low-Medium · Complexity: Low**

`schedule_member_ref_counts` (`reference_counts/mod.rs:333`) waits for an
edit pause once, then loops `compute_pending_member_ref_counts` on a
blocking thread until the queue is empty. An item stays queued whenever
the invalidation epoch moved during the run (lines 296 to 305), and the
epoch moves on every reparse of any file: `invalidate_member`
(`reference_counts/cache.rs:331`) bumps it before its early return, and
`invalidate_locations_in` bumps it whether or not an entry matched. While
the user keeps typing, whole-workspace searches therefore run back to
back, each invalidated by the next keystroke.

**Fix:** only bump the epoch when an entry was actually marked, and wait
for an edit pause between iterations rather than only before the first.

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
