//! What invoking an array callable returns.
//!
//! `[$object, 'method']`, `[Foo::class, 'method']` and `['Foo', 'method']`
//! name the method they call exactly, so invoking one is the same call as
//! `$object->method()` or `Foo::method()`.

use crate::php_type::{PhpType, ShapeEntry, TypeKind};
use crate::type_engine::resolver::FileCtx;
use crate::virtual_members::ResolvedClassCache;

/// The return type of the method the array callable `callable` names, with
/// `self` and `static` bound to the class it is called on.
///
/// `None` when `callable` is not a two-element array naming a method by a
/// literal string, or the method cannot be found.  A class named in the
/// first slot (`Foo::class`, `'Foo'`) is called statically, so only its static
/// methods answer: PHP refuses to call an instance method that way.
pub(crate) fn array_callable_return_type(
    callable: &PhpType,
    file: &FileCtx<'_>,
    current_class_fqn: &str,
    resolved_class_cache: Option<&ResolvedClassCache>,
) -> Option<PhpType> {
    let [receiver, method] = callable.shape_entries()? else {
        return None;
    };
    let positional = |entry: &ShapeEntry, index: &str| {
        !entry.optional && entry.key.as_deref().is_none_or(|k| k == index)
    };
    if !positional(receiver, "0") || !positional(method, "1") {
        return None;
    }
    let TypeKind::Literal(literal) = method.value_type.kind() else {
        return None;
    };
    let method_name = literal.string_content()?;

    let receiver = &receiver.value_type;
    let static_only = !receiver.is_object_like();
    let owners = if static_only {
        crate::type_engine::variable::class_string_resolution::classes_named_by_type(
            receiver,
            current_class_fqn,
            file.all_classes,
            file.class_loader,
        )
    } else {
        crate::type_engine::type_resolution::type_hint_to_classes_typed(
            receiver,
            current_class_fqn,
            file.all_classes,
            file.class_loader,
        )
    };

    owners.iter().find_map(|owner| {
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            owner,
            file.class_loader,
            resolved_class_cache,
        );
        let method = merged.get_method(&method_name)?;
        if static_only && !method.is_static {
            return None;
        }
        let ret = method.return_type.as_ref()?;
        Some(ret.replace_self_bound(&owner.fqn(), None))
    })
}
