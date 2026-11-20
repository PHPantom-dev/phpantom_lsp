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

### B752. A required shape key removed by `unset()` is not reported

**Impact: Low-Medium · Complexity: Medium**

```php
/** @param array{foo: int} $shape */
function f(array $shape): void {
    unset($shape['foo']);
    takesFoo($shape); // expects array{foo: int}; nothing reported
}
```

The variable resolves to `array{}` correctly, but a missing required key
is only reported for an array literal at the call site
(`enumerates_all_keys` in `src/diagnostics/type_errors/arguments.rs`).
A shape held in a variable is treated as open, since the engine may not
have seen every key assigned. An explicit `unset()` of a key the
declared shape requires is not that kind of uncertainty: the key is
provably gone. The same applies to `$e = []; takesFoo($e);`, where the
value was a literal one assignment ago.

The shape needs to record whether its key set is known to be complete
(a literal, a sealed declared type, or one of those minus `unset` keys)
rather than the call site guessing from the argument's syntax.
(php-typing-conformance: `arrays_sealed_shape_mutation`.)

**Where to look:** the `unset` handling in the forward walker,
`missing_required_shape_keys` and `enumerates_all_keys` in
`src/diagnostics/type_errors/arguments.rs`.

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

No outstanding items.
