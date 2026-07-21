# PHPantom — Type Inference

Type resolution gaps: generic resolution, conditional return types,
type narrowing, PHP version features, and stub attribute handling.
Items that are purely about *completion UX* or *stub metadata
extraction* live in [completion.md](completion.md).

Items are ordered by **impact** (descending), then **effort** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Effort** | **Low** (≤ 1 day), **Medium** (2-5 days), **Medium-High** (1-2 weeks), **High** (2-4 weeks), **Very High** (> 1 month) |

---

## T2. File system watching for vendor and project changes
**Impact: Medium-High · Effort: Medium**

PHPantom loads Composer artifacts (classmap, PSR-4 mappings, autoload
files) once during `initialized` and caches them for the session. If
the user runs `composer update`, `composer require`, or `composer remove`
while the editor is open, the cached data goes stale. The user gets
completions and go-to-definition based on the old package versions
until they restart the editor.

### What to watch

| Path | Trigger | Action |
|---|---|---|
| `vendor/composer/autoload_classmap.php` | Changed | Reload classmap |
| `vendor/composer/autoload_psr4.php` | Changed | Reload PSR-4 mappings |
| `vendor/composer/autoload_files.php` | Changed | Re-scan autoload files for global functions/constants |
| `composer.json` | Changed | Reload project PSR-4 prefixes, re-check vendor dir |
| `composer.lock` | Changed | Good secondary signal that packages changed |

All three `autoload_*.php` files are rewritten atomically by Composer
on every `install`, `update`, `require`, `remove`, and `dump-autoload`.
Watching these is sufficient to catch any package change.

### Implementation options

1. **LSP `workspace/didChangeWatchedFiles`** — register file watchers
   via `client/registerCapability` during `initialized`. The editor
   handles the OS-level watching and sends notifications. This is the
   cleanest approach and works cross-platform. Register glob patterns
   for the vendor Composer files and `composer.json`.

2. **Server-side `notify` crate** — use the `notify` Rust crate to
   watch the file system directly. More control but adds a dependency
   and duplicates what the editor already provides.

Option 1 is preferred. The LSP spec's `DidChangeWatchedFilesRegistrationOptions`
supports glob patterns like `**/vendor/composer/autoload_*.php`.

### Reload strategy

- On change notification, re-run the same parsing logic from
  `initialized` for the affected artifact.
- Invalidate `fqn_uri_index` entries that came from vendor files (their
  parsed AST may have changed).
- Clear and re-populate `fqn_uri_index` from the new `autoload_classmap.php`.
- Log the reload to the output panel so the user knows it happened.
- Debounce rapid changes (Composer writes multiple files in sequence)
  with a short delay (e.g. 500ms) to avoid redundant reloads.

### `textDocument/didSave` handler

PHPantom does not currently implement `textDocument/didSave`. This
means changes to files that are not open in the editor (e.g. files
saved by a script, a git checkout, or another tool) are invisible
until the file is opened. This is standard behaviour for most LSPs,
but it matters for the file-watching story: even after
`workspace/didChangeWatchedFiles` is wired up for Composer artifacts,
changes to user PHP files made outside the editor (e.g. code
generation, `artisan make:model`) will not be picked up until the
file is opened.

