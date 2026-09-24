# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

Each entry below carries an **Impact · Complexity** rating using the same
scale defined in [`docs/todo.md`](../todo.md); that table is also where
each bug's row lives in the current sprint/backlog.

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

### B343. A `switch` case does not narrow a literal union
**Impact: Low-Medium · Complexity: Medium**

```php
/** @param "foo"|"bar" $s */
function f(string $s): void {
    switch ($s) {
        case "foo":
            $s; // should be 'foo', is "foo"|"bar"
            break;
    }
}
```

`if ($s === "foo")` narrows the same parameter, so the literal is
understood; the `case` arm does not apply the loose comparison PHP makes
there. Found porting Psalm's `ConstValuesTest` (`noRedundantConditionWithSwitch`);
the assertion is `// SKIP` in `tests/psalm_assertions/const_values.php`.

### B344. The falsy branch of a truthiness check keeps the object half
**Impact: Low · Complexity: Low-Medium**

```php
/** @param Foo|false $a  @param class-string<Foo>|null $c */
function f($a, $c): void {
    if (!$a) { $a; } // should be false, is Foo|false
    if (!$c) { $c; } // should be null, is class-string<Foo>|null
}
```

An object is always truthy (`SimpleXMLElement` aside) and a `class-string`
is never empty, so the falsy branch can drop both. The truthy branch
already drops `false`/`null`. Found porting Psalm's `ReconcilerTest`;
the assertions are `// SKIP` in
`tests/psalm_assertions/type_reconciliation_reconciler.php`.

### B345. A type check does not split an `iterable`
**Impact: Low-Medium · Complexity: Medium**

On `iterable<int, string>`, `is_array()` should leave `array<int, string>`
and `is_object()` / `instanceof \Traversable` / `!is_array()` should leave
`Traversable<int, string>`. Today `is_array()` keeps the whole iterable and
the object checks produce a bare `Traversable`, losing the generic
arguments that drive foreach value completion. Found porting Psalm's
`ReconcilerTest` (`iterableToArray`, `iterableAndObject`, …); the assertions
are `// SKIP` in `tests/psalm_assertions/type_reconciliation_reconciler.php`.

### B346. `instanceof` on a class-or-interface union drops the intersection
**Impact: Low · Complexity: Medium**

On `SomeClass|SomeInterface`, `$x instanceof SomeInterface` should give
`(SomeClass&SomeInterface)|SomeInterface`: a non-final `SomeClass` can have
a subclass that implements the interface. PHPantom gives `SomeInterface`,
so the `SomeClass` members of that half are lost. Found porting Psalm's
`ReconcilerTest`; the assertions are `// SKIP` in
`tests/psalm_assertions/type_reconciliation_reconciler.php`.

## Arithmetic

No outstanding items.

## Symbol resolution

### B347. A `Class::CONST` operand resolves to the first same-named class in the file
**Impact: Low · Complexity: Medium**

In a file with several braced `namespace` blocks that each declare an `A`,
a method's `@return key-of<A::FOO>` is evaluated against the first `A` in
the file, whichever block the method is declared in. The operand reaches
`constant_operand_shape` unqualified, and `find_class_by_name` matches the
short name. Class names in the same position already resolve against the
block they are used in; the class half of a constant operand has to be
qualified the same way (against the declaring file's namespace and `use`
imports) before the type leaves its declaration. Rare outside test
fixtures. Found porting Psalm's `KeyOfArrayTest`; two assertions are
`// SKIP` in `tests/psalm_assertions/key_of_array.php`.

## Array types

No outstanding items.

## Docblock handling

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Miscellaneous

No outstanding items.
