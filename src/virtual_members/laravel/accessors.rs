//! Eloquent accessor detection and property name extraction.
//!
//! This module handles both legacy (`getXAttribute()`) and modern
//! (Laravel 9+ `Attribute` cast) accessor patterns, mapping method
//! signatures to virtual property names and types.

use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, MethodInfo};

use super::helpers::camel_to_snake;

/// The fully-qualified name of the `Attribute` cast class used by
/// Laravel 9+ modern accessors/mutators.
const ATTRIBUTE_CAST_FQN: &str = "Illuminate\\Database\\Eloquent\\Casts\\Attribute";

/// Record a modern attribute's setter while its declaration and imports are available.
pub(crate) fn has_attribute_setter(
    return_type: Option<&PhpType>,
    method: &mago_syntax::cst::Method<'_>,
    doc_ctx: Option<&crate::parser::DocblockCtx<'_>>,
) -> bool {
    let Some(return_type) = return_type else {
        return false;
    };
    if !return_type
        .base_name()
        .is_some_and(|name| written_attribute_class(name, doc_ctx))
    {
        return false;
    }
    if let TypeKind::Generic(generic) = return_type.kind()
        && generic.args.get(1).is_some_and(|setter| !setter.is_never())
    {
        return true;
    }
    let mago_syntax::cst::MethodBody::Concrete(body) = &method.body else {
        return false;
    };
    statements_have_setter(body.statements.as_slice(), doc_ctx)
}

fn written_attribute_class(name: &str, doc_ctx: Option<&crate::parser::DocblockCtx<'_>>) -> bool {
    if !name
        .rsplit('\\')
        .next()
        .is_some_and(|short| short.eq_ignore_ascii_case("Attribute"))
        && !doc_ctx.is_some_and(|ctx| {
            crate::names::lookup_import(&ctx.use_map, name)
                .is_some_and(|target| target.eq_ignore_ascii_case(ATTRIBUTE_CAST_FQN))
        })
    {
        return false;
    }
    match doc_ctx {
        Some(ctx) => crate::names::resolve_to_fqn(name, &ctx.use_map, &ctx.namespace)
            .eq_ignore_ascii_case(ATTRIBUTE_CAST_FQN),
        None => name
            .trim_start_matches('\\')
            .eq_ignore_ascii_case(ATTRIBUTE_CAST_FQN),
    }
}

