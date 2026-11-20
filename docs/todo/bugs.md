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

### B755. A write through a dynamic key pools a shape's entries and loses a benevolent union

**Impact: Low-Medium · Complexity: Medium**

```php
/** @return list<string> */
function children(array $e): array { return []; }
function merge(array $a, array $b): array { return $a; }

/** @param array $element */
function preRenderLinks($element) {
    $element += ['#links' => [], '#attached' => []];
    foreach (children($element) as $key) {
        if (isset($element[$key]['#links'])) {
            $element['#links'] += $element[$key]['#links'];
            $element[$key]['#printed'] = true;
        }
        if (isset($element[$key]['#attached'])) {
            // reported: expects array, got array|array{'#printed': true, ...}|int|float
            $element['#attached'] = merge($element['#attached'], $element[$key]['#attached']);
        }
    }
}
```

A write at a key the walker cannot name widens the shape to
`array<K, V>`, pooling every entry's value into `V`, so a later read of
`'#attached'` sees the `'#links'` value too. That value is the benevolent
`array|int|float` `+=` produces for two untyped operands, which the
pooling turns into a plain union that the argument check then reports.
Either half alone would leave the read clean: keeping the known entries
next to the dynamic one (an unsealed shape), or keeping the benevolence
through the join. Drupal's `Link::preRenderLinks()` hits this through a
reference to `$element[$key]`.

**Where to look:** `merge_nested_array_write` in
`src/type_engine/variable/array_shape_writes.rs`.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

### B756. A union of array shapes binds `TValue` from one alternative

**Impact: Low-Medium · Complexity: Low-Medium**

```php
class C {
    public static function reduceOrder($array, $a) { return []; }

    /** @param list<string> $list */
    public static function t(array $regions, array $list, $w) {
        $tree = ['' => ['name' => '', 'children' => []]];
        $trees = array_fill_keys(array_keys($regions), $tree);
        foreach ($list as $name) {
            $r = (string) $w;
            $trees[$r]['']['children'][$name] = ['name' => $name, 'weight' => $w];
        }
        foreach ($regions as $region_name => $region) {
            // reported: expects array<array{name: '', children: non-empty-array<…>}>,
            // got array{'': array{name: '', children: array{}}}|array{'': array{…non-empty…}}
            $x = array_reduce($trees[$region_name], [static::class, 'reduceOrder']);
        }
    }
}
```

The argument is a union of two shapes (before and after the loop wrote
into it), and `array_reduce()`'s `array<TValue>` binds `TValue` to the
values of only one of them, so the other alternative fails the check
against the parameter it was inferred from. Binding a template from a
union should join what each alternative binds. Drupal's
`FieldUiTable::tablePreRender()` hits this.

**Where to look:**
`src/type_engine/call_resolution/template_binding.rs`.

## Framework names in strings

No outstanding items.

## Name resolution

No outstanding items.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
