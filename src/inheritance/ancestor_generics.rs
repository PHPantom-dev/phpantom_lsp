//! Generic arguments projected through declared class and interface ancestry.

use std::collections::HashMap;

use super::ancestry::ClassLoader;
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
///
/// A union argument projects each alternative and unions what they give,
/// so `HasMany<Post, User>|HasOne<Comment, User>` hands `Relation` a
/// `Post|Comment` related model.
pub(crate) fn extract_generic_arg_from_ancestor(
    arg_type: &PhpType,
    wrapper_name: &str,
    tpl_position: usize,
    class_loader: ClassLoader<'_>,
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
    extract_generic_args_from_ancestor(arg_type, wrapper_name, class_loader)?
        .into_iter()
        .nth(tpl_position)
}

/// Every generic argument `arg_type` hands its `wrapper_name` ancestor, for
/// a caller that picks the position itself (a single-argument hint names
/// the last of several).
pub(crate) fn extract_generic_args_from_ancestor(
    arg_type: &PhpType,
    wrapper_name: &str,
    class_loader: ClassLoader<'_>,
) -> Option<Vec<PhpType>> {
    let class_name = match arg_type.kind() {
        TypeKind::Named(n) => n.as_str(),
        TypeKind::Generic(g) => g.name.as_str(),
        _ => return None,
    };

    // If the arg type itself is already generic with the wrapper name,
    // extract directly.  E.g. argument type is `Container<Foo>`.
    if let TypeKind::Generic(g) = arg_type.kind()
        && generic_wrapper_matches(&g.name, wrapper_name)
    {
        return Some(g.args.clone());
    }

    let cls = class_loader(class_name)?;

    // The argument's own type arguments are what its `@extends`/
    // `@implements` names stand for: `ClassStringType<class-string<Foo>>`
    // with `@implements Type<class-string<T>>` hands `Type` a
    // `class-string<Foo>`, not a `class-string<T>`.
    let subs = match arg_type.kind() {
        TypeKind::Generic(g) => super::build_generic_subs(&cls, &g.args),
        _ => HashMap::new(),
    };
    let mut visited = Vec::new();
    ancestor_generic_args(&cls, wrapper_name, &subs, &mut visited, class_loader)
}

/// Maximum ancestry depth walked while looking for an ancestor's generic
/// argument.  A backstop against an `extends`/`implements` cycle the
/// loader hands back; the `visited` set is what actually bounds the work.
const MAX_ANCESTOR_GENERIC_DEPTH: usize = 15;

/// The type arguments the `target` ancestor receives, as seen from `cls`.
///
/// Walks the parent chain **and** the interface list, threading each
/// level's `@extends`/`@implements` arguments into the next, so a class
/// that reaches the ancestor only through an intermediate generic
/// interface still reports concrete arguments.
/// `X implements CollectorWithPaths<never, array{…}>` together with
/// `CollectorWithPaths extends Collector<TNodeType, TValue>` is what says
/// `Collector`'s value argument is that `array{…}`.
fn ancestor_generic_args(
    cls: &ClassInfo,
    target: &str,
    subs: &HashMap<String, PhpType>,
    visited: &mut Vec<crate::atom::Atom>,
    class_loader: ClassLoader<'_>,
) -> Option<Vec<PhpType>> {
    if visited.len() > MAX_ANCESTOR_GENERIC_DEPTH {
        return None;
    }
    let fqn = cls.fqn();
    if visited.contains(&fqn) {
        return None;
    }
    visited.push(fqn);

    if let Some(args) = find_extends_generic_args(cls, target) {
        return Some(if subs.is_empty() {
            args.to_vec()
        } else {
            args.iter().map(|arg| arg.substitute(subs)).collect()
        });
    }

    for ancestor_name in cls.parent_class.iter().chain(cls.interfaces.iter()) {
        let Some(ancestor) = class_loader(ancestor_name) else {
            continue;
        };
        let next_subs =
            super::build_substitution_map(&super::ClassRef::Borrowed(cls), &ancestor, subs);
        if let Some(args) =
            ancestor_generic_args(&ancestor, target, &next_subs, visited, class_loader)
        {
            return Some(args);
        }
    }

    None
}

/// The generic args of a class's `@extends`/`@implements` clause naming
/// `target`.
fn find_extends_generic_args<'c>(cls: &'c ClassInfo, target: &str) -> Option<&'c [PhpType]> {
    cls.extends_generics
        .iter()
        .chain(cls.implements_generics.iter())
        .find(|(name, _)| generic_wrapper_matches(name, target))
        .map(|(_, args)| args.as_slice())
}

/// Whether `name` is the class `target` names. A qualified target must match
/// exactly, so `App\Builder` is not mistaken for Eloquent's `Builder`; a bare
/// short name still matches any namespace.
fn generic_wrapper_matches(name: &str, target: &str) -> bool {
    if target.contains('\\') {
        name.trim_start_matches('\\')
            .eq_ignore_ascii_case(target.trim_start_matches('\\'))
    } else {
        crate::util::short_name(name).eq_ignore_ascii_case(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morph_builder_wrappers_match_qualified_identity_and_explicit_short_names() {
        assert!(generic_wrapper_matches("App\\Builder", "Builder"));
        assert!(generic_wrapper_matches("App\\Builder", "\\app\\builder"));
        assert!(!generic_wrapper_matches(
            "App\\Builder",
            "Illuminate\\Database\\Eloquent\\Builder"
        ));
        assert!(!generic_wrapper_matches("App\\Other", "Builder"));
    }
}
