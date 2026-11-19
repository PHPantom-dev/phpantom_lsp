//! Argument type mismatch diagnostics.
//!
//! Walk the precomputed [`CallSite`] entries in the symbol map and flag
//! every call where an argument's resolved type is incompatible with
//! the declared parameter type (see [`arguments`]).
//!
//! This is Phase 1 of the type error diagnostic suite. Only clearly
//! incompatible types are flagged — when in doubt (unresolved types,
//! `mixed`, complex generics), the diagnostic is suppressed to avoid
//! false positives.
//!
//! [`CallSite`]: crate::symbol_map::CallSite

mod arguments;
mod enum_from;

pub(crate) use arguments::{PassedArgument, PassedCall};

use mago_syntax::cst::Program;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::statement::Statement;

use crate::atom::bytes_to_str;
use crate::class_lookup::ResolvedClassLoader;

/// Diagnostic code used for argument type mismatch diagnostics.
pub(crate) const TYPE_MISMATCH_ARGUMENT_CODE: &str = "type_mismatch_argument";

/// Returns `true` when the file declares `strict_types=1`.
///
/// Scans the top-level statements of the parsed program for a
/// `declare(strict_types=1)` directive.  In PHP this must appear as
/// the very first statement (after `<?php`), but we check all
/// top-level statements for robustness.
pub(super) fn has_strict_types(program: &Program<'_>) -> bool {
    for stmt in program.statements.iter() {
        if let Statement::Declare(declare) = stmt {
            for item in declare.items.iter() {
                if bytes_to_str(item.name.value).eq_ignore_ascii_case("strict_types")
                    && let Expression::Literal(Literal::Integer(i)) = item.value
                    && bytes_to_str(i.raw) == "1"
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether a `value` that does not fit `declared` only fails on being `null`
/// or `false`: every other member fits, and there is at least one.
///
/// The code is not wrong, only wrong if the value turns out empty, so these
/// mismatches are only reported when `possibly-empty-mismatches` asks for them.
pub(super) fn fails_only_on_empty(
    value: &crate::php_type::PhpType,
    declared: &crate::php_type::PhpType,
    class_loader: &ResolvedClassLoader<'_>,
    strict_types: bool,
) -> bool {
    use crate::php_type::TypeKind;
    let fits = |ty: &crate::php_type::PhpType| {
        is_type_compatible(ty, declared, class_loader, strict_types)
    };
    match value.kind() {
        TypeKind::Nullable(inner) => fits(inner),
        TypeKind::Union(members) => {
            let mut others = members
                .iter()
                .filter(|member| !member.is_null() && !member.is_false())
                .peekable();
            others.peek().is_some() && others.all(fits)
        }
        _ => false,
    }
}

/// Whether a `value` that does not fit `declared` is a union with at least
/// one member that does, such as an `array|string` passed to a `string`
/// parameter.
///
/// The code fails only when the value turns out to be one of the other
/// members, so `partial-union-mismatches = false` leaves these unreported,
/// the way PHPStan below level 7 accepts a union it judges "maybe".
pub(super) fn fits_partially(
    value: &crate::php_type::PhpType,
    declared: &crate::php_type::PhpType,
    class_loader: &ResolvedClassLoader<'_>,
    strict_types: bool,
) -> bool {
    use crate::php_type::{PhpType, TypeKind};
    let fits = |ty: &PhpType| is_type_compatible(ty, declared, class_loader, strict_types);
    match value.kind() {
        TypeKind::Nullable(inner) => fits(inner) || fits(&PhpType::null()),
        TypeKind::Union(members) => members.iter().any(fits),
        _ => false,
    }
}

/// Whether `arg_type` can be passed where `param_type` is declared, with
/// the Laravel model-type operators (`builder-of<Model>`, …) on either side
/// expanded to the classes they name first.
///
/// See [`crate::class_lookup::is_type_compatible`].
pub(super) fn is_type_compatible(
    arg_type: &crate::php_type::PhpType,
    param_type: &crate::php_type::PhpType,
    class_loader: &ResolvedClassLoader<'_>,
    strict_types: bool,
) -> bool {
    use crate::virtual_members::laravel::{expand_model_type, has_model_type_operator};
    if has_model_type_operator(arg_type) || has_model_type_operator(param_type) {
        return crate::class_lookup::is_type_compatible(
            &expand_model_type(arg_type, class_loader),
            &expand_model_type(param_type, class_loader),
            class_loader,
            strict_types,
        );
    }
    crate::class_lookup::is_type_compatible(arg_type, param_type, class_loader, strict_types)
}
