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

## B66. Type-guard narrowing lost on compound conditions and non-variable subjects

**Severity: High (~39 errors: bladestan 28, luxplus-website 6, pdepend 5) · Confirmed with fixture**

`instanceof` (and `assert*`) narrowing only survives the simplest
shape: a single negated variable/property guard with early return.
It is lost when:

1. The check is one conjunct of an `&&` chain and the narrowed
   expression is used in later conjuncts or the body:
   `$node->expr instanceof Assign && $node->expr->var instanceof Variable && ...`
2. The guard is an `||` of a variable check and a property check:
   `if (! $arg instanceof Arg || ! $arg->value instanceof String_) return;`
3. The subject contains an array index anywhere: `$stmts[0]`,
   `$args[0]->value`, `$constants['C']` — indexed subjects are
   never narrowed, even in the simple negated-guard form
   (bladestan `src/PhpParser/ArrayStringToArrayConverter.php:53`,
   pdepend `tests/.../PHP82/FetchPropertiesOfEnumsInConstExpressionsTest.php:79`).
4. The subject is an inline assignment:
   `if (($node = $this->getReturnType()) instanceof ASTClassOrInterfaceReference) { $node->getType(); }`
   (pdepend `src/Source/AST/AbstractASTCallable.php:255`).
5. The narrowing comes from `assertInstanceOf(X::class, $view->component)`
   on a property or indexed subject — later accesses to the same
   expression are not narrowed (luxplus-website
   `tests/Feature/Tracking/Google/*EventTest.php`, 6 errors).

Everything downstream (foreach element types, `end()` results,
reassigned variables) cascades into `unresolved_member_access` /
`scalar_member_access`. Minimal repro (all three fail, while the
single negated variable guard works):

```php
return $arg->value instanceof StringExpr && $arg->value->value === 'x';

if (! $stmts[0] instanceof StringExpr) { return; }
takeString($stmts[0]->value);

if (! $arg instanceof Arg || ! $arg->value instanceof StringExpr) { return; }
takeString($arg->value->value);
```

This is the concrete false-positive backlog behind the T20
narrowing reconciliation engine (`docs/todo/type-inference.md`);
fixing it structurally is preferred over adding one-off cases.

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

## B70. Call-expression arguments are not resolved for template binding and callback parameter inference

**Severity: Medium-High (~12 errors: phpmd 2, pdepend ~5, luxplus-website 3, agcms 1, + masked variants) · Confirmed with fixture**

Two facets of the same call-site gap:

1. `@template T` binding from `array<T>` fails when the argument
   is a call expression (works for variables and array literals):

   ```php
   /** @template T  @param array<T> $a  @return T */
   function first(array $a): mixed { ... }

   $emails = first(self::getEmailConfigs());  // getEmailConfigs(): array<string, EmailConfig>
   $emails->address;   // "type of '$emails' could not be resolved"
   ```

2. Callback parameter types are not inferred when the array
   argument of `array_map`/`array_filter` is a call or property
   expression (works when it is a variable):

   ```php
   array_map(static fn($node) => $node->getImage(), $new->getChildren());
   //                            ^ "type of '$node' could not be resolved"
   ```

The root is the same: `build_function_template_subs`' generic
wrapper arm only resolves `$variable` arguments and array literals
(see T25 in `docs/todo/type-inference.md`, where the array-literal
case was added) — route argument resolution through the shared
`resolve_rhs_expression` pipeline instead of special-casing
argument syntax shapes.

Related scope defect confirmed by the same fixtures: when the
callback parameter shares its name with an outer variable, the
parameter silently borrows the *outer* variable's type instead of
failing (masking the gap and producing wrong types). Closure
parameters must shadow outer variables unconditionally.

A third facet of the same call-site gap: `app()->make($repository)`
where `$repository` is a foreach element of a literal
`[Foo::class, Bar::class, ...]` array — the declared
`class-string<T>` union never binds `make()`'s template, so the
chained call is unresolved (2 errors, luxplus-backoffice
`app/Jobs/SalesInfo/UpdateSalesInfoLocalJob.php:37`). All facts are
declared; only the argument-shape special-casing is in the way.

## B71. Mockery mock intersection types lost in collections and arguments

**Severity: Medium (~10 errors, luxplus-backoffice) · Confirmed from output**

`Mockery::mock(X::class)` resolves to the intersection with `X` in
simple assignments (B64 fixed that), but the `X` half is lost when
mocks flow through arrays or into typed parameters:
"Argument 1 ($failed) expects array<IFileValidationRule>, got
list<Mockery\MockInterface>"
(luxplus-backoffice `tests/Feature/Brands/BrandPromotionsControllerTest.php:347,385`,
`tests/Feature/Jobs/BusinessCentral/UpdateExpiredMemberJobTest.php:25`),
and chained expectation calls report "Method 'with' not found on
class 'Mockery\LegacyMockInterface'"
(`tests/Feature/Storage/*`, `$storageMock` / `$storageResultMock`
cascades). Fix the intersection propagation, not the diagnostic.

