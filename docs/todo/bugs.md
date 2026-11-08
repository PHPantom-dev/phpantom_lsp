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

### B735. A class that does not load fails every parameter type it is passed to

**Impact: Medium · Complexity: Low-Medium**

```php
use ColinODell\PsrTestLogger\TestLogger; // package not installed

$logger = new TestLogger();
$factory->addLogger($logger); // expects Psr\Log\LoggerInterface, got TestLogger
```

When the argument's class cannot be loaded, nothing is known about its
parents or interfaces, so whether it satisfies `LoggerInterface` cannot be
decided. It is still reported as a mismatch, beside the `unknown_class`
already reported for the name itself, so one missing package gives two
errors per use. Drupal core's tests hit it wherever a dev-only package is
absent (about a dozen `addLogger()` calls in `projects/drupal`). An
argument whose class does not load should leave the subtype check
undecided rather than failed.

## Conditional return types

No outstanding items.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

No outstanding items.

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

No outstanding items.
