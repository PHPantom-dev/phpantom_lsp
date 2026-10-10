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
`ClosureFromCallableDynamicReturnTypeExtension` in PHPStan. A
`Closure::fromCallable($cb)` assertion in
`tests/phpstan_data/Analyser/Fiber/fnsr.php` is `// SKIP` on this.

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
emits the concrete callable signature in the `@param` tag, and un-SKIP
the `Closure(): 1`-style assertions in
`tests/phpstan_data/Analyser/Fiber/fnsr.php` and the closure calls in
`tests/phpstan_data/Rules/Functions/bug-anonymous-function-method-constant.php`
(a closure without a declared return type, whose call reads back as
`mixed`).

---

## T20. Type narrowing reconciliation engine
**Impact: Medium-High · Complexity: Very High**

PHPantom's type narrowing in `type_engine/types/narrowing/` handles
basic patterns (instanceof, is_* calls, null checks) but lacks the
algebraic framework that PHPStan and Psalm use. Key gaps:

1. No separate tracking of "sure types" vs "sure-not types". When
   `$x !== null`, PHPantom should remove `null` from the union
   (sure-not) rather than trying to intersect with "not-null".
2. No clause algebra. `&&`, `||` and `!` compose what their operands
   prove (`forward_walk/condition.rs`), but a `||` that held leaves the
   join of its operands' scopes, which cannot say how they relate:
   inside `if ($a instanceof Foo || $b instanceof Bar)`, a later
   `!$a instanceof Foo` should prove `$b` is a `Bar`, and does not.
3. No truthy/falsey distinction. `if ($x)` (truthy) vs
   `if ($x === true)` (strict true) should produce different
   narrowings. PHPStan uses a 4-state bitmask context.

`@phpstan-assert`/`@psalm-assert` annotations on called functions are
now applied as narrowings at call sites (`extract_call_assertions` and
`CallAssertionInfo` in `type_engine/types/narrowing/assertions.rs`,
consulted from `type_engine/variable/forward_walk/cond_narrowing/`), so
that gap from the original list is closed. It is still ad hoc rather than
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

## T48. A sub-expression's type is resolved again by every consumer
**Impact: Low-Medium · Complexity: High**

The forward walker threads the scope through each expression in
evaluation order (`forward_walk/expr.rs`) and records the scope each node
ran on, but not the node's type. The expression's own type is resolved
once, by the caller, against the scope after the whole expression
(`resolve_rhs_with_scope`). The sub-expressions that read an earlier scope
(evaluated before a later write, or the old value `$d++` yields) are typed
where they ran and handed back by span in `ExprTypes`, which only
`resolve_rhs_expression` and array-item inference consult. Diagnostics,
hover and completion resolve the nodes they look at again, each against
the scope recorded there, and `CHAIN_CACHE` exists to keep re-resolving a
chain affordable.

PHPStan 2.3 keeps each processed node's `ExpressionResult`, whose type is
computed lazily, and `Scope::getType()` on a node already processed reads
it back. It recomputes only for synthetic nodes, or under a scope that
disagrees on a variable the node reads.

**Design.** Give each node `ExprWalk` evaluates a lazily computed type
against the scope it ran on, stored by span next to the node scopes the
diagnostic walk records. `ExprTypes` and its pending/flush bookkeeping
then go away, `resolve_rhs_with_scope` reads the stored type, and a
diagnostic reads the type of the node it checks instead of resolving it
again. Measure first: the win is the resolutions it saves, and a stored
type per node costs memory on every file the diagnostic pass walks.

**Where to look:** PHPStan 2.3's `src/Analyser/ExpressionResult.php` and
`MutatingScope::resolveTypeOfNewWorldHandlerNode`. Ours:
`forward_walk/expr.rs` (`ExprTypes`, `ExprWalk`),
`forward_walk/diagnostic_cache.rs` (`record_node_scope`),
`rhs_resolution/mod.rs`, and `CHAIN_CACHE` in `resolver/context.rs`.

---

## T26. Class constants named as docblock types (`Foo::BAR`, `Foo::BAR_*`)

**Impact: Low-Medium · Complexity: Medium**

