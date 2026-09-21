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

### B323. An external formatter that prints nothing empties the document

**Impact: High · Complexity: Low**

The stdin-driven formatters (Pint, php-cs-fixer) return the child's
stdout verbatim from `run_tool` in `formatting/external.rs`, with no
check that anything came back. `format_content` then sees a formatted
text that differs from the original, and `compute_edits` turns that into
a single `TextEdit` covering the whole document whose replacement is the
empty string. Formatting the file deletes it.

Two paths produce an empty stdout while the exit code stays `0`, so
nothing upstream reports a failure:

- `process.rs` drains the child with `let _ = s.read_to_string(&mut buf)`.
  A single non-UTF-8 byte anywhere in the output leaves `buf` empty and
  throws the error away.
- The same function collects the drain threads with
  `.and_then(|h| h.join().ok()).unwrap_or_default()`, so a panicking
  reader thread also yields `""`.

The configured Pint command is a free-form string, and a wrapper that
formats in place and prints nothing is a common way to write one. That
is enough on its own: exit `0`, no stdout, document wiped.

The sibling-file arm is already defensive about partial results; the
stdin arm needs the same care. Reject an empty result for a non-empty
input, and make the drain threads propagate a read failure as an error
instead of an empty string. `run_pint_on_blade` has the same hole.

### B324. "Extract variable (all occurrences)" can emit overlapping edits

**Impact: Medium · Complexity: Low**

`find_identical_occurrences` (`code_actions/helpers.rs`) resumes its scan
one byte past the *start* of the match it just accepted rather than past
its end, so a selection that can overlap itself is found twice:

```php
<?php
$x = $a . $a . $a;
```

Selecting `$a . $a` and taking "Extract variable (all occurrences)"
matches at the first `$a` and again at the second, and both pass the
word-boundary checks. The workspace edit then carries two overlapping
`TextEdit`s for one document, which is undefined in LSP: an editor either
rejects the whole edit or corrupts the line. "Extract constant (all
occurrences)" shares the helper and the same shape.

The one-byte step is also why the scan can index a `&str` at a
non-boundary: when the selection begins with a multibyte character
(`Ünit::TAX` is a legal PHP name), the resumed slice starts inside it.
That path panics into the code-action resolve guard, so the action
silently does nothing.

Advance by the match length instead, and cover both shapes with tests.
