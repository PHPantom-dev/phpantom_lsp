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

No outstanding items.

## Standard-library return types

### B778. Other builtins over literal arguments are not folded

**Impact: Low · Complexity: Low-Medium**

```php
'foo'[0];   // string, should be 'f'
```

The string functions that rewrite a literal, the numeric conversions
(`intval()` and friends, `abs()`, `pow()`), `str_increment()`,
`str_decrement()`, `strlen()`/`mb_strlen()`, `implode()`,
`array_sum()`/`array_product()` and `date()` fold, but an offset into a
literal string and the rest below do not. `mb_strlen()` with an encoding other than UTF-8 is not
measured either (`// SKIP` lines in `tests/phpstan_nsrt/mb-strlen-php83.php`).

**Fix:** fold each function over its literal arguments, one at a time; each
upstream fixture is the spec for its function.

**Upstream cases** (PHPStan `nsrt/`): `range-function.php` (11), `constantTypes.php` (8), `string-offsets.php` (8), 19 more.

### B910. `**` with an operand PHP rejects is not an error type

**Impact: Low · Complexity: Low-Medium**

```php
$int ** [];         // int|float, should be *ERROR*
'a' ** 1;           // int|float, should be *ERROR*
pow(-1, 5.5);       // float, should be NAN
```

`**` and `pow()` throw a `TypeError` for an array or plain object operand,
and for a non-numeric string, but the result is still typed as a number.
A result that is not a number (`NAN`) is a plain `float`. `// SKIP` lines in
`tests/phpstan_nsrt/pow.php`.

### B819. Built-ins whose result their arguments decide return the declared type

**Impact: Medium · Complexity: Medium**

```php
version_compare('7.0.0', '7.0.1');   // int, should be -1
mb_http_output();                    // string|bool, should be string
gettimeofday(true);                  // array<int>|float, should be float
$dt->format('');                     // string, should be ''
$px->getColor(1);                    // array{r: int|float, ...}, should be array{r: float, ...}
```

No argument-aware return rules for: version_compare, microtime(null), mb_http_output/mb_regex_encoding/mb_internal_encoding/mb_encoding_aliases/mb_chr/mb_ord, gettimeofday, DateTimeInterface::format literals, ImagickPixel::getColor, fpm_get_status, sscanf scansets, proc_open's $pipes, getopt, CachingIterator::getCache, get_called_class, array_find_key on literal keys. Each needs its own rule; grouped because the mechanism (a per-function return rule) is the same.

It also shows as:

- `openssl_encrypt()`'s `$tag` out-parameter ignores the cipher
- PECL extension calls whose return type PHPStan picks from the arguments use the stub's full union
- `PDO::connect()` does not return the driver subclass named by the DSN
- By-reference arrays of `socket_select()` and `proc_open()` are not typed from their inputs
- `stat()`, `lstat()`, `fstat()` and `ssh2_sftp_stat()` return shapes lack the numeric keys
- `ArrayObject` offset reads are not nullable
- `Closure::bind()`, `bindTo()` and `Closure::getCurrent()` lose the closure's signature
- `ReflectionEnum` methods are not specialized by the reflected enum
- `DatePeriod`'s constructor arguments do not decide its end-date and recurrence types
- `strtotime()` of a literal string is not folded
- `DateTimeInterface::format()` with a literal format is not typed by the format
- `sscanf()` with a literal format and no output arguments is not typed as a shape

**Fix:** add per-function rules (stub patches with conditional returns, or the array/function rule tables) one function at a time; each upstream fixture is the spec for its function.

**Cases** (PHPStan `nsrt/`, 358 assertions): `functions.php` (42), `array-functions.php` (23), `array-search.php` (20), `mb-convert-encoding-php8.php` (18), `sscanf.php` (18), `get-parent-class.php` (12), `pr-1244-php-84.php` (12), `DateTimeDynamicReturnTypes.php` (12), `DateTimeCreateDynamicReturnTypes.php` (10), `mb_substitute_character-php8.php` (10), `get-defined-vars.php` (10), `bug-3789.php` (8), 54 more.

### B820. `array_splice()` neither returns the removed slice nor updates its array

**Impact: Medium · Complexity: Low-Medium**

```php
/** @param array<int, int> $arr */
function f(array $arr): void {
    array_splice($arr, 0, 0, [1]);
    $arr;   // array<int, int>, should be non-empty-list<int>
    /** @var array{17: 'foo', b: 'bar', 19: 'baz'} $c */
    array_splice($c, 0, 1, ['hello']);
    $c;     // unchanged, should be array{0: 'hello', b: 'bar', 1: 'baz'}
}
```

The by-reference array keeps its pre-call type: int keys are not renumbered (so it never becomes a list), the replacement values are not merged in and removed entries stay. Applies to shapes, generic arrays and properties alike.

It also shows as:

- `array_splice()` returns the input array's type instead of the removed slice
- `array_splice()` leaves its by-reference array unchanged and returns a generic list

**Cases** (PHPStan `nsrt/`, 100 assertions): `array_splice.php` (91), `bug-5017.php` (6), `bug-14037.php` (2), `bug-11917.php`.

### B821. `preg_match()` gives a generic array for a pattern that is not one plain literal

**Impact: Medium · Complexity: Low-Medium**

```php
if (preg_match('{' . preg_quote($s) . '(z)}', $s, $m)) {
    $m; // got array<array-key, string>, should be array{non-falsy-string, 'z'}
} else {
    $m; // got array<array-key, string>, should be array{}
}
```

Concatenation with `preg_quote()`, a variable holding one of several literal patterns, a variable assigned a double-quoted pattern, and nowdoc/heredoc patterns all give the generic fallback, even in the no-match branch where the result is always `array{}`.

It also shows as:

- `preg_match()` gives a generic array for an `x`-flag pattern or a pattern held in a variable or constant
- A `preg_match()` pattern concatenated with a constant is not read
- `preg_match()` with a non-constant flags argument ignores the flags

**Cases** (PHPStan `nsrt/`, 46 assertions): `preg_match_shapes.php` (38), `bug-11311.php` (3), `bug-12242.php` (3), `bug-12211.php`, `bug11384.php`.

### B822. A `preg_match()` capture group that can only match fixed text is typed `string`

**Impact: Low · Complexity: Low-Medium**

```php
if (preg_match('/Price: (£|€)\d+/', $s, $m)) {
    $m; // got array{0: string, 1: string}, should be array{string, '£'|'€'}
}
```

The shape and its keys are right; only the capture values lose literals. PHPStan derives literal unions from alternations, character classes (`[157]`, `ba[rz]`), escaped literals and empty groups (`''`).

It also shows as:

- A capture group of literal text is typed `string` instead of its literal alternatives
- `preg_match()` capture groups that can only match fixed text are typed `string`, not the literal

**Cases** (PHPStan `nsrt/`, 58 assertions): `preg_match_shapes.php` (36), `preg_match_all_shapes.php` (12), `bug-11311.php` (6), `preg_replace_callback_shapes.php` (2), `bug-12210.php`, `bug-12173.php`.

### B823. `preg_match()` types a pattern whose alternatives capture different groups as one shape with optional keys instead of a union per alternative

**Impact: Low · Complexity: Medium**

