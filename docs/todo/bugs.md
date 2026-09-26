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

### B446. Writing into a property's array offset does not narrow the property
**Impact: Low-Medium · Complexity: Medium**

```php
/** @var array{foo?: bool} */
private array $arr = [];
public function f(): bool {
    if (!isset($this->arr['foo'])) {
        $this->arr['foo'] = true;
    }
    $this->arr; // should be array{foo: bool}, is array{foo?: bool}
}
```

An offset write on a local variable updates its shape, but the same write through `$this->prop[...]` leaves the property on its declared type, and a nested write (`$this->arr[$i]['foo'] = true`) makes the offset read back `null`.

Found porting PHPStan's `Rules/Arrays/data/bug-11679.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B447. `array_key_exists()` with a variable key does not narrow the offset read
**Impact: Low · Complexity: Medium**

```php
$results = [];
foreach ($items as $item) {
    $key = $item->key();
    if (array_key_exists($key, $results)) {
        $results[$key]; // should be array{...}, includes null
    }
    $results[$key] = ['count' => 1];
}
```

Inside a guard that proved the key exists, reading that key should not include the `null` that comes from the array possibly being empty (here, the `[]` it was initialised with). The same missing proof makes a compound write inside the guard (`$results[$key]['count'] += $n`) create a new element on the loop's first pass, when the array is still `[]`, so the array's type after the loop carries a variant with only the written key.

Found porting PHPStan's `Rules/Arrays/data/slevomat-foreach-array-key-exists-bug.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B449. Exhausted narrowing leaves the last type standing instead of `never`
**Impact: Low · Complexity: Medium**

```php
function f(true|null $v) {
    if (is_null($v)) { return; }
    if (is_bool($v)) { return; }
    $v; // should be never, is true
}
```

When every member of a type has been ruled out (by type checks, by comparing each enum case, by an `@phpstan-assert` the value cannot satisfy, or by an impossible `count()`), the value should be `never`. Today the narrowing that would remove the last member is skipped, so the branch keeps it.

Found porting PHPStan's `Analyser/Fiber/data/fnsr.php`, `Rules/Comparison/data/bug-8169.php`, `Rules/Comparison/data/bug-8485.php`, `Rules/Comparison/data/docblock-assert-equality.php` and `Rules/Methods/data/true-typehint.php`; the assertions are `// SKIP` in the ported copies under `tests/phpstan_data/`.

## Arithmetic

No outstanding items.

## Symbol resolution

### B437. `self` in an inherited property's docblock names the class it is read through
**Impact: Medium · Complexity: Medium**

```php
class A { /** @var string|self */ public $table; }
class B extends A {}
function f(B $b) { $b->table; } // should be A|string, is B|string
```

`self` is lexical: in a docblock it names the class the docblock is written in, wherever the member is inherited to. Reading an inherited property through a subclass rebinds it to that subclass, as if it were `static`. A subclass that redeclares the property without a type (`public $table = 'a';`) should inherit the parent's docblock type too.

Found porting PHPStan's `Rules/Properties/data/bug-7839.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B438. An inline `@phpstan-var` above an assignment is ignored
**Impact: Medium · Complexity: Medium**

```php
/** @phpstan-var array<Foo> $res */
$res = [];
$res; // should be array<Foo>, is array{}
```

The same annotation spelled `@var` works. The inline-`@var` readers the forward walker and the backward docblock scans use (`parse_var_docblock_pairs`, `parse_inline_var_docblock_no_var` and the line scanners in `docblock/tags.rs`) each match the literal text `@var`, so the vendor-prefixed `@phpstan-var` and `@psalm-var` never match. The mago-based `extract_var_type_with_name` already reads the vendor spellings first; moving those scanners onto it fixes all of them at once.

Found porting PHPStan's `Rules/Methods/data/bug-7511.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B439. `@mixin` on a trait is not applied to the class that uses it
**Impact: Low-Medium · Complexity: Medium**

```php
/** @template T */
interface Foo { /** @return T */ public function get(); }
/** @mixin Foo<static> */
trait FooTrait {}
class Usages { use FooTrait; }
function f(Usages $u) { $u->get(); } // should be Usages, resolves to nothing
```

A `@mixin` tag is read off the class it is declared on, but not off the traits a class uses, so the mixin's members never reach the using class. Once they do, `static` in the mixin's generic argument has to bind to the class the member is reached through (`ChildUsages` for a subclass).

Found porting PHPStan's `Rules/Methods/data/trait-mixin.php`, `Rules/Properties/data/trait-mixin.php` and `Rules/Classes/data/mixin-trait-use.php`; the assertions are `// SKIP` in the ported copies under `tests/phpstan_data/`.