A class constant named in a type position is not read at all: `@param
Foo::BAR $x`, `@param self::FOO|self::BAR $x` and `@param C::class|D::class
$x` all stay as unresolved text (or fall back to the native hint), where
PHPStan and Psalm give the constant's value (`'bar'`, `'bar'|'foo'`,
`'App\C'|'App\D'`). The same constants are read when they are the operand
of `key-of<>`/`value-of<>`, through `constant_operand_shape`, so the value
lookup exists; the bare reference just never reaches it. Found porting
Psalm's `ConstValuesTest` and `ReconcilerTest`; the assertions are `// SKIP`
in `tests/psalm_assertions/const_values.php` and
`tests/psalm_assertions/type_reconciliation_reconciler.php`.

The wildcard form resolves a pattern like `Foo::BAR_*` to the union of all
matching constant types on the class. PHPStan supports this syntax in
docblock type strings:

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

Knowing that a target is definitely set also settles `??=`: on a target
that is set and never null (a non-nullable parameter, a required shape
key) the fallback can never be assigned, so `$string ??= 1` is still
`string`. Today the fallback is always added; the assertions in
`tests/phpstan_nsrt/coalesce-assign.php` are `// SKIP` against this item.

**References:**
- Psalm: `Context::$vars_in_scope` and `Context::$vars_possibly_in_scope`
  (`Psalm\Context`)

---

## T46. Template arguments are not inferred from how the object is used later
**Impact: Medium · Complexity: High**

```php
/** @template T */
class Box {
    /** @param T $item */
    public function add($item): void {}
    /** @return T */
    public function first() {}
}

$box = new Box();
$box->add(new Foo);
$box->first()->  // PHPStan 2.3: Foo; PHPantom: mixed (`Box<mixed>`)
```

A `new` whose constructor leaves a template open erases it to its default,
then its bound, then `mixed` (`default_type_arg` in
`inheritance/generics.rs`, reached from `instantiate_class` in
`call_resolution/template_binding.rs`), and members are substituted with
that immediately. PHPStan 2.3 keeps the template open and fills it from
the rest of the body. It ships behind the `unresolvedTemplateArguments`
bleeding-edge flag, so it is not yet on by default there either.

**How PHPStan does it.** The open template becomes a placeholder type
(`UnresolvedTemplateArgumentType`) that answers every type question as its
default or bound, so nothing downstream needs to know about it. While the
body is walked, facts about each placeholder accumulate:

- a value flows into it (`add(T $v)` called with `Foo`, `offsetSet` on an
  `ArrayAccess`): a lower bound;
- the object is passed to, returned as, or assigned to something declared
  `Box<int>`: a send, with that position's variance;
- the object is passed to `mixed` or an untyped slot: it escaped, and the
  placeholder falls back to default or bound.

Lower bounds are unioned (each first checked against the template's bound,
so `add('x')` on `T of object` leaves `Box<object>`). An invariant send
wins over lower bounds. Invariant sends between two placeholders unify
them, so `$a->add(1); $b->add('x'); join($a, $b);` gives both
`Box<1|'x'>`.

The body is then walked a second time with the solved arguments. That is
where the 1-3% cost comes from: statements before the first `new` replay
from a recording, and a later statement is re-walked only if it holds a
site or mentions a variable whose type changed (a syntactic check, cached
on the AST node). Everything else replays its recorded scope delta. The
second walk stops early once no variable differs and no site remains.
(`StatementsHandler::processBodyStmtNodesTwoPass`,
`Analyser/Generics/TemplateArgumentSolver.php`.)

**What this means for an LSP.** In PHPStan an earlier line's type can
depend on a later line: `$box->first()` hovered on the line before
`takeInts($box)` reads `int`. Our cursor-bounded walk stops at the cursor's
statement, so hover would read `mixed` there and `int` below, while
diagnostics (which walk the whole body) would disagree with hover. The
facts therefore have to come from a cursor-free pass over the whole body
(`ctx.with_cursor_offset(u32::MAX)`, as `by_ref.rs` and `loops.rs` already
do for discovery), cached per body and content version so hover and
completion don't pay for a full walk per request. The solved arguments
seed the cursor walk before any snapshot is recorded.

