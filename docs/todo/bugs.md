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

## Conditional return types

### B807. A conditional comparing a template with a class constant takes the last branch

**Impact: Low · Complexity: Low**

```php
/**
 * @template T as int
 * @param T $i
 * @return (T is self::TYPE_STRING ? string : (T is self::TYPE_INT ? int : bool))
 */
public static function getDifferentType(int $i) {}

A::getDifferentType(0);   // bool, should be string (TYPE_STRING = 0)
```

The condition's target is a class constant, which is compared as written
rather than as the literal it holds, so no argument ever matches it.

**Fix:** resolve a `self::`/`Foo::` constant target to its value before
comparing. The cases are the `// SKIP` lines in
`tests/psalm_assertions/template_conditional_return_type.php`.

### B808. A conditional on a callable argument's return type keeps both branches

**Impact: Low · Complexity: Medium**

```php
/**
 * @template T
 * @param callable(): T $callback
 * @return (T is Promise ? Promise<TPromise> : Promise<TReturn>)
 */
function call(callable $callback): Promise {}

call(function (): Promise { return new Success(9); });  // Promise<mixed>|Promise<int>, should be Promise<int>
```

`T` is bound from the closure's return, but the conditional is read as
undecided and both branches are joined.

**Fix:** decide the conditional once `T` is bound from the callable's
return type. See `tests/psalm_assertions/template_conditional_return_type.php`.

### B809. `func_num_args()` in a conditional return type is never decided

**Impact: Low · Complexity: Low**

```php
/** @return (func_num_args() is 0 ? false : string) */
function f(string $s = "") {}

f("hello");   // false|string, should be string
```

**Fix:** compare the call site's argument count with the target. See
`tests/psalm_assertions/template_conditional_return_type.php`.

### B811. A conditional return type on `PHP_VERSION_ID` takes the else branch

**Impact: Low · Complexity: Low**

```php
/** @return (PHP_VERSION_ID is int<70300, max> ? string : int) */
function getSomething() {}

getSomething();   // int, should be string on any PHP 8 project
```

The subject is a constant rather than a parameter, so the condition is
never evaluated against the project's PHP version.

**Fix:** read `PHP_VERSION_ID` (and other constants with a known value) as
the condition's subject. See
`tests/psalm_assertions/template_conditional_return_type.php`.

## Standard-library return types

### B773. `constant()` with a literal name returns `mixed`

**Impact: Low · Complexity: Low**

```php
define('FOO', 'foo');
constant('FOO');                        // mixed, should be 'foo'
constant('\Constant\Baz::BAZ');         // mixed, should be 'baz'
constant('\Constant\Suit::Hearts');     // mixed, should be Suit::Hearts
```

**Fix:** resolve a literal (or literal-union) argument as the constant or
class constant it names, the way the bare name resolves. `// SKIP` lines in
`tests/phpstan_nsrt/constant.php`.

### B774. `filter_input()` and `filter_var()` with `FILTER_FORCE_ARRAY` return `mixed`

**Impact: Low · Complexity: Low**

```php
filter_input(INPUT_GET, 'foo', FILTER_VALIDATE_INT, ['flags' => FILTER_FORCE_ARRAY]);
// mixed, should be array<int|false>|null
```

**Fix:** teach the filter return-type rule the array flags, alone and
combined with `FILTER_NULL_ON_FAILURE`. `// SKIP` lines in
`tests/phpstan_nsrt/filter-input.php`.

### B775. `compact()` returns `array` rather than a shape

**Impact: Low · Complexity: Low**

```php
$v = 1; $k = 'a';
compact('v', 'k');   // array, should be array{v: 1, k: 'a'}
```

**Fix:** build the shape from the literal names and the variables' types in
scope. `// SKIP` line in `tests/phpstan_nsrt/array-find.php`.

### B776. `array_merge()` with a spread argument returns `array`

**Impact: Low · Complexity: Low**

```php
/** @param string[][] $foo */
array_merge([], ...$foo);   // array, should be array<string>
```

**Fix:** merge the spread iterable's element type like any other argument.
`// SKIP` lines in `tests/phpstan_nsrt/array-merge.php`.

### B777. `array_find()` does not narrow by a type-checking callback

**Impact: Low · Complexity: Low**

```php
/** @param array{1, 'foo', \DateTime} $array */
array_find($array, 'is_int');   // 1|'foo'|DateTime|null, should be 1
```

**Fix:** narrow the found value by a callback named as a type check
(`'is_int'`, `'is_string'`, …), as `array_filter()` does. `// SKIP` lines in
`tests/phpstan_nsrt/array-find.php`.

