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

### B321. Echo-delimiter hover fires on `{{` that is not an echo

**Impact: Low · Complexity: Low**

`blade_echo_delimiter_col` decides whether the cursor is on an echo
delimiter from the characters around it and nothing else, so any `{{`
or `}}` on the line answers. Hovering one inside a `@verbatim` block,
inside a `{{-- --}}` comment, or inside an `@`-escaped `@{{ … }}`
reports "Blade escaped echo. Output is passed through `e()`" and
go-to-definition jumps to `e()`, none of which the template compiles
to: all three spans are literal output.

The preprocessor already knows which of them is which, and
`directive_completion::is_html_position` is a second scanner that
answers a neighbouring question without modelling the escape either.
Both callers want one answer to "what does the compiler make of this
offset?", which should come from a single place rather than from a
third round of character peeking.

**Where to look:** `src/blade/mod.rs` (`blade_echo_delimiter_col`,
`blade_echo_delimiter_hover`, `blade_echo_delimiter_definition`),
`src/blade/directive_completion.rs` (`is_html_position`).
