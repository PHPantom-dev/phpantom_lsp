//! Tempest view paths, URI calls, and route attributes collected during indexing.

use super::*;
use crate::symbol_map::StringKeyKind;
use crate::virtual_members::laravel::helpers::resolved_string_literal;

/// Keep route-shaped attributes during the class walk. Their types are
/// resolved later, when cross-file interfaces are available.
pub(super) fn record_route_attributes(class: &Class<'_>, ctx: &mut ExtractionCtx<'_>) {
    use crate::tempest::routes::{RouteAttribute, RouteParameter, placeholders};

    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        if method
            .modifiers
            .iter()
            .any(|modifier| matches!(modifier, Modifier::Private(_) | Modifier::Protected(_)))
        {
            continue;
        }
        'attributes: for attribute in method
            .attribute_lists
            .iter()
            .flat_map(|list| list.attributes.iter())
        {
            let Some(value) = attribute.argument_list.as_ref().and_then(|args| {
                args.arguments
                    .iter()
                    .enumerate()
                    .find_map(|(index, arg)| match arg {
                        PartialArgument::Named(arg) if arg.name.value == b"uri" => Some(arg.value),
                        PartialArgument::Positional(arg) if index == 0 => Some(arg.value),
                        _ => None,
                    })
            }) else {
                continue;
            };
            let Some((path, start, end)) = resolved_string_literal(value, ctx.content) else {
                continue;
            };
            if !path.starts_with('/') {
                continue;
            }
            let Some(class_name) = ctx.resolved_name_at(class.name.span.start.offset) else {
                continue;
            };
            let attribute_name = ctx
                .resolved_name_at(attribute.name.span().start.offset)
                .unwrap_or_else(|| bytes_to_str(attribute.name.value()));
            let parameters = method
                .parameter_list
                .parameters
                .iter()
                .map(|parameter| RouteParameter {
                    name: crate::atom::atom(
                        bytes_to_str(parameter.variable.name).trim_start_matches('$'),
                    ),
                    offset: parameter.variable.span.start.offset,
                })
                .collect();
            let mut route_path = path.to_string();
            let mut route_placeholders: Vec<_> = placeholders(&ctx.content[start..end])
                .into_iter()
                .map(|(from, to)| (start as u32 + from as u32, start as u32 + to as u32))
                .collect();
            for decorator in method
                .attribute_lists
                .iter()
                .chain(class.attribute_lists.iter())
                .flat_map(|list| list.attributes.iter())
            {
                if !ctx
                    .resolved_name_at(decorator.name.span().start.offset)
                    .is_some_and(|name| name.eq_ignore_ascii_case("Tempest\\Router\\Prefix"))
                {
                    continue;
                }
                let Some(value) = decorator.argument_list.as_ref().and_then(|args| {
                    args.arguments
                        .iter()
                        .enumerate()
                        .find_map(|(index, arg)| match arg {
                            PartialArgument::Named(arg) if arg.name.value == b"prefix" => {
                                Some(arg.value)
                            }
                            PartialArgument::Positional(arg) if index == 0 => Some(arg.value),
                            _ => None,
                        })
                }) else {
                    continue 'attributes;
                };
                let Some((prefix, start, end)) = resolved_string_literal(value, ctx.content) else {
                    continue 'attributes;
                };
                route_path = format!(
                    "{}/{}",
                    prefix.trim_end_matches('/'),
                    route_path.trim_start_matches('/')
                );
                route_placeholders.extend(
                    placeholders(&ctx.content[start..end])
                        .into_iter()
                        .map(|(from, to)| (start as u32 + from as u32, start as u32 + to as u32)),
                );
            }
            if !route_path.starts_with('/') {
                route_path.insert(0, '/');
            }
            ctx.tempest_routes.push(RouteAttribute {
                attribute: crate::atom::atom(attribute_name),
                class: crate::atom::atom(class_name),
                method: crate::atom::atom_bytes(method.name.value),
                method_offset: method.name.span.start.offset,
                path: route_path,
                start: start as u32,
                placeholders: route_placeholders,
                parameters,
            });
        }
    }
}

pub(super) fn emit_function_call(
    call: &FunctionCall<'_>,
    name: &Identifier<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let resolved = ctx
        .resolved_name_at(name.span().start.offset)
        .unwrap_or_else(|| bytes_to_str(name.value()));
    if resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("Tempest\\Router\\uri")
    {
        ctx.tempest_uri_calls
            .push(call.argument_list.left_parenthesis.end.offset);
        return;
    }
    if !resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("Tempest\\View\\view")
    {
        return;
    }
    let argument =
        call.argument_list
            .arguments
            .iter()
            .enumerate()
            .find_map(|(index, arg)| match arg {
                Argument::Named(named) if named.name.value == b"path" => Some(named.value),
                Argument::Positional(arg) if index == 0 => Some(arg.value),
                _ => None,
            });
    let Some(expression) = argument else {
        return;
    };
    let (expression, relative) = match expression {
        Expression::Binary(binary)
            if binary.operator.is_concatenation()
                && matches!(
                    binary.lhs,
                    Expression::MagicConstant(MagicConstant::Directory(_))
                ) =>
        {
            (binary.rhs, true)
        }
        expression => (expression, false),
    };
    let Expression::Literal(Literal::String(literal)) = expression else {
        return;
    };
    let Some(value) = literal.value.and_then(literal_bytes_to_str) else {
        return;
    };
    ctx.spans.push(SymbolSpan {
        start: literal.span.start.offset + 1,
        end: literal.span.end.offset.saturating_sub(1),
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::TempestView { relative },
            key: value.to_string(),
            is_write: false,
            is_optional: false,
        },
    });
}
