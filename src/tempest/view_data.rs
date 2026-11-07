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

/// Resolve known path values with the same engine that types the caller's
/// variables, constants, arrays and expressions.
pub(super) fn path_definition(
    backend: &Backend,
    uri: &str,
    expr: &Expression<'_>,
    content: &str,
    loaders: &FileLoaders<'_>,
) -> Option<tower_lsp_server::gen_lsp_types::Location> {
    if let Some((name, relative)) = literal_path(expr) {
        return backend.tempest_view_definition(uri, name, relative);
    }
    let ty = expression_type(expr, content, loaders);
    let name = ty.as_literal()?.string_content()?;
    backend.tempest_view_definition(uri, &name, false)
}

/// Collect each render chain once, including its final `data()` overrides.
pub(super) fn collect(
    backend: &Backend,
    uri: &str,
    file: &FileContext,
    content: &str,
    custom: &BTreeMap<String, super::custom_views::CustomView>,
    instantiated: &mut HashSet<String>,
    wrappers: &super::wrappers::Names,
) -> Vec<(String, Scope)> {
    RenderContext {
        backend,
        custom,
        instantiated,
        wrappers,
        active: HashSet::new(),
    }
    .collect(uri, file, content, None)
}

struct RenderContext<'a> {
    backend: &'a Backend,
    custom: &'a BTreeMap<String, super::custom_views::CustomView>,
    instantiated: &'a mut HashSet<String>,
    wrappers: &'a super::wrappers::Names,
    active: HashSet<(String, u32)>,
}

impl RenderContext<'_> {
    fn collect(
        &mut self,
        uri: &str,
        file: &FileContext,
        content: &str,
        invocation: Option<&super::wrappers::Invocation>,
    ) -> Vec<(String, Scope)> {
        let backend = self.backend;
        let custom = self.custom;
        let loaders = backend.file_loaders(file);
        let _engine = crate::type_engine::call_resolution::activate_type_engine_caches();
        let _parse = crate::parser::with_parse_cache(content);
        crate::parser::with_parsed_program(content, "tempest_view_data", |program, content| {
            let declaration = invocation
                .and_then(|invocation| super::wrappers::declaration(program, invocation.offset));
            if invocation.is_some() && declaration.is_none() {
                return Vec::new();
            }
            let _arguments = declaration.zip(invocation).map(|((start, _), invocation)| {
                crate::type_engine::call_resolution::with_declaration_arguments(
                    uri,
                    start,
                    &invocation.arguments,
                )
            });
            let mut expressions = Vec::new();
            Calls.walk_program(program, &mut expressions);
            let mut seen = HashSet::new();
            let mut result = Vec::new();
            for expr in expressions {
                if declaration.is_some_and(|(start, end)| {
                    expr.span().start.offset < start || expr.span().end.offset > end
                }) {
                    continue;
                }
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
                                .resolve_name_at(
                                    bytes_to_str(name.value()),
                                    name.span().start.offset,
                                )
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
                    let ty = expression_type(root, content, &loaders);
                    if let Some(name) = ty.base_name()
                        && let Some(view) = custom.get(name.trim_start_matches('\\'))
                        && let Some(class) =
                            loaders.resolved_class_loader()(crate::class_lookup::Fqn::new(name))
                    {
                        if !seen.insert(root.span().start.offset) {
                            continue;
                        }
                        let mut scope = view.scope.clone();
                        for args in chained.into_iter().rev() {
                            arguments(args, false, content, &loaders, &mut scope.vars);
                        }
                        super::custom_views::bind_this(backend, &class, &mut scope);
                        scope.globals = scope.vars.clone();
                        self.instantiated
                            .insert(name.trim_start_matches('\\').to_owned());
                        result.push((view.target.clone(), scope));
                        continue;
                    }
                }
                if !seen.insert(root.span().start.offset) {
                    continue;
                }
                let Some(args) = generic.or_else(|| view_arguments(root, file)) else {
                    if self.active.len() < 8
                        && let Some(invocation) =
                            super::wrappers::invocation(backend, root, content, file, self.wrappers)
                    {
                        let key = (invocation.uri.clone(), invocation.offset);
                        if self.active.insert(key.clone()) {
                            if let Some(body) = backend.analysable_content(&invocation.uri) {
                                let declaration_file = backend.file_context(&invocation.uri);
                                for (target, mut scope) in self.collect(
                                    &invocation.uri,
                                    &declaration_file,
                                    &body,
                                    Some(&invocation),
                                ) {
                                    for args in chained.iter().rev() {
                                        arguments(args, false, content, &loaders, &mut scope.vars);
                                    }
                                    scope.globals = scope.vars.clone();
                                    result.push((target, scope));
                                }
                            }
                            self.active.remove(&key);
                        }
                    }
                    continue;
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
                let Some(target) =
                    path.and_then(|path| path_definition(backend, uri, path, content, &loaders))
                else {
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
}

fn view_arguments<'a>(
    expr: &'a Expression<'a>,
    file: &FileContext,
) -> Option<&'a ArgumentList<'a>> {
    let Expression::Call(Call::Function(call)) = expr else {
        return None;
    };
    let Expression::Identifier(name) = call.function else {
        return None;
    };
    let offset = name.span().start.offset;
    let (resolved, _) = crate::names::function_lookup_names(
        bytes_to_str(name.value()),
        file.use_map_at(offset),
        &file.symbol_imports_at(offset).functions,
        file.namespace_at(offset),
    );
    resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("Tempest\\View\\view")
        .then_some(&call.argument_list)
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
            Expression::Call(Call::Function(_) | Call::Method(_) | Call::StaticMethod(_))
                | Expression::Instantiation(_)
        ) {
            calls.push(expr);
        }
    }
}
