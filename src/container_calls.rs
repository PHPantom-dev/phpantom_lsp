//! Parameters forwarded to callables by Tempest and Laravel containers.

use std::ops::Range as ByteRange;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::{Atom, atom, bytes_to_str};
use crate::class_lookup::{Fqn, is_subtype_of};
use crate::completion::eloquent_string::extract_identifier_backwards;
use crate::completion::named_args::{NamedArgContext, build_named_arg_completions};
use crate::completion::source::code_context::{CodeContext, code_context_at};
use crate::completion::source::string_argument::{StringArgumentPosition, string_argument};
use crate::php_type::{PhpType, TypeKind};
use crate::text_position::{byte_range_to_lsp_range, position_to_offset};
use crate::type_engine::call_resolution::{
    literal_arg_type, with_array_callable_method, with_callable_method,
};
use crate::types::{ClassInfo, FileContext, MethodInfo, ParameterInfo, SharedVec, Visibility};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContainerKind {
    Tempest,
    TempestFunction,
    Laravel,
}

impl ContainerKind {
    fn callback_name(self) -> &'static str {
        match self {
            Self::Tempest => "method",
            Self::TempestFunction => "callable",
            Self::Laravel => "callback",
        }
    }
}

enum Declaration {
    Method(Atom, Atom),
    Function(String),
    Closure(u32),
}

struct Callable {
    parameters: SharedVec<ParameterInfo>,
    declaration: Declaration,
}

fn method_callable(owner: &ClassInfo, method: &MethodInfo) -> Option<Callable> {
    (method.visibility == Visibility::Public && !method.is_virtual).then(|| Callable {
        parameters: method.parameters.clone(),
        declaration: Declaration::Method(owner.fqn(), method.name),
    })
}

fn is_tempest_invoke(name: &str) -> bool {
    name.eq_ignore_ascii_case("Tempest\\Container\\invoke")
        || name.eq_ignore_ascii_case("Tempest\\invoke")
}

impl Backend {
    fn forwarding_container(
        &self,
        expression: &str,
        content: &str,
        offset: u32,
        ctx: &FileContext,
    ) -> Option<ContainerKind> {
        let (name, _) = extract_identifier_backwards(expression)?;
        let (kind, ancestor) = if !expression.contains(['$', ':', '>', '(', ')']) {
            let imports = &ctx.symbol_imports_at(offset).functions;
            if !name.eq_ignore_ascii_case("invoke")
                && !crate::names::lookup_import(imports, expression)
                    .is_some_and(|name| is_tempest_invoke(name))
            {
                return None;
            }
            let (resolved, _) = crate::names::function_lookup_names(
                expression,
                ctx.use_map_at(offset),
                imports,
                ctx.namespace_at(offset),
            );
            if !is_tempest_invoke(&resolved) {
                return None;
            }
            (ContainerKind::TempestFunction, None)
        } else if name.eq_ignore_ascii_case("invoke") {
            (
                ContainerKind::Tempest,
                Some("Tempest\\Container\\Container"),
            )
        } else if name.eq_ignore_ascii_case("call") {
            (
                ContainerKind::Laravel,
                Some("Illuminate\\Contracts\\Container\\Container"),
            )
        } else {
            return None;
        };
        let target = self.resolve_callable_target_at_offset(expression, content, offset, ctx)?;
        if let Some(ancestor) = ancestor {
            let owner = target.owner_class?;
            let loader = self.resolved_class_loader_with(&ctx.classes);
            let class = loader(Fqn::new(&owner))?;
            if !is_subtype_of(&class, ancestor, &loader) {
                return None;
            }
        }
        let first = target.parameters.first()?;
        let second = target.parameters.get(1)?;
        (first.name.trim_start_matches('$') == kind.callback_name()
            && match kind {
                ContainerKind::Tempest | ContainerKind::TempestFunction => second.is_variadic,
                ContainerKind::Laravel => second.name == "$parameters" && !second.is_variadic,
            })
        .then_some(kind)
    }

