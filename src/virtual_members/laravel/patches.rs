//! Centralized Laravel class patch system.
//!
//! After virtual members are applied during [`resolve_class_fully_inner`],
//! certain Laravel classes need post-resolution fixups that cannot be
//! expressed as virtual member providers (which add new members) but
//! instead modify existing members' type information.
//!
//! This module provides a single entry point, [`apply_laravel_patches`],
//! that dispatches to per-class patch functions based on the fully-qualified
//! class name.  All Laravel-specific class mutations live here, making it
//! easy to audit and extend the patch inventory.
//!
//! ## Patch inventory
//!
//! 1. **`Eloquent\Builder::__call` / `__callStatic` return type.**
//!    Overrides the `mixed` return type to `static` so that method chains
//!    through unknown calls (scope dispatch, macro dispatch, Query\Builder
//!    forwarding) preserve the Builder type.
//!
//! 2. **`Conditionable::when()` / `unless()` return type.**
//!    The trait declares `@return $this|TWhenReturnType` (or a conditional
//!    form in Larastan stubs).  The unresolved `TWhenReturnType` template
//!    parameter breaks `is_self_like_type` checks, degrading Builder chains.
//!    The patch replaces the return type with `$this` so that chained
//!    `when()` / `unless()` calls preserve the receiver type.
//!
//! 3. **Bare `Builder` return types on scope methods** are handled
//!    separately in `scopes.rs` (`is_bare_builder_type`) because that
//!    patch runs at scope-injection time (post-generic-substitution),
//!    not during `resolve_class_fully_inner`.  It is documented here
//!    as part of the patch inventory but not dispatched from this module.
//!
//! 4. **`Redis\Connections\Connection` mixin.**
//!    The base `Connection` class delegates all Redis commands to the
//!    underlying `\Redis` client via `__call`, but lacks a `@mixin`
//!    annotation.  The patch injects `@mixin \Redis` **pre-resolution**
//!    (in `resolve_class_fully_inner`, before virtual member providers
//!    run) so that `collect_mixin_members` picks it up and merges
//!    `del()`, `get()`, `set()`, etc. from the stubs.  This patch is
//!    not dispatched from `apply_laravel_patches` because that runs
//!    post-resolution, after mixin collection has already completed.
//!
//! 5. **`DB` facade / `Connection` select method return types.**
//!    The facade's `@method` annotations and the underlying
//!    `Connection` class both declare `select()`,
//!    `selectFromWriteConnection()`, and `selectResultSets()` as
//!    returning bare `array`.  The actual return type is
//!    `array<int, stdClass>`.  Similarly, `selectOne()` is declared as
//!    `mixed` but actually returns `stdClass|null`.  The patch
//!    overrides these return types so that downstream property access
//!    on query results resolves correctly.
//!
//! 6. **Eloquent Builder paginator element types.**
//!    `paginate()`, `simplePaginate()`, and `cursorPaginate()` declare
//!    an unparameterised paginator return type, so iterating the result
//!    yields no element type.  The patch parameterises them with
//!    `<int, TModel>` so `foreach (Model::paginate() as $row)` resolves
//!    `$row` to the concrete model.
//!
//! 7. **`Storage::fake()` / `persistentFake()` return types.**
//!    Both declare the `Filesystem` contract but always build a
//!    concrete `FilesystemAdapter`.  The patch corrects the return type
//!    so that adapter-only assertion helpers (`assertExists()`,
//!    `assertMissing()`, …) resolve on the faked disk.

use std::sync::Arc;

use crate::php_type::PhpType;
use crate::types::ClassInfo;

use super::ELOQUENT_BUILDER_FQN;

/// FQN of the `Conditionable` trait from `illuminate/support`.
const CONDITIONABLE_FQN: &str = "Illuminate\\Support\\Traits\\Conditionable";

/// FQN of the `DB` facade from `illuminate/support`.
const DB_FACADE_FQN: &str = "Illuminate\\Support\\Facades\\DB";

/// FQN of the `Cache` facade from `illuminate/support`.
const CACHE_FACADE_FQN: &str = "Illuminate\\Support\\Facades\\Cache";

/// FQN of the `Storage` facade from `illuminate/support`.
const STORAGE_FACADE_FQN: &str = "Illuminate\\Support\\Facades\\Storage";

/// FQN of the base `Connection` class from `illuminate/database`.
const DB_CONNECTION_FQN: &str = "Illuminate\\Database\\Connection";

