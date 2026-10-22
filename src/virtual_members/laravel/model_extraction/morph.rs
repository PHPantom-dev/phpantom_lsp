use mago_span::HasSpan;

use crate::atom::{Atom, atom, bytes_to_str, literal_bytes_to_str};

use super::*;

/// Extract the type columns declared by returned `$this->morphTo()` calls.
///
/// Only the returned receiver chain counts: a call inside an argument,
/// closure, or unrelated statement does not declare a relationship.
pub(crate) fn extract_morph_type_columns<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Vec<(Atom, Option<Atom>)> {
    let mut columns = Vec::new();
    let mut returns = Vec::new();
    for member in members {
        let class_like::member::ClassLikeMember::Method(method) = member else {
            continue;
        };
        let class_like::method::MethodBody::Concrete(block) = &method.body else {
            continue;
        };
        let span = block.span();
        let Some(body) = content.get(span.start.offset as usize..span.end.offset as usize) else {
            continue;
        };
        if !memchr::memchr2_iter(b'm', b'M', body.as_bytes()).any(|offset| {
            body.as_bytes()
                .get(offset..offset + 7)
                .is_some_and(|name| name.eq_ignore_ascii_case(b"morphTo"))
        }) {
            continue;
        }

        returns.clear();
        crate::return_collection::collect_returns(block.statements.iter(), &mut returns);
        let name = bytes_to_str(method.name.value);
        let mut column = None;
        let mut consistent = !returns.is_empty();
        for (value, _, _, _) in &returns {
            let current = value.and_then(morph_to_column);
            if current.is_none() || column.is_some_and(|column| Some(column) != current) {
                consistent = false;
                break;
            }
            column = current;
        }
        if consistent && let Some(column) = column {
            columns.push((atom(name), column));
        }
    }
    columns
}

fn morph_to_column(mut value: &Expression<'_>) -> Option<Option<Atom>> {
    loop {
        match value {
            Expression::Parenthesized(parenthesized) => value = parenthesized.expression,
            Expression::Call(Call::Method(call)) => {
                if let ClassLikeMemberSelector::Identifier(name) = &call.method
                    && name.value.eq_ignore_ascii_case(b"morphTo")
                    && matches!(
                        call.object,
                        Expression::Variable(Variable::Direct(variable))
                            if variable.name == b"$this"
                    )
                {
                    return morph_to_arguments_column(&call.argument_list);
                }
                value = call.object;
            }
            _ => return None,
        }
    }
}

fn morph_to_arguments_column(arguments: &ArgumentList<'_>) -> Option<Option<Atom>> {
    let mut name = None;
    let mut column = None;
    for (position, argument) in arguments.arguments.iter().enumerate() {
        if argument.is_unpacked() {
            return None;
        }
        match argument {
            Argument::Named(argument) => match argument.name.value {
                b"name" => name = Some(argument.value),
                b"type" => column = Some(argument.value),
                _ => {}
            },
            Argument::Positional(argument) => match position {
                0 => name = Some(argument.value),
                1 => column = Some(argument.value),
                _ => {}
            },
        }
    }

    if let Some(column) = morph_argument_string(column)? {
        return Some(Some(atom(column)));
    }
    let Some(name) = morph_argument_string(name)? else {
        return Some(None);
    };
    let mut column = String::with_capacity(name.len() + 5);
    let mut word_start = true;
    for character in name.chars() {
        if character.is_whitespace() {
            word_start = word_start || character.is_ascii_whitespace();
            continue;
        }
        let character = if word_start {
            character.to_ascii_uppercase()
        } else {
            character
        };
        // Laravel's Str::snake separates every ASCII capital, including
        // acronyms (URLTarget becomes u_r_l_target).
        if character.is_ascii_uppercase() && !column.is_empty() {
            column.push('_');
        }
        column.extend(character.to_lowercase());
        word_start = false;
    }
    column.push_str("_type");
    Some(Some(atom(&column)))
}

// The outer None means dynamic; the inner None requests Laravel's default.
fn morph_argument_string<'a>(value: Option<&Expression<'a>>) -> Option<Option<&'a str>> {
    match value {
        None | Some(Expression::Literal(Literal::Null(_))) => Some(None),
        Some(Expression::Parenthesized(parenthesized)) => {
            morph_argument_string(Some(parenthesized.expression))
        }
        Some(Expression::Literal(Literal::String(string))) => {
            let value = literal_bytes_to_str(string.value?)?;
            Some((!value.is_empty() && value != "0").then_some(value))
        }
        _ => None,
    }
}