    fn container_callable(
        &self,
        kind: ContainerKind,
        content: &str,
        args_start: u32,
        ctx: &FileContext,
    ) -> Option<Callable> {
        let (callback, default_method) = callback_arguments(content, args_start, kind)?;
        let offset = callback.start as u32;
        let loaders = self.file_loaders(ctx);
        let rctx = loaders.resolution_ctx_at(content, offset);
        let default_method = if let Some(range) = default_method {
            let text = content.get(range)?;
            let ty =
                literal_arg_type(text).or_else(|| Self::resolve_arg_text_to_type(text, &rctx))?;
            if ty.is_null() {
                None
            } else {
                Some(ty.as_literal()?.string_content()?.into_owned())
            }
        } else {
            None
        };
        self.injected_callable(content, callback, ctx, kind, default_method.as_deref())
    }

    fn injected_callable(
        &self,
        content: &str,
        callback: ByteRange<usize>,
        ctx: &FileContext,
        kind: ContainerKind,
        default_method: Option<&str>,
    ) -> Option<Callable> {
        let offset = callback.start as u32;
        let text = content.get(callback)?.trim();
        if (text.starts_with("function") || text.starts_with("fn") || text.starts_with("static "))
            && let Some(mut parameters) = parameters_at(content, offset)
        {
            let loaders = self.file_loaders(ctx);
            let rctx = loaders.resolution_ctx_at(content, offset);
            for (parameter, _) in &mut parameters {
                parameter.type_hint = parameter.type_hint.as_ref().map(|hint| {
                    crate::names::resolve_code_php_type_names(
                        hint,
                        ctx.namespace_at(offset).as_deref(),
                        Some(ctx.use_map_at(offset)),
                        &ctx.classes,
                        rctx.file.source_class_loader,
                    )
                });
            }
            return Some(Callable {
                parameters: parameters
                    .into_iter()
                    .map(|(parameter, _)| parameter)
                    .collect(),
                declaration: Declaration::Closure(offset),
            });
        }
        if let Some(target) = text.strip_suffix("(...)") {
            return self.written_callable(target, content, offset, ctx);
        }
        if text.starts_with('$')
            && let Some(assigned) =
                Self::extract_callable_target_from_variable(text, content, offset)
            && assigned.bound.is_none()
        {
            return self.written_callable(&assigned.target, content, assigned.offset, ctx);
        }
        let loaders = self.file_loaders(ctx);
        let rctx = loaders.resolution_ctx_at(content, offset);
        let ty = literal_arg_type(text).or_else(|| Self::resolve_arg_text_to_type(text, &rctx))?;
        let current = rctx
            .current_class
            .map(|class| class.fqn())
            .unwrap_or_default();
        if let Some(callable) = with_array_callable_method(
            &ty,
            &rctx.file,
            &current,
            Some(&self.resolved_class_cache),
            &mut method_callable,
        ) {
            return Some(callable);
        }
        if let Some(name) = ty.as_literal().and_then(|literal| literal.string_content()) {
            if kind == ContainerKind::Laravel {
                if let Some((class, method)) = name.split_once('@') {
                    return with_callable_method(
                        &PhpType::named(atom(class.trim_start_matches('\\'))),
                        method,
                        &rctx.file,
                        &current,
                        Some(&self.resolved_class_cache),
                        &mut method_callable,
                    );
                }
                if let Some(method) = default_method {
                    return with_callable_method(
                        &PhpType::named(atom(name.trim_start_matches('\\'))),
                        method,
                        &rctx.file,
                        &current,
                        Some(&self.resolved_class_cache),
                        &mut method_callable,
                    );
                }
            }
            if let Some((class, method)) = name.split_once("::") {
                return self.written_callable(
                    &format!("\\{}::{method}", class.trim_start_matches('\\')),
                    content,
                    offset,
                    ctx,
                );
            }
            let instance = PhpType::named(atom(name.trim_start_matches('\\')));
            if let Some(callable) = with_callable_method(
                &instance,
                "__invoke",
                &rctx.file,
                &current,
                Some(&self.resolved_class_cache),
                &mut method_callable,
            ) {
                return Some(callable);
            }
            return self.written_callable(
                &format!("\\{}", name.trim_start_matches('\\')),
                content,
                offset,
                ctx,
            );
        }
        let instance = match ty.kind() {
            TypeKind::ClassString(Some(inner)) => inner,
            _ => &ty,
        };
        with_callable_method(
            instance,
            default_method.unwrap_or("__invoke"),
            &rctx.file,
            &current,
            Some(&self.resolved_class_cache),
            &mut method_callable,
        )
    }

