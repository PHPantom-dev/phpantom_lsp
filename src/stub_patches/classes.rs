//! Patches for the classes phpstorm-stubs declares incompletely.

use crate::atom::{Atom, atom};
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
        "IteratorIterator" => patch_iterator_iterator(class),
        "RecursiveIteratorIterator" => patch_recursive_iterator_iterator(class),
        "FilterIterator" => patch_filter_iterator(class),
        "NoRewindIterator" => patch_no_rewind_iterator(class),
        "CachingIterator" => patch_caching_iterator(class),
        "InfiniteIterator" => patch_infinite_iterator(class),
        "LimitIterator" => patch_limit_iterator(class),
        "CallbackFilterIterator" => patch_callback_filter_iterator(class),
        "SimpleXMLElement" => patch_simple_xml_element(class),
        "ReflectionClass" => patch_reflection_class(class),
        "ReflectionObject" => patch_reflection_object(class),
        "DOMNamedNodeMap" => patch_dom_named_node_map(class),
        "DOMNode" => patch_dom_node(class),
        "DOMElement" => patch_dom_element(class),
        _ => {}
    }
    mark_benevolent_methods(class);
    mark_impure_methods(class);
}

/// Built-in methods that change their object's state while returning a
/// value, so nothing but a tag can say so.  The list is PHPStan's
/// (`functionMetadata.php`, every class method with `hasSideEffects`).
///
/// `SplFileObject::fgets()` returns the line it read, and moves the cursor
/// that `eof()` reports on; without the tag, a checked `eof()` would stay
/// proven across the read.
const IMPURE_BUILTIN_METHODS: &[(&str, &[&str])] = &[
    (
        "DateTime",
        &[
            "add",
            "modify",
            "setDate",
            "setISODate",
            "setTime",
            "setTimestamp",
            "setTimezone",
            "sub",
        ],
    ),
    ("SplDoublyLinkedList", &["pop", "shift"]),
    (
        "SplFileObject",
        &[
            "fflush",
            "fgetc",
            "fgetcsv",
            "fgets",
            "fgetss",
            "fpassthru",
            "fputcsv",
            "fread",
            "fscanf",
            "fseek",
            "ftruncate",
            "fwrite",
        ],
    ),
    ("SplFixedArray", &["extract"]),
    ("SplHeap", &["extract", "insert", "recoverFromCorruption"]),
    (
        "SplObjectStorage",
        &["addAll", "attach", "detach", "removeAll", "removeAllExcept"],
    ),
    (
        "SplPriorityQueue",
        &["extract", "insert", "recoverFromCorruption"],
    ),
    ("SplQueue", &["dequeue"]),
    ("XMLReader", &["next", "read"]),
];

/// Tag the class's [`IMPURE_BUILTIN_METHODS`] `@impure`.
fn mark_impure_methods(class: &mut ClassInfo) {
    let Some((_, methods)) = IMPURE_BUILTIN_METHODS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&class.name))
    else {
        return;
    };
    for idx in 0..class.methods.len() {
        let method = &class.methods[idx];
        if method.is_impure
            || method.is_pure
            || !methods.iter().any(|m| m.eq_ignore_ascii_case(&method.name))
        {
            continue;
        }
        update_method_at(class, idx, |method| method.is_impure = true);
    }
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
        let Some(idx) = method_index(class, method_name) else {
            continue;
        };
        // A version that grew the tag upstream keeps its own.
        if !class.methods[idx].type_assertions().is_empty() {
            continue;
        }
        update_method_at(class, idx, |method| {
            method
                .extras_mut()
                .type_assertions
                .push(crate::types::TypeAssertion {
                    kind: crate::types::AssertionKind::IfTrue,
                    param_name: (*subject).to_string(),
                    asserted_type: PhpType::null(),
                    negated: true,
                    is_equality: false,
                });
        });
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
        update_method_at(class, idx, |method| method.return_type = Some(tagged));
    }
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
    add_iterator_templates(class);
    add_implements_generics(class, "OuterIterator", &["TKey", "TValue"]);
    // Add @mixin TIterator so that methods from the wrapped iterator
    // are available on the wrapper.
    let t_iter = atom("TIterator");
    if !class.mixins.contains(&t_iter) {
        class.mixins.push(t_iter);
    }

    // Patch current() → TValue and key() → TKey.
    // phpstorm-stubs declare `current(): mixed` and `key(): mixed` which
    // hides the generic type.  PHPStan's stubs override these.
    patch_method_return_type(class, "current", PhpType::named(atom("TValue")));
    patch_method_return_type(class, "key", PhpType::named(atom("TKey")));
    patch_inner_iterator_return_type(class);

    // Patch the constructor: add template binding TIterator → $iterator
    // so that `new IteratorIterator(new Subject())` infers TIterator = Subject.
    patch_constructor_template_binding(class, "TIterator", "$iterator");
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
    patch_constructor_template_binding(class, "T", "$iterator");
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
    patch_inner_iterator_return_type(class);
}

