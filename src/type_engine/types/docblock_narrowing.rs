//! Keeping a docblock type only where it narrows the native hint.
//!
//! PHP enforces a declaration's native hint, so a docblock type may stand in
//! for it only when every value the docblock describes is one the hint
//! accepts (PHPStan's `TypehintHelper::decideType`).  The parser makes that
//! call from the two types alone as it reads the declaration, which it cannot
//! do for a class the hint does not name: whether that class narrows the hint
//! takes the class hierarchy, and a docblock name that looks like the hint's
//! (`DateTimeImmutable` inside `namespace App`) may not even be the same
//! class.
//!
//! The parser lets the docblock win there, and records the type together
//! with the native hint qualified ([`names_class_beyond_native`],
//! [`UnverifiedDocType`]).  Loading the declaration keeps the docblock type
//! only where the class hierarchy confirms that it narrows the hint, and
//! puts the hint back otherwise ([`verify_class`], [`verify_function`]), the
//! way a class's imported type aliases are linked when it is loaded.

use std::sync::Arc;

use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, DocTypeSlot, FunctionInfo, UnverifiedDocType};

/// Whether `settled`, the type a declaration settled on in place of the
/// native hint `native`, names a class `native` does not.
///
/// Both types have their class names qualified.  Only a class a value can be
/// an instance of counts: a type argument (`Collection<User>`) is nothing
/// PHP enforces, and neither is a keyword (`static`) or a name
/// `not_a_class` claims (a template or a type alias).  A native hint that
/// names no class, or names `self`, `static` or `parent`, whose class is not
/// known by name, is left to the parser's judgement.
pub(crate) fn names_class_beyond_native(
    settled: &PhpType,
    native: &PhpType,
    not_a_class: &dyn Fn(&str) -> bool,
) -> bool {
    hint_names_classes(native)
        && alternatives(settled)
            .iter()
            .any(|alternative| is_beyond(alternative, native, not_a_class))
}

/// Whether the native hint `native` names a class by its name, and nothing
/// that stands for a class it does not name (`self`, `static`, `parent`).
///
/// Cheap enough to run before the hint is qualified, to skip qualifying
/// one that names no class.
pub(crate) fn hint_names_classes(native: &PhpType) -> bool {
    let mut names_one = false;
    for alternative in alternatives(native) {
        for part in intersected(alternative) {
            if part.is_self_like() {
                return false;
            }
            names_one |= instance_class(part).is_some();
        }
    }
    names_one
}

