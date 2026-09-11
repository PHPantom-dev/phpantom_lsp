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

### B50. Namespaced class completions vanish from Blade templates

**Impact: Medium-High · Complexity: Medium**

In a `.blade.php` file, class-name completion drops every candidate that
would need a `use` import. Typing `new Widg` or `Widg` in an echo offers a
global `Widgetry` but not `App\Models\Widget`; the same request in a PHP
file offers both, the latter with its import edit.

The item is built against the virtual PHP the template lowers to, so
`build_use_edit` (`src/completion/use_edit.rs`) places the import where a
PHP file would take it: the line after `<?php`, which in the virtual file
is the preprocessor's prologue. `translate_completion_item`
(`src/blade/translate.rs`) then finds that the additional edit has no
template position behind it and drops the whole item rather than a
misplaced edit, so the candidate never reaches the editor.

The fix is a template-aware import edit, not a translation tweak: a
template imports a name with `@use('App\Models\Widget')` at its top (or
a `use` inside an existing `@php` block), written in Blade coordinates
from the raw template text, the way `src/blade/use_directive.rs` already
reads existing `@use` directives. It has to live in the shared use-edit
builder so completion and the "Import class" code action (BL1) both get
it; the code action would otherwise offer an import whose edit is dropped
the same way.
