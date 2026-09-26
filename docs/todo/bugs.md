# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

Each entry below carries an **Impact · Complexity** rating using the same
scale defined in [`docs/todo.md`](../todo.md); that table is also where
each bug's row lives in the current sprint/backlog.

Bugs land here from wherever they surface: found while working on another
task, or sweeps of the sample projects under `projects/`. Entries are
grouped by the mechanism that has to change, not by the symptom that
surfaced: one entry is one root cause, however many shapes it shows up in.

## Crashes

No outstanding items.

## Type comparison

No outstanding items.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

### B424. A closure invalidates receiver state only when it is a literal at the call site, not when held in a variable
**Impact: Low · Complexity: Medium**

```php
while ($this->running) {
    $cb = function () { $this->stop(); };
    call_user_func($cb);
    $this->running;          // should be bool, is true
}
```

The walker invalidates `$this` and `use (…)` captures for a closure
written inline as the call's own argument, or invoked immediately where
it is defined. A closure assigned to a variable first and passed by that
variable is not recognised: nothing records that `$cb` still names that
literal at the call site, so the call is walked as if the callee were
opaque.

Found porting PHPStan's `nsrt/bug-10566.php`; two of its cases stay
`// SKIP` for this reason.

**Where to look:** `type_engine/variable/forward_walk/receiver_mutation.rs`
(`collect_closure_invalidations`), and whatever tracks the closure
literal a plain assignment (`$cb = function () {...};`) stores, if
anything currently does. Invoking such a variable already resolves
through the callable type the assignment records (`Closure(): T`), but
that type does not carry the literal's body, which is what the
invalidation needs.

## Arithmetic

No outstanding items.

## Symbol resolution

### B407. A readonly property is not narrowed to what the constructor assigns
**Impact: Low · Complexity: Medium**

```php
class Foo {
    private readonly int|float $i;
    public function __construct() { $this->i = getInt(); }
    public function f() {
        $this->i; // should be int, is int|float
    }
}
```

A readonly property can only be written once, so what the constructor
leaves in it is what every other method reads. PHPStan remembers the
constructor's final scope and applies its readonly properties to the other
methods of the class, keeping the declared type where the constructor's is
not narrower (`?int` assigned to an `int` property stays `int`).

Doing the same here means walking the constructor when another method's
scope is seeded, which neither walker entry point
(`resolve_in_method_body` for hover and completion,
`seed_and_walk_function_body` for diagnostics) can do today: both are
handed one method's parameters and statements, not its siblings. It also
needs a keyed guard, since walking the constructor can resolve a
`$this->method()` whose body seeds `$this` again.

Found porting PHPStan's
`nsrt/remember-non-nullable-property-non-strict.php`; the assertion is
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

**Where to look:** `seed_this` in
`type_engine/variable/forward_walk/callable_inference.rs`, and the two
entry points above.

### B434. An assignment inside an arrow function body is not seen
**Impact: Low · Complexity: Low-Medium**

```php
$f = fn () => $x = $this->make();
// hovering `$x` inside the arrow body finds no assignment
```

The forward walker enters an arrow function by seeding its parameters and
never applies the assignments its body expression makes, so a variable
assigned there has no type at the cursor. A statement body is walked
statement by statement up to the cursor; an arrow body needs the same for
the sub-expressions that run before it.

Found porting PHPStan's `nsrt/closure-types.php`, whose `assertType()`
inside an arrow function passed to `->call()` becomes such an assignment
under the runner; that line is `// SKIP` in the ported copy under
`tests/phpstan_nsrt/`.

**Where to look:** the `Expression::ArrowFunction` arm of
`try_enter_closure_expr` in `type_engine/variable/forward_walk/closures.rs`.

### B427. The forward walker ignores `@param-closure-this`
**Impact: Low-Medium · Complexity: Medium**

```php
class Reg {
    /** @param-closure-this Target $cb */
    public static function on(\Closure $cb): void {}
}
Reg::on(function () {
    $t = $this; // should be Target, is the enclosing class
});
```

Completion and hover on `$this->` itself honour the tag, through
`find_closure_this_types` in `type_engine/variable/closure_resolution.rs`,
but the walker seeds every closure scope with the enclosing `$this`
(`seed_closure_captures`), so anything that reads `$this` through the
scope, a variable assigned from it or a diagnostic on it, sees the
lexical class. `Closure::call()` already rebinds the walker's scope
(`closure_call_scope`); a closure passed to a parameter carrying the tag
needs the same, from the call-argument arms of both walker entry points.

**Where to look:** `try_enter_closure_expr` in
`type_engine/variable/forward_walk/closures.rs` and
`walk_closures_in_call` in `type_engine/variable/forward_walk/diagnostic_walk.rs`.

## Array types

### B436. `array_keys()`'s key type widens to `int|string` only inside a diagnostic-scope walk
**Impact: Low-Medium · Complexity: Medium**

```php
/** @param array<string, array{string, bool, string}> $pairs */
function add_flag(array $pairs): array {
    foreach (array_keys($pairs) as $cn) {
        $pairs[$cn][3] = ['I'];
    }
    return $pairs;
}
```

Hovering `$pairs` right after the loop reports the precise
`non-empty-array<string, array{string, bool, string, array{'I'}}>`, but
the return-type diagnostic for the same function sees
`non-empty-array<string|int, array{string, bool, string, array{'I'}}>`
for the identical write and flags it as incompatible with the declared
`@return array<string, array{string, bool, string, array{'I'}}>`. The
only difference between the two runs is `is_diagnostic_scope_active()`,
so `$cn`'s key type — correctly narrowed to `string` outside
diagnostics — falls back to the benevolent `int|string` array-key
default during a diagnostic-scope walk.

**Where to look:** `resolve_foreach_iterable_type_raw` and
`bind_foreach_value` in `type_engine/variable/forward_walk/foreach.rs`,
and whether the diagnostic-scope cache
(`type_engine/variable/forward_walk/diagnostic_cache.rs`) or the chain
resolution cache (`type_engine/resolver/context.rs`) returns a less
precise resolution for `array_keys($pairs)` during a diagnostic-scope
walk than the same expression gets outside one.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Miscellaneous

No outstanding items.
