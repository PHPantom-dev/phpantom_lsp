# PHPantom — Type Inference

Type resolution gaps: generic resolution, conditional return types,
type narrowing, PHP version features, and stub attribute handling.
Items that are purely about *completion UX* or *stub metadata
extraction* live in [completion.md](completion.md).

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## T3. Property hooks (PHP 8.4)
**Impact: Medium · Complexity: Medium-High**

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

- **Asymmetric visibility** (`public private(set) string $name`) is
  not recognised — the `set` visibility is ignored, so filtering
  may incorrectly allow setting a property that should be
  write-restricted.

**Fix:** Parse the set-visibility modifier into a new
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
**Impact: Low-Medium · Complexity: High**

PHPStan tracks `non-empty-string` and `non-empty-array` through
built-in functions. These narrowings don't directly enable
class-based completion, but they improve hover type display and
would catch bugs if we add diagnostics. The first three sub-items
share the same implementation pattern (function-name-triggered type
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

**Element writes produce a non-empty array.** After `$a[$k] = $v` or
`$a[] = $v` the array holds at least one element, so PHPStan reports
`non-empty-array<K, V>` / `non-empty-list<V>`. We report the
possibly-empty spellings, which loses the guarantee that `reset()`,
`array_pop()` and friends return a value rather than `false`.

Emitting the non-empty spelling from the write helpers
(`merge_push_type`, `merge_keyed_type` in
`type_engine/variable/resolution.rs`) is a two-line change, but it is
only half the work: every place that joins branches has to relax the
guarantee again when one side never ran. A `foreach` whose body writes
a key merges the pre-loop scope back in at exit, and today that join
either drops the `array{}` alternative outright (reporting
`non-empty-array` after a loop that may not have iterated at all) or
keeps it as a separate union member (`array{}|non-empty-list<string>`,
whose offset reads then carry a spurious `null`). Both spellings are
wrong; the join has to widen `non-empty-X` to `X` instead. So the work
is a possibly-empty widening at each join point, not a change to the
write helpers.

---

## T5. Fiber type resolution
**Impact: Low · Complexity: Medium**

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
**Impact: Low · Complexity: Medium-High**

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



## T10. Ternary expression as RHS of list destructuring
**Impact: Low · Complexity: Medium**

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
**Impact: Low · Complexity: Medium**

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
**Impact: Low-Medium · Complexity: Medium-High**

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
`TypeKind::Callable`, but always leaves `params` empty, so it does not
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
**Impact: Medium-High · Complexity: Very High**

PHPantom's type narrowing in `type_engine/types/narrowing/` handles
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

`@phpstan-assert`/`@psalm-assert` annotations on called functions are
now applied as narrowings at call sites (`extract_call_assertions` and
`CallAssertionInfo` in `type_engine/types/narrowing/assertions.rs`,
consulted from `type_engine/types/narrowing/guards.rs` and
`type_engine/variable/forward_walk/cond_narrowing.rs`), so that gap
from the original list is closed. It is still ad hoc rather than
going through a unified `reconcile` dispatch, so items 1-3 remain.

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

## T26. Globbed constant unions (`Foo::BAR_*`)

**Impact: Low · Complexity: Medium**

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

## T28. Template inference depth priority (shallowest bound wins)

**Impact: Medium · Complexity: Medium-High**

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

**Impact: Medium · Complexity: High**

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

**Impact: Low-Medium · Complexity: Medium**

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

**Impact: Low-Medium · Complexity: Medium-High**

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
unresolved. Depends on nothing else in this file.

**References:**
- PHPStan: closure return type inference in
  `ClosureReturnStatementsNode` / `TypeSpecifyingExtension` flow.
- Psalm: `ClosureAnalyzer` return-type widening.

---

## T33. Class constant on an expression (`$obj::CONST`) resolves to nothing
**Impact: Low-Medium · Complexity: Medium**

A class constant reached through a variable produces no type at all, so
hover is blank and anything downstream of it loses the value. The same
constant reached through a class name or `self` resolves fine.

```php
class Foo {
    const INTEGER_CONSTANT = 1;
}

$foo = new Foo();
$a = Foo::INTEGER_CONSTANT;    // 1
$b = $foo::INTEGER_CONSTANT;   // (nothing — PHPStan: 1)
```

The class-constant branch of `resolve_rhs_property_access` matches only
`Identifier`, `Self_`, `Static`, and `Parent` for the left-hand side and
returns an empty vec for anything else. Resolving the subject expression
to a class first (the forward walker already does this for `$obj->prop`
in the same function) would cover `$obj::CONST`, and the constant lookup
below it needs no change.

**Tests to update once fixed:** upstream's `nsrt/deducted-types.php` has
a `$foo::INTEGER_CONSTANT` block that was dropped when
`tests/phpstan_nsrt/deducted-types.php` was ported; port it back.

---

## T34. `static::CONST` over-narrows to the declaring class's value
**Impact: Medium · Complexity: Medium-High**

Late static binding is ignored for class constants: `static::CONST`
resolves to the value declared on the *current* class, even though a
subclass may redeclare it. This is unsound in the false-positive
direction, since the narrowed literal can drive a bogus argument-type
mismatch or mark a live `match` arm dead.

```php
class Foo {
    const NO_TYPE = 1;
    /** @var string */
    const TYPE = 'foo';

    public function doFoo(): void {
        self::NO_TYPE;      // 1      (correct)
        static::NO_TYPE;    // 1      (PHPStan: mixed — a child may redeclare it)
        static::TYPE;       // 'foo'  (PHPStan: string — a child must respect @var)
    }
}
```

PHPStan's model: `self::` always yields the declared value. `static::`
(and `$this::`) yields the value only when it cannot be overridden, i.e.
the class is `final` or the constant is `final`/`@final`; otherwise it
widens to the constant's declared type, or `mixed` when the constant is
untyped.

**Where to change:** the class-constant branch of
`resolve_rhs_property_access`, which currently maps `Expression::Static`
to the current class name identically to `Expression::Self_`.

**Tests to update once fixed:** the `static::`/`$this::` assertions in
upstream's `nsrt/class-constant-types.php` were dropped when
`tests/phpstan_nsrt/class-constant-types.php` was ported (only the
`self::` cases survive); port them back.


---

## T40. `pathinfo()` returns a shape or a string depending on the flags argument
**Impact: Low-Medium · Complexity: Medium**

```php
$parts = pathinfo($path);                      // array{dirname: …, basename: …, extension?: …, filename: …}
$name  = pathinfo($path, PATHINFO_FILENAME);   // string
needsString($name); // reported: got string|array{…} — a single component is always a string
```

`pathinfo()` hands back an array of every component when its `$flags`
argument is left at the default `PATHINFO_ALL`, and a plain string when
the caller names one component. phpstorm-stubs declare the flat union, so
both call shapes carry the branch the other one takes.

The mechanism this needs already exists, in the shape used for
`json_encode`'s `JSON_THROW_ON_ERROR`
(`type_engine/types/flag_returns.rs`): read the flags argument's text at
the call site and pick a branch from it. A conditional return type in
`stub_patches.rs` cannot express this one, because the deciding value
arrives as a global constant (`PATHINFO_FILENAME`) and a condition can
only name a literal value or a class constant.

**Fix:** add `pathinfo` to `flag_returns.rs`: the string branch when the
flags argument names one of `PATHINFO_DIRNAME`, `PATHINFO_BASENAME`,
`PATHINFO_EXTENSION` or `PATHINFO_FILENAME` (or an integer with a single
bit set), and the array branch when the argument is left out. Anything
else keeps the declared union. Left over from the work that took the
`preg_replace`/`str_replace` family and `json_encode`, the two shapes
that accounted for the bulk of the volume.

## T41. `@param-out` is parsed but never read
**Impact: Medium · Complexity: Medium**

```php
/**
 * @param-out list<string> $lines
 */
function readInto(string $path, ?array &$lines = null): int { … }

readInto($path, $lines);
$lines[0];   // list<string> is what the tag promises; the declared
             // `?array` is what the caller gets
```

`TagKind::ParamOut` exists in `docblock/tag_kind.rs` and nothing
consumes it, so the only thing describing an argument after a call is
the parameter's declared type. That type describes what goes *in*: a
function that takes `array &$buffer` and fills it with `Token` objects
has no way to say so, and a `?array &$out = null` says null where the
callee guarantees an array.

The null half of that is handled by `ParameterInfo::out_type()`, which
drops the null a by-reference parameter's `null` default implies. That
is a heuristic standing in for the tag: it is right for the standard
library's out-parameters and for the ordinary `&$out = null` idiom, but
it cannot be argued with by a callee that genuinely may leave the
argument null, and it says nothing about the *element* types the callee
writes.

**Fix:** record the tag on `ParameterInfo` alongside
`closure_this_type`, and have `out_type()` prefer it over both the
declared type and the null-default heuristic. Then the two write-back
paths (`seed_pass_by_ref_primitives` and
`try_apply_pass_by_reference_type`) pick it up with no further change,
both going through `effective_out_type` in
`type_engine/call_resolution/out_param.rs`. A declared tag is the
author's word on what the callee writes, so it must also win over the
reading that helper takes from the body.
Once it is read, the pcre stub patches can declare `preg_match`'s and
`preg_match_all`'s out types directly rather than inheriting
phpstorm-stubs' `null|string[]`.
