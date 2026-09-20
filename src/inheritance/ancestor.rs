//! Generic arguments projected through declared class and interface ancestry.

use std::collections::HashMap;
use std::sync::Arc;

use crate::php_type::{PhpType, TypeKind};
use crate::types::ClassInfo;

/// Extract a generic type argument from a class's ancestor chain.
///
/// Given an argument type (e.g. `FooContainer`) and a target wrapper class
/// (e.g. `Container`), walks the `@extends` chain to find where the argument
/// type (or one of its ancestors) extends the wrapper class, then extracts the
/// generic argument at `tpl_position`.
///
/// For example, if `FooContainer` has `@extends Container<Foo>`, calling
/// `extract_generic_arg_from_ancestor(FooContainer, "Container", 0, ...)` returns `Foo`.
pub(crate) fn extract_generic_arg_from_ancestor(
    arg_type: &PhpType,
    wrapper_name: &str,
    tpl_position: usize,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    if let TypeKind::Union(parts) = arg_type.kind() {
        let args: Vec<_> = parts
            .iter()
            .filter_map(|part| {
                extract_generic_arg_from_ancestor(part, wrapper_name, tpl_position, class_loader)
            })
            .collect();
        return (!args.is_empty()).then(|| PhpType::union(args).simplified());
    }
    let class_name = match arg_type.kind() {
        TypeKind::Named(n) => n.as_str(),
        TypeKind::Generic(g) => g.name.as_str(),
        _ => return None,
    };

    // If the arg type itself is already generic with the wrapper name,
    // extract directly.  E.g. argument type is `Container<Foo>`.
    if let TypeKind::Generic(g) = arg_type.kind()
        && ancestor_name_matches(&g.name, wrapper_name)
    {
        return g.args.get(tpl_position).cloned();
    }

    let cls = class_loader(class_name)?;

    let subs = match arg_type.kind() {
        TypeKind::Generic(g) => super::build_generic_subs(&cls, &g.args),
        _ => HashMap::new(),
    };
    let mut visited = Vec::new();
    ancestor_generic_arg(
        &cls,
        wrapper_name,
        tpl_position,
        &subs,
        &mut visited,
        class_loader,
    )
}

/// Maximum ancestry depth walked while looking for an ancestor's generic
/// argument.  A backstop against an `extends`/`implements` cycle the
/// loader hands back; the `visited` set is what actually bounds the work.
const MAX_ANCESTOR_GENERIC_DEPTH: usize = 15;

/// The type argument `target_name` receives at `position`, as seen from
/// `cls`.
///
/// Walks the parent chain **and** the interface list, threading each
/// level's `@extends`/`@implements` arguments into the next, so a class
/// that reaches the ancestor only through an intermediate generic
/// interface still reports a concrete argument.
/// `X implements CollectorWithPaths<never, array{…}>` together with
/// `CollectorWithPaths extends Collector<TNodeType, TValue>` is what says
/// `Collector`'s value argument is that `array{…}`.
fn ancestor_generic_arg(
    cls: &ClassInfo,
    target_name: &str,
    position: usize,
    subs: &HashMap<String, PhpType>,
    visited: &mut Vec<crate::atom::Atom>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    if visited.len() > MAX_ANCESTOR_GENERIC_DEPTH {
        return None;
    }
    let fqn = cls.fqn();
    if visited.contains(&fqn) {
        return None;
    }
    visited.push(fqn);

    if let Some(arg) = find_extends_generic_arg(cls, target_name, position) {
        return Some(if subs.is_empty() {
            arg
        } else {
            arg.substitute(subs)
        });
    }

    for ancestor_name in cls.parent_class.iter().chain(cls.interfaces.iter()) {
        let Some(ancestor) = class_loader(ancestor_name) else {
            continue;
        };
        let next_subs = crate::inheritance::build_substitution_map(
            &crate::inheritance::ClassRef::Borrowed(cls),
            &ancestor,
            subs,
        );
        if let Some(arg) = ancestor_generic_arg(
            &ancestor,
            target_name,
            position,
            &next_subs,
            visited,
            class_loader,
        ) {
            return Some(arg);
        }
    }

    None
}

/// Find a generic arg at `position` from a class's `@extends` generics
/// matching a qualified or unqualified ancestor name.
fn find_extends_generic_arg(
    cls: &ClassInfo,
    target_name: &str,
    position: usize,
) -> Option<PhpType> {
    for (name, args) in cls
        .extends_generics
        .iter()
        .chain(cls.implements_generics.iter())
    {
        if ancestor_name_matches(name, target_name) {
            return args.get(position).cloned();
        }
    }
    None
}

fn ancestor_name_matches(actual: &str, target: &str) -> bool {
    let target = target.trim_start_matches('\\');
    let actual = actual.trim_start_matches('\\');
    if target.contains('\\') {
        actual.eq_ignore_ascii_case(target)
    } else {
        crate::util::short_name(actual).eq_ignore_ascii_case(target)
    }
}