```php
if (preg_match('~^(?:(\d+)x(\d+)|(\d+)|x(\d+))$~', $s, $m) === 1) {
    $m; // got array{0: string, 1?: string, 2?: string, 3?: string, 4?: string}
        // should be array{string, '', '', '', numeric-string}|array{string, '', '', numeric-string}|array{string, numeric-string, numeric-string}
}
```

PHPStan builds one shape per top-level alternative (unmatched earlier groups are `''`) and marks a shape whose later groups are optional as a `list{}`. PHPantom makes every group after the first optional key in a single `array{}` shape.

**Cases** (PHPStan `nsrt/`, 19 assertions): `preg_match_shapes.php` (19).

### B824. `preg_match()` gives a generic array for regex syntax its parser does not handle

**Impact: Low · Complexity: Medium**

```php
if (preg_match('/( \d+ )/x', $s, $m)) {
    $m; // got array<array-key, string>, should be array{non-empty-string, numeric-string}
}
```

Unsupported: the `x` modifier, inline modifier groups `(?i)` / `(?xs-i)`, duplicate names with `(?J)`, branch reset `(?|...)`, `\Q...\E`, and `(*MARK)` verbs (which also add a `MARK` key). The no-match branch then also loses `array{}`.

It also shows as:

- An inline `(?n)` flag makes the `preg_match()` result a generic array

**Cases** (PHPStan `nsrt/`, 20 assertions): `preg_match_shapes.php` (18), `preg_match_shapes_php80.php` (2).

### B829. `json_decode()` into arrays does not rule out objects

**Impact: Low · Complexity: Low**

```php
json_decode($mixed, true);                          // mixed, should be mixed~object
json_decode($mixed, null, 512, JSON_OBJECT_AS_ARRAY); // mixed, should be mixed~object
```

With objects decoded as arrays the result of an unknown string can never be
an object. `// SKIP` lines in `tests/phpstan_nsrt/json-decode-json_object_as_array.php`
and `json-decode-narrow_type_with_force_array.php`.

### B907. `get_debug_type()` of an object built with `new` returns `string`

**Impact: Low · Complexity: Low**

```php
$o = new \stdClass();
get_debug_type($o);   // string, should be 'stdClass'
```

`get_debug_type()` only names an object whose class is final (or anonymous),
since a variable typed with a class may hold a subclass instance. An object
built with `new` is exactly that class, but its type does not say so.
`// SKIP` line in `tests/phpstan_nsrt/get-debug-type.php`. The same gap keeps
`array_column()` over `[new Foo()]` reading a `protected` or `private`
property as possibly there (`// SKIP` lines in
`tests/phpstan_nsrt/array-column-php82.php`).

### B832. Array functions lose a shape merged with a generic array, an object key, or an array-or-string subject

**Impact: Low · Complexity: Low-Medium**

```php
/** @param array{foo: '1'} $a @param array<string, int> $b */
array_replace($a, $b);   // non-empty-array<string, '1'|int>, should also keep `foo`
array_fill_keys([new Foo()], 'b');   // array<Foo, 'b'>, should be array{foo: 'b'} from __toString()
$s = rand() ? [] : 'foo';
str_replace('a', 'b', $s);   // array<array-key, string>|string, should be array{}|string
```

The key-preserving array functions read shapes and generic arrays, but:

- A shape replaced or intersected with a generic array keeps none of the
  shape's keys (PHPStan's `hasOffset()`), so the result reads as a plain
  generic array.
- `array_fill_keys()` and `array_combine()` do not read an object key's
  `__toString()`.
- `str_replace()` and the `preg_` replacers on a subject that may be an array
  or a string lose the array's shape, and `preg_replace()` drops its
  subject's string refinements.

`// SKIP` lines in `tests/phpstan_nsrt/array-replace.php`,
`array-intersect-key.php`, `array-fill-keys.php`, `array-combine-php8.php`
and `replaceFunctions.php`.

**Upstream cases** (PHPStan `nsrt/`): `array-functions.php` (32), not yet ported.

### B833. Shuffling or chunking a constant array forgets how many entries it had

**Impact: Low · Complexity: Low-Medium**

```php
$arr = ['a' => 1, 'b' => 2, 'c' => 3];
shuffle($arr);
array_keys($arr);   // non-empty-list<int<0, max>>, should be non-empty-list<0|1|2>
```

`shuffle()` and `sort()` turn a shape into a list of its values, losing its
size, so the keys read back afterwards are any non-negative int. The same
gap shows when `array_chunk()` is handed a length range (`int<2, 3>`) rather
than a literal, and when `array_reverse()` renumbers a list shape with
optional entries. `// SKIP` lines in `tests/phpstan_nsrt/shuffle.php`,
`bug-6138.php`, `bug-10721.php`, `array-chunk.php` and `array-reverse.php`.

### B834. `array_filter()` with a callback neither filters a shape entry by entry nor narrows by the callback

**Impact: Medium · Complexity: Medium**

```php
/** @param array{one?: string, two?: string|null} $d */
array_filter($d, fn($v) => $v !== null);   // array<string, string|null>, should be array{one?: string, two?: string}
array_filter(['file1', 'file2'], 'file_exists');   // array<int, 'file1'|'file2'>, should be array{0?: 'file1', 1?: 'file2'}
```

With a callback the result degrades to a generic array of the original values; the shape should be kept with every key optional, and the value narrowed by the callback's truthy check (!== null, (bool) cast).

It also shows as:

- `array_filter()` with a callback that can never return true does not give `array{}`
- `array_filter()` with a callback on an array shape returns a generic array instead of filtering each entry
- UNCLEAR: a closure passed to `array_filter()` gets no parameter types
- An inline `array_find()` callback's key parameter is `string`, not the array's literal keys (`// SKIP` line in `tests/phpstan_nsrt/array-find.php`)

**Cases** (PHPStan `nsrt/`, 31 assertions): `bug-11730.php` (15), `bug-3132.php` (4), `array-filter-constant.php` (3), `array-filter-arrow-functions.php` (3), `array-functions.php` (2), `bug-7993.php`, `bug-11570.php`, `bug-6006.php`, `bug-7909.php`.

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
`bug-14369.php`, `bug-7913.php`, `comparison-operators.php`,
`conditional-types-inference.php`, `bug-9734.php` and
`strlen-int-range.php` are the cases; the unported
`array-filter-arrow-functions.php` and `array-filter-callables.php` in
PHPStan's `nsrt/` have the filter ones.

**Upstream cases** (PHPStan `nsrt/`, 67 assertions): `math.php` (28), `array-filter-callables.php` (3), `bug-4099-php8.php` (3), `array-is-list-type-specifying.php` (3), `functions.php` (3), `array-shape-list-optional.php` (2), `assert-docblock.php` (2), `bug-12376.php` (2), `bug-13481.php` (2), `count-type.php` (2), `bug-4434.php` (2), 14 more.

### B838. Loops nested more than six deep are not walked, so nothing inside them has a type

**Impact: High · Complexity: Medium**

```php
foreach ($a as $x1) { foreach ($b as $x2) { /* ... 9 levels ... */
    $this->prop;   // no type at all
} }
```

`MAX_LOOP_DEPTH = 6` in `forward_walk/loop_control.rs` skips any deeper loop body entirely, so every expression in it (even `$this->prop`) resolves to nothing. Per the perf guidelines the fix is a structural bound on loop re-walks, not a bigger cap.

