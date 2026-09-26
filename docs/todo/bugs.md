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

### B424. An array's type arguments are checked with scalar coercion in a file without `strict_types`
**Impact: Low-Medium · Complexity: Low-Medium**

```php
/** @return array<string, string> */
function f(int $k): array {
    $r = [];
    $r[$k] = 'a';
    return $r; // should be reported, is not; is with declare(strict_types=1)
}
```

PHP converts a scalar handed to a parameter in a coercive-mode file, but
never the keys or values inside an array passed whole, so `array<int, V>`
is no more an `array<string, V>` in a lenient file than in a strict one.
The same-base generic rule in `is_type_compatible` already compares class
type arguments strictly for this reason; array-likes still pass the file's
`strict_types` through, so `int` satisfies a `string` key (and value) in
a lenient file. A `list<string>` for a `list<int>` is caught either way,
since `string` does not coerce to `int`.

**Where to look:** `args_strict` in the same-base generic rule in
`src/diagnostics/type_errors/compatibility.rs`. Expect new diagnostics on
lenient projects, so check them against the `projects/` corpus.

### B425. A declared generic type that omits a defaulted argument does not spell it out
**Impact: Low · Complexity: Medium**

```php
/** @template T1 = true  @template T2 = true */
class Test {}
/** @param Test<false> $one */
function f(Test $one) {} // $one should be Test<false, true>, is Test<false>
```

Members already see the default (`build_generic_subs` fills it in), but
the type itself keeps only the arguments written, so hover shows
`Test<false>` and a comparison against `Test<false, true>` sees two
different arities. PHPStan fills omitted arguments with their defaults
when it resolves the type. The fill needs the class loader, so it belongs
where every declared type (parameter, `@var`, return, property) is
resolved, not in one consumer.

Found porting PHPStan's `nsrt/template-default.php`; the assertion is
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

## Standard-library return types

No outstanding items.

## Reachability

### B417. An argument in a branch a `!== null` test rules out is still checked
**Impact: Low-Medium · Complexity: Low-Medium**

```php
function takesNode(Node $n): void {}
function f(?Node $x): void {
    if ($x instanceof Node) { return; }
    if ($x !== null) { takesNode($x); } // reported: expects Node, got null
}
```

With `$x` down to `null`, `strip_null_from_scope` marks the branch
unreachable, but the argument check inside it still resolves `$x` as
`null` and reports it. A branch nothing can enter should either resolve
its variables to `never` or not be checked at all. `get_class()` on a
property narrowed the same way reports `expects object, got null`, which
is how it turned up in phpstan-src (`src/PhpDoc/TypeNodeResolver.php`).

