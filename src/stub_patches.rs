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
//! 2. **`stream_bucket_make_writeable`** -- phpstorm-stubs type the
//!    return as `object|null` below PHP 8.4 (the `StreamBucket` class
//!    only exists from 8.4 onward). Bare `object` in a union is not
//!    recognised as the universal-container case, so property access
//!    on the result is unverifiable. We override the pre-8.4 case to
//!    `stdClass|null`, matching PHPStan's function map.
//!
//! 3. **`array_map`** / **`array_filter`** -- phpstorm-stubs type the
//!    callback as bare `callable` and the array as bare `array`, so a
//!    closure passed to them (`array_map(fn($x) => …, $items)`) leaves
//!    its parameter untyped. We add `@template TValue`, retype the
//!    callback's first parameter as `TValue`, the array as
//!    `array<TValue>`, and bind `TValue` from the array argument. Only
//!    the callback's *input* type is patched here; the callback's
//!    return type (and thus the function's own return) stays in the
//!    value-inspecting logic in `raw_type_inference.rs`.
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
//! ## Removing patches
//!
//! When phpstorm-stubs gains proper annotations for a patched symbol,
//! delete the corresponding patch function here and remove its dispatch
//! from the entry point.  Run the test suite to verify that the stub's
//! own annotations produce the same result.

use crate::atom::atom;
use crate::php_type::PhpType;
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
        "stream_bucket_make_writeable" => patch_stream_bucket_make_writeable(func),
        "array_map" => patch_array_map(func),
        "array_filter" => patch_array_filter(func),
        _ => {}
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
/// type (the callback receives each element and returns a boolean).
///
/// Unlike `array_map`, `array_filter` takes the array first and the
/// callback second: `array_filter(array $array, ?callable $callback, …)`.
fn patch_array_filter(func: &mut FunctionInfo) {
    let array_name = match func.parameters.first() {
        Some(p) if p.name.as_str() == "$array" => p.name,
        _ => return,
    };
    let callback_name = match func.parameters.get(1) {
        Some(p) if p.name.as_str() == "$callback" => p.name,
        _ => return,
    };
    link_callback_to_array_element(func, callback_name, array_name, "bool");
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

    for param in &mut func.parameters {
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

/// Patch `range()` to have a conditional return type.
///
/// phpstorm-stubs declare `range()` as returning bare `array`.
/// PHPStan infers `list<int>`, `list<float>`, or `list<string>` depending
/// on the argument types.  We approximate this with:
/// `($start is string ? list<string> : list<int|float>)`.
fn patch_range(func: &mut FunctionInfo) {
    func.conditional_return = Some(PhpType::Conditional {
        param: "$start".to_string(),
        negated: false,
        condition: Box::new(PhpType::Named("string".to_string())),
        then_type: Box::new(PhpType::list(PhpType::string())),
        else_type: Box::new(PhpType::list(PhpType::Union(vec![
            PhpType::int(),
            PhpType::float(),
        ]))),
    });
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
    match ty {
        PhpType::Named(name) => name.eq_ignore_ascii_case("object"),
        PhpType::Nullable(inner) => is_pre_84_object_type(inner),
        PhpType::Union(members) => members.iter().any(is_pre_84_object_type),
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
        "FilterIterator" => patch_filter_iterator(class),
        "NoRewindIterator" => patch_no_rewind_iterator(class),
        "CachingIterator" => patch_caching_iterator(class),
        "InfiniteIterator" => patch_infinite_iterator(class),
        "LimitIterator" => patch_limit_iterator(class),
        "CallbackFilterIterator" => patch_callback_filter_iterator(class),
        "ArrayIterator" => patch_array_iterator(class),
        _ => {}
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
            PhpType::Generic(
                "Traversable".to_string(),
                vec![
                    PhpType::Named("TKey".to_string()),
                    PhpType::Named("TValue".to_string()),
                ],
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
    patch_method_return_type(class, "current", PhpType::Named("TValue".to_string()));
    patch_method_return_type(class, "key", PhpType::Named("TKey".to_string()));

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
        if let Some(param) = ctor.parameters.iter_mut().find(|p| p.name == "$iterator") {
            param.type_hint = Some(PhpType::Named("TIterator".to_string()));
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
    patch_method_return_type(class, "current", PhpType::Named("TValue".to_string()));
    patch_method_return_type(class, "key", PhpType::Named("TKey".to_string()));
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
    patch_method_return_type(class, "current", PhpType::Named("TValue".to_string()));
    patch_method_return_type(class, "key", PhpType::Named("TKey".to_string()));
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
    patch_method_return_type(class, "current", PhpType::Named("TValue".to_string()));
    patch_method_return_type(class, "key", PhpType::Named("TKey".to_string()));
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

        // Add template bindings: TKey → $array, TValue → $array
        for tpl_name in ["TKey", "TValue"] {
            let binding = (atom(tpl_name), atom("$array"));
            if !ctor.template_bindings.iter().any(|(t, _)| t == &binding.0) {
                ctor.template_bindings.push(binding);
            }
        }

        // Set the parameter type hint to array<TKey, TValue> so that
        // classify_template_binding can determine the GenericWrapper mode.
        if let Some(param) = ctor.parameters.iter_mut().find(|p| p.name == "$array") {
            param.type_hint = Some(PhpType::Generic(
                "array".to_string(),
                vec![
                    PhpType::Named("TKey".to_string()),
                    PhpType::Named("TValue".to_string()),
                ],
            ));
        }

        class.methods.make_mut()[ctor_idx] = std::sync::Arc::new(ctor);
    }
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
            PhpType::Generic(
                "Traversable".to_string(),
                vec![
                    PhpType::Named("TKey".to_string()),
                    PhpType::Named("TValue".to_string()),
                ],
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
                PhpType::Named("TKey".to_string()),
                PhpType::Named("TValue".to_string()),
                PhpType::Named("TIterator".to_string()),
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
        if let Some(param) = ctor.parameters.iter_mut().find(|p| p.name == "$iterator") {
            param.type_hint = Some(PhpType::Named("TIterator".to_string()));
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
    let args: Vec<PhpType> = params
        .iter()
        .map(|p| PhpType::Named((*p).to_string()))
        .collect();
    add_implements_generics_typed(class, iface_name, &args);
}

/// Add an `@implements InterfaceName<Type1, Type2, ...>` entry with
/// pre-built `PhpType` arguments.
fn add_implements_generics_typed(class: &mut ClassInfo, iface_name: &str, args: &[PhpType]) {
    // Don't add duplicate entries.
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
            parameters: Vec::new(),
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
                    && args[0] == PhpType::Named("TKey".to_string())
                    && args[1] == PhpType::Named("TValue".to_string())),
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
        func.parameters = vec![param("$callback", "callable"), param("$array", "array")];
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
        func.parameters = vec![param("$array", "array"), param("$callback", "callable")];

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
            Some(PhpType::parse("callable(TValue): bool"))
        );
    }

    #[test]
    fn array_map_unexpected_shape_not_patched() {
        // A hand-written `@method array_map(...)` or a differently-shaped
        // stub must not be rewritten.
        let mut func = empty_function("array_map");
        func.parameters = vec![param("$other", "array")];

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
}
