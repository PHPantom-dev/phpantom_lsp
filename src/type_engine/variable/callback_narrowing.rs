//! What a filter callback proves about the value it is handed.
//!
//! `array_filter($data, fn ($k) => is_string($k), ARRAY_FILTER_USE_KEY)`
//! keeps exactly the entries whose key passed `is_string()`, so the
//! callback body is as much a proof about the result's key type as an
//! `if (is_string($k))` body is about the variable it guards. Both
//! spellings of a call's arguments (a parsed argument list and raw source
//! text) reach the same analysis here, so an inline call and one assigned
//! to a variable answer alike.

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;

use crate::atom::{bytes_to_str, literal_bytes_to_str};
use crate::php_type::PhpType;
use crate::type_engine::resolver::WrittenNames;
use crate::type_engine::types::narrowing::{
    GuardClassLoader, narrow_type_by_condition, narrow_type_by_guard_name,
};

/// Narrow `subject` to the values that make the callback's parameter at
/// `param_index` pass.
///
/// Returns `None` when the argument is neither an inline function nor a
/// named type guard, declares no such parameter, or asserts nothing about
/// it.
pub(in crate::type_engine) fn narrow_callback_param(
    callback: &Expression<'_>,
    param_index: usize,
    subject: &PhpType,
    class_loader: GuardClassLoader<'_>,
    names: &WrittenNames<'_>,
) -> Option<PhpType> {
    // `array_filter($a, 'is_string', ARRAY_FILTER_USE_KEY)` asserts
    // exactly what the guard it names does, about the single argument it
    // is handed — so it describes the leading parameter and no other.
    if let Expression::Literal(Literal::String(name)) = callback {
        if param_index != 0 {
            return None;
        }
        let name = name.value.and_then(literal_bytes_to_str)?;
        return narrow_type_by_guard_name(name, subject, class_loader);
    }

    let (param_name, body) = callback_param_and_body(callback, param_index)?;
    narrow_type_by_condition(body, &param_name, subject, class_loader, names)
}

/// [`narrow_callback_param`] over a callback written as source text.
pub(in crate::type_engine) fn narrow_callback_param_text(
    callback: &str,
    param_index: usize,
    subject: &PhpType,
    class_loader: GuardClassLoader<'_>,
    names: &WrittenNames<'_>,
) -> Option<PhpType> {
    let trimmed = callback.trim();
    // Only an inline function or a named guard says anything; anything
    // else would be parsed just to be rejected.
    if !(trimmed.starts_with("fn")
        || trimmed.starts_with("function")
        || trimmed.starts_with("static")
        || trimmed.starts_with(['\'', '"']))
    {
        return None;
    }
    let arena = LocalArena::new();
    let source = format!("<?php {trimmed};");
    let program = parse_file_content(&arena, FileId::new(b"callback.php"), source.as_bytes());
    let expr = program.statements.iter().find_map(|stmt| match stmt {
        Statement::Expression(stmt) => Some(stmt.expression),
        _ => None,
    })?;
    narrow_callback_param(expr, param_index, subject, class_loader, names)
}

