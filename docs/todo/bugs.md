# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

## B1. `resolve_function_name` guesses a single namespace, missing same-file multi-namespace declarations

`resolve_function_name` (`src/resolution.rs`) builds its candidate
list from a single `file_namespace` — the *call site's* namespace
block (`FileContext::namespace` / `namespace_at_offset`). In a file
that declares multiple `namespace` blocks, a function declared under
a namespace other than the call site's does not match any candidate,
even though it is already sitting in `global_functions` under its own
FQN.

`FileContext::resolve_name_at` (`src/types/mod.rs`) already solves
this correctly for other identifiers by consulting `resolved_names`
(mago-names' per-offset resolution, which understands multiple
namespace blocks in one file). `resolve_function_name` should try
`ctx.resolve_name_at(name, offset)` as a candidate before falling
back to the single-namespace guess.

This is why `src/completion/source/helpers.rs::
extract_function_return_from_source` still exists: it is a same-file
backward-text scanner for a function's `@return` docblock, used as a
fallback in `rhs_resolution.rs` (guarded by `!loader_found`) exactly
when `resolve_function_name` fails on this multi-namespace case. Once
`resolve_function_name` is offset-aware, this text scanner becomes
dead code and should be deleted along with its call site.

**Scope note:** `function_loader`/`function_loader_with`
(`src/resolution.rs`) are `Fn(&str) -> Option<FunctionInfo>` closures
bound once per `FileContext`, with no offset parameter, across ~30
call sites (completion, hover, definition, references, code actions).
Fixing this requires either threading the call expression's byte
offset through to `resolve_function_name` or adding an offset-aware
variant of the loader closure. Size the fix accordingly — this is not
a one-line change.
