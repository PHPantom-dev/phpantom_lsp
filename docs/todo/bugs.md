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

### B579. An override's parameter hint is compared to its ancestor's by short name

**Impact: Low · Complexity: Medium**

```php
namespace P {
    class Box {}
    class Base {
        /** @param Box&\Countable $b */
        public function take(Box $b): void {}
    }
}
namespace Q {
    class Box { public function fromQ(): void {} }
    class Child extends \P\Base {
        public function take(Box $b): void {
            $b->fromQ(); // reported: not found on P\Box, Countable
        }
    }
}
```

`Q\Child::take()` declares its own `Q\Box`, but it inherits the parent's
`@param P\Box&\Countable` as if it had restated the parent's hint.

Methods keep their native hints (`native_type_hint`, `native_return_type`)
as written in the source, never resolved to FQNs, while their docblock types
are. `child_native_hint_overrides` (`inheritance/enrichment.rs`) compares the
two methods' native hints, so `Box` in one namespace equals `Box` in another.
The return-type side checks the class hierarchy through the resolved
effective type instead, which is why it is not affected.

Resolving method native hints in `resolve_parent_class_names`
(`parser/ast_update.rs`), as is already done for functions' return hints,
fixes the comparison but breaks consumers that re-resolve a native hint
against the calling file's imports (a `use X\Foo` file then reads `B\X\Foo`).

**Fix:** Resolve method native hints at parse time and move the consumers
that re-resolve them over to the FQN form.

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Miscellaneous