**Fix:** replace the depth skip with a bound on re-walks that still walks the body once (see the performance anti-patterns: a depth cap that drops the body is the wrong tool).

**Cases** (PHPStan `nsrt/`, 30 assertions): `iterable.php` (30).

### B839. A variable assigned before a `throw` inside `try` has no type in `catch` and `finally`

**Impact: Medium · Complexity: Low-Medium**

```php
try {
    if (c()) { $foo = 1; throw new InvalidArgumentException(); }
    if (d()) { $foo = 2; throw new InvalidArgumentException(); }
} catch (InvalidArgumentException $e) {
    $foo;   // no type, should be 1|2
}
```

Assignments on paths that end in a throw are dropped instead of flowing into the matching catch block and finally.

**Fix:** join the `try` body's state at every statement that can throw into the `catch` and `finally` entry state, not only its end state.

**Cases** (PHPStan `nsrt/`, 4 assertions): `throw-points-try-catch.php` (4).

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

**Upstream cases** (PHPStan `nsrt/`, 23 assertions): `tagged-unions.php` (11), `bug-12401.php` (4), `bug-10544.php` (2), `bug-5128.php` (2), `bug-8724.php` (2), `bug-6383.php`, `bug-9908.php`.

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

**Upstream cases** (PHPStan `nsrt/`, 2 assertions): `bug-14108c.php`, `bug-14108.php`.

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

**Upstream cases** (PHPStan `nsrt/`, 12 assertions): `bug-11276.php` (8), `bug-13301-php8.php` (4).

### B840. Assigning to a property narrows it to the assigned value even where PHP coerces it or a hook intervenes

**Impact: Medium · Complexity: Medium**

```php
class A {
    private float $f; public int $i; private string $s;
    function m(int $n, string $str, mixed $m): void {
        $this->f = $n;   $this->f;   // int, should be float
        $this->i = $str; $this->i;   // string, should be int (weak mode coerces)
        $this->s = $m;   $this->s;   // mixed, should be string
    }
}
```

Property narrowing after an assignment stores the right-hand type verbatim. The stored type should be the assigned type intersected with (or, under weak typing, coerced to) the native property type, e.g. `mixed` -> declared type, `int` -> `float`, `'123'` -> `123`.

It also shows as:

- Assigning to a natively typed property does not apply the property's type
- Assigning to a hooked, virtual or magic property narrows later reads to the assigned value
- Assigning to a typed property keeps the assigned value's type even when PHP coerces it

**Cases** (PHPStan `nsrt/`, 73 assertions): `bug-12393b.php` (61), `bug-12393.php` (8), `property-hooks.php` (2), `bug-9062.php`, `bug-12393b-php84.php`.

### B841. `is_subclass_of()` and `is_a()` with class strings do not narrow their subject

**Impact: Medium · Complexity: Low-Medium**

```php
/** @param string|object $a */
function f($a) {
    if (is_subclass_of($a, 'DateTimeInterface')) {
        $a;         // string|object, should be class-string<DateTimeInterface>|DateTimeInterface
        new $a();   // no type, should be DateTimeInterface
    }
}
/** @param class-string<A>|class-string<B> $c */
if (!is_a($c, B::class, true)) { $c; }   // both, should be class-string<A>
```

No narrowing rule for is_subclass_of() (with or without allow_string), nor for negated is_a(..., true) on a union of class-strings; downstream `new $a()` and `$a::f()` lose their types as a result.

It also shows as:

- `is_subclass_of()` with a `class-string<T>` second argument does not narrow to `class-string<T>`

**Cases** (PHPStan `nsrt/`, 36 assertions): `generic-class-string.php` (15), `specifiedTypesUsingIsFunctions.php` (14), `bug-6687.php` (3), `bug-7987.php` (2), `bug-6704.php`, `bug-6698.php`.

### B842. A loose `==` or `!=` with `null`, `false`, `0` or `''` narrows by strict rules or not at all

**Impact: Medium · Complexity: Medium**

```php
/** @param 0|0.0|1|''|'0'|'x'|array{}|bool|object|null $x */
if ($x == false) {
    $x;   // false, should be 0|0.0|''|'0'|array{}|false|null
} 
if ($x == null) {} else {
    $x;   // unchanged, should be 1|'0'|'x'|object|true
}
```

The == false branch keeps only `false` (as if ===) and the else branches of == null/0/'' remove nothing; PHP 8 loose-comparison tables are needed per literal (null/false/0/'').

It also shows as:

- A loose `== ''` comparison does not narrow out `false`

**Cases** (PHPStan `nsrt/`, 18 assertions): `equal-narrow.php` (17), `bug-10338.php`.

### B843. A type check does not narrow a variable that is undefined or has no resolved type

**Impact: Medium · Complexity: Low**

```php
if (!is_int($integer)) { throw new \Exception(); }
$integer;   // no type, should be int
$foo = doFoo();   // unknown function
if (!is_a($foo, 'Foo')) { return; }
$foo;       // no type, should be Foo
```

The narrowing applies to an existing type; when the variable has none (never assigned, or assigned from an unresolvable call) the check produces nothing instead of the checked type.

**Cases** (PHPStan `nsrt/`, 16 assertions): `specifiedTypesUsingIsFunctions.php` (15), `switch-instanceof-truthy.php`.

### B844. `key_exists()` does not narrow like `array_key_exists()`

**Impact: Low · Complexity: Low**

```php
$a = ['foo' => 2, 3 => 'bar'];
if (key_exists($key, $a)) { $key; }   // string, should be '3'|'foo'
/** @param array<string, mixed> $b */
if (key_exists($k, $b)) { $k; }       // int|string, should be string
```

key_exists() is an alias of array_key_exists() but no narrowing is applied at all; the alias should share the array_key_exists() rule (B782 then applies to the declared-key-type cases).

**Fix:** register `key_exists` wherever `array_key_exists` is matched by name.

**Cases** (PHPStan `nsrt/`, 12 assertions): `key-exists.php` (12).

### B845. `@phpstan-assert string[] $arr` narrows an array parameter to a bare `array`

**Impact: Low · Complexity: Low**

```php
/** @param mixed[] $arr @phpstan-assert string[] $arr */ function validate(array $arr): void {}
/** @param mixed[] $arr */ function f(array $arr) { validate($arr); $arr; } // got array, should be array<string>
```

The asserted element type is dropped. Same for `-if-true`/`-if-false` forms and `=non-empty-array<int>`.

**Cases** (PHPStan `nsrt/`, 8 assertions): `assert-docblock.php` (8).

### B846. Comparing `get_class()` or `get_parent_class()` with a class name does not narrow the object

**Impact: Low · Complexity: Low**

```php
if (get_parent_class($o) === A::class) { $o; } // got object, should be A
if (get_parent_class($s) === A::class) { $s; } // got string, should be class-string<A>
```

With a mixed argument the branch shows no type at all (should be `A|class-string<A>`).

It also shows as:

- Comparing `get_class($x)` with a class string does not narrow `$x`
- `get_class($x) === $classString` does not narrow `$x`

**Cases** (PHPStan `nsrt/`, 10 assertions): `bug-5552.php` (8), `bug-6294.php`, `bug-2580.php`.

### B847. A `throw` on one side of `?:`, `||`, `&&` or `??` does not narrow the other side

