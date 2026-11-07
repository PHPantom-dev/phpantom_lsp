//! PHP render sites. Expression types always come from the shared engine.

use std::collections::{BTreeMap, HashSet};

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::php_type::{PhpType, TypeKind};
use crate::resolution::FileLoaders;
use crate::types::FileContext;

use super::inference::{Scope, variable_name};

/// Resolve in the declaration's scope and qualify names for another file.
pub(super) fn expression_type(
    expr: &Expression<'_>,
    content: &str,
    loaders: &FileLoaders<'_>,
) -> PhpType {
    let offset = expr.span().start.offset;
    let ctx = loaders.var_ctx_at(content, offset);
    let ty = match expr {
        Expression::Variable(Variable::Direct(var)) => {
            crate::type_engine::variable::resolution::resolve_variable_php_type(
                ctx.var_query(bytes_to_str(var.name), offset),
            )
        }
        expr => {
            crate::type_engine::variable::iterable_elements::resolve_expression_type(expr, &ctx)
        }
    }
    .unwrap_or_else(PhpType::mixed);
    // These names have already been resolved in the caller. An annotation
    // injected into a different file must not resolve them a second time.
    ty.resolve_names(&|name| format!("\\{}", name.trim_start_matches('\\')))
}

/// Extract guaranteed string keys from a statically known data array.
pub(super) fn shape_vars(ty: &PhpType, vars: &mut BTreeMap<String, PhpType>) {
    if let TypeKind::ArrayShape(entries) = ty.kind() {
        for entry in entries {
            if !entry.optional
                && let Some(name) = entry.key.as_deref().filter(|name| variable_name(name))
            {
                vars.insert(name.to_owned(), entry.value_type.clone());
            }
        }
    }
}

/// Apply variadic named data in PHP argument order; later calls overwrite it.
pub(super) fn arguments(
    args: &ArgumentList<'_>,
    skip_path: bool,
    content: &str,
    loaders: &FileLoaders<'_>,
    vars: &mut BTreeMap<String, PhpType>,
) {
    for arg in args.arguments.iter() {
        match arg {
            Argument::Named(arg) => {
                let name = bytes_to_str(arg.name.value);
                if (!skip_path || name != "path") && variable_name(name) {
                    vars.insert(
                        name.to_owned(),
                        expression_type(arg.value, content, loaders),
                    );
                }
            }
            Argument::Positional(arg) if arg.ellipsis.is_some() => {
                shape_vars(&expression_type(arg.value, content, loaders), vars);
                if skip_path {
                    vars.remove("path");
                }
            }
            _ => {}
        }
    }
}

/// A literal path, optionally anchored to the declaring file with `__DIR__`.
pub(super) fn literal_path<'a>(expr: &'a Expression<'_>) -> Option<(&'a str, bool)> {
    let (expr, relative) = match expr {
        Expression::Binary(binary)
            if binary.operator.is_concatenation()
                && matches!(
                    binary.lhs,
                    Expression::MagicConstant(MagicConstant::Directory(_))
                ) =>
        {
            (binary.rhs, true)
        }
        expr => (expr, false),
    };
    let Expression::Literal(Literal::String(literal)) = expr else {
        return None;
    };
    Some((crate::atom::literal_bytes_to_str(literal.value?)?, relative))
}