Inferring from code below the cursor means half-typed code can flip a type
while the user is typing. Two rules keep that stable: union lower bounds
instead of PHPStan's "first invariant send wins" (which depends on order),
and never narrow below default or bound from a lone escape.

**First slice.** `new` with no arguments that bind the template, assigned
to a plain variable; lower bounds from template-typed method parameters
and `offsetSet`; union merging; no cross-variable unification. That covers
the common "empty collection, then fill it" shape. Returns, property
writes and sends to typed parameters can follow.

**Shared work with T47.** Both need the same machinery: a cursor-free fact
pass over the body, a per-body cache, and a "does this statement mention
one of these variables" filter to limit the re-walk. `assignment_deps.rs`
already collects the variables a statement reads and writes
(`collect_rhs_variables`, `collect_assignment_target_vars`) and has a
convergence check (`scope_has_changes`); `seed_by_ref_capture_fixed_point`
in `by_ref.rs` is the closest existing probe-then-reseed pattern. Build it
once, for whichever of the two lands first.

**Where to look:** PHPStan 2.3's `src/Type/Generic/UnresolvedTemplateArgumentType.php`,
`src/Analyser/Generics/`, `NewHandler::unresolvedArgumentList()`, and the
`tests/PHPStan/Analyser/nsrt/template-argument-*.php` files. Ours:
`rhs_resolution/instantiation.rs`, `call_resolution/template_binding.rs`,
`forward_walk/mod.rs` (`resolve_in_method_body`, next to the existing
whole-body pre-passes `seed_static_locals` and
`try_generator_yield_inference`), `forward_walk/assignment.rs`
(`process_expression_statement`, where receiver calls are already visited).

---

## T47. A closure assigned to a variable gets no parameter types from where it is used
**Impact: Medium · Complexity: High**

```php
$byName = function ($a, $b) {
    return $a->  // PHPStan 2.3: User; PHPantom: nothing
};
usort($users, $byName);   // list<User>
```

A closure literal passed straight to a typed callable parameter already
gets its parameter types, generics included (`try_enter_closure_expr` →
`infer_callable_params_for_call` in `forward_walk/closures.rs` and
`callable_inference.rs`). Assigned to a variable first, it gets nothing:
the assignment branch passes no inferred types, and the cursor walk stops
before the statements that use it. This is the same blind spot from the
other direction as T46, and the payoff is the same: completion and hover
on an untyped parameter inside the body. PHPStan 2.3 ships it behind the
`closureSignaturesFromUsages` bleeding-edge flag.

**What PHPStan infers from.** Direct calls (`$f(1); $f(2);` gives `1|2`),
passing the variable to a typed or generic callable parameter (`usort`,
`array_map`, `array_filter`, `$collection->map($f)`, taking the parameter
type after template resolution), aliases and ternaries (`$d = $f; $d(1);`),
closures stored in array offsets or lists and called from there, and
`return $f` / a typed property / `@var` against a declared `Closure(int):
void`. All observed argument types are unioned. If the closure escapes
(passed to `mixed`, bare `callable` or `Closure`, `call_user_func`, a
by-ref parameter, `global`, `compact`/`extract`/`get_defined_vars`, `$$`),
or the union doesn't fit the declared type, the declared type stands. A
body is only observed when nothing outside it can reach its variables
(`isClosedBody`).

The mechanism is the two-pass walk described in T46: an untyped parameter
starts as a placeholder that remembers its closure, and since it is an
ordinary type it flows through variables, array shapes and foreach without
special cases. Call sites add lower bounds, typed targets add bounds, and
the second walk re-enters the closure body with the solved types.
(`ClosureSignatureInference`, `TemplateArgumentObserver`,
`TemplateArgumentSolver::resolveClosureSignatureObservation`; tests in
`nsrt/closure-signature-from-usages*.php`.)

**Plan.**
1. When `try_enter_closure_expr` meets `$v = <closure>` (or an array
   offset or list element) with the cursor inside it and untyped
   parameters, scan the rest of the enclosing statement list for uses of
   `$v` and its aliases. Run the syntactic escape check first and give up
   on any hit.
