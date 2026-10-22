# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

Each entry below carries an **Impact · Complexity** rating using the same
scale defined in [`docs/todo.md`](../todo.md), but a bug's row lives
**here only** — do not add or link a bug entry to `docs/todo.md`'s sprint
or backlog tables. This file is its own list, not a domain document
sprint items draw from: whenever it holds anything, that is actively
addressed, independently of sprint planning.

Bugs land here from wherever they surface: found while working on another
task, or sweeps of the sample projects under `projects/`. Entries are
grouped by the mechanism that has to change, not by the symptom that
surfaced: one entry is one root cause, however many shapes it shows up in.

## Crashes

No outstanding items.

## Type comparison

No outstanding items.

## Standard-library return types

### B652. Arithmetic on `BcMath\Number` and `GMP` is typed `int|float`

**Impact: Medium · Complexity: Medium**

```php
use BcMath\Number;

function takesNumber(Number $v): void {}
function takesGmp(\GMP $v): void {}

function f(Number $price, int $qty): void {
    takesNumber($price * $qty);   // reported: expects BcMath\Number, got int|float
}
function g(\GMP $l, \GMP $r): void {
    takesGmp($l + $r);            // reported: expects GMP, got int|float
}
```

PHP 8.4 overloads `+`, `-`, `*`, `/`, `%` and `**` on `BcMath\Number`, and
`GMP` has overloaded the same set for much longer. In both cases an
operation on two operands of the class, or the class and an `int`, produces
an instance of the class. The binary-arithmetic result type is computed
from the numeric promotion rules alone and answers `int|float` whatever the
operands were, so every such expression is a false positive at the next
parameter that wants the class. Special-case the two classes in the
arithmetic result rule: `GMP` with `GMP|int` gives `GMP`, `BcMath\Number`
with `BcMath\Number|int|string` gives `BcMath\Number`, and division by the
same operand types follows. Conformance row
`native_types_overloaded_number_operators.php`.

### B653. `Closure::fromCallable()` loses the callee's signature

**Impact: Medium · Complexity: Medium**

```php
final class Repo { public function find(): User { return new User(); } }

function f(Repo $r): void {
    $c = \Closure::fromCallable([$r, 'find']);
    $c()->name();   // Cannot verify method 'name' — type of '$c()' could not be resolved
}
```

