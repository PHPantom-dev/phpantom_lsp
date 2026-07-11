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

## B44. `@phpstan-assert` narrowing only applies when called via the declaring class name

**Severity: High (~200+ false positives; the largest single bucket) · Reproduced**

A static method with `@template T of object`, `@param
class-string<T> $expected`, and `@phpstan-assert T $actual`
narrows `$actual` only when invoked as
`DeclaringClass::assertInstanceOf(...)`. The same call via
`self::`, `static::`, `$this->`, or a subclass name
(`Sub::assertInstanceOf(...)`) performs no narrowing, so the
variable stays `mixed`/unresolved afterwards.

This is exactly the PHPUnit shape: `assertInstanceOf` is declared
on `PHPUnit\Framework\Assert` and always called through a
`TestCase` subclass as `static::assertInstanceOf(X::class, $v)`
or `$this->assertInstanceOf(...)`. Dominates the pdepend and
phpmd test suites (`$copy = unserialize(...)` +
`assertInstanceOf` alone is ~25 errors; the PHP 8.x feature test
files are ~60 more) and appears in the Luxplus projects.

Repro: declare the asserter above, subclass it, call
`self::assertInstanceOf(Node::class, $v)` on a `mixed $v`, then
`$v->getName()` → `unresolved_member_access`. Calling via the
declaring class name works.

Note: PHPUnit needs no stubs or patches for this — since ~9.5 it
ships the assertion annotations natively (`@psalm-assert
=ExpectedType` on PHPUnit 10, `@phpstan-assert =ExpectedType` on
11+, verified in the pdepend/phpmd/luxplus vendor dirs). The
PHPUnit test-suite errors are this bug compounded with B61 (the
`=` prefix is not parsed); both must be fixed. Only projects on
ancient PHPUnit (< 9.5, no annotations) would need a stub patch
via the external-stubs system (`docs/todo/external-stubs.md`) —
that is a follow-up, not part of this fix.

**Fix:** resolve the assertion metadata through the same
class-loading path used for the call itself (merged/inherited
method lookup), not just a direct name match on the receiver
text. `self`, `static`, `parent`, `$this`, and subclass receivers
must all find `type_assertions` on the declaring method.

## B45. No narrowing of property or method-call subjects inside ternary conditions

**Severity: High (widespread in pdepend/phpmd src + Laravel projects) · Reproduced**

`$this->node instanceof Artifact ? $this->node->getCompilationUnit() : null`
flags `getCompilationUnit` as unknown on the declared type — the
`instanceof` in the ternary condition does not narrow
`$this->node` in the then-branch. Same for repeated method-call
subjects: `$request->user() ? $request->user()->id : null`
(luxplus-website `app/Exceptions/Handler.php:74`).

Plain **variable** subjects narrow correctly in ternaries
(verified) — the gap is property accesses and method calls as
subjects.

Cascading effect: because the then-branch fails to resolve, the
whole ternary's result type collapses to the else-branch (`null`)
instead of a union, so a later
`$compilationUnit ? $compilationUnit->getFileName() : null`
reports `scalar_member_access` on type 'null'
(phpmd `src/AbstractNode.php:248-252`).

**Fix:** apply the existing narrowing logic to non-variable
subjects keyed by normalized expression text (PHPStan keys its
resolved-type cache by printed expression), and make the ternary
result type the union of both branch types even when one branch
only partially resolves.

## B46. Short-circuit narrowing missing on the right side of boolean OR

**Severity: Medium-High (phpmd/pdepend src, common guard idiom) · Reproduced**

```php
if (!$arg instanceof ASTMemberPrimaryPrefix || !$arg->isStatic()) {
    continue;
}
```

The right operand of `||` executes only when the left is false,
so `$arg` is `ASTMemberPrimaryPrefix` there — we flag
`isStatic` as unknown on the declared type instead
(phpmd `src/Node/Attributes.php:73`,
`src/Rule/AbstractLocalVariable.php:107`). Verify the `&&`
mirror case (`$x instanceof T && $x->m()`) while fixing.

**Fix:** propagate left-operand narrowing (negated) into the
right operand of `||`, and the non-negated form into the right
operand of `&&`, within a single condition expression.

## B47. Assignment inside a condition leaves the variable unresolved

**Severity: Medium-High (~40+ errors: pdepend `$token` loops, luxplus) · Reproduced**

Assignments embedded in `if`/`while` conditions do not register
as definition sites:

```php
if (!$item = $this->getCartItemForProduct($productId)) {
    return;
}
$item->getQty();            // unresolved

while (is_object($token = $tokenizer->next())) {
    $actual[] = $token->type;   // unresolved (~29 errors in pdepend)
}
```

