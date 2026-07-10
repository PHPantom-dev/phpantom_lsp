# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

## B43. Alternate `if:`/`endif;` syntax skips inverse condition narrowing in branch merge

**Severity: Low.** When resolving types after an `if` statement written
in the alternate colon syntax (`if (…): … elseif (…): … else: … endif;`),
the branch-merge path walks each `elseif` and `else` branch without
applying inverse narrowing from the preceding conditions. The brace
syntax applies inverse narrowing from the `if` condition (and every
preceding `elseif` condition) to each later branch before walking it;
the colon syntax does not. As a result, type information carried by a
failed condition (e.g. `$x === null` implying `$x` is not null in the
`else` branch) is lost in the alternate syntax, which can produce both
false positives and false negatives in `elseif`/`else` bodies and in the
scope merged after the block. The fix is to mirror the inverse-narrowing
calls from the brace-syntax merge path in the colon-syntax merge path.
