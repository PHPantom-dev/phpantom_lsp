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

The B91–B102 batch below comes from the 2026-07-16 full re-triage of
the three remaining non-clean projects (PDepend, Luxplus Website,
Luxplus Backoffice). Together they account for every remaining
analyze error in those projects (42 errors after the same sweep's
project-side patches). Each was reproduced with the minimal fixture
shown unless noted otherwise. Note that `unresolved_member_access`
is off by default — fixtures must be run in a project whose
`.phpantom.toml` sets `[diagnostics] unresolved-member-access = true`,
matching the sample projects.

## B91. Narrowing guards do not apply to array-index subject expressions

**Severity: Medium (~5 errors: pdepend ×3, luxplus-backoffice ×2) · Reproduced with fixture**

```php
/** @psalm-assert =ExpectedType $actual */ // PHPUnit's assertInstanceOf
static::assertInstanceOf(Wanted::class, $constants['C']);
$constants['C']->getImage(); // "type of '$constants['C']' could not be resolved"

if (!is_a($config['class'], Extension::class, true)) {
    throw new RuntimeException('nope');
}
$m->activateExtension($config['class']); // "expects class-string<Extension>, got string"
```

Assert-tag narrowing (`assertInstanceOf`) and `is_a(..., true)`
class-string narrowing both work when the subject is a plain
variable, but are silently dropped when the subject is an array
index expression (`$arr['key']`, `$arr[0]`). PHPStan keys narrowing
by printed expression, so index expressions narrow like any other
subject. Covers PDepend's `PdependExtension.php:87`
`type_mismatch_argument` and the `$constants['C']`/`$elements[0]`
accesses in its PHP 8.2 enum tests, plus the two
`$parameters['amount']->toFixed(2)` accesses in Backoffice's
BusinessCentral tests.

## B92. Assert narrowing cannot override variables assigned by list-destructuring from an unresolvable RHS

**Severity: Medium (~8 errors, pdepend) · Reproduced with fixture**

```php
[$type, $variable] = $declarations[0]; // RHS type unknown (bare array param)
static::assertInstanceOf(Wanted::class, $type);
$type->getImage(); // "type of '$type' could not be resolved"
```

A plain assignment from the same unresolvable RHS
(`$type = $declarations[0];`) narrows fine, and destructuring from a
*typed* RHS works. Only the combination — list-destructure whose RHS
cannot be resolved — leaves the variables in a state that later
assert narrowing cannot override. Accounts for all 8 `getImage()`
errors in PDepend's parser tests.

## B93. A `for` loop's init-clause assignment is invisible to the condition and update clauses

**Severity: Low (1 error, pdepend) · Reproduced with fixture**

```php
for ($previous = $e->getPrevious(); $previous; $previous = $previous->getPrevious()) {
    echo $previous->getMessage(); // body resolves fine
}
// update clause: "type of '$previous' could not be resolved"
```

The loop body sees `$previous`, but the update expression on the
`for` line itself does not, so the diagnostic fires on the `for`
statement. Rewriting as a `while` loop resolves. PDepend
`src/TextUI/Command.php:288`.

## B94. A closure parameter's declared union type is overridden by the inferred collection element type

**Severity: Medium (shares 3 errors with B95, luxplus-website) · Reproduced with fixture**

```php
/** @param Collection<int, CanApply>|Collection<int, ViewModel>|Collection<int, stdClass> $items */
public function probe(Collection $items): void
{
    $items->filter(function (CanApply|ViewModel|stdClass $item): bool {
        if (isset($item->salesCampaignGroupId)) {
            return $item->salesCampaignGroupId === 1; // "Property ... not found on class 'CanApply'"
        }
        return false;
    });
}
```

When the subject is a union of differently-parameterized
collections, the closure parameter collapses to the first union
member's element type (`CanApply`), discarding the parameter's own
declared union. A declared parameter type must win over (or at
least union with) the inferred element type. Website
`SalesCampaignGroupDiscountService.php`.

## B95. `isset($obj->prop)` guards and `property_exists()` ternaries do not prove the property on a single-typed subject

**Severity: Medium (shares 3 errors with B94, luxplus-website) · Reproduced with fixture**

```php
function probeIsset(CanApply $item): int
{
    if (isset($item->salesCampaignGroupId)) {
        return $item->salesCampaignGroupId; // "Property ... not found on class 'CanApply'"
    }
    return 0;
}

function probeTernary(CanApply $item): mixed
{
    return property_exists($item, 'qty') ? $item->qty : 1; // same
}
```

`property_exists()` in an `if` statement already proves the member
(shipped earlier); the ternary form does not, and `isset($obj->prop)`
does not in either form. PHPStan treats both as existence proofs for
the guarded access.

## B96. `reduce()`'s return template is not bound from the closure's inferred return type

**Severity: Medium (1 error, luxplus-website) · Reproduced with fixture**

```php
/**
 * @template TReduceInitial
 * @template TReduceReturnType
 * @param callable(TReduceInitial|TReduceReturnType, TValue, TKey): TReduceReturnType $callback
 * @param TReduceInitial $initial
 * @return TReduceReturnType
 */
public function reduce(callable $callback, $initial = null) {}

$total = $items->reduce(fn(Decimal $carry, $op) => $carry->add($op->getPrice()), new Decimal('0'));
$total->toFixed(2); // "type of '$total' could not be resolved"
```

`TReduceReturnType` only appears in the callable's return position,
so binding it requires inferring the closure's return type
(`Decimal`, from `$carry->add(...)`). Website
`OrderDetailedResource.php:155`.