/// The name of the callback's `param_index`th parameter and the
/// expression whose truth decides what the callback keeps.
///
/// A closure body is read only when a single `return` is the whole of it.
/// With any other shape an earlier path may admit a value the `return`
/// expression rejects, and the assertion no longer holds for everything
/// the filter kept.
fn callback_param_and_body<'ast>(
    callback: &Expression<'ast>,
    param_index: usize,
) -> Option<(String, &'ast Expression<'ast>)> {
    match callback {
        Expression::ArrowFunction(arrow) => {
            let param = arrow.parameter_list.parameters.get(param_index)?;
            Some((
                bytes_to_str(param.variable.name).to_string(),
                arrow.expression,
            ))
        }
        Expression::Closure(closure) => {
            let param = closure.parameter_list.parameters.get(param_index)?;
            let mut statements = closure.body.statements.iter();
            let Some(Statement::Return(ret)) = statements.next() else {
                return None;
            };
            if statements.next().is_some() {
                return None;
            }
            Some((
                bytes_to_str(param.variable.name).to_string(),
                *ret.value.as_ref()?,
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `string|int` an `array<string|int, …>` key parameter starts from.
    fn array_key() -> PhpType {
        PhpType::union(vec![PhpType::string(), PhpType::int()])
    }

    fn narrow(callback: &str, param_index: usize) -> Option<String> {
        narrow_callback_param_text(
            callback,
            param_index,
            &array_key(),
            None,
            &WrittenNames::verbatim(),
        )
        .map(|ty| ty.to_string())
    }

    #[test]
    fn an_arrow_function_body_narrows_the_parameter_it_tests() {
        assert_eq!(
            narrow("fn ($k) => is_string($k)", 0).as_deref(),
            Some("string")
        );
        assert_eq!(
            narrow("fn ($k) => !is_string($k)", 0).as_deref(),
            Some("int")
        );
        assert_eq!(
            narrow("fn ($k) => is_int($k) && $k > 2", 0).as_deref(),
            Some("int")
        );
        // `ARRAY_FILTER_USE_BOTH` hands the value first, so the key is the
        // second parameter and the first says nothing about it.
        assert_eq!(
            narrow("fn ($v, $k) => is_string($k)", 1).as_deref(),
            Some("string")
        );
        assert_eq!(narrow("fn ($v, $k) => is_string($k)", 0), None);
        assert_eq!(narrow("fn ($k) => $k !== ''", 0), None);
    }

    #[test]
    fn a_closure_body_is_read_only_when_a_single_return_is_all_of_it() {
        assert_eq!(
            narrow("function ($k) { return is_string($k); }", 0).as_deref(),
            Some("string")
        );
        assert_eq!(
            narrow(
                "function ($k) { if ($k === 0) { return true; } return is_string($k); }",
                0
            ),
            None
        );
    }

    #[test]
    fn a_callback_that_admits_every_key_narrows_nothing() {
        // Each operand narrows on its own, but between them they let the
        // whole key domain through.
        assert_eq!(
            narrow("fn ($k) => is_string($k) || is_int($k)", 0).as_deref(),
            Some("string|int")
        );
    }

    /// The value half of a filter is most often tested against `null`
    /// directly rather than through `is_null()`, and both spellings prove
    /// the same thing. The loose `== null` is the exception: it is also
    /// true for `''` and `0`, so it cannot report `null`.
    #[test]
    fn a_null_comparison_narrows_the_parameter_it_tests() {
        let nullable = PhpType::nullable(PhpType::string());
        let narrow = |callback: &str| {
            narrow_callback_param_text(callback, 0, &nullable, None, &WrittenNames::verbatim())
                .map(|ty| ty.to_string())
        };
        assert_eq!(narrow("fn ($v) => $v !== null").as_deref(), Some("string"));
        assert_eq!(narrow("fn ($v) => $v != null").as_deref(), Some("string"));
        assert_eq!(narrow("fn ($v) => null !== $v").as_deref(), Some("string"));
        assert_eq!(narrow("fn ($v) => $v === null").as_deref(), Some("null"));
        assert_eq!(narrow("fn ($v) => !($v !== null)").as_deref(), Some("null"));
        assert_eq!(narrow("fn ($v) => $v == null"), None);
        assert_eq!(narrow("fn ($v) => $v !== ''"), None);
    }

    #[test]
    fn a_named_guard_narrows_the_argument_it_is_handed() {
        assert_eq!(narrow("'is_string'", 0).as_deref(), Some("string"));
        assert_eq!(narrow("'is_int'", 0).as_deref(), Some("int"));
        // A one-argument guard only ever sees the value in
        // `ARRAY_FILTER_USE_BOTH` mode, so it says nothing about the key.
        assert_eq!(narrow("'is_string'", 1), None);
        assert_eq!(narrow("'trim'", 0), None);
    }

    #[test]
    fn a_callback_that_is_neither_is_declined() {
        assert_eq!(narrow("$callback", 0), None);
        assert_eq!(narrow("[$this, 'accept']", 0), None);
        assert_eq!(narrow("self::accept(...)", 0), None);
    }
}
