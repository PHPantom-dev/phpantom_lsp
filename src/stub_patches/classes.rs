//! Patches for the classes phpstorm-stubs declares incompletely.

use crate::atom::atom;
use crate::php_type::PhpType;
use crate::types::ClassInfo;

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