fn statements_have_setter(
    statements: &[mago_syntax::cst::Statement<'_>],
    doc_ctx: Option<&crate::parser::DocblockCtx<'_>>,
) -> bool {
    statements.iter().any(|statement| {
        if let mago_syntax::cst::Statement::Return(returned) = statement {
            return returned
                .value
                .is_some_and(|value| expression_has_setter(value, doc_ctx));
        }
        let mut found = false;
        crate::parser::for_each_nested_statement_list(statement, |nested| {
            found |= statements_have_setter(nested, doc_ctx);
        });
        found
    })
}

fn expression_has_setter(
    expression: &mago_syntax::cst::Expression<'_>,
    doc_ctx: Option<&crate::parser::DocblockCtx<'_>>,
) -> bool {
    use mago_syntax::cst::*;
    let (class, arguments, position) = match expression {
        Expression::Parenthesized(expression) => {
            return expression_has_setter(expression.expression, doc_ctx);
        }
        Expression::Call(Call::Method(call)) => return expression_has_setter(call.object, doc_ctx),
        Expression::Call(Call::StaticMethod(call)) => {
            let ClassLikeMemberSelector::Identifier(method) = &call.method else {
                return false;
            };
            let position = if method.value.eq_ignore_ascii_case(b"make") {
                1
            } else if method.value.eq_ignore_ascii_case(b"set") {
                0
            } else {
                return false;
            };
            (call.class, &call.argument_list, position)
        }
        Expression::Instantiation(new) => {
            let Some(arguments) = &new.argument_list else {
                return false;
            };
            (new.class, arguments, 1)
        }
        _ => return false,
    };
    let Expression::Identifier(class) = class else {
        return false;
    };
    if !written_attribute_class(crate::atom::bytes_to_str(class.value()), doc_ctx) {
        return false;
    }
    arguments
        .arguments
        .iter()
        .enumerate()
        .any(|(index, argument)| {
            let value = match argument {
                Argument::Named(named) if named.name.value == b"set" => named.value,
                Argument::Positional(positional)
                    if index == position && positional.ellipsis.is_none() =>
                {
                    positional.value
                }
                _ => return false,
            };
            !matches!(value, Expression::Literal(Literal::Null(_)))
        })
}

/// Determine whether a method is a legacy Eloquent accessor.
///
/// Legacy accessors follow the `getXAttribute()` naming convention where
/// `X` starts with an uppercase letter.  For example,
/// `getFullNameAttribute()` is a legacy accessor that produces a virtual
/// property `$full_name`.
pub(super) fn is_legacy_accessor(method: &MethodInfo) -> bool {
    let name = &method.name;
    if !name.starts_with("get") || !name.ends_with("Attribute") {
        return false;
    }
    // Must have at least one character between "get" and "Attribute".
    // "getAttribute" itself (len 12) is a real Eloquent method, not an accessor.
    let middle = &name[3..name.len() - 9]; // strip "get" (3) and "Attribute" (9)
    if middle.is_empty() {
        return false;
    }
    middle.starts_with(|c: char| c.is_uppercase())
}

pub(super) fn is_legacy_mutator(method: &MethodInfo) -> bool {
    let name = &method.name;
    if !name.starts_with("set") || !name.ends_with("Attribute") {
        return false;
    }
    let middle = &name[3..name.len() - 9];
    !middle.is_empty() && middle.starts_with(|c: char| c.is_uppercase())
}

/// Extract the virtual property name from a legacy accessor method name.
///
/// Strips `get` prefix and `Attribute` suffix, then converts the
/// remaining CamelCase portion to snake_case.
///
/// `getFullNameAttribute` → `full_name`
/// `getNameAttribute` → `name`
pub(super) fn legacy_accessor_property_name(method_name: &str) -> String {
    let middle = &method_name[3..method_name.len() - 9];
    camel_to_snake(middle)
}

pub(super) fn legacy_mutator_property_name(method_name: &str) -> String {
    let middle = &method_name[3..method_name.len() - 9];
    camel_to_snake(middle)
}

/// Determine whether a method is a modern Eloquent accessor (Laravel 9+).
///
/// Modern accessors are methods that return
/// `Illuminate\Database\Eloquent\Casts\Attribute`.  The method name
/// is in camelCase and the virtual property name is the snake_case
/// equivalent.  For example, `fullName(): Attribute` produces
/// `$full_name`.
pub(super) fn is_modern_accessor(method: &MethodInfo) -> bool {
    match method.return_type.as_ref() {
        Some(rt) => {
            if let Some(base) = rt.base_name() {
                base.eq_ignore_ascii_case(ATTRIBUTE_CAST_FQN)
            } else {
                false
            }
        }
        None => false,
    }
}

/// Extract the get-type from a modern accessor's return type.
///
/// Given a return type like `Attribute<string>` or
/// `Attribute<string, never>`, returns the first generic argument
/// (`string` in both examples).  Falls back to `PhpType::mixed()` when
/// no generic parameter is present.
pub(super) fn extract_modern_accessor_type(method: &MethodInfo) -> PhpType {
    if let Some(rt) = method.return_type.as_ref()
        && let TypeKind::Generic(g) = rt.kind()
        && let Some(first) = g.args.first()
        && !matches!(first.kind(), TypeKind::Named(s) if s.is_empty())
        && !matches!(first.kind(), TypeKind::Raw(s) if s.is_empty())
    {
        return first.clone();
    }
    PhpType::mixed()
}

/// Check whether a method on `class` with the given `method_name` is
/// actually a legacy or modern Eloquent accessor.
///
/// Go-to-definition uses this after `accessor_method_candidates` finds a
/// matching method, to avoid jumping to a relationship or other method
/// that happens to share the camelCase name.  For example,
/// `masterRecipe()` returning `BelongsToMany` is NOT an accessor even
/// though `snake_to_camel("master_recipe")` produces `"masterRecipe"`.
pub(crate) fn is_accessor_or_mutator_method(class: &ClassInfo, method_name: &str) -> bool {
    let method = match class.get_method(method_name) {
        Some(m) => m,
        None => return false,
    };
    is_legacy_accessor(method) || is_legacy_mutator(method) || is_modern_accessor(method)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "accessors_tests.rs"]
mod tests;
