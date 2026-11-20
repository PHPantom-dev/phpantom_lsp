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

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

### B749. `switch` and `match` do not narrow the way the same `===` in an `if` does

**Impact: Medium · Complexity: Medium**

`if (get_class($x) === Foo::class)` and `if ($x::class === Foo::class)`
both narrow `Foo|Bar` to `Foo`. The same comparison spelled as a `switch`
or `match` subject only narrows in one of four forms:

```php
function f(Foo|Bar $x): void {
    switch (get_class($x)) { case Foo::class: $x->fooOnly(); } // partial_union_member
    switch ($x::class)     { case Foo::class: $x->fooOnly(); } // partial_union_member
    echo match (get_class($x)) { Foo::class => $x->fooOnly(), default => '' }; // partial_union_member
    echo match ($x::class)     { Foo::class => $x->fooOnly(), default => '' }; // narrows
}
```

The `case`/arm narrowing recognises its own subset of subjects instead
of applying the facts `subject === label` would produce in an `if`. The
fix is to have `process_switch` and the match-arm narrowing build that
comparison and send it through the shared condition narrowing, so every
form the `if` path learns (including `gettype()`, B750) works in all
three places.

**Where to look:** `process_switch` in
`src/type_engine/variable/forward_walk/control_flow.rs`, the match-arm
narrowing, and `src/type_engine/type_ops/condition_facts/class_checks.rs`
for the `if` path that already works.

### B750. `gettype()` comparisons narrow nothing

**Impact: Low-Medium · Complexity: Low-Medium**

`gettype($x) === 'integer'` leaves `int|string` as it is, in an `if` as
well as in a `switch` or `match`:

```php
function f(int|string $x): void {
    if (gettype($x) === 'integer') {
        takesInt($x); // reported: expects int, got int|string
    }
}
```

`gettype()` returns a fixed set of names (`'boolean'`, `'integer'`,
`'double'`, `'string'`, `'array'`, `'object'`, `'resource'`,
`'resource (closed)'`, `'NULL'`, `'unknown type'`), each standing for one
type, so comparing it to a literal narrows like the matching `is_*()`
call. Psalm and PHPStan both do this. Once B749 is fixed, the `switch`
and `match` forms come for free.

**Where to look:** `src/type_engine/type_ops/condition_facts/builtin_checks.rs`,
next to the `is_*()` handling.

### B751. Loose `==` against a non-numeric string keeps `int` and `float`

**Impact: Low-Medium · Complexity: Low**

In PHP 8, comparing an `int` or `float` to a non-numeric string compares
them as strings, so `$key == 'foo'` can never hold for a number. The
narrowing keeps those alternatives anyway:

```php
/** @param 'foo'|'bar'|int $key */
function f(string|int $key): void {
    if ($key == 'foo') {
        takesFoo($key); // reported: expects 'foo', got 'foo'|int
    }
}
```

`apply_loose_literal_narrowing` pins only alternatives for which `==`
means `===`, and leaves the rest alone in both directions. In the
true branch it can drop `int`, `float` and their literals when the
literal is a non-numeric string. Leading and trailing whitespace still
counts as numeric (`' 1'`), so use PHP's own numeric-string rule. The
false branch must leave them alone. (php-typing-conformance:
`assertions_key_loose_narrowing`.)

**Where to look:**
`src/type_engine/variable/forward_walk/cond_narrowing/loose_equality.rs`.

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Framework names in strings

No outstanding items.

## Name resolution

No outstanding items.

## Code actions

No outstanding items.

## Miscellaneous

### B753. A write through a reference does not reach the variable it aliases

**Impact: Low-Medium · Complexity: Medium**

```php
function f(): void {
    $a = 'text';
    $r = &$a;
    $r = 1;
    takesInt($a); // reported: expects int, got 'text'

    $b = ['x' => 1];
    $e = &$b['y']; // creates $b['y'] (null)
    $e = 1;
    takesY($b); // $b still reads as array{x: 1}
}
```

`$r = &$a` binds `$r` to the same value as `$a`, but the forward walker
treats it as a plain assignment, so later writes to `$r` leave `$a`'s
type as it was. Taking a reference to an element (`&$b['y']`) also
creates that element, which the walker does not record either. Every
consumer reads the stale type: hover, completion, and every diagnostic
that checks the variable, including the missing-shape-key check.

The walker needs to track reference bindings: a write to either side of
an alias updates (or, where it cannot follow, forgets) the other.
`by_ref.rs` already handles references passed to by-reference
parameters and `use (&$x)` captures; a local `=&` is the case it misses.

**Where to look:** the `=&` assignment handling in
`src/type_engine/variable/forward_walk/assignment.rs`.

### B754. An include whose path does not resolve leaves a function's locals as they were

**Impact: Low · Complexity: Medium**

```php
function f(Output $output): void {
    $data = require $cacheFilePath; // may assign $output
    $output->write('…');            // still read as Output
}
```

An `include`/`require` inside a function runs the file in the function's
scope. When the path resolves, the walker applies what the file's top
level assigns. When it does not, the walker keeps every local's type, as
PHPStan does. Widening every local to `mixed` there, as `extract()` of
unknown keys and `$$name = …` do, is sound but costly: the common
`$data = require $path;` idiom for loading a PHP array file then turns
every typed parameter and local after it into `mixed`, which put 20 new
errors on phpstan-src's `ResultCacheManager` and `PHPStanDiagnoseExtension`.
A fix needs a way to tell the two apart, such as reading an include whose
value is used as a file that returns rather than one that assigns, or a
config switch, rather than widening unconditionally.

**Where to look:** `ExprWalk::include` in
`src/type_engine/variable/forward_walk/expr.rs` and `include_locals` in
`includes.rs`.
