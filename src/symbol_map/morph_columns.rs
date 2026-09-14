//! Syntactic candidates for aliases compared with a model's morph type column.
//!
//! Extraction only retains source ranges and literal names. The shared type
//! engine later confirms the receiver and the column before exposing a symbol.

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{Atom, atom, bytes_to_str};

/// The receiver whose model determines whether a column stores morph aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MorphColumnReceiver {
    /// An instance property read in a PHP comparison.
    Model,
    /// An instance query-method receiver.
    Query,
    /// The class expression of a static query-method call.
    StaticQuery,
}

/// An unresolved literal that may name an alias in a morph-column comparison.
#[derive(Debug, Clone)]
pub(crate) struct MorphColumnSite {
    /// First byte of the alias, excluding its opening quote.
    pub start: u32,
    /// Byte following the alias, excluding its closing quote.
    pub end: u32,
    /// Literal alias, including an empty value while completing a string.
    pub key: String,
    /// Literal property or column name, preserving any table qualification.
    pub column: Atom,
    /// First byte of the receiver expression.
    pub receiver_start: u32,
    /// Byte following the receiver expression.
    pub receiver_end: u32,
    /// Whether the receiver is a model instance, query, or static class.
    pub receiver_kind: MorphColumnReceiver,
}

impl MorphColumnSite {
    /// The alias span this site contributes once its model and column are confirmed.
    pub(crate) fn to_span(&self) -> super::SymbolSpan {
        super::SymbolSpan {
            start: self.start,
            end: self.end,
            kind: super::SymbolKind::LaravelStringKey {
                key: self.key.clone(),
                kind: super::LaravelStringKind::MorphAlias,
                is_write: false,
                is_optional: false,
            },
        }
    }
}

/// Record literal values of supported scalar and set-membership query clauses.
pub(crate) fn record_morph_column_call(
    call: &Call<'_>,
    content: &str,
    out: &mut Vec<MorphColumnSite>,
) {
    let (receiver, method, kind) = match call {
        Call::Method(call) => (call.object, &call.method, MorphColumnReceiver::Query),
        Call::NullSafeMethod(call) => (call.object, &call.method, MorphColumnReceiver::Query),
        Call::StaticMethod(call) => (call.class, &call.method, MorphColumnReceiver::StaticQuery),
        Call::Function(_) => return,
    };
    let ClassLikeMemberSelector::Identifier(method) = method else {
        return;
    };
    let method = bytes_to_str(method.value);
    let (parameters, is_array): (&[&str], bool) = if method.eq_ignore_ascii_case("where")
        || method.eq_ignore_ascii_case("whereNot")
    {
        (&["column", "operator", "value", "boolean"], false)
    } else if method.eq_ignore_ascii_case("orWhere") || method.eq_ignore_ascii_case("orWhereNot") {
        (&["column", "operator", "value"], false)
    } else if method.eq_ignore_ascii_case("whereIn") {
        (&["column", "values", "boolean", "not"], true)
    } else if method.eq_ignore_ascii_case("whereNotIn") {
        (&["column", "values", "boolean"], true)
    } else if method.eq_ignore_ascii_case("orWhereIn")
        || method.eq_ignore_ascii_case("orWhereNotIn")
    {
        (&["column", "values"], true)
    } else {
        return;
    };
    let arguments = call.get_argument_list();
    let Some(bound) = bind_arguments(arguments, parameters) else {
        return;
    };
    let Some(column) = bound[0].and_then(|column| literal_text(column, content)) else {
        return;
    };
    if column.is_empty() {
        return;
    }
    let Some(second) = bound[1] else {
        return;
    };
    if is_array {
        let elements = match unparenthesized(second) {
            Expression::Array(array) => &array.elements,
            Expression::LegacyArray(array) => &array.elements,
            _ => return,
        };
        for element in elements.iter() {
            let value = match element {
                ArrayElement::Value(value) => value.value,
                ArrayElement::KeyValue(value) => value.value,
                _ => continue,
            };
            push_site(value, column, receiver, kind, content, out);
        }
    } else if let Some(value) = bound[2] {
        if matches!(literal_text(second, content), Some("=" | "!=" | "<>")) {
            push_site(value, column, receiver, kind, content, out);
        }
    } else if arguments.arguments.len() == 2 {
        // The negated wrappers forward four arguments to `where()`, so a
        // recognized operator with no value compares NULL or throws instead
        // of becoming the value as it does in two-argument `where()`.
        if (method.eq_ignore_ascii_case("whereNot") || method.eq_ignore_ascii_case("orWhereNot"))
            && literal_text(second, content).is_some_and(is_laravel_query_operator)
        {
            return;
        }
        push_site(second, column, receiver, kind, content, out);
    }
}