## B72. String-literal class names keep their source escape sequences

**Severity: Medium (~9 errors, pdepend) · Confirmed with fixture**

```php
$expr = $n->getFirstChildOfType('Fixture9\\ASTExpression');
// "subject type 'Fixture9\\ASTExpression' could not be resolved"
```

A single-quoted `'Foo\\Bar'` means `Foo\Bar` at runtime, but the
raw source text (with the doubled backslash) is used as the
class-string value, so the class lookup fails. Unescape string
literals before using them as type/class names — this affects
every `class-string` parameter fed by a string literal.

## B73. `@template T of <array type>` identity generics are not bound

**Severity: Medium (~9 errors, pdepend) · Confirmed from source**

```php
/**
 * @template T of Token[]
 * @param T $tokens
 * @return T
 */
private function stripTrailingComments(array $tokens): array { ... }
```

A template whose *constraint* is an array type (`Token[]`,
`list<ASTNode>`) used as a pass-through (`@param T` / `@return T`)
never binds, so the return value is unresolved
(pdepend `src/Source/Language/PHP/AbstractPHPParser.php`
`reduceUnaryExpression` / `stripTrailingComments` call sites:
`$expressions[]`, `end($tokens)->type`).

## B74. Nested `stdClass` property chains are unresolved

**Severity: Medium (~8 errors, pdepend) · Confirmed from output**

```php
$settings = new stdClass();
$settings->cache = new stdClass();
$settings->cache->ttl = $config['cache']['ttl'];
// "Cannot verify property 'ttl' — type of '$settings->cache' could not be resolved"
```

T24 (`docs/todo/type-inference.md`) suppressed member checks on
variables typed `stdClass`, but a *property of* a `stdClass` is not
covered: the subject `$settings->cache` fails to resolve even
though it was assigned `new stdClass()` two lines up
(pdepend `src/DependencyInjection/PdependExtension.php:127-135`).
Track property assignments on `stdClass` values (PHPStan models
this exactly), or at minimum resolve `stdClass` property reads to
`stdClass`-typed values assigned in the same scope.

## B75. `[Foo::class, 'name']` array literals are validated as callables

**Severity: Medium (~5 errors, pdepend) · Confirmed from source**

A two-element array literal whose first element is a class constant
or object and whose second is a string literal is checked as a
static/instance callable even when it is plain data:

```php
return [
    [Chart::class, 'svg'],   // "Method 'svg' not found on class '...Chart'"
    [$namespacePrefix, 'match'],  // "Cannot access method 'match' on type 'string'"
];
```

(pdepend `tests/php/PDepend/Bugs/PHPDependBug13405179Test.php:97-100`,
`tests/.../PHP81/MatchExpressionTest.php:81`.) Only treat such an
array as a callable when it flows into a callable-typed context
(callable parameter, `is_callable`, invocation).

## B76. Same-namespace class loses to a global stub class of the same name

**Severity: Medium (~5 errors, pdepend) · Confirmed from output**

