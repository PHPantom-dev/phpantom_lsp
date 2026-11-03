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

### B692. `(int)` of a `ctype_digit()`-checked string is not non-negative

**Impact: Low · Complexity: Low-Medium**

```php
function wait(string $delay): void {
    if (!ctype_digit($delay)) {
        return;
    }
    sleep((int) $delay);   // expects int<0, max>, got int
}
```

A string that passes `ctype_digit()` holds only the digits 0-9, so casting
it to `int` cannot give a negative number, and `sleep()`'s
`int<0, max>` parameter accepts it. The narrowing leaves `$delay` a plain
`string` and the cast gives `int`. PHPStan reports nothing here only
because its function map declares `sleep()` and `usleep()` as taking a
plain `int`. phpstorm-stubs and Mago both declare `int<0, max>`, which
matches PHP 8 throwing a `ValueError` on a negative.

Found in a Laravel project's API client, which sleeps for the number of
seconds a rate-limit message names.

### B686. A positive `@phpstan-assert` of a nullable or union type keeps only the class

**Impact: Low · Complexity: Medium**

```php
/** @phpstan-assert ?Dog $x */
function isMaybeDog(mixed $x): void {}

function f(Dog|Pet|null $x): void {
    isMaybeDog($x);
    $x; // Dog, should be Dog|null
}
```

The same happens when a template is bound to such a type from the call's
arguments: `assertSame($expected, $actual)` with `?Dog $expected` leaves
`$actual` a `Dog`, and with `int|string $expected` it leaves `$actual`
unnarrowed. The reverse shows up on a plain class assertion:
`assertInstanceOf($petClass, $x)` with a `class-string<Pet>` argument leaves
a `Dog|Pet|int` subject as `Pet|int`. `try_apply_custom_assert_narrowing`
(`type_engine/type_ops/condition_facts/assert_annotations.rs`) hands the
whole asserted type to `apply_instanceof_inclusion`, which works on the
class entries only, so the `null` member is lost and scalar members are never kept or dropped. The
`-if-true` / `-if-false` path (`apply_assertion_to_key` in
`forward_walk/cond_narrowing/assertions.rs`) leaves an included union whole
for the same reason. Inclusion should keep each subject member that some
member of the asserted union admits, routing scalar members through the
type guards and class members through the `instanceof` machinery.

### B687. A sentinel check through `?? <sentinel>` other than `null` narrows nothing

**Impact: Low · Complexity: Low**

```php
/** @param array{flag?: string|false|null} $cell */
function f(array $cell): void {
    if (false !== ($cell['flag'] ?? false)) {
        take($cell['flag']); // expects string, got string|false|null
    }
}
```

The coalesce is `false` when the entry is unset, `null` or `false`, so
passing the check proves the entry set and neither `null` nor `false`.
Only the `?? null` form is read through today.

**Where to change:** `coalesced_onto_null_subject` in
`src/type_engine/variable/forward_walk/cond_narrowing/predicates.rs`
reads only a `null` fallback. Generalize it to a fallback that is the
compared sentinel, and have the `false` sites in `null_narrowing.rs`
(`extract_non_false_check_var` truthy, `extract_false_equality_check_var`
inverse) strip `null` as well as `false` from the coalesced subject.

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

### B704. A function declared in two files resolves to whichever is found first

**Impact: Medium · Complexity: Medium**

```php
// wp-admin/includes/noop.php, loaded only by load-scripts.php
function __() {}

// wp-includes/l10n.php, loaded everywhere else
function __( $text, $domain = 'default' ) { … }

__( 'Hello' );   // Expected 0 arguments, got 1
```

WordPress core declares nineteen of its functions a second time as
no-ops for its script loader. Calls across the project resolve to the
no-op, so with `extra-arguments` on WordPress core gets 17,435
argument-count reports (13 once `noop.php` is excluded, the same 13
PhpStorm reports), and hover, signature help, and return types all
describe the wrong declaration. PHP only ever loads one of them, and
nothing in the call says which.

When a name has several declarations, a call is valid if any of them
accepts it, and its return type is the join of theirs. Where one
declaration is plainly the real one (the other has an empty body and no
parameters, or sits in a file only one entry point includes), prefer it
for hover and go-to-definition, and offer the others alongside.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