**Where to look:** `strip_null_from_scope` in
`type_engine/variable/forward_walk/cond_narrowing/scope_edits.rs` and how
the argument diagnostics treat an unreachable scope.

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
anything currently does. Related to
[B405](#b405-a-closure-called-through--call-or-a-callable-held-in-a-variable-is-not-resolved),
which needs the same "what closure does this variable hold" link for a
different purpose (its return type).

## Arithmetic

No outstanding items.

## Symbol resolution

### B418. `Foo::class` of a class in the file's own namespace can stay unqualified
**Impact: Medium · Complexity: Medium**

```php
namespace Acme\Model\Events;

use Illuminate\Database\Eloquent\Model;

final class EventSubcategory extends Model {
    public function event(): BelongsTo {
        // reported: expects class-string<Model>, got class-string<Event>
        return $this->belongsTo(Event::class, 'event_id');
    }
}
```

`Event` is `Acme\Model\Events\Event`, a model in the same directory and
namespace, and no `use` names it. In a full-project `analyze` of a
Laravel app the argument resolves to the unqualified `class-string<Event>`,
which means the `::class` arm in `rhs_resolution/property_access.rs`
found no class and fell back to the spelling. The bare `Event` then loads
as Laravel's global `Event` facade alias, which is not a `Model`.
Analysing only that directory resolves it correctly, so it depends on
what else the run has loaded first. No standalone reproduction yet.

**Where to look:** the `::class` arm of `Access::ClassConstant` in
`type_engine/variable/rhs_resolution/property_access.rs`, and the
namespace-relative lookup `type_hint_to_classes_typed` does against the
facade alias fallback in `resolution.rs`.

### B403. `new` of a class that cannot be loaded has no type
**Impact: Medium · Complexity: Low-Medium**

```php
$x = new UndeclaredFoo(); // should be UndeclaredFoo, has no type
$arr = [];
$arr[] = new UndeclaredFoo(); // should gain an UndeclaredFoo entry, stays array{}
```

`new` resolves to the classes the name loads, and an unloadable name gives an
empty list, so hover is empty and appends of the value are skipped. The
named type is known regardless and should be kept without its `ClassInfo`.

Found porting PHPStan's `nsrt/if.php`, which is not ported yet because
it is too slow under the runner (see
[P65](performance.md#p65-every-call-site-repeats-the-full-function-lookup-hit-or-miss)).

**Where to look:** `type_engine/variable/rhs_resolution/instantiation.rs`.

### B404. `$this` in a `@phpstan-require-extends` trait does not see the required class's members
**Impact: Low-Medium · Complexity: Medium**

```php
/** @phpstan-require-extends SomeClass */
trait T {
    function f(): void {
        $this->x; // should be int, has no type
    }
}
class SomeClass { public int $x = 1; }
```

The static-access half of this is
[C13](completion.md#c13-selfstatic-inside-a-require-extends-trait-does-not-see-the-required-classs-static-members).
In PHPStan's fixture the required class is declared after the trait.

Found porting PHPStan's `nsrt/bug-10302-trait-extends.php`; the
assertions are `// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

### B405. A closure called through `->call()`, or a callable held in a variable, is not resolved
**Impact: Low · Complexity: Medium**

```php
class Foo {
    public function doFoo(): float { return 1.0; }
    public function f(\stdClass $o): void {
        (fn () => $this)->call($o); // should be stdClass, is Foo
        $c = function (): string {};
        $c();                        // should be string, is mixed
        $cb = [$this, 'doFoo'];
        $cb();                       // should be float, has no type
    }
}
```

`Closure::call()` rebinds `$this` to its argument, and invoking a variable
that holds a closure or an array callable resolves to its target's return.

Found porting PHPStan's `nsrt/callables.php`, `nsrt/closure-types.php`;
the assertions are `// SKIP` in the ported copies under
`tests/phpstan_nsrt/`.

### B406. Closure variadics are lists or untyped, and a `null` default does not always make a parameter nullable
**Impact: Low · Complexity: Low**

```php
$c = function (string $s, string ...$y) {}; // $y should be array<int|string, string>, is list<string>
$d = function (...$arr) {};                 // $arr should be array<int|string, mixed>, has no type
$e = function (bool $a = null) {};          // $a should be bool|null, is bool
function g(bool $a = Null) {}               // $a should be bool|null, is bool
```

Named arguments put string keys in a variadic, which a named function's
variadic already reflects. A `null` default makes a typed parameter nullable;
a closure never gets that, and a named function misses it when the default is
spelled in another casing (`Null`, `NULL`).

Found porting PHPStan's `nsrt/anonymous-function.php`,
`nsrt/bug-2600-php8.php`, `nsrt/typehints-anonymous-function.php`; the
assertions are `// SKIP` in the ported copies under
`tests/phpstan_nsrt/`.

### B407. A property inferred from the constructor drops `[]` and does not narrow a readonly union
**Impact: Low · Complexity: Low-Medium**

```php
class Foo {
    private $items;
    private readonly int|float $i;
    public function __construct() { $this->items = []; $this->i = getInt(); }
    public function f() {
        $this->items; // should be array, has no type
        $this->i;     // should be int, is int|float
    }
}
```

The empty-array literal is dropped when inferring an untyped property, and a
readonly property assigned once in the constructor could take the assigned
type.

Found porting PHPStan's
`nsrt/infer-private-property-type-from-constructor.php`,
`nsrt/remember-non-nullable-property-non-strict.php`; the assertions are
`// SKIP` in the ported copies under `tests/phpstan_nsrt/`.

### B408. An assignment inside an argument to `new` is not seen
**Impact: Low · Complexity: Low**

```php
new Foo([$inArray = 1]);
$inArray; // should be 1, has no type
foo($direct = 3); // this one works
```

Nested assignments are collected from call arguments but not from an array
literal inside a `new` argument list.

Found porting PHPStan's `nsrt/if.php`, which is not ported yet because
it is too slow under the runner (see
[P65](performance.md#p65-every-call-site-repeats-the-full-function-lookup-hit-or-miss)).

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Miscellaneous

No outstanding items.