In `namespace PDepend\Input`, `new Iterator(...)` must resolve to
`PDepend\Input\Iterator`, but PHPantom resolves it to the global
SPL `Iterator` interface, so every member on the instance is
unknown ("Method 'accept' not found on class 'Iterator'",
`tests/php/PDepend/Input/IteratorTest.php:127,144,161`, plus the
foreach cascades at lines 102 and 179). The subtype-check variant
of this was fixed previously ("a project class sharing a global
interface's short name"); the `new X()` resolution path still
prefers the stub.

## B77. Conditional types are not evaluated at call sites in parameter and return position

**Severity: Low-Medium (~3 errors, luxplus-backoffice) · Confirmed from output**

1. Parameter position: Laravel Collection `groupBy`/`keyBy`
   parameters typed with PHPStan conditional types are compared
   unevaluated, producing messages that print the raw conditional
   ("Argument 1 ($key) expects $groupBy is array|string ? array-key
   : TGroupKey is UnitEnum ? array-key : ..., got string" —
   `app/Domain/ProductCacheService/Services/ProductCacheRedisService.php:56`,
   `app/Http/Controllers/Products/Routines/ProductRoutineTemplatesController.php:252`).
   The conditional must be resolved against the bound template
   arguments before the subtype check (and never printed raw).
2. Return position with non-variable arguments:
   `Str::replace(...)` has `@return ($subject is string ? string : string[])`;
   passing a method-call chain as `$subject` picks the array branch
   ("expects string, got array<string>" —
   `app/Services/Feeds/ProductFeedService.php:1047`).

## B78. `@mixin` with a template parameter resolves no members

**Severity: Low-Medium (2 direct errors, phpmd; unblocks the phpmd/pdepend wrapper hierarchy) · Confirmed with fixture**

```php
/**
 * @template-covariant TNode of Engine
 * @mixin TNode
 */
abstract class Wrapper { }

$wrapper->getLabel();  // "Method 'getLabel' not found on class 'Wrapper'"
```

When the `@mixin` target is a template parameter, members should
resolve through the template's upper bound (`Engine` here), and
through the concrete binding where one exists
(phpmd `src/Rule/Controversial/CamelCaseParameterName.php:47,51`
via `AbstractNode`'s `@mixin TNode`).

## B79. `?object` return type loses the `object` type

**Severity: Low-Medium (2 errors, luxplus-site-manager) · Confirmed with fixture**

```php
public function all():? object { ... }

$r->all()->projects ?? [];
// "Cannot verify property 'projects' — type of '$r->all()' could not be resolved"
```

A plain `object` return works (property access is not flagged);
adding nullability makes the whole type unresolvable. The
null-stripping step discards `object` instead of keeping it
(luxplus-site-manager `app/Services/EnvoyerService.php:19,47`).

## B80. `@phpstan-type` / `@phpstan-import-type` aliases treated as class names

**Severity: Low-Medium (2 errors, luxplus-backoffice) · Confirmed from source**

A vendor method typed `@param CountParams $query` (where
`CountParams` is declared via `@phpstan-type` on another class and
pulled in with `@phpstan-import-type`) is compared as if
`CountParams` were a class, producing "expects
rajmundtoth0\AuditDriver\Services\CountParams, got array{...}"
(luxplus-backoffice `app/Services/Audit/AuditService.php:44,59`).
Resolve local and imported type aliases during docblock type
parsing.

## B81. Guard-function narrowing gaps (`is_object`, `is_a`, `isset` on unions, truthiness, `is_numeric`)

**Severity: Low-Medium (~8 errors across projects) · Confirmed from output**

Assorted guard forms that PHPStan reconciles and we do not:

1. `isset($item->prop)` on a union that includes `stdClass` still
   flags the property as not found on the other union members —
   the check must pass when any candidate allows dynamic
   properties, and `isset()` itself guards the access
   (luxplus-website `app/Features/Carts/Services/SalesCampaignGroupDiscountService.php:200-205`).
2. `if (is_object($file))` does not stop `scalar_member_access` on
   the object branch (pdepend `tests/php/PDepend/AbstractTestCase.php:752`;
   the foreach union feeding it is B68).
3. `is_a($x, Extension::class, true)` / `class_exists($x)` do not
   narrow a `string` to `class-string<...>`
   (pdepend `src/DependencyInjection/PdependExtension.php:87`,
   luxplus-backoffice `app/Services/ScheduledJobsService.php:139`).
4. `if ($var)` truthiness does not strip `null` from a
   `string|null` produced by an earlier branch reassignment
   (luxplus-backoffice `app/Http/Controllers/Excel/InvoiceAnalyzerController.php:57`).
5. `is_numeric($s)` on a string narrows to bare `numeric`, which
   then fails a `string` parameter — the narrowed type must stay
   within the original (`numeric-string`)
   (luxplus-backoffice `app/Domain/UpdateFromSheet/Services/ProductPriceSheetService.php:378`).

Same structural home as B66 / T20 — prefer fixing these in the
reconciliation layer over one-off patches.

## B82. `assertInstanceOf` with a variable class argument destroys the subject's type

**Severity: Low (~3 errors, pdepend) · Confirmed from output**

`static::assertInstanceOf($expectedTypeClass, $reference)` where
the class is a variable cannot narrow to a concrete class — but it
must keep the subject's prior declared type instead of unresolving
it (`$reference->getImage()` fails afterwards even though
`$reference` was `ASTNode|null` before, pdepend
`tests/php/PDepend/Source/AST/ASTInstanceOfExpressionTest.php:204`).
Narrow to `object` (PHPStan's behaviour) intersected with the
prior type.

## B83. `class-string<A|B>` argument vs `class-string<T>` parameter reports a bogus mismatch

**Severity: Low (1 error, phpmd) · Confirmed with fixture**

```php
/** @param class-string<FunctionNode|MethodNode> $mockBuilder */
function demo(Builder $b, string $mockBuilder): void
{
    $b->build($mockBuilder);
    // "Argument 1 ($className) expects class-string<FunctionNode>,
    //  got FunctionNode|MethodNode"
}
```

Binding `class-string<A|B>` against `class-string<T>` truncates
the union to its first member on the "expects" side and strips the
`class-string` wrapper on the "got" side; the call is actually
well-typed (phpmd `tests/php/PHPMD/AbstractTestCase.php:556`).