**Impact: Medium · Complexity: Low**

```php
function f(?string $data): void {
    $data === null ? throw new \Exception() : $data;
    $data;   // ?string, should be string
    $data ?? throw new \Exception();
    $data;   // ?string, should be string
}
```

An expression statement whose one branch is a `throw` should leave the scope of the other branch's condition, as an `if (...) throw` does.

It also shows as:

- `$x ?? throw` (or a never-returning right side) does not narrow `$x` to non-null

**Cases** (PHPStan `nsrt/`, 11 assertions): `bug-6870.php` (7), `bug-4885.php` (2), `bug-5351.php` (2).

### B848. `switch` narrows its subject less than the equivalent `if` chain

**Impact: Medium · Complexity: Low-Medium**

```php
switch (true) { case $a === null: exit; }
$a;   // Foo|null, should be Foo
/** @var 1|2|3 $b */ switch ($b) { case 1: exit; }
$b;   // 1|2|3, should be 2|3
switch (true) { case is_int($x): break; case doFoo(): $x; }   // string|int, should be string
```

A case that always exits does not remove its condition after the switch, and a later case does not exclude the earlier cases that did not match.

**Cases** (PHPStan `nsrt/`, 7 assertions): `bug-13211.php` (6), `switch-type-elimination.php`.

### B849. `preg_match_all()` and a stored `preg_match()` result do not narrow `$matches`

**Impact: Low · Complexity: Low**

```php
if (preg_match_all('/ab(?P<num>\d+)?/', $s, $matches)) {
} else {
    $matches;   // array{0: list<string>, num: list<string>, 1: list<string>}, should be array{}
}
$matches;       // should be array{}|array{0: ..., ...}
```

preg_match() already narrows $matches in the failing branch (bug-11293 gives array{}), but preg_match_all() in a truthy, `!= false` or `== true` condition does not.

It also shows as:

- Checking a variable that holds `preg_match()`'s result does not narrow `$matches`

**Cases** (PHPStan `nsrt/`, 11 assertions): `preg_match_all_shapes.php` (6), `bug-11699.php` (5).

### B850. `@phpstan-assert` is not applied when the function is called through a callable variable

**Impact: Low · Complexity: Low**

```php
$f1 = is_positive_int(...);
$f2 = 'ns\is_positive_int';
if ($f1($v)) { $v; }   // mixed, should be int (int<1, max>)
if ($f2($v)) { $v; }   // mixed, should be int
```

Assertions are read only for direct calls; a first-class callable or a literal function-name string held in a variable resolves the return type but not the assertion.

**Cases** (PHPStan `nsrt/`, 6 assertions): `bug-14249.php` (6).

### B851. Negating a multi-variable `isset()` narrows every variable to `null`

**Impact: High · Complexity: Low**

```php
function f(?DateTimeImmutable $d, ?int $c) {
    if (isset($d, $c)) { throw new Exception(); }
    $d;   // null, should be DateTimeImmutable|null
    $c;   // null, should be int|null
}
```

!isset($a, $b) means at least one is null, not all; the false branch applies the per-variable negation to each one. Later checks then collapse to never.

**Fix:** a failed `isset($a, $b)` proves only that one of them is unset: narrow nothing to `null`, or narrow to the union of each variable's null alternative.

**Cases** (PHPStan `nsrt/`, 6 assertions): `bug-8366.php` (6).

### B852. Narrowing a float by `=== 0.0` or `== 0` gives the int `0` and forgets `-0.0`

**Impact: Low · Complexity: Low**

```php
function f(float $n): void {
    if ($n === 0.0) { $n; (string) $n; }   // 0 and '0', should be 0.0 and '-0'|'0'
    if ($n == 0) { $n; }                   // float, should be 0.0
}
```

The narrowed literal is stored as an int literal, the loose comparison with an int does not narrow a float, and the string cast assumes positive zero although `-0.0 === 0.0`.

**Cases** (PHPStan `nsrt/`, 6 assertions): `bug-12225.php` (6).

### B853. `array_is_list()` does not narrow its argument to a list

**Impact: Low · Complexity: Low**

```php
/** @param array<mixed> $a */
if (array_is_list($a)) { $a; }   // array<mixed>, should be list<mixed>
```

No type-specifying rule for `array_is_list()`; a `mixed` argument even loses its type in the true branch.

**Cases:** the `// SKIP` lines in `tests/phpstan_nsrt/bug-9734.php`, and `array-is-list-type-specifying.php` (3) in PHPStan's `nsrt/`.

### B854. The negation of an array-element-type assertion narrows `mixed[]` to `never`

**Impact: Medium · Complexity: Low**

```php
/** @param mixed[] $arr @phpstan-assert-if-true string[] $arr */ function isStrings(array $arr): bool {}
if (!isStrings($arr)) { $arr; } // got never, should be non-empty-array<mixed>
```

Removing `array<string>` from `array<mixed>` should leave a non-empty array (an empty array would have passed). With both `-if-true` and `-if-false` assertions the true branch also becomes `never`.

**Cases** (PHPStan `nsrt/`, 5 assertions): `assert-docblock.php` (5).

### B855. `count()` comparisons narrow shapes and unions wrongly or not at all

**Impact: Medium · Complexity: Low-Medium**

```php
/** @param array{0: string, 1?: string} $a */
if (count($a) <= 1) { return; }
$a;   // array{0: string, 1?: string}, should be array{string, string}
/** @param array{}|array{0: string, 1: string, 2?: string} $b */
if (count($b) === 3) { return; }
$b;   // array{}|array{0: string, 1: string}, should keep 2?
if (count($mixed) === 0) { $mixed; }   // no type, should be array{}|Countable
```

Size comparisons do not make optional keys required when the count rules out the shorter form, a failed `=== n` strips the optional key instead of keeping the shape, `> 1` does not remove a one-element member of a union, and a `mixed` subject loses its type.

It also shows as:

- Comparing `count()` with a variable int union or range does not narrow the array
- `count($this->prop) === 0` after `array_shift($this->prop)` in a `while (count(...) > 0)` loop narrows to `never`

**Cases** (PHPStan `nsrt/`, 10 assertions): `bug11480.php` (4), `count-type.php` (3), `bug-14314.php`, `bug-3558.php`, `bug-4558.php`.

### B856. A ternary or `?:` of type checks used as a condition does not narrow

**Impact: Low · Complexity: Low**

```php
/** @param resource|object $c */
assert(rand(0, 1) ? $c instanceof Foo : $c instanceof Bar);
$c;   // resource|object, should be Foo|Bar
assert($c instanceof Foo ?: $c instanceof Bar);   // same
```

Conditions are narrowed through &&, ||, ! but not through a ternary whose branches are both checks (and the short ternary form).

**Cases** (PHPStan `nsrt/`, 4 assertions): `bug-14100.php` (4).

### B857. Comparing a `class-string<A|B>` with `X::class` does not narrow

**Impact: Low · Complexity: Low**

```php
/** @param class-string<Car|Bike|Boat> $s */
if ($s === Car::class) { $s; return; }   // whole union, should be Car's class string
$s;                                      // whole union, should be class-string<Bike|Boat>
```

Identity comparison with a `::class` literal does not pick or remove the matching member of a class-string union.

**Cases** (PHPStan `nsrt/`, 4 assertions): `bug-9534.php` (4).

