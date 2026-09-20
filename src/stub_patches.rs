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
//! delete the corresponding patch function here and remove its dispatch
//! from the entry point.  Run the test suite to verify that the stub's
//! own annotations produce the same result.

use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, FunctionInfo};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Function patches
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Apply all registered stub patches to a freshly-parsed function.
///
/// Called from [`find_or_load_function`](crate::resolution) after a
/// `FunctionInfo` is parsed from embedded phpstorm-stubs, before it is
/// cached in `global_functions`.  Only functions with known deficiencies
/// are patched; all others pass through unchanged.
pub fn apply_function_stub_patches(func: &mut FunctionInfo) {
    match func.name.as_str() {
        "range" => patch_range(func),
        "str_word_count" => patch_str_word_count(func),
        "stream_bucket_make_writeable" => patch_stream_bucket_make_writeable(func),
        "array_map" => patch_array_map(func),
        "array_filter" => patch_array_filter(func),
        "usort" | "uasort" => patch_user_sort(func, SortComparand::Value),
        "uksort" => patch_user_sort(func, SortComparand::Key),
        "array_fill_keys" => patch_array_fill_keys(func),
        "array_keys" => patch_array_key_value_generics(func, "$array", "list<TKey>"),
        "array_values" => patch_array_key_value_generics(func, "$array", "list<TValue>"),
        "array_search" => patch_array_key_value_generics(func, "$haystack", "TKey|false"),
        "array_key_first" | "array_key_last" => {
            patch_array_key_value_generics(func, "$array", "TKey|null")
        }
        "key" => patch_array_key_value_generics(func, "$array", "TKey|null"),
        "pathinfo" => patch_pathinfo(func),
        "print_r" => patch_print_r(func),
        "hrtime" => patch_hrtime(func),
        "microtime" => patch_microtime(func),
        "getenv" => patch_getenv(func),
        "mb_convert_encoding" => patch_mb_convert_encoding(func),
        "abs" => patch_abs(func),
        "var_export" => patch_var_export(func),
        "mb_internal_encoding" => patch_mb_internal_encoding(func),
        "version_compare" => patch_version_compare(func),
        "sscanf" => patch_scanf_family(func, "int", "array|null"),
        "fscanf" => patch_scanf_family(func, "int|false", "array|false|null"),
        "array_reduce" => patch_array_reduce(func),
        "pow" => patch_pow(func),
        "ini_get" => patch_ini_get(func),
        "get_class" => patch_get_class(func),
        "preg_replace"
        | "preg_replace_callback"
        | "preg_replace_callback_array"
        | "preg_filter" => patch_replace_family(func, "$subject", true),
        "str_replace" | "str_ireplace" => patch_replace_family(func, "$subject", false),
        "substr_replace" => patch_replace_family(func, "$string", false),
        "spl_autoload_register" => patch_spl_autoload_register(func),
        "define" => widen_parameter_to_mixed(func, "$value"),
        name if name.starts_with("ctype_") => widen_parameter_to_mixed(func, "$text"),
        _ => {}
    }
    if crate::benevolent_builtins::function_is_benevolent(&func.name) {
        mark_benevolent(&mut func.return_type);
    }
}

/// Tag a return type as one whose failure branch is not worth enforcing.
///
/// The type is unchanged in every other respect — see
/// [`crate::benevolent_builtins`] — and a return type that is not a union
/// on this PHP version comes back untagged.
fn mark_benevolent(return_type: &mut Option<PhpType>) {
    if let Some(ty) = return_type.take() {
        *return_type = Some(PhpType::benevolent(ty));
    }
}

/// Widen a parameter the stubs type more narrowly than php-src does.
///
/// `ctype_digit()` and its siblings take `mixed $text` (passing an int is a
/// deprecation, not a type error), and `define()`'s `$value` has been `mixed`
/// since PHP 8.0, but the stubs keep the old `string` hint and the pre-7.0
/// `null|array|bool|int|float|string` `@param` tag respectively. Both the
/// docblock type and the native hint are widened so hover shows what the
/// function really accepts.
fn widen_parameter_to_mixed(func: &mut FunctionInfo, param_name: &str) {
    for param in func.parameters.make_mut() {
        if param.name == param_name {
            param.type_hint = Some(PhpType::mixed());
            param.native_type_hint = Some(PhpType::mixed());
        }
    }
}

/// Spell out `spl_autoload_register`'s autoloader signature.
///
/// phpstorm-stubs type the callback as bare `?callable`, so the closure
/// idiom `spl_autoload_register(function ($class) { … })` leaves
/// `$class` untyped and every string operation on it widens to the full
/// union its argument allows.  PHP always calls the autoloader with the
/// requested class name and ignores whatever it returns, which is
/// exactly `callable(string): void`.  The nullable wrapper stays: calling
/// `spl_autoload_register()` with no arguments is still valid.
fn patch_spl_autoload_register(func: &mut FunctionInfo) {
    // Expected stub shape: `spl_autoload_register(?callable $callback = null, …)`.
    if func
        .parameters
        .first()
        .is_none_or(|p| p.name.as_str() != "$callback")
    {
        return;
    }
    let hint = PhpType::parse("?callable(string): void");
    for param in func.parameters.make_mut() {
        if param.name.as_str() == "$callback" {
            param.type_hint = Some(hint.clone());
        }
    }
}

/// Link `array_map`'s callback parameter to the input array's element
/// type so a closure passed to it gets its parameter typed.
///
/// phpstorm-stubs declare the callback as bare `callable|null` and the
/// array as bare `array`, so `array_map(fn($x) => $x->foo(), $items)`
/// leaves `$x` untyped.  We add `@template TValue`, retype the callback
/// as `callable(TValue): mixed`, and the array as `array<TValue>`, then
/// bind `TValue` from the array argument.  The callback's *return* type
/// (and thus `array_map`'s own return) is still resolved by the
/// value-inspecting logic in `raw_type_inference.rs`, which this patch
/// leaves untouched by keeping the bare `array` return type.
fn patch_array_map(func: &mut FunctionInfo) {
    // Expected stub shape: `array_map(?callable $callback, array $array, …)`.
    let callback_name = match func.parameters.first() {
        Some(p) if p.name.as_str() == "$callback" => p.name,
        _ => return,
    };
    let array_name = match func.parameters.get(1) {
        Some(p) if p.name.as_str() == "$array" => p.name,
        _ => return,
    };
    link_callback_to_array_element(func, callback_name, array_name, "mixed");
}

/// Link `array_filter`'s callback parameter to the input array's element
/// type (the callback receives each element and its result is tested for
/// truthiness).
///
/// Unlike `array_map`, `array_filter` takes the array first and the
/// callback second: `array_filter(array $array, ?callable $callback, …)`.
///
/// The callback's return stays `mixed` because that is what PHP accepts:
/// `array_filter($items, fn ($i) => preg_match($re, $i))` keeps every
/// element the callback returns a truthy value for, and typing the
/// return as `bool` would call that idiom a type error.
fn patch_array_filter(func: &mut FunctionInfo) {
    let array_name = match func.parameters.first() {
        Some(p) if p.name.as_str() == "$array" => p.name,
        _ => return,
    };
    let callback_name = match func.parameters.get(1) {
        Some(p) if p.name.as_str() == "$callback" => p.name,
        _ => return,
    };
    link_callback_to_array_element(func, callback_name, array_name, "mixed");
}

/// Which half of the array a user-comparison sort hands its callback.
#[derive(Copy, Clone)]
enum SortComparand {
    /// `usort`/`uasort` compare values.
    Value,
    /// `uksort` compares keys.
    Key,
}

