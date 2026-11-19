//! Indexed Tempest route attributes and the URI arguments that name them.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, is_subtype_of};
use crate::completion::named_args::NamedArgContext;
use crate::completion::source::code_context::CodeContext;
use crate::php_type::{PhpType, TypeKind};
use crate::symbol_map::CallSite;
use crate::text_position::{byte_range_to_lsp_range, position_to_offset};
use crate::types::FileContext;

#[cfg(test)]
mod tests;

/// A method attribute with a literal URI, confirmed by its implemented interfaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RouteAttribute {
    /// Resolved class of the attribute, including custom route attributes.
    pub attribute: Atom,
    /// Resolved controller class.
    pub class: Atom,
    /// Action method name.
    pub method: Atom,
    /// Start of the action's name token.
    pub method_offset: u32,
    /// URI with built-in prefixes applied, ready for the common case.
    pub path: String,
    /// Original URI, retained only when normalization or built-in prefixes changed it.
    pub original_path: Option<Box<str>>,
    /// Method decorators followed by class decorators, in discovery order.
    pub decorators: Box<[RouteDecorator]>,
    /// Start of the URI's literal contents in the source file.
    pub start: u32,
    /// Source ranges of the placeholder names, excluding braces and patterns.
    pub placeholders: Vec<(u32, u32)>,
    /// Parameters of the action, including those absent from its URI.
    pub parameters: Box<[RouteParameter]>,
}

/// Prefix values are indexed immediately; other attributes need their PHP type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RouteDecorator {
    /// A built-in prefix's decoded literal argument.
    Prefix(Atom),
    /// An attribute whose resolved class may implement RouteDecorator.
    Custom(Atom),
}

/// A controller parameter's exact declaration token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RouteParameter {
    /// Parameter name without its dollar sign.
    pub name: Atom,
    /// Start of the parameter token, including its dollar sign.
    pub offset: u32,
}

/// Tempest placeholders admit an optional marker and nested regex quantifiers.
pub(crate) fn placeholders(path: &str) -> Vec<(usize, usize)> {
    let bytes = path.as_bytes();
    let mut result = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = memchr::memchr(b'{', &bytes[cursor..]) {
        let open = cursor + relative;
        let start = open + 1 + usize::from(bytes.get(open + 1) == Some(&b'?'));
        let mut end = start;
        while bytes
            .get(end)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            end += 1;
        }
        let mut depth = 1;
        cursor = open + 1;
        while cursor < bytes.len() && depth > 0 {
            match bytes[cursor] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            cursor += 1;
        }
        if depth == 0 && end > start && matches!(bytes.get(end), Some(b'}' | b':')) {
            result.push((start, end));
        }
    }
    result
}

fn is_uri_call(name: &str, ctx: &FileContext, offset: u32) -> bool {
    let (resolved, _) = crate::names::function_lookup_names(
        name,
        ctx.use_map_at(offset),
        &ctx.symbol_imports_at(offset).functions,
        ctx.namespace_at(offset),
    );
    resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("Tempest\\Router\\uri")
}

/// The action is positional first, or named anywhere in the argument list.
fn action_text(args: &str) -> Option<&str> {
    crate::text_scan::split_top_level_commas(args)
        .into_iter()
        .enumerate()
        .find_map(|(index, arg)| {
            let arg = arg.trim();
            match crate::completion::named_args::extract_named_arg_name(arg) {
                Some(name) if name == "action" => {
                    arg.split_once(':').map(|(_, value)| value.trim())
                }
                None if index == 0 && !arg.is_empty() => Some(arg),
                _ => None,
            }
        })
}

fn literal(ty: &PhpType) -> Option<std::borrow::Cow<'_, str>> {
    match ty.kind() {
        TypeKind::Literal(value) => value.string_content(),
        TypeKind::ClassString(Some(class)) => match class.kind() {
            TypeKind::Named(name) if name != "object" => Some(std::borrow::Cow::Borrowed(name)),
            _ => None,
        },
        _ => None,
    }
}