When file system watching is implemented, consider also registering
a `didSave` handler (or a broad `*.php` watcher) to trigger a
targeted single-file rescan for files in PSR-4 directories, matching
the plan described in [indexing.md Phase 2](indexing.md#phase-2-staleness-detection-and-auto-refresh).

---

## T3. Property hooks (PHP 8.4)
**Impact: Medium · Effort: Medium**

PHP 8.4 introduced property hooks (`get` / `set`):

```php
class User {
    public string $name {
        get => strtoupper($this->name);
        set => trim($value);
    }
}
```

The mago parser (v1.8) already produces `Property::Hooked` and
`PropertyHook` AST nodes, and the generic `.modifiers()`, `.hint()`,
`.variables()` methods mean hooked properties are extracted for basic
completion. However:

- **Hook bodies are never walked.** Variables and anonymous classes
  inside `get`/`set` bodies are invisible to resolution.
- **`$value` parameter** inside `set` hooks is not offered for
  variable completion.
- **Asymmetric visibility** (`public private(set) string $name`) is
  not recognised — the `set` visibility is ignored, so filtering
  may incorrectly allow setting a property that should be
  write-restricted.

**Fix:** In `extract_class_like_members`, match `Property::Hooked`
explicitly, walk hook bodies for anonymous classes and variable
scopes, and parse the set-visibility modifier into a new
`set_visibility` field on `PropertyInfo`.

### Asymmetric visibility (also PHP 8.4 / 8.5)

Separate from hooks, PHP 8.4 allows asymmetric visibility on plain
and promoted properties. PHP 8.5 extended this to static properties.

```php
class Settings {
    public private(set) string $name;

    public function __construct(
        public protected(set) int $retries = 3,
    ) {}
}
```

PHPantom currently extracts a single `Visibility` per property.
Completion filtering uses this to decide whether a property should
appear. A `public private(set)` property should appear for reading
from outside the class but not for assignment contexts.

Add an optional `set_visibility: Option<Visibility>` to
`PropertyInfo`. Populate it from the AST modifier list (the parser
exposes the set-visibility keyword). Completion filtering does not
currently distinguish read vs write contexts, so the immediate fix
is just to store the value; context-aware filtering can follow later.

This shares the same `set_visibility` field as the hooked-property
fix above, so both should be implemented together.

---

## T4. Non-empty-* type narrowing and propagation
**Impact: Low-Medium · Effort: Low**

PHPStan tracks `non-empty-string` and `non-empty-array` through
built-in functions. These narrowings don't directly enable
class-based completion, but they improve hover type display and
would catch bugs if we add diagnostics. All three sub-items share
the same implementation pattern (function-name-triggered type
narrowing in conditions or return types) and should be implemented
together.

**String containment narrowing.** When `str_contains($haystack,
$needle)` appears in a condition and `$needle` is known to be a
non-empty string, narrow `$haystack` to `non-empty-string`. Same
for `str_starts_with`, `str_ends_with`, `strpos`, `strrpos`,
`stripos`, `strripos`, `strstr`, and the `mb_*` equivalents.
See `StrContainingTypeSpecifyingExtension` in PHPStan.

**Count narrowing.** `if (count($arr) > 0)` or
`if (count($arr) >= 1)` narrows `$arr` to `non-empty-array`.
PHPStan handles a full matrix of comparison operators and integer
range types against `count()` / `sizeof()` calls. See
`CountFunctionTypeSpecifyingExtension`.

**String function propagation.** Passing a `non-empty-string` to
`addslashes()`, `urlencode()`, `htmlspecialchars()`,
`escapeshellarg()`, `escapeshellcmd()`, `preg_quote()`,
`rawurlencode()`, or `rawurldecode()` should return
`non-empty-string`. See `NonEmptyStringFunctionsReturnTypeExtension`.

---

## T5. Fiber type resolution
**Impact: Low · Effort: Low**

`Generator<TKey, TValue, TSend, TReturn>` has dedicated support for
extracting each type parameter (value type for foreach, send type
for `$var = yield`, return type for `getReturn()`). `Fiber` has no
equivalent handling — `Fiber::start()`, `Fiber::resume()`, and
`Fiber::getReturn()` don't resolve their generic types.

PHP userland rarely annotates Fiber with generics (unlike Generator),
so this is low priority. If demand appears, the fix would mirror the
Generator extraction in `docblock/types.rs`.

---

## T6. `Closure::bind()` / `Closure::fromCallable()` return type preservation
**Impact: Low · Effort: Low-Medium**

Variables holding closure literals, arrow functions, and first-class
callables now resolve to the `Closure` class, so `$fn->bindTo()`,
`$fn->call()`, etc. offer completions.  The remaining gap is
*preserving the closure's callable signature* through `Closure::bind()`
and resolving `Closure::fromCallable('functionName')` to the actual
function's signature as a typed `Closure`.  This is relevant for DI
containers and middleware patterns but is a niche use case.

See `ClosureBindDynamicReturnTypeExtension` and
`ClosureFromCallableDynamicReturnTypeExtension` in PHPStan.

---



## T9. Dead-code elimination after `never`-returning calls
**Impact: Low · Effort: Low-Medium**

When a function or method has return type `never`, any code path that
calls it is guaranteed to terminate. Variables assigned before the
`never` call in a conditional branch should not have their type
polluted by the branch's assignments.

```php
$x = 'hello';
if (rand(0,1)) {
    $x = 'other';
    abort(); // returns never
}
$x; // should be "hello", not "hello"|"other"
```

Today PHPantom's branch-merging logic unions all branch assignments
regardless of whether the branch terminates. Recognising `never` as a
terminating statement (alongside `return`, `throw`, `die`, `exit`)
would fix this.

**Fixture to activate:**

- `type/never_return_type.fixture`

**phpactor ref:** `type/never.test`

---

## T10. Ternary expression as RHS of list destructuring
**Impact: Low · Effort: Low-Medium**

List destructuring (`[$a, $b] = expr`) resolves element types when
the RHS is a function call returning an array shape, or a simple
array literal. When the RHS is a ternary expression whose branches
are array literals or array-shape-returning calls, the resolver
doesn't drill into the branches to union the element types.

```php
[$a, $b] = $cond ? [new Foo(), new Bar()] : [new Bar(), new Foo()];
$a->  // should see Foo|Bar members
```

**Fixture to activate:**

- `assignment/list_destructuring_conditional.fixture`

**phpactor ref:** `assignment/list_assignment.test`

---

## T11. Nested list destructuring
**Impact: Low · Effort: Low-Medium**

Nested destructuring like `[[$one, $two]] = $source` is not resolved.
When the RHS has a type like `array{array{Foo, Bar}}`, the outer
destructuring peels the first dimension but the inner destructuring
doesn't resolve individual elements.

```php
/** @return array{array{Foo, Bar}} */
function getPair(): array { return [[new Foo(), new Bar()]]; }

[[$one, $two]] = getPair();
$one->  // should see Foo members
```

**Fixture to activate:**

- `assignment/nested_list_destructuring.fixture`

**phpactor ref:** `assignment/list_desconstruct_nested.test`

---

## T13. Closure variables lose callable signature detail
**Impact: Low-Medium · Effort: Medium**

When a variable holds a closure or arrow function, the resolution
pipeline resolves it to the `Closure` class name. The callable
signature (parameter types, return type) is lost. This means:

1. Passing `$fn` to an extracted method produces `Closure $fn` with
   `@param (Closure(): mixed)` instead of the concrete signature.
2. An explicit `/** @var (Closure(int): string) $fn */` annotation
   is recognised by variable resolution (`find_var_raw_type_in_source`
   returns the annotated type), but `clean_type_for_signature` now
   correctly extracts `Closure` as the native hint. The raw type is
   preserved for docblock generation.

The remaining gap is that *unannotated* closures like
`$fn = function(int $x): string { ... }` resolve to bare `Closure`
with no signature detail. `infer_closure_literal_type` (in
`rhs_resolution.rs`) already embeds the inferred return type in a
`PhpType::Callable`, but always leaves `params` empty, so it does not
produce a full callable type string for variable-type contexts.

**Example:**

```php
$fn = function(int $x): string { return (string)$x; };
// Extracting code that uses $fn as a parameter produces:
//   @param (Closure(): mixed) $fn
// Instead of:
//   @param (Closure(int): string) $fn
```

**What needs to change:**

1. When resolving a variable whose assignment RHS is a closure or
   arrow function, build a callable type string from the literal's
   parameter list and return type hint (e.g. `(Closure(int): string)`).
   Return this as the variable's type string instead of bare `Closure`.

2. `clean_type_for_signature` already handles parenthesized callable
   types by extracting the base name (`Closure` or `callable`), so
   the native hint will be correct.

3. `enrichment_plain` should recognise that a raw type like
   `(Closure(int): string)` already carries a full signature and
   should not be re-enriched to `(Closure(): mixed)`.

**After fixing:** verify that extract function docblock generation
emits the concrete callable signature in the `@param` tag.

---

## T20. Type narrowing reconciliation engine
**Impact: Medium-High · Effort: High**

PHPantom's type narrowing in `completion/types/narrowing.rs` handles
basic patterns (instanceof, is_* calls, null checks) but lacks the
algebraic framework that PHPStan and Psalm use. Key gaps:

1. No separate tracking of "sure types" vs "sure-not types". When
   `$x !== null`, PHPantom should remove `null` from the union
   (sure-not) rather than trying to intersect with "not-null".
2. No proper AND/OR algebra. `$a instanceof Foo && $b instanceof Bar`
   should union the narrowings in true context and intersect them in
   false context. Currently only simple cases work.
3. No truthy/falsey distinction. `if ($x)` (truthy) vs
   `if ($x === true)` (strict true) should produce different
   narrowings. PHPStan uses a 4-state bitmask context.
4. No assertion propagation from `@phpstan-assert` /
   `@psalm-assert` annotations on called functions. PHPantom parses
   these assertions but doesn't apply them as type narrowings at
   call sites.

**Design:** create a
`fn reconcile(existing: PhpType, assertion: Assertion, negated: bool) -> PhpType`
function that dispatches to per-assertion-kind narrowing logic. Start
with 15 core assertion kinds: IsType, IsNotType, IsNull, IsNotNull,
Truthy, Falsy, IsIdentical, IsNotIdentical, IsInstanceOf,
IsNotInstanceOf, HasMethod, HasProperty, IsGreaterThan, IsLessThan,
NonEmptyCountable.

**Reference:** Psalm has 41 assertion types under
`Psalm/Storage/Assertion/`. PHPStan's `TypeSpecifier` returns
`SpecifiedTypes` with dual sure/sureNot maps.

**Psalm's architecture (reference/psalm):**

Psalm converts conditions to **Conjunctive Normal Form** (CNF). Each
`Clause` is an OR-disjunction mapping variable string keys to assertion
sets. The `Algebra` class provides pure functions:

- `simplifyCNF` — unit propagation (`($a) ∧ ($a ∨ $b) → $a`)
- `getTruthsFromFormula` — extract definite truths from unit clauses
- `negateFormula` — De Morgan's law for the else-branch (mechanical,
  no separate logic needed)
