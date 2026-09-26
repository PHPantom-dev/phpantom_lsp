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

### B379. The `try` body's variables are missing in `catch`, and a `catch` variable is not merged after
**Impact: Medium · Complexity: Medium**

```php
$ex = null;
try {
    $x = 1;
    maybeThrows();
} catch (\RuntimeException $ex) {
    $x; // should be 1 (maybe defined), has no type
}
$ex; // should be RuntimeException|null, is null
```

A `catch` block starts from the scope before the `try`, so assignments the
body made before throwing are invisible there, and the exception variable a
non-exiting `catch` binds is not joined into the scope after the statement.

Found porting PHPStan's `nsrt/if.php`, which is not ported yet because
it is too slow under the runner (see
[P65](performance.md#p65-every-call-site-repeats-the-full-function-lookup-hit-or-miss)).

### B380. A loop that must run, or a `switch` that cannot fall out, still joins the path that skips it
**Impact: Low · Complexity: Medium**

```php
$v = null;
for ($i = 0; $i < 1; $i++) { $v = 1; }
$v; // should be 1, is 1|null

$w = null;
switch (doFoo()) {
    case 1: $w = 1; break;
    default: throw new \Exception();
}
$w; // should be 1, is 1|null
```

A `for` whose condition is provably true on entry always runs its body, and a
`switch` whose `default` throws cannot be skipped, but the walker still joins
the pre-statement scope into the one after.

Found porting PHPStan's `nsrt/if.php`, which is not ported yet because
it is too slow under the runner (see
[P65](performance.md#p65-every-call-site-repeats-the-full-function-lookup-hit-or-miss)).

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

### B429. A `class-string` key is widened to `string` when an array is written through it
**Impact: Low-Medium · Complexity: Low**

```php
/** @param class-string $n */
function f(string $n): array {
    $mapping = [];
    $mapping[$n] = 'y'; // should be non-empty-array<class-string, 'y'>, is non-empty-array<string, string>
    return $mapping;    // so a declared array<class-string, string> is reported
}
```

`normalize_array_key_type` maps every non-numeric string domain
(`class-string`, `interface-string`, and the rest) to plain `string`.
PHP never coerces those keys, so the refined type is a valid key as it
stands, and PHPStan keeps it. Erasing it makes every
`array<class-string, …>` return that is built by writing keys reject its
own value. The unit test
`collection_key_normalization_preserves_non_numeric_string_domains` in
`array_shape_writes_tests.rs` pins the current `string` answer, so
changing this means deciding against that test.

Seen in phpstan-src (`src/DependencyInjection/ValidateServiceTagsExtension.php`,
the `$mapping[$class->name] = …` loop in `getInterfaceTagMapping()`).

**Where to look:** `normalize_array_key_type` in
`type_engine/variable/array_shape_writes.rs`.

### B430. `isset()` on a constant shape read with a dynamic key loses the element type
**Impact: Low-Medium · Complexity: Low-Medium**

```php
$g = [$a];
$g[] = $b;                         // array{P, P}
if (!isset($g[$k])) { return; }    // $k is int
$g[$k]->id;                        // $g[$k] should be P, is mixed
```

Without the `isset()` guard, `$g[$k]` is `P`, and the same guard over a
`list<P>` keeps `P` too. Only a constant shape guarded through a
non-literal key comes out `mixed`, which then reports
`Cannot verify property 'id'` on the read. Seen in a Laravel feature
test that indexes a two-element list of models with the entries of a
data-provider array.

**Where to look:** the `isset`/`!isset` arm of the null narrowing that
records the synthetic `$g[$k]` key
(`type_engine/variable/forward_walk/cond_narrowing/null_narrowing.rs`),
and how the shape's element type is looked up for a key that is not a
literal.

### B431. A class constant array keyed by `Foo::class` is a bare `array`
**Impact: Low-Medium · Complexity: Low-Medium**

```php
class C {
    private const B = [\stdClass::class => 'X'];
    private const A = ['k' => 'X'];
    public function f(): void {
        self::B; // should be array{stdClass: 'X'}, is array
        self::A; // array{k: 'X'}, as expected
    }
}
```

An initialiser whose keys are `::class` constants is not inferred at
all, so the constant loses both its keys and its values. A literal
string key works. Seen in phpstan-src
(`build/PHPStan/Build/TurboAttributeCollector.php`, `VENDORED_PAIRS`),
where the bare `array` survives as the `array|` at the front of the
inferred return type.

**Where to look:** the constant-initialiser inference behind
`infer_type_from_constant_value_resolved` and
`folded_class_constant_type` (`rhs_resolution/property_access.rs`
calls both), for array keys that are class-constant accesses.

### B432. A write through a dynamic key into a nested offset turns each shape entry into a generic array
**Impact: Low-Medium · Complexity: Medium**

```php
/** @var array<string, array{string, bool, string}> $pairs */
foreach (array_keys($pairs) as $cn) {
    $pairs[$cn][3] = ['I'];
}
// should be array<string, array{string, bool, string, array{'I'}}>
// is non-empty-array<string, array{…}|non-empty-array<int, string|bool|array{'I'}>>
```

A write to a literal offset below a dynamic key should add that offset to
the element shape it reaches. Instead the element is joined with a
generic `non-empty-array<int, …>` holding every value, so the shape is
lost and any declared element shape rejects it. Seen in phpstan-src
(`build/PHPStan/Build/TurboAttributeCollector.php`, the
`$pairs[$className][3] = $reflection->getInterfaceNames()` loop), whose
return type is reported against its own `@return` array shape. That
report also carries
[B431](#b431-a-class-constant-array-keyed-by-fooclass-is-a-bare-array).

**Where to look:** `merge_nested_array_write` in
`type_engine/variable/array_shape_writes.rs`, for an `ArrayWriteKey::Keyed`
level followed by an `ArrayWriteKey::Shape` one.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Miscellaneous

No outstanding items.
