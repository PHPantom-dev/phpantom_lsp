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

### B49. A bracket inside a PHP comment counts as a bracket in a Blade template

**Impact: Low · Complexity: Medium**

Neither the Blade reindenter's bracket scan
(`open_bracket`/`close_bracket` in `src/formatting/blade/reindent.rs`)
nor `matching_paren` in `src/blade/signature.rs` skips a PHP comment, so
a `)` or `]` written inside one closes a construct that is still open:

```blade
@props([
    'a' => 1, // trailing )
])
```

The reindenter reads the comment's `)` as the one closing `@props(` and
writes the real closing line one level too deep (`    ])`). The
embedded-PHP pass reads the same comment the same way, so the argument
range it hands the formatter stops short of the `]`, the snippet does not
parse, and the fragment is left as written instead of being formatted.

Both scans need the same PHP comment handling they already have for
string literals: `//` and `#` to the end of the line, and `/* … */` to
its terminator. `matching_paren` is shared with the signature parser, so
the fix has to hold for a `@bladestan-signature` argument list too.