### B858. Assigning a property through destructuring or as a `foreach` target does not narrow it

**Impact: Low · Complexity: Low**

```php
[$this->foo] = [1];
$this->foo;   // int, should be 1
foreach ([1, 2, 3] as $this->foo) { $this->foo; }   // int, should be 1|2|3
```

Property narrowing is recorded for `$this->p = expr` but not when the property is a list()/[...] element or a foreach key/value target.

**Cases** (PHPStan `nsrt/`, 4 assertions): `array-destructuring-types.php` (4).

### B859. `@phpstan-assert-if-true` misses properties of arguments, `!== true`, nullsafe calls and intersection receivers

**Impact: Low · Complexity: Low-Medium**

```php
/** @phpstan-assert-if-true !null $a->foo */ function check(A $a): bool {}
if (check($a1)) { $a1->foo; } // got string|null, should be string
```

Also through a boolean variable holding the call's result.

It also shows as:

- `@phpstan-assert-if-true` on `$this` is not applied through `!== true` or a nullsafe call
- A call on an intersection receiver applies neither member's `@phpstan-assert`

**Cases** (PHPStan `nsrt/`, 6 assertions): `bug-10040.php` (3), `bug-12866.php` (2), `assert-intersected.php`.

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

**Upstream cases** (PHPStan `nsrt/`, 70 assertions): `preg_match_shapes.php` (26), `bug11480.php` (16), `isset.php` (7), `bug-6936-limit.php` (3), `bug-11311.php` (3), `preg_match_shapes_php82.php` (3), `overwritten-arrays.php` (3), `constant-array-union-unshift.php` (2), `tagged-unions.php` (2), `bug-9908.php`, `constant-array-optional-set.php`, `bug-2001-php8.php`, 2 more.

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

**Upstream cases** (PHPStan `nsrt/`, 8 assertions): `bug-9753.php` (3), `bug-8924.php` (3), `bug-5316.php`, `bug-12274.php`.

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

**Upstream cases** (PHPStan `nsrt/`, 10 assertions): `bug-13857.php` (7), `bug-6173.php` (2), `bug-optionalArrayKeyInMultiDimArrayLoop.php`.

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

**Upstream cases** (PHPStan `nsrt/`, 9 assertions): `bug-12078.php` (3), `bug-10640.php` (2), `bug-8015.php` (2), `bug-10025.php` (2).

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

**Upstream cases** (PHPStan `nsrt/`, 23 assertions): `bug-12274.php` (10), `bug-14245.php` (10), `for-loop-expr.php` (2), `bug-10089.php`.

### B805. `array_values()` on a shape with optional keys gives a generic list

**Impact: Low · Complexity: Low**

```php
/** @var array{a?: 'a', b?: 'b', c?: 'c'} $numbers */
array_values($numbers);   // list<'a'|'b'|'c'>, should be list{0?: 'a'|'b'|'c', 1?: 'b'|'c', 2?: 'c'}
```

**Fix:** renumber the shape's entries, each position taking every value
that can land there. See `tests/phpstan_nsrt/array_values.php`.

**Upstream cases** (PHPStan `nsrt/`, 3 assertions): `bug-14297.php` (3).

### B860. References into array elements are not tracked

**Impact: Medium · Complexity: Medium**

```php
$a = 1;
$b = [&$a];
$b[0] = 2;
$a; // got 1, should be 2
```

References to locals are tracked, but not when the `&$var` sits inside an array literal. Neighbour of B812 (references to properties).

It also shows as:

- Passing an array offset to a by-reference parameter does not update the offset
- Inside a by-reference `foreach`, a write to the value is not seen through the array
- Passing a variable to a closure's by-reference parameter keeps its old type

**Cases** (PHPStan `nsrt/`, 38 assertions): `bug-14333.php` (23), `bug-5077.php` (6), `overwritten-arrays.php` (4), `bug-11846.php` (2), `bug-5615.php` (2), `bug-11322.php`.

### B861. `++`, `--` and compound assignment on an array offset or property do not change the stored type

**Impact: Medium · Complexity: Low**

```php
$a = ['a' => 1];
$a['a']++;
$a;   // array{a: 1}, should be array{a: 2}
$this->p = 1; $this->p++;
$this->p;   // 1, should be 2
foreach ($xs as $k) { $counts[$k] ??= ...; $counts[$k]++; }   // values stay 1
```

The increment of a non-variable lvalue is not written back, so offset/property types keep the pre-increment literal (and loops never widen them). Static properties also lose the earlier `self::$p = 1` entirely.

It also shows as:

- `++` and compound assignment on an array offset do not change the offset's type
- A compound assignment (`.=`) to a property or static property does not update its narrowed type

**Cases** (PHPStan `nsrt/`, 21 assertions): `has-offset-type-bug.php` (7), `constantTypes.php` (5), `throw-points-assign-op.php` (4), `shopware-connection-profiler.php` (3), `array-keys-branches.php`, `generalize-scope.php`.

### B862. Array keys are not normalised the way PHP casts them

**Impact: Low · Complexity: Low**

```php
foreach (['foo' => 1, 'two', 'three'] as $k => $v) { $k; }  // 'foo'|1|2, should be 0|1|'foo'
['1' => 'a', '2' => 'b'];                 // keys string|'1', should be 1|2
[1 => 'a', 2.5 => 'b'];                   // array-key, should be 1|2
[STRING_ONE => 'x'];                      // define()d '1' gives string, should be 1
['id' => null, $key => 's'];              // $key is 'foo'|'bar': generic array, should be array{id: null, foo?: 's', bar?: 's'}
```

The implicit index after a string key, numeric-string and float keys cast to int, `define()`d constant keys, and a literal-union key are not handled when building the literal's type.

It also shows as:

- A `bool` offset is not cast to `0`/`1` when writing an array

**Cases** (PHPStan `nsrt/`, 9 assertions): `literal-arrays-keys.php` (6), `constant-array-type-set.php` (2), `bug-9907.php`.

### B863. Destructuring a nullable shape with `list()` gives the variables no type

**Impact: Low · Complexity: Low**

```php
list($month, $day, $year) = sscanf($date, '%s %d %d');   // array{...}|null
$month;   // no type, should be string|null
```

`sscanf()` itself resolves to the right shape; destructuring a `shape|null` value leaves every target untyped instead of reading each offset (null offsets give null).

**Cases** (PHPStan `nsrt/`, 6 assertions): `sscanf.php` (6).

### B864. Writes to an array's keys inside a foreach replace the pre-loop value instead of joining with it

**Impact: Low · Complexity: Low-Medium**

```php
$pre = ['a' => null];
foreach ($arr as $v) { $pre['a'] = $v['a']; } // $v['a']: string
$pre['a']; // got string, should be string|null
```

The loop may run zero times, so after it each written key holds its old value or the written one. PHPStan also correlates the keys (if `a` is still null, so is `b`).

**Cases** (PHPStan `nsrt/`, 5 assertions): `bug-7921.php` (5).

### B865. Writing to an array under the `foreach` loop's own key does not replace the element type

**Impact: Low · Complexity: Low**

```php
/** @param array<int, string> $a */
foreach ($a as $k => $v) { $a[$k] = 1; }
$a;   // array<int, string|1>, should be array<int, 1>
```

A write under the current iteration key overwrites that element, and after the loop every element has been overwritten; PHPantom joins the written value with the old element type instead.