/// FQN of the `Filesystem` contract that `Storage::fake()` declares but
/// never actually returns.
const FILESYSTEM_CONTRACT_FQN: &str = "Illuminate\\Contracts\\Filesystem\\Filesystem";

/// FQN of the concrete `FilesystemAdapter` that `Storage::fake()` and
/// `Storage::persistentFake()` always construct at runtime.
const FILESYSTEM_ADAPTER_FQN: &str = "Illuminate\\Filesystem\\FilesystemAdapter";

/// Apply all registered Laravel class patches to a fully-resolved class.
///
/// Called from [`resolve_class_fully_inner`] after virtual members have
/// been merged and before the result is cached.  Dispatches to per-class
/// patch functions based on `fqn`.
///
/// This is also applied transitively: when a class uses the
/// `Conditionable` trait, the trait's `when()` / `unless()` methods are
/// merged into the class.  The patch scans the merged method list by
/// name, so it fixes the return type regardless of whether the method
/// was inherited from the trait directly or through a parent class.
pub fn apply_laravel_patches(class: &mut ClassInfo, fqn: &str) {
    if fqn == ELOQUENT_BUILDER_FQN {
        patch_eloquent_builder_call_return_type(class);
        // Builder uses Conditionable, so patch when/unless too.
        patch_conditionable_when_unless(class);
        patch_eloquent_builder_paginate_element_type(class);
    } else if fqn == CONDITIONABLE_FQN || class_uses_conditionable(class) {
        patch_conditionable_when_unless(class);
    }

    if fqn == DB_FACADE_FQN || fqn == DB_CONNECTION_FQN {
        patch_db_select_return_types(class);
    }

    if fqn == CACHE_FACADE_FQN {
        patch_cache_facade_generics(class);
    }

    if fqn == STORAGE_FACADE_FQN {
        patch_storage_fake_return_types(class);
    }
}

/// Override `__call` and `__callStatic` return types on Eloquent Builder
/// from `mixed` to `static`.
///
/// Builder's `__call` dispatches to scope methods (`callNamedScope`),
/// macros, and `Query\Builder` forwarding — all of which return `$this`.
/// The `@return mixed` docblock is a PHP limitation; the actual return
/// type is always the Builder instance.  Patching this here means every
/// consumer of the resolved Builder (completion, diagnostics, hover)
/// automatically gets correct chain continuation through unknown methods.
fn patch_eloquent_builder_call_return_type(class: &mut ClassInfo) {
    let static_type = PhpType::static_();
    for method in class.methods.make_mut().iter_mut() {
        let method = Arc::make_mut(method);
        if (method.name == "__call" || method.name == "__callStatic")
            && method.return_type.as_ref().is_some_and(|rt| rt.is_mixed())
        {
            method.return_type = Some(static_type.clone());
        }
    }
}

/// Patch `when()` and `unless()` return types to `$this`.
///
/// The `Conditionable` trait declares these methods with return types
/// like `$this|TWhenReturnType` or the Larastan conditional form
/// `(TWhenReturnType is void|null ? $this : TWhenReturnType)`.  In
/// either case the unresolved method-level template parameter
/// `TWhenReturnType` / `TUnlessReturnType` prevents `is_self_like_type`
/// from recognizing the return as self-referential, which breaks method
/// chain resolution on Builder and Collection.
///
/// Since we cannot currently bind method-level templates during chain
/// resolution, the pragmatic fix is to treat these methods as returning
/// `$this` unconditionally.  This matches the common case (the callback
/// returns void and the method returns the receiver) and preserves
/// chain continuation.
fn patch_conditionable_when_unless(class: &mut ClassInfo) {
    let this_type = PhpType::this();
    for method in class.methods.make_mut().iter_mut() {
        let method = Arc::make_mut(method);
        if method.name != "when" && method.name != "unless" {
            continue;
        }
        let dominated_by_template = match &method.return_type {
            Some(rt) => return_type_has_unresolved_template(rt),
            None => false,
        };
        if dominated_by_template {
            method.return_type = Some(this_type.clone());
        }
    }
}

/// Check whether a return type contains an unresolved template parameter
/// that would prevent `is_self_like_type` from matching.
///
/// Recognizes patterns like:
/// - `$this|TWhenReturnType` (union with an unknown non-self member)
/// - `TWhenReturnType` (bare template parameter)
/// - `static|TWhenReturnType` (union mixing self-like and template)
///
/// A type name is considered a template parameter if it starts with an
/// uppercase `T` followed by another uppercase letter, or if it is not
/// a known keyword / built-in type and is not fully-qualified (no `\`).
fn return_type_has_unresolved_template(ty: &PhpType) -> bool {
    match ty {
        PhpType::Union(members) => members.iter().any(is_likely_template_param),
        other => is_likely_template_param(other),
    }
}