- `combineOredClauses` — cartesian product for `||`

Key design decisions to adopt:

1. **Clauses are content-addressed** (xxh128 hash for dedup). In Rust,
   derive `Hash + Eq` and use `FxHashSet`.
2. **Complexity guards** — >65K clauses → bail out. Prevents exponential
   blowup without depth caps.
3. **Clauses accumulate in context** — entering an `if` ANDs new clauses
   with existing ones, giving compound narrowing for free.
4. **Variables identified by string keys** (`$a`, `$this->prop`,
   `$a->b[c]`) via an `ExpressionIdentifier` — maps to our
   `subject_extraction` approach.
5. **Assertions are first-class objects** with `getNegation()` — makes
   else-branch derivation trivial.
6. **Separate extraction from reconciliation** — `AssertionFinder`
   (AST → assertions) is purely separate from `Reconciler`
   (assertions + types → narrowed types). Each is independently
   testable.

See Psalm's `Psalm\Internal\Algebra`, `Psalm\Internal\Clause`, and
`Psalm\Internal\Analyzer\Statements\Expression\AssertionFinder`.

**Depends on:** The structured type representation (`PhpType`) has
landed, which makes reconciliation much simpler than working with
raw strings.

---



---

## T24. `stdClass` dynamic property access
**Impact: Low-Medium · Effort: Low**

