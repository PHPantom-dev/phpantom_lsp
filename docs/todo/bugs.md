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

## B73. `elseif` narrowing on a property-path subject leaks the preceding branch's `instanceof` type

**Severity: Low-Medium (1 error, bladestan) · Confirmed with a minimal fixture**

```php
class ArgNode {
    public StringNode|ArrayNode $value;
}

/** @param list<ArgNode> $args */
function extract(array $args): array {
    $values = [];
    if (count($args) === 2 && $args[0]->value instanceof StringNode) {
        $values[] = $args[0]->value->value;
    } elseif (count($args) === 1 && $args[0]->value instanceof ArrayNode) {
        foreach ($args[0]->value->items as $element) {   // false positive here
            $values[] = $element;
        }
    }
    return $values;
}
```

`$args[0]->value->items` in the `elseif` branch is flagged as
`Property 'items' not found on class 'StringNode'` — the `elseif`'s
own `instanceof ArrayNode` check is not being applied; the subject
is still carrying the *first* branch's `StringNode` narrowing. This
is the real-world Bladestan pattern (`src/PhpParser/NodeVisitor/
ViewFunctionArgumentsNodeVisitor.php:82`, `->with('key', $var)` vs.
`->with(['key' => $var])`, using `$args[0]->value` — a
`nikic/php-parser` `Node\Expr\ArrayItem::$value`, unioned as
`Expr\String_|Expr\Array_` in that codebase's terms).

Isolated behaviour: reproduces for **property-path subjects**
(`$args[0]->value`, and equally `$this->arg->value` — confirmed with
both shapes) inside an `if`/`elseif` chain where each branch's
condition is a compound `&&` (e.g. `count(...) === N && $subject
instanceof X`). Does **not** reproduce for a bare-variable subject in
the same if/elseif shape (`$node instanceof StringNode2` /
`elseif (... && $node instanceof ArrayNode2)`), which narrows
correctly. So the gap is specific to how property-path subjects
(as opposed to plain `$variable`s) are tracked across `elseif`
branches — plain variables get a fresh scope clone per branch
(`ei_scope = pre_if_scope.clone()` in `apply_condition_narrowing_inverse`'s
caller, `completion/variable/forward_walk.rs`), but the property-path
resolution may be going through a different, non-scope-cloning path.

Not root-caused to a specific function — two candidates were traced
but neither was conclusively confirmed as the one the `analyze` CLI
actually exercises for this case:

- `walk_property_narrowing_if` in `completion/resolver.rs` (~line
  1883) threads a single `&mut Vec<ClassInfo>` through the `if` body
  and then each `elseif` clause *in sequence* without resetting it to
  a pre-if snapshot between branches — structurally exactly the bug
  this report describes — but each mutation call
  (`try_apply_instanceof_narrowing`) is cursor-gated
  (`ctx.cursor_offset` must fall inside the branch's span), so if the
  diagnostic query's cursor sits only inside the `elseif` body, the
  `if` branch's narrowing call should be a no-op. Whether diagnostics
  actually invoke this cursor-based resolver per-usage-site (with
  cursor set to each access in turn), or exclusively use the
  scope-based forward walker, was not confirmed.
- `apply_condition_narrowing` / `apply_condition_narrowing_inverse`
  and their per-elseif caller in `completion/variable/forward_walk.rs`
  (~line 5952) do correctly clone `pre_if_scope` per elseif branch for
  scope-tracked keys, including property-path keys collected via
  `collect_condition_property_keys` — this looked correct on
  inspection, so if this is the actual path, the bug is more likely a
  key-mismatch (e.g. `expr_to_subject_key` producing a different
  string for the same subject in the condition-scan helpers vs. the
  member-access check) than a missing scope clone.

Whoever picks this up should add a `println!`/breakpoint trace (or a
`cargo test` fixture under the existing narrowing test files —
`tests/integration/completion_compound_narrowing.rs` or
`diagnostics_compound_narrowing.rs` look like the right home) to
confirm which pipeline handles this specific case before fixing it,
rather than assuming one of the two candidates above.

## B74. The auth-user-model patch does not apply through `FormRequest` inheritance

**Severity: Medium (~9 errors, luxplus-backoffice) · Confirmed with a minimal fixture against the real project's config**

```php
final class UpdateMembershipBenefitRequest extends FormRequest
{
    public function authorize(): bool
    {
        return $this->user()?->can('memberships.manage') ?? false;
    }
}
```

`$this->user()` is flagged "type of '$this->user()' could not be
resolved" (`unresolved_member_access` on `->can(...)`) in every
`FormRequest` subclass across `projects/luxplus-backoffice` that
calls `$this->user()`. Isolated with a throwaway fixture added
directly to the real project (using its real `config/auth.php` and
vendor tree, then removed): a class extending
`Illuminate\Foundation\Http\FormRequest` and calling
`$this->user()?->can(...)` reproduces the failure, while — per
`virtual_members/laravel/auth.rs`'s own doc comment — the identical
call on `Illuminate\Http\Request` or `Illuminate\Contracts\Auth\Guard`
directly is supposed to resolve to the project's configured auth user
model (that mechanism is `patch_auth_user_class`, gated in
`resolution.rs::find_or_load_class_typed` on `loaded.name.as_str()`
being exactly `"Guard"` or `"Request"`).

`FormRequest extends \Illuminate\Http\Request` and does not redeclare
`user()`, so the method is only reachable through inheritance
merging. The gate's exact-short-name check suggests the patch never
fires for `FormRequest` (or any other `Request` subclass) directly;
whether the *inherited* `user()` method picks up the patched return
type depends on whether the inheritance merge in `inheritance.rs`
loads the `Request` parent through `find_or_load_class_typed` (which
would already carry the patch) or through a different lookup that
bypasses it. Not traced further — whoever fixes this should add a
`FormRequest`-based case alongside the existing `auth_tests.rs`
fixtures once the actual code path is confirmed. Likely affects every
Laravel project's `FormRequest`/`Notification`/other `Request`
subclasses that call `$this->user()`, not just this one.