/// Spell out the comparison callback of `usort`, `uasort` and `uksort`.
///
/// phpstorm-stubs declare all three as
/// `usort(array &$array, callable $callback)`, so the comparison closure
/// idiom `usort($errors, fn ($a, $b) => $a->getLine() <=> $b->getLine())`
/// leaves both parameters untyped. PHP calls the callback with two
/// elements of the array it is sorting: two values for `usort`/`uasort`,
/// two keys for `uksort`. We add the `@template` pair, retype the array as
/// `array<TKey, TValue>` and the callback as `callable(T, T): int` over
/// whichever of the two it compares, then bind both from the array
/// argument. Mirrors PHPStan's own `stubs/arrayFunctions.stub`.
fn patch_user_sort(func: &mut FunctionInfo, comparand: SortComparand) {
    const TKEY: &str = "TKey";
    const TVALUE: &str = "TValue";

    // Expected stub shape: `usort(array &$array, callable $callback)`.
    let array_name = match func.parameters.first() {
        Some(p) if p.name.as_str() == "$array" => p.name,
        _ => return,
    };
    let callback_name = match func.parameters.get(1) {
        Some(p) if p.name.as_str() == "$callback" => p.name,
        _ => return,
    };

    let compared = match comparand {
        SortComparand::Value => TVALUE,
        SortComparand::Key => TKEY,
    };
    let array_hint = PhpType::parse(&format!("array<{TKEY}, {TVALUE}>"));
    let callback_hint = PhpType::parse(&format!("callable({compared}, {compared}): int"));

    for param in func.parameters.make_mut() {
        if param.name == array_name {
            param.type_hint = Some(array_hint.clone());
        } else if param.name == callback_name {
            param.type_hint = Some(callback_hint.clone());
        }
    }

    func.template_params = vec![atom(TKEY), atom(TVALUE)];
    // A bare `array` argument binds neither param, and `array-key` is
    // PHP's own answer for a key that nothing narrowed.
    func.template_param_bounds = [(atom(TKEY), PhpType::parse("array-key"))]
        .into_iter()
        .collect();
    // Only the array argument can bind them: the callback is the very
    // thing whose parameters these templates are there to type.
    func.template_bindings = vec![(atom(TKEY), array_name), (atom(TVALUE), array_name)];
}

/// Shared helper: give `func` a single `TValue` template bound from the
/// array parameter, and retype the callback as
/// `callable(TValue): <callback_return>` so a closure argument's first
/// parameter is inferred as the array's element type.
fn link_callback_to_array_element(
    func: &mut FunctionInfo,
    callback_name: crate::atom::Atom,
    array_name: crate::atom::Atom,
    callback_return: &str,
) {
    const TVALUE: &str = "TValue";
    let callback_hint = PhpType::parse(&format!("callable({}): {}", TVALUE, callback_return));
    let array_hint = PhpType::parse(&format!("array<{}>", TVALUE));

    for param in func.parameters.make_mut() {
        if param.name == callback_name {
            param.type_hint = Some(callback_hint.clone());
        } else if param.name == array_name {
            param.type_hint = Some(array_hint.clone());
        }
    }

    func.template_params = vec![atom(TVALUE)];
    func.template_param_bounds = Default::default();
    // Bind `TValue` from the array argument only.  The callback argument
    // (an unannotated closure) can't bind it, and listing it would just
    // add a no-op binding attempt.
    func.template_bindings = vec![(atom(TVALUE), array_name)];
}

/// Give a key- or value-returning array builtin the `@template` pair the
/// stubs leave off, so its return substitutes the caller's own generics.
///
/// phpstorm-stubs spell these out as concrete unions (`array_keys` returns
/// `int[]|string[]`, `array_search` returns `string|int|false`) or as a
/// bare `array`, which is the widest thing the signature can say without
/// generics. `array_flip` is the counter-example that shows the machinery
/// already works: it ships a real `@template` pair and resolves correctly
/// today, so the fix for the rest is to annotate them the same way rather
/// than to add per-function logic in Rust.
///
/// `array_param` is the parameter the generics bind from (`$haystack` for
/// `array_search`, `$array` for everything else) and `return_type` is
/// written in terms of `TKey`/`TValue`.
fn patch_array_key_value_generics(func: &mut FunctionInfo, array_param: &str, return_type: &str) {
    const TKEY: &str = "TKey";
    const TVALUE: &str = "TValue";

    let array_name = match func
        .parameters
        .iter()
        .find(|p| p.name.as_str() == array_param)
    {
        Some(p) => p.name,
        None => return,
    };

    let array_hint = PhpType::parse(&format!("array<{TKEY}, {TVALUE}>"));
    for param in func.parameters.make_mut() {
        if param.name == array_name {
            param.type_hint = Some(array_hint.clone());
        }
    }

    func.return_type = Some(PhpType::parse(return_type));
    func.template_params = vec![atom(TKEY), atom(TVALUE)];
    // A bare `array` argument binds neither param. `TKey` still has PHP's
    // own answer to fall back on — an array key is an `array-key` — which
    // beats the `mixed` an undeclared bound would leave behind.
    func.template_param_bounds = [(atom(TKEY), PhpType::parse("array-key"))]
        .into_iter()
        .collect();
    func.template_bindings = vec![(atom(TKEY), array_name), (atom(TVALUE), array_name)];
}

/// Give `array_fill_keys()` the generics that turn its two arguments into
/// the result's key and value types.
///
/// The stub is `array_fill_keys(array $keys, mixed $value): array`, so a
/// caller loses the one thing the call establishes: the keys of the result
/// are exactly the *values* of `$keys`. That matters downstream, because
/// `array_keys(array_fill_keys($names, true))` should hand back the
/// `$names` it started from rather than a bare `array-key`.
///
/// Unlike [`patch_array_key_value_generics`], `TKey` binds from the array
/// parameter's *element* type, not its key type.
fn patch_array_fill_keys(func: &mut FunctionInfo) {
    const TKEY: &str = "TKey";
    const TVALUE: &str = "TValue";

    let keys_name = match func.parameters.first() {
        Some(p) if p.name.as_str() == "$keys" => p.name,
        _ => return,
    };
    let value_name = match func.parameters.get(1) {
        Some(p) if p.name.as_str() == "$value" => p.name,
        _ => return,
    };

    let keys_hint = PhpType::parse(&format!("array<{TKEY}>"));
    let value_hint = PhpType::parse(TVALUE);
    for param in func.parameters.make_mut() {
        if param.name == keys_name {
            param.type_hint = Some(keys_hint.clone());
        } else if param.name == value_name {
            param.type_hint = Some(value_hint.clone());
        }
    }

    func.return_type = Some(PhpType::parse(&format!("array<{TKEY}, {TVALUE}>")));
    func.template_params = vec![atom(TKEY), atom(TVALUE)];
    // `$keys` whose element type is unknown leaves `TKey` on PHP's own
    // answer — whatever a `foreach` writes into an array is an `array-key`.
    func.template_param_bounds = [(atom(TKEY), PhpType::parse("array-key"))]
        .into_iter()
        .collect();
    func.template_bindings = vec![(atom(TKEY), keys_name), (atom(TVALUE), value_name)];
}

/// Patch `range()` to have a conditional return type.
///
/// phpstorm-stubs declare `range()` as returning bare `array`.
/// PHPStan infers `list<int>`, `list<float>`, or `list<string>` depending
/// on the argument types.  We approximate this with:
/// `($start is string ? list<string> : list<int|float>)`.
///
/// Splitting the numeric branch needs every bound at once — a single
/// fractional one makes the whole range fractional — which a conditional
/// keyed on one parameter cannot ask. That half lives in
/// `type_engine::variable::array_func_rules` and answers first; this
/// conditional is what a range it cannot pin down falls back to.
fn patch_range(func: &mut FunctionInfo) {
    func.conditional_return = Some(PhpType::conditional(
        "$start",
        false,
        PhpType::named(atom("string")),
        PhpType::list(PhpType::string()),
        PhpType::list(PhpType::union(vec![PhpType::int(), PhpType::float()])),
    ));
}

/// Patch `array_reduce()` to have a conditional return type keyed on
/// `$initial`.
///
/// The stub's `@return TCarry|null` covers the one case that really can
/// produce `null`: an empty array with no initial value. Handing the call an
/// initial value makes that the result instead, so the `null` branch cannot
/// happen and every reduction over a seeded accumulator carries a nullable it
/// never is.
fn patch_array_reduce(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$initial",
        PhpType::null(),
        PhpType::union(vec![PhpType::named(atom("TCarry")), PhpType::null()]),
        PhpType::named(atom("TCarry")),
    );
}

/// Give `get_class()` the generic that ties its result to the object it was
/// asked about.
///
/// The stub returns bare `string`, so the one thing the call establishes — the
/// result names *this* object's class — is lost the moment it is assigned.
/// PHPStan's stub declares `@template T of object` / `@param T $object` /
/// `@return class-string<T>`, which keeps `new ($className)` and
/// `$className::create()` resolvable and lets the result satisfy a
/// `class-string` parameter.
fn patch_get_class(func: &mut FunctionInfo) {
    const T: &str = "T";
    let Some(object) = func.parameters.first().map(|p| p.name) else {
        return;
    };
    for param in func.parameters.make_mut() {
        if param.name == object {
            param.type_hint = Some(PhpType::named(atom(T)));
        }
    }
    func.return_type = Some(PhpType::parse("class-string<T>"));
    func.template_params = vec![atom(T)];
    // The no-argument form (deprecated, and removed in 8.4) asks about the
    // enclosing class, which the binding cannot see. `object` keeps that call
    // on `class-string<object>` — still a class-string, just not a specific
    // one.
    func.template_param_bounds = [(atom(T), PhpType::named(atom("object")))]
        .into_iter()
        .collect();
    func.template_bindings = vec![(atom(T), object)];
}