`stdClass` is PHP's generic dynamic-property container. Accessing any
property on a value known to be `stdClass` (or narrowed to `object`
via `is_object()`) should not produce `unresolved_member_access`
diagnostics, because `stdClass` permits arbitrary properties by
design.

**Partially resolved.** Three changes landed:

1. `filter_type_by_guard` now narrows `mixed` → the canonical type
   for each guard kind (e.g. `is_object()` → `object`) instead of
   filtering `mixed` to empty.
2. `resolve_subject_outcome_variable` returns a synthetic
   `Resolved(stdClass)` when the resolved type is `object` or
   `stdClass`, so the existing `check_member_on_resolved_classes`
   suppression kicks in.
3. `try_apply_type_guard_narrowing` decomposes compound `&&`
   conditions so `if (is_object($x) && $x->prop)` narrows in both
   the condition RHS and the if-body. `apply_and_lhs_narrowing` also
   handles `is_object()` in `&&` inline narrowing.

This fixed `Order:646,647` (`json_decode` → `mixed` → `is_object`
guard → property access).

## T25. Call-site template argument inference for callable parameters

**Impact: Medium · Effort: Medium — partially done**

When a function has a `@template T` and a parameter typed
`callable(T): T`, the closure inlay hint system cannot resolve `T`
to a concrete type because it reads the callable signature literally.
For example:

```php
/**
 * @template T
 * @param array<T> $items
 * @param callable(T): T $fn
 * @return array<T>
 */
function transform(array $items, callable $fn): array { ... }

transform([1, 2, 3], fn($x) => $x * 2);
//                      ^ no hint — $x is T, not int
```

To show `int` for `$x`, the hint system needs to:

