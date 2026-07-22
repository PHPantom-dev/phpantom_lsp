# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

## Background index publish can clobber files opened and edited mid-index

The full background index snapshots the set of already-parsed URIs
when it starts, parses everything else (Phase 2 reads straight from
disk), and publishes all results in one batch merge at the end. A file
that is opened *after* the snapshot and edited *during* the parse
window gets its fresh `did_change` state (symbol map, classes,
resolved names) overwritten by the stale disk parse when the batch
publishes. Hover, diagnostics, and references for that file are then
computed from pre-edit content until the next keystroke or save
re-parses it. The window is the full index duration (tens of seconds
on large projects), and the same race exists for any
`ensure_workspace_indexed` run triggered by find-references. Fix: at
batch-publish time, skip updates whose URI is currently in
`open_files` (the open buffer's `update_ast` already produced newer
state), or re-check open-file content per update before applying.

## Go-to-implementation results for vendor classes are session-dependent once the workspace index is ready

`find_implementors` returns early after the reverse-inheritance-index
phase once the workspace index is ready, intentionally skipping the
vendor/classmap/stub scan phases (the tests assert vendor
implementors are not returned). But the reverse inheritance index is
populated by *every* `update_ast`, including vendor files parsed
lazily during class resolution. So a vendor implementor that happens
to have been loaded earlier in the session (because the user hovered
or completed something that resolved it) **does** appear in
go-to-implementation and type hierarchy results, while an identical
vendor implementor that was never loaded does not. Results depend on
session history. Pick one behaviour and enforce it: either filter
vendor FQNs out of the index-ready fast path (deterministic user-only
results, matching the tests' intent), or keep a bounded vendor
fallback so vendor implementors are always included. If user-only is
chosen, consider whether stub implementors (e.g. SPL classes
implementing `Iterator`) need the same treatment.