### B778. String functions and `++` on literals are not folded

**Impact: Low · Complexity: Low-Medium**

```php
str_repeat($anyString, 0);               // string, should be ''
/** @param 'foo'|'bar' $fooOrBar */
trim('foo', $fooOrBar);                  // string, should be ''|'foo'
mb_strtolower('Ａｂｃ');                  // lowercase-string, should be 'ａｂｃ'
mb_convert_case('Česká', MB_CASE_UPPER); // string, should be 'ČESKÁ'
ucwords('hello|world', '|');             // string, should be 'Hello|World'
mb_substr('déjà_vu', 0, 3);              // string, should be 'déj'
$a = 'a'; $a++;                          // string, should be 'b'
```

`strtolower()`, `trim()` with a literal character list, `str_repeat()` with a
literal count and others already fold a literal argument. The multibyte
variants, a union of literals in the second argument, a zero count, the
delimiter form of `ucwords()`, and PHP's alphanumeric string increment do not.

**Fix:** fold these the way their single-byte, single-literal forms are
folded, over each member of a literal union. `// SKIP` lines in
`tests/phpstan_nsrt/str-casing.php`, `trim.php`, `literal-string.php`,
`non-empty-string.php`, `non-empty-string-substr.php` and
`mixed-to-number.php`.

### B779. Folding reads a double-quoted literal's escapes as written

**Impact: Low · Complexity: Low**

```php
mb_strtolower("\xfe\xff\x65", 'CP1252');   // '\xfe\xff\x65', the source text
```

A string literal keeps its source spelling, so a function folded over a
double-quoted literal works on the backslash sequences rather than the bytes
they stand for, and the result can be a value PHP never produces.

**Fix:** fold over the literal's decoded value and print the result in a
spelling that round-trips. `// SKIP` line in `tests/phpstan_nsrt/str-casing.php`.

### B795. `array_column()` on a constant array returns `array`

**Impact: Low · Complexity: Low**

```php
$plop = [12 => ['name' => 'ROLE_USER'], 28 => ['name' => 'ROLE_ADMIN']];
array_column($plop, 'name', null);   // array, should be array{'ROLE_USER', 'ROLE_ADMIN'}
```

**Fix:** when the input is a shape whose rows are shapes, build the result
from the named column of each row (and the index column's values as keys,
when given). `// SKIP` line in `tests/phpstan_nsrt/bug-12954.php`.

## Reachability

### B763. A branch that cannot run keeps its subject's type instead of `never`

**Impact: Low · Complexity: Medium**

```php
/** @param 1|2|3|4 $i */
function f(int $i): void {
    $x = match ($i) { 0 => $i };                  // 1|2|3|4, should be never
    foreach (['a', 'b', 'c'] as $item) {
        if (!in_array($item, ['a', 'b', 'c'], true)) {
            $item;                                // 'a'|'b'|'c', should be never
        }
    }
    $key = 'x';
    X::get($key);  // @phpstan-assert key-of<self::KEYS> $key, KEYS = ['a' => 1, 'b' => 2]
    $key;                                         // 'x', should be never
}
```