fn matches_action(route: &super::route_decorators::ResolvedRoute<'_>, ty: &PhpType) -> bool {
    if let TypeKind::Union(types) = ty.kind() {
        return types.iter().any(|ty| matches_action(route, ty));
    }
    if let Some(entries) = ty.shape_entries() {
        return matches!(entries, [class, method]
            if !class.optional && !method.optional
            && class.key.as_deref().is_none_or(|key| key == "0")
            && method.key.as_deref().is_none_or(|key| key == "1")
            && literal(&class.value_type).is_some_and(|name| name.trim_start_matches('\\').eq_ignore_ascii_case(&route.class))
            && literal(&method.value_type).is_some_and(|name| name.eq_ignore_ascii_case(&route.method)));
    }
    literal(ty).is_some_and(|name| {
        if name.starts_with('/') {
            route.path == name
        } else {
            route.method.eq_ignore_ascii_case("__invoke")
                && name
                    .trim_start_matches('\\')
                    .eq_ignore_ascii_case(&route.class)
        }
    })
}

fn names_route_uri(route: &super::route_decorators::ResolvedRoute<'_>, ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Union(types) => types.iter().any(|ty| names_route_uri(route, ty)),
        _ => literal(ty).is_some_and(|value| value == route.path),
    }
}

impl Backend {
    /// A placeholder in a class-level prefix binds the parameters of every action that uses it.
    pub(crate) fn tempest_route_parameter_scopes(
        &self,
        map: &crate::symbol_map::SymbolMap,
        content: &str,
        name: &str,
        scope: u32,
    ) -> Vec<u32> {
        let shared: Vec<_> = map
            .tempest_routes
            .iter()
            .filter(|route| {
                route.parameters.iter().any(|parameter| {
                    parameter.name == name
                        && map.find_variable_scope(name, parameter.offset) == scope
                }) && self.is_tempest_route(route)
            })
            .flat_map(|route| &route.placeholders)
            .filter(|&&(start, end)| content.get(start as usize..end as usize) == Some(name))
            .collect();
        let mut scopes = Vec::new();
        if !shared.is_empty() {
            for route in &map.tempest_routes {
                if route
                    .placeholders
                    .iter()
                    .any(|range| shared.contains(&range))
                    && self.is_tempest_route(route)
                {
                    for parameter in route
                        .parameters
                        .iter()
                        .filter(|parameter| parameter.name == name)
                    {
                        let other = map.find_variable_scope(name, parameter.offset);
                        if other != scope && !scopes.contains(&other) {
                            scopes.push(other);
                        }
                    }
                }
            }
        }
        scopes
    }