/// Patch `ini_get()` to have a conditional return type keyed on `$option`.
///
/// `false` means "no such directive", so an option PHP always defines cannot
/// produce it. The stubs declare `string|false` for every call, which puts a
/// failure branch on the `ini_get('memory_limit')` idiom that no amount of
/// checking can reach.
///
/// The list is PHPStan's (`IniGetReturnTypeExtension`): the core directives it
/// is willing to promise are always set. Anything outside it keeps the
/// declared union, since a directive an extension registers really can be
/// missing.
fn patch_ini_get(func: &mut FunctionInfo) {
    const ALWAYS_SET: [&str; 7] = [
        "date.timezone",
        "memory_limit",
        "max_memory_limit",
        "max_execution_time",
        "max_input_time",
        "default_socket_timeout",
        "precision",
    ];
    let Some(option) = func.parameters.first().map(|p| p.name) else {
        return;
    };
    conditional_on(
        func,
        option.as_str(),
        PhpType::union(
            ALWAYS_SET
                .iter()
                .map(PhpType::literal_string_value)
                .collect(),
        ),
        PhpType::string(),
        PhpType::union(vec![PhpType::string(), PhpType::named(atom("false"))]),
    );
}

/// Patch `pow()` to have a conditional return type keyed on its operands.
///
/// The `object` branch only exists for the operator-overloading extensions
/// (GMP, BCMath): raising two numbers to a power can only produce a number.
/// The stubs declare `object|int|float` for every call, so arithmetic on the
/// result of an ordinary `pow(2, $n)` is checked against a class.
/// An operand nobody typed decides nothing, and the union of both
/// branches would put `object` back into every such call — so both
/// conditionals here take the numeric branch unless an operand is
/// provably an object.
fn patch_pow(func: &mut FunctionInfo) {
    let numeric = PhpType::union(vec![PhpType::int(), PhpType::float()]);
    let object = PhpType::named(atom("object"));
    func.conditional_return = Some(PhpType::conditional_defaulting_to_else(
        "$num",
        false,
        object.clone(),
        object.clone(),
        PhpType::conditional_defaulting_to_else(
            "$exponent",
            false,
            object.clone(),
            object,
            numeric,
        ),
    ));
}

/// Patch `str_word_count()` to have a conditional return type.
///
/// phpstorm-stubs declare the flat union `string[]|int`, but the return type
/// is decided by `$format`: `0` (the default) counts the words, `1` lists
/// them, and `2` maps each word to the offset it starts at. A `$format` that
/// isn't a literal leaves the declared union, which is all the call site can
/// promise.
fn patch_str_word_count(func: &mut FunctionInfo) {
    let count = PhpType::int();
    let words = PhpType::list(PhpType::string());
    let words_by_offset = PhpType::generic_array(PhpType::int(), PhpType::string());
    let unknown_format = PhpType::union(vec![words.clone(), count.clone()]);

    func.conditional_return = Some(PhpType::conditional(
        "$format",
        false,
        PhpType::literal_int("0"),
        count,
        PhpType::conditional(
            "$format",
            false,
            PhpType::literal_int("1"),
            words,
            PhpType::conditional(
                "$format",
                false,
                PhpType::literal_int("2"),
                words_by_offset,
                unknown_format,
            ),
        ),
    ));
}

/// Give `func` a conditional return type keyed on one of its parameters.
///
/// Bails out when the stub does not declare that parameter: without it the
/// conditional would be decided against whichever argument happened to land
/// in slot 0, which is worse than the declared union.
fn conditional_on(
    func: &mut FunctionInfo,
    param_name: &str,
    condition: PhpType,
    then_type: PhpType,
    else_type: PhpType,
) {
    if !func.parameters.iter().any(|p| p.name == param_name) {
        return;
    }
    func.conditional_return = Some(PhpType::conditional(
        param_name, false, condition, then_type, else_type,
    ));
}

/// Patch `pathinfo()` to have a conditional return type keyed on `$flags`.
///
/// The stubs declare the flat union `string|array{…}`, but only the
/// all-elements form returns the array: any other flag asks for one component
/// and gets a `string` back. `PATHINFO_ALL` is the parameter's declared
/// default, so the one-argument call takes the array branch through the same
/// route an explicit `PATHINFO_ALL` does.
///
/// `extension` is optional in the shape because a path without a dot has no
/// extension key at all. Mirrors PHPStan's
/// `PathinfoFunctionDynamicReturnTypeExtension`.
fn patch_pathinfo(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$flags",
        PhpType::literal_int(PATHINFO_ALL.to_string()),
        PhpType::parse(
            "array{dirname: string, basename: string, extension?: string, filename: string}",
        ),
        PhpType::string(),
    );
}

/// `PATHINFO_ALL`, as defined by PHP's `ext/standard/string.h`.
///
/// Spelled out rather than read back from the stubs because the conditional is
/// built when the function is parsed, before any constant lookup is available.
/// It is part of PHP's stable ABI.
const PATHINFO_ALL: i64 = 15;

/// Patch `print_r()` to have a conditional return type keyed on `$return`.
///
/// The stubs declare `string|bool` (`string|true` from 8.4 on). php-src only
/// ever returns `true` when it printed, so the `false` half is impossible and
/// the `string` half only exists for `print_r($v, true)`.
fn patch_print_r(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$return",
        PhpType::parse("true"),
        PhpType::string(),
        PhpType::parse("true"),
    );
}

/// Patch `hrtime()` to have a conditional return type keyed on `$as_number`.
///
/// The stubs declare `int[]|int|float|false`, but the two shapes are decided
/// by the argument: the number form is an `int` (a `float` on 32-bit builds)
/// and the array form is the `[seconds, nanoseconds]` pair.
fn patch_hrtime(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$as_number",
        PhpType::parse("true"),
        PhpType::union(vec![PhpType::int(), PhpType::float()]),
        PhpType::parse("array{int, int}|false"),
    );
}

/// Patch `microtime()` to have a conditional return type keyed on `$as_float`.
///
/// The stubs carry `#[TypeContract(true: "float", false: "string")]` on the
/// parameter, which says exactly this, but the attribute is not read; the
/// declared `string|float` union is all that survives.
fn patch_microtime(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$as_float",
        PhpType::parse("true"),
        PhpType::float(),
        PhpType::string(),
    );
}

/// Patch `getenv()` to have a conditional return type keyed on its name
/// argument.
///
/// Only the no-argument form returns the whole environment; naming a variable
/// returns its value, or `false` when it is not set. The stubs declare the
/// union of both, so every `getenv('NAME')` carries an impossible array
/// branch.
///
/// The parameter is keyed by position rather than by name: the stubs declare
/// both the pre-7.1 `$varname` and the current `$name`, and which one is in
/// play depends on the configured PHP version.
fn patch_getenv(func: &mut FunctionInfo) {
    let Some(name_param) = func.parameters.first().map(|p| p.name) else {
        return;
    };
    conditional_on(
        func,
        name_param.as_str(),
        PhpType::null(),
        PhpType::generic_array(PhpType::string(), PhpType::string()),
        PhpType::union(vec![PhpType::string(), PhpType::named(atom("false"))]),
    );
}

/// Patch `mb_convert_encoding()` to have a conditional return type keyed on
/// its subject.
///
/// Like the replace family, the function answers in the shape it was handed,
/// but the stubs can only declare `array|string|false`. An array subject is
/// converted per element, so no error branch survives there.
fn patch_mb_convert_encoding(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$string",
        PhpType::array(),
        PhpType::generic_array(PhpType::named(atom("array-key")), PhpType::string()),
        PhpType::union(vec![PhpType::string(), PhpType::named(atom("false"))]),
    );
}

/// Patch `abs()` to have a conditional return type keyed on `$num`.
///
/// `abs()` returns the type it was given; the declared `int|float` union
/// leaves an `int` argument's result carrying a `float` branch that cannot
/// happen. An argument that is neither (a numeric string, a `mixed`) leaves
/// both branches, which is all the call can promise.
fn patch_abs(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$num",
        PhpType::int(),
        PhpType::int(),
        PhpType::float(),
    );
}

/// Patch `var_export()` to have a conditional return type keyed on
/// `$return`.
///
/// The stubs declare `?string` for both forms, so the rendered string a
/// `var_export($v, true)` is written for carries a `null` it cannot be, and
/// the printing form promises a string it never hands back.
fn patch_var_export(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$return",
        PhpType::parse("true"),
        PhpType::string(),
        PhpType::null(),
    );
}