/// `class` with each of its [`ClassInfo::unverified_doc_types`] put back to
/// the native hint, unless `class_loader` confirms that the docblock type
/// narrows it.  `None` when every one of them does.
///
/// `class_loader` takes resolved names.
pub(crate) fn verify_class(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<ClassInfo> {
    let mut verified: Option<ClassInfo> = None;
    for unverified in class.unverified_doc_types.iter() {
        let current = verified.as_ref().unwrap_or(class);
        let Some(target) = find_member(current, unverified) else {
            continue;
        };
        let (settled, method) = match target {
            Member::Property(index) => (current.properties[index].type_hint.as_ref(), None),
            Member::Method(index) => {
                let method = &*current.methods[index];
                let settled = match unverified.slot {
                    DocTypeSlot::Param(position) => method
                        .parameters
                        .get(position as usize)
                        .and_then(|param| param.type_hint.as_ref()),
                    _ => method.return_type.as_ref(),
                };
                (settled, Some(method))
            }
        };
        let not_a_class = |name: &str| {
            current.template_params.iter().any(|t| t == name)
                || current.type_aliases.keys().any(|alias| alias == name)
                || method.is_some_and(|method| method.template_params().iter().any(|t| t == name))
        };
        if settled.is_none_or(|settled| {
            narrows_native(settled, &unverified.native, &not_a_class, class_loader)
        }) {
            continue;
        }
        let class = verified.get_or_insert_with(|| class.clone());
        let native = Some(unverified.native.clone());
        match target {
            Member::Method(index) => {
                let method = Arc::make_mut(&mut class.methods.make_mut()[index]);
                match unverified.slot {
                    DocTypeSlot::Param(position) => {
                        if let Some(param) = method.parameters.make_mut().get_mut(position as usize)
                        {
                            param.type_hint = native;
                        }
                    }
                    _ => method.return_type = native,
                }
            }
            Member::Property(index) => {
                Arc::make_mut(&mut class.properties.make_mut()[index]).type_hint = native;
            }
        }
    }
    verified
}

/// [`verify_class`] for a function's own
/// [`FunctionInfo::unverified_doc_types`].
pub(crate) fn verify_function(
    function: &mut FunctionInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) {
    let not_a_class = |name: &str| function.template_params.iter().any(|t| t == name);
    let losing: Vec<(DocTypeSlot, PhpType)> = function
        .unverified_doc_types
        .iter()
        .filter(|unverified| {
            let settled = match unverified.slot {
                DocTypeSlot::Return => function.return_type.as_ref(),
                DocTypeSlot::Param(position) => function
                    .parameters
                    .get(position as usize)
                    .and_then(|param| param.type_hint.as_ref()),
                DocTypeSlot::Property => None,
            };
            settled.is_some_and(|settled| {
                !narrows_native(settled, &unverified.native, &not_a_class, class_loader)
            })
        })
        .map(|unverified| (unverified.slot, unverified.native.clone()))
        .collect();
    for (slot, native) in losing {
        match slot {
            DocTypeSlot::Return => function.return_type = Some(native),
            DocTypeSlot::Param(position) => {
                if let Some(param) = function.parameters.make_mut().get_mut(position as usize) {
                    param.type_hint = Some(native);
                }
            }
            DocTypeSlot::Property => {}
        }
    }
}

/// Whether every class `settled` names beyond the native hint `native` is
/// one `native` accepts, as `class_loader` sees the hierarchy, so that
/// `settled` may stand in for the hint.
///
/// `class_loader` takes resolved names, and `not_a_class` claims the
/// templates and type aliases in scope, which say nothing about a class.
pub(crate) fn narrows_native(
    settled: &PhpType,
    native: &PhpType,
    not_a_class: &dyn Fn(&str) -> bool,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    !hint_names_classes(native)
        || alternatives(settled)
            .iter()
            // `object` and `mixed` name no class, but each takes in every
            // class the hint names and more besides.
            .filter(|alternative| {
                alternative.is_object()
                    || alternative.is_mixed()
                    || is_beyond(alternative, native, not_a_class)
            })
            .all(|alternative| {
                crate::class_lookup::is_subtype_of_typed(alternative, native, class_loader)
            })
}

/// Where the member an [`UnverifiedDocType`] belongs to sits in its class.
#[derive(Clone, Copy)]
enum Member {
    Method(usize),
    Property(usize),
}

/// The member of `class` that `unverified` was recorded for.
fn find_member(class: &ClassInfo, unverified: &UnverifiedDocType) -> Option<Member> {
    match unverified.slot {
        DocTypeSlot::Property => class
            .properties
            .iter()
            .position(|prop| prop.name == unverified.member)
            .map(Member::Property),
        _ => class
            .methods
            .iter()
            .position(|method| method.name == unverified.member)
            .map(Member::Method),
    }
}

/// Whether `alternative`, one alternative of a settled type, is a class the
/// native hint does not name, or an intersection with one.
fn is_beyond(alternative: &PhpType, native: &PhpType, not_a_class: &dyn Fn(&str) -> bool) -> bool {
    intersected(alternative).iter().any(|part| {
        instance_class(part)
            .is_some_and(|class| !hint_names_class(native, class) && !not_a_class(class))
    })
}

/// Whether the native hint `native` names `class`.
fn hint_names_class(native: &PhpType, class: &str) -> bool {
    alternatives(native).iter().any(|alternative| {
        intersected(alternative).iter().any(|part| {
            instance_class(part).is_some_and(|hinted| hinted.eq_ignore_ascii_case(class))
        })
    })
}

/// The alternatives of a union, or the type under a `?`, or `ty` itself.
///
/// The `null` a `?` adds is left out: it is no class.  A template stays
/// whole, bound and all (see [`instance_class`]).
fn alternatives(ty: &PhpType) -> &[PhpType] {
    if ty.as_template_param().is_some() {
        return std::slice::from_ref(ty);
    }
    match ty.kind() {
        TypeKind::Union(members) => members,
        TypeKind::Nullable(inner) => std::slice::from_ref(inner),
        _ => std::slice::from_ref(ty),
    }
}

/// The parts of an intersection, or `ty` itself.
fn intersected(ty: &PhpType) -> &[PhpType] {
    if ty.as_template_param().is_some() {
        return std::slice::from_ref(ty);
    }
    match ty.kind() {
        TypeKind::Intersection(members) => members,
        _ => std::slice::from_ref(ty),
    }
}

/// The class `ty` holds an instance of, when it names one: `Foo` or
/// `Foo<T>`, not `list<Foo>`, `static` or `mixed`.
///
/// A template is none, even marked with a bound that names classes: the
/// declaration's own template stands in for whatever its call binds, which
/// is the call's to check.
fn instance_class(ty: &PhpType) -> Option<&str> {
    if ty.as_template_param().is_some() {
        return None;
    }
    let name = match ty.kind() {
        TypeKind::Named(name) => name.as_str(),
        TypeKind::Generic(generic) => generic.name.as_str(),
        _ => return None,
    };
    let name = name.strip_prefix('\\').unwrap_or(name);
    (!crate::php_type::is_keyword_type(name)).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beyond(settled: &str, native: &str) -> bool {
        names_class_beyond_native(&PhpType::parse(settled), &PhpType::parse(native), &|name| {
            name == "T"
        })
    }

    #[test]
    fn a_class_the_hint_does_not_name_is_beyond_it() {
        assert!(beyond("App\\DateTimeImmutable", "DateTimeImmutable"));
        assert!(beyond("App\\Sub|null", "?App\\Base"));
        assert!(beyond("App\\Sub&Countable", "App\\Base"));
        assert!(beyond("App\\Sub<int>", "App\\Base"));
    }

    #[test]
    fn what_the_hint_names_is_not_beyond_it() {
        assert!(!beyond("App\\Base<int, App\\User>", "App\\Base"));
        assert!(!beyond("App\\Base|null", "?App\\Base"));
        assert!(!beyond("app\\base", "App\\Base"));
        assert!(!beyond("App\\Base&Countable", "Countable&App\\Base"));
    }

    #[test]
    fn templates_keywords_and_non_classes_are_not_beyond_the_hint() {
        assert!(!beyond("T", "App\\Base"));
        assert!(!beyond("static", "App\\Base"));
        assert!(!beyond("$this", "App\\Base"));
        assert!(!beyond("list<App\\User>", "App\\Base"));
        assert!(!beyond("mixed", "App\\Base"));
    }

    #[test]
    fn a_hint_naming_no_class_by_name_is_left_alone() {
        assert!(!beyond("App\\User", "object"));
        assert!(!beyond("App\\Collection<int, App\\User>", "iterable"));
        assert!(!beyond("App\\Sub", "self"));
        assert!(!beyond("App\\Sub", "static|null"));
        assert!(!beyond("App\\Sub", "App\\Base|parent"));
    }
}
