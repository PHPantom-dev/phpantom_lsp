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

## B58. Indexing a positional array shape does not resolve the element type

**Severity: Low-Medium (~10 errors in pdepend tests) · Confirmed from output**

```php
/** @var array{ASTSwitchLabel, ASTThrowStatement} $pair */
$pair = $entries[2]->getChildren();
$pair[0]->getImage();   // "type of '$pair[]' could not be resolved"
```

Keyed shapes (`array{a: Foo}`) resolve via string keys, but
positional tuple shapes indexed with int literals (`$pair[0]`,
`$pair[1]`) do not
(pdepend `tests/.../PHP81/MatchExpressionTest.php:144`).

**Fix:** map int-literal index access onto positional shape
entries in the array-access resolution path.

## B59. Project class sharing a global interface name breaks subtype checks

**Severity: Low (5 errors, pdepend-specific) · Not reproduced in isolation — needs investigation**

pdepend defines `PDepend\Input\Iterator`. In `src/Engine.php:736`
passing a `RecursiveIteratorIterator` to a param typed
`Iterator<int, SplFileInfo>` (and a `RecursiveDirectoryIterator`
to `Traversable`) reports `type_mismatch_argument`, even though
both implement the global interfaces. The same calls pass in an
isolated fixture with full stubs, so the suspected trigger is the
project-local `Iterator` class shadowing the global `\Iterator`
during the subtype walk in that file's namespace context.

**Fix:** investigate name resolution inside
`is_subtype_of`/hierarchy walking when a project class collides
with a global stub interface; hierarchy names originating from
stubs must resolve in the global namespace, not the consuming
file's.

## B60. Template binding from closure return types through facade `@method` tags

**Severity: Medium-High (suspected driver of many Luxplus unresolved errors) · Root cause unconfirmed**

`$linkCampaign = Cache::remember($key, 3600, fn() => LinkCampaignRepository::getByCampaignId(...));`
leaves `$linkCampaign` unresolved
(luxplus-website `app/Features/Products/Services/Products/DiscountService.php:42`).
`Cache::remember` is `@method static TCacheValue
remember(string $key, ..., Closure(): TCacheValue $callback)` —
binding `TCacheValue` from the closure's return type at the call
site does not happen through the facade's virtual `@method` path.
Closure-return template binding works in some paths (generator
closures, per the changelog), so scope this to which call shapes
miss it (facade static + virtual method at minimum) and fix the
shared binding path.

**Fix:** confirm with a minimal facade fixture, then bind
method-level templates from closure literal return types in the
same place existing `@method` template inference runs.

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
Related to positional-shape indexing (see B58) but the trigger here
is the foreach element type plus the null-coalesce.

**Fix:** infer positional array-shape unions for foreach elements
of heterogeneous array literals so int-literal indexing and `??`
preserve the element's `class-string` type.
