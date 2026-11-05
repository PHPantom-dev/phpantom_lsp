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

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

### B706. A template nested in an iterable parameter's value type binds to the whole argument

**Impact: Medium-High · Complexity: Medium**

```php
/**
 * @template Tk of array-key
 * @template Tv
 * @param iterable<Tk, (Closure(): Tv)> $tasks
 * @return array<Tk, Tv>
 */
function concurrently(iterable $tasks): array { … }

[$server, $client] = concurrently([$listener->accept(...), fn(): Stream => connect()]);
$server->writeAll('…');   // Cannot access method 'writeAll' on type 'list{Closure(): Stream, …}'
```

`Tk` is inferred from the keys, but `Tv` is bound to the whole array
literal (`list{Closure(): Stream, Closure(): Stream}`) instead of to the
return type of each closure in it. A lone `@param Closure(): Tv` infers
`Tv` correctly, so the closure-return binding is there; it is not applied
when the closure type is the value type of an `iterable`/`array`
parameter. The values should each bind `Tv` from their return type,
first-class callables included, and the bindings union.

The same happens when the value type is a generic class rather than a
closure:

```php
/**
 * @template Tk of array-key
 * @template Tv
 * @param array<Tk, TypeInterface<Tv>> $elements
 * @return TypeInterface<array<Tk, Tv>>
 */
function shape(array $elements): TypeInterface { … }

shape(['x' => int(), 'label' => string()])->coerce($data);
// array<string, array{x: TypeInterface<int>, label: TypeInterface<string>}>,
// want array<string, int|string>
```

So the fix belongs where an argument's element types are matched against
the parameter's value type, not in the closure-return binding.

Found by `analyze` on azjezz/psl: `Async\concurrently()` and
`Async\all()` in its tests (about 70 false positives), and `Type\shape()`.

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
