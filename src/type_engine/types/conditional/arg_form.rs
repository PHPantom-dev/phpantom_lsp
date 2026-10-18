//! Deciding a condition from an argument's source text.
//!
//! A literal argument (`'json'`, `0x1`, `null`, `[]`, `fn () => …`) has a
//! fully known type and value, so a condition keyed on it is settled from
//! the text alone, before any type resolution.  The runtime categories
//! (`string`, `int`, `array`, `object`, …) that decide mutually exclusive
//! conditions live here too, shared with the decisions the parent module
//! makes from resolved types.

use crate::php_type::{LiteralValue, PhpType, TypeKind};

/// Checks whether the argument text is a quoted string literal.
fn arg_is_string_literal(arg: &str) -> bool {
    let t = arg.trim();
    (t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"'))
}

/// Checks whether the argument text is an integer literal, in any of the
/// notations PHP accepts (`10`, `-10`, `0x0a`, `0b1010`, `012`, `1_0`).
fn arg_is_int_literal(arg: &str) -> bool {
    crate::php_type::parse_php_int_literal(arg.trim()).is_some()
}

/// Checks whether the argument text is a float literal.
fn arg_is_float_literal(arg: &str) -> bool {
    let t = arg.trim();
    let t = t.strip_prefix('-').unwrap_or(t);
    t.contains('.') && t.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// The syntactic form of an argument's source text, as far as it can be
/// classified without resolving its type. Used to decide `is <Type>`
/// conditions against literal arguments.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgForm {
    StringLit,
    IntLit,
    FloatLit,
    /// The two boolean literals are kept apart so a condition naming one of
    /// them (`$as_number is true`) is decided rather than treated as a plain
    /// `is bool`.
    True,
    False,
    Null,
    ArrayLit,
    /// A closure or arrow function literal, which is always a `Closure`.
    Closure,
    /// Any expression whose type cannot be read from its syntax alone
    /// (variables, property/method chains, function calls, …).
    Unknown,
}

/// Classify the syntactic form of an argument's source text.
fn classify_arg_form(arg: &str) -> ArgForm {
    let t = arg.trim();
    if t.is_empty() {
        return ArgForm::Unknown;
    }
    if arg_is_string_literal(t) {
        return ArgForm::StringLit;
    }
    if arg_is_int_literal(t) {
        return ArgForm::IntLit;
    }
    if arg_is_float_literal(t) {
        return ArgForm::FloatLit;
    }
    if t.eq_ignore_ascii_case("true") {
        return ArgForm::True;
    }
    if t.eq_ignore_ascii_case("false") {
        return ArgForm::False;
    }
    if t.eq_ignore_ascii_case("null") {
        return ArgForm::Null;
    }
    if t.starts_with('[') || t.to_ascii_lowercase().starts_with("array(") {
        return ArgForm::ArrayLit;
    }
    if is_closure_literal(t) {
        return ArgForm::Closure;
    }
    ArgForm::Unknown
}

/// Whether `t` is written as `function (…) …` or `fn (…) => …`, optionally
/// `static` and by-reference.
fn is_closure_literal(t: &str) -> bool {
    let starts_with_keyword = |text: &str, keyword: &str| {
        text.get(..keyword.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(keyword))
            && text[keyword.len()..].trim_start().starts_with(['(', '&'])
    };
    let t = match t.get(..6) {
        Some(head) if head.eq_ignore_ascii_case("static") => t[6..].trim_start(),
        _ => t,
    };
    starts_with_keyword(t, "fn") || starts_with_keyword(t, "function")
}

/// The literal value an argument's source text denotes, when the text is a
/// literal at all (`0`, `-1`, `1.5`, `'json'`).
fn literal_value_from_text(arg_text: &str) -> Option<LiteralValue> {
    let t = arg_text.trim();
    match classify_arg_form(t) {
        ArgForm::StringLit => Some(LiteralValue::string_raw(t)),
        ArgForm::IntLit => Some(LiteralValue::int(t)),
        ArgForm::FloatLit => Some(LiteralValue::float(t)),
        _ => None,
    }
}

/// Decide a conditional's condition against an argument's source text.
///
/// A value condition (`$format is 0`) is settled by comparing literal values,
/// so `0x1` satisfies `is 1` and `"foo"` satisfies `is 'foo'`. A type condition
/// (`$x is string`, `$x is array|string`) is settled by the argument's
/// syntactic form. Returns `None` when the text settles neither, which happens
/// whenever the argument is an expression rather than a literal.
pub(super) fn condition_result_from_text(condition: &PhpType, arg_text: &str) -> Option<bool> {
    match condition.kind() {
        TypeKind::Union(members) => combine_union_results(
            members
                .iter()
                .map(|member| condition_result_from_text(member, arg_text)),
        ),
        TypeKind::Literal(expected) => match literal_value_from_text(arg_text) {
            Some(actual) => Some(crate::php_type::literals_equal(expected, &actual)),
            // A value of another kind entirely cannot be the literal, whatever
            // it holds: `null` is never `'array'`.
            None => match (literal_category(expected), form_category(arg_text)) {
                (lit, Some(form)) if lit != form => Some(false),
                _ => None,
            },
        },
        _ => condition_result_from_form(condition, classify_arg_form(arg_text)),
    }
}

/// Decide a value condition (`$flags is 15`) against an argument's *resolved*
/// type, for the arguments whose source text is not a literal.
///
/// A resolved literal settles the comparison both ways: `PATHINFO_FILENAME`
/// resolves to `8`, which refutes `is 15` as conclusively as the text `8`
/// would have. Anything broader than a literal (`int`, a union, an unresolved
/// expression) leaves the condition undecided.
pub(super) fn literal_condition_result(condition: &PhpType, arg_ty: &PhpType) -> Option<bool> {
    match condition.kind() {
        TypeKind::Union(members) => combine_union_results(
            members
                .iter()
                .map(|member| literal_condition_result(member, arg_ty)),
        ),
        TypeKind::Literal(expected) => match arg_ty.as_literal() {
            Some(actual) => Some(crate::php_type::literals_equal(expected, actual)),
            // Even without a literal value, a type from another category
            // cannot hold one: an `int` argument is never `'array'`.
            None => match type_category(arg_ty) {
                Some(category) if category != literal_category(expected) => Some(false),
                _ => None,
            },
        },
        _ => None,
    }
}

/// The runtime category a literal value belongs to, in the same vocabulary
/// [`type_category`] reports.
fn literal_category(value: &LiteralValue) -> &'static str {
    match value {
        LiteralValue::Int(_) => "int",
        LiteralValue::Float(_) => "float",
        LiteralValue::String(_) => "string",
    }
}