/// Heuristic: does this type look like an unresolved template parameter?
///
/// Template parameters in PHPDoc are typically `TFoo` (uppercase T + more).
/// We also catch any single bare name that is not a PHP keyword, not
/// fully-qualified, and not a self-reference.
fn is_likely_template_param(ty: &PhpType) -> bool {
    let name = match ty {
        PhpType::Named(n) => n.as_str(),
        _ => return false,
    };

    // PHP built-in / keyword types (includes self, static, $this, parent).
    if crate::php_type::is_keyword_type(name) {
        return false;
    }

    // FQN references (contain `\`) are concrete classes, not template params.
    if name.contains('\\') {
        return false;
    }

    // Common Conditionable template param names.
    if name == "TWhenReturnType" || name == "TUnlessReturnType" {
        return true;
    }

    // General heuristic: starts with T followed by an uppercase letter.
    if name.len() >= 2 {
        let mut chars = name.chars();
        if let (Some('T'), Some(c)) = (chars.next(), chars.next())
            && c.is_ascii_uppercase()
        {
            return true;
        }
    }

    false
}

/// Patch `select()`, `selectFromWriteConnection()`, `selectResultSets()`
/// return types from bare `array` to `array<int, stdClass>`, and
/// `selectOne()` from `mixed` to `stdClass|null`.
///
/// Both the `DB` facade (`@method` annotations) and the underlying
/// `Illuminate\Database\Connection` class declare these methods with
/// imprecise return types.  The actual runtime return is always an
/// array of `stdClass` rows (or a single `stdClass|null` for
/// `selectOne`).  Patching this here lets property access on query
/// results resolve correctly across the codebase.
fn patch_db_select_return_types(class: &mut ClassInfo) {
    let std_class = PhpType::Named("stdClass".to_owned());
    let array_of_std = PhpType::generic_array(PhpType::int(), std_class.clone());
    let std_or_null = PhpType::Nullable(Box::new(std_class));

    for method in class.methods.make_mut().iter_mut() {
        let method = Arc::make_mut(method);
        match method.name.as_str() {
            "select" | "selectFromWriteConnection" | "selectResultSets"
                if method
                    .return_type
                    .as_ref()
                    .is_some_and(|rt| rt.is_bare_array()) =>
            {
                method.return_type = Some(array_of_std.clone());
            }
            "selectOne" if method.return_type.as_ref().is_some_and(|rt| rt.is_mixed()) => {
                method.return_type = Some(std_or_null.clone());
            }
            _ => {}
        }
    }
}

/// Restore the `@template TCacheValue` generics that Laravel strips from
/// the `Cache` facade's `@method` tags.
///
/// The facade's generated docblock declares closure-caching methods like
/// `remember()` as `@method static mixed remember(…, \Closure $callback)`,
/// erasing the `@template TCacheValue` / `\Closure(): TCacheValue` /
/// `@return TCacheValue` that the underlying `Illuminate\Cache\Repository`
/// method carries.  This patch re-types the callback parameter and adds
/// the method-level template so `Cache::remember($k, $ttl, fn() => new
/// Foo())` resolves to the closure's return type instead of `mixed`.
///
/// Binding is keyed on the parameter named `$callback`, which is the
/// value-producing closure across all of these methods (the `$ttl`
/// parameter of `remember()` also accepts a `\Closure`, so matching by
/// name rather than by type is required).
fn patch_cache_facade_generics(class: &mut ClassInfo) {
    use crate::atom::atom;

    const TEMPLATE: &str = "TCacheValue";
    // Methods whose `$callback` closure produces the cached value.
    const CALLBACK_METHODS: &[&str] = &[
        "remember",
        "rememberForever",
        "sear",
        "flexible",
        "withoutOverlapping",
    ];

    let callback_hint = PhpType::parse("Closure(): TCacheValue");
    let template_return = PhpType::Named(TEMPLATE.to_owned());

    for method in class.methods.make_mut().iter_mut() {
        if !CALLBACK_METHODS.contains(&method.name.as_str()) {
            continue;
        }
        // Only patch the generated `mixed` form; leave a hand-written
        // override with a real return type untouched.
        if !method.return_type.as_ref().is_some_and(|rt| rt.is_mixed()) {
            continue;
        }
        // Locate the value-producing closure parameter by name.
        let callback_name = match method
            .parameters
            .iter()
            .find(|p| p.name.as_str() == "$callback")
        {
            Some(param) => param.name,
            None => continue,
        };

        let method = Arc::make_mut(method);
        // Re-type the callback as `Closure(): TCacheValue` so the template
        // binder classifies it as a callable-return binding.
        for param in method.parameters.iter_mut() {
            if param.name == callback_name {
                param.type_hint = Some(callback_hint.clone());
                param.native_type_hint = Some(callback_hint.clone());
                break;
            }
        }
        method.template_params = vec![atom(TEMPLATE)];
        method.template_param_bounds = Default::default();
        method.template_bindings = vec![(atom(TEMPLATE), callback_name)];
        method.return_type = Some(template_return.clone());
    }
}