### B440. `extract()` defines no variables
**Impact: Low · Complexity: Medium**

```php
/** @return array{x: string, y?: string} */
function foo(): array { return ['x' => 'foo']; }
$x = $y = null;
extract(foo());
$x; // should be string, is null
$y; // should be string|null, is null
```

`extract()` writes one local per key of its array argument. With a shape argument the keys are known, so a required key defines (or overwrites) its variable and an optional one may. Other arguments leave every variable possibly overwritten with anything.

Found porting PHPStan's `Rules/Variables/data/bug-12364.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B441. A class constant whose initializer names an enum case is not resolved
**Impact: Low · Complexity: Medium**

```php
enum E { const FOO = E::A; case A; }
E::FOO; // should be E, resolves to nothing

enum A: string { case X = 'x'; case Y = 'y'; }
class B { public const A = [A::X->value, A::Y->value]; }
B::A; // should be array{'x', 'y'}, is array
```

Constant folding stops at an enum case in the initializer, whether the case is the value itself or the case's `->value`. A typed constant declared `const static FOO = Foo::A` inside the enum reads back as `static` for the same reason.

Found porting PHPStan's `Rules/Methods/data/return-type-class-constant.php` and `Rules/Constants/data/bug-8957.php`; the assertions are `// SKIP` in the ported copies under `tests/phpstan_data/`.

### B442. `$value::class` resolves to nothing
**Impact: Low-Medium · Complexity: Low**

```php
function f(Foo $o) {
    $o::class; // should be class-string<Foo>, resolves to nothing
}
```

`Foo::class` resolves to `class-string<Foo>`, but the same fetch on an expression (allowed since PHP 8.0) has no type at all. It should be `class-string<T>` for whatever `T` the expression holds, and plain `class-string` for an `object`.

Found porting PHPStan's `Rules/Functions/data/bug-7823.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B443. A closure parameter is not inferred from a `callable(static)` type alias declared on a trait
**Impact: Low · Complexity: Medium**

```php
/** @phpstan-type SettingsFactory callable(static): array<string,mixed> */
trait WithConfig {
    /** @param SettingsFactory $settings */
    public function setConfig(callable $settings): void {}
}
class A { use WithConfig; }
function (A $a) {
    $a->setConfig(function ($who) { $who; }); // should be A, resolves to nothing
};
```

