# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

## B1. `resolve_function_name` guesses a single namespace, missing same-file multi-namespace declarations

`resolve_function_name` (`src/resolution.rs`) builds its candidate
list from a single `file_namespace` — the *call site's* namespace
block (`FileContext::namespace` / `namespace_at_offset`). In a file
that declares multiple `namespace` blocks, a function declared under
a namespace other than the call site's does not match any candidate,
even though it is already sitting in `global_functions` under its own
FQN.

`FileContext::resolve_name_at` (`src/types/mod.rs`) already solves
this correctly for other identifiers by consulting `resolved_names`
(mago-names' per-offset resolution, which understands multiple
namespace blocks in one file). `resolve_function_name` should try
`ctx.resolve_name_at(name, offset)` as a candidate before falling
back to the single-namespace guess.

This is why `src/completion/source/helpers.rs::
extract_function_return_from_source` still exists: it is a same-file
backward-text scanner for a function's `@return` docblock, used as a
fallback in `rhs_resolution.rs` (guarded by `!loader_found`) exactly
when `resolve_function_name` fails on this multi-namespace case. Once
`resolve_function_name` is offset-aware, this text scanner becomes
dead code and should be deleted along with its call site.

**Scope note:** `function_loader`/`function_loader_with`
(`src/resolution.rs`) are `Fn(&str) -> Option<FunctionInfo>` closures
bound once per `FileContext`, with no offset parameter, across ~30
call sites (completion, hover, definition, references, code actions).
Fixing this requires either threading the call expression's byte
offset through to `resolve_function_name` or adding an offset-aware
variant of the loader closure. Size the fix accordingly — this is not
a one-line change.

## B2. `self::`/`static::` inside macro closures resolve to the enclosing class, not the macro target

Inside a closure passed to `Target::macro(...)` (Laravel `Macroable`
or Carbon), `$this` correctly resolves to the macro target class via
`laravel_macro_this_resolver` (`closure_this_from_static_receiver` in
`src/completion/variable/closure_resolution.rs`). But `self::` and
`static::` inside the same closure still resolve to the class that
lexically encloses the registration (e.g. the service provider), so
Carbon's static macro idiom

```php
CarbonImmutable::macro('diffFromYear', function (int $year): string {
    return self::this()->diffForHumans(...);
});
```

reports a false `Method 'this' not found on class
'App\Providers\DemoServiceProvider'` (`unknown_member`). At runtime
both `Macroable::__call`/`__callStatic` and Carbon bind the closure
with the target as scope, so `self`/`static` refer to the target and
protected members like `Carbon\Traits\Mixin::this()` are accessible.

Subject resolution for `Self_`/`Static` needs the same "am I inside a
macro registration closure?" awareness that `$this` resolution already
has. The `self::this()` demo in
`examples/laravel/app/Providers/DemoServiceProvider.php` was switched
to the supported `$this->` form when this was discovered; restore the
static idiom there once fixed.
