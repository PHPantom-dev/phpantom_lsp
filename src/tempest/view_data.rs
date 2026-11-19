//! PHP render sites. Expression types always come from the shared engine.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

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

/// The data one caller file supplies to views.
#[derive(Default)]
pub(super) struct Collected {
    pub(super) targets: Vec<(String, Scope)>,
    /// Custom View classes this caller instantiates with its own data.
    pub(super) instantiated: HashSet<String>,
    /// Other files declaring the View helpers this caller's data went through.
    pub(super) dependencies: HashSet<String>,
}

/// Collect each render chain once, including its final `data()` overrides.
pub(super) fn collect(
    backend: &Backend,
    uri: &str,
    file: &FileContext,
    content: &str,
    custom: &BTreeMap<String, super::custom_views::CustomView>,
    wrappers: &super::wrappers::Names,
) -> Collected {
    let mut context = RenderContext {
        backend,
        custom,
        instantiated: HashSet::new(),
        wrappers,
        dependencies: HashSet::new(),
        active: HashSet::new(),
        memo: HashMap::new(),
    };
    let targets = context.collect(uri, file, content, None);
    Collected {
        targets,
        instantiated: context.instantiated,
        dependencies: context.dependencies,
    }
}

struct RenderContext<'a> {
    backend: &'a Backend,
    custom: &'a BTreeMap<String, super::custom_views::CustomView>,
    instantiated: HashSet<String>,
    wrappers: &'a super::wrappers::Names,
    /// Files declaring the helpers walked, whose edits change this data.
    dependencies: HashSet<String>,
    /// Helpers being read, to cut recursion where a helper reaches itself.
    active: HashSet<(String, u32)>,
    /// Data each helper produced for a given set of argument types, so a
    /// helper reached from several call sites with the same arguments is
    /// read once however the calls branch.
    memo: HashMap<HelperKey, Rc<Vec<(String, Scope)>>>,
}

/// A helper declaration and the argument types it is read under.
type HelperKey = (String, u32, Vec<PhpType>);

impl RenderContext<'_> {
    /// The data a helper's body passes to views under `invocation`'s
    /// arguments, read once per distinct input.  A helper that reaches
    /// itself contributes nothing on the inner call.
    fn helper(&mut self, invocation: super::wrappers::Invocation) -> Rc<Vec<(String, Scope)>> {
        let key = (
            invocation.uri.clone(),
            invocation.offset,
            invocation.arguments.clone(),
        );
        if let Some(found) = self.memo.get(&key) {
            return Rc::clone(found);
        }
        let active = (invocation.uri.clone(), invocation.offset);
        if !self.active.insert(active.clone()) {
            return Rc::default();
        }
        self.dependencies.insert(invocation.uri.clone());
        let found = Rc::new(
            self.backend
                .analysable_content(&invocation.uri)
                .map(|body| {
                    let declaration_file = self.backend.file_context(&invocation.uri);
                    self.collect(&invocation.uri, &declaration_file, &body, Some(&invocation))
                })
                .unwrap_or_default(),
        );
        self.active.remove(&active);
        self.memo.insert(key, Rc::clone(&found));
        found
    }

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
                    if let Some(invocation) =
                        super::wrappers::invocation(backend, root, content, file, self.wrappers)
                    {
                        for (target, scope) in self.helper(invocation).iter() {
                            let mut scope = scope.clone();
                            for args in chained.iter().rev() {
                                arguments(args, false, content, &loaders, &mut scope.vars);
                            }
                            scope.globals = scope.vars.clone();
                            result.push((target.clone(), scope));
                        }
                    }
                    continue;
                };
                let path = super::configuration::argument(args, "path", 0);
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
    view_call_arguments(call, file)
}

fn view_call_arguments<'a>(
    call: &'a FunctionCall<'a>,
    file: &FileContext,
) -> Option<&'a ArgumentList<'a>> {
    let Expression::Identifier(name) = call.function else {
        return None;
    };
    is_view_function(bytes_to_str(name.value()), file, name.span().start.offset)
        .then_some(&call.argument_list)
}

/// Identify the view helper in the namespace block where its name was written.
pub(super) fn is_view_function(name: &str, file: &FileContext, offset: u32) -> bool {
    let (resolved, _) = crate::names::function_lookup_names(
        name,
        file.use_map_at(offset),
        &file.symbol_imports_at(offset).functions,
        file.namespace_at(offset),
    );
    resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("Tempest\\View\\view")
}

/// A view call's target and the data names it already supplies.
pub(super) struct CallTarget {
    /// Canonical URI of the template the call renders.
    pub(super) uri: String,
    /// Data names guaranteed by named arguments and known spread keys.
    pub(super) supplied: std::collections::BTreeSet<String>,
}

/// Reverse the same path lookup used when inferring a template's incoming data.
pub(super) fn call_target(
    backend: &Backend,
    uri: &str,
    content: &str,
    file: &FileContext,
    args_start: u32,
) -> Option<CallTarget> {
    let loaders = backend.file_loaders(file);
    crate::parser::with_parsed_program(content, "tempest_view_arguments", |program, content| {
        let args = crate::parser::innermost_node_at(Node::Program(program), args_start, |node| {
            let Node::FunctionCall(call) = node else {
                return None;
            };
            view_call_arguments(call, file)
                .filter(|args| args.left_parenthesis.end.offset == args_start)
        })?;
        let target = path_definition(
            backend,
            uri,
            super::configuration::argument(args, "path", 0)?,
            content,
            &loaders,
        )?;
        let mut supplied = std::collections::BTreeSet::new();
        for argument in args.arguments.iter() {
            match argument {
                Argument::Named(arg) => {
                    supplied.insert(bytes_to_str(arg.name.value).to_owned());
                }
                Argument::Positional(arg) if arg.ellipsis.is_some() => {
                    let mut vars = BTreeMap::new();
                    shape_vars(&expression_type(arg.value, content, &loaders), &mut vars);
                    supplied.extend(vars.into_keys());
                }
                _ => {}
            }
        }
        Some(CallTarget {
            uri: target.uri.to_string(),
            supplied,
        })
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
            Expression::Call(Call::Function(_) | Call::Method(_) | Call::StaticMethod(_))
                | Expression::Instantiation(_)
        ) {
            calls.push(expr);
        }
    }
}
