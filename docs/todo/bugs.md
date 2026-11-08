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

No outstanding items.

## Code actions

No outstanding items.

## Miscellaneous

### B742. A global constant holding `Foo::class` or an enum case has no type

**Impact: Low · Complexity: Low**

```php
namespace Lib;

class Widget {}
enum Suit { case Hearts; }

const WIDGET = Widget::class;
const SUIT = Suit::Hearts;

$w = WIDGET;                    // class-string<Widget>, but untyped
$s = SUIT;                      // Suit::Hearts, but untyped

/**
 * @template T of object
 * @param class-string<T> $class
 * @return T
 */
function make(string $class = WIDGET): object { … }

$made = make();                 // Widget, but mixed
$made = make(WIDGET);           // the same
```

A class constant initialized to `Foo::class` or `Suit::Hearts` reads as
that class name or case (`class_constants.rs` falls back to
`class_name_literal_type` and `resolve_static_access_type` after the
literal fold), but a global or namespaced constant does not: both the AST
path (`global_constant_type` in `rhs_resolution/constant_access.rs`) and
the text path (`Backend::resolve_arg_text_to_type`) stop at
`infer_type_from_constant_value` and `folded_constant_type`, which only
read literals. Both need the same fallback.

Once fixed, cover the template case with a callee and a caller in
different namespaces that each declare a constant of the same name: an
omitted argument's default is read in the callee's namespace
(`TemplateCallee::namespace`), which nothing can observe until such a
constant has a type.
