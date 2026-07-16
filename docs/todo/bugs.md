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

Laravel-specific items from the same sweep are in
`docs/todo/laravel.md` (L21 alias parsing); ~50 further errors
were reclassified as intended
diagnostics per the declared-types philosophy there. The closure
literal-return shape gap is filed as T31 in
`docs/todo/type-inference.md`.

## B67. Positional array-shape indexing does not resolve the element type

**Severity: Medium-High (~20 errors, pdepend) · Confirmed with fixture**

```php
/** @var array{Label, Stmt} $pair */
$pair = $n->getChildren();
$pair[0]->getImage();   // "type of '$pair[]' could not be resolved"
```

Both single-line and multiline `@var array{...}` shapes fail
(pdepend `tests/.../PHP81/MatchExpressionTest.php` and several
other parser feature tests: `$pair[]`, `$children[]`,
`$elements[]`). This is the same symptom as the previously fixed
B58 — either the fix regressed or it never covered the
`@var`-annotation path; the old fix's tests should be extended.

## B68. Foreach over an Iterator subclass ignores the inherited generic value type

**Severity: Medium (~5 errors, pdepend) · Confirmed from output**

```php
/** @extends FilterIterator<int, SplFileInfo, \Iterator<int, SplFileInfo>> */
class Iterator extends FilterIterator { ... }

foreach ($fileIterator as $file) {
    $file->getRealPath();  // "Method 'getRealPath' not found on class 'PDepend\Input\Iterator'"
}
```

Iterating an object that implements `Iterator`/`IteratorAggregate`
should use the value type from the class's inherited generic
iterator parameters (or the `current()` return type as fallback).
Instead the element is typed as the iterator class itself, or not
at all. Also fails for direct SPL iteration
(`foreach (new DirectoryIterator(...) as $file)`, pdepend
`tests/php/PDepend/ParserRegressionTest.php:80`).

Note: the ~12 luxplus-backoffice paginator errors
(`foreach (ProductGroup::paginate(25) as $productGroup)`) initially
filed here were *not* this bug — they were a framework docblock gap
(`Builder::paginate()` declared an unparameterized
`LengthAwarePaginator`), now corrected so the paginators resolve
their element type through `IteratorAggregate`. This bug is only
the SPL / direct-iteration case above.

## B69. Indexing a call result inline breaks the rest of the chain

**Severity: Medium-High (~16 errors: pdepend ~9, luxplus-backoffice 7) · Confirmed with fixture**

```php
$a->findChildrenOfType(ASTAttribute::class)[0]->getParent();
// "type of '$a->findChildrenOfType(ASTAttribute::class)[]' could not be resolved"

Country::cases()[0]->value;   // same failure on enum cases()
```

Splitting into two statements (`$children = $a->findChildrenOfType(...);
$children[0]->getParent();`) works, so the array element type is
available — only the inline `call(...)[index]->member` chain form
fails in subject extraction/resolution.