/// Patch `mb_internal_encoding()` to have a conditional return type keyed on
/// `$encoding`.
///
/// The one function is both the getter and the setter: without an argument it
/// reports the current internal encoding, and with one it reports whether the
/// change took. The stubs declare `string|bool` for both.
fn patch_mb_internal_encoding(func: &mut FunctionInfo) {
    let Some(encoding) = func.parameters.first().map(|p| p.name) else {
        return;
    };
    conditional_on(
        func,
        encoding.as_str(),
        PhpType::null(),
        PhpType::string(),
        PhpType::bool(),
    );
}

/// Patch `version_compare()` to have a conditional return type keyed on
/// `$operator`.
///
/// Naming an operator asks a yes/no question and gets a `bool`; leaving it out
/// asks for the ordering and gets `-1`, `0` or `1`. The stubs declare the
/// union of both.
fn patch_version_compare(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$operator",
        PhpType::null(),
        PhpType::int(),
        PhpType::bool(),
    );
}

/// Patch a member of the `scanf` family to have a conditional return type
/// keyed on its variadic out-parameters.
///
/// `sscanf($s, $format)` collects the parsed values into an array and returns
/// it; passing by-reference targets instead writes into them and returns how
/// many were assigned. The stubs carry exactly this on the variadic
/// (`#[TypeContract(exists: …, notExists: …)]`) but the attribute is not read,
/// leaving the flat union both forms share.
///
/// `assigned` is the count branch (`fscanf` adds a `false` for a read
/// failure); `collected` is the array branch.
fn patch_scanf_family(func: &mut FunctionInfo, assigned: &str, collected: &str) {
    let Some(vars) = func
        .parameters
        .last()
        .filter(|p| p.is_variadic)
        .map(|p| p.name)
    else {
        return;
    };
    conditional_on(
        func,
        vars.as_str(),
        PhpType::null(),
        PhpType::parse(collected),
        PhpType::parse(assigned),
    );
}

/// Patch a member of the replace family to have a conditional return type
/// keyed on its subject argument.
///
/// `preg_replace`, `str_replace` and their relatives take a subject that may
/// be either a string or an array of strings, and return the same shape they
/// were given. The stubs can only declare the flat union (`array|string`, plus
/// `null` for the `preg_` family, which returns `null` on a PCRE error), so a
/// call with a string subject carries an array branch that cannot happen, and
/// vice versa.
///
/// `subject_param` names the parameter holding the subject (`$string` for
/// `substr_replace`, `$subject` for the rest) and `nullable_on_error` marks
/// the functions whose string branch keeps the `null` error result. An array
/// subject is answered per element, so no error branch survives there.
///
/// A subject whose type cannot be pinned down at the call site leaves the
/// declared union, which is all the call can promise.
fn patch_replace_family(func: &mut FunctionInfo, subject_param: &str, nullable_on_error: bool) {
    // Bail out if the stub does not have the parameter the conditional keys
    // on: without it the subject cannot be identified and the conditional
    // would be decided against an unrelated argument.
    if !func.parameters.iter().any(|p| p.name == subject_param) {
        return;
    }

    // The subject's keys carry over untouched, so a string-keyed subject
    // keeps its string keys — hence `array-key` rather than `int`.
    let replaced_array =
        PhpType::generic_array(PhpType::named(atom("array-key")), PhpType::string());
    let replaced_string = if nullable_on_error {
        PhpType::union(vec![PhpType::string(), PhpType::null()])
    } else {
        PhpType::string()
    };

    func.conditional_return = Some(PhpType::conditional(
        subject_param,
        false,
        PhpType::array(),
        replaced_array,
        replaced_string,
    ));
}

/// Override the pre-8.4 return type of `stream_bucket_make_writeable()`.
///
/// phpstorm-stubs resolve the return type to bare `object|null` for PHP
/// versions before 8.4 (the `StreamBucket` class was only introduced in
/// 8.4). Bare `object` inside a union is not recognised by the type
/// engine's universal-container fallback the way `object` or `?object`
/// alone are, so `$bucket->data` / `$bucket->datalen` become
/// unverifiable. PHPStan's function map overrides this same case to
/// `stdClass|null`, which the type engine already treats as accepting
/// arbitrary properties. The real PHP 8.4+ `StreamBucket|null` type is
/// left untouched.
fn patch_stream_bucket_make_writeable(func: &mut FunctionInfo) {
    if func.return_type.as_ref().is_some_and(is_pre_84_object_type) {
        func.return_type = Some(PhpType::parse("stdClass|null"));
    }
    if func
        .native_return_type
        .as_ref()
        .is_some_and(is_pre_84_object_type)
    {
        func.native_return_type = Some(PhpType::parse("stdClass|null"));
    }
}