When a check rules out every value a variable can hold, the code after it
cannot run, but the narrowing keeps the type the variable came in with.
`apply_in_array_narrowing` does this on purpose ("a branch that cannot run
… keeps the type it came in with"), while `!==` against an enum's last case
already narrows to `never`, so the two disagree. A `match` arm whose
condition no value meets, a `match` with no arm that can match, and an
`@phpstan-assert` disjoint from the argument behave the same way, and so
does `!empty(X)` on a constant holding `[]`. An `array_filter()` callback no
entry can pass (`fn ($v) => is_string($v)` over `array<int, int>`) should
leave `array{}`, but `narrow_type_by_condition` reports the impossible
narrowing as no narrowing at all, so the filter keeps its input type.

**Fix:** decide once whether an impossible narrowing yields `never`, and
apply it to `in_array()`, `match` arms and conditions, assertion tags and
filter callbacks alike, and to the code after an always-throwing
`$x ??= throw …` on a value that is always `null` and to an ordering against
`null` that never holds (`$i < null`). The `// SKIP` lines in
`tests/phpstan_nsrt/enum-in-array.php`, `match-expr.php`, `bug-8559.php`,
`bug-14369.php`, `bug-7913.php`, `comparison-operators.php` and
`strlen-int-range.php` are the cases; the unported
`array-filter-arrow-functions.php` and `array-filter-callables.php` in
PHPStan's `nsrt/` have the filter ones.

## Narrowing

### B764. `match` narrows through fewer subject shapes than `if` does

**Impact: Low · Complexity: Low-Medium**

```php
match (get_debug_type($class)) {                  // FinalFoo|FinalBar $class
    FinalFoo::class => $class,                    // still FinalFoo|FinalBar
};
match ($foo = $val) {                             // ?int $val
    null => $val,                                 // ?int, should be null ($foo is narrowed)
};
$intOrString = rand(0, 1) ? 'integer' : 'string';
match (gettype($value)) {
    $intOrString => $value,                       // unchanged, should be int|string
};
```

The match subject is recognised as `$x::class`, `get_class($x)`, a plain
variable, or `gettype($x)` against literal arms. `get_debug_type($x)`
(which also returns scalar names such as `'int'`), the source of an
assignment used as the subject, and an arm condition that is a variable
holding literals narrow nothing.

**Fix:** route each of these through the same comparison narrowing `if
(get_debug_type($x) === Foo::class)` and `$a = $b` aliasing use. The cases
are the `// SKIP` lines in `tests/phpstan_nsrt/match-expr.php` and
`bug-10084.php`.

### B765. An assignment inside a `match` arm's condition is not walked

**Impact: Low · Complexity: Low**

```php
match ($i) {
    $a = $i, 1, $b = $i => null,
};
// hover on $a or $b: no type
```

The forward walker resolves arm conditions as comparisons but never walks
them as expressions, so an assignment written there (or anything else with
a side effect on the scope) is lost.

**Fix:** walk each arm condition in order, the way the subject is walked.
`tests/phpstan_nsrt/match-expr.php` has the `// SKIP` lines.

### B780. `??` keeps its right side when the left side cannot be null

**Impact: Medium · Complexity: Low**

```php
function f(int $i, Foo $foo): void {
    $i ?? false;            // int|false, should be int
    $foo->string ?? false;  // string|false, should be string (@var string)
    $three = 3;
    $three ?? 4;            // 3|4, should be 3
    $shape = ['dim' => 1];
    $shape['dim'] ?? 0;     // 1|0, should be 1
}
```

The fallback is joined in whether or not the left side can be `null` or
missing, so a non-nullable value gains the fallback's type, which then shows
up in hover and in argument-type checks downstream.

**Fix:** add the right side only when the left side's type includes `null`
or it may be unset (an optional shape key, a maybe-defined variable, an
offset the array type cannot rule out). `// SKIP` lines in
`tests/phpstan_nsrt/isset-coalesce-empty-type.php` and
`isset-coalesce-empty-type-root.php`.

### B781. `??` on an offset or variable that cannot exist gives `mixed`

**Impact: Low · Complexity: Low**

```php
$list = [1, 2, 3];
$list['string'] ?? 0;          // mixed, should be 0
function g(): void {
    $bar->bar ?? 'foo';        // mixed, should be 'foo' ($bar is never defined)
}
```

When the left side is known not to exist (a key a sealed shape does not
have, an undefined variable) the result is the right side alone. An
undefined plain variable already works (`$doesNotExist ?? 0` is `0`); an
offset or property read through one does not.

**Fix:** treat a left side that is certainly missing as contributing nothing.
`// SKIP` lines in `tests/phpstan_nsrt/isset-coalesce-empty-type.php`.

### B782. `array_key_exists()` narrows a key by the declared key type

**Impact: Low · Complexity: Low**

```php
/**
 * @param array<int, mixed> $a
 * @param array<mixed> $c
 */
function f(array $a, array $c, int|string $k, mixed $m): void {
    if (array_key_exists($k, $a)) { $k; }  // int, should be int|numeric-string
    if (array_key_exists($m, $c)) { $m; }  // mixed, should be int|string
}
```

PHP casts a numeric-string key to an int, so a string can find an int key,
and any key that is found is an `array-key`. The narrowing intersects the
key with the array's key type instead.

**Fix:** narrow to what can be found: the array's key type plus the numeric
strings that cast to its int keys, and `int|string` when the key type says
nothing. `// SKIP` lines in `tests/phpstan_nsrt/array-key-exists.php`.

### B783. A null check on `array_key_first()` or `array_key_last()` does not narrow the array

**Impact: Low · Complexity: Low**

```php
/** @param list<string> $array */
function f(array $array) {
    if (array_key_first($array) !== null) { return; }
    $array;   // list<string>, should be array{}
}
```

**Fix:** treat `array_key_first($a) !== null` (and `array_key_last()`, either
operand order, also through an assignment) like `$a !== []`. `// SKIP` lines in
`tests/phpstan_nsrt/bug-13546.php`.

### B784. Comparing a shape entry with `=== true` does not pick the union member

**Impact: Medium · Complexity: Low**

```php
/** @return array{success: false, reason: string}|array{success: true, id: int} */
function r(): array {}
$ret = r();
if ($ret['success'] === true) {
    $ret;   // both shapes, should be array{success: true, id: int}
}
```

Tagged unions keyed on a literal string or int narrow already. A `true` or
`false` entry does not, so the `id` key is reported as possibly missing.

**Fix:** let the comparison narrowing that picks union members by a literal
entry accept the bool literals. `// SKIP` lines in
`tests/phpstan_nsrt/bug-6728.php`.

### B785. Comparing an array with `array_filter()` of itself narrows nothing

**Impact: Low · Complexity: Medium**

```php
if (is_array($c) && $c === array_filter($c, 'is_string', ARRAY_FILTER_USE_KEY)) {
    $c;     // array, should be array<string, mixed>
}
$args = [$one, $two];                       // array{string, string}
assert(array_filter($args) === $args);
$args;      // array{string, string}, should be array{non-falsy-string, non-falsy-string}
```

**Fix:** when an identity comparison has a side whose type is a narrowing of
the other variable, narrow the variable to it. `// SKIP` lines in
`tests/phpstan_nsrt/bug-6329.php` and `bug-8373.php`.

### B786. `in_array()` with enum cases does not narrow, and a negated one keeps its needle in the haystack

**Impact: Low · Complexity: Low**

```php
function f(mixed $v, array $haystack): void {
    if (in_array($v, Suit::cases())) {
        $v;            // mixed, should be Suit::A|Suit::B
    }
    /** @var array<Suit|int> $haystack */
    if (!in_array(Suit::A, $haystack)) {
        $haystack;     // array<Suit|int>, should be array<Suit::B|int>
    }
}
/** @param array{string, string|null} $data */
function g(array $data): void {
    if (in_array(null, $data, true)) { throw new Exception(); }
    $data;             // array{string, ?string}, should be array{string, string}
}
```

**Fix:** read `Enum::cases()` as the union of its cases when narrowing a
needle, and remove a single-value needle (an enum case, `null`, any literal)
from the haystack's element type in the negated strict branch. `// SKIP`
lines in `tests/phpstan_nsrt/in-array-enum.php` and `bug-7153.php`.

### B787. A `strlen()` comparison does not narrow a string to `''`

**Impact: Low · Complexity: Low**

```php
if (0 < strlen($s)) { return; }
$s;   // string, should be ''
```

The non-empty branch narrows; the branch where the length must be zero does
not.

**Fix:** narrow to `''` in the branch where the comparison rules out every
length above zero. `// SKIP` line in `tests/phpstan_nsrt/non-empty-string.php`.

### B796. A narrowed call result survives a by-reference call that can change it

**Impact: Low · Complexity: Low-Medium**

```php
if (false === openssl_error_string()) {
    openssl_sign('1', $signature, '');
    openssl_error_string();   // false, should be string|false
}
```

A condition on a call's result is remembered under the call's text, and a
later call to an impure function is meant to forget it. `openssl_sign()`
writes through a reference and queues an error, yet the remembered `false`
is still read back after it.

**Fix:** treat a call to a function with by-reference parameters (and any
other impure builtin) as forgetting remembered call results, the way an
impure method call does. `// SKIP` line in `tests/phpstan_nsrt/bug-7106.php`.

### B798. A key a list is known to have does not make the keys below it required

**Impact: Low · Complexity: Low-Medium**

```php
/** @param list{0?: string, 1?: string, 2?: string} $l */
function f(array $l): void {
    if (array_key_exists(2, $l)) {
        $l;   // array{0?: string, 1?: string, 2: string}, should be array{string, string, string}
        if (!array_key_exists(1, $l)) {
            $l;   // array{0?: string, 2: string}, should be never
        }
    }
}
```

A list's keys run from `0` without gaps, so proving key `n` exists proves
every key below it does too. The narrowing marks only the key it tested as
required, and in doing so stops treating the shape as a list.

**Fix:** when a key check on a list shape proves an index set, mark every
lower index required as well and keep the shape a list. `// SKIP` lines in
`tests/phpstan_nsrt/bug-14177.php`.

### B801. A check on a copy of a variable does not narrow the original

**Impact: Low · Complexity: Medium**

```php
$parent = $my->getParent();   // MyInterface|null
$case = $parent;
if (!$case) {
    return;                   // $parent: MyInterface|null, should be null
}
$parent;                      // MyInterface|null, should be MyInterface
```

A bare boolean standing for a check is followed back to the check, but a
plain copy of the subject is not, so narrowing the copy leaves the
original as it was.

**Fix:** treat `$b = $a` as an alias while neither is written again, and
narrow both. See `tests/phpstan_nsrt/bug-2977.php`.

### B803. An assertion one class of a union receiver makes narrows as if every class made it

**Impact: Medium · Complexity: Low-Medium**

```php
// Foo::narrowGetParam(): @phpstan-assert-if-true string $this->getParam()
// Bar::narrowGetParam(): @phpstan-assert int $this->getParam()
function test(Foo|Bar $fooOrBar): void {
    if ($fooOrBar->narrowGetParam()) {
        $fooOrBar->getParam();   // string, should be int|string|null at most
    } else {
        $fooOrBar->getParam();   // int|null, should be int|string|null
    }
}
```

Each class's assertion is applied to the whole receiver, so the branch
loses the alternatives the other class still allows.

**Fix:** narrow each receiver class by its own assertion and join the
results. See `tests/phpstan_nsrt/bug-14108d.php`.

### B804. `array_search()` does not narrow its haystack to an array

**Impact: Low · Complexity: Low**

```php
function f($mixed) {
    if (array_search('x', $mixed) !== false) {
        $mixed;   // mixed, should be array
    }
}
```

The call throws on anything but an array, so a branch that saw it return
already knows its haystack is one.

**Fix:** narrow the haystack argument the way `in_array()` narrows its
needle. See `tests/phpstan_nsrt/array-search-type-specifying.php`.

## Array types

### B766. Conditional appends forget which value sits at which offset

**Impact: Low · Complexity: Medium**

```php
$x = [];
if (rand(0, 1)) { $x[] = 'ab'; }
if (rand(0, 1)) { $x[] = 'xy'; }
// array{}|array{'ab'|'xy'}|array{'ab'|'xy', 1: 'xy'}
// should be array{}|array{'xy'}|array{0: 'ab', 1?: 'xy'}
```

Joining the two branches of each `if` merges the shapes entry by entry, so
the first offset takes both values even in the two-entry shape, where it can
only hold `'ab'`.

The same join turns `rand(0, 1) ? ['b' => 1] : ['c' => 2]` into
`array{b?: 1, c?: 2}`, which also allows an empty array and one holding
both keys.

**Fix:** keep the shapes from the two branches apart when they differ in
length or keys, as PHPStan's constant-array builder does. `// SKIP` lines in
`tests/phpstan_nsrt/narrow-tagged-union.php`,
`array-merge-const-non-const.php` and `array-replace-const-non-const.php`.

### B767. A value written into an array inside a loop loses its literals

**Impact: Low · Complexity: Medium**

```php
$ints = [];
foreach ($xs as $x) { $ints[] = rand(0, 1) ? 1 : 2; }
// list<int>, should be list<1|2>
```

`loop_written_value` widens every non-constant value written into an array
in a loop body to its base type, so that a value that grows on each pass
(`$s .= 'x'`) cannot keep the walk from settling. A literal union that is
the same on every pass loses its literals too, including one that reads
nothing the loop changes.

**Fix:** widen only what the loop's own fixed-point walk shows to grow
between passes, as `widen_grown_int_locals` does for integers. `// SKIP`
lines in `tests/phpstan_nsrt/foreach-loop-variables.php`.

### B768. Removing an end entry from a shape with optional keys gives the generic array

**Impact: Low · Complexity: Low-Medium**

```php
/** @var array{a: 0, b: 1, c?: 2} $arr */
array_pop($arr);
$arr;   // array<string, 0|1|2>, should be array{a: 0, b?: 1}
```

`array_pop()` and `array_shift()` rebuild the shape when every entry is
required, but fall back to `after_unknown_removal` once one is optional,
though which entries can remain is still known.

**Fix:** when an optional entry sits at the removed end, take the union of
the shapes for each entry that may be the one removed. `// SKIP` lines in
`tests/phpstan_nsrt/array-pop.php` and `array-shift.php`.

### B788. Writing below a `mixed` element replaces every element's type

**Impact: Low · Complexity: Low-Medium**

```php
/** @param array<mixed> $data */
function f(array $data): void {
    foreach ($data as $k => $v) {
        $data[$k]['a'] = true;
        $data;   // array<array{a: true}>, should be non-empty-array<mixed>
    }
}
```

The write sets one element, which was `mixed` and may hold other keys, yet
the array's element type becomes the written shape for every element.

**Fix:** join the written element with the existing element type, and keep
`mixed` when the element was `mixed`. `// SKIP` line in
`tests/phpstan_nsrt/bug-13270a.php`.

### B789. A nested write under a dynamic key inside a loop is lost after the loop

**Impact: Medium · Complexity: Medium**

```php
$files[$ns] = [];
foreach ($helpers as $helper) {
    $files[$ns][$helper['name']] = $helper;
}
$files[$ns];   // array{}, should be array<string, …>
```

The assignment before the loop survives, and the writes inside it are
dropped, so the variable reads as always empty.

**Fix:** carry nested-dimension writes under a non-literal key out of the
loop's fixed-point walk the way single-dimension writes are, so they also
reach the loop head for the next pass. `// SKIP` lines in
`tests/phpstan_nsrt/bug-13629.php` and `bug-10438.php`.

### B794. An append after `unset()` of the last entry reuses its index

**Impact: Low · Complexity: Low-Medium**

```php
$a = [1, 2, 3];
unset($a[2]);
$a[] = 4;
$a;   // array{1, 2, 4}, should be array{0: 1, 1: 2, 3: 4}
```

PHP keeps an array's next free index when an entry is removed, so the
append lands at `3`. The shape left by the `unset()` is positional and has
forgotten that index.

**Fix:** have the shape remember the next free index once an `unset()` has
removed the entry that set it, and let an append read it. `// SKIP` line in
`tests/phpstan_nsrt/array-is-list-unset.php`.

### B797. Writing the offset after a known one turns a list into an array

**Impact: Low · Complexity: Low-Medium**

```php
/** @param list<int> $list */
function f(array $list): void {
    if (isset($list[5])) {
        $list[6] = 21;
    }
    $list;   // list<int>|non-empty-array<int, int>, should be list<int>
}
```

A write to an integer key a list may not have breaks its key order in
general, but once the previous index is known to exist the write extends the
list or overwrites an entry, and either way it stays a list.

**Fix:** keep a list a list when the written index is one past an index a
condition proved set. `// SKIP` lines in `tests/phpstan_nsrt/list-type.php`.

### B805. `array_values()` on a shape with optional keys gives a generic list

**Impact: Low · Complexity: Low**

```php
/** @var array{a?: 'a', b?: 'b', c?: 'c'} $numbers */
array_values($numbers);   // list<'a'|'b'|'c'>, should be list{0?: 'a'|'b'|'c', 1?: 'b'|'c', 2?: 'c'}
```

**Fix:** renumber the shape's entries, each position taking every value
that can land there. See `tests/phpstan_nsrt/array_values.php`.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

### B769. A template bound that names another template leaks that template's name

**Impact: Medium · Complexity: Medium**

```php
/**
 * @template T
 * @template U of SomeFoo<T>
 * @param U $foo
 * @return U
 */
function testSome($foo) {}

testSome($someBar);                    // SomeFoo<T>, should be SomeFoo<mixed>

/**
 * @template T of SomeFoo<U>
 * @template U
 * @return T
 */
function unwrapGenericWithoutParam() {}
unwrapGenericWithoutParam();           // SomeFoo<U>, should be SomeFoo<mixed>
```

When `U` cannot be inferred from the argument it falls back to its bound,
but the other template the bound names is left unsubstituted, so a raw
template name reaches hover and completion.

**Fix:** resolve the fallback bounds of a call's templates together, each
bound substituted with what the others resolved to (or their own bounds).
`// SKIP` lines in `tests/phpstan_nsrt/nested-generic-types.php`.

### B770. A template inferred from several closure parameters takes the first one

**Impact: Low · Complexity: Medium**

```php
/**
 * @template T as object
 * @param \Closure(T, T): void $outmaker
 * @return T
 */
function createProxy2(\Closure $outmaker): object {}

createProxy2(function (A $a, B $o): void {});   // A, should be B (B extends A)
```

A template in parameter positions is contravariant, so every position must
accept the bound type and the narrowest one is the answer. The binding
takes the first parameter it meets.

**Fix:** when a template is bound from more than one contravariant position,
keep the most specific type. `// SKIP` line in `tests/phpstan_nsrt/bug-3158.php`.

### B771. `@phpstan-self-out` skips `$this` and a `@return $this` result

**Impact: Low · Complexity: Medium**

```php
/** @template T of object */
class Foo {
    /**
     * @template U of object
     * @param class-string<U> $class
     * @return $this
     * @phpstan-self-out static<T&U>
     */
    public function add(string $class) { return $this; }
}

/** @param Bar<A> $bar */
function f(Bar $bar): void {
    $bar->add(B::class);    // Bar<A>, should be Bar<A&B>
    $bar;                   // Bar<A&B>
}
// and inside Bar: $this->add(A::class); $this  → Bar, should be Bar<A&T>
```

The self-out type is applied to a variable receiver after the call, but not
to the value the call returns when that is `$this`, nor to `$this` itself
when the method is called on it inside the class.

**Fix:** apply the self-out type to `$this` in the class body, and give a
`@return $this` call the receiver's post-call type. `// SKIP` lines in
`tests/phpstan_nsrt/bug-12575.php`.

### B772. Callables written as strings and arrays lose the target's signature

**Impact: Low · Complexity: Medium**

```php
'strlen'(...);                                   // Closure(): int<0, max>, should be Closure(string): int<0, max>
array_map([Enum::class, 'get'], ['test']);      // array, should be array{Enum}
$g = \Closure::fromCallable([$this, 'doFoo']);   // doFoo is @template T, @param T, @return T
$g(1);                                           // T, should be 1
```

`strlen(...)` and `$this->doFoo(...)` resolve to their targets, but the same
targets named through a string or a `[class, method]` array keep only part
of the signature: the parameters, the return type at the call site, or the
template binding.

**Fix:** resolve a string or array callable to the function or method it
names once, and hand that signature to first-class callable syntax,
`Closure::fromCallable()` and callback-taking functions alike. `// SKIP` lines
in `tests/phpstan_nsrt/first-class-callables.php` and `bug-4213.php`.

### B790. A template bound from a literal is generalized by the wrong rule

**Impact: Low · Complexity: Low-Medium**

```php
/** @template T */ class Foo { /** @param T $p */ public function __construct($p) {} }
/** @template-covariant T */ class FooCovariant { /** @param T $p */ public function __construct($p) {} }
/** @template T @param T $p @return Foo<T> */ function test2($p) {}

new Foo(1);            // Foo<int>, right
test2(1);              // Foo<1>, should be Foo<int>
new FooCovariant(1);   // FooCovariant<int>, should be FooCovariant<1>
```

A literal bound to an invariant template inside a generic type is widened
to its base type, and one bound to a covariant template is kept. `new`
widens regardless of variance, and a function's generic return widens
nothing.

**Fix:** apply one rule wherever a literal binds a template that ends up in
a generic argument: widen for an invariant template, keep for a covariant
one. `// SKIP` lines in `tests/phpstan_nsrt/generics-do-not-generalize.php`.

### B791. An inherited `@phpstan-assert` on a template leaves the variable without a type

**Impact: Medium · Complexity: Low-Medium**

```php
/** @template T */
interface WrapperInterface {
    /** @phpstan-assert T $param */
    public function assert(mixed $param): void;
}
/** @implements WrapperInterface<int> */
class IntWrapper implements WrapperInterface {
    public function assert(mixed $param): void {}
}

$test->assert($val);
$val;   // no type at all, should be int
```

`@phpstan-assert-if-true` inherited the same way narrows correctly, so the
gap is in applying the unconditional tag after a statement call, where the
template is not substituted and the narrowed type is lost entirely.

**Fix:** substitute the implementing class's template arguments into an
inherited `@phpstan-assert` before applying it. `// SKIP` line in
`tests/phpstan_nsrt/assert-inheritance.php`.

### B806. A generic function passed as a first-class callable to `array_map()` leaves its template unbound

**Impact: Low · Complexity: Medium**

```php
/** @param array<string, array<int>> $a */
array_map(array_values(...), $a);   // array<string, list<TValue>>, should be array<string, list<int>>
```

The callable's signature is read with its templates still open, and the
callback's parameter is never bound from the array's value type.

**Fix:** bind the first-class callable's templates from the parameter
types `array_map()` gives it. See `tests/phpstan_nsrt/array_values.php`.

### B810. A bare `class-string` argument leaves `class-string<T>`'s template unbound

**Impact: Low · Complexity: Low**

```php
/**
 * @template T
 * @param literal-string|class-string<T> $name
 * @return ($name is class-string ? T : mixed)
 */
function get(string $name) {}

/** @var class-string $class_string */
get($class_string);   // mixed, should be object
```

**Fix:** bind `T` to `object` from an unparameterised `class-string`. See
`tests/psalm_assertions/template_conditional_return_type.php`.

## Framework names in strings

No outstanding items.

## Name resolution

No outstanding items.

## Code actions

No outstanding items.

## Miscellaneous

### B792. A static property read through `$object::` has no type

**Impact: Medium · Complexity: Low**

```php
class Foo { /** @var string */ public static $s = ''; }
function f(Foo $foo): void {
    $foo::$s;   // no type, should be string (Foo::$s works)
}
```

**Fix:** resolve the class of `$object::` from the variable's type, as
`$object::method()` and `$object::CONST` do. `// SKIP` lines in
`tests/phpstan_nsrt/isset-coalesce-empty-type.php`.

### B793. `<<`, `>>` and `/` on literal ints are not folded

**Impact: Low · Complexity: Low**

```php
/** @param 1|3.0|"5" $e */
function f($e, int $x): void {
    $e << 1;      // int, should be 2|6|10
    true << 1;    // int, should be 2
    15.9 >> 1;    // int, should be 7
}
/** @param 20|40|60 $x @param 2|4 $y */
function g($x, $y) { $x / $y; }   // int|float, should be 5|10|15|20|30
```

`+`, `-`, `*` and the bitwise `&`, `|`, `^` fold literal operands and literal
unions; the shifts and division do not.

**Fix:** fold them over each pair of literal members, coercing operands the
way PHP does, and fall back to the current type when a member is not a
literal or the result is not exact. `// SKIP` lines in
`tests/phpstan_nsrt/integer-range-types.php`.

### B799. `T[]|Collection` on a native `Collection` return is not a collection of `T`

**Impact: Low-Medium · Complexity: Low-Medium**

```php
interface Collection extends \Traversable {}
class Bar {
    /** @return static[]|Collection */
    public function items(): Collection {}
}
function f(Bar $bar): void {
    $bar->items();                     // Collection, should be iterable<Bar>&Collection
    foreach ($bar->items() as $item) {
        $item;                         // mixed, should be Bar
    }
}
```

Before generics, `Foo[]|Collection` was the usual way to say "a collection of
`Foo`", and PHPStan still reads it that way when the other member is a
`Traversable` class: the union becomes the class intersected with
`iterable<Foo>`. PHPantom drops the array member because the native return
type rules it out, and with it the only thing that said what the collection
holds.

**Fix:** when a docblock union pairs an array type with a `Traversable`
class, read it as that class intersected with an iterable of the array's
value type before the native type is applied. `// SKIP` lines in
`tests/phpstan_nsrt/methodPhpDocs-trait.php` and
`methodPhpDocs-recursiveTrait.php`.

### B800. Inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it

**Impact: Medium · Complexity: Medium-High**

```php
class FooParent {
    /** @return static */
    public function doLorem() {}
    private function helper(): array {}
}
class FooWithTrait extends FooParent {
    use FooTrait;
    private function helper() {}
}
trait FooTrait {
    /** @param self $other */
    public function doFoo($other): void {
        $other;              // FooTrait&FooWithTrait&FooParent, should be FooWithTrait
        parent::doLorem();   // no type, should be static(FooWithTrait)
        static::doLorem();   // no type
        $this->helper();     // FooParent's private `array`, should be FooWithTrait's own
    }
}
```

PHP flattens a trait into each class that uses it, so inside its methods
`self`, `static`, `parent` and `$this` all refer to that class. PHPStan
analyses a trait body once per using class. PHPantom analyses it on its own,
standing `$this` in for the intersection of the trait with everything its
users share (`type_engine/trait_context.rs`); `self` gets the same
intersection, `parent::` and `static::` get nothing, and a member lookup on
the intersection can land on an ancestor's private method. A trait method's
`{@inheritdoc}` likewise never sees the using class's interface (PHPStan's
`inheritdoc-from-trait.php` in `nsrt/`, unported).

**Fix:** when a trait has a single using class (or a `@phpstan-require-extends`
bound), resolve `self`, `static`, `parent` and `$this` in its body against
that class, and look its members up with that class's visibility. With
several users, keep the shared-ancestor intersection for `$this` but resolve
`parent::` against the users' common parent. Fix C13 in
[completion.md](completion.md) alongside, since `self::`/`static::`
completion goes through the same path. `// SKIP` lines in
`tests/phpstan_nsrt/methodPhpDocs-trait.php` and
`methodPhpDocs-recursiveTrait.php`.

### B802. `::class` on a string literal has no type

**Impact: Low · Complexity: Low**

```php
'Foo'::class;   // no type, should be 'Foo'
```

PHP returns the string itself.

**Fix:** resolve `::class` on a string literal to that literal. See
`tests/phpstan_nsrt/class-constant-on-expr.php`.

### B812. A write to a property is not seen through a reference to it

**Impact: Low · Complexity: Medium**

```php
$bar = &$foo->bar;
$foo->bar = "bar";
$bar;   // string, should be 'bar'
```

References to locals are tracked; a reference to a property is not.

**Fix:** record `&$obj->prop` as a reference the way `&$local` is. See
`tests/psalm_assertions/reference.php`.