    fn written_callable(
        &self,
        expression: &str,
        content: &str,
        offset: u32,
        ctx: &FileContext,
    ) -> Option<Callable> {
        let expression = expression.trim();
        if !expression.contains(['$', ':', '>', '(', ')']) {
            let function = self.resolve_function_name(
                expression,
                ctx.use_map_at(offset),
                &ctx.symbol_imports_at(offset).functions,
                ctx.namespace_at(offset),
            )?;
            let fqn = function.namespace.as_ref().map_or_else(
                || function.name.to_string(),
                |namespace| format!("{namespace}\\{}", function.name),
            );
            return Some(Callable {
                parameters: function.parameters.clone(),
                declaration: Declaration::Function(fqn),
            });
        }
        let target = self.resolve_callable_target_at_offset(expression, content, offset, ctx)?;
        if !target.binds_named_args {
            return None;
        }
        let owner = target.owner_class?;
        let (method, _) = extract_identifier_backwards(expression)?;
        Some(Callable {
            parameters: target.parameters,
            declaration: Declaration::Method(owner, atom(method)),
        })
    }

    fn callable_parameter_location(
        &self,
        callable: &Callable,
        caller_uri: &str,
        content: &str,
        name: &str,
    ) -> Option<Location> {
        if !callable
            .parameters
            .iter()
            .any(|parameter| parameter.name.trim_start_matches('$') == name)
        {
            return None;
        }
        let location = match &callable.declaration {
            Declaration::Closure(offset) => {
                return parameter_location(caller_uri, content, *offset, name);
            }
            Declaration::Method(class, method) => {
                self.class_member_declaration_location(class, method, false)?
            }
            Declaration::Function(name) => {
                self.resolve_function_definition(std::slice::from_ref(name))?
            }
        };
        let source = self.get_file_content_arc(location.uri.as_str())?;
        parameter_location(
            location.uri.as_str(),
            &source,
            position_to_offset(&source, location.range.start),
            name,
        )
    }

    /// Complete the names Tempest forwards after its callable argument.
    pub(crate) fn container_named_completions(
        &self,
        context: &NamedArgContext,
        content: &str,
        position: Position,
        ctx: &FileContext,
    ) -> Option<Vec<CompletionItem>> {
        let offset = position_to_offset(content, position);
        let kind = self.forwarding_container(&context.call_expression, content, offset, ctx)?;
        if kind == ContainerKind::Laravel {
            return None;
        }
        let args_start = offset as usize - context.prefix.len() - context.args_text.len();
        let callable = self.container_callable(kind, content, args_start as u32, ctx)?;
        let mut forwarded = context.clone();
        forwarded.positional_count = forwarded.positional_count.saturating_sub(1);
        forwarded
            .existing_named_args
            .push(kind.callback_name().to_string());
        Some(build_named_arg_completions(
            &forwarded,
            &callable.parameters,
        ))
    }

