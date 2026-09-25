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

### B412. An array shape argument is only checked on its values, and only against a typed array
**Impact: Medium · Complexity: Medium**

```php
declare(strict_types=1);
/** @param array{foo: int} $a  @param list{string} $b  @param non-empty-list<int> $c  @param list<string> $d */
function f(array $a, array $b, array $c, array $d): void {}
f(['foo' => 'one'], [null], [], ['x', 'k' => 'y']); // all four should be reported, none is
```

When a shape argument fails the shape-superset rule in `is_type_compatible`
(a key the parameter requires holds the wrong type), nothing rejects it: it
falls through to the "typed array → `ArrayShape`: MAYBE" rule, which answers
yes for any array-like argument, shapes included. The "`ArrayShape` → typed
array" rule compares only the entries' values with the parameter's value
type. The keys are never checked, so a string key passes for a `list<T>`
or an `array<string, V>` with an int key. An empty shape counts as a
`non-empty-list` or `non-empty-array` because no entries means no failures.
A shape literal is a complete, known value, so all three are definite
mismatches rather than MAYBEs.

Leave the extra-key question alone. `array{foo: int}` accepting a shape
that also has `buz` is the deliberate "shapes are open by convention" rule.

Found running the php-typing-conformance suite
(`arrays_shape_sealed_by_default.php` line 36, `arrays_non_empty_list.php`,
`regressions_array_element_null_subtraction.php`,
`regressions_list_or_map_union_rejects_hybrid.php`). PHPStan, Psalm and
mago report all four.

**Where to look:** the `ArrayShape` rules in
`src/diagnostics/type_errors/compatibility.rs` (shape superset, shape →
typed array, typed array → shape).

### B413. A subclass that binds its parent's template satisfies every parameterisation of the parent
**Impact: Medium · Complexity: Medium**

```php
/** @template T */ class Box {}
/** @extends Box<int> */ final class IntBox extends Box {}
/** @param Box<string> $box */ function f(Box $box): void {}
f(new IntBox()); // should be reported, is not
/** @var Box<int> $b */ $b = new Box();
f($b);           // reported
```

The same-base generic rule compares `Box<int>` with `Box<string>`
argument by argument. `IntBox` has no type arguments of its own, though,
so it reaches the nominal fallback, which only asks whether `IntBox`
extends `Box`. The argument's `@extends`/`@implements` binding for the
parameter's base class should be substituted first, then judged by the
generic rule.

Found running the php-typing-conformance suite
(`generics_extends_implements.php`). PHPStan, Psalm and mago report it.

**Where to look:** the same-base generic covariance block in
`src/diagnostics/type_errors/compatibility.rs`, with the ancestor's
bindings from the inheritance merge.

### B414. A template's `of` bound is not checked at the call that binds it
**Impact: Low-Medium · Complexity: Medium**

```php
/** @template T of array */
final class Collection { /** @param T $items */ public function __construct(public $items) {} }
new Collection(1); // should be reported, is not

/** @template T of array  @param T $a */
function g($a): void {}
g(1);              // should be reported, is not
```

A parameter typed by a bounded template is checked against nothing, so an
argument outside the bound binds `T` to it silently. The bound should be
the parameter type the argument has to satisfy, as it already is when
nothing binds `T`.

Found running the php-typing-conformance suite
(`generics_template_bound_array.php`). PHPStan, Psalm and mago report it.

**Where to look:** template substitution for argument checks in
`src/diagnostics/type_errors/`.