The same `callable(static)` written inline in the `@param` works (the ported file's `@method` and property variants pass). Through the alias, the closure gets no parameter type: either the alias is not expanded where callable parameter inference reads the signature, or `static` inside it is not bound to the using class.

Found porting PHPStan's `Rules/Classes/data/bug-11591.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

## Array types

### B456. Writes through a list's own keys drop `list`, while `unset()` of an element keeps it
**Impact: Low-Medium · Complexity: Medium**

```php
/** @param list<array<string, string>> $list */
function f(array $list) {
    foreach ($list as $k => $v) {
        unset($list[$k]['abc']);
    }
    $list; // should be list<array<string, string>>, is array<int, ...>

    foreach ($list as $k => $v) {
        if (rand(0, 1)) { unset($list[$k]); }
    }
    $list; // should be array<int, ...>, is list<...>
}
```

Writing to (or unsetting inside) an element the list already has keeps it a list. Removing an element does not: it leaves a gap in the keys. Both are backwards today. Writing through the keys of a `foreach` also marks the outer array non-empty, though the loop may not have run at all.

Found porting PHPStan's `Rules/Methods/data/bug-12927.php`, `Rules/Variables/data/bug-14124.php` and `Rules/Variables/data/bug-14124b.php`; the assertions are `// SKIP` in the ported copies under `tests/phpstan_data/`.

### B457. `array_push()` does not change the array's type
**Impact: Medium · Complexity: Low-Medium**

```php
$result = [];
for ($i = 0; $i < $max; $i++) {
    array_push($result, $i);
}
$result; // should be list<int>, is array
```

`array_push($a, ...$values)` is `$a[] = $value` for each value, and should update the array the way the append does. `array_unshift()` is the same with the new values first.

Found porting PHPStan's `Rules/Variables/data/bug-9403.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B458. An `(array)` cast of a union with a non-array member is plain `array`
**Impact: Low · Complexity: Low**

```php
/** @var string|list<string> $var */
(array) $var; // should be list<string>, is array
```

Casting to array is applied member by member: an array stays itself, and a scalar becomes a one-element list of it. The union of those is the result.

Found porting PHPStan's `Rules/Functions/data/bug-8280.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B459. Writing a literal into an array offset widens it to its base type
**Impact: Low-Medium · Complexity: Medium**

```php
$a['bla'] = 1;
$a; // should be array{bla: 1}, is array{bla: int}

$logs = [];
$logs[$i] = '';
$logs[$i]; // should be '', is string
```

A literal assigned to a variable keeps its literal type, but the same literal written into an array offset (a literal key on an undefined variable, or a dynamic key) is widened to its base type.

Found porting PHPStan's `Analyser/Fiber/data/fnsr.php` and `Rules/Arrays/data/bug-13538.php`; the assertions are `// SKIP` in the ported copies under `tests/phpstan_data/`.

### B460. Array shape unions are merged differently from PHPStan
**Impact: Low · Complexity: Medium**

```php
/** @var mixed[][] $review */
$review = ['Review' => ['id' => 23], /* … */];
if ($cond) {
    $review['Review'] = ['id' => null, 'text' => null];
}
$review; // should be array<array<mixed>>, is array<int|string, array|array{id: null, text: null}>
```

Joining branches that wrote different shapes leaves a redundant `array|array{…}` union where the shape is already covered by `array`, and joining shapes with differing optional keys (`Rules/Comparison/data/bug-7898.php`) marks keys required that only one branch set.

Found porting PHPStan's `Rules/Variables/data/bug-8113.php`, `Rules/Comparison/data/bug-7898.php` and `Rules/Methods/data/bug-5749.php`; the assertions are `// SKIP` in the ported copies under `tests/phpstan_data/`.

### B481. `??=` on an offset whose key is not a variable leaves the offset `null`
**Impact: Low · Complexity: Low-Medium**

```php
$totals = [];
foreach ($items as $item) {
    $totals[(string) $item] ??= 0;
    $totals[(string) $item]; // should be int, is null
}
```

With a plain variable key (`$totals[$key] ??= 0`) the read afterwards is `int`. A key that is any other expression (a cast, a call, a concatenation) is not recorded, so the read only sees the empty array the variable started as.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

### B463. A method template with a bound is shown as its bound inside the method
**Impact: Low · Complexity: Medium**

```php
/** @template T of A&B @param iterable<T> $items */
function f($items) {
    foreach ($items as $item) {
        $item; // should be T, is A&B
    }
}
```

An unbounded method template, and a class template, keep their name inside the body (`T`); a method template with a bound is replaced by the bound when the parameter is seeded. The bound is the right thing for completion, but the value is still `T`, and returning it should keep the template (a method template bounded by a class template shows the class template's name instead).

Found porting PHPStan's `Rules/Methods/data/bug-7511.php`, `Rules/Methods/data/bug-5562.php`, `Rules/Generics/data/bug-3769.php` and `Rules/PhpDoc/data/bug-4643.php`; the assertions are `// SKIP` in the ported copies under `tests/phpstan_data/`.

### B464. Template inference from a literal argument widens it
**Impact: Low · Complexity: Medium**

```php
/** @template T of int @param T $a @return T */
function intBound(int $a) { return $a; }
intBound(1); // should be 1, is int

/** @template T @param iterable<T> $it @return iterable<array<T>> */
function chunk(iterable $it) {}
chunk([1]); // should be iterable<array<1>>, is iterable<array<int>>
chunk([]);  // should be iterable<array<never>>, is iterable<array<mixed>>
```

An unbounded template bound from a literal keeps it (`mixedBound(1)` is `1`), but a template bounded by a scalar type widens it to the bound, and a template bound through an array literal's elements widens them. An empty literal should bind `never`.

Found porting PHPStan's `Rules/Generics/data/bug-3769.php` and `Rules/Methods/data/bug-5757.php`; the assertions are `// SKIP` in the ported copies under `tests/phpstan_data/`.

### B465. A new object with unbound templates assigned to a generic property keeps the bounds
**Impact: Low-Medium · Complexity: Medium**

```php
/** @var \SplObjectStorage<\DateTimeImmutable, null> */
public $dates;
public function __construct() {
    $this->dates = new \SplObjectStorage();
    $this->dates; // should be SplObjectStorage<DateTimeImmutable, null>, is SplObjectStorage<object, mixed>
}
```

When nothing in the constructor call binds a template, the object can still become whatever the declared type of the place it is stored says. PHPStan infers those templates from the property's declared type; here they fall back to their bounds and the read-back type loses the declaration.

Found porting PHPStan's `Rules/Properties/data/bug-3777.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B466. A template nested in `class-string<Foo<T>>` is not inferred
**Impact: Low · Complexity: Medium**

```php
/** @template T of OptionPresenter
 *  @param class-string<OptionDefinition<T>> $definition @return T */
function present($definition) {}
present(SimpleOptionDefinition::class); // should be SimpleOptionPresenter
                                        // (via @implements OptionDefinition<SimpleOptionPresenter>),
                                        // is class-string<SimpleOptionDefinition>
```

Binding `T` needs the named class's ancestor `OptionDefinition<…>` arguments. `class-string<T>` binds directly, but the nested case falls back to the argument's own type.

Found porting PHPStan's `Rules/Methods/data/bug-4552.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B468. A conditional return type on `$param is not null` does not pick up a template bound by a callable argument
**Impact: Low · Complexity: Medium-High**

```php
/** @template T */
interface PromiseInterface {
    /** @template TFulfilled
     *  @param (callable(T): TFulfilled)|null $onFulfilled
     *  @return PromiseInterface<($onFulfilled is not null ? TFulfilled : T)> */
    public function then(callable $onFulfilled = null);
}
/** @param PromiseInterface<true> $p */
function f(PromiseInterface $p) {
    $p->then(static fn (bool $b): bool => $b); // should be PromiseInterface<bool>, is PromiseInterface<mixed>
}
```

The conditional picks its branch, but the template in that branch is bound from the closure's return type, and that binding does not reach the conditional's evaluation.

Found porting PHPStan's `Rules/Methods/data/conditional-complex-templates.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B469. A conditional return type whose subject is an offset of a template is not evaluated
**Impact: Low · Complexity: Medium**

```php
/** @template T of list<string>|list<list<string>>
 *  @param T $bar @return (T[0] is string ? array{T} : T) */
function foo(array $bar): array {}
foo(['foo', 'bar']); // should be array{array{'foo', 'bar'}}, is array{0: 'foo', 1: 'bar'}
```

The subject `T[0]` is an offset access on the bound template, which has to be evaluated before the condition can be decided. Today the else branch is taken.

Found porting PHPStan's `Rules/PhpDoc/data/bug-8609-function.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B470. An offset access on a type alias is not evaluated
**Impact: Low · Complexity: Medium**

```php
/** @phpstan-type Bob array{a: string, b: bool} */
class Y {
    /** @template TKey of key-of<Bob> @param TKey $key @return Bob[TKey] */
    public function x(string $key) {}
}
$y->x('b'); // should be bool, is Bob['b']
```

The offset access is printed raw rather than resolved: the alias inside it is never expanded, so the key lookup has no shape to read. `Alias[value-of<T>]` with an enum-case template argument (`Rules/PhpDoc/data/bug-11033.php`) fails the same way.

Found porting PHPStan's `Rules/PhpDoc/data/bug-13652.php` and `Rules/PhpDoc/data/bug-11033.php`; the assertions are `// SKIP` in the ported copies under `tests/phpstan_data/`.

### B480. An argument outside a method template's bound binds the template anyway
**Impact: Low · Complexity: Medium**

```php
/** @template E of Entity */
class Repository {
    /** @template F of E @param F $entity @return F */
    function store(Entity $entity): Entity {}
}
/** @extends Repository<User> */
class UserRepository extends Repository {}
$r->store(new Article()); // should be User, is Article
```

`Article` is an `Entity` but not a `User`, so it cannot be `F`. The call binds `F` to the argument's type regardless of the bound, where it should fall back to the bound it fails to satisfy.

Found porting PHPStan's `Rules/PhpDoc/data/bug-4643.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B472. A template bound through a nested callable parameter is not inferred
**Impact: Low · Complexity: Medium**

```php
/** @template T @param callable(callable():T):T $closure @return T */
function bar(callable $closure) {}
/** @param callable(callable():int):string $callable */
function testBar($callable) { bar($callable); } // should be string, is mixed
```

Unifying the parameter's `callable(callable(): T): T` with the argument's signature should bind `T` from the outer return type (where the argument says `string`).

Found porting PHPStan's `Rules/Functions/data/varying-acceptor.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

## Miscellaneous

### B482. `__PROPERTY__` inside a property hook resolves to its base type rather than its value
**Impact: Low · Complexity: Low**

```php
class User {
    public string $name {
        get {
            __PROPERTY__; // should be 'name', is string
        }
    }
}
```

`__PROPERTY__` (PHP 8.4) is known at the point it is written, the way `__FUNCTION__`/`__METHOD__` are for a method.

Found porting PHPStan's `Analyser/Fiber/data/fnsr.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.

### B475. A variable a closure captures by reference does not take on the closure's assignments
**Impact: Low · Complexity: Medium**

```php
$a = 0;
$cb = function () use (&$a): void {
    $a; // should be 0|'s', is 0
    $a = 's';
};
$a; // should be 0|'s', is 0
```

A by-reference capture shares the variable with the closure, and the closure may run any number of times before or after either read. Both sides should see the union of every value the closure assigns.

Found porting PHPStan's `Analyser/Fiber/data/fnsr.php`; the assertion is `// SKIP` in the ported copy under `tests/phpstan_data/`.
