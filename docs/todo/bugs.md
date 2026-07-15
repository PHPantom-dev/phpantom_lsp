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

## B71. `property_exists()` / `method_exists()` guards do not narrow the member set

**Severity: Medium (6 errors, api-php) · Confirmed against the real project**

```php
function validateResponse(AbstractResponse $response): void
{
    if (property_exists($response, 'MerchantErrorMessage')) {
        if ($response->MerchantErrorMessage && is_string($response->MerchantErrorMessage)) {
            throw new ResponseMessageException($response->MerchantErrorMessage);
        }
    }
}
```

(`src/AbstractApi.php:258-265` in `projects/api-php`, real code, not a
fixture.) `AbstractResponse` doesn't declare `MerchantErrorMessage` —
it's a dynamically populated response property — so accessing it
unconditionally would be a genuine gap. But the access is guarded by
`property_exists($response, 'MerchantErrorMessage')`, which proves the
property exists for the rest of the branch. PHPStan models this via
its `PropertyExistsTypeSpecifyingExtension`
(`references/phpstan-src/src/Type/Php/PropertyExistsTypeSpecifyingExtension.php`),
narrowing `$response` to `object&hasProperty('MerchantErrorMessage')`
in the truthy branch. We have no equivalent, so all 6 accesses in this
pattern (`MerchantErrorMessage` ×3, `CardHolderErrorMessage` ×2,
`CardHolderMessageMustBeShown` ×1) are reported as
`unknown_member` even though PHPStan considers the file clean at
level max. This bucket in `projects/analyze-triage.md` was previously
(and incorrectly) written up as an intentional "documented SDK gap" —
that classification undercounted PHPantom's false positives and
should be reverted once this is fixed.

`method_exists($x, 'name')` has the identical shape and is presumably
affected too, though no sample project exercises it.

A previous session started on a fix (narrowing via a virtual member
injected into the resolved-type's `ClassInfo` for the guarded branch,
hooked into `apply_condition_narrowing` /
`apply_condition_narrowing_inverse` in
`completion/variable/forward_walk.rs`, with the guard extraction in
`completion/types/narrowing.rs::try_extract_member_exists_guard`) but
was stopped mid-implementation because it had not been authorized —
see the project rule about one task at a time and no sub-agents
working the LSP in parallel. The unfinished diff, including its own
integration tests, is saved at
`docs/todo/patches/property-exists-narrowing.patch` (apply with `git
apply` from the repo root). It compiled and 9 of 10 new tests passed;
the one known-failing test
(`property_access_outside_property_exists_guard_still_flagged`)
indicates the narrowing was leaking out of the guarded branch,
suspected to be a missing case in `ScopeState::merge_branch` (the
newly-added virtual property must not survive a branch merge with a
sibling branch that lacks it). Treat the patch as a reference starting
point, not a finished fix — it needs review, the merge-branch leak
fixed, and a decision on whether `already_present` should also check
inherited members (currently it only checks `class_info.properties`
directly, which is a deliberate but undocumented-to-the-team
trade-off; see the patch's own comment).

## B72. `new $className()` does not resolve to the class named by a `@var class-string<T>` annotation

**Severity: Medium (6 errors, phpmd) · Confirmed against the real project, not yet isolated to a minimal fixture**

```php
$className = $ruleNode['class'] ?? (...);
if ((!$className instanceof Stringable) && !is_string($className)) {
    throw new RuntimeException('Invalid class');
}

/** @var class-string<Rule> */
$className = (string) $className;

$rule = new $className();
$this->withNonEmptyStringAtKey($ruleNode, 'name', $rule->setName(...));
// ...
if ($rule->getPriority() <= $this->minimumPriority && ...) {
```

(`src/RuleSetFactory.php:82-123` in `projects/phpmd`, real code.) The
bare `/** @var class-string<Rule> */` (no `$variableName` in the tag,
which is valid PHPDoc/PHPStan syntax when the tag directly precedes
the assignment it documents) should override `$className`'s type to
`class-string<Rule>`, and `new $className()` should then resolve to
`Rule`. Instead every member access on `$rule` after the
instantiation (`setName`, `setMessage`, `setExternalInfoUrl`,
`setRuleSetName`, `getPriority` ×2) is reported as
`unresolved_member_access` ("type of '$rule' could not be resolved").
PHPStan resolves this cleanly (the project passes at level max), so
this is a real gap, not an intended diagnostic.

Suspect areas: whether the bare (name-less) `@var` tag form is
recognized at all by the docblock parser when the annotated statement
is a self-reassignment (`$className = (string) $className;`, same
variable on both sides) rather than a fresh binding, and whether
`new $className()` reads the class-string's type argument via the
same effective-type path as an explicit `@var class-string<Rule>
$className`. Not yet isolated to a standalone fixture outside the
project — attempts to reproduce it in a scratch file did not trigger
the same failure, and further live bisection inside
`projects/phpmd` was abandoned because the installed
`phpantom_lsp` CLI (`~/.local/bin/phpantom_lsp` →
`target/debug/phpantom_lsp`) is being actively rebuilt by a
concurrent session, making repeated `analyze` runs an unreliable
moving target for A/B comparison. Whoever picks this up should
rebuild a pinned binary first (or use `cargo test`/a fixture-based
repro instead of the CLI) before bisecting further.
