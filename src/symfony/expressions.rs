//! Configured Symfony ExpressionLanguage navigation and diagnostics.
//!
//! The scanner only knows generic PHP attribute and constructor shapes. The
//! package names, argument positions, and expression-variable type contracts
//! are supplied by `.phpantom.toml`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tower_lsp::lsp_types::{Location, Position, Range, Url};

use super::php_attributes::{
    PhpArgument, attribute_calls, configured_argument, is_php_identifier, is_php_name,
    method_after_attribute, php_arguments, skip_whitespace,
};
use crate::Backend;
use crate::config::{SymfonyExpressionAttributeConfig, SymfonyExpressionConstructorConfig};
use crate::symbol_map::VarDefKind;
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::{ClassInfo, ClassLikeKind, FileContext, MethodInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpressionAccessKind {
    Property,
    Method,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpressionSegment {
    name: String,
    kind: ExpressionAccessKind,
    start: u32,
    end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpressionContract {
    method_parameters: bool,
    bindings: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpressionChain {
    root: String,
    root_start: u32,
    root_end: u32,
    segments: Vec<ExpressionSegment>,
    method_offset: u32,
    contract: ExpressionContract,
}

struct ExpressionRoot {
    file: FileContext,
    definitions: Vec<Location>,
    classes: Vec<Arc<ClassInfo>>,
}

enum ExpressionMemberStatus {
    Valid,
    Missing(Vec<String>),
    Unresolved,
}

/// A configured ExpressionLanguage member that does not exist in PHP.
pub(crate) struct ExpressionProblem {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) member: String,
    pub(crate) is_method: bool,
    pub(crate) classes: Vec<String>,
}

impl Backend {
    /// Resolve a configured ExpressionLanguage root or member under the cursor.
    pub(crate) fn symfony_expression_definitions_at(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Vec<Location>> {
        let offset = position_to_offset(content, position);
        for chain in self.symfony_expression_chains(uri, content) {
            if contains_offset(chain.root_start, chain.root_end, offset) {
                let definitions = self
                    .resolve_expression_root(uri, content, &chain)?
                    .definitions;
                return (!definitions.is_empty()).then_some(definitions);
            }

            let Some(index) = chain
                .segments
                .iter()
                .position(|segment| contains_offset(segment.start, segment.end, offset))
            else {
                continue;
            };
            let root = self.resolve_expression_root(uri, content, &chain)?;
            let class_loader = self.class_loader(&root.file);
            let path = &chain.segments[..=index];
            let classes = expression_path_target_classes(
                root.classes,
                path,
                &class_loader,
                Some(&self.resolved_class_cache),
            )?;
            let target = path.last()?;
            let mut locations = Vec::new();
            for class in classes {
                let mut location =
                    self.class_member_declaration_location(&class.fqn(), &target.name);
                if location.is_none()
                    && target.kind == ExpressionAccessKind::Property
                    && matches!(target.name.as_str(), "name" | "value")
                    && class.kind == ClassLikeKind::Enum
                {
                    location = self.class_declaration_location(&class.fqn());
                }
                if let Some(location) = location {
                    locations.push(location);
                }
            }
            let locations = dedupe_locations(locations);
            return (!locations.is_empty()).then_some(locations);
        }
        None
    }

    /// Find missing members in configured ExpressionLanguage strings.
    pub(crate) fn symfony_expression_problems(
        &self,
        uri: &str,
        content: &str,
    ) -> Vec<ExpressionProblem> {
        let mut problems = Vec::new();
        for chain in self.symfony_expression_chains(uri, content) {
            let Some(root) = self.resolve_expression_root(uri, content, &chain) else {
                continue;
            };
            if root.classes.is_empty() {
                continue;
            }
            let class_loader = self.class_loader(&root.file);
            let mut classes = root.classes;
            for segment in &chain.segments {
                match expression_member_status(
                    &classes,
                    segment,
                    &class_loader,
                    Some(&self.resolved_class_cache),
                ) {
                    ExpressionMemberStatus::Missing(mut names) => {
                        names.sort();
                        names.dedup();
                        problems.push(ExpressionProblem {
                            start: segment.start as usize,
                            end: segment.end as usize,
                            member: segment.name.clone(),
                            is_method: segment.kind == ExpressionAccessKind::Method,
                            classes: names,
                        });
                        break;
                    }
                    ExpressionMemberStatus::Unresolved => break,
                    ExpressionMemberStatus::Valid => {}
                }
                classes = next_expression_classes(&classes, segment, &class_loader);
                if classes.is_empty() {
                    break;
                }
            }
        }
        problems
    }

    fn symfony_expression_chains(&self, uri: &str, content: &str) -> Vec<ExpressionChain> {
        let config = self.config().symfony.expression_language;
        if !is_php_document(uri) || (config.attributes.is_empty() && config.constructors.is_empty())
        {
            return Vec::new();
        }

        let use_map = self
            .file_imports
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let mut chains = Vec::new();

        for attribute in attribute_calls(content) {
            let namespace = crate::text_scan::namespace_at_offset(content, attribute.name_start)
                .map(str::to_string);
            let raw_name = &content[attribute.name_start..attribute.name_end];
            let attribute_fqn =
                normalize_fqn(&crate::util::resolve_to_fqn(raw_name, &use_map, &namespace));
            let Some((method_start, _)) = method_after_attribute(content, attribute.group_end)
            else {
                continue;
            };
            let Some((args_start, args_end)) = attribute.args else {
                continue;
            };
            let arguments = php_arguments(content, args_start, args_end);

            for rule in config
                .attributes
                .iter()
                .filter(|rule| same_fqn(&rule.attribute, &attribute_fqn))
            {
                let position = rule
                    .position
                    .or_else(|| rule.argument.is_none().then_some(0));
                let Some(argument) =
                    configured_argument(&arguments, rule.argument.as_deref(), position)
                else {
                    continue;
                };
                add_argument_expressions(
                    content,
                    argument,
                    method_start,
                    attribute_contract(rule),
                    &mut chains,
                );
            }

            for rule in config.constructors.iter().filter(|rule| {
                attribute_prefix_allowed(&attribute_fqn, &rule.inside_attribute_prefixes)
            }) {
                for (start, end) in constructor_argument_lists(
                    content, args_start, args_end, rule, &use_map, &namespace,
                ) {
                    let arguments = php_arguments(content, start, end);
                    let position = rule
                        .position
                        .or_else(|| rule.argument.is_none().then_some(0));
                    let Some(argument) =
                        configured_argument(&arguments, rule.argument.as_deref(), position)
                    else {
                        continue;
                    };
                    add_argument_expressions(
                        content,
                        argument,
                        method_start,
                        constructor_contract(rule),
                        &mut chains,
                    );
                }
            }
        }

        chains.sort_by(|left, right| {
            left.root_start
                .cmp(&right.root_start)
                .then(left.root_end.cmp(&right.root_end))
                .then(left.method_offset.cmp(&right.method_offset))
        });
        chains.dedup();
        chains
    }

    fn resolve_expression_root(
        &self,
        uri: &str,
        content: &str,
        chain: &ExpressionChain,
    ) -> Option<ExpressionRoot> {
        let file = self.file_context_at(uri, chain.method_offset);
        let (owner, method) = method_at_offset(&file.classes, chain.method_offset)?;
        let source = chain
            .contract
            .bindings
            .get(&chain.root)
            .map(String::as_str)
            .or_else(|| {
                chain
                    .contract
                    .method_parameters
                    .then_some(chain.root.as_str())
            })?;

        let class_loader = self.class_loader(&file);
        let (definitions, classes) = if source.eq_ignore_ascii_case("return") {
            let definitions = current_file_location(
                uri,
                content,
                method.name_offset as usize,
                method.name_offset as usize + method.name.len(),
            )
            .into_iter()
            .collect();
            let classes = method
                .return_type
                .as_ref()
                .map_or_else(Vec::new, |type_hint| {
                    crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        type_hint,
                        &owner.fqn(),
                        &file.classes,
                        &class_loader,
                    )
                });
            (definitions, classes)
        } else if let Some(class_fqn) = source.strip_prefix("class:") {
            let class_fqn = normalize_fqn(class_fqn.trim());
            let definitions = self
                .class_declaration_location(&class_fqn)
                .into_iter()
                .collect();
            let classes = class_loader(&class_fqn).into_iter().collect();
            (definitions, classes)
        } else {
            let selector = source.strip_prefix("parameter:").unwrap_or(source);
            let parameter = if let Ok(index) = selector.parse::<usize>() {
                method.parameters.get(index)
            } else {
                let name = selector.trim().trim_start_matches('$');
                method
                    .parameters
                    .iter()
                    .find(|parameter| parameter.name.trim_start_matches('$') == name)
            }?;
            let parameter_name = parameter.name.trim_start_matches('$');
            let parameter_offset = self
                .symbol_map_for(uri)?
                .var_defs
                .iter()
                .filter(|site| {
                    site.kind == VarDefKind::Parameter
                        && site.name == parameter_name
                        && site.offset > method.name_offset
                        && (owner.end_offset == 0 || site.offset < owner.end_offset)
                })
                .map(|site| site.offset)
                .min()?;
            let definitions = current_file_location(
                uri,
                content,
                parameter_offset as usize + 1,
                parameter_offset as usize + 1 + parameter_name.len(),
            )
            .into_iter()
            .collect();
            let classes = parameter
                .type_hint
                .as_ref()
                .map_or_else(Vec::new, |type_hint| {
                    crate::type_engine::type_resolution::type_hint_to_classes_typed(
                        type_hint,
                        &owner.fqn(),
                        &file.classes,
                        &class_loader,
                    )
                });
            (definitions, classes)
        };

        drop(class_loader);
        Some(ExpressionRoot {
            file,
            definitions: dedupe_locations(definitions),
            classes: dedupe_classes(classes),
        })
    }
}

fn attribute_contract(rule: &SymfonyExpressionAttributeConfig) -> ExpressionContract {
    ExpressionContract {
        method_parameters: rule.method_parameters,
        bindings: rule.bindings.clone(),
    }
}

fn constructor_contract(rule: &SymfonyExpressionConstructorConfig) -> ExpressionContract {
    ExpressionContract {
        method_parameters: rule.method_parameters,
        bindings: rule.bindings.clone(),
    }
}

fn add_argument_expressions(
    content: &str,
    argument: PhpArgument<'_>,
    method_offset: usize,
    contract: ExpressionContract,
    chains: &mut Vec<ExpressionChain>,
) {
    for (start, end) in php_string_literals(content, argument.value_start, argument.value_end) {
        scan_expression(
            content,
            start,
            end,
            method_offset as u32,
            contract.clone(),
            chains,
        );
    }
}

fn constructor_argument_lists(
    content: &str,
    start: usize,
    end: usize,
    rule: &SymfonyExpressionConstructorConfig,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut lists = Vec::new();
    let mut cursor = start;
    while cursor < end {
        match bytes[cursor] {
            b'\'' | b'"' => {
                cursor = crate::text_scan::skip_string_forward(bytes, cursor).min(end);
                continue;
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'/') => {
                cursor = crate::text_scan::skip_line_comment(bytes, cursor).min(end);
                continue;
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => {
                cursor = crate::text_scan::skip_block_comment(bytes, cursor).min(end);
                continue;
            }
            _ => {}
        }
        if !content[cursor..].starts_with("new")
            || bytes
                .get(cursor.wrapping_sub(1))
                .is_some_and(|byte| is_php_identifier(*byte))
            || bytes
                .get(cursor + 3)
                .is_some_and(|byte| is_php_identifier(*byte))
        {
            cursor += 1;
            continue;
        }

        let mut name_start = cursor + 3;
        skip_whitespace(bytes, &mut name_start);
        let mut name_end = name_start;
        while name_end < end && is_php_name(bytes[name_end]) {
            name_end += 1;
        }
        if name_end == name_start {
            cursor += 3;
            continue;
        }
        let mut open = name_end;
        skip_whitespace(bytes, &mut open);
        if open >= end || bytes[open] != b'(' {
            cursor = name_end;
            continue;
        }
        let Some(close) = crate::text_scan::find_matching_forward(content, open, b'(', b')')
            .filter(|close| *close <= end)
        else {
            cursor = open + 1;
            continue;
        };
        let fqn = crate::util::resolve_to_fqn(&content[name_start..name_end], use_map, namespace);
        if same_fqn(&fqn, &rule.class) {
            lists.push((open + 1, close));
        }
        cursor = close + 1;
    }
    lists
}

fn php_string_literals(content: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut literals = Vec::new();
    let mut cursor = start;
    while cursor < end {
        match bytes[cursor] {
            b'\'' | b'"' => {
                let after = crate::text_scan::skip_string_forward(bytes, cursor);
                if after <= end && after > cursor + 1 {
                    literals.push((cursor + 1, after - 1));
                }
                cursor = after.min(end);
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'/') => {
                cursor = crate::text_scan::skip_line_comment(bytes, cursor).min(end);
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => {
                cursor = crate::text_scan::skip_block_comment(bytes, cursor).min(end);
            }
            _ => cursor += 1,
        }
    }
    literals
}

fn scan_expression(
    content: &str,
    start: usize,
    end: usize,
    method_offset: u32,
    contract: ExpressionContract,
    chains: &mut Vec<ExpressionChain>,
) {
    let expression = &content[start..end];
    let bytes = expression.as_bytes();
    let mut cursor = 0usize;
    let mut quote = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if byte == b'\\' {
                cursor = (cursor + 2).min(bytes.len());
                continue;
            }
            if byte == active_quote {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
            cursor += 1;
            continue;
        }
        if !is_expression_identifier_start(byte) {
            cursor += 1;
            continue;
        }

        let root_start = cursor;
        cursor += 1;
        while cursor < bytes.len() && is_php_identifier(bytes[cursor]) {
            cursor += 1;
        }
        let root_end = cursor;
        if previous_non_whitespace(bytes, root_start) == Some(b'.') {
            continue;
        }
        let mut chain = ExpressionChain {
            root: expression[root_start..root_end].to_string(),
            root_start: (start + root_start) as u32,
            root_end: (start + root_end) as u32,
            segments: Vec::new(),
            method_offset,
            contract: contract.clone(),
        };

        let mut chain_cursor = root_end;
        loop {
            skip_whitespace(bytes, &mut chain_cursor);
            if bytes.get(chain_cursor..chain_cursor + 2) == Some(b"?.") {
                chain_cursor += 2;
            } else if bytes.get(chain_cursor) == Some(&b'.') {
                chain_cursor += 1;
            } else {
                break;
            }
            skip_whitespace(bytes, &mut chain_cursor);
            if !bytes
                .get(chain_cursor)
                .is_some_and(|byte| is_expression_identifier_start(*byte))
            {
                break;
            }
            let member_start = chain_cursor;
            chain_cursor += 1;
            while chain_cursor < bytes.len() && is_php_identifier(bytes[chain_cursor]) {
                chain_cursor += 1;
            }
            let member_end = chain_cursor;
            let mut after_member = chain_cursor;
            skip_whitespace(bytes, &mut after_member);
            let kind = if bytes.get(after_member) == Some(&b'(') {
                ExpressionAccessKind::Method
            } else {
                ExpressionAccessKind::Property
            };
            chain.segments.push(ExpressionSegment {
                name: expression[member_start..member_end].to_string(),
                kind,
                start: (start + member_start) as u32,
                end: (start + member_end) as u32,
            });
            chain_cursor = if kind == ExpressionAccessKind::Method {
                crate::text_scan::find_matching_forward(expression, after_member, b'(', b')')
                    .map_or(member_end, |close| close + 1)
            } else {
                member_end
            };
        }
        chains.push(chain);
    }
}

fn method_at_offset(
    classes: &[Arc<ClassInfo>],
    offset: u32,
) -> Option<(Arc<ClassInfo>, Arc<MethodInfo>)> {
    classes.iter().find_map(|class| {
        class
            .methods
            .iter()
            .find(|method| method.name_offset == offset)
            .map(|method| (Arc::clone(class), Arc::clone(method)))
    })
}

fn expression_path_target_classes(
    mut classes: Vec<Arc<ClassInfo>>,
    path: &[ExpressionSegment],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&crate::virtual_members::ResolvedClassCache>,
) -> Option<Vec<Arc<ClassInfo>>> {
    for (index, segment) in path.iter().enumerate() {
        let (matching, _) = matching_member_classes(&classes, segment, class_loader, cache);
        if matching.is_empty() {
            return None;
        }
        if index + 1 == path.len() {
            return Some(matching);
        }
        classes = next_expression_classes(&matching, segment, class_loader);
        if classes.is_empty() {
            return None;
        }
    }
    None
}

fn expression_member_status(
    classes: &[Arc<ClassInfo>],
    segment: &ExpressionSegment,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&crate::virtual_members::ResolvedClassCache>,
) -> ExpressionMemberStatus {
    let (matching, dynamic) = matching_member_classes(classes, segment, class_loader, cache);
    if !matching.is_empty() {
        return ExpressionMemberStatus::Valid;
    }
    if dynamic {
        return ExpressionMemberStatus::Unresolved;
    }
    ExpressionMemberStatus::Missing(
        classes
            .iter()
            .map(|class| class.fqn().to_string())
            .collect(),
    )
}

fn matching_member_classes(
    classes: &[Arc<ClassInfo>],
    segment: &ExpressionSegment,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&crate::virtual_members::ResolvedClassCache>,
) -> (Vec<Arc<ClassInfo>>, bool) {
    let mut matching = Vec::new();
    let mut dynamic = false;
    for class in classes {
        let resolved = if class.name == "__object_shape" {
            Arc::clone(class)
        } else {
            crate::virtual_members::resolve_class_fully_maybe_cached(class, class_loader, cache)
        };
        let exists = match segment.kind {
            ExpressionAccessKind::Property => resolved.has_property(&segment.name),
            ExpressionAccessKind::Method => resolved.has_method(&segment.name),
        };
        if exists {
            matching.push(Arc::clone(class));
            continue;
        }
        dynamic |= match segment.kind {
            ExpressionAccessKind::Property => {
                resolved.name.eq_ignore_ascii_case("stdClass") || resolved.has_method("__get")
            }
            ExpressionAccessKind::Method => resolved.has_method("__call"),
        };
    }
    (matching, dynamic)
}

fn next_expression_classes(
    classes: &[Arc<ClassInfo>],
    segment: &ExpressionSegment,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<Arc<ClassInfo>> {
    let mut next = Vec::new();
    for class in classes {
        match segment.kind {
            ExpressionAccessKind::Property => {
                next.extend(crate::type_engine::type_resolution::resolve_property_types(
                    &segment.name,
                    class,
                    classes,
                    class_loader,
                ));
            }
            ExpressionAccessKind::Method => {
                if let Some(return_type) = crate::inheritance::resolve_method_return_type(
                    class,
                    &segment.name,
                    class_loader,
                ) {
                    next.extend(
                        crate::type_engine::type_resolution::type_hint_to_classes_typed(
                            &return_type,
                            &class.fqn(),
                            classes,
                            class_loader,
                        ),
                    );
                }
            }
        }
    }
    dedupe_classes(next)
}

fn current_file_location(uri: &str, content: &str, start: usize, end: usize) -> Option<Location> {
    let uri = Url::parse(uri).ok()?;
    Some(Location::new(
        uri,
        Range::new(
            offset_to_position(content, start),
            offset_to_position(content, end),
        ),
    ))
}

fn dedupe_classes(classes: Vec<Arc<ClassInfo>>) -> Vec<Arc<ClassInfo>> {
    let mut seen = HashSet::new();
    classes
        .into_iter()
        .filter(|class| seen.insert(class.fqn().to_ascii_lowercase()))
        .collect()
}

fn dedupe_locations(locations: Vec<Location>) -> Vec<Location> {
    let mut seen = HashSet::new();
    locations
        .into_iter()
        .filter(|location| {
            seen.insert((
                location.uri.to_string(),
                location.range.start.line,
                location.range.start.character,
            ))
        })
        .collect()
}

fn attribute_prefix_allowed(attribute: &str, prefixes: &[String]) -> bool {
    prefixes.is_empty()
        || prefixes.iter().any(|prefix| {
            let prefix = normalize_fqn(prefix);
            attribute
                .get(..prefix.len())
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&prefix))
        })
}

fn same_fqn(left: &str, right: &str) -> bool {
    normalize_fqn(left).eq_ignore_ascii_case(&normalize_fqn(right))
}

fn normalize_fqn(name: &str) -> String {
    name.trim().trim_start_matches('\\').to_string()
}

fn is_php_document(uri: &str) -> bool {
    uri.split(['?', '#'])
        .next()
        .unwrap_or(uri)
        .to_ascii_lowercase()
        .ends_with(".php")
}

fn is_expression_identifier_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn previous_non_whitespace(bytes: &[u8], start: usize) -> Option<u8> {
    bytes[..start]
        .iter()
        .rev()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
}

fn contains_offset(start: u32, end: u32, offset: u32) -> bool {
    start <= offset && offset <= end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_nullable_member_chains_and_method_calls() {
        let content = "'request?.course().owner.id'";
        let mut chains = Vec::new();
        scan_expression(
            content,
            1,
            content.len() - 1,
            42,
            ExpressionContract {
                method_parameters: true,
                bindings: HashMap::new(),
            },
            &mut chains,
        );

        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].root, "request");
        assert_eq!(chains[0].segments.len(), 3);
        assert_eq!(chains[0].segments[0].kind, ExpressionAccessKind::Method);
        assert_eq!(chains[0].segments[2].name, "id");
    }

    #[test]
    fn extracts_each_string_from_an_array_argument() {
        let content = "['request.id', 'request.owner.id']";
        assert_eq!(
            php_string_literals(content, 0, content.len()),
            vec![(2, 12), (16, 32)]
        );
    }
}
