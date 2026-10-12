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

No outstanding items.

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

#### B6. A closure returning a local variable binds the template from its declared type

**Impact: Medium · Complexity: Medium**

A closure's declared return type is narrowed by what its body returns
before it binds a `Closure(): T` template, but only when the returned
expression resolves without the closure's own local variables:

```php
/**
 * @template T
 * @param \Closure(): T $cb
 * @return T
 */
function remember(\Closure $cb): mixed { return $cb(); }

function local(): Sub {
    return remember(function (): Base {
        $p = make();          // returns Sub
        return $p->sortBy();  // returns static
    });
    // Return type Base is incompatible with declared return type Sub
}

function direct(): Sub {
    return remember(function (): Base {
        return make()->sortBy();  // fine, T = Sub
    });
}
```

`Sub` extends `Base`. PHPStan intersects the declared type with the
union of every `return` statement's type, so `T` is `Sub` in both.

**Where to change:** `infer_closure_return_type_seeded` in
`src/type_engine/call_resolution/template_subs.rs` resolves the text of
the first `return EXPR;` with only the closure's parameters seeded, so a
variable assigned inside the body is unknown and the declared type wins.
The body should be walked by the forward walker so locals are in scope,
and every `return` should contribute.

#### B7. A class named like a template parameter still shadows it in type checks

**Impact: Low-Medium · Complexity: Low**

With a class-like named `T` anywhere in the project (PDepend's test
fixtures declare `trait T`), member access on a `@template T` value now
resolves correctly, but argument and return compatibility checks still
compare against the class instead of the template's bound:

```php
trait T {}   // anywhere in the index
```

```php
namespace App;

/** @template T of Art */
class ArtList {
    /** @param T[] $artifacts */
    public function __construct(array $artifacts) {
        foreach ($artifacts as $artifact) {
            (new Filter())->accept($artifact);  // expects Art, got T
        }
    }
}

interface Node {
    /**
     * @template T of Node
     * @param class-string<T> $targetType
     * @return T|null
     */
    public function first($targetType);
}

abstract class AbstractNode implements Node {
    public function first($targetType): ?Node {
        foreach ($this->children() as $node) {
            if ($node instanceof $targetType) {
                return $node;  // Return type T is incompatible with ?Node
            }
            if (($child = $node->first($targetType)) !== null) {
                // expects class-string<Node>, got class-string<T>
                return $child;
            }
        }
        return null;
    }
}
```

Removing the trait makes all three clean.

**Where to change:** the argument and return type compatibility checks
need the template names in scope (including ones inherited from an
interface method's docblock) to win over class lookup, the way member
access does since the earlier fix. An unqualified `T` inside
`namespace App` also should not fall back to the global `\T` at all.

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

**Where to change:** `resolve_class_name` and `class_loader_with` in
`src/resolution.rs` deliberately skip an import when another import's
target is the bare name, because the loader cannot tell a name spelled
in source (`Redis`, meaning the facade) from an already-resolved FQN
(`Redis`, expanded from `PhpRedis`). The real fix is to stop feeding
the loader a mix of the two: resolve `PhpRedis` to `\Redis` (or mark it
as already qualified) before it reaches the use-map check, then drop
the heuristic.