/// Whether `ty` is the pre-8.4 `object|null` (or bare `object`) shape,
/// as opposed to the real `StreamBucket|null` type used from 8.4 on.
fn is_pre_84_object_type(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(name) => name.eq_ignore_ascii_case("object"),
        TypeKind::Nullable(inner) => is_pre_84_object_type(inner),
        TypeKind::Union(members) => members.iter().any(is_pre_84_object_type),
        _ => false,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Class patches
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Apply all registered stub patches to a freshly-parsed class.
///
/// Called from [`parse_and_cache_content_versioned`](crate::resolution)
/// after a `ClassInfo` is parsed from embedded phpstorm-stubs, before it
/// is cached in `uri_classes_index` and `fqn_index`.  Only classes with known
/// deficiencies are patched; all others pass through unchanged.
///
/// This is the class-level counterpart of [`apply_function_stub_patches`].
pub fn apply_class_stub_patches(class: &mut ClassInfo) {
    match class.name.as_str() {
        "WeakMap" => patch_weak_map(class),
        "IteratorIterator" => patch_iterator_iterator(class),
        "RecursiveIteratorIterator" => patch_recursive_iterator_iterator(class),
        "FilterIterator" => patch_filter_iterator(class),
        "NoRewindIterator" => patch_no_rewind_iterator(class),
        "CachingIterator" => patch_caching_iterator(class),
        "InfiniteIterator" => patch_infinite_iterator(class),
        "LimitIterator" => patch_limit_iterator(class),
        "CallbackFilterIterator" => patch_callback_filter_iterator(class),
        "ArrayIterator" => patch_array_iterator(class),
        "SimpleXMLElement" => patch_simple_xml_element(class),
        "ReflectionClass" => patch_reflection_class(class),
        "ReflectionObject" => patch_reflection_object(class),
        _ => {}
    }
    mark_benevolent_methods(class);
}

/// A `@phpstan-assert-if-true` promise a third-party class makes in its
/// implementation but forgets to declare.
///
/// Each entry is `(class FQN, predicate method, member the predicate
/// proves non-null)`. The member is spelled as the tag would spell it, so
/// the narrowing that reads it needs no special case of its own.
///
/// This list exists only for promises the library *documents elsewhere*
/// (in prose, or by annotating its siblings) — never to paper over a
/// method that really can return null. PHPStan annotates `isInTrait()`
/// with `@phpstan-assert-if-true !null $this->getTraitReflection()` and
/// leaves the identical `isInClass()` bare, and every PHPStan extension
/// is written against the pairing regardless.
const THIRD_PARTY_ASSERT_IF_TRUE: &[(&str, &str, &str)] = &[(
    "PHPStan\\Analyser\\Scope",
    "isInClass",
    "$this->getClassReflection()",
)];

/// Supply the `@phpstan-assert-if-true` tags that [`THIRD_PARTY_ASSERT_IF_TRUE`]
/// records, for a class parsed from the user's project or its vendor tree.
///
/// Separate from [`apply_class_stub_patches`], which is deliberately
/// confined to the embedded stubs: this one has to reach vendor code, so
/// it does nothing at all for a class whose FQN is not in the list.
pub fn apply_third_party_class_patches(class: &mut ClassInfo) {
    let fqn = class.fqn();
    for (class_fqn, method_name, subject) in THIRD_PARTY_ASSERT_IF_TRUE {
        if fqn.as_str() != *class_fqn {
            continue;
        }
        let Some(idx) = class
            .methods
            .iter()
            .position(|m| m.name.as_str() == *method_name)
        else {
            continue;
        };
        // A version that grew the tag upstream keeps its own.
        if !class.methods[idx].type_assertions.is_empty() {
            continue;
        }
        let mut method = (*class.methods[idx]).clone();
        method.type_assertions.push(crate::types::TypeAssertion {
            kind: crate::types::AssertionKind::IfTrue,
            param_name: (*subject).to_string(),
            asserted_type: PhpType::null(),
            negated: true,
            is_equality: false,
        });
        class.methods.make_mut()[idx] = std::sync::Arc::new(method);
    }
}

/// Tag the class's benevolent methods (`Redis::get`, `SplFileInfo::getSize`,
/// `DateTime::modify`, …) so their `|false` branch stops being enforced at
/// call sites.
fn mark_benevolent_methods(class: &mut ClassInfo) {
    if !crate::benevolent_builtins::class_has_benevolent_methods(&class.name) {
        return;
    }
    for idx in 0..class.methods.len() {
        if !crate::benevolent_builtins::method_is_benevolent(&class.name, &class.methods[idx].name)
        {
            continue;
        }
        let Some(tagged) = class.methods[idx]
            .return_type
            .as_ref()
            .map(|ty| PhpType::benevolent(ty.clone()))
            .filter(|tagged| tagged.is_benevolent())
        else {
            continue;
        };
        let mut method = (*class.methods[idx]).clone();
        method.return_type = Some(tagged);
        class.methods.make_mut()[idx] = std::sync::Arc::new(method);
    }
}

/// Add `@implements ArrayAccess<TKey, TValue>` for WeakMap.
///
/// Upstream phpstorm-stubs have `@template TKey of object`, `@template TValue`,
/// and `@template-implements IteratorAggregate<TKey, TValue>`, but are still
/// missing `@template-implements ArrayAccess<TKey, TValue>`.
fn patch_weak_map(class: &mut ClassInfo) {
    add_implements_generics(class, "ArrayAccess", &["TKey", "TValue"]);
}

/// Add `@template TKey`, `@template TValue`,
/// `@template TIterator of Traversable<TKey, TValue>`,
/// `@implements OuterIterator<TKey, TValue>`,
/// `@mixin TIterator`.
///
/// PHPStan ref: `stubs/iterable.stub`
fn patch_iterator_iterator(class: &mut ClassInfo) {
    if !class.template_params.is_empty() {
        return;
    }
    add_templates(class, &[("TKey", None), ("TValue", None)]);
    // TIterator has a complex bound `Traversable<TKey, TValue>` — add it
    // manually since `add_templates` only handles simple string bounds.
    let t_iter = atom("TIterator");
    if !class.template_params.contains(&t_iter) {
        class.template_params.push(t_iter);
    }
    class
        .template_param_bounds
        .entry(atom("TIterator"))
        .or_insert_with(|| {
            PhpType::generic(
                "Traversable",
                vec![PhpType::named(atom("TKey")), PhpType::named(atom("TValue"))],
            )
        });
    add_implements_generics(class, "OuterIterator", &["TKey", "TValue"]);
    // Add @mixin TIterator so that methods from the wrapped iterator
    // are available on the wrapper.
    if !class.mixins.contains(&t_iter) {
        class.mixins.push(t_iter);
    }

    // Patch current() → TValue and key() → TKey.
    // phpstorm-stubs declare `current(): mixed` and `key(): mixed` which
    // hides the generic type.  PHPStan's stubs override these.
    patch_method_return_type(class, "current", PhpType::named(atom("TValue")));
    patch_method_return_type(class, "key", PhpType::named(atom("TKey")));

    // Patch the constructor: add template binding TIterator → $iterator
    // so that `new IteratorIterator(new Subject())` infers TIterator = Subject.
    if let Some(ctor_idx) = class
        .methods
        .iter()
        .position(|m| m.name.as_str() == "__construct")
    {
        let mut ctor = (*class.methods[ctor_idx]).clone();
        let binding = (atom("TIterator"), atom("$iterator"));
        if !ctor.template_bindings.iter().any(|(t, _)| t == &binding.0) {
            ctor.template_bindings.push(binding);
        }
        // Update the parameter type hint from Traversable to TIterator
        // so that classify_template_binding recognises a Direct binding.
        if let Some(param) = ctor
            .parameters
            .make_mut()
            .iter_mut()
            .find(|p| p.name == "$iterator")
        {
            param.type_hint = Some(PhpType::named(atom("TIterator")));
        }
        class.methods.make_mut()[ctor_idx] = std::sync::Arc::new(ctor);
    }
}

/// Add `@template T of RecursiveIterator|IteratorAggregate` and
/// `@mixin T`, bound from the constructor's `$iterator` argument.
///
/// `RecursiveIteratorIterator` does not extend `IteratorIterator`, so it
/// gets its own patch rather than
/// [`patch_iterator_iterator_subclass`]. phpstorm-stubs type the
/// constructor `Traversable $iterator` and every accessor `mixed`, so the
/// directory-walk idiom
/// `foreach (new RecursiveIteratorIterator(new RecursiveDirectoryIterator($dir)) as $file)`
/// leaves `$file` with nothing behind it. Binding `T` to the wrapped
/// iterator and mixing it in is what gives the traversal its element type.
///
/// PHPStan ref: `stubs/iterable.stub`
fn patch_recursive_iterator_iterator(class: &mut ClassInfo) {
    if !class.template_params.is_empty() {
        return;
    }
    let t = atom("T");
    class.template_params.push(t);
    class.template_param_bounds.entry(t).or_insert_with(|| {
        PhpType::union(vec![
            PhpType::named(atom("RecursiveIterator")),
            PhpType::named(atom("IteratorAggregate")),
        ])
    });
    if !class.mixins.contains(&t) {
        class.mixins.push(t);
    }

    if let Some(ctor_idx) = class
        .methods
        .iter()
        .position(|m| m.name.as_str() == "__construct")
    {
        let mut ctor = (*class.methods[ctor_idx]).clone();
        let binding = (t, atom("$iterator"));
        if !ctor.template_bindings.iter().any(|(n, _)| *n == t) {
            ctor.template_bindings.push(binding);
        }
        // A `T` hint (rather than the stub's `Traversable`) is what makes
        // `classify_template_binding` read the argument as a direct bind.
        if let Some(param) = ctor
            .parameters
            .make_mut()
            .iter_mut()
            .find(|p| p.name == "$iterator")
        {
            param.type_hint = Some(PhpType::named(t));
        }
        class.methods.make_mut()[ctor_idx] = std::sync::Arc::new(ctor);
    }
}

/// Add `@template TKey`, `@template TValue`,
/// `@template TIterator of Traversable<TKey, TValue>`,
/// `@extends IteratorIterator<TKey, TValue, TIterator>`.
///
/// `FilterIterator` is abstract and extends `IteratorIterator`.
/// PHPStan ref: `stubs/iterable.stub`
fn patch_filter_iterator(class: &mut ClassInfo) {
    if !class.template_params.is_empty() {
        return;
    }
    patch_iterator_iterator_subclass(class, "IteratorIterator");
    patch_method_return_type(class, "current", PhpType::named(atom("TValue")));
    patch_method_return_type(class, "key", PhpType::named(atom("TKey")));
}

/// Patch `NoRewindIterator` with template params inherited from `IteratorIterator`.
///
/// Without this patch, `new NoRewindIterator(generator())` resolves as
/// bare `NoRewindIterator` without propagating the generator's type params.
/// PHPStan ref: `stubs/iterable.stub`
fn patch_no_rewind_iterator(class: &mut ClassInfo) {
    if !class.template_params.is_empty() {
        return;
    }
    patch_iterator_iterator_subclass(class, "IteratorIterator");
    patch_constructor_iterator_binding(class);
}

/// Patch `CachingIterator` with template params inherited from `IteratorIterator`.
///
/// `CachingIterator` extends `IteratorIterator` and wraps an iterator.
/// PHPStan ref: `stubs/iterable.stub`
fn patch_caching_iterator(class: &mut ClassInfo) {
    if !class.template_params.is_empty() {
        return;
    }
    patch_iterator_iterator_subclass(class, "IteratorIterator");
    patch_method_return_type(class, "current", PhpType::named(atom("TValue")));
    patch_method_return_type(class, "key", PhpType::named(atom("TKey")));
    patch_constructor_iterator_binding(class);
}

/// Patch `InfiniteIterator` with template params inherited from `IteratorIterator`.
///
/// PHPStan ref: `stubs/iterable.stub`
fn patch_infinite_iterator(class: &mut ClassInfo) {
    if !class.template_params.is_empty() {
        return;
    }
    patch_iterator_iterator_subclass(class, "IteratorIterator");
    patch_constructor_iterator_binding(class);
}

/// Patch `LimitIterator` with template params inherited from `IteratorIterator`.
///
/// PHPStan ref: `stubs/iterable.stub`
fn patch_limit_iterator(class: &mut ClassInfo) {
    if !class.template_params.is_empty() {
        return;
    }
    patch_iterator_iterator_subclass(class, "IteratorIterator");
    patch_method_return_type(class, "current", PhpType::named(atom("TValue")));
    patch_method_return_type(class, "key", PhpType::named(atom("TKey")));
    patch_constructor_iterator_binding(class);
}

/// Patch `CallbackFilterIterator` with template params inherited from `FilterIterator`.
///
/// `CallbackFilterIterator` extends `FilterIterator` (not `IteratorIterator` directly).
/// PHPStan ref: `stubs/iterable.stub`
fn patch_callback_filter_iterator(class: &mut ClassInfo) {
    if !class.template_params.is_empty() {
        return;
    }
    patch_iterator_iterator_subclass(class, "FilterIterator");
    patch_constructor_iterator_binding(class);
}

/// Patch `ArrayIterator` constructor to bind template params from the `$array` arg.
///
/// phpstorm-stubs declare `@template TKey of array-key` and `@template TValue`
/// on the class, but the constructor's `@param` is just `object|array` with no
/// generics.  PHPStan's stubs use `@param array<TKey, TValue> $array`.
/// PHPStan ref: `stubs/iterable.stub`
fn patch_array_iterator(class: &mut ClassInfo) {
    if let Some(ctor_idx) = class
        .methods
        .iter()
        .position(|m| m.name.as_str() == "__construct")
    {
        let mut ctor = (*class.methods[ctor_idx]).clone();

        for tpl_name in ["TKey", "TValue"] {
            let binding = (atom(tpl_name), atom("$array"));
            if !ctor.template_bindings.iter().any(|(t, _)| t == &binding.0) {
                ctor.template_bindings.push(binding);
            }
        }

        // Set the parameter type hint to array<TKey, TValue> so that
        // classify_template_binding can determine the GenericWrapper mode.
        if let Some(param) = ctor
            .parameters
            .make_mut()
            .iter_mut()
            .find(|p| p.name == "$array")
        {
            param.type_hint = Some(PhpType::generic(
                "array",
                vec![PhpType::named(atom("TKey")), PhpType::named(atom("TValue"))],
            ));
        }

        class.methods.make_mut()[ctor_idx] = std::sync::Arc::new(ctor);
    }
}

/// Give `SimpleXMLElement::asXML()` / `saveXML()` a conditional return type
/// keyed on `$filename`.
///
/// Both are declared `string|bool`: without a filename they return the
/// document as a string (`false` on error), and with one they write the file
/// and report whether it worked. The flat union means neither result can be
/// split, so `assertNotFalse($xml->asXML())` still leaves a `bool` the caller
/// has to defend against.
fn patch_simple_xml_element(class: &mut ClassInfo) {
    let serialised = PhpType::union(vec![PhpType::string(), PhpType::named(atom("false"))]);
    for method_name in ["asXML", "saveXML"] {
        patch_method_conditional_return(
            class,
            method_name,
            "$filename",
            PhpType::conditional(
                "$filename",
                false,
                PhpType::null(),
                serialised.clone(),
                PhpType::bool(),
            ),
        );
    }
}

/// Fix two `ReflectionClass` return types phpstorm-stubs understate.
///
/// `newInstanceArgs()` is declared `@return T|null` (mirroring php-src's
/// vestigial `?object` hint), but the method has thrown a
/// `ReflectionException` instead of returning null since PHP 5.  The
/// null branch only makes the result differ from `newInstance()`'s `T`
/// for no reason, which forces callers to null-check a value that is
/// never null.  PHPStan's own stub types both as `T`.
///
/// `getInterfaceNames()` is declared bare `array`, so the names it hands back
/// — every one of them a class-string, since reflection only reports
/// interfaces that exist — arrive as plain strings and cannot be passed on to
/// anything that asks for a `class-string`. PHPStan's stub says
/// `list<class-string>`.
fn patch_reflection_class(class: &mut ClassInfo) {
    if let Some(idx) = class
        .methods
        .iter()
        .position(|m| m.name.as_str() == "getInterfaceNames")
    {
        let mut method = (*class.methods[idx]).clone();
        method.return_type = Some(PhpType::parse("list<class-string>"));
        class.methods.make_mut()[idx] = std::sync::Arc::new(method);
    }

    let Some(idx) = class
        .methods
        .iter()
        .position(|m| m.name.as_str() == "newInstanceArgs")
    else {
        return;
    };
    let non_null_return = class.methods[idx]
        .return_type
        .as_ref()
        .and_then(PhpType::non_null_type);
    let non_null_native = class.methods[idx]
        .native_return_type
        .as_ref()
        .and_then(PhpType::non_null_type);
    if non_null_return.is_none() && non_null_native.is_none() {
        return;
    }
    let mut method = (*class.methods[idx]).clone();
    if non_null_return.is_some() {
        method.return_type = non_null_return;
    }
    if non_null_native.is_some() {
        method.native_return_type = non_null_native;
    }
    class.methods.make_mut()[idx] = std::sync::Arc::new(method);
}

/// Carry `ReflectionObject`'s reflected class the way `ReflectionClass`
/// already carries it.
///
/// phpstorm-stubs annotate `ReflectionClass` with `@template T of object`
/// and bind `T` from the constructor's `class-string<T>|T` parameter, but
/// `ReflectionObject` -- the same class narrowed to an instance -- declares
/// neither the template nor the `@extends`, so `new ReflectionObject($x)`
/// forgets what it reflects and `newInstance()` widens back to `object`.
/// PHPStan's stubs carry `@template-extends ReflectionClass<T>` here.
fn patch_reflection_object(class: &mut ClassInfo) {
    if !class.template_params.is_empty() {
        return;
    }
    add_templates(class, &[("T", Some("object"))]);
    let parent = atom("ReflectionClass");
    if !class.extends_generics.iter().any(|(n, _)| *n == parent) {
        class
            .extends_generics
            .push((parent, vec![PhpType::named(atom("T"))]));
    }

    let Some(ctor_idx) = class
        .methods
        .iter()
        .position(|m| m.name.as_str() == "__construct")
    else {
        return;
    };
    let mut ctor = (*class.methods[ctor_idx]).clone();
    let binding = (atom("T"), atom("$object"));
    if !ctor.template_bindings.iter().any(|(t, _)| t == &binding.0) {
        ctor.template_bindings.push(binding);
    }
    if let Some(param) = ctor
        .parameters
        .make_mut()
        .iter_mut()
        .find(|p| p.name == "$object")
    {
        param.type_hint = Some(PhpType::named(atom("T")));
    }
    class.methods.make_mut()[ctor_idx] = std::sync::Arc::new(ctor);
}

/// Give a method a conditional return type, provided the stub declares the
/// parameter the conditional keys on.
fn patch_method_conditional_return(
    class: &mut ClassInfo,
    method_name: &str,
    param_name: &str,
    conditional: PhpType,
) {
    let Some(idx) = class
        .methods
        .iter()
        .position(|m| m.name.as_str() == method_name)
    else {
        return;
    };
    if !class.methods[idx]
        .parameters
        .iter()
        .any(|p| p.name == param_name)
    {
        return;
    }
    let mut method = (*class.methods[idx]).clone();
    method.conditional_return = Some(conditional);
    class.methods.make_mut()[idx] = std::sync::Arc::new(method);
}

/// Shared helper: add `@template TKey, TValue, TIterator` and
/// `@extends <parent><TKey, TValue, TIterator>` to an `IteratorIterator`
/// subclass (or sub-subclass like `CallbackFilterIterator`).
fn patch_iterator_iterator_subclass(class: &mut ClassInfo, parent: &str) {
    add_templates(class, &[("TKey", None), ("TValue", None)]);
    let t_iter = atom("TIterator");
    if !class.template_params.contains(&t_iter) {
        class.template_params.push(t_iter);
    }
    class
        .template_param_bounds
        .entry(atom("TIterator"))
        .or_insert_with(|| {
            PhpType::generic(
                "Traversable",
                vec![PhpType::named(atom("TKey")), PhpType::named(atom("TValue"))],
            )
        });
    let parent_atom = atom(parent);
    if !class
        .extends_generics
        .iter()
        .any(|(n, _)| *n == parent_atom)
    {
        class.extends_generics.push((
            parent_atom,
            vec![
                PhpType::named(atom("TKey")),
                PhpType::named(atom("TValue")),
                PhpType::named(atom("TIterator")),
            ],
        ));
    }
}

/// Shared helper: patch the constructor to bind `TIterator` from the
/// `$iterator` parameter.
fn patch_constructor_iterator_binding(class: &mut ClassInfo) {
    if let Some(ctor_idx) = class
        .methods
        .iter()
        .position(|m| m.name.as_str() == "__construct")
    {
        let mut ctor = (*class.methods[ctor_idx]).clone();
        let binding = (atom("TIterator"), atom("$iterator"));
        if !ctor.template_bindings.iter().any(|(t, _)| t == &binding.0) {
            ctor.template_bindings.push(binding);
        }
        if let Some(param) = ctor
            .parameters
            .make_mut()
            .iter_mut()
            .find(|p| p.name == "$iterator")
        {
            param.type_hint = Some(PhpType::named(atom("TIterator")));
        }
        class.methods.make_mut()[ctor_idx] = std::sync::Arc::new(ctor);
    }
}

/// Override a method's return type on a class.
///
/// If the method exists, replaces its `return_type` with the given type.
/// Used to patch stub methods like `current(): mixed` → `current(): TValue`.
fn patch_method_return_type(class: &mut ClassInfo, method_name: &str, return_type: PhpType) {
    if let Some(idx) = class
        .methods
        .iter()
        .position(|m| m.name.as_str() == method_name)
    {
        let mut method = (*class.methods[idx]).clone();
        method.return_type = Some(return_type);
        class.methods.make_mut()[idx] = std::sync::Arc::new(method);
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Add template parameters with optional upper bounds.
///
/// Each entry is `(param_name, optional_bound)`.  The bound, if present,
/// is parsed into a `PhpType` and stored in `template_param_bounds`.
fn add_templates(class: &mut ClassInfo, templates: &[(&str, Option<&str>)]) {
    for &(name, bound) in templates {
        let param = atom(name);
        if !class.template_params.contains(&param) {
            class.template_params.push(param);
        }
        if let Some(bound_str) = bound {
            class
                .template_param_bounds
                .entry(atom(name))
                .or_insert_with(|| PhpType::parse(bound_str));
        }
    }
}

/// Add an `@implements InterfaceName<Param1, Param2, ...>` entry where
/// all type arguments are template parameter names (the common case).
fn add_implements_generics(class: &mut ClassInfo, iface_name: &str, params: &[&str]) {
    let args: Vec<PhpType> = params.iter().map(|p| PhpType::named(atom(p))).collect();
    add_implements_generics_typed(class, iface_name, &args);
}

/// Add an `@implements InterfaceName<Type1, Type2, ...>` entry with
/// pre-built `PhpType` arguments.
fn add_implements_generics_typed(class: &mut ClassInfo, iface_name: &str, args: &[PhpType]) {
    if class
        .implements_generics
        .iter()
        .any(|(n, _)| n.as_str() == iface_name)
    {
        return;
    }
    class
        .implements_generics
        .push((atom(iface_name), args.to_vec()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;
    use crate::php_type::PhpType;

    fn empty_class(name: &str) -> ClassInfo {
        ClassInfo {
            name: atom(name),
            ..ClassInfo::default()
        }
    }

    fn empty_function(name: &str) -> FunctionInfo {
        FunctionInfo {
            name: atom(name),
            name_offset: 0,
            parameters: Vec::new().into(),
            return_type: None,
            native_return_type: None,
            description: None,
            return_description: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            namespace: None,
            conditional_return: None,
            type_assertions: Vec::new(),
            deprecation_message: None,
            deprecated_replacement: None,
            throws: Vec::new(),
            template_params: Vec::new(),
            template_param_bounds: Default::default(),
            template_bindings: Vec::new(),
            is_polyfill: false,
            overloads: Vec::new(),
            is_pure: false,
        }
    }

    #[test]
    fn weak_map_gets_array_access_generics() {
        let mut class = empty_class("WeakMap");
        apply_class_stub_patches(&mut class);

        assert!(
            class
                .implements_generics
                .iter()
                .any(|(n, args)| n.as_str() == "ArrayAccess"
                    && args.len() == 2
                    && args[0] == PhpType::named(atom("TKey"))
                    && args[1] == PhpType::named(atom("TValue"))),
            "Should have @implements ArrayAccess<TKey, TValue>"
        );
    }

    #[test]
    fn unrelated_class_not_patched() {
        let mut class = empty_class("MyApp\\Foo");
        let original_params = class.template_params.clone();

        apply_class_stub_patches(&mut class);

        assert_eq!(class.template_params, original_params);
        assert!(class.implements_generics.is_empty());
    }

    #[test]
    fn iterator_iterator_gets_templates_and_mixin() {
        let mut class = empty_class("IteratorIterator");
        apply_class_stub_patches(&mut class);

        assert_eq!(
            class.template_params,
            vec![atom("TKey"), atom("TValue"), atom("TIterator")]
        );
        assert!(
            class
                .implements_generics
                .iter()
                .any(|(n, args)| n.as_str() == "OuterIterator" && args.len() == 2),
            "Should have @implements OuterIterator<TKey, TValue>"
        );
        assert_eq!(class.mixins, vec![atom("TIterator")]);
        assert!(
            class.template_param_bounds.contains_key(&atom("TIterator")),
            "TIterator should have a bound"
        );
    }

    fn param(name: &str, type_hint: &str) -> crate::types::ParameterInfo {
        crate::types::ParameterInfo {
            name: atom(name),
            is_required: true,
            type_hint: Some(PhpType::parse(type_hint)),
            native_type_hint: Some(PhpType::parse(type_hint)),
            description: None,
            default_value: None,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        }
    }

    #[test]
    fn array_map_links_callback_to_array_element() {
        let mut func = empty_function("array_map");
        func.parameters = vec![param("$callback", "callable"), param("$array", "array")].into();
        func.return_type = Some(PhpType::parse("array"));

        apply_function_stub_patches(&mut func);

        assert_eq!(func.template_params, vec![atom("TValue")]);
        assert_eq!(
            func.template_bindings,
            vec![(atom("TValue"), atom("$array"))]
        );
        // The callback's first parameter is now `TValue`.
        let callback = &func.parameters[0];
        assert_eq!(
            callback.type_hint,
            Some(PhpType::parse("callable(TValue): mixed"))
        );
        // The array is `array<TValue>`.
        assert_eq!(
            func.parameters[1].type_hint,
            Some(PhpType::parse("array<TValue>"))
        );
        // The return type is left bare so the value-inspecting element
        // logic in raw_type_inference.rs stays authoritative.
        assert_eq!(func.return_type, Some(PhpType::parse("array")));
    }

    #[test]
    fn array_filter_links_callback_to_array_element() {
        let mut func = empty_function("array_filter");
        func.parameters = vec![param("$array", "array"), param("$callback", "callable")].into();

        apply_function_stub_patches(&mut func);

        assert_eq!(func.template_params, vec![atom("TValue")]);
        assert_eq!(
            func.template_bindings,
            vec![(atom("TValue"), atom("$array"))]
        );
        assert_eq!(
            func.parameters[0].type_hint,
            Some(PhpType::parse("array<TValue>"))
        );
        assert_eq!(
            func.parameters[1].type_hint,
            Some(PhpType::parse("callable(TValue): mixed"))
        );
    }

    #[test]
    fn array_map_unexpected_shape_not_patched() {
        // A hand-written `@method array_map(...)` or a differently-shaped
        // stub must not be rewritten.
        let mut func = empty_function("array_map");
        func.parameters = vec![param("$other", "array")].into();

        apply_function_stub_patches(&mut func);

        assert!(func.template_params.is_empty());
        assert!(func.template_bindings.is_empty());
    }

    #[test]
    fn range_gets_conditional_return() {
        let mut func = empty_function("range");
        apply_function_stub_patches(&mut func);
        assert!(
            func.conditional_return.is_some(),
            "range() should have a conditional return type after patching"
        );
    }

    #[test]
    fn str_word_count_gets_conditional_return() {
        let mut func = empty_function("str_word_count");
        apply_function_stub_patches(&mut func);
        let cond = func
            .conditional_return
            .expect("str_word_count() should have a conditional return type after patching");
        assert_eq!(
            cond.to_string(),
            "$format is 0 ? int : $format is 1 ? list<string> : \
             $format is 2 ? array<int, string> : list<string>|int"
        );
    }

    #[test]
    fn stream_bucket_make_writeable_pre_84_becomes_stdclass() {
        let mut func = empty_function("stream_bucket_make_writeable");
        func.return_type = Some(PhpType::parse("object|null"));
        func.native_return_type = Some(PhpType::parse("object|null"));

        apply_function_stub_patches(&mut func);

        assert_eq!(func.return_type, Some(PhpType::parse("stdClass|null")));
        assert_eq!(
            func.native_return_type,
            Some(PhpType::parse("stdClass|null"))
        );
    }

    #[test]
    fn stream_bucket_make_writeable_84_plus_unchanged() {
        let mut func = empty_function("stream_bucket_make_writeable");
        func.return_type = Some(PhpType::parse("StreamBucket|null"));
        func.native_return_type = Some(PhpType::parse("StreamBucket|null"));

        apply_function_stub_patches(&mut func);

        assert_eq!(func.return_type, Some(PhpType::parse("StreamBucket|null")));
        assert_eq!(
            func.native_return_type,
            Some(PhpType::parse("StreamBucket|null"))
        );
    }

    #[test]
    fn ctype_family_and_define_accept_mixed() {
        for name in ["ctype_digit", "ctype_alpha", "ctype_xdigit"] {
            let mut func = empty_function(name);
            func.parameters = vec![param("$text", "string")].into();

            apply_function_stub_patches(&mut func);

            assert_eq!(
                func.parameters[0].type_hint,
                Some(PhpType::mixed()),
                "{name} should accept mixed"
            );
            assert_eq!(func.parameters[0].native_type_hint, Some(PhpType::mixed()));
        }

        let mut define = empty_function("define");
        define.parameters = vec![
            param("$constant_name", "string"),
            param("$value", "null|array|bool|int|float|string"),
        ]
        .into();

        apply_function_stub_patches(&mut define);

        assert_eq!(
            define.parameters[0].type_hint,
            Some(PhpType::string()),
            "the constant name is still a string"
        );
        assert_eq!(define.parameters[1].type_hint, Some(PhpType::mixed()));
    }

    /// Each conditional is keyed on the parameter that really decides the
    /// return type, so a call that omits the argument is answered by that
    /// parameter's declared default rather than by argument position.
    #[test]
    fn argument_dependent_builtins_get_conditional_returns() {
        /// A stub's name, its `(parameter, type)` list, and the conditional
        /// return type the patch is expected to give it.
        type PatchCase = (
            &'static str,
            &'static [(&'static str, &'static str)],
            &'static str,
        );

        let cases: &[PatchCase] = &[
            (
                "pathinfo",
                &[("$path", "string"), ("$flags", "int")],
                "$flags is 15 ? array{dirname: string, basename: string, \
                 extension?: string, filename: string} : string",
            ),
            (
                "print_r",
                &[("$value", "mixed"), ("$return", "bool")],
                "$return is true ? string : true",
            ),
            (
                "hrtime",
                &[("$as_number", "bool")],
                "$as_number is true ? int|float : array{int, int}|false",
            ),
            (
                "microtime",
                &[("$as_float", "bool")],
                "$as_float is true ? float : string",
            ),
            (
                "getenv",
                &[("$name", "string"), ("$local_only", "bool")],
                "$name is null ? array<string, string> : string|false",
            ),
            (
                "mb_convert_encoding",
                &[("$string", "array|string"), ("$to_encoding", "string")],
                "$string is array ? array<array-key, string> : string|false",
            ),
            ("abs", &[("$num", "int|float")], "$num is int ? int : float"),
        ];

        for (name, params, expected) in cases {
            let mut func = empty_function(name);
            func.parameters = params
                .iter()
                .map(|(n, t)| param(n, t))
                .collect::<Vec<_>>()
                .into();

            apply_function_stub_patches(&mut func);

            let cond = func
                .conditional_return
                .unwrap_or_else(|| panic!("{name} should have a conditional return type"));
            assert_eq!(cond.to_string(), *expected, "{name}");
        }
    }

    /// The pre-7.1 `$varname` spelling is what a call binds to on an older
    /// configured PHP version, so the conditional follows the first parameter
    /// rather than a hard-coded name.
    #[test]
    fn getenv_keys_on_whichever_name_parameter_the_stub_declares() {
        let mut func = empty_function("getenv");
        func.parameters = vec![param("$varname", "string"), param("$local_only", "bool")].into();

        apply_function_stub_patches(&mut func);

        assert_eq!(
            func.conditional_return.map(|c| c.to_string()).as_deref(),
            Some("$varname is null ? array<string, string> : string|false")
        );
    }

    /// A stub that does not declare the parameter the conditional keys on is
    /// left alone: deciding the branch against whichever argument landed in
    /// slot 0 is worse than the declared union.
    #[test]
    fn a_differently_shaped_stub_keeps_its_declared_return() {
        let mut func = empty_function("pathinfo");
        func.parameters = vec![param("$path", "string")].into();

        apply_function_stub_patches(&mut func);

        assert!(func.conditional_return.is_none());
    }

    #[test]
    fn simple_xml_serialisers_get_conditional_returns() {
        let mut class = empty_class("SimpleXMLElement");
        for name in ["asXML", "saveXML"] {
            let mut method = crate::types::MethodInfo::virtual_method(name, Some("string|bool"));
            method.parameters = vec![param("$filename", "string|null")].into();
            class.methods.make_mut().push(std::sync::Arc::new(method));
        }

        apply_class_stub_patches(&mut class);

        for method in class.methods.iter() {
            assert_eq!(
                method.conditional_return.as_ref().map(|c| c.to_string()),
                Some("$filename is null ? string|false : bool".to_string()),
                "{}",
                method.name
            );
        }
    }

    #[test]
    fn reflection_class_new_instance_args_loses_its_null_branch() {
        let mut class = empty_class("ReflectionClass");
        let mut method =
            crate::types::MethodInfo::virtual_method("newInstanceArgs", Some("T|null"));
        method.native_return_type = Some(PhpType::parse("?object"));
        class.methods.make_mut().push(std::sync::Arc::new(method));

        apply_class_stub_patches(&mut class);

        assert_eq!(class.methods[0].return_type, Some(PhpType::parse("T")));
        assert_eq!(
            class.methods[0].native_return_type,
            Some(PhpType::parse("object"))
        );
    }

    #[test]
    fn reflection_class_leaves_an_already_non_null_return_alone() {
        let mut class = empty_class("ReflectionClass");
        let method = crate::types::MethodInfo::virtual_method("newInstanceArgs", Some("T"));
        class.methods.make_mut().push(std::sync::Arc::new(method));

        apply_class_stub_patches(&mut class);

        assert_eq!(class.methods[0].return_type, Some(PhpType::parse("T")));
    }

    #[test]
    fn spl_autoload_register_types_its_callback_parameter() {
        let mut func = empty_function("spl_autoload_register");
        func.parameters = vec![param("$callback", "?callable"), param("$throw", "bool")].into();

        apply_function_stub_patches(&mut func);

        assert_eq!(
            func.parameters[0].type_hint,
            Some(PhpType::parse("?callable(string): void"))
        );
        assert_eq!(func.parameters[1].type_hint, Some(PhpType::parse("bool")));
    }

    #[test]
    fn spl_autoload_register_unexpected_shape_not_patched() {
        let mut func = empty_function("spl_autoload_register");
        func.parameters = vec![param("$other", "?callable")].into();

        apply_function_stub_patches(&mut func);

        assert_eq!(
            func.parameters[0].type_hint,
            Some(PhpType::parse("?callable"))
        );
    }

    #[test]
    fn the_user_sort_family_types_its_comparison_callback() {
        for (name, compared) in [
            ("usort", "TValue"),
            ("uasort", "TValue"),
            ("uksort", "TKey"),
        ] {
            let mut func = empty_function(name);
            func.parameters = vec![param("$array", "array"), param("$callback", "callable")].into();

            apply_function_stub_patches(&mut func);

            assert_eq!(
                func.parameters[0].type_hint,
                Some(PhpType::parse("array<TKey, TValue>")),
                "{name} array parameter"
            );
            assert_eq!(
                func.parameters[1].type_hint,
                Some(PhpType::parse(&format!(
                    "callable({compared}, {compared}): int"
                ))),
                "{name} callback parameter"
            );
            assert_eq!(
                func.template_bindings,
                vec![
                    (atom("TKey"), atom("$array")),
                    (atom("TValue"), atom("$array")),
                ],
                "{name} bindings"
            );
        }
    }

    #[test]
    fn a_differently_shaped_sort_stub_is_not_patched() {
        let mut func = empty_function("usort");
        func.parameters = vec![param("$callback", "callable"), param("$array", "array")].into();

        apply_function_stub_patches(&mut func);

        assert!(func.template_params.is_empty());
        assert_eq!(
            func.parameters[0].type_hint,
            Some(PhpType::parse("callable"))
        );
    }

    #[test]
    fn an_unrelated_function_keeps_its_parameter_types() {
        let mut func = empty_function("str_pad");
        func.parameters = vec![param("$string", "string")].into();

        apply_function_stub_patches(&mut func);

        assert_eq!(func.parameters[0].type_hint, Some(PhpType::string()));
    }
}
