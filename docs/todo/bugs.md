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

No outstanding items.

## Miscellaneous

### B580. Convert to instance variable rewrites positions where a property access is invalid

**Impact: Medium · Complexity: Medium**

```php
class Foo {
    public function bar() {
        $x = 1;                                  // cursor here
        $f = function () use ($x) { return $x; };
        $h = fn() => $x;
        try {} catch (\Exception $x) {}
        static $x;
        global $x;
        return compact('x');
    }
}
```

The action produces `use ($this->x)`, `use (&$this->x)`,
`catch (\Exception $this->x)`, `static $this->x;` and `global $this->x;`,
all syntax errors. It leaves `$x` inside the closure body and the arrow
function untouched, so those now read an undefined local, and `compact('x')`
silently loses the variable.

`resolve_convert_to_instance_variable` replaces every occurrence
`ScopeMap::all_occurrences` returns, and that list does not say which
occurrences sit in a declaring position (closure `use`, `catch`, `static`,
`global`, parameters) or how a nested closure or arrow function captures the
variable.

**Fix:** Carry the syntactic role of each occurrence out of the scope
collector. Rewrite a closure `use` capture by dropping it from the `use` list
and converting the closure body's occurrences, and convert arrow-function
bodies directly (both bind `$this`). Decline the action when the variable
appears in a `catch`, `static`, or `global` declaration, or by name in
`compact()` / `extract()` / `$$`.