**Cases** (PHPStan `nsrt/`, 4 assertions): `overwritten-arrays.php` (3), `bug-13730.php`.

### B866. A class constant array whose keys or values are other constants is typed `array`

**Impact: Low · Complexity: Low**

```php
const A = 'a'; const B = 'b';
const MAP = [self::A => [self::B]];
self::MAP;            // array, should be array{a: array{'b'}}
self::MAP[self::A];   // mixed, should be array{'b'}
```

The constant initializer is not evaluated when its keys/values are themselves class constants, so the whole array degrades.

**Cases** (PHPStan `nsrt/`, 4 assertions): `bug-7621-1.php` (4).

### B867. Array `+` loses the operands' shapes

**Impact: Low · Complexity: Low**

```php
$a = rand(0,1) ? [1, 1, 1] : [1, 1, 1, 2, 3];
$b = rand(0,1) ? ['x', 'y', 1] : ['x', 'y', 1, 1, 1];
$a + $b; // got array, should be the union of each pair's union
```

PHPStan computes the array union for each combination of shapes.

It also shows as:

- Array `+` with an operand of unknown element type takes the other operand's types
- The array `+` operator with a shape on the left loses the shape's entries

**Cases** (PHPStan `nsrt/`, 4 assertions): `constant-array-optional-set.php` (2), `bug-7492.php`, `bug-11912.php`.

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

**Upstream cases** (PHPStan `nsrt/`, 28 assertions): `call-user-func-array-php8.php` (10), `call-user-func-php8.php` (7), `call-user-func_array.php` (6), `call-user-func.php` (4), `bug-12691.php`.

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

**Upstream cases** (PHPStan `nsrt/`, 6 assertions): `bug-10254.php` (4), `bug-7078.php`, `self-out.php`.

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

**Upstream cases** (PHPStan `nsrt/`, 6 assertions): `assert-methods.php` (5), `assert-class-type.php`.

### B813. A `static` return through a `$this` narrowed to an intersection drops the other members

**Impact: Low · Complexity: Low**

```php
abstract class Ipsum implements Foo {
    public function testThis(): void {
        if ($this instanceof Bar) {
            $this->returnStatic();   // static(Ipsum), should be static(Ipsum)&Bar
        }
    }
}
```

`$this` itself is narrowed to `Ipsum&Bar`, but a forwarding call binds
`static` to the enclosing class alone, so the `Bar` the check proved is lost.

**Fix:** bind `static` over the narrowed `$this` the way an intersection
receiver variable already is. See `tests/phpstan_nsrt/intersection-static.php`.

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

**Upstream cases** (PHPStan `nsrt/`, 4 assertions): `bug-12691.php` (3), `bug-11861.php`.

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

### B868. The callback of `array_diff_uassoc()`, `array_udiff()`, `array_intersect_ukey()` and friends gets no parameter types

**Impact: Medium · Complexity: Low**

```php
array_diff_ukey([1, 2], [3, 4], static function ($a, $b) {
    $a; // got nothing, should be 0|1
    return 0;
});
```

The callback comes after a variadic list of arrays, so it is not matched to a callable parameter. Hover on its parameters shows nothing at all.

**Fix:** link each callback parameter to the arrays' element (or key) type, as `usort()` already does.

**Cases** (PHPStan `nsrt/`, 24 assertions): `array_diff_intersect_callbacks.php` (24).

### B869. A callable on the right of `|>` gets nothing from the piped value

**Impact: Medium · Complexity: Low-Medium**

```php
doFoo() |> function ($x) {   // doFoo(): positive-int
    $x; // got nothing, should be int<1, max>
};
$ints |> (fn ($x) => array_map(fn ($i) => $i, $x)); // got array, should be array<int<1, max>>
```

Even a closure with a native `int $x` parameter shows nothing inside its body. Because the parameter is untyped, everything computed from it inside the closure (array_map, array_filter, conditional returns, array_find_key) degrades too.

It also shows as:

- A first-class callable on the right of `|>` does not bind the piped value as its argument

**Cases** (PHPStan `nsrt/`, 22 assertions): `pipe-operator.php` (22).

### B870. An immediately invoked closure gets no parameter types from its arguments

**Impact: Medium · Complexity: Low-Medium**

```php
(function ($context) { $context; })($integer);   // no type, should be int
(fn($a, $b) => [$a, $b])($i, $arr);              // array{mixed, mixed}, should be array{int, array{a: int}}
```

Closure parameter inference handles closures passed to typed callable parameters; a closure called directly (`(function ($x) {...})($arg)`, also `($cb = fn...)(...)`) should take its parameter types from the call's arguments. Related to T47 but not covered by it.

It also shows as:

- An immediately invoked closure gets no parameter types from its arguments

**Cases** (PHPStan `nsrt/`, 25 assertions): `pipe-operator.php` (10), `closure-argument-type.php` (9), `arrow-function-argument-type.php` (6).

### B871. Inside its declaring scope a template-typed value shows the template's bound instead of the template

**Impact: Low · Complexity: Low-Medium**

```php
/** @template T of A|B|C */
abstract class HelloWorld {
    /** @return T */ abstract protected function getObject(): A|B|C;
    function f() { $o = $this->getObject(); $o; } // got A|B|C, should be T of A|B|C
}
```

Inside the class or function that declares it, a template is replaced by its bound, so narrowing gives the bound's members (`C` instead of `T of C`). Mostly a display difference for hover, but it also loses the link back to `T` for return-type checks. bug-6591.php additionally shows the bound kept beside the narrowed type after the branches join (`UpdatableInterface|T`).

**Cases** (PHPStan `nsrt/`, 12 assertions): `bug-6566-types.php` (6), `bug-6591.php` (4), `bug-12989.php`, `bug-9939.php`.

### B872. Generic callable types (`Closure<T>(T): T`) are not supported

**Impact: Low · Complexity: Medium**

```php
/** @param Closure<TRet of mixed>(TRet $val): TRet $c */
function f(Closure $c, int $i) {
    $c;       // Closure, should be Closure<TRet>(TRet): TRet
    $c($i);   // mixed, should be int
}
```

Callable/Closure types with their own template list are not parsed, so the signature is lost and calls cannot bind the per-call template (also when returned from a generic function or nested).

**Cases** (PHPStan `nsrt/`, 12 assertions): `generic-callables.php` (12).

### B873. `@param-closure-this` is not inherited, and its template is not bound from a `class-string<T>`

**Impact: Low · Complexity: Low**

```php
class Foo { /** @param-closure-this Some $cb */ public function run(callable $cb) {} }
class Child extends Foo {
    public function run(callable $cb) {}
    function t() { $this->run(function () { $this; }); } // got Child, should be Some
}
```

When the override has no docblock, or a docblock with only a plain `@param callable`, the closure's `$this` falls back to the enclosing class.

It also shows as:

- A template `@param-closure-this T` is not bound from a `class-string<T>` argument
- A conditional `@param-closure-this` type is not decided by the call's arguments

**Cases** (PHPStan `nsrt/`, 12 assertions): `param-closure-this.php` (12).

### B874. `static<…>` on an inherited method fills the calling class's templates positionally instead of through `@implements`/`@extends`

**Impact: Low · Complexity: Low-Medium**

