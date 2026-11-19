//! Prophecy test doubles.
//!
//! `$this->prophesize(OrderRepository::class)` returns an
//! `ObjectProphecy<OrderRepository>`.  Its `reveal()` gives the double,
//! which the generic declaration already types, and a call named after one
//! of the mocked class's methods goes through `ObjectProphecy::__call()` to
//! set up a `MethodProphecy`.  This module grafts those methods onto the
//! resolved prophecy, so they complete and navigate like real members, and
//! finds the mocked class again for go-to-definition.

use std::sync::Arc;

use crate::atom::atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, MethodInfo, Visibility};
use crate::virtual_members::{ResolvedClassCache, VirtualMembers, merge_virtual_members};

const OBJECT_PROPHECY: &str = "Prophecy\\Prophecy\\ObjectProphecy";
const METHOD_PROPHECY: &str = "Prophecy\\Prophecy\\MethodProphecy";
const TOKEN_INTERFACE: &str = "Prophecy\\Argument\\Token\\TokenInterface";

/// Give a resolved `ObjectProphecy<T>` the methods of the class `T` names.
/// `raw` is the class as parsed and `generic_args` the type arguments the
/// resolution binds.
///
/// Each grafted method returns a `MethodProphecy` and takes, in place of
/// each parameter's type, that type or an argument token
/// (`Argument::any()`), which is what `__call()` accepts.  Methods the
/// prophecy declares itself, such as `reveal()`, keep their declaration.
pub(crate) fn apply_to_object_prophecy(
    resolved: &mut ClassInfo,
    raw: &ClassInfo,
    generic_args: &[PhpType],
    class_loader: &ResolvedClassLoader<'_>,
    cache: Option<&ResolvedClassCache>,
) {
    let Some(mocked) = generic_args.first() else {
        return;
    };
    if raw.fqn() != OBJECT_PROPHECY {
        return;
    }
    let return_type = PhpType::named(atom(METHOD_PROPHECY));
    let mut methods = Vec::new();
    for class in loaded_classes(mocked, class_loader) {
        let class =
            crate::virtual_members::resolve_class_fully_maybe_cached(&class, class_loader, cache);
        for method in class.methods.iter() {
            if method.is_static
                || method.visibility != Visibility::Public
                || method.name.starts_with("__")
            {
                continue;
            }
            methods.push(Arc::new(prophesied(method, &return_type)));
        }
    }
    if methods.is_empty() {
        return;
    }
    merge_virtual_members(
        resolved,
        VirtualMembers {
            methods,
            properties: Vec::new(),
            constants: Vec::new(),
        },
    );
    resolved.rebuild_method_index();
}

/// `method` as a call on its prophecy: the same name, description, and
/// parameters, each also taking a token, returning `return_type`.
fn prophesied(method: &MethodInfo, return_type: &PhpType) -> MethodInfo {
    let token = PhpType::named(atom(TOKEN_INTERFACE));
    let parameters = method
        .parameters
        .iter()
        .map(|param| {
            let mut param = param.clone();
            if let Some(hint) = param.type_hint.take() {
                param.type_hint = Some(if hint.is_mixed() {
                    hint
                } else {
                    PhpType::union(vec![hint, token.clone()])
                });
            }
            param.native_type_hint = None;
            param.param_out_type = None;
            param.closure_this_type = None;
            param.callable_invocation = None;
            param.expected_values = None;
            param
        })
        .collect();
    MethodInfo {
        parameters,
        description: method.description.clone(),
        // Setting up an expectation is the point of the call, so a
        // statement that discards the `MethodProphecy` is not an unused
        // result.
        is_impure: true,
        ..MethodInfo::virtual_method_typed(&method.name, Some(return_type))
    }
}

/// The classes a mocked type names: the class itself, or each member of
/// the intersection `willImplement()` builds.
fn loaded_classes(mocked: &PhpType, class_loader: &ResolvedClassLoader<'_>) -> Vec<Arc<ClassInfo>> {
    mocked
        .intersection_members()
        .into_iter()
        .filter_map(|member| match member.kind() {
            TypeKind::Named(name) => class_loader(Fqn::new(name)),
            TypeKind::Generic(generic) => class_loader(Fqn::new(&generic.name)),
            _ => None,
        })
        .filter(|class| class.fqn() != OBJECT_PROPHECY)
        .collect()
}

/// The classes a resolved `ObjectProphecy<T>` doubles, read off what its
/// `reveal()` returns; empty for anything else.
pub(crate) fn revealed_classes(
    resolved: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    if resolved.fqn() != OBJECT_PROPHECY {
        return Vec::new();
    }
    resolved
        .methods
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case("reveal") && !m.is_static)
        .and_then(|m| m.return_type.as_ref())
        .map(|revealed| loaded_classes(revealed, class_loader))
        .unwrap_or_default()
}