Both the bare negated form (`!$x = expr`) and the
wrapped-in-a-call form (`is_object($x = expr)`) fail
(luxplus-website
`app/Features/Carts/Services/ShoppingCartItemService.php:62`,
pdepend `tests/php/PDepend/Bugs/ParserBug124Test.php:74`).
As a bonus the guard should also narrow (falsy check /
`is_object`), but the primary bug is that the assignment is not
seen at all.

**Fix:** the forward walker must treat assignment expressions in
condition position (including nested inside call arguments and
unary `!`) as def sites, then apply the surrounding guard's
narrowing.

## B48. Error-suppression prefix breaks RHS resolution

**Severity: Medium · Reproduced**

`$xml = @simplexml_load_string($content);` leaves `$xml`
unresolved — every later member access reports
`unresolved_member_access` (pdepend
`src/Baseline/BaselineSetFactory.php:31-32`). Without the `@` the
same assignment resolves.

**Fix:** unwrap the suppression unary prefix in
`resolve_rhs_expression` and resolve the inner expression.

## B49. SimpleXMLElement iteration and children()/attributes() yield untyped elements

**Severity: Medium (~25+ errors in api-php, cascades) · Reproduced**

`foreach ($xml->children() as $child)` leaves `$child`
unresolved, so `$child->getName()` etc. all report
`unresolved_member_access`
(api-php `src/Response/AbstractResponse.php:81-121`). Iterating a
`SimpleXMLElement` yields `SimpleXMLElement`s, but the stub types
`children()`/`attributes()` as `?SimpleXMLElement` and the class
iterates itself without generics, so element extraction finds
nothing. PHPStan hardcodes this: iterating `SimpleXMLElement` (or
a subclass) yields `static`.

**Fix:** special-case foreach element types for
`SimpleXMLElement` and subclasses (yield the receiver class), the
same way PHPStan does.

## B50. Integer literals rejected by refined-int pseudo-types

**Severity: Medium (clear FP, common PHPUnit pattern) · Reproduced**

`takesNonNeg(1)` against `@param non-negative-int $count` reports
`type_mismatch_argument` "expects non-negative-int, got 1"
(luxplus-backoffice `$this->addToAssertionCount(1)`, 6 errors).
Literal ints already satisfy `int<min,max>` ranges; the named
refinements (`non-negative-int`, `positive-int`, `negative-int`,
`non-positive-int`, `non-zero-int`) must accept (or reject)
literals by value the same way.

**Fix:** in the argument compatibility check, evaluate int
literals against the named refinement's constraint instead of
falling through to name comparison.

## B51. String literal naming a class rejected by `class-string<Bound>`

**Severity: Medium (31 errors in pdepend) · Reproduced**

`$this->expectException('RuntimeException')` reports "expects
class-string<Throwable>, got 'RuntimeException'". A string
literal that names an existing class satisfying the bound is a
valid `class-string<Bound>`.

**Fix:** when the argument is a string literal and the param is
`class-string<Bound>`, resolve the literal's content as a class
name; stay silent when it resolves to a subtype of the bound (or
when it cannot be resolved); flag only a provable non-subtype.

## B52. String literals bind class-string templates to the string type

**Severity: Medium (~25 errors in pdepend/phpmd/api-php) · Reproduced**

`assertInstanceOf('Iterator', $engine->analyze())` reports
"Argument 1 ($expected) expects class-string<string>, got
'Iterator'": `T` was bound to the literal's own PHP type
(`string`) instead of the class it names, producing the absurd
`class-string<string>` and a guaranteed mismatch. `X::class`
arguments bind correctly; plain string literals do not. Also
produces "expects class-string<class-string>, got class-string"
(api-php `tests/FactoryTest.php`).

**Fix:** when binding `T` through a `class-string<T>` parameter,
bind to the class named by the literal's content (mirroring the
`::class` path), never to the literal's own type.

## B53. Template binding from `Class::CONST` arguments binds the class, not the constant type

**Severity: Medium (~12 errors: phpmd, pdepend, luxplus) · Reproduced**

`assertSameLike(WithConsts::CODE, $x)` with `@template T @param T
$expected` reports "expects FPCheck\WithConsts, got int" — `T`
was bound to the constant's owning class instead of the
constant's value type, then the argument (correctly typed `int`)
mismatches the wrong binding. Real-world:
`static::assertSame(Command::INVALID, $exitCode)`
(phpmd `tests/php/PHPMD/TextUI/CommandTest.php:168` and friends;
same shape with `Response::HTTP_OK` in luxplus).

