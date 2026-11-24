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

### B762. `$this` in a class declared twice reads the other declaration

**Impact: Low · Complexity: Medium**

```php
// a.php
namespace V;
class Variant {}

// b.php
namespace V;
class Variant { private int $x = 1; public function f(): int { return $this->x; } }
// "Property V\Variant::$x not found"
```

When more than one file declares a class, the class index keeps one
declaration for the name (the lowest-sorting file, or the one that does
not wait for the autoloader behind a `class_exists` guard). Inside the
body of a declaration that lost, `$this`, `self` and `static` resolve
by name to that winner, so every member only the losing declaration has
is reported unknown in its own file, and hover and completion there show
the other declaration's members. Polyfills and version-specific variants
of a class (`symfony/polyfill-*`, Carbon's `DatePeriodBase`) are where
this shows up in real code.

**Fix:** resolve `$this`, `self` and `static` inside a class body to the
declaration that encloses them in the current file, not to whatever the
class index holds for the name.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