/// Parameterise the `paginate()`, `simplePaginate()`, and
/// `cursorPaginate()` return types on the Eloquent Builder with the
/// element type `<int, TModel>`.
///
/// The framework declares these methods as returning an unparameterised
/// paginator (`\Illuminate\Pagination\LengthAwarePaginator`,
/// `\Illuminate\Contracts\Pagination\Paginator`, and
/// `\Illuminate\Contracts\Pagination\CursorPaginator`), so
/// `foreach (Model::paginate() as $row)` has no declared element type.
/// Every paginator class and contract carries `@template TKey` /
/// `@template TValue` and exposes its values via
/// `IteratorAggregate<TKey, TValue>`, so binding `TValue` to the
/// Builder's `TModel` recovers the element type.  `TModel` is
/// substituted to the concrete model during Builder resolution, exactly
/// as it is for `get()`'s `Collection<int, TModel>` return type.
fn patch_eloquent_builder_paginate_element_type(class: &mut ClassInfo) {
    for method in class.methods.make_mut().iter_mut() {
        if !matches!(
            method.name.as_str(),
            "paginate" | "simplePaginate" | "cursorPaginate"
        ) {
            continue;
        }
        // Only patch the bare, unparameterised paginator declaration.  A
        // hand-written override that already carries generics (a
        // `PhpType::Generic`) is left untouched.
        let paginator_name = match method.return_type.as_ref() {
            Some(PhpType::Named(name)) if name.contains("Paginator") => name.clone(),
            _ => continue,
        };
        let element_type = PhpType::Generic(
            paginator_name,
            vec![PhpType::int(), PhpType::Named("TModel".to_owned())],
        );
        Arc::make_mut(method).return_type = Some(element_type);
    }
}

/// Correct `Storage::fake()` and `Storage::persistentFake()` return
/// types from the `Filesystem` contract to the concrete
/// `Illuminate\Filesystem\FilesystemAdapter`.
///
/// Both methods declare `@return \Illuminate\Contracts\Filesystem\Filesystem`
/// but their bodies unconditionally build a `FilesystemAdapter` via
/// `createLocalDriver()`.  Assertion helpers like `assertExists()` and
/// `assertMissing()` live only on the concrete adapter, so the idiomatic
/// `$disk = Storage::fake(); $disk->assertExists(...)` pattern needs the
/// precise return type to resolve.  This is a declared-type correction
/// (the runtime type is always the adapter), not container-binding
/// resolution.
fn patch_storage_fake_return_types(class: &mut ClassInfo) {
    let adapter = PhpType::Named(FILESYSTEM_ADAPTER_FQN.to_owned());
    for method in class.methods.make_mut().iter_mut() {
        if method.name != "fake" && method.name != "persistentFake" {
            continue;
        }
        // Only correct the honestly-declared contract return.  Leave a
        // hand-written override with a different type untouched.
        let returns_contract = method
            .return_type
            .as_ref()
            .and_then(|rt| rt.class_name())
            .is_some_and(|n| n.trim_start_matches('\\') == FILESYSTEM_CONTRACT_FQN);
        if returns_contract {
            Arc::make_mut(method).return_type = Some(adapter.clone());
        }
    }
}

/// Check whether a class uses the `Conditionable` trait (directly or
/// through its trait list / parent chain markers).
///
/// We check `used_traits` for both the FQN and the short name since
/// trait usage may be recorded in either form depending on how the
/// source was parsed.
fn class_uses_conditionable(class: &ClassInfo) -> bool {
    class
        .used_traits
        .iter()
        .any(|t| t == CONDITIONABLE_FQN || t == "Conditionable" || t.ends_with("\\Conditionable"))
}

#[cfg(test)]
#[path = "patches_tests.rs"]
mod tests;
