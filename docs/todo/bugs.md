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

### B710. A `try` body that always returns still flows past its `catch`

**Impact: Low-Medium · Complexity: Low-Medium**

```php
public function getId(): int {
    $tryAgain = false;
    try {
        return $this->insert();
    } catch (\Exception $e) {
        if (!$tryAgain = $this->ensureTableExists()) {
            throw $e;
        }
    }
    if ($tryAgain) {             // always true here
        return $this->insert();
    }
}   // Method getId() should return int but can reach its end without returning
```

Code after a `try`/`catch` can only be reached from the end of a block
that does not exit. Here the `try` body always returns, so only the
`catch` reaches the `if`, and there `$tryAgain` is `true`. The state at
the end of the `try` body is merged in anyway, so `$tryAgain` is `bool`,
the `if` may be skipped, and the missing-return check reports the end of
the method. The same shape with a plain assignment
(`try { return; } catch (\Exception $e) { $y = true; }`) leaves `$y` as
`bool` instead of `true`.

Found by `analyze` on Drupal core (`Drupal\Core\Batch\BatchStorage::getId()`).

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

### B714. A constant written into an array in a loop loses its literal

**Impact: Medium · Complexity: Medium**

```php
private const int WHITE = 0;

/** @param array<int, self::WHITE|self::GRAY|self::BLACK> $labels */
private static function check(array $labels): void {}

$labels = [];
foreach ($nodes as $node) {
    $labels[$node] = self::WHITE;
}
self::check($labels);
// Argument 1 ($labels) expects array<int, 0|1|2>, got array<int, int>
```

The same write outside a loop keeps `array<int, 0>`, and PHPStan keeps
the literal in the loop too. A keyed or appended write inside a loop body
widens the value's scalar literals before storing it, so a fixed-point
re-walk cannot grow a literal union (or a concatenated string) once per
pass. That is only needed when the value changes from one pass to the
next; a constant, or any value whose type the re-walk leaves unchanged,
should keep its literal.

**Where to look:** the `in_loop` widening in
`merge_nested_array_write_inner` and `pushed_value`
(`src/type_engine/variable/array_shape_writes.rs`).

Found by `analyze` on Vytrvalec Server (`App\Algorithm\Graph::hasCycle()`).

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

### B713. Hover shows two `class-string`s that share a short name as one

**Impact: Low · Complexity: Low-Medium**

```php
namespace App;

/** @param class-string<\App\C>|class-string<\Other\C> $s */
function foo(string $s): void {
    $s; // hover: $s = class-string<C>
}
```

Hover shortens every class name in a type to its short name, so the two
members print the same and the union shows one of them. A union of the
classes themselves does not have this problem: each gets its own block
under its namespace. The same collapse hides `C::class|E::class` with `E`
an import of another namespace's `C`, which is why that assertion in
`tests/psalm_assertions/const_values.php` is skipped.

**Where to look:** `shorten_php_type` (`src/hover/formatting.rs`), which
shortens before the union is printed.