    /// Include URI labels and attribute placeholders when a controller parameter changes.
    pub(crate) fn extend_tempest_route_parameter_references(
        &self,
        uri: &str,
        name: &str,
        scope: u32,
        locations: &mut Vec<Location>,
    ) {
        let Some(controller_map) = self.symbol_map_for(uri) else {
            return;
        };
        let Some(route) = controller_map.tempest_routes.iter().find(|route| {
            route.parameters.iter().any(|p| {
                p.name == name && controller_map.find_variable_scope(name, p.offset) == scope
            }) && self.is_tempest_route(route)
        }) else {
            return;
        };
        let related_scopes = self
            .reference_file_content_arc(uri)
            .map(|content| {
                self.tempest_route_parameter_scopes(&controller_map, &content, name, scope)
            })
            .unwrap_or_default();
        let mut decorators = super::route_decorators::Decorators::default();
        let action_routes: Vec<_> = controller_map
            .tempest_routes
            .iter()
            .filter(|candidate| {
                ((candidate.class == route.class && candidate.method == route.method)
                    || candidate.parameters.iter().any(|parameter| {
                        parameter.name == name
                            && related_scopes.contains(
                                &controller_map.find_variable_scope(name, parameter.offset),
                            )
                    }))
                    && self.is_tempest_route(candidate)
            })
            .map(|route| decorators.resolve(self, route))
            .collect();
        for (file_uri, map) in self.user_file_symbol_maps() {
            if map.tempest_routes.is_empty() && map.tempest_uri_calls.is_empty() {
                continue;
            }
            let Some(content) = self.reference_file_content_arc(&file_uri) else {
                continue;
            };
            if map.source(&content).is_none() {
                continue;
            }
            let Some(parsed_uri) = crate::util::parse_url(&file_uri) else {
                continue;
            };
            let mut push = |start: u32, end: u32| {
                locations.push(Location {
                    uri: parsed_uri.clone(),
                    range: byte_range_to_lsp_range(&content, start as usize, end as usize),
                })
            };
            for declaration in &map.tempest_routes {
                if !action_routes.iter().any(|candidate| {
                    candidate.class == declaration.class && candidate.method == declaration.method
                }) || !self.is_tempest_route(declaration)
                {
                    continue;
                }
                for &(start, end) in &declaration.placeholders {
                    if content.get(start as usize..end as usize) == Some(name) {
                        push(start, end);
                    }
                }
            }
            let ctx = self.file_context(&file_uri);
            for call in &map.call_sites {
                if map
                    .tempest_uri_calls
                    .binary_search(&call.args_start)
                    .is_err()
                {
                    continue;
                }
                let args = &content[call.args_start as usize..call.args_end as usize];
                let Some(action) = self.route_action_type(args, &content, call.args_start, &ctx)
                else {
                    continue;
                };
                if !action_routes.iter().enumerate().any(|(index, candidate)| {
                    (!action_routes[..index].iter().any(|previous| {
                        previous.class == candidate.class && previous.method == candidate.method
                    }) || names_route_uri(candidate, &action))
                        && matches_action(candidate, &action)
                        && route_has_parameter(candidate, name)
                }) {
                    continue;
                }
                if literal(&action).is_some_and(|value| value.starts_with('/'))
                    && let Some((start, text)) = action_source(call, &content)
                    && text.starts_with(['\'', '"'])
                {
                    for (from, to) in placeholders(text) {
                        if &text[from..to] == name {
                            push(start + from as u32, start + to as u32);
                        }
                    }
                }
                for (&index, arg) in call
                    .named_arg_indices
                    .iter()
                    .zip(call.named_arg_names.iter())
                {
                    if arg == name {
                        let start = call.arg_offsets[index as usize];
                        push(start, start + name.len() as u32);
                    }
                }
            }
        }
    }

    fn is_tempest_route(&self, route: &RouteAttribute) -> bool {
        let context = FileContext::default();
        let loader = self.resolved_class_loader(&context);
        loader(Fqn::new(&route.attribute))
            .is_some_and(|class| is_subtype_of(&class, "Tempest\\Router\\Route", &loader))
    }

    fn tempest_route_http_method(&self, route: &RouteAttribute) -> Option<String> {
        let context = FileContext::default();
        let loader = self.resolved_class_loader(&context);
        let mut class = loader(Fqn::new(&route.attribute))?;
        let mut seen = Vec::new();
        loop {
            if seen.contains(&class.fqn()) {
                return None;
            }
            seen.push(class.fqn());
            if let Some(property) = class.get_property("method") {
                let Some(crate::types::PropertySource::DeclaredDefault { value }) =
                    &property.source
                else {
                    return None;
                };
                let uri = self.find_class_file_uri(&class.fqn(), "")?;
                let content = self.get_file_content_arc(&uri)?;
                let file = self.file_context(&uri);
                let loaders = self.file_loaders(&file);
                let ty = Self::resolve_arg_text_to_type(
                    value,
                    &loaders.resolution_ctx_at(&content, property.name_offset),
                )?;
                return ty
                    .as_enum_case()
                    .filter(|(class, _)| class.eq_ignore_ascii_case("Tempest\\Http\\Method"))
                    .map(|(_, name)| name.to_string());
            }
            class = loader(Fqn::new(class.parent_class.as_ref()?))?;
        }
    }