1. Resolve other arguments at the call site to infer `T = int` from
   `array<T>` matched against `[1, 2, 3]` (which is `array<int>`).
2. Substitute `T → int` in the callable's parameter and return types.
3. Pass the substituted `callable(int): int` to the hint emitter.

**Step 1 (done):** `emit_closure_hints` in `inlay_hints.rs` now
accepts the `call_sites` slice, finds the matching `CallSite` for
each `UntypedClosureSite`, extracts the full argument text from
content, and passes it to `resolve_callable_target_with_args`
instead of the no-args `resolve_callable_target`. This wires the
existing `build_function_template_subs` / `build_method_template_subs`
machinery into the inlay hint path. Integration tests document the
desired behaviour.

**Step 2 (done):** The `CallSite` matching logic works correctly
for offset comparison. The actual issue was that
`build_function_template_subs` did not handle array literal
arguments (e.g. `[1, 2, 3]`) for `GenericWrapper` binding mode.
The `GenericWrapper("array", 0)` arm only resolved `$variable`
arguments via `resolve_arg_variable_raw_type`, skipping literals.
Fixed by adding array-literal element inference in the
`GenericWrapper` arm of `build_function_template_subs`, so
`each([1, 2, 3], fn($x) => ...)` now infers `T = int` and shows
the correct type hint for `$x`.

**References:**
- PHPStan: `GenericFunctionsReturnTypeExtension`, argument-based
  template inference in `FunctionCallNode`.
- Mago: `resolve_template_arguments` in the type checker.

## T26. Globbed constant unions (`Foo::BAR_*`)

**Impact: Low · Effort: Low**

Resolve wildcard constant patterns like `Foo::BAR_*` to the union of
all matching constant types on the class. PHPStan supports this syntax
in docblock type strings:

```php
class Status {
    const STATUS_ACTIVE = 1;
    const STATUS_INACTIVE = 2;
    const STATUS_PENDING = 3;
}

/** @param Status::STATUS_* $status */
function setStatus(int $status): void { ... }
// $status should resolve to 1|2|3
```

When the type engine encounters a constant pattern containing `*`,
it should:

1. Resolve the class (`Status`).
2. Enumerate all constants matching the glob pattern (`STATUS_*`).
3. Build a union of their literal types.

**References:**
- PHPStan: `ConstantWildcardType` / constant enum resolution.
- Phpactor: `GlobbedConstantUnionType`.

---

## T27. Per-expression type caching during forward walk

**Impact: Medium-High · Effort: Medium**

The forward walker re-resolves the same expressions repeatedly during
a single analysis pass. For example, in `$a->foo()->bar()->baz()`,
resolving `baz()` requires re-resolving `$a->foo()->bar()`, which
re-resolves `$a->foo()`, which re-resolves `$a`. This creates
exponential work on chained expressions.

**Fix:** Cache resolved types per AST span (or node identity) for the
duration of a single forward-walk pass. When a sub-expression has
already been resolved, return the cached result in O(1). The cache is
invalidated entirely when the file changes (since spans change).

Psalm's `NodeTypeProvider` implements exactly this pattern: a simple
`setType(node, type)` / `getType(node)` interface keyed by AST node
identity. Types are cached for one analysis pass, not persisted. Only
expressions, names, and return statements get cached (not all nodes).

**Design:**

1. Add a `HashMap<TextRange, PhpType>` (or `FxHashMap`) to the forward
   walker's state, scoped to the current walk invocation.
2. Before resolving any expression, check the cache by span.
3. After resolving, store the result.
4. Clear the cache when starting a new file or when the file content
   changes.

This eliminates the exponential re-resolution that causes performance
issues on deeply chained expressions (P20 class of problems).

**References:**
- Psalm: `NodeTypeProvider` interface (`Psalm\NodeTypeProvider`)
- Mago: per-node type caching via `spl_object_id` equivalent

---

## T28. Template inference depth priority (shallowest bound wins)

**Impact: Medium · Effort: Low-Medium**

When a generic type parameter is inferred from multiple argument
positions at a call site, all inferred types are currently unioned.
This produces overly broad types when a shallow (direct) inference
and a deep (nested) inference compete.

**Fix:** Track an `appearance_depth` on each inferred template bound.
When the same template param receives bounds from multiple sources,
the shallowest (most direct) match wins. Only union bounds at the
same depth level.

For example, given:
```php
/** @template T */
function wrap(T $value, array<T> $context): T { ... }
wrap("hello", [1, 2, 3]);
```

