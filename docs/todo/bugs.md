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

### B423. A call inside an invoked closure does not invalidate what the closure captures
**Impact: Low · Complexity: Medium**

```php
while ($this->running) {
    call_user_func(function () {
        $this->stop();       // void, so it may change $this->running
    });
    $this->running;          // should be bool, is true
}
```

A call on an object forgets what was proved about that object's
properties and call results when the call returns `void`, returns
`$this`, or is `@phpstan-impure`. The same call made inside a closure
that runs on the spot (`(function () { … })()`, `call_user_func($cb)`,
an immediately invoked callable parameter) should forget it for `$this`
and for every variable the closure captures, and does not.

Found porting PHPStan's `nsrt/bug-10566.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

**Where to look:** `type_engine/variable/forward_walk/receiver_mutation.rs`
(`collect_call_invalidations`), and the by-reference capture handling
the walker already runs for invoked closures.

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

### B419. Writing through a key of unknown type makes the keys `int|string`
**Impact: Low · Complexity: Low**

```php
/** @return array<string, string> */
function f(mixed $k): array {
    $r = [];
    $r[$k] = 'a';
    return $r; // reported: non-empty-array<int|string, string> is incompatible
}
```

A key whose type is unknown is `array-key`, which the type comparison
treats as benevolent because nobody measured it. Written through an
array, it comes out as a plain `int|string` union instead, which both
halves have to satisfy, so a `string`-keyed return or parameter rejects
it. Showed up in phpstan-src
(`build/PHPStan/Build/TurboAttributeCollector.php`, a key read off
`ReflectionAttribute::newInstance()`).

**Where to look:** the key type recorded by the keyed-write arm in
`type_engine/variable/array_shape_writes.rs`.

### B390. A literal argument bound to a function template is widened
**Impact: Low-Medium · Complexity: Medium**

```php
/** @template T  @param T $a  @return T */
function id($a) { return $a; }
id('hello'); // should be 'hello', is string
```

Binding `T` from a literal argument generalises it to its base type.

Found porting PHPStan's `nsrt/generic-generalization.php`; the
assertions are `// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

### B392. Template defaults are ignored
**Impact: Low-Medium · Complexity: Medium**

```php
/** @template T1 = true  @template T2 = string */
interface Foo { /** @return T2 */ public function get(): mixed; }
/** @extends Foo<int> */
interface Bar extends Foo {}
function f(Bar $b) { $b->get(); } // should be string, is mixed
```

`@template T = Default` supplies the argument when a type or an `@extends`
omits it, or when a call leaves it unbound. A default that names another
template (`@template EO of DI = DI`) should resolve to that template's
argument.

Found porting PHPStan's `nsrt/template-default-referring-other.php`,
`nsrt/template-default.php`; the assertions are `// SKIP` in the ported
copies under `tests/phpstan_nsrt/`.

### B393. A template bound through an argument's ancestors, or by several arguments, is lost
**Impact: Low · Complexity: Medium-High**

```php
/** @template T @implements Type<T[]> */
final class Coll implements Type {
    /** @param Type<T> $t */ public function __construct(public Type $t) {}
}
$c = new Coll(new IntType()); // IntType implements Type<int>
$c->get(); // should be array<int>, is array

/** @template T @extends P<T, T> */
class C extends P {}
new C(new Cat(), new Dog()); // should be C<Cat|Dog>, is C<Dog>
```

Inference does not walk an argument's own `@implements`/`@extends` to match
a generic parameter type, and a template bound by several arguments keeps
only the last binding instead of their union.

Found porting PHPStan's `nsrt/bug-2735.php`, `nsrt/bug-6505.php`; the
assertions are `// SKIP` in the ported copies under
`tests/phpstan_nsrt/`.

### B394. A `null` default on an untyped `@param T` parameter makes it `?T`
**Impact: Low · Complexity: Low-Medium**

```php
class C {
    /** @template T  @param T $t  @return T */
    public function same($t = null) { return $t; } // $t should be T, is ?T
    public function g(?int $x) { $this->same($x); } // should be int|null, is int
}
```

The implicit `null` of the default is folded into the declared template, so
the body sees `?T` and the call site binds `T` with `null` stripped.

Found porting PHPStan's `nsrt/bug-6584.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

### B395. `key-of<array<V>>` is `int` instead of `int|string`
**Impact: Low · Complexity: Low**

```php
/** @template T of array<mixed> */
interface R { /** @return key-of<T>|null */ public function key(); }
/** @param R<array<mixed>> $r */
function f(R $r) { $r->key(); } // should be int|string|null, is int|null
```

The single-argument `array<V>` form has `array-key` keys.

Found porting PHPStan's `nsrt/key-of-generic.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Miscellaneous

No outstanding items.
