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
