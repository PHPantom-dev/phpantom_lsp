# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

The entries below come from the 2026-07-18 analyze triage
refresh over the sample projects (see `projects/analyze-triage.md`).

## B110. Container string alias resolution (`app('x')` / `resolve('x')`) does not apply once the call result is assigned to a variable

**Severity: Low (1 error, bladestan) · Reproduced with fixture**

```php
$compiler = resolve('blade.compiler');
$compiler->component('dynamic-component', DynamicComponent::class); // "type of '$compiler' could not be resolved"
```

`resolve('blade.compiler')->component(...)` (no intermediate variable)
resolves correctly — the direct-call-subject path
(`completion/call_resolution.rs`) intercepts `app`/`resolve` with a
literal string argument and looks it up in Laravel's container alias
table. The variable-assignment RHS resolver
(`completion/variable/rhs_resolution.rs::resolve_rhs_function_call`)
has no equivalent check, so `$var = resolve('blade.compiler');` loses
the binding. Bladestan's `BladeCompilerFactory.php:17-18` is the only
sample-project occurrence.

## B111. `assertInstanceOf()` does not narrow when the expected class is list-destructured from a source array

**Severity: Low (1 error, pdepend) · Reproduced with fixture**

```php
$items = [
    ['null|int|float', '$number', ASTUnionType::class],
];
foreach ($items as $index => $expected) {
    [$expectedType, $expectedVariable, $expectedTypeClass] = $expected;
    [$type, $variable] = $declarations[$index];
    static::assertInstanceOf($expectedTypeClass, $type);
    $type->getImage(); // "type of '$type' could not be resolved"
}
```

A variable assigned a `::class` value directly (`$cls = Foo::class;`)
or via null-coalesce (`$cls = $arr[2] ?? Foo::class;`), including inside
a braced loop body, now narrows the assert subject. The remaining case
is when the expected-class variable is *list-destructured* out of a
source array: `$expectedTypeClass` is the third element of each
`$items` row, bound through the foreach value variable `$expected`.
Resolving it requires matching the destructure position against the
foreach source array's element at that index, which the class-string
resolver (`completion/variable/class_string_resolution.rs`) does not do
— it only recognizes direct assignments. The proper fix routes
class-string-value resolution through the shared forward walker (which
already tracks destructuring and foreach bindings) rather than
extending the special-purpose resolver, to avoid a parallel resolution
path (see CLAUDE.md performance anti-pattern #6). Accounts for the one
remaining `getImage()` error in PDepend's
`PHPParserVersion81Test.php` (the `testUnionTypesX` provider loop).