/// The runtime category an argument's source text belongs to, or `None` when
/// its syntax does not say.
fn form_category(arg_text: &str) -> Option<&'static str> {
    match classify_arg_form(arg_text) {
        ArgForm::StringLit => Some("string"),
        ArgForm::IntLit => Some("int"),
        ArgForm::FloatLit => Some("float"),
        ArgForm::True | ArgForm::False => Some("bool"),
        ArgForm::Null => Some("null"),
        ArgForm::ArrayLit => Some("array"),
        ArgForm::Closure => Some("object"),
        ArgForm::Unknown => None,
    }
}

/// Decide a type condition (`$x is string`, `$x is array|string`) from the
/// argument's syntactic form.
///
/// A literal argument has a fully known type, so the answer is conclusive;
/// [`ArgForm::Unknown`] yields `None` for the caller to handle. Union
/// conditions are satisfied when any member is.
fn condition_result_from_form(condition: &PhpType, form: ArgForm) -> Option<bool> {
    if form == ArgForm::Unknown {
        return None;
    }
    match condition.kind() {
        TypeKind::Union(members) => combine_union_results(
            members
                .iter()
                .map(|member| condition_result_from_form(member, form)),
        ),
        _ if form == ArgForm::Closure => closure_matches_condition(condition),
        _ => Some(scalar_condition_matches_form(condition, form)),
    }
}

