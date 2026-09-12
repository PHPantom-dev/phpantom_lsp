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

### B52. `SubjectText::as_str` slices a span against the wrong content

**Impact: High · Complexity: Medium**

`SubjectText::as_str` (`src/symbol_map/mod.rs:217`) indexes `content`
with the byte span recorded when the symbol map was built. When the two
disagree the slice panics and takes the worker thread with it:

```
thread 'fix-worker' panicked at src/symbol_map/mod.rs:217:51:
end byte index 8602 is out of bounds for string of length 8600
start byte index 1058 is out of bounds for string of length 781
```

Reproduce with `phpantom_lsp fix --dry-run --rule unused_import
--project-root <a large Laravel project>`: several workers die and the
run finishes reporting "No fixable issues found" for the files they
were holding, so a crash is indistinguishable from a clean result.

The spans and the string come from different revisions of the same
file. The symbol map is cached per URI while the content is passed in
by the caller, and at least one path hands in content the cached map
was not built from (a stale map that survived an `update_ast`, or a map
built from the Blade virtual PHP being sliced against the raw template).
The fix is to make the pairing explicit rather than to bounds-check the
slice: a symbol map should carry the content revision it was extracted
from so a consumer cannot pair it with a different one.

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

No outstanding items.