/// Operators recognized by Laravel's query builder independently of the
/// database grammar. Matching is case-insensitive, like `invalidOperator()`.
fn is_laravel_query_operator(value: &str) -> bool {
    [
        "=",
        "<",
        ">",
        "<=",
        ">=",
        "<>",
        "!=",
        "<=>",
        "like",
        "like binary",
        "not like",
        "ilike",
        "&",
        "|",
        "^",
        "<<",
        ">>",
        "&~",
        "is",
        "is not",
        "rlike",
        "not rlike",
        "regexp",
        "not regexp",
        "~",
        "~*",
        "!~",
        "!~*",
        "similar to",
        "not similar to",
        "not ilike",
        "~~*",
        "!~~*",
    ]
    .iter()
    .any(|operator| value.eq_ignore_ascii_case(operator))
}

/// Record an alias on either side of a direct or nullsafe property comparison.
pub(crate) fn record_morph_column_comparison(
    binary: &Binary<'_>,
    content: &str,
    out: &mut Vec<MorphColumnSite>,
) {
    if !matches!(
        binary.operator,
        BinaryOperator::Equal(_)
            | BinaryOperator::NotEqual(_)
            | BinaryOperator::Identical(_)
            | BinaryOperator::NotIdentical(_)
            | BinaryOperator::AngledNotEqual(_)
    ) {
        return;
    }
    let (property, value) = match (unparenthesized(binary.lhs), unparenthesized(binary.rhs)) {
        (Expression::Access(property), value) | (value, Expression::Access(property)) => {
            (property, value)
        }
        _ => return,
    };
    let (receiver, property) = match property {
        Access::Property(property) => (property.object, &property.property),
        Access::NullSafeProperty(property) => (property.object, &property.property),
        _ => return,
    };
    let ClassLikeMemberSelector::Identifier(property) = property else {
        return;
    };
    push_site(
        value,
        bytes_to_str(property.value),
        receiver,
        MorphColumnReceiver::Model,
        content,
        out,
    );
}

/// Bind the small, fixed query signatures without allocating argument maps.
fn bind_arguments<'ast, 'arena>(
    arguments: &'ast ArgumentList<'arena>,
    parameters: &[&str],
) -> Option<[Option<&'ast Expression<'arena>>; 4]> {
    if arguments.arguments.len() > parameters.len() {
        return None;
    }
    let mut bound = [None; 4];
    let mut has_named = false;
    for (position, argument) in arguments.arguments.iter().enumerate() {
        let index = match argument {
            Argument::Positional(argument) => {
                if has_named || argument.ellipsis.is_some() {
                    return None;
                }
                position
            }
            Argument::Named(argument) => {
                has_named = true;
                let name = bytes_to_str(argument.name.value);
                parameters.iter().position(|parameter| *parameter == name)?
            }
        };
        if bound[index].replace(argument.value()).is_some() {
            return None;
        }
    }
    Some(bound)
}

fn unparenthesized<'ast, 'arena>(
    mut expression: &'ast Expression<'arena>,
) -> &'ast Expression<'arena> {
    while let Expression::Parenthesized(parenthesized) = expression {
        expression = parenthesized.expression;
    }
    expression
}

fn literal_text<'content>(
    expression: &Expression<'_>,
    content: &'content str,
) -> Option<&'content str> {
    let (start, end) = literal_range(expression)?;
    content.get(start as usize..end as usize)
}

fn literal_range(expression: &Expression<'_>) -> Option<(u32, u32)> {
    let Expression::Literal(literal::Literal::String(string)) = unparenthesized(expression) else {
        return None;
    };
    let quote_offset = if matches!(string.raw.first(), Some(b'b' | b'B')) {
        2
    } else {
        1
    };
    Some((
        string.span.start.offset + quote_offset,
        string.span.end.offset - 1,
    ))
}

fn push_site(
    value: &Expression<'_>,
    column: &str,
    receiver: &Expression<'_>,
    receiver_kind: MorphColumnReceiver,
    content: &str,
    out: &mut Vec<MorphColumnSite>,
) {
    let Some((start, end)) = literal_range(value) else {
        return;
    };
    let Some(key) = content.get(start as usize..end as usize) else {
        return;
    };
    // Class-name strings retain the existing morph APIs' semantics and do
    // not name aliases. Escaped strings are also opaque.
    if key.contains('\\') {
        return;
    }
    out.push(MorphColumnSite {
        start,
        end,
        key: key.to_owned(),
        column: atom(column),
        receiver_start: receiver.span().start.offset,
        receiver_end: receiver.span().end.offset,
        receiver_kind,
    });
}

#[cfg(test)]
#[path = "morph_columns_tests.rs"]
mod tests;
