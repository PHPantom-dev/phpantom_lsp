//! `get_parent_class()`: the parent of the class its argument names, or of
//! the class the call is written in when it has none. Mirrors PHPStan's
//! `GetParentClassDynamicFunctionReturnTypeExtension`.

use crate::atom::atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{LiteralValue, PhpType};
use crate::types::{ClassInfo, ClassLikeKind};

use super::CallArgs;

pub(super) fn return_type(
    args: &CallArgs<'_>,
    class_loader: Option<&ResolvedClassLoader<'_>>,
) -> Option<PhpType> {
    let class_loader = class_loader?;
    let Some(text) = args.text("$object_or_class") else {
        // The class the call is written in. A trait is only known once it
        // is used, so it may have any parent; outside a class the call
        // returns `false`.
        let class = args
            .resolve("self::class")
            .and_then(|scope| class_loader(Fqn::new(scope.as_class_name_literal()?)))
            .filter(|class| class.kind != ClassLikeKind::Trait);
        return Some(class.map_or_else(unknown, |class| parent_of(&class)));
    };
    let subject = args.resolve(&text)?;
    let mut results: Vec<PhpType> = Vec::new();
    for member in subject.strip_benevolence().union_members() {
        let ty = member_parent(member, class_loader);
        if !results.contains(&ty) {
            results.push(ty);
        }
    }
    Some(PhpType::union(results))
}

/// The parent of the class one member of the argument's type names.
fn member_parent(member: &PhpType, class_loader: &ResolvedClassLoader<'_>) -> PhpType {
    let named = member
        .as_class_name_literal()
        .map(str::to_owned)
        .or_else(|| match member.as_literal()? {
            literal @ LiteralValue::String(_) => Some(
                literal
                    .string_content()?
                    .trim_start_matches('\\')
                    .to_owned(),
            ),
            _ => None,
        });
    if let Some(name) = named {
        // A class that is not loaded here may still be autoloaded at runtime.
        return class_loader(Fqn::new(&name)).map_or_else(unknown, |class| parent_of(&class));
    }
    if let Some(name) = member.class_name()
        && !member.is_self_ref()
        && let Some(class) = class_loader(Fqn::new(name))
        && class.kind != ClassLikeKind::Trait
    {
        let exact = parent_of(&class);
        if class.is_final || class.kind == ClassLikeKind::Enum {
            return exact;
        }
        // An instance of a subclass has the declared class, or one below
        // it, as its parent.
        let below = PhpType::class_string(Some(PhpType::named(class.fqn())));
        return PhpType::union(vec![below, exact]);
    }
    if member.is_never() {
        return PhpType::never();
    }
    unknown()
}

/// The exact answer for `class`: its parent's name, or `false` without one.
fn parent_of(class: &ClassInfo) -> PhpType {
    match &class.parent_class {
        Some(parent) => PhpType::class_name_literal(atom(parent.trim_start_matches('\\'))),
        None => PhpType::false_(),
    }
}

fn unknown() -> PhpType {
    PhpType::union(vec![PhpType::class_string(None), PhpType::false_()])
}
