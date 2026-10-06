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

## Arithmetic

No outstanding items.

## Symbol resolution

### B542. Import edits treat a file with several `namespace` blocks as having one `use` list

**Impact: Low · Complexity: Medium**

```php
namespace A {
    use X\Foo;
    function a(): Foo {}
}
namespace B {
    // `B\Foo` is unknown, but block A's import makes `Foo` look
    // already imported, so no "import class" action is offered.
    function b(Foo $f) {}
}
```

Name resolution is per block (each `NamespaceSpan` carries its own
`use_map`), but everything that *writes* imports still works on the
file-wide `file_imports` table and `first_file_namespace`: the import-class
and qualified-name-to-import code actions, the PHPStan `add_throws` /
`add_override` / `remove_throws` fixes, and class rename/move rewriting
(`rename/class/rewrite.rs`). They check for an existing import against every
block's imports at once, and work out where a new one goes from the first
block, so an edit in a later block is skipped because another block
imports the name, or is placed against the wrong block. Fixing it means locating the target block by offset and
reading, inserting into, and rewriting that block's own `use` statements.

### B543. Unused-import detection pools the imports of every `namespace` block

**Impact: Low · Complexity: Medium**

```php
namespace A {
    use X\Foo;              // unused in A, but not reported
}
namespace B {
    use X\Foo;
    function b(Foo $f) {}
}
```

`diagnostics/unused_imports.rs` checks declared imports against the merged
file-wide import table, so an import used only in another block counts as
used, and two blocks importing the same alias collapse into one entry.
It needs to track each block's imports (and their source ranges) and match
references against the block they are written in.

### B544. `self::` inside a `@param-closure-this` closure names a different class depending on the feature

**Impact: Low · Complexity: Medium**

```php
/** @param-closure-this Ctx $callback */
function bindOne(\Closure $callback): void {}

class Host {
    public function run(): void {
        bindOne(function () {
            self::next()->;         // completion offers Ctx members
            $n = self::next();      // the forward walker types $n as Host
        });
    }
}
```

The subject resolver (`resolve_self_static_classes` in
`type_engine/resolver/mod.rs`) treats `self::`/`static::` in a bound
closure as the bound class, which is what `Closure::call()` and Laravel's
`Macroable` (`bindTo($this, static::class)`) do at runtime. The forward
walker's static-call path (`resolve_rhs_static_call` in
`rhs_resolution/calls.rs`) always uses the lexical class, which is what
PHPStan does, and resolves to nothing in a closure outside any class. So
`self::next()->` and `$n = self::next(); $n->` disagree, and diagnostics
follow the walker. Pick one model and make both paths share it; if the
bound class wins, the walker needs the binding in its scope rather than an
AST walk per `self::` call.

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Miscellaneous

No outstanding items.