The `$value` argument gives `T = string` at depth 0. The `$context`
argument gives `T = int` at depth 1 (inside `array<>`). The shallow
bound (`string`) should win, not produce `string|int`.

**Design:**

```
struct TemplateBound {
    ty: PhpType,
    depth: u8,        // 0 = direct match, 1 = one generic layer deep, etc.
    arg_offset: u8,   // which argument produced this bound
}
```

When selecting the final type for a template param:
1. Group bounds by depth.
2. Take the shallowest depth group.
3. Union the types within that group.

**References:**
- Psalm: `TemplateBound` with `appearance_depth` and
  `getMostSpecificTypeFromBounds` in `Psalm\Internal\Type\TemplateResult`

---

## T29. Definite vs possible variable existence tracking

**Impact: Medium · Effort: Medium**

PHPantom currently treats all assigned variables as definitely in
scope. This causes false negatives: a variable assigned only inside
one branch of an `if` (without the other branch) is treated as
always available after the `if`.

**Fix:** Split variable tracking into two maps:

1. **`vars_in_scope`** — variables with a definite type (assigned on
   all code paths reaching this point).
2. **`vars_possibly_in_scope`** — variables that *might* exist
   (assigned in only one branch). Accessing these without a guard
   could be flagged as "possibly undefined."

Psalm's `Context` uses exactly this split. The `vars_in_scope` map
holds `Union` types for definitely-typed variables. The
`vars_possibly_in_scope` map is a boolean set tracking variables that
might exist. After an if/else where only one branch assigns `$x`,
`$x` moves from `vars_in_scope` to `vars_possibly_in_scope` (or is
removed from `vars_in_scope` and added to `vars_possibly_in_scope`).

Additionally, contextual flags like `inside_isset` and
`inside_conditional` should suppress diagnostics about undefined
variables in those positions (accessing `$x` inside `isset($x)` is
intentional).

**Design:**

1. Add a `possibly_defined: HashSet<SmolStr>` alongside the existing
   variable type map in the forward walker state.
2. When merging branches (if/else, try/catch), variables assigned in
   only one path move to `possibly_defined`.
3. Hover shows `T|undefined` or similar annotation for possibly-defined
   variables.
4. Future diagnostic (D-series) can warn on access of possibly-undefined
   variables.

**References:**
- Psalm: `Context::$vars_in_scope` and `Context::$vars_possibly_in_scope`
  (`Psalm\Context`)

---

## T30. Literal type collapse limit

**Impact: Low-Medium · Effort: Low**

When combining union types, if the number of literal type variants
(literal strings, literal ints) exceeds a threshold, collapse them to
the parent scalar type. Without this, large switch statements, array
initializers, or enum-like constant groups can produce unbounded union
types that consume excessive memory and slow down type display.

**Fix:** In `PhpType` union combining logic, after merging two types:
if the result contains more than 500 literal string values, replace
them all with `string`. Same for literal ints → `int`.

Psalm uses exactly this threshold (500 literals → scalar collapse) in
`Type::combineUnionTypes()`. The number is chosen to be high enough
that normal code never hits it, but low enough to prevent pathological
blowup.

**References:**
- Psalm: literal limit in `Type::combineUnionTypes()` (`Psalm\Type`)





## T31. Closure literal-return shape inference

**Impact: Low-Medium · Effort: Medium**

A closure whose native return hint is just `array` but whose body
returns a literal array should get the literal shape as its
inferred return type, so `array_map()` results carry it:

```php
$declarations = array_map(function (ASTNode $child): array {
    return [$child->getType(), $childChildren[1]];
}, $children);

[$type, $variable] = $declarations[$index];
$type->getImage();   // "type of '$type' could not be resolved"
```

Found in the 2026-07 analyze triage: ~10 pdepend errors
(`tests/.../PHP82/TrueTypeTest.php:89`,
`AllowNullAndFalseAsStandAloneTypesTest.php:94`,
`PHPParserVersion81Test.php:1191,1480`) destructure tuples out of
`array_map` results built this way. PHPStan infers the shape from
the return statements; without it the destructured elements are
unresolved. Depends on nothing else in this file; complements the
call-site inference work in T25.

**References:**
- PHPStan: closure return type inference in
  `ClosureReturnStatementsNode` / `TypeSpecifyingExtension` flow.
- Psalm: `ClosureAnalyzer` return-type widening.