```php
/** @template T @template U */ interface Foo { /** @return static<T, U> */ function fluent(); }
/** @template T @implements Foo<float, T> */ abstract class I2 implements Foo {}
/** @param I2<int> $s */ $s->fluent();   // I2<float, int>, should be I2<int>
/** @extends A<int, string> */ class B extends A {}
(new B)->doFoo();                       // B<int, string>, should be B
```

The arguments of `static<...>` are written in the declaring class's template order and must be mapped back through the `@implements`/`@extends` arguments to the calling class's own templates (none for a non-generic subclass).

**Cases** (PHPStan `nsrt/`, 8 assertions): `generic-static.php` (8).

### B875. A value typed by an enclosing template turns into the template's bound when it flows through a call

**Impact: Low · Complexity: Low-Medium**

```php
/** @template T of DateTime|DateTimeImmutable @param T $d @return T */
function m($d) {
    $d = $d->modify('+1 day');   // DateTime|DateTimeImmutable, should be T
}
// passing class-string<T> to a callee @template U @param class-string<U> @return U gives T's bound, should be T
```

A `static` return on a template-typed receiver, and a callee template bound from an outer template, are both resolved through the bound instead of keeping the outer template.

**Cases** (PHPStan `nsrt/`, 5 assertions): `bug-7987.php` (3), `bug-6609-83.php` (2).

### B876. `key-of<>` and `value-of<>` stay unresolved for constant arrays keyed by class constants or for non-array arguments

**Impact: Low · Complexity: Low**

```php
private const ALL = [self::JFK => '...', self::LGA => '...'];
/** @param key-of<self::ALL> $code */ $code;   // key-of<self::ALL>, should be 'jfk'|'lga'
/** @param key-of<'jfk'> $code */ $code;      // key-of<'jfk'>, should be string
```

Keys written as `self::CONST` are not folded, and an argument that is not an array should fall back to the native parameter type rather than display the raw tag.

**Cases** (PHPStan `nsrt/`, 5 assertions): `key-of.php` (3), `value-of.php` (2).

### B877. `::cases()` through a template or an enum-typed value returns `array<UnitEnum>` or `list<Enum>` instead of `list<T>` or the case shape

**Impact: Low · Complexity: Low**

```php
/** @template T of \UnitEnum @param class-string<T> $e */
$e::cases();    // array<UnitEnum>, should be list<T>
function f(Test $c) { $c::cases(); }   // list<Test>, should be array{Test::ONE}
```

`cases()` called on a class-string or instance whose class is a template (or a concrete enum reached through an instance) does not get the enum-aware return type.

**Cases** (PHPStan `nsrt/`, 4 assertions): `bug-7162.php` (2), `bug-11233.php` (2).

### B878. A template inferred through a union parameter type (`T|Closure(): T`) loses literals

**Impact: Low · Complexity: Low**

```php
/** @template TValue @template TArgs @param TValue|\Closure(TArgs): TValue $value @return TValue */ function value($value, ...$args) {}
value('foo');       // got string, should be 'foo'
value(fn () => 42); // got int, should be 42
```

A literal bound directly to `@param T` is kept (see `collect('a', 'b')`), but through the union parameter it is widened. Possibly the same generalization rule as B790.

It also shows as:

- A template in a union with a concrete type (`T|string`) binds the whole argument instead of the remainder

**Cases** (PHPStan `nsrt/`, 6 assertions): `bug-2572.php` (4), `generics-reduce-types-first.php` (2).

### B879. A property typed by a template whose bound mixes classes and scalars keeps only the class members inside the class

**Impact: Low · Complexity: Low**

```php
/** @template T of string|\Stringable|array<string|\Stringable>|Combinator|null */
class CssValue {
    /** @param T $value */ public function __construct(private readonly mixed $value) {}
    function f() { $this->value; } // got Stringable|Combinator, should be T of array<...>|Combinator|string|Stringable|null
}
```

The scalar, array and null members of the bound are dropped, so `is_array()` narrowing gives bare `array`. This is a wrong type, not only the template-label display difference.

**Cases** (PHPStan `nsrt/`, 4 assertions): `bug-9939.php` (4).

### B880. A generic class extending `FilterIterator` built from a closure's generator leaves the generator's template names unresolved

**Impact: Low · Complexity: Low-Medium**

```php
$gen = static function (): Generator { yield true => true; };
$it = new FooFilterIterator($gen()); // @extends FilterIterator<TKey, T, Iterator<TKey, T>>
$it->key(); // got TKey, should be bool
```

Raw template names (`TKey`, `TYield`) reach hover. The generator's key and value types are inferred from the `yield`s by PHPStan; PHPantom neither infers them nor falls back to the bound.

**Cases** (PHPStan `nsrt/`, 4 assertions): `bug-7519.php` (4).

### B881. `@no-named-arguments` does not make a variadic parameter a list

**Impact: Low · Complexity: Low**

```php
/** @no-named-arguments */ function f(float ...$args) { $args; } // got array<int|string, float>, should be list<float>
```

Also when the tag sits on the parent method or interface method being implemented.

**Cases** (PHPStan `nsrt/`, 4 assertions): `no-named-arguments.php` (4).

### B882. `array_map()` takes its callback's types from the first array or callable member only

**Impact: Medium · Complexity: Medium**

```php
array_map(function ($a, $b) { $b; }, ['foo' => $i], ['bar' => $s]); // got nothing, should be string; result got list<int|mixed>, should be non-empty-list<int|string>
```



It also shows as:

- `array_map()` with a union of callables uses only the first member's return type
- `array_map()` widens a literal returned by its callback
- `array_map()` with an untyped callable drops the key type

**Cases** (PHPStan `nsrt/`, 29 assertions): `array-map.php` (10), `array_map_multiple.php` (10), `bug-10685.php` (2), `bug-9224b.php` (2), `array-functions.php` (2), `bug-4902-php8.php`, `bug-10442.php`, `pipe-operator.php`.

## Framework names in strings

### B911. Symfony, Yii and Tempest files are looked up from the workspace root, not their project's

**Impact: Medium · Complexity: Low**

In a monorepo opened at its top, with the application in a subdirectory
(`backend/`) and no `composer.json` at the root, the framework's own files
are looked for relative to the workspace root. Laravel already finds its
application's base path at indexing time (`Backend::laravel_root`), but
the other frameworks read `workspace_root` directly: Symfony's
`%kernel.project_dir%` (`framework.rs`, `symfony/route_world.rs`,
`symfony/translations.rs`), Yii's aliases, params, views, migrations and
assets (`yii/`), and Tempest's discovery (`tempest/discovery.rs`). Their
route, service, template and translation names then resolve against the
wrong directory.

The fix is the same shape as Laravel's: record the subproject holding the
framework's application when the root is not one (Symfony's `bin/console`
and `config/bundles.php`, Yii's `yii` script and `config/`, Tempest's
`tempest` script), and route each conventional lookup through an accessor
that falls back to the workspace root. Laravel's detection is
`composer::laravel_app_subproject`; a per-framework variant of it, or one
helper keyed by marker file, would serve all three.

## Name resolution

### B888. A class-level `@method` or `@property` tag does not override a real member

**Impact: Low · Complexity: Low (needs a decision first)**

```php
/** @method string sayHi() */
class HelloWorld extends ParentClass {} // ParentClass::sayHi(): int
(new HelloWorld())->sayHi(); // got int, PHPStan says string
```