### B415. A closure's parameter types are not checked against a `callable(…)` parameter
**Impact: Low-Medium · Complexity: Medium** (depends on [T13](type-inference.md#t13-closure-variables-lose-callable-signature-detail))

```php
/** @param callable(int): string $cb */
function f(callable $cb): void {}
f(static fn (string $v): string => $v); // should be reported, is not
```

The callable-vs-callable rule in `is_type_compatible` checks the return
type only. A resolved closure never records its parameter list, so an
empty `params` means "unknown" and the parameters stay a MAYBE. Once T13
records the declared parameter types, compare them contravariantly: each
parameter the callable spec passes must be accepted by the closure's
parameter.

Found running the php-typing-conformance suite
(`callables_docblock_signature.php`). PHPStan, Psalm and mago report it.

**Where to look:** the "Callable specification ↔ callable specification"
rule in `src/diagnostics/type_errors/compatibility.rs`.

## Standard-library return types

### B386. Array functions flatten the shapes they are given
**Impact: Low-Medium · Complexity: Medium**

```php
/** @param array{a: int, b: string} $x  @param array{a: int, b?: string} $y */
function f(\DateTimeInterface $d, ?\DateTimeInterface $e, array $x, array $y): void {
    array_filter([$d, $e]); // should be array{0: DateTimeInterface, 1?: DateTimeInterface}
    array_merge($x, $y);    // should be array{a: int, b: string}, is array<string, int|string>
    min([3, 1, 2]);         // should be 1, is 1|2|3
    $cb = rand(0, 1) ? 'is_string' : 'is_int';
    array_filter($list, $cb); // should narrow by both callbacks
}
```

`array_filter` and `array_merge` generalise a shape argument before working
on it, `min()`/`max()` over a literal array do not pick the extreme value,
and `array_filter` only recognises a callable-string callback written inline.

Found porting PHPStan's `nsrt/array-filter-string-callables.php`,
`nsrt/bug-6927.php`, `nsrt/minmax-php8.php`; the assertions are
`// SKIP` in the ported copies under `tests/phpstan_nsrt/`.

**Where to look:** `filtered_container`, `array_merge_type`, and
`min_max_type` in `type_engine/variable/array_func_rules.rs`.

### B387. `foreach` over `SplObjectStorage` swaps its keys and values
**Impact: Medium · Complexity: Low-Medium**

```php
/** @param SplObjectStorage<Order, int> $s */
function f(SplObjectStorage $s): void {
    foreach ($s as $k => $v) {
        $k; // should be int, is Order
        $v; // should be Order, is int
    }
}
```

Iterating the storage yields integer positions and the stored objects; the
stub's `@template-implements Iterator<int, TObject>` says so. The `foreach`
key/value extraction takes the `ArrayAccess<TObject, TData>` binding instead,
so completion inside the loop offers the data type's members for the object.

Found porting PHPStan's `nsrt/bug-13985.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

### B388. `DOMNamedNodeMap::item()` ignores the map's node type
**Impact: Low · Complexity: Low**

```php
function f(\DOMElement $e): void {
    $attrs = $e->attributes;
    $attrs->item(0); // should be DOMAttr|null, is ?DOMNode
}
```

The stub declares `@return DOMNode|null` where its siblings use `TNode`; a
stub patch fixes it.

Found porting PHPStan's `nsrt/bug-13365.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

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

### B368. A narrowed member read or call result survives a call that can change it
**Impact: Medium · Complexity: Medium-High**

```php
class Counter {
    private int $n = 0;
    public function bump(): void { $this->n++; }
    public function f(): void {
        $this->n = 1;
        $this->bump();
        $this->n; // should be int, is 1
    }
}
function g(Foo $foo): void {
    assert($foo->getName() === 'foo');
    mutate($foo);            // a void function may change $foo
    $foo->getName();         // should be string, is 'foo'
}
```

Narrowing remembered for `$this->prop`, `$obj->method()`, or a call such as
`is_file($path)` is never dropped. PHPStan forgets it after a call that may
change the state it read: a method on the same object that returns `void`
or `$this` or is `@phpstan-impure` (including impurity a parent declares),
`parent::__construct()`, passing the object to a `void` or impure function,
or `clearstatcache()` for the filesystem checks. The same missing
invalidation is why stateful reads such as `SplFileObject::eof()` keep a
literal `true`/`false` after the call that moves the cursor.

Found porting PHPStan's `nsrt/bug-10566.php`, `nsrt/bug-11200.php`,
`nsrt/bug-4351.php`, `nsrt/bug-4816.php`, `nsrt/bug-5051.php`,
`nsrt/bug-5501.php`, `nsrt/bug-8543.php`, `nsrt/clear-stat-cache.php`,
`nsrt/impure-constructor.php`, `nsrt/impure-method.php`,
`nsrt/invalidate-object-argument-function.php`,
`nsrt/invalidate-object-argument-static.php`,
`nsrt/invalidate-object-argument.php`,
`nsrt/remember-possibly-impure-function-values.php`; the assertions are
`// SKIP` in the ported copies under `tests/phpstan_nsrt/`.

**Where to look:** `type_engine/variable/forward_walk/receiver_mutation.rs`
and the call-expression narrowing store.

### B369. `isset($arr[$k])` and `array_key_exists($k, $arr)` do not narrow `$k` to the array's keys
**Impact: Medium · Complexity: Medium**

```php
function f(string $s): void {
    $arr = ['1' => 1, '2' => 2, 3 => 3];
    if (isset($arr[$s])) {
        $s; // should be '1'|'2'|'3', is string
    }
    $seen = ['|' => false, '&' => false];
    assert(isset($seen[$s]));
    $seen[$s] = true; // should stay array{'|': bool, '&': bool}
}

/** @param array<string, int> $values */
function g(int|string $key, array $values): void {
    if (array_key_exists($key, $values)) {
        takesString($key); // reported: expects string, got int|string
    }
}
```

A successful `isset()` on an offset, or `array_key_exists()`, proves the key
is in the array's key domain. When the keys are all known, that means one of
them. When only the key type is declared, it means the key type. Without
this narrowing, a later write through the same key cannot find its entry and
widens the whole shape to `non-empty-array<string, bool>`. In the
`array_key_exists()` case the missing narrowing is a false positive that
PHPStan and mago do not report.

Found porting PHPStan's `nsrt/bug-11716.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`. The
`array_key_exists()` false positive was found running the
php-typing-conformance suite (`assertions_array_key_exists_key_narrowing.php`).

**Where to look:** `array_key_exists_target` in `assertions.rs` and the
`isset` handling, both under `type_engine/variable/forward_walk/cond_narrowing/`. B371
narrows the *array* from the same call; this entry narrows the *key*.

### B370. A loose comparison against a literal does not narrow
**Impact: Low-Medium · Complexity: Medium**

```php
/** @param 'one'|'two' $s */
function f(string $s, float $x, string $t): void {
    if ($s == 'one') { $s; } // should be 'one', is 'one'|'two'
    if ($x == 3.5) { $x; }   // should be 3.5, is float
    if (in_array($t, ['a', 'b'])) { $t; } // should be 'a'|'b', is string
}
```

`==`, `!=`, and a non-strict `in_array()` do not narrow at all, while `===`
does. Where both sides have the same scalar kind (a string against a
non-numeric string literal, a float against a float), the loose comparison
means the same as the strict one and can narrow the same way; `$a == []`
narrows an array to `array{}`.

Found porting PHPStan's `nsrt/equal.php`, `nsrt/in_array_loose.php`; the
assertions are `// SKIP` in the ported copies under
`tests/phpstan_nsrt/`.

### B371. A check compared to `true`, or `array_key_exists()` with a non-literal key, does not narrow
**Impact: Low · Complexity: Low-Medium**

```php
/** @param array<int> $haystack  @param array{0: 1, 1?: 2} $shape */
function f(?int $x, array $haystack, array $shape): void {
    if (in_array($x, $haystack, true) === true) { $x; } // should be int, is ?int
    $k = 1;
    if (array_key_exists($k, $shape)) { $shape; } // should be array{1, 2}
}
```

Wrapping a narrowing call in `=== true` hides it from the condition
analysis, and `array_key_exists()` only narrows when its key is written as a
string literal, not a variable holding a literal or an integer key.

Found porting PHPStan's `nsrt/bug-3013.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

**Where to look:** `array_key_exists_target` in
`type_engine/variable/forward_walk/cond_narrowing/assertions.rs`.

### B372. A condition stored in a variable loses its narrowing
**Impact: Low-Medium · Complexity: Medium**

```php
function f(object $c, ?int $limit, int $count, array|string $v): void {
    $ok = $c instanceof Server ? $c->ok() : false;
    if ($ok) { $c; } // should be Server, is object
    $show = $limit !== null && $count > $limit;
    if ($show) { $limit; } // should be int, is ?int
    $isArray = is_array($v);
    if ($isArray) { $v; } // should be array, is array|string
}
```

`$flag = $a && $b;` followed by `if ($flag)` already narrows in some shapes;
a ternary with a `false` arm, a `!== null` operand, and a bare type guard
stored in a variable do not carry their narrowing to the later test.

Found porting PHPStan's `nsrt/bug-1209.php`, `nsrt/bug-3190.php`,
`nsrt/falsy-isset.php`; the assertions are `// SKIP` in the ported
copies under `tests/phpstan_nsrt/`.

### B373. `is_a()` narrowing ignores `allow_string`, class-string variables, and a narrower subject
**Impact: Low · Complexity: Medium**

```php
/** @param class-string<Bar> $b  @param class-string<Foo> $cs */
function f(string $s, string $b, object $o, string $cs): void {
    if (is_a($s, Foo::class, true)) { $s; } // should be class-string<Foo>, is Foo
    if (is_a($b, Foo::class, true)) { $b; } // should stay class-string<Bar> (Bar extends Foo)
    if (is_a($o, $cs)) { $o; }              // should be Foo, is object
}
```

With `allow_string` a string subject narrows to `class-string<Foo>` (and
`mixed` to `Foo|class-string<Foo>`), not to an instance. A subject already
narrower than the target keeps its own type, and a class argument held in a
`class-string<Foo>` variable narrows as the literal `Foo::class` does.

Found porting PHPStan's `nsrt/bug-6404.php`, `nsrt/is-a.php`; the
assertions are `// SKIP` in the ported copies under
`tests/phpstan_nsrt/`.

### B374. `instanceof` against a class that cannot be loaded clears the variable's type
**Impact: Medium · Complexity: Medium**

```php
/** @var Foo|Missing|Other $x */
if ($x instanceof Foo) {
} elseif ($x instanceof Missing) {
    $x; // should be Missing, has no type
} else {
    $x; // should be Other, is Missing|Other
}
$x; // should be Foo|Missing|Other, has no type
```

`apply_instanceof_inclusion` empties the variable when the target class is
not loadable, on purpose, so that the diagnostics engine treats it as
untyped and stays quiet. That is diagnostic suppression through an empty
result, which the project rules forbid: the branch should hold the named
class even without its `ClassInfo`, and the `else` branch should drop it.
Emptying the branch also empties the variable after the chain joins.

Found porting PHPStan's `nsrt/type-elimination.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

**Where to look:** `apply_instanceof_inclusion` / `apply_instanceof_exclusion`
in `type_engine/types/narrowing/instanceof.rs`.

### B375. `@phpstan-assert-if-true` misses the receiver's template binding and untyped subjects
**Impact: Low · Complexity: Medium**

```php
/** @template T of Id */
interface Fetcher {
    /** @phpstan-assert-if-true T $id */
    public function supports(Id $id): bool;
}
/** @implements Fetcher<PostId> */
final class PostFetcher implements Fetcher { /* … */ }
function f(Id $i, $untyped): void {
    if ((new PostFetcher())->supports($i)) { $i; } // should be PostId, is Id
    if ((new PostFetcher())->supports($untyped)) { $untyped; } // should be PostId, has no type
}
```

The asserted `T` is applied at its bound instead of the receiver's
`@implements` argument, and an argument variable with no type is not seeded
with the asserted one.

Found porting PHPStan's `nsrt/bug-10037.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

### B376. `ReflectionClass::isSubclassOf()` does not narrow the reflected class
**Impact: Low · Complexity: Low-Medium**

```php
/** @param class-string $a */
function f(string $a): void {
    $r = new ReflectionClass($a);
    if ($r->isSubclassOf(Picture::class)) {
        $r; // should be ReflectionClass<Picture>, is ReflectionClass<object>
    }
}
```

PHPStan narrows the template argument through the check; nothing does so here.

Found porting PHPStan's `nsrt/bug-12473-types.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

### B377. A type guard on an array offset does not narrow the array
**Impact: Low · Complexity: Medium**

```php
$x = ['x' => foo()]; // foo(): mixed
if (is_int($x['x'])) {
    $x['x']; // int
    $x;      // should be array{x: int}, is array{x: mixed}
}
```

The offset expression narrows, but the entry of the shape it reads is not
written back.

Found porting PHPStan's `nsrt/bug-8249.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

### B378. `count($a) == count($b)` does not give `$b` `$a`'s length
**Impact: Low · Complexity: Medium**

```php
/** @param array{int, int, int} $a  @param list<mixed> $b */
function f(array $a, array $b): void {
    if (count($a) == count($b)) {
        $b; // should be array{mixed, mixed, mixed}, is list<mixed>
    }
}
```

A list compared against a fixed-size shape's count has that many entries.

Found porting PHPStan's `nsrt/list-count2.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

## Arithmetic

### B411. Arithmetic on float literals is not folded
**Impact: Low · Complexity: Low**

```php
$w = 1;
$scale = 2.0;
$w *= $scale; // should be 2.0, is float
```

Integer literal arithmetic folds; a float operand widens the result to
`float`.

Found porting PHPStan's `nsrt/if.php`, which is not ported yet because
it is too slow under the runner (see
[P65](performance.md#p65-every-call-site-repeats-the-full-function-lookup-hit-or-miss)).

## Symbol resolution

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

### B381. Appending to an array shape turns it into a list
**Impact: Low-Medium · Complexity: Medium**

```php
$a[] = 'one';  // should be array{'one'}, is non-empty-list<string>
$b = [1, 2, 3];
$b[] = null;   // should be array{1, 2, 3, null}, is non-empty-list<1|2|3|null>
```

`$v[] = x` outside a loop could extend a sealed shape by one positional
entry. The widening to a list (and of literals to their base type) is
deliberate in `merge_push_type` / the append arm of
`merge_nested_array_write` in `type_engine/variable/array_shape_writes.rs`,
to keep appends inside loops from growing the shape without bound, so this
needs a decision on whether straight-line appends may keep the shape.

Found porting PHPStan's `nsrt/if.php`, which is not ported yet because
it is too slow under the runner (see
[P65](performance.md#p65-every-call-site-repeats-the-full-function-lookup-hit-or-miss)).

### B382. Writing an integer key into an integer-keyed shape widens the shape
**Impact: Low · Complexity: Low**

```php
/** @param array{1: string, 2: int} $arr */
function f(array $arr): void {
    $arr[1] = 'x'; // should stay array{1: string, 2: int}, is non-empty-array<int, string|int>
}
```

The in-place update only looks for positional (unkeyed) entries, so an
explicit `1:` key falls through to the generic keyed write.

Found porting PHPStan's `nsrt/preserve-large-constant-array.php`; the
assertions are `// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

**Where to look:** the keyed arm of `merge_nested_array_write_inner` in
`type_engine/variable/array_shape_writes.rs`.

### B383. Joining `array{}` with a non-empty list loses the element type
**Impact: Low · Complexity: Low-Medium**

```php
/** @param list<int> $c */
function f(array $c): void {
    if (count($c) > 0) {
        $c = array_map(fn() => new \stdClass(), $c);
    }
    $c; // should be list<stdClass>, is array
}
```

The `else` path narrowed `$c` to `array{}`, and joining that with
`non-empty-list<stdClass>` falls back to bare `array`.

Found porting PHPStan's `nsrt/bug-10264.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

### B384. A `foreach` by-reference write does not change the array's element type
**Impact: Low · Complexity: Medium**

```php
/** @param list<mixed> $list */
function f(array $list): void {
    foreach ($list as $k => &$v) { $v = 'foo'; }
    $list; // should be list<'foo'>, is list<mixed>
}
```

An unconditional write to the by-reference value replaces every element.

Found porting PHPStan's `nsrt/bug-13809.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

### B385. Assigning through `ArrayAccess` does not replace an offset's narrowing
**Impact: Low · Complexity: Low-Medium**

```php
/** @param ArrayAccess<int, ?object> $o */
function f(ArrayAccess $o): void {
    assert($o[1] === null);
    $o[1] = new stdClass();
    $o[1]; // should be stdClass, is null
}
```

Plain arrays already replace the narrowed offset on write; the object branch
of the offset assignment does not.

Found porting PHPStan's `nsrt/bug-13214.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

## Docblock handling

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

### B396. A conditional type on `$this` or in `@param-out` is not evaluated
**Impact: Low · Complexity: Medium**

```php
class T {
    /** @param A|null $arg  @param-out ($arg is null ? A&I : A) $arg */
    public static function m(?A &$arg = null): void {}
}
$b = null;
T::m($b);
$b; // should be A&I, is A
```

A conditional `@param-out`, and a conditional return keyed on `$this`
(`($this is self<true> ? int : string)` after a `@phpstan-self-out`), fall
back to a branch without comparing against the argument or receiver.

Found porting PHPStan's `nsrt/pr-5108.php`, `nsrt/template-default.php`;
the assertions are `// SKIP` in the ported copies under
`tests/phpstan_nsrt/`.

### B397. `static` inside a generic type is left unbound on a `Class::` access
**Impact: Low-Medium · Complexity: Low-Medium**

```php
class Foo {
    /** @return array<static> */ public static function all() {}
    /** @var array<static> */ public static $items;
}
class Bar extends Foo {}
Foo::all();   // should be array<Foo>, is array<static()>
Bar::$items;  // should be array<Bar>, is array<static(Bar)>
```

Instance calls bind a nested `static`; static calls and static property reads
leave it open (or empty) instead of fixing it to the named class.

Found porting PHPStan's `nsrt/static-methods.php`,
`nsrt/static-properties.php`; the assertions are `// SKIP` in the ported
copies under `tests/phpstan_nsrt/`.

### B398. `self` inside an object shape is not bound to the declaring class
**Impact: Low · Complexity: Low-Medium**

```php
class Foo {
    /** @param object{foo: self} $o */
    public function f($o): void { $o->foo; } // should be Foo, is self
    /** @return object{foo: self} */
    public function g(): object {}
}
```

The keyword survives into the shape's property type, so reading it later
names whatever class is asking. Found porting PHPStan's `nsrt/object-shape.php`;
the three assertions are `// SKIP` in `tests/phpstan_nsrt/object-shape.php`
(they used to pass only because the runner accepted `self` for any class).

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Miscellaneous

### B416. `(object)` of a non-empty array loses `stdClass`
**Impact: Low · Complexity: Low**

```php
/** @param object{foo: int}&\stdClass $s */
function f(object $s): void {}
f((object) ['foo' => 1]); // reported: expects object{foo: int}&stdClass, got object{foo: int}
```

Casting an array to an object always produces a `stdClass`. `object_cast_type`
returns bare `stdClass` for `(object) []` and for anything it cannot read,
but for a shape or a scalar it returns only the `object{…}` shape. The result
should be `object{…}&stdClass`, so it keeps `stdClass`'s class identity (and
the dynamic properties that come with it) alongside the known keys.

Found running the php-typing-conformance suite
(`phpdoc_advanced_object_shape_variants.php`). Every other column except
Psalm accepts the call.

**Where to look:** `object_cast_type` in
`type_engine/variable/rhs_resolution/mod.rs`.

### B409. `??` on an undefined or null-only left side gives `mixed`
**Impact: Low · Complexity: Low**

```php
function f(): void {
    $x = $a ?? 1; // $a is never assigned; should be 1, is mixed
    if (rand(0, 1)) { $b = null; }
    $y = $b ?? 2; // should be 2, is mixed
}
```

An operand that is undefined, or `null` wherever it is defined, contributes
nothing to the result; `resolve_null_coalesce_chain` treats it as an
unresolvable operand and adds `mixed`.

Found porting PHPStan's `nsrt/falsey-coalesce.php`; the assertions are
`// SKIP` in the ported copy under `tests/phpstan_nsrt/`.

### B410. A `void` call's result is typed `void` instead of `null`
**Impact: Low · Complexity: Low**

```php
function f(\EmptyIterator $it): void {
    $x = $it->rewind(); // should be null, is void
}
```

A variable never holds `void`; the value of a `void` call is `null`.

Found porting PHPStan's `nsrt/emptyiterator.php`,
`nsrt/template-default.php`; the assertions are `// SKIP` in the ported
copies under `tests/phpstan_nsrt/`.
