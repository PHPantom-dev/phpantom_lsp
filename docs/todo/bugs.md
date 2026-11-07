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

### B711. `foreach` over a `RecursiveIteratorIterator` gives the wrapped iterator

**Impact: Medium · Complexity: Low-Medium**

```php
/** @param array<string, int> $data */
function size(array $data): int {
    $leaves = new RecursiveIteratorIterator(new RecursiveArrayIterator($data));
    foreach ($leaves as $datum) {
        strlen((string) $datum);
        // Object of class RecursiveArrayIterator cannot be converted to string
    }
}
```

A `RecursiveIteratorIterator` yields the elements of the iterator it
wraps (the leaves, in the default mode), not the iterator. Our stub patch
makes the class generic over the wrapped iterator,
`RecursiveIteratorIterator<RecursiveArrayIterator<string, int>>`, and
declares no traversal binding. `foreach` then reads the lone template
argument as the element type, so `$datum` is the iterator. A directory
walk only looks right because `RecursiveDirectoryIterator` extends
`SplFileInfo`.

The element type is the wrapped iterator's own element type (`int` here,
`SplFileInfo` for a directory walk with the default flags), and the key
its key type.

**Where to look:** `patch_recursive_iterator_iterator`
(`src/stub_patches/classes.rs`) and the single-argument reading in
`iteration_value_type` (`src/type_engine/variable/iterable_elements.rs`).

Found by `analyze` on WordPress (`WpOrg\Requests\Transport\Curl`).

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

### B712. A used `void` result is judged by one member of a union

**Impact: Low-Medium · Complexity: Low-Medium**

```php
/** @var LoggerInterface|ObjectProphecy */
private $logger;

$this->logger->warning('x')->shouldBeCalled();
// Result of method LoggerInterface::warning() (void) is used
```

The call's result is `null` only if every class the receiver can be
returns nothing. `ObjectProphecy` answers the call through `__call`,
which returns a `MethodProphecy`, so the result is used correctly. The
check takes the first union member that declares the method and asks
only it: `Logger|Mock` is reported and `Mock|Logger` is not, when `Mock`
declares `warning()` with a return value. Report the call only when every
member that answers it, through a declared method or `__call`, is natively
`void`.

**Where to look:** `collect_void_result_diagnostics_with_context`
(`src/diagnostics/void_results.rs`), which reads `native_void` off the
single target `resolve_callable_target_at_offset` returns.

Found by `analyze` on Drupal core's Prophecy-based tests.

No outstanding items.