/// Collect each render chain once, including its final `data()` overrides.
pub(super) fn collect(
    backend: &Backend,
    uri: &str,
    file: &FileContext,
    content: &str,
    custom: &BTreeMap<String, super::custom_views::CustomView>,
    instantiated: &mut HashSet<String>,
) -> Vec<(String, Scope)> {
    let loaders = backend.file_loaders(file);
    let _engine = crate::type_engine::call_resolution::activate_type_engine_caches();
    let _parse = crate::parser::with_parse_cache(content);
    crate::parser::with_parsed_program(content, "tempest_view_data", |program, content| {
        let mut expressions = Vec::new();
        Calls.walk_program(program, &mut expressions);
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for expr in expressions {
            let mut root = expr;
            let mut chained = Vec::new();
            loop {
                match root {
                    Expression::Parenthesized(paren) => root = paren.expression,
                    Expression::Call(Call::Method(call)) if matches!(&call.method, ClassLikeMemberSelector::Identifier(name) if name.value.eq_ignore_ascii_case(b"data")) =>
                    {
                        chained.push(&call.argument_list);
                        root = call.object;
                    }
                    _ => break,
                }
            }
            let generic = match root {
                Expression::Instantiation(new) => match new.class {
                    Expression::Identifier(name)
                        if file
                            .resolve_name_at(bytes_to_str(name.value()), name.span().start.offset)
                            .trim_start_matches('\\')
                            .eq_ignore_ascii_case("Tempest\\View\\GenericView") =>
                    {
                        new.argument_list.as_ref()
                    }
                    _ => None,
                },
                _ => None,
            };
            if generic.is_none()
                && !matches!(root, Expression::Call(Call::Function(_)))
                && !chained.is_empty()
            {
                if !seen.insert(root.span().start.offset) {
                    continue;
                }
                let ty = expression_type(root, content, &loaders);
                if let Some(name) = ty.base_name()
                    && let Some(view) = custom.get(name.trim_start_matches('\\'))
                    && let Some(class) =
                        loaders.resolved_class_loader()(crate::class_lookup::Fqn::new(name))
                {
                    let mut scope = view.scope.clone();
                    for args in chained.into_iter().rev() {
                        arguments(args, false, content, &loaders, &mut scope.vars);
                    }
                    super::custom_views::bind_this(backend, &class, &mut scope);
                    scope.globals = scope.vars.clone();
                    instantiated.insert(name.trim_start_matches('\\').to_owned());
                    result.push((view.target.clone(), scope));
                }
                continue;
            }
            if !seen.insert(root.span().start.offset) {
                continue;
            }
            let args = if let Some(args) = generic {
                args
            } else {
                let Expression::Call(Call::Function(call)) = root else {
                    continue;
                };
                let Expression::Identifier(name) = call.function else {
                    continue;
                };
                let offset = name.span().start.offset;
                let (resolved, _) = crate::names::function_lookup_names(
                    bytes_to_str(name.value()),
                    file.use_map_at(offset),
                    &file.symbol_imports_at(offset).functions,
                    file.namespace_at(offset),
                );
                if !resolved
                    .trim_start_matches('\\')
                    .eq_ignore_ascii_case("Tempest\\View\\view")
                {
                    continue;
                }
                &call.argument_list
            };
            let path = args
                .arguments
                .iter()
                .enumerate()
                .find_map(|(i, arg)| match arg {
                    Argument::Named(arg) if arg.name.value == b"path" => Some(arg.value),
                    Argument::Positional(arg) if i == 0 && arg.ellipsis.is_none() => {
                        Some(arg.value)
                    }
                    _ => None,
                });
            let Some((path, relative)) = path.and_then(literal_path) else {
                continue;
            };
            let Some(target) = backend.tempest_view_definition(uri, path, relative) else {
                continue;
            };
            let mut scope = Scope::default();
            if generic.is_some() {
                for (index, arg) in args.arguments.iter().enumerate() {
                    let data = match arg {
                        Argument::Named(arg) if arg.name.value == b"data" => Some(arg.value),
                        Argument::Positional(arg) if index == 1 => Some(arg.value),
                        _ => None,
                    };
                    if let Some(data) = data {
                        shape_vars(&expression_type(data, content, &loaders), &mut scope.vars);
                    }
                }
            } else {
                arguments(args, true, content, &loaders, &mut scope.vars);
            }
            for args in chained.into_iter().rev() {
                arguments(args, false, content, &loaders, &mut scope.vars);
            }
            scope.globals = scope.vars.clone();
            result.push((target.uri.to_string(), scope));
        }
        result
    })
}

struct Calls;

impl<'ast, 'arena> Walker<'ast, 'arena, Vec<&'ast Expression<'arena>>> for Calls {
    fn walk_in_expression(
        &self,
        expr: &'ast Expression<'arena>,
        calls: &mut Vec<&'ast Expression<'arena>>,
    ) {
        if matches!(
            expr,
            Expression::Call(Call::Function(_) | Call::Method(_)) | Expression::Instantiation(_)
        ) {
            calls.push(expr);
        }
    }
}