/// Type `getInnerIterator()` as `Iterator<TKey, TValue>`, the return
/// PHPStan's stub gives `OuterIterator::getInnerIterator()`.
///
/// phpstorm-stubs redeclare the method as `?Iterator` on `IteratorIterator`
/// and `FilterIterator`, so the inner iterator's elements would otherwise
/// be `mixed` however the wrapper is parameterised.
fn patch_inner_iterator_return_type(class: &mut ClassInfo) {
    patch_method_return_type(
        class,
        "getInnerIterator",
        PhpType::generic(
            "Iterator",
            vec![PhpType::named(atom("TKey")), PhpType::named(atom("TValue"))],
        ),
    );
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
    patch_constructor_template_binding(class, "TIterator", "$iterator");
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
    patch_constructor_template_binding(class, "TIterator", "$iterator");
}

/// Patch `InfiniteIterator` with template params inherited from `IteratorIterator`.
///
/// PHPStan ref: `stubs/iterable.stub`
fn patch_infinite_iterator(class: &mut ClassInfo) {
    if !class.template_params.is_empty() {
        return;
    }
    patch_iterator_iterator_subclass(class, "IteratorIterator");
    patch_constructor_template_binding(class, "TIterator", "$iterator");
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
    patch_constructor_template_binding(class, "TIterator", "$iterator");
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
    patch_constructor_template_binding(class, "TIterator", "$iterator");
}

/// Type `DOMNamedNodeMap::item()` by the map's node type.
///
/// The class is `@template-covariant TNode of DOMNode` and its siblings
/// (`getNamedItem()`, `getNamedItemNS()`, `getIterator()`) hand back
/// `TNode`, but `item()` is declared `DOMNode|null`, so an element's
/// `attributes->item(0)` loses the `DOMAttr` every other accessor keeps.
fn patch_dom_named_node_map(class: &mut ClassInfo) {
    with_method_mut(class, "item", |method| {
        method.return_type = Some(PhpType::union(vec![
            PhpType::named(atom("TNode")),
            PhpType::null(),
        ]));
    });
}

/// Let `DOMNode::hasAttributes()` prove `$attributes` is not null.
///
/// Only an element has an attribute map, so a node that has attributes is
/// one.  The tag is the equality form (`!=null`) because the promise is
/// one-way: an element without attributes answers false and still has an
/// (empty) map, so the false branch must not narrow to `null`.
fn patch_dom_node(class: &mut ClassInfo) {
    let Some(idx) = class
        .methods
        .iter()
        .position(|m| m.name.eq_ignore_ascii_case("hasAttributes"))
    else {
        return;
    };
    update_method_at(class, idx, |method| {
        method
            .extras_mut()
            .type_assertions
            .push(crate::types::TypeAssertion {
                kind: crate::types::AssertionKind::IfTrue,
                param_name: "$this->attributes".to_string(),
                asserted_type: PhpType::null(),
                negated: true,
                is_equality: true,
            });
    });
}