PHPStan lets a class's `@method` tag win over a method from the class itself,
its parent or a used trait, and lets a `@property` tag narrow an inherited
public property when the native type accepts the tag's type (`?int`
re-declared `@property int` reads as `int`).

This deliberately conflicts with an earlier decision: PHP only reaches
`__call()` / `__get()` when no accessible member exists, so PHPantom lets the
real member win. Laravel test bases that carry an imprecise
`@method MockInterface mock(...)` next to the framework's real `mock()` are the
motivating case. `mock_helpers_survive_an_imprecise_method_tag_on_the_test_base`
and `test_property_tag_overrides_inherited_non_public_property` encode it.
Either keep the current rule and drop these cases, or follow PHPStan and
rewrite those tests.

**Cases** (PHPStan `nsrt/`, 5 assertions): `bug-3142.php` (3), `bug-6001.php`, `override-property-union-type-with-tag-subtype.php`.

## Code actions

No outstanding items.

## Miscellaneous

### B909. Hover on an assignment reads an arithmetic argument less precisely than the walker

**Impact: Low · Complexity: Low-Medium**

```php
/** @param list<int> $ids */
function f(array $ids): void {
    $filled = array_fill(0, count($ids) + 1, 'a');
    // hover on $filled: list<'a'>; at a later statement: non-empty-list<'a'>
}
```

Hovering the variable at its assignment resolves the call's arguments
through a different path than the forward walker, and `count($ids) + 1`
is not read as `int<1, max>` there. Any array rule that reads the
argument's range sees less than it does from the next statement on.

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

**Upstream cases** (PHPStan `nsrt/`, 27 assertions): `modulo-operator.php` (15), `math.php` (7), `bug-13097.php` (3), `bug-8803.php` (2).

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

**Upstream cases** (PHPStan `nsrt/`, 6 assertions): `bug-4423.php` (3), `bug-9341.php` (2), `inheritdoc-without-curly-braces-from-trait.php`.

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

### B889. An assignment to a variable variable (`$$name = ...`) erases the type of every variable assigned before it

**Impact: Medium · Complexity: Low**

```php
$lorem = 1;
$arr = ['one'];
$$lorem = 'ipsum';
$lorem; // got mixed, should be 1
$arr;   // got mixed, should be array{'one'}
```

In if.php every variable assigned before line 283's `$$lorem = 'ipsum'` reads as mixed (or nothing) afterwards, and every variable assigned after it is right. A dynamic-name write seems to invalidate the whole scope. PHPStan only writes the one variable when the name is a known literal, and invalidates nothing otherwise.

**Fix:** write only the named variable when the name is a known literal, and leave the scope alone otherwise.

**Cases** (PHPStan `nsrt/`, 51 assertions): `if.php` (51).

### B890. The `**` operator does not use `pow()`'s return-type rules

**Impact: Low · Complexity: Low**

```php
$r = rand(1, 3);
$r ** 2;        // int|float, should be int (pow($r, 2) is right)
$intA ** null;  // int|float, should be 1
$float ** '0';  // float, should be 1.0
$std ** $gmp;   // GMP, should be GMP|stdClass
```

Every pow() line in pow.php passes while the matching `**` line fails, so the operator is resolved by a separate, cruder path (always int|float for ints, no literal folding, no object operand handling). Route `**` through the pow() rule.

**Fix:** route `**` through the same rule `pow()` uses.

**Cases** (PHPStan `nsrt/`, 31 assertions): `pow.php` (31).

### B891. Concatenating a union of literal strings gives `literal-string` instead of a union of the results

**Impact: Low · Complexity: Low**

```php
$left = rand() ? 'a' : 'b';
$right = rand() ? 'x' : 'y';
$left . $right;  // got literal-string&non-falsy-string, should be 'ax'|'ay'|'bx'|'by'
".$left.";       // got string, should be '.a.'|'.b.'
```

A single literal on each side is folded, but a union on either side is not distributed. This covers `.`, `.=` and interpolation (where the result even drops to `string`). PHPStan distributes up to a limit of 128 combinations and generalizes past it.

It also shows as:

- Concatenating a union of literals is not folded into a union of literals

**Fix:** fold concatenation over the cross product of literal operands, up to the same union size cap the other folds use.

**Cases** (PHPStan `nsrt/`, 23 assertions): `constant-string-unions.php` (17), `bug-11129.php` (6).

### B892. Magic child and attribute access on `SimpleXMLElement` is not nullable, and on a subclass it has no type

**Impact: Low · Complexity: Low**

```php
$xml->foo;          // got SimpleXMLElement, should be SimpleXMLElement|null
$item['name'];      // got static(SimpleXMLElement), should be SimpleXMLElement|null
class Bar extends SimpleXMLElement {}
/** @var Bar $b */ $b->item; // got nothing, should be Bar|null
```

Two parts: the nullability PHPStan gives magic element/attribute reads, and a user subclass of SimpleXMLElement losing its magic child access entirely (foreach over it gives mixed).

**Cases** (PHPStan `nsrt/`, 11 assertions): `simplexml.php` (8), `bug-9575.php` (3).

### B893. `int-mask<…>` and `int-mask-of<…>` are not expanded to their values

**Impact: Low · Complexity: Low**

```php
const A = 0; const B = 1; const C = 2;
/** @var int-mask-of<self::*> */ private static $t;
self::$t;       // int-mask-of<self::TYPE_*>, should be 0|1|2|3
self::$t | 1;   // int, should be 1|3
```

The mask type is kept as unresolved text, so bitwise operators on it fall back to int.

It also shows as:

- `int-mask<...>` and `int-mask-of<...>` over non-contiguous values resolve to `int`

**Cases** (PHPStan `nsrt/`, 12 assertions): `bug-9384.php` (9), `int-mask.php` (3).

### B912. Analysis time doubles with each level of nested callback arguments

**Impact: Low · Complexity: Medium**

```php
function t($x) {
    $r = array_map(function ($q) use ($x) {
        $y = $q;
        if ($y) {
            // … the same `array_map(function …)` again, one level deeper …
        }
        return array_filter([$y], function ($z) use ($x) { return $z; });
    }, [$x]);
    return $r;
}
```

Each level of closures passed as call arguments doubles the time
`analyze` spends on the file: 0.8 s of CPU at 14 levels, 3.1 s at 16,
12.6 s at 18 and 49 s at 20 (release build). The same nesting written
as closures assigned to variables stays flat, because a closure
literal's own body reading is memoized per request. The callback
argument paths (template binding from a closure argument, and the
callback return inference behind `array_map`/`array_filter`) re-read a
nested callback once per enclosing level instead of sharing one answer.

### B895. `new $var()` has no type unless `$var` is a `class-string<T>`

**Impact: Low · Complexity: Low**

```php
/** @param mixed $a */
function f($a, string $s) {
    new $a();   // no type, should be object
    new $s();   // no type, should be object
}
```

`new` on class-string<Foo> works, but a mixed/string/object/plain class-string operand yields nothing instead of object.

It also shows as:

- `new $class` has no type when `$class` is a plain string or an intersection with `class-string`

**Cases** (PHPStan `nsrt/`, 9 assertions): `generic-class-string.php` (6), `bug-4579.php` (2), `bug-7374.php`.