`Closure::fromCallable()` is typed as a bare `Closure` with no parameter or
return types, so invoking the result resolves to nothing. The first-class
callable form of the same thing (`$r->find(...)`) already carries the
method's return type through, so the information is available — the stub's
return type just discards it. Give `fromCallable()` a return type derived
from its argument, the same way the first-class callable syntax is handled.
See also [B661](#b661-invoking-an-array-callable-has-no-return-type), which
is the other half of the same hole.

## Reachability

No outstanding items.

## Narrowing

No outstanding items.

## Array types

### B654. Array shapes are unsealed: an undeclared key is admitted

**Impact: Medium · Complexity: Medium**

```php
/** @param array{foo: int} $shape */
function takesShape(array $shape): void {}

takesShape([]);                            // reported: missing required key 'foo'
takesShape(['foo' => 'one']);              // reported: foo is int, not string
takesShape(['foo' => 2, 'buz' => 42.0]);   // not reported: buz is undeclared

/** @param array{foo: int, buz: float} $wider */
function passAlong(array $wider): void {
    takesShape($wider);                    // not reported: wider shape is not a subtype
}
```

A shape written without a trailing `...` declares exactly the keys it
handles; `array{foo: int, ...}` is the opt-in for anything else. The
required-key and value-type halves of the check are already there, so what
is missing is the third question: a candidate shape whose key set is not a
subset of the declared one does not satisfy it. That holds both for a
literal at the call site and for a value whose extra key comes from a
declaration. PHPStan, Psalm and Mago all seal by default; conformance row
`arrays_shape_sealed_by_default.php`.

### B666. An array filled by key inside a `foreach` loses every required key

**Impact: Medium · Complexity: Medium**

```php
/** @param array{dk: string, en?: string} $texts
 *  @return array{int, Translation} */
function f(array $texts): array {
    $out = [];
    foreach ($texts as $market => $text) {
        $out[$market] = new Translation($text);
    }
    return [1, $out['dk']];   // reported: got array{1, ?Translation}
}
```

`$out` comes out of the loop as `array{dk?: Translation, en?: Translation}`.
Every key is optional, so reading `dk`, which the source shape guarantees
the loop wrote, picks up `null`. The same happens when the source is a
`non-empty-array<'dk'|'en', string>`: the loop runs at least once, but the
result is still an all-optional shape. A key the iterated shape requires
should stay required in the result. Otherwise the result should be
`non-empty-array<K, V>` when the source is non-empty, the way PHPStan
generalizes it, so a read neither adds `null` nor claims a key that may be
missing. Found in the Laravel sample.

### B667. A key read on an intersection of array types resolves nothing

**Impact: Medium · Complexity: Medium**

```php
/** @param array<string, mixed>&array{at: Carbon} $entry */
function f(array $entry): string {
    return $entry['at']->toIso8601String();   // Cannot verify method — type of
}                                             // '$entry['at']' could not be resolved
```

`array<string, mixed>&array{at: Carbon}` is the usual way to say "an
open-ended array that has at least `at`". The intersection itself resolves
and survives spreads, `usort()` and closure-parameter inference from
`array_map()`, but an offset read on it gives no type at all. A read should
take the most specific answer among the members: `Carbon` here, from the
shape. Found in the Laravel sample.

## Laravel

### B655. An unknown config key is not reported

**Impact: Low-Medium · Complexity: Low**

```php
config('app.name');       // fine
config('gate.missing');   // not reported
view('missing.page');     // reported: Unknown view
route('gate.missing');    // reported: Unknown route
__('boards.missing');     // reported: Unknown translation key
```

Views, routes and translation keys all have a string-key diagnostic, and
`config()` already resolves far enough to hover a key and name the file it
came from, so the index that would answer this is built. The diagnostic
itself is simply absent. Add it alongside the other three, with the same
conservative stance they take when the index is incomplete. Conformance row
`framework.config-diagnostic` in the language-server probes, the only
non-green framework cell for our column.

### B669. `response($content)` with a nullable argument is typed `ResponseFactory|Response`

**Impact: Medium · Complexity: Low**

```php
public function download(): Response {
    $file = Storage::disk('feeds')->get('feed.xml');   // string|null
    return response($file, 200, $headers);             // reported: ResponseFactory|Response
}
```

The helper's docblock says `($content is null ? ResponseFactory : Response)`,
but its body branches on `func_num_args() === 0`, not on the value: any call
with an argument returns `$factory->make($content ?? '', …)`, a `Response`,
even when that argument is `null`. Following the docblock literally turns
every `response($maybeNull)` into a return-type false positive. The return
type should depend on whether an argument was passed. Found in the Laravel
sample.

## Blade

No outstanding items.

## Templates

### B656. A nested conditional return type tests its inner condition against the unnarrowed parameter

**Impact: Medium · Complexity: Medium-High**

```php
/**
 * @return ($key is Model ? TModel|null : ($key is Arrayable ? static : TModel|null))
 */
public function find(mixed $key): mixed { /* … */ }

function f(Collection $collection, ?User $user): void {
    takesUserOrNull($collection->find($user));
    // reported: got User|Collection<User>|null
}
```

`?User` takes the outer `$key is Model` branch for its `User` part, so the
inner condition only ever sees what the outer one rejected — `null`, which
is not `Arrayable`. We evaluate the inner `$key is Arrayable` against the
whole `?User` instead, `User` matches, and `static` joins a return type it
cannot be part of. Each nested arm has to be resolved against the subject
the enclosing condition left for it, not against the argument as written.
This was PHPStan's own bug until 2.2.16 (phpstan-src#6586); conformance row
`regressions_nested_param_conditional.php`.

## Name resolution

### B657. `new (<expression>)` is not resolved

**Impact: Medium · Complexity: Medium**

```php
$a = new (User::class)();        // unresolved
$b = new ('App\\User')();        // unresolved
$c = new ($factory->class)();    // unresolved, even with @var class-string<User>
$d = new $someClassString();     // resolves
```

PHP 8.0 allows an arbitrary parenthesised expression as the operand of
`new`. The resolver handles the bare-variable spelling (`new $c()`) and
nothing else: with parentheses around the operand the whole `new` resolves
to nothing, so every member access on the instance is an
`unresolved_member_access` false positive. The parenthesised operand should
go through the same class-string resolution the bare variable already uses.
Conformance row `generics_class_string_property_new.php`.

### B658. A member access written directly on `new $var` resolves the variable, not the instance

**Impact: High · Complexity: Medium**

```php
function f(Customer $c): void {
    $class = $c::class;
    $copy = new $class();
    $copy->name();                 // resolves to Customer

    echo (new $class())->name();   // Cannot verify method 'name' — type of
                                   // '$class' could not be resolved
}

function g(): void {
    $class = 'App\\Customer';
    echo (new $class())->name();   // Cannot access method 'name' on type
                                   // ''App\Customer''        [scalar_member_access]
}
```

Two resolution paths answer "what does `new $var` produce", and they
disagree. The assignment handler gets it right for every class-string
flavour — `$obj::class`, a literal string, `get_class()`, a
`class-string<T>` docblock. The member-access subject resolver only handles
`class-string<T>`, and only when it is the variable's declared type: a
`class-string<Model>` obtained by `$class = $map[$type] ?? null` and a
`null` guard still fails, with or without the parentheses PHP 8.4 makes
optional. For anything else it either gives up or, worse, hands
back the *operand's* type, so a literal class name is reported as a method
call on a string. That is the parallel-resolution-path anti-pattern:
whichever of the two paths is right, there should be one. Conformance row
`objects_class_constant_on_instance.php`.

### B659. `new $classString` skips the class-kind and constructor checks

**Impact: Low-Medium · Complexity: Low**

```php
new \Countable();                           // reported: Cannot instantiate interface
function f(): void { $c = 'Countable';  new $c(); }   // not reported
function g(): void { $c = '\\Countable'; new $c(); }  // not reported
```

`invalid_class_kind` (and the argument-count check on the constructor) only
runs when `new` names a class syntactically. Once the class comes from a
class-string the checks are skipped entirely, even though the resolver
knows which class it is — including the leading-backslash spelling, which
names the same class as the slash-free one. Run the same checks on the
resolved class whatever syntax named it. Conformance row
`regressions_new_class_string_leading_backslash.php`.

## Code actions

No outstanding items.

## Miscellaneous

### B660. A return type is inferred from an untyped body only for methods

**Impact: High · Complexity: Medium**

```php
function makeUser() { return new User(); }
final class Repo { public function find() { return new User(); } }

function f(Repo $r): void {
    makeUser()->name();        // Cannot verify method 'name' — type of
                               // 'makeUser()' could not be resolved
    $r->find()->name();        // resolves to User
}
```

A method with neither a native return type nor `@return` gets its return
type read off its body; a free function with the same shape gets nothing.
Closures and arrow functions do infer, so the method path is the rule and
the free function is the exception. The result is an
`unresolved_member_access` false positive on every call to an unannotated
function — common in legacy code, which is where a language server is worth
the most. Route free functions through the same body inference the method
path uses. Conformance row `native_types_untyped_return_inferred.php`, and
the `inferred-return` hover probe, which shows no type for the same reason.

### B661. Invoking an array callable has no return type

**Impact: Medium · Complexity: Medium**

```php
function f(Repo $r): void {
    $find = [$r, 'find'];
    $find()->name();               // type of '$find()' could not be resolved
}
function g(Repo $r): void {
    $f = $r->find(...);
    $f()->name();                  // resolves to User
}
```

`[$object, 'method']` and `[Repo::class, 'method']` name their target
exactly, so invoking one is the same call as `$object->method()`. We type
the array as an array and the invocation as nothing, which makes the member
access on the result a false positive. First-class callable syntax and
closures both resolve, so the gap is the array spelling alone —
`call_user_func([$r, 'find'])` is `mixed` for the same reason. Conformance
row `callables_array_callable_invocation.php`; the sibling stub gap is
[B653](#b653-closurefromcallable-loses-the-callees-signature).
