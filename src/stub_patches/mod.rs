//! Centralized stub patch system for phpstorm-stubs deficiencies.
//!
//! The embedded [phpstorm-stubs](https://github.com/JetBrains/phpstorm-stubs)
//! sometimes lack `@template` annotations or have incomplete generic
//! interface declarations. We solve this by patching the parsed
//! [`FunctionInfo`] / [`ClassInfo`] at load time.
//!
//! This module provides two entry points:
//!
//! - [`apply_function_stub_patches`]: patches a freshly-parsed `FunctionInfo`
//!   (called from `find_or_load_function` after stub parsing).
//! - [`apply_class_stub_patches`]: patches a freshly-parsed `ClassInfo`
//!   (called from `parse_and_cache_content_versioned` for stub URIs).
//!
//! ## When to add a patch here vs. hardcoded logic elsewhere
//!
//! If the correct behaviour can be expressed with `@template` / `@return` /
//! `@implements` annotations (i.e. PHPStan's own stubs already have the
//! fix), it belongs here as a `FunctionInfo` or `ClassInfo` patch.  If the
//! behaviour requires inspecting call-site argument *values* at resolution
//! time (e.g. `array_map`'s callback return type), it must stay as hardcoded
//! logic in `rhs_resolution.rs` / `raw_type_inference.rs`.
//!
//! ## Patch inventory
//!
//! ### Function patches
//!
//! 1. **`range`** -- phpstorm-stubs return bare `array`.  We patch with a
//!    conditional return type: `($start is string ? list<string> : list<int|float>)`.
//!
//! 2. **`str_word_count`** -- phpstorm-stubs declare the flat union
//!    `string[]|int`.  We patch with a conditional return type keyed on
//!    `$format`, so the count, the word list, and the offset-keyed map each
//!    resolve on their own.
//!
//! 3. **The replace family** -- `preg_replace`, `preg_replace_callback`,
//!    `preg_replace_callback_array`, `preg_filter`, `str_replace`,
//!    `str_ireplace` and `substr_replace` all return an array when their
//!    subject is an array and a string when it is a string, but the stubs
//!    declare the flat union `array|string` (plus `null` for the `preg_`
//!    ones). We patch each with a conditional return type keyed on the
//!    subject, so a string subject stops carrying an impossible array
//!    branch. Mirrors PHPStan's `ReplaceFunctionsDynamicReturnTypeExtension`.
//!
//! 4. **`stream_bucket_make_writeable`** -- phpstorm-stubs type the
//!    return as `object|null` below PHP 8.4 (the `StreamBucket` class
//!    only exists from 8.4 onward). Bare `object` in a union is not
//!    recognised as the universal-container case, so property access
//!    on the result is unverifiable. We override the pre-8.4 case to
//!    `stdClass|null`, matching PHPStan's function map.
//!
//! 5. **`array_map`** / **`array_filter`** -- phpstorm-stubs type the
//!    callback as bare `callable` and the array as bare `array`, so a
//!    closure passed to them (`array_map(fn($x) => …, $items)`) leaves
//!    its parameter untyped. We add `@template TValue`, retype the
//!    callback's first parameter as `TValue`, the array as
//!    `array<TValue>`, and bind `TValue` from the array argument. Only
//!    the callback's *input* type is patched here; the callback's
//!    return type (and thus the function's own return) stays in the
//!    value-inspecting logic in `raw_type_inference.rs`.
//!
//!    **`usort`** / **`uasort`** / **`uksort`** are the same deficiency in
//!    the comparison-callback family: both parameters of
//!    `usort($errors, fn ($a, $b) => …)` are untyped until the array binds
//!    them. We add the `@template TKey`/`TValue` pair and retype the
//!    callback `callable(T, T): int` over whichever of the two it compares.
//!
//! 6. **`spl_autoload_register`** -- the callback is typed bare
//!    `?callable`, so the closure an autoloader is normally written as
//!    leaves its parameter untyped. We retype it
//!    `?callable(string): void`, which is what PHP actually calls the
//!    autoloader with.
//!
//! 7. **`ctype_*`** and **`define`** -- php-src declares both with `mixed`
//!    where the stubs narrow: the `ctype_*` family takes `mixed $text`
//!    (a non-string argument is a deprecation, not a type error), and
//!    `define`'s `$value` is `mixed` since PHP 8.0, not the pre-7.0
//!    scalar-or-array union the `@param` tag still spells out. We widen
//!    both back so `ctype_digit($int)` and
//!    `define('X', fopen(…))` stop being reported.
//!
//! 8. **Argument-decided builtins** -- `pathinfo`, `print_r`, `hrtime`,
//!    `microtime`, `getenv`, `mb_convert_encoding`, `abs`, `var_export`,
//!    `mb_internal_encoding`, `version_compare`, `sscanf`/`fscanf`,
//!    `array_reduce`, `pow` and `ini_get` each return one of several shapes
//!    depending on an argument, but the stubs can only declare the union of
//!    all of them. Each gets a conditional return type keyed on the deciding
//!    parameter, so a call that provably takes one branch stops carrying the
//!    others. An argument whose value cannot be pinned down keeps the union,
//!    which is all the call can promise.
//!
//!    Three of them are keyed on something other than a value: the `scanf`
//!    family on whether its variadic out-parameters were passed at all,
//!    `pow` on whether either operand can be an object, and `ini_get` on
//!    whether the directive is one of the core ones PHP always defines.
//!
//! 9. **Key/value array builtins** -- `array_keys`, `array_values`,
//!    `array_search`, `array_key_first`/`array_key_last` and `key` all
//!    answer in terms of the *caller's* key or value type, which the stubs
//!    spell out as `int[]|string[]`, `string|int|false` or a bare `array`
//!    because a signature without generics cannot say it. Each gets a
//!    `@template TKey of array-key` / `@template TValue` pair bound from the
//!    array argument. `array_flip` is the counter-example that shows why
//!    these belong here: it already ships the annotations and already
//!    resolves. The value-inspecting rules that cannot be written this way
//!    (`array_filter`'s falsy strip, `array_sum`'s element check, `range`'s
//!    all-bounds-at-once rule) stay in
//!    `type_engine::variable::array_func_rules`.
//!
//! 10. **`get_class`** -- declared bare `string`, so the one thing the call
//!     establishes (the result names *this* object's class) is lost at the
//!     assignment. Gets `@template T of object` / `@param T $object` /
//!     `@return class-string<T>`, matching PHPStan's stub.
//!
//! 11. **Benevolent builtins** -- `tempnam`, `curl_init`, `scandir`,
//!     `mktime` and the rest of [`crate::benevolent_builtins`] declare a
//!     failure branch that idiomatic PHP never checks. Their return type is
//!     tagged so the diagnostics stop enforcing that branch. Unlike the
//!     patches above this one is applied by name lookup rather than a
//!     hand-written function, because the list runs to a couple of hundred
//!     entries.
//!
//! ### Class patches
//!
//! 1. **`WeakMap`** -- phpstorm-stubs have `@template TKey of object`,
//!    `@template TValue`, `@template-implements IteratorAggregate<TKey, TValue>`
//!    but are still missing `@template-implements ArrayAccess<TKey, TValue>`.
//!
//! 2. **`IteratorIterator`** -- phpstorm-stubs lack `@template` and `@mixin`.
//!    PHPStan adds `@template TKey`, `@template TValue`,
//!    `@template TIterator of Traversable<TKey, TValue>`,
//!    `@implements OuterIterator<TKey, TValue>`,
//!    `@mixin TIterator`.  The `@mixin` makes methods from the wrapped
//!    iterator available on the wrapper.
//!    PHPStan ref: `stubs/iterable.stub`
//!
//! 3. **`FilterIterator`** -- extends `IteratorIterator` but stubs lack
//!    `@template` params.  PHPStan adds the same three template params
//!    and `@template-extends IteratorIterator<TKey, TValue, TIterator>`.
//!
//! 4. **`NoRewindIterator`**, **`CachingIterator`**, **`InfiniteIterator`**,
//!    **`LimitIterator`** -- all extend `IteratorIterator`.  Same template
//!    params + `@extends` generics + constructor binding `TIterator → $iterator`.
//!
//! 5. **`CallbackFilterIterator`** -- extends `FilterIterator`.
//!    Same template params + `@extends FilterIterator<TKey, TValue, TIterator>`
//!    + constructor binding.
//!
//! 6. **`ArrayIterator`** -- phpstorm-stubs declare `@template TKey of
//!    array-key` / `@template TValue` on the class but the constructor's
//!    `@param` is untyped `object|array`.  We bind `TKey`/`TValue` from
//!    the `$array` argument, matching PHPStan's stubs.
//!
//! 7. **`SimpleXMLElement`** -- `asXML()` and `saveXML()` are declared
//!    `string|bool`, but without a filename they serialise to a string and
//!    with one they report whether the write succeeded. Each gets a
//!    conditional return type keyed on `$filename`.
//!
//! 8. **`ReflectionClass`** -- `newInstanceArgs()` is declared
//!    `@return T|null` where `newInstance()` is `@return T`, so the same
//!    instantiation carries a null branch depending on which one built
//!    it. The method throws instead of returning null, so we drop the
//!    branch and the two stay in sync. `getInterfaceNames()` is declared
//!    bare `array`, losing the fact that reflection only reports interfaces
//!    that exist; it becomes `list<class-string>`, as in PHPStan's stub.
//!
//! 9. **`ReflectionObject`** -- the instance-only specialisation of
//!    `ReflectionClass`, but without its `@template T of object` or an
//!    `@extends ReflectionClass<T>`, so it forgets the class it reflects.
//!    PHPStan's stubs declare both, plus the constructor binding
//!    `T → $object`.
//!
//! 10. **Benevolent methods** -- the class-level half of function patch 11,
//!     covering `Redis`, `SplFileInfo`, the DOM classes, `PDO::prepare`,
//!     `DateTime::modify` and `Closure::bind`.
//!
//! ## Removing patches
//!
//! When phpstorm-stubs gains proper annotations for a patched symbol,
//! delete the corresponding patch function (in `functions.rs` or
//! `classes.rs`) and remove its dispatch from the entry point.  Run the test suite to verify that the stub's
//! own annotations produce the same result.

mod classes;
mod functions;

pub use classes::{apply_class_stub_patches, apply_third_party_class_patches};
pub use functions::apply_function_stub_patches;

#[cfg(test)]
mod tests;
