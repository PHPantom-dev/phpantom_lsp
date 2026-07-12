# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

All entries below come from the 2026-07 analyze triage sweep over
the sample projects (see `projects/analyze-triage.md`). Except
where noted, each was reproduced in isolation with a minimal
fixture against a debug build. Counts are the number of analyze
errors the bug accounts for across the sample projects and are
approximate — fixing an upstream bug often clears cascading
errors attributed to other buckets.

## B61. Indexed access with `??` on a heterogeneous array element widens to `string`

**Severity: Low (~2 errors in pdepend tests) · Reproduced**

```php
$items = [['int', '$id'], ['array', '$list', ArrayType::class]];
foreach ($items as $expected) {
    $expectedTypeClass = $expected[2] ?? ScalarType::class;
    assertInstanceOf($expectedTypeClass, null); // "expects class-string<object>, got string"
}
```

The foreach element `$expected` from a heterogeneous array literal
is not inferred as a union of positional shapes, so `$expected[2]`
widens to `string` instead of the `class-string` it actually holds.
The `?? ScalarType::class` fallback (itself a `class-string`) is
then lost in the union and the value is passed to a
`class-string<T>` parameter as a plain `string`
(pdepend `tests/.../PHPParserVersion81Test.php:1187`, `:1476`).
Related to positional-shape indexing, but the trigger here is the
foreach element type plus the null-coalesce.

**Fix:** infer positional array-shape unions for foreach elements
of heterogeneous array literals so int-literal indexing and `??`
preserve the element's `class-string` type.