    fn visit_tempest_routes(
        &self,
        mut visit: impl FnMut(&str, &super::route_decorators::ResolvedRoute<'_>, bool),
    ) {
        self.ensure_workspace_index_ready_for_request();
        let mut confirmed = crate::atom::AtomMap::default();
        let mut actions = std::collections::HashSet::new();
        let mut decorators = super::route_decorators::Decorators::default();
        self.visit_declaring_symbol_maps(|uri, map| {
            for route in &map.tempest_routes {
                if *confirmed
                    .entry(route.attribute)
                    .or_insert_with(|| self.is_tempest_route(route))
                {
                    let resolved = decorators.resolve(self, route);
                    visit(uri, &resolved, actions.insert((route.class, route.method)));
                }
            }
        });
    }

    fn route_action_type(
        &self,
        args: &str,
        content: &str,
        offset: u32,
        ctx: &FileContext,
    ) -> Option<PhpType> {
        let loaders = self.file_loaders(ctx);
        let expression = action_text(args)?;
        let context = loaders.resolution_ctx_at(content, offset);
        let resolve = |text: &str| Self::resolve_arg_text_to_type(text, &context);
        crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value_resolved(
            expression, &resolve,
        )
        .or_else(|| resolve(expression))
    }

    fn route_location(
        &self,
        uri: &str,
        route: &RouteAttribute,
        parameter: Option<&str>,
    ) -> Option<Location> {
        let (start, len) = match parameter {
            Some(name) => (
                route.parameters.iter().find(|p| p.name == name)?.offset,
                name.len() + 1,
            ),
            None => (route.method_offset, route.method.len()),
        };
        let content = self.get_file_content_arc(uri)?;
        Some(Location {
            uri: crate::util::parse_url(uri)?,
            range: byte_range_to_lsp_range(&content, start as usize, start as usize + len),
        })
    }

    /// Navigate a URI action, a forwarded label, or a route placeholder.
    pub(crate) fn tempest_route_definition(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Vec<Location> {
        let offset = position_to_offset(content, position);
        let Some(map) = self.symbol_map_for(uri) else {
            return Vec::new();
        };
        if map.tempest_routes.is_empty() && map.tempest_uri_calls.is_empty() {
            return Vec::new();
        }
        if !map.built_from(content) {
            return Vec::new();
        }
        let mut targets = Vec::new();
        for route in &map.tempest_routes {
            if let Some(&(start, end)) = route
                .placeholders
                .iter()
                .find(|&&(start, end)| start <= offset && offset <= end)
                && self.is_tempest_route(route)
                && let Some(location) =
                    self.route_location(uri, route, content.get(start as usize..end as usize))
                && !targets.contains(&location)
            {
                targets.push(location);
            }
        }
        if !targets.is_empty() {
            return targets;
        }
        let Some(call) = map.find_enclosing_call_site(offset) else {
            return Vec::new();
        };
        if map
            .tempest_uri_calls
            .binary_search(&call.args_start)
            .is_err()
        {
            return Vec::new();
        }
        let ctx = self.file_context(uri);
        let parameter = named_at(call, offset);
        // Values passed after the action retain their normal PHP navigation.
        if parameter.is_none() {
            let action_index = call
                .named_arg_indices
                .iter()
                .zip(call.named_arg_names.iter())
                .find_map(|(&index, name)| (name == "action").then_some(index as usize))
                .unwrap_or(0);
            if call
                .comma_offsets
                .iter()
                .filter(|&&comma| comma < offset)
                .count()
                != action_index
                || crate::completion::source::code_context::code_context_at(
                    content,
                    offset as usize,
                )
                .is_none_or(|code| code.open_string.is_none())
            {
                return Vec::new();
            }
        }
        let args = &content[call.args_start as usize..call.args_end as usize];
        let Some(action) = self.route_action_type(args, content, call.args_start, &ctx) else {
            return Vec::new();
        };
        let mut locations = Vec::new();
        self.visit_tempest_routes(|uri, route, first| {
            if (first || names_route_uri(route, &action))
                && matches_action(route, &action)
                && parameter.is_none_or(|name| route_has_parameter(route, name))
                && let Some(location) = self.route_location(uri, route, parameter)
            {
                locations.push(location);
            }
        });
        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
        });
        locations.dedup();
        locations
    }

