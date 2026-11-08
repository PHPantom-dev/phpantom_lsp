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

### B740. A constant named in another file's initializer has no type

**Impact: Low-Medium · Complexity: Medium**

```php
// src/Lib/Box.php
namespace Lib;
use const Vendor\DEFAULT_LIMIT;

const LIMIT = 5;
class Box {
    const MAX = LIMIT;          // 5
    const FLOOR = DEFAULT_LIMIT;
}

// src/App/run.php
namespace App;
$max = \Lib\Box::MAX;           // should be 5, has no type
```

A class constant's initializer, a namespaced constant's initializer and a
parameter default are source text that the reader resolves later, in a
different file. The parser qualifies the class names and `__DIR__` in that
text against the declaring file, but not constant names. The reader can't
look them up through its own namespace and `use const` imports, since those
belong to a different file, so only a global constant of that name is
found. A namespaced or imported constant in such an initializer comes out
untyped.

One path still hands such text to the reader's lookup. A conditional return
type decided by an omitted argument reads the parameter's default through
the call site's argument resolver
(`type_ops/conditional/text_args.rs`), so a default naming a namespaced
constant finds the call site's constant of that name, if it has one.

The parser could qualify constant names too. An imported name has one
answer. An unqualified name in a namespace falls back to the global
constant at runtime, so it needs both candidates kept, or the declaring
file's location recorded so the reader can look it up there.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
