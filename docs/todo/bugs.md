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

### B741. A parameter default's unqualified constant is read in the caller's namespace

**Impact: Low · Complexity: Medium**

```php
// src/Lib/pick.php
namespace Lib;

const MODE = 1;

/** @return ($mode is 1 ? int : string) */
function pick(int $mode = MODE): int|string { … }

// src/App/run.php
namespace App;

const MODE = 2;
$picked = \Lib\pick();          // int, but read as string
```

A parameter default is source text read at each call site. The parser
spells the constants in it that have one meaning fully qualified (a
`use const` import, a qualified name), but an unqualified name inside a
namespace means that namespace's constant when one is declared and the
global constant otherwise, which only an index of the declared constants
can say. A class constant's initializer is read in its class's namespace,
and a namespaced constant's value is resolved where it is looked up
(`Backend::lookup_global_constant_candidates`), but a default reaches its
readers without the callee's namespace: the conditional return type of an
omitted argument reads it through the call site's argument resolver
(`type_ops/conditional/text_args.rs`), which tries the call site's
namespace, and template binding (`call_resolution/template_binding.rs`)
reads it as global.

Either reader needs the callee's namespace: `ConditionalClassContext` and
`TemplateCallee` could carry it, with each default's bare names looked up
there before the global constant, as `FileCtx::namespace` does for a class
constant's initializer.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
