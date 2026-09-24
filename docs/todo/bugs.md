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

## Laravel

## B359. Go-to-definition for a config/trans key misses an escaped declaration

**Impact: Low · Complexity: Low**

`walk_elements` (`src/virtual_members/laravel/array_file.rs`) reads an array
key's text with `extract_string_literal`, which slices the literal's raw
source rather than resolving its unescaped `value`. A config key declared as
`'it\'s' => …` is therefore recorded as `it\'s`, not `it's`. Every consumer
of `for_each_entry`/`walk_elements` compares that raw text against a key
built from the literal's actual value elsewhere in the pipeline (config
reads go through `push_laravel_string_span`, which now resolves `value`; the
config-value tree in `config_values.rs` already does the same), so a
declaration spelled with an escape sequence never matches: go-to-definition
(`collect_laravel_config_declarations` in `config_keys.rs`,
`resolve_config_key_definition_fallback`) and find-all-references
(`find_config_references`, `find_all_config_references`) all miss it, and the
same applies to the trans-key equivalent in `trans_keys.rs`. The fix is the
same shape as the one applied to `push_laravel_string_span`: read the
literal's `value` for the key text, keep the span on the source text.

## Blade

No outstanding items.

## Miscellaneous

No outstanding items.
