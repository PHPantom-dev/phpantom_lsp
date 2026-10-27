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

### B690. A member access through an override with a native return type loses the call's method template

**Impact: Medium · Complexity: Medium**

```php
interface Node {
    /**
     * @template T of Node
     * @param class-string<T> $type
     * @return T|null
     */
    public function first($type): ?self;
}
abstract class Base implements Node {
    public function first($type): ?Node { return null; }
}
final class Leaf extends Base { public function leafOnly(): int { return 1; } }

function f(Base $b): void {
    $v = $b->first(Leaf::class);   // hover and argument checks: Leaf|null
    $v->leafOnly();                // Method 'leafOnly' not found on class 'Node'
    $b->first(Leaf::class)->leafOnly();   // same
}
```

The two consumers disagree about the same value. The forward walker
types `$v` as `Leaf|null`, so hover and argument checks are right, but
the member-access subject resolver (`resolve_subject_outcome` →
`resolve_target_classes`, used by `unknown_member`) resolves `$v` to the
override's native `Node`. The same call through a `Node` or a `Leaf`
receiver resolves to `Leaf`, as does `Base::first()` without a native
return type. So the subject path looks the method up on the receiver's
own class, where the override has no `@template` until the inheritance
merge adds it, and does not prefer the merged method the way
`resolve_owner_method_call` (`rhs_resolution/owner_method_call.rs`)
does. Fix the subject path to read the merged method so the two cannot
diverge, rather than adding a second preference rule.

Predates the 2026-10-12 sweep. Found while reducing a PDepend regression
with a scratch project. PDepend's own calls go through subclasses, so
they do not hit it.

## Name resolution

No outstanding items.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
