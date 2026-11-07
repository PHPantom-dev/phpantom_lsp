//! Locate PHP helpers returning a Tempest View; PHP supplies all value types.

use std::collections::BTreeSet;

use mago_span::HasSpan;
use mago_syntax::{cst::*, walker::Walker};

use crate::{
    Backend,
    atom::bytes_to_str,
    php_type::PhpType,
    types::{FileContext, ParameterInfo, SharedVec},
};

/// Names used to select indexed callers without walking unrelated PHP bodies.
#[derive(Default, PartialEq, Eq)]
pub(super) struct Names {
    /// Fully qualified function names, normalized for PHP's case-insensitive lookup.
    pub(super) functions: BTreeSet<String>,
    /// Method names; the actual receiver is resolved before following a call.
    pub(super) methods: BTreeSet<String>,
}

fn returns_view(ty: Option<&PhpType>) -> bool {
    ty.and_then(PhpType::base_name).is_some_and(|name| {
        ["Tempest\\View\\View", "Tempest\\View\\GenericView"]
            .iter()
            .any(|expected| name.trim_start_matches('\\').eq_ignore_ascii_case(expected))
    })
}

/// Discover declared view helpers using metadata already in the PHP index.
pub(super) fn names(backend: &Backend) -> Names {
    let mut names = Names::default();
    for (name, (_, function)) in backend.symbols.global_functions.read().iter() {
        if function.name_offset != 0 && returns_view(function.return_type.as_ref()) {
            names.functions.insert(name.to_ascii_lowercase());
        }
    }
    for classes in backend.symbols.uri_classes_index.read().values() {
        for class in classes {
            for method in &class.methods {
                if method.name_offset != 0 && returns_view(method.return_type.as_ref()) {
                    names.methods.insert(method.name.to_ascii_lowercase());
                }
            }
        }
    }
    names
}

/// The declaration and call-site values needed to read one helper's body.
pub(super) struct Invocation {
    /// URI in which the helper was declared, including inherited methods.
    pub(super) uri: String,
    /// Declaration name offset used to find its precise AST scope.
    pub(super) offset: u32,
    /// Types indexed by declared parameter; unknown arguments remain mixed.
    pub(super) arguments: Vec<PhpType>,
}

/// Resolve the callable through the PHP engine, retaining its declaring file.
pub(super) fn invocation(
    backend: &Backend,
    expr: &Expression<'_>,
    content: &str,
    file: &FileContext,
    names: &Names,
) -> Option<Invocation> {
    let loaders = backend.file_loaders(file);
    let (uri, offset, params, args): (_, _, SharedVec<ParameterInfo>, _) = match expr {
        Expression::Call(Call::Function(call)) => {
            let Expression::Identifier(name) = call.function else {
                return None;
            };
            let at = name.span().start.offset;
            let (resolved, fallback) = crate::names::function_lookup_names(
                bytes_to_str(name.value()),
                file.use_map_at(at),
                &file.symbol_imports_at(at).functions,
                file.namespace_at(at),
            );
            if !names.functions.contains(&resolved.to_ascii_lowercase())
                && !fallback
                    .as_ref()
                    .is_some_and(|name| names.functions.contains(&name.to_ascii_lowercase()))
            {
                return None;
            }
            let candidates: Vec<_> = std::iter::once(resolved.as_str()).chain(fallback).collect();
            let function = backend.find_or_load_function(&candidates)?;
            if !returns_view(function.return_type.as_ref()) {
                return None;
            }
            let fqn = function
                .namespace
                .as_ref()
                .filter(|ns| !ns.is_empty())
                .map_or_else(
                    || function.name.to_string(),
                    |ns| format!("{ns}\\{}", function.name),
                );
            let uri = backend
                .symbols
                .global_functions
                .read()
                .get(fqn.as_str())?
                .0
                .clone();
            (
                uri,
                function.name_offset,
                function.parameters,
                &call.argument_list,
            )
        }
        Expression::Call(Call::Method(call)) if matches!(&call.method, ClassLikeMemberSelector::Identifier(name) if names.methods.contains(&bytes_to_str(name.value).to_ascii_lowercase())) => {
            method(
                backend,
                expr,
                &call.method,
                &call.argument_list,
                content,
                file,
                &loaders,
            )?
        }
        Expression::Call(Call::StaticMethod(call)) if matches!(&call.method, ClassLikeMemberSelector::Identifier(name) if names.methods.contains(&bytes_to_str(name.value).to_ascii_lowercase())) => {
            method(
                backend,
                expr,
                &call.method,
                &call.argument_list,
                content,
                file,
                &loaders,
            )?
        }
        _ => return None,
    };
    // A spread can change every positional binding. Leave such values unknown
    // until the shared argument binder exposes its expanded parameter types.
    let spread = args
        .arguments
        .iter()
        .any(|arg| matches!(arg, Argument::Positional(arg) if arg.ellipsis.is_some()));
    let bindings = crate::call_args::bind_args_to_params(&params, args);
    let mut arguments: Vec<_> = bindings
        .iter()
        .map(|expr| {
            expr.filter(|_| !spread)
                .map_or_else(PhpType::mixed, |expr| {
                    super::view_data::expression_type(expr, content, &loaders)
                })
        })
        .collect();
    if !spread
        && bindings
            .iter()
            .zip(params.iter())
            .any(|(value, param)| value.is_none() && param.default_value.is_some())
    {
        let declaration = backend.file_context(&uri);
        let loaders = backend.file_loaders(&declaration);
        if let Some(body) = backend.get_file_content_arc(&uri) {
            let ctx = loaders.resolution_ctx_at(&body, offset);
            for (index, (value, param)) in bindings.iter().zip(params.iter()).enumerate() {
                if value.is_none()
                    && let Some(default) = &param.default_value
                {
                    arguments[index] = Backend::resolve_arg_text_to_type(default, &ctx)
                        .unwrap_or_else(PhpType::mixed);
                }
            }
        }
    }
    Some(Invocation {
        uri,
        offset,
        arguments,
    })
}