2. For a direct call `$v(...)`, resolve the arguments at that position.
   For `$v` passed as an argument, reuse `infer_callable_params_for_call`
   on that call to get the callable parameter types, so `usort`,
   `array_map` and `->map()` come for free.
3. Union the observations, keep the declared hint unless the union fits
   inside it, and feed the result through the existing `inferred_params`
   slot of `try_enter_closure_body` / `seed_closure_params`. Run the same
   inference from `walk_closures_in_expr` so diagnostics and completion
   agree.
4. Later: give the variable's own type the inferred signature
   (`infer_closure_literal_type`), so `$f('x')` and `->map($f)` see it.
   That overlaps T13.

Resolving each use's arguments must go through the shared walker (a
cursor-free walk up to that offset, per T46's shared machinery), not a
separate resolver. Lower-value cases can wait: `use (&$x)` fixed points,
nested closures typed from an expected return type, generator escapes.

**Where to look:** ours: `forward_walk/closures.rs`,
`forward_walk/callable_inference.rs`, `diagnostic_walk.rs`
(`walk_closures_in_call`, `seed_closure_params`), `rhs_resolution/calls.rs`
(`$f()` reads only the variable's callable return type today).

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
`tests/phpstan_nsrt/deducted-types.php` was ported; port it back. The
`$hw::B`/`$self::FOO` assertions in
`tests/phpstan_data/Rules/Constants/bug-10212.php` and
`tests/phpstan_data/Rules/Methods/return-type-class-constant.php` are
`// SKIP` on this.

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
`self::` cases survive); port them back. The `static::`/`$this::`
assertions in `tests/phpstan_nsrt/class-constant-native-type.php` are
`// SKIP` against this item.


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
`stub_patches/` cannot express this one, because the deciding value
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

## T42. A static factory's own `@return self<T>` breaks every chained call after it
**Impact: Medium · Complexity: Unknown (needs investigation)**

Found while porting Mago's `issue_362.php` (Test Porting Phase 4A): a
static method that declares its own `@template T` and returns
`self<T>` makes every completion request on a call chained off it come
back `None` (not empty — no completion response at all), even when the
receiving variable's type is separately pinned down by a `@var`
annotation that makes the method's own template irrelevant.

Minimal repro:

```php
class Product {
    public function getPrice(): float { return 0.0; }
}

/** @template T */
class Box {
    /**
     * @template T
     * @return self<T>
     */
    public static function make(): self { return new self(); }

    /** @return T */
    public function get(): mixed {}
}

/** @var Box<Product> $box */
$box = Box::make();
$box->get()->  // completion() returns None here, not just an empty list
```

Removing either the method-level `@template T` + `@return self<T>` on
`make()` (letting it return a bare `self` and relying entirely on the
`@var` hint), or making `get()`'s call chain off a plain `new Box()`
instead of `Box::make()`, makes the same chain resolve `getPrice`
correctly. So the break is specific to a `self<T>`-returning static
method sitting in the middle of a chain — one hop after it (`Box::make()
->`) still lists `Box`'s own members fine; it is the *second* hop
(`->get()->`, the one that needs `T` substituted from the instantiated
type) that comes back empty-handed.

The same failure reproduces with the original Mago pattern too: two
independent method-level templates (`K`, `V`, both distinct from the
class-level `TKey`/`TValue`) inferred from an `array<K, V>` argument and
returned as `self<K, V>`, chained straight into an instance method
whose own `@return TValue` should resolve through it. So this is not
purely a same-name shadowing edge case between the class-level and
method-level template — investigate whether both share one underlying
cause before assuming they are separate bugs.

Whatever produces the instantiated receiver type for the second hop
comes back with nothing usable — worth checking whether the generic
substitution map built for `self<T>` ends up unable to resolve `T` (no
argument binds the method-level template, and the external `@var`
override does not appear to reach it), and whether that unresolved-`T`
case is handled by returning an error/`None` instead of falling back to
the class's own declared (or unsubstituted) member types the way other
partial-resolution failures in this pipeline do.

