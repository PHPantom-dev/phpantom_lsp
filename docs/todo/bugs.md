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

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

### B552. A by-reference closure capture keeps the narrowing from where the closure was created

**Impact: Low · Complexity: Medium**

```php
if ($c !== null) {
    $f = function () use (&$c) { take($c); };   // `$c` reads as non-null
    $c = null;
    $f();                                        // TypeError at runtime, not reported
}
```

A by-value capture is copied when the closure is created, so seeding it from
the scope at that point is right. A by-reference capture is read when the
closure runs, and the variable can be reassigned in between, so the
narrowing at the creation point does not hold inside the body.
`seed_closure_captures` seeds both kinds the same way, in an `if` block as in
a ternary, `&&` or `match` branch. A by-reference capture should also
carry every type the enclosing function assigns to the variable after the
closure is created.

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Miscellaneous

#### B8. `use X as Y` makes a bare `X` skip another import of the same short name

**Impact: Low-Medium · Complexity: Medium**

```php
namespace App;

use Illuminate\Support\Facades\Redis;
use Redis as PhpRedis;

Redis::connection('cache');
// Method 'connection' not found on class 'Redis'
```

PHP resolves `Redis` through the first import, the facade. PHPantom
resolves it to the global `\Redis` extension class.

The same ambiguity lets an unqualified name inside a namespace fall back
to a global class: with `trait T {}` in the global namespace, a bare `T`
in `namespace App` loads it, where PHP only ever means `App\T`.

**Where to change:** `resolve_class_name` and `class_loader_with` in
`src/resolution.rs` deliberately skip an import when another import's
target is the bare name, because the loader cannot tell a name spelled
in source (`Redis`, meaning the facade) from an already-resolved FQN
(`Redis`, expanded from `PhpRedis`). The real fix is to stop feeding
the loader a mix of the two: resolve `PhpRedis` to `\Redis` (or mark it
as already qualified) before it reaches the use-map check, then drop
the heuristic.