**Fix:** in call-site template binding, type a
class-constant-access argument by the constant's declared/value
type (the same type the argument-check side already computes).

## B54. Variables captured by reference in closures are flagged unused

**Severity: Medium (unused_variable FP) · Reproduced**

```php
$lastId = null;
$fn = function () use (&$lastId): void { $lastId = 5; };
$fn();
return $lastId;    // "Unused variable '$lastId'" on the init line
```

Real-world: luxplus-backoffice
`app/Jobs/Elastic/ReindexCustomers.php:58` (init + by-ref capture
+ read after the closure). A `use (&$var)` capture must count as
a use (conservatively: both read and write) of the outer
variable.

**Fix:** in the unused-variable scan, treat by-reference closure
captures as uses of the captured variable.

## B55. Union-typed unknown-member check ignores `__call` catch-alls

**Severity: Low-Medium (5+ errors, Mockery-heavy test suites) · Confirmed from output**

"Method 'requestRefund' not found on any of the 3 possible types
(Mockery\Expectation, Mockery\ExpectationInterface,
Mockery\ExpectsHigherOrderMessage)" — but
`ExpectsHigherOrderMessage` has `__call`
(mockery `library/Mockery/ExpectsHigherOrderMessage.php:26`), so
the access is dynamically dispatched and must not be flagged. The
single-class path already respects `__call`; the
union/multiple-candidates path does not
(luxplus-backoffice `tests/Feature/Orders/CreateRefundTest.php:113`).

**Fix:** in the union branch of the unknown-member check, stay
silent when any candidate type has `__call`/`__callStatic`/`__get`
(matching the single-type behavior).

## B56. `__benevolent<T>` pseudo-type reported as unknown class

**Severity: Low (1 error, trivial) · Confirmed from output**

`@var __benevolent<Loop|null>` produces "Class
'Bladestan\ValueObject\__benevolent' not found"
(bladestan `src/ValueObject/Loop.php:39`). PHPStan's
`__benevolent<T>` wrapper should parse as its inner type and
never be treated as a class name.

**Fix:** recognize `__benevolent<T>` in the type parser and
unwrap to `T`.

## B57. Parameter nullability lost from docblock overrides and null defaults

**Severity: Medium (~12 errors across luxplus/api-php/phpmd) · Confirmed from output**

Two facets of the same effective-type computation:

1. A docblock `@param MenuItemViewModel[] $menu_items` overriding
   a native `?array $menu_items` drops the `null`, so passing
   `null` reports "expects array<MenuItemViewModel>, got null"
   (luxplus-website `app/View/Components/Menu/Menu.php:136`,
   7 errors — the flagged calls construct `MenuItemViewModel`,
   whose param 3 is natively nullable).
2. A parameter with default `null` (`$baseurl = null`, including
   the pre-8.4 implicit-nullable form `Type $x = null`) must
   accept `null` regardless of the docblock type
   (api-php `src/Api/Test/TestConnection.php:46`, phpmd
   `src/TextUI/Command.php:404` `$context` resource null).

**Fix:** when merging docblock and native param types, preserve
native nullability; and union `null` into the effective type when
the default value is `null`.

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

## B61. Exact-type assertion prefix `=` is not parsed

**Severity: High (blocks all PHPUnit narrowing even after B44) · Reproduced**

`@phpstan-assert =T $actual` and `@psalm-assert =ExpectedType
$actual` (the "exact type, no subtype widening" form) are not
recognized: the `=` is kept as part of the type name, producing a
bogus `unknown_class` diagnostic on the docblock ("Class '=T' not
found") and no narrowing at the call site. The plain form
(`@psalm-assert T $actual`) works.

This is the exact form PHPUnit ships: PHPUnit 10 uses
`@psalm-assert =ExpectedType $actual` and PHPUnit 11+ uses
`@phpstan-assert =ExpectedType $actual` on `assertInstanceOf`,
`assertSame`, and friends — so every vendored PHPUnit in the
sample projects hits this. Repro: copy PHPUnit's
`assertInstanceOf` docblock onto a stub class and call it via the
declaring class name; narrowing fails and the `=T` unknown-class
error appears on the declaration.

**Fix:** strip the leading `=` when parsing assertion types (for
narrowing purposes exact and subtype asserts behave the same) in
the `@phpstan-assert` / `@psalm-assert` family, including the
`-if-true` / `-if-false` variants, and make sure docblock type
spans for these tags don't feed the unknown-class diagnostic with
the raw `=`-prefixed token.