/// Drop the `null` from `DOMElement::$attributes`.
///
/// The property is inherited from `DOMNode`, where it is null for every
/// node that is not an element, and the stub keeps the native
/// `DOMNamedNodeMap|null` on the element's redeclaration too, so its
/// `@var DOMNamedNodeMap<DOMAttr>` only refines the non-null half.  An
/// element always has an attribute map (empty when it has no attributes).
fn patch_dom_element(class: &mut ClassInfo) {
    let Some(idx) = class
        .properties
        .iter()
        .position(|p| p.name.as_str() == "attributes")
    else {
        return;
    };
    let mut property = (*class.properties[idx]).clone();
    let map = PhpType::generic("DOMNamedNodeMap", vec![PhpType::named(atom("DOMAttr"))]);
    property.native_type_hint = Some(PhpType::named(atom("DOMNamedNodeMap")));
    property.type_hint = Some(map);
    class.properties.make_mut()[idx] = std::sync::Arc::new(property);
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

/// Let `ReflectionClass::isSubclassOf()` narrow the class it reflects.
///
/// `$r->isSubclassOf(Picture::class)` holding means `$r` reflects a
/// `Picture`, so `$r` is a `ReflectionClass<Picture>` in that branch.
/// PHPStan says so with a type-specifying extension; spelling the same
/// promise as a method template and an `-if-true` tag lets the ordinary
/// assertion narrowing carry it.
fn patch_reflection_is_subclass_of(class: &mut ClassInfo) {
    let Some(idx) = class
        .methods
        .iter()
        .position(|m| m.name.eq_ignore_ascii_case("isSubclassOf"))
    else {
        return;
    };
    let Some(param) = class.methods[idx].parameters.first().map(|p| p.name) else {
        return;
    };
    if !class.methods[idx].type_assertions().is_empty() {
        return;
    }
    let template = atom("TIsSubclassOf");
    update_method_at(class, idx, |method| {
        let extras = method.extras_mut();
        extras.template_params.push(template);
        extras
            .template_param_bounds
            .insert(template, PhpType::object());
        extras.template_bindings.push((template, param));
        extras.type_assertions.push(crate::types::TypeAssertion {
            kind: crate::types::AssertionKind::IfTrue,
            param_name: "$this".to_string(),
            asserted_type: PhpType::generic_atom(
                atom("ReflectionClass"),
                vec![PhpType::named(template)],
            ),
            negated: false,
            is_equality: false,
        });
    });
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
    patch_reflection_is_subclass_of(class);

    with_method_mut(class, "getInterfaceNames", |method| {
        method.return_type = Some(PhpType::parse("list<class-string>"));
    });

    let Some(idx) = method_index(class, "newInstanceArgs") else {
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
    update_method_at(class, idx, |method| {
        if non_null_return.is_some() {
            method.return_type = non_null_return;
        }
        if non_null_native.is_some() {
            method.native_return_type = non_null_native;
        }
    });
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

    patch_constructor_template_binding(class, "T", "$object");
}

/// Give a method a conditional return type, provided the stub declares the
/// parameter the conditional keys on.
fn patch_method_conditional_return(
    class: &mut ClassInfo,
    method_name: &str,
    param_name: &str,
    conditional: PhpType,
) {
    let Some(idx) = method_index(class, method_name) else {
        return;
    };
    if !class.methods[idx]
        .parameters
        .iter()
        .any(|p| p.name == param_name)
    {
        return;
    }
    update_method_at(class, idx, |method| {
        method.extras_mut().conditional_return = Some(conditional);
    });
}

/// Shared helper: add `@template TKey, TValue, TIterator` and
/// `@extends <parent><TKey, TValue, TIterator>` to an `IteratorIterator`
/// subclass (or sub-subclass like `CallbackFilterIterator`).
fn patch_iterator_iterator_subclass(class: &mut ClassInfo, parent: &str) {
    add_iterator_templates(class);
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

/// Bind the class template `template` from the constructor's `param`.
///
/// Like a parsed constructor whose `@param` names a class template, the
/// constructor takes on the class's template list, which is what lets a
/// call site bind it from the argument rather than leave the parameter
/// typed with the bare template name.
fn bind_ctor_template(
    ctor: &mut crate::types::MethodInfo,
    class_template_params: &[Atom],
    template: Atom,
    param: Atom,
) {
    let extras = ctor.extras_mut();
    if extras.template_params.is_empty() {
        extras.template_params = class_template_params.to_vec();
    }
    if !extras.template_bindings.iter().any(|(t, _)| *t == template) {
        extras.template_bindings.push((template, param));
    }
}

/// Patch the constructor to bind the class template `template` from its
/// `param` parameter, and retype that parameter to the template.
///
/// The template hint (rather than the stub's `Traversable` or `object`) is
/// what makes `classify_template_binding` read the argument as a direct
/// bind.
fn patch_constructor_template_binding(class: &mut ClassInfo, template: &str, param: &str) {
    let Some(ctor_idx) = method_index(class, "__construct") else {
        return;
    };
    let template = atom(template);
    let mut ctor = (*class.methods[ctor_idx]).clone();
    bind_ctor_template(&mut ctor, &class.template_params, template, atom(param));
    if let Some(p) = ctor
        .parameters
        .make_mut()
        .iter_mut()
        .find(|p| p.name == param)
    {
        p.type_hint = Some(PhpType::named(template));
    }
    class.methods.make_mut()[ctor_idx] = std::sync::Arc::new(ctor);
}

/// Override a method's return type on a class.
///
/// If the method exists, replaces its `return_type` with the given type.
/// Used to patch stub methods like `current(): mixed` → `current(): TValue`.
fn patch_method_return_type(class: &mut ClassInfo, method_name: &str, return_type: PhpType) {
    with_method_mut(class, method_name, |method| {
        method.return_type = Some(return_type);
    });
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Index of the method named exactly `name`.
fn method_index(class: &ClassInfo, name: &str) -> Option<usize> {
    class.methods.iter().position(|m| m.name.as_str() == name)
}

/// Replace the method at `idx` with a copy `f` has mutated.
fn update_method_at(
    class: &mut ClassInfo,
    idx: usize,
    f: impl FnOnce(&mut crate::types::MethodInfo),
) {
    let mut method = (*class.methods[idx]).clone();
    f(&mut method);
    class.methods.make_mut()[idx] = std::sync::Arc::new(method);
}

/// Mutate the method named exactly `name`, if the class declares it.
fn with_method_mut(
    class: &mut ClassInfo,
    name: &str,
    f: impl FnOnce(&mut crate::types::MethodInfo),
) {
    if let Some(idx) = method_index(class, name) {
        update_method_at(class, idx, f);
    }
}

/// Add `@template TKey`, `@template TValue` and
/// `@template TIterator of Traversable<TKey, TValue>`.
fn add_iterator_templates(class: &mut ClassInfo) {
    add_templates(class, &[("TKey", None), ("TValue", None)]);
    // TIterator has a complex bound `Traversable<TKey, TValue>` — add it
    // manually since `add_templates` only handles simple string bounds.
    let t_iter = atom("TIterator");
    if !class.template_params.contains(&t_iter) {
        class.template_params.push(t_iter);
    }
    class
        .template_param_bounds
        .entry(t_iter)
        .or_insert_with(|| {
            PhpType::generic(
                "Traversable",
                vec![PhpType::named(atom("TKey")), PhpType::named(atom("TValue"))],
            )
        });
}

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
