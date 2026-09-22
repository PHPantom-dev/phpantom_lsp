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

### B325. A member search misses the implementors nothing has parsed yet

**Impact: Medium · Complexity: Medium**

A member reference search scopes itself to the hierarchy that declares
the member, and `collect_descendants_for_roots` reads that hierarchy's
descendants out of `gti_index` alone. That index only holds classes from
files that have been fully parsed, and a package class is parsed the
first time something needs it, so an implementor sitting in a dependency
is missing from the scope until an unrelated lookup happens to pull its
file in. Every access on it is then judged to be on an unrelated class
and dropped from the results.

`type_hierarchy.rs` documents the same limitation for its own use of the
index and works around it by calling `find_implementors`, whose second
phase scans for the classes the index does not know about yet. The member
search has no such phase.

The result is that the same search answers differently depending on what
the session has done before it. On a Laravel application, Find References
on a method declared by a package interface and implemented both by the
application and by a package class misses the package implementor on the
first run and finds it on the second: the first run resolves a
`$service = app()->make(PackageService::class)` receiver while scanning,
which parses `PackageService` and files it under the interface, and the
second run's hierarchy is built from the index that first run filled.
Anything that loads the workspace's classes first (the startup receiver
warm-up, a completion, a hover) has the same effect, which is what makes
this look intermittent.

**Where to look:** `collect_descendants_for_roots` and
`collect_member_receiver_scope` in `src/references/members.rs`,
`find_implementors` and its scan phase, and the `gti_index` note in
`src/type_hierarchy.rs`.

## Array types

No outstanding items.

## Docblock handling

No outstanding items.

## Miscellaneous

No outstanding items.
