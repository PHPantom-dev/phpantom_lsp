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