fn method<'a>(
    backend: &Backend,
    expr: &Expression<'_>,
    selector: &ClassLikeMemberSelector<'_>,
    args: &'a ArgumentList<'a>,
    content: &str,
    file: &FileContext,
    loaders: &crate::resolution::FileLoaders<'_>,
) -> Option<(String, u32, SharedVec<ParameterInfo>, &'a ArgumentList<'a>)> {
    let ClassLikeMemberSelector::Identifier(name) = selector else {
        return None;
    };
    let target = backend.resolve_callable_target_at_offset(
        content.get(expr.span().start.offset as usize..args.span().start.offset as usize)?,
        content,
        expr.span().start.offset,
        file,
    )?;
    if !returns_view(target.return_type.as_ref()) {
        return None;
    }
    let loader = loaders.resolved_class_loader();
    let owner = loader(crate::class_lookup::Fqn::new(&target.owner_class?))?;
    let declaring = crate::hover::find_declaring_class(
        &owner,
        bytes_to_str(name.value),
        &crate::hover::MemberKindForOrigin::Method,
        loader,
    );
    let method = declaring
        .methods
        .iter()
        .find(|method| method.name.eq_ignore_ascii_case(bytes_to_str(name.value)))?;
    Some((
        backend.class_declaration_uri(&declaring.fqn())?,
        method.name_offset,
        method.parameters.clone(),
        args,
    ))
}

/// Find the function-like span whose name occupies the indexed offset.
pub(super) fn declaration(program: &Program<'_>, offset: u32) -> Option<(u32, u32)> {
    struct Finder;
    impl<'ast, 'arena> Walker<'ast, 'arena, (u32, Option<(u32, u32)>)> for Finder {
        fn walk_in_function(
            &self,
            node: &'ast Function<'arena>,
            found: &mut (u32, Option<(u32, u32)>),
        ) {
            if node.name.span.start.offset == found.0 {
                found.1 = Some((node.span().start.offset, node.span().end.offset));
            }
        }
        fn walk_in_method(
            &self,
            node: &'ast Method<'arena>,
            found: &mut (u32, Option<(u32, u32)>),
        ) {
            if node.name.span.start.offset == found.0 {
                found.1 = Some((node.span().start.offset, node.span().end.offset));
            }
        }
    }
    let mut found = (offset, None);
    Finder.walk_program(program, &mut found);
    found.1
}