/// Combine the per-member results of a union condition: it is satisfied when
/// any member is, and refuted only when every member is refuted.
pub(super) fn combine_union_results(results: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    let mut any_true = false;
    let mut all_false = true;
    for result in results {
        match result {
            Some(true) => {
                any_true = true;
                all_false = false;
            }
            Some(false) => {}
            None => all_false = false,
        }
    }
    if any_true {
        Some(true)
    } else if all_false {
        Some(false)
    } else {
        None
    }
}

/// Whether a single (non-union) type condition is satisfied by a literal
/// argument form. A literal always has a fully known type, so this is
/// conclusive (unlike [`ArgForm::Unknown`], which the caller handles).
fn scalar_condition_matches_form(condition: &PhpType, form: ArgForm) -> bool {
    match form {
        ArgForm::StringLit => condition.is_string_type(),
        ArgForm::IntLit => condition.is_int(),
        ArgForm::FloatLit => condition.is_float(),
        // A boolean literal satisfies `is bool` either way, and the matching
        // half of `is true` / `is false`.
        ArgForm::True => condition.is_bool() || condition.is_true(),
        ArgForm::False => condition.is_bool() || condition.is_false(),
        ArgForm::Null => condition.is_null(),
        ArgForm::ArrayLit => condition.is_array_like(),
        ArgForm::Closure | ArgForm::Unknown => false,
    }
}

/// Whether a closure literal satisfies a single (non-union) type condition.
///
/// A class condition other than `Closure` itself stays undecided, and so
/// does a callable signature, which the literal's own signature would have
/// to be checked against.
fn closure_matches_condition(condition: &PhpType) -> Option<bool> {
    if condition.is_mixed()
        || condition.is_object()
        || (condition.is_callable() && !matches!(condition.kind(), TypeKind::Callable(_)))
    {
        Some(true)
    } else if condition.is_true() || condition.is_false() || condition_category(condition).is_some()
    {
        Some(false)
    } else {
        None
    }
}

/// The broad runtime category a resolved type belongs to, used to decide
/// mutually-exclusive `is <scalar>` conditions. Returns `None` when the type
/// cannot be placed in a single category (so the condition stays undecided).
pub(super) fn type_category(t: &PhpType) -> Option<&'static str> {
    if t.is_string_subtype() {
        Some("string")
    } else if t.is_int_subtype() {
        Some("int")
    } else if t.is_float_subtype() {
        Some("float")
    } else if t.is_bool() || t.is_true() || t.is_false() {
        // `true` and `false` are boolean values, not class names — without
        // this they fall through to the class-instance case below.
        Some("bool")
    } else if t.is_null() {
        Some("null")
    } else if t.is_array_like() {
        Some("array")
    } else if matches!(t.kind(), TypeKind::Callable(_)) || t.base_name().is_some() {
        // A closure/callable or any class instance — not a scalar or array.
        Some("object")
    } else {
        None
    }
}

/// The category named by a (non-union) `is <Type>` condition, or `None` when
/// the condition is not a plain scalar/array type we can categorise.
pub(super) fn condition_category(condition: &PhpType) -> Option<&'static str> {
    if condition.is_string_type() {
        Some("string")
    } else if condition.is_int() {
        Some("int")
    } else if condition.is_float() {
        Some("float")
    } else if condition.is_bool() {
        Some("bool")
    } else if condition.is_null() {
        Some("null")
    } else if condition.is_array_like() {
        Some("array")
    } else if condition.is_object() {
        // The one category [`type_category`] reports that has no keyword of
        // its own below: `is object` asks the same question it answers for
        // every class instance, so a scalar argument refutes it outright
        // rather than being handed to the class-hierarchy check, where the
        // bare keyword resolves to no class at all and settles nothing.
        Some("object")
    } else {
        None
    }
}