    /// Complete the parameter keys Laravel forwards to its callable.
    pub(crate) fn container_string_completions(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let offset = position_to_offset(content, position) as usize;
        let (quote, quote_char) = code.open_string?;
        let paren = code.open_brackets.iter().rev().find(|bracket| {
            bracket.byte == b'('
                && !extract_identifier_backwards(&content[..bracket.code_before])
                    .is_some_and(|(name, _)| name.eq_ignore_ascii_case("array"))
        })?;
        let (method, _) = extract_identifier_backwards(&content[..paren.code_before])?;
        if !method.eq_ignore_ascii_case("call") {
            return None;
        }
        let argument = string_argument(content, offset, code, paren)?;
        if !matches!(
            argument.position,
            StringArgumentPosition::ArrayKey | StringArgumentPosition::ArrayElement
        ) || !argument
            .name
            .as_deref()
            .map_or(argument.index == 1, |name| name == "parameters")
        {
            return None;
        }
        let map = self.symbol_map_for(ctx.uri.as_deref()?)?;
        let call = map
            .find_enclosing_call_site(offset as u32)
            .filter(|call| call.args_start as usize == paren.offset + 1)?;
        let kind = self.forwarding_container(call.call_expression, content, offset as u32, ctx)?;
        let callable = self.container_callable(kind, content, paren.offset as u32 + 1, ctx)?;
        let prefix = content.get(quote + 1..offset)?;
        let range = byte_range_to_lsp_range(content, quote + 1, argument.string_end);
        Some(CompletionResponse::CompletionItemList(
            callable
                .parameters
                .iter()
                .filter_map(|parameter| {
                    let name = parameter.name.trim_start_matches('$');
                    (name.starts_with(prefix) && !argument.used_keys.iter().any(|key| key == name))
                        .then(|| CompletionItem {
                            label: name.to_owned(),
                            kind: Some(CompletionItemKind::Variable),
                            detail: parameter.type_hint.as_ref().map(ToString::to_string),
                            text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                                range,
                                new_text: crate::text_scan::escape_literal_value(name, quote_char),
                            })),
                            ..Default::default()
                        })
                })
                .collect(),
        ))
    }

    /// Navigate forwarded names to the callable's parameter declarations.
    pub(crate) fn container_argument_definition(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Location> {
        let offset = position_to_offset(content, position);
        let map = self.symbol_map_for(uri)?;
        let call = map.find_enclosing_call_site(offset)?;
        let (method, _) = extract_identifier_backwards(call.call_expression)?;
        if !method.eq_ignore_ascii_case("invoke")
            && !method.eq_ignore_ascii_case("call")
            && (call.call_expression.contains(['$', ':', '>']) || call.named_arg_indices.is_empty())
        {
            return None;
        }
        let ctx = self.file_context(uri);
        let kind = self.forwarding_container(call.call_expression, content, offset, &ctx)?;
        let name = match kind {
            ContainerKind::Tempest | ContainerKind::TempestFunction => call
                .named_arg_indices
                .iter()
                .zip(call.named_arg_names.iter())
                .find_map(|(&index, name)| {
                    let start = call.arg_offsets[index as usize];
                    (name != kind.callback_name()
                        && start <= offset
                        && offset <= start + name.len() as u32)
                        .then_some(name)
                })?,
            ContainerKind::Laravel => {
                let code = code_context_at(content, offset as usize)?;
                let (quote, _) = code.open_string?;
                let paren = code.open_brackets.iter().find(|bracket| {
                    bracket.byte == b'(' && bracket.offset as u32 + 1 == call.args_start
                })?;
                let argument = string_argument(content, offset as usize, &code, paren)?;
                if argument.position != StringArgumentPosition::ArrayKey
                    || !argument
                        .name
                        .as_deref()
                        .map_or(argument.index == 1, |name| name == "parameters")
                {
                    return None;
                }
                content.get(quote + 1..argument.string_end)?
            }
        };
        let callable = self.container_callable(kind, content, call.args_start, &ctx)?;
        self.callable_parameter_location(&callable, uri, content, name)
    }
}