    /// Placeholder names supplied as variadic named arguments to `uri()`.
    pub(crate) fn tempest_route_named_completions(
        &self,
        context: &NamedArgContext,
        content: &str,
        position: Position,
        ctx: &FileContext,
    ) -> Option<Vec<CompletionItem>> {
        let offset = position_to_offset(content, position);
        if !is_uri_call(&context.call_expression, ctx, offset) {
            return None;
        }
        if context.args_text.trim().is_empty() {
            let mut items = Vec::new();
            let mut methods = crate::atom::AtomMap::default();
            self.visit_tempest_routes(|_, route, _| {
                let label = format!("{}::{}", route.class, route.method);
                if !context.prefix.is_empty()
                    && !label
                        .to_ascii_lowercase()
                        .contains(&context.prefix.to_ascii_lowercase())
                {
                    return;
                }
                items.push(CompletionItem {
                    label,
                    kind: Some(CompletionItemKind::Reference),
                    detail: Some(format!(
                        "{} {}",
                        methods
                            .entry(route.attribute)
                            .or_insert_with(|| self.tempest_route_http_method(route))
                            .as_deref()
                            .unwrap_or("Tempest route"),
                        route.path
                    )),
                    insert_text: Some(if route.method.eq_ignore_ascii_case("__invoke") {
                        format!("\\{}::class", route.class)
                    } else {
                        format!("[\\{}::class, '{}']", route.class, route.method)
                    }),
                    ..Default::default()
                });
            });
            return Some(items);
        }
        let action = self.route_action_type(&context.args_text, content, offset, ctx)?;
        let mut items = Vec::new();
        self.visit_tempest_routes(|_, route, first| {
            if !(first || names_route_uri(route, &action)) || !matches_action(route, &action) {
                return;
            }
            for (start, end) in placeholders(&route.path) {
                let name = &route.path[start..end];
                if name.starts_with(&context.prefix)
                    && !context.existing_named_args.iter().any(|n| n == name)
                {
                    items.push(CompletionItem {
                        label: format!("{name}:"),
                        kind: Some(CompletionItemKind::Variable),
                        detail: Some(format!(
                            "{}::{} — {}",
                            route.class, route.method, route.path
                        )),
                        insert_text: Some(format!("{name}: ")),
                        ..Default::default()
                    });
                }
            }
        });
        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label);
        Some(items)
    }

    /// Route URI/class strings, and parameter names inside an attribute's braces.
    pub(crate) fn tempest_route_string_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let offset = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        if let Some(map) = self.symbol_map_for(uri) {
            for route in &map.tempest_routes {
                if route.start as usize != quote + 1 || !self.is_tempest_route(route) {
                    continue;
                }
                let before = content.get(quote + 1..offset)?;
                let open = before.rfind('{')?;
                let prefix = before[open + 1..]
                    .strip_prefix('?')
                    .unwrap_or(&before[open + 1..]);
                if !prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    return None;
                }
                let start = offset - prefix.len();
                let end = content.as_bytes()[offset..]
                    .iter()
                    .position(|b| !b.is_ascii_alphanumeric() && *b != b'_')
                    .map_or(offset, |len| offset + len);
                let items = route
                    .parameters
                    .iter()
                    .filter(|p| p.name.starts_with(prefix))
                    .map(|parameter| CompletionItem {
                        label: parameter.name.to_string(),
                        kind: Some(CompletionItemKind::Variable),
                        text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                            range: byte_range_to_lsp_range(content, start, end),
                            new_text: parameter.name.to_string(),
                        })),
                        ..Default::default()
                    })
                    .collect();
                return Some(CompletionResponse::CompletionItemList(items));
            }
        }
        let innermost = code.open_brackets.last()?;
        let array = (innermost.byte == b'[').then_some(innermost);
        let call = if array.is_some() {
            code.open_brackets
                .get(code.open_brackets.len().checked_sub(2)?)?
        } else {
            innermost
        };
        if call.byte != b'(' || call.callee_operator.is_some() {
            return None;
        }
        let name_start = content[..call.code_before]
            .bytes()
            .rposition(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'_' | b'\\')))
            .map_or(0, |at| at + 1);
        if !is_uri_call(&content[name_start..call.code_before], ctx, offset as u32) {
            return None;
        }
        let before = content[call.offset + 1..array.map_or(quote, |array| array.offset)].trim();
        let arguments = crate::text_scan::split_top_level_commas(before);
        if !(before.is_empty() || arguments.last().is_some_and(|arg| arg.trim() == "action:")) {
            return None;
        }
        let owner = if let Some(array) = array {
            let before = content[array.offset + 1..quote]
                .trim_end()
                .strip_suffix(',')?
                .trim();
            let loaders = self.file_loaders(ctx);
            let context = loaders.resolution_ctx_at(content, offset as u32);
            let resolve = |text: &str| Self::resolve_arg_text_to_type(text, &context);
            let ty = crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value_resolved(before, &resolve).or_else(|| resolve(before))?;
            Some(literal(&ty)?.into_owned())
        } else {
            None
        };
        let raw_prefix = content.get(quote..offset)?;
        let closed_prefix = format!("{raw_prefix}{}", content.as_bytes()[quote] as char);
        let prefix = crate::text_scan::decode_php_string_literal(&closed_prefix)?;
        let end =
            crate::text_scan::skip_string_forward(content.as_bytes(), quote).min(content.len());
        let end = if content.as_bytes().get(end.saturating_sub(1)) == content.as_bytes().get(quote)
        {
            end - 1
        } else {
            offset
        };
        let mut items = Vec::new();
        let mut methods = crate::atom::AtomMap::default();
        self.visit_tempest_routes(|_, route, _| {
            if owner.as_ref().is_some_and(|owner| {
                !owner
                    .trim_start_matches('\\')
                    .eq_ignore_ascii_case(&route.class)
            }) {
                return;
            }
            let names = if owner.is_some() {
                [Some(route.method.as_str()), None]
            } else {
                [
                    Some(route.path.as_ref()),
                    route
                        .method
                        .eq_ignore_ascii_case("__invoke")
                        .then_some(route.class.as_str()),
                ]
            };
            for name in names.into_iter().flatten() {
                if !name.starts_with(prefix.as_ref()) {
                    continue;
                }
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::Reference),
                    detail: Some(format!(
                        "{} — {}::{}",
                        methods
                            .entry(route.attribute)
                            .or_insert_with(|| self.tempest_route_http_method(route))
                            .as_deref()
                            .unwrap_or("Tempest route"),
                        route.class,
                        route.method
                    )),
                    text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                        range: byte_range_to_lsp_range(content, quote + 1, end),
                        new_text: crate::text_scan::escape_literal_value(
                            name,
                            content.as_bytes()[quote] as char,
                        ),
                    })),
                    ..Default::default()
                });
            }
        });
        items.sort_by(|a, b| a.label.cmp(&b.label));
        Some(CompletionResponse::CompletionItemList(items))
    }
}

