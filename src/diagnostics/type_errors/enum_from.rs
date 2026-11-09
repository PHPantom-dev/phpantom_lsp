//! `BackedEnum::from()` and `tryFrom()` called with a value no case of the
//! enum is backed by.
//!
//! The argument's resolved type decides it: when every value the argument
//! can hold is a literal and none of them matches a case, `from()` throws a
//! `ValueError` and `tryFrom()` returns `null`.

use tower_lsp_server::gen_lsp_types::*;

use crate::class_lookup::Fqn;
use crate::php_type::{LiteralValue, PhpType, TypeKind};
use crate::symbol_map::CallSite;
use crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value;
use crate::types::{BackedEnumType, ClassLikeKind};

use super::super::helpers::make_diagnostic;

/// Diagnostic code for a `from()`/`tryFrom()` value no enum case backs.
pub(crate) const UNKNOWN_ENUM_VALUE_CODE: &str = "unknown_enum_value";

/// A backing value, as the enum compares it.
#[derive(PartialEq)]
enum BackingValue<'a> {
    Int(i64),
    String(std::borrow::Cow<'a, str>),
}

/// The value `literal` turns into when passed to the `from()` of an enum
/// backed by `backing`, or `None` when it is not one PHP accepts there
/// (a float, a non-numeric string for an `int` enum, or any value of the
/// other scalar type under `strict_types`).
fn coerce(
    literal: &LiteralValue,
    backing: BackedEnumType,
    strict_types: bool,
) -> Option<BackingValue<'_>> {
    match (backing, literal) {
        (BackedEnumType::Int, LiteralValue::Int(_)) => literal.parse_i64().map(BackingValue::Int),
        (BackedEnumType::String, LiteralValue::String(_)) => {
            literal.string_content().map(BackingValue::String)
        }
        (_, LiteralValue::Float(_)) => None,
        _ if strict_types => None,
        (BackedEnumType::String, LiteralValue::Int(_)) => literal
            .parse_i64()
            .map(|n| BackingValue::String(n.to_string().into())),
        (BackedEnumType::Int, LiteralValue::String(_)) => match literal.numeric_string_value()? {
            int @ LiteralValue::Int(_) => int.parse_i64().map(BackingValue::Int),
            _ => None,
        },
    }
}

/// The literal values `ty` can hold, or `None` when it can hold anything
/// that is not a literal.
fn literal_members(ty: &PhpType) -> Option<Vec<&LiteralValue>> {
    match ty.kind() {
        TypeKind::Literal(value) => Some(vec![value]),
        TypeKind::Union(members) => members.iter().map(PhpType::as_literal).collect(),
        _ => None,
    }
}

impl super::arguments::ArgumentCheck<'_> {
    /// Report the value argument of a `from()`/`tryFrom()` call on a
    /// backed enum when no case is backed by any value it can hold.
    pub(super) fn check_enum_from(
        &self,
        call_site: &CallSite,
        owner: Option<&str>,
        value: Option<(&PhpType, usize, usize)>,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some((_, method)) = call_site.call_expression.rsplit_once("::") else {
            return;
        };
        let (method, throws) = if method.eq_ignore_ascii_case("from") {
            ("from", true)
        } else if method.eq_ignore_ascii_case("tryFrom") {
            ("tryFrom", false)
        } else {
            return;
        };
        let (Some(owner), Some((arg_type, start, end))) = (owner, value) else {
            return;
        };
        let Some(enum_info) = (self.class_loader)(Fqn::new(owner)) else {
            return;
        };
        if enum_info.kind != ClassLikeKind::Enum {
            return;
        }
        let Some(backing) = enum_info.backed_type else {
            return;
        };

        let Some(passed) = literal_members(arg_type) else {
            return;
        };
        let Some(passed) = passed
            .into_iter()
            .map(|literal| coerce(literal, backing, self.strict_types))
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };

        // A case whose value is not a plain literal (a constant
        // expression) could be backed by anything, so nothing is known.
        let case_literals = enum_info
            .constants
            .iter()
            .filter(|c| c.is_enum_case)
            .map(|c| {
                c.enum_value
                    .as_deref()
                    .and_then(infer_type_from_constant_value)
            })
            .collect::<Option<Vec<PhpType>>>();
        let Some(case_literals) = case_literals else {
            return;
        };
        let Some(case_values) = case_literals
            .iter()
            .map(|ty| {
                ty.as_literal()
                    .and_then(|literal| coerce(literal, backing, true))
            })
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };

        if passed.iter().any(|value| case_values.contains(value)) {
            return;
        }

        let Some(range) = self.range(start, end) else {
            return;
        };
        let enum_name = enum_info.fqn();
        let (severity, consequence) = if throws {
            (DiagnosticSeverity::Error, "throws a ValueError")
        } else {
            (DiagnosticSeverity::Warning, "always returns null")
        };
        out.push(make_diagnostic(
            range,
            severity,
            UNKNOWN_ENUM_VALUE_CODE,
            format!(
                "No case of enum {enum_name} is backed by {arg_type}, so {enum_name}::{method}() {consequence}"
            ),
        ));
    }
}