fn parameter_location(uri: &str, content: &str, offset: u32, name: &str) -> Option<Location> {
    let (_, offset) = parameters_at(content, offset)?
        .into_iter()
        .find(|(parameter, _)| parameter.name.trim_start_matches('$') == name)?;
    Some(Location {
        uri: crate::util::parse_url(uri)?,
        range: byte_range_to_lsp_range(content, offset as usize, offset as usize + name.len() + 1),
    })
}

fn callback_arguments(
    content: &str,
    start: u32,
    kind: ContainerKind,
) -> Option<(ByteRange<usize>, Option<ByteRange<usize>>)> {
    struct Finder(u32, ContainerKind);
    impl<'ast, 'arena> Walker<'ast, 'arena, Option<(ByteRange<usize>, Option<ByteRange<usize>>)>>
        for Finder
    {
        fn walk_in_argument_list(
            &self,
            list: &'ast ArgumentList<'arena>,
            found: &mut Option<(ByteRange<usize>, Option<ByteRange<usize>>)>,
        ) {
            if list.left_parenthesis.end.offset != self.0 {
                return;
            }
            let Some(callback) = crate::call_args::first_param_arg(list, self.1.callback_name())
                .filter(|argument| !argument.is_unpacked())
            else {
                return;
            };
            let span = callback.value().span();
            let method = (self.1 == ContainerKind::Laravel)
                .then(|| {
                    list.arguments
                        .iter()
                        .enumerate()
                        .find_map(|(index, argument)| {
                            let matches = match argument {
                                Argument::Named(named) => {
                                    bytes_to_str(named.name.value) == "defaultMethod"
                                }
                                Argument::Positional(_) => index == 2,
                            };
                            matches.then(|| {
                                let span = argument.value().span();
                                span.start.offset as usize..span.end.offset as usize
                            })
                        })
                })
                .flatten();
            *found = Some((span.start.offset as usize..span.end.offset as usize, method));
        }
    }
    crate::parser::with_parsed_program(content, "container_callable", |program, _| {
        let mut found = None;
        Finder(start, kind).walk_program(program, &mut found);
        found
    })
}

fn parameters_at(content: &str, offset: u32) -> Option<Vec<(ParameterInfo, u32)>> {
    struct Finder(u32);
    impl Finder {
        fn record(
            &self,
            offset: u32,
            parameters: &FunctionLikeParameterList<'_>,
            found: &mut Option<Vec<(ParameterInfo, u32)>>,
        ) {
            if offset == self.0 {
                *found = Some(
                    crate::parser::extract_parameters(parameters, None, None, None)
                        .into_iter()
                        .zip(
                            parameters
                                .parameters
                                .iter()
                                .map(|parameter| parameter.variable.span.start.offset),
                        )
                        .collect(),
                );
            }
        }
    }
    impl<'ast, 'arena> Walker<'ast, 'arena, Option<Vec<(ParameterInfo, u32)>>> for Finder {
        fn walk_in_method(
            &self,
            node: &'ast Method<'arena>,
            found: &mut Option<Vec<(ParameterInfo, u32)>>,
        ) {
            self.record(node.name.span.start.offset, &node.parameter_list, found);
        }
        fn walk_in_function(
            &self,
            node: &'ast Function<'arena>,
            found: &mut Option<Vec<(ParameterInfo, u32)>>,
        ) {
            self.record(node.name.span.start.offset, &node.parameter_list, found);
        }
        fn walk_in_closure(
            &self,
            node: &'ast Closure<'arena>,
            found: &mut Option<Vec<(ParameterInfo, u32)>>,
        ) {
            self.record(node.span().start.offset, &node.parameter_list, found);
        }
        fn walk_in_arrow_function(
            &self,
            node: &'ast ArrowFunction<'arena>,
            found: &mut Option<Vec<(ParameterInfo, u32)>>,
        ) {
            self.record(node.span().start.offset, &node.parameter_list, found);
        }
    }
    crate::parser::with_parsed_program(content, "callable_parameters", |program, _| {
        let mut found = None;
        Finder(offset).walk_program(program, &mut found);
        found
    })
}