fn named_at(call: CallSite<'_>, offset: u32) -> Option<&str> {
    call.named_arg_indices
        .iter()
        .zip(call.named_arg_names.iter())
        .find_map(|(&index, name)| {
            let start = call.arg_offsets[index as usize];
            (start <= offset && offset <= start + name.len() as u32 && name != "action")
                .then_some(name)
        })
}

fn action_source<'a>(call: CallSite<'_>, content: &'a str) -> Option<(u32, &'a str)> {
    let named = call
        .named_arg_indices
        .iter()
        .zip(call.named_arg_names.iter())
        .find_map(|(&index, name)| (name == "action").then_some(index as usize));
    let index = named.unwrap_or(0);
    let start = *call.arg_offsets.get(index)?;
    let end = call
        .comma_offsets
        .get(index)
        .copied()
        .unwrap_or(call.args_end);
    let text = content.get(start as usize..end as usize)?.trim_end();
    let value = if named.is_some() {
        text.split_once(':')?.1
    } else {
        text
    }
    .trim_start();
    Some((start + (text.len() - value.len()) as u32, value))
}

fn route_has_parameter(route: &super::route_decorators::ResolvedRoute<'_>, name: &str) -> bool {
    placeholders(&route.path)
        .iter()
        .any(|&(start, end)| &route.path[start..end] == name)
}