**Blocks:** Test Porting Phase 4A pattern 362 (generic class implementing
`ArrayAccess`/`IteratorAggregate` with a `self<K, V>`-returning static
factory) cannot be ported until this is fixed — the ported test would
assert on exactly the completion this bug empties out.

## T43. `self::TypeAlias` inside `@extends`'s generic argument is not resolved
**Impact: Low · Complexity: Medium**

Found while porting Mago's `issue_870.php` (Test Porting Phase 4A). A
class can declare its own `@type`/`@phpstan-type` alias and hand it to
its own parent's template parameter via `self::`:

```php
/** @template TData as array<string, mixed> */
abstract class Car {
    /** @return TData */
    abstract function getData(): array;
}

/**
 * @type DataArray = array{'Gewicht': int}
 * @extends Car<self::DataArray>
 */
class RedCar extends Car {
    public function getData(): array { return ['Gewicht' => 1000]; }
}
```

`getData()`'s inherited `@return TData` should resolve to the
`DataArray` shape (offering `Gewicht` for array-key completion), but
`self::DataArray` inside the `@extends` tag's generic argument list is
not recognised as a reference to the class's own type alias, so `TData`
never gets substituted and completion sees only the untyped `array`
declared return.

`extract_generics_tag` (`docblock.rs`, exercised by
`test_extract_generics_tag_extends_single_param` and friends in
`completion_generics.rs`) parses `@extends Foo<Bar>` generic arguments
as plain type tokens; `self::DataArray` most likely parses as some
`Named`/member-access token rather than being recognised as "look up
`DataArray` in this class's own `type_aliases`, the same alias
`resolve_type_alias_typed` already expands everywhere else."

**Fix:** when parsing a generic argument off `@extends`/`@implements`/
`@template-implements`, recognise a `self::Identifier` token and resolve
it against the *declaring* class's own `type_aliases` map (via
`resolve_type_alias_typed` with that class as `owning_class_name`)
before substituting it into the parent's template.

**Blocks:** Test Porting Phase 4A pattern 870.

---

## T44. A single enum case has no type
**Impact: Low-Medium · Complexity: High**

```php
enum Suit { case Hearts; case Spades; case Clubs; }
function f(Suit $s) {
    if ($s === Suit::Hearts) {
    } elseif ($s === Suit::Spades) {
    } else {
        $s; // PHPStan: Suit::Clubs; PHPantom: Suit
    }
}
```

`Suit::Hearts` resolves to the enum class, so narrowing by comparing
against cases can only ever say "some `Suit`". PHPStan and Psalm give each
case its own type, a subtype of the enum: comparing narrows to the cases
left, a `match` over the rest is known exhaustive, a `readonly` property
assigned one case keeps it, and `->value`/`->name` on it are the case's
literal values.

The type model needs an enum-case variant (or a literal kind for it) that
is a subtype of its enum in `is_subtype_of`, prints as `Enum::CASE`, and
joins back into the enum when every case is present. Narrowing by `===`
and `instanceof` then subtracts cases the way it subtracts union members,
and a branch that has compared away every case holds `never`.

Since `value-of<…>` over an enum is evaluated, the gap also produces false
positives. A case passed to a template binds the template to the whole
enum, so the operator gives every case's value:

```php
/**
 * @template T of Suit
 * @param T $case
 * @return value-of<T>
 */
function backingValue(Suit $case) { return $case->value; }

/** @param 'hearts' $value */
function acceptsHearts(string $value): void {}

acceptsHearts(backingValue(Suit::Hearts)); // reported: 'spades' does not satisfy 'hearts'
```

A case held in a variable loses its value the same way: after `$case =
Suit::Hearts;`, `acceptsHearts($case->value)` is reported too. Found
running the php-typing-conformance suite
(`phpdoc_advanced_fallback_value_of_template_enum.php`).

Found porting PHPStan's `Rules/Comparison/data/bug-8485.php` and `Rules/Methods/data/return-type-class-constant.php`; both its
case and its `never` assertion are `// SKIP` in the ported copy under
`tests/phpstan_data/`.

Found porting PHPStan's `Rules/Comparison/data/bug-8485.php`; the
assertion is `// SKIP` in `tests/phpstan_data/`.
