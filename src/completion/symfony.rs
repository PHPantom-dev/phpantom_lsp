//! Symfony named-resource completion.
//!
//! The framework index stores semantic names that PHP's AST treats as plain
//! strings: service IDs and container parameters. This module recognizes the
//! corresponding PHP, YAML, and XML string positions and completes from the
//! declarations already present in the workspace index.

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit, Position, Range,
    TextEdit,
};

use crate::Backend;
use crate::framework::{SymfonySymbolKind, is_framework_resource_uri};
use crate::text_position::{offset_to_position, position_to_offset};

struct SymfonyCompletionContext {
    kind: SymfonySymbolKind,
    prefix: String,
    content_start: usize,
    escape_backslashes: bool,
    route_name: Option<String>,
}

impl Backend {
    pub(crate) fn try_symfony_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        let context = if is_framework_resource_uri(uri) {
            detect_resource_context(uri, content, position)?
        } else {
            detect_php_context(content, position)?
        };
        let candidates = if context.kind == SymfonySymbolKind::RouteParameter {
            self.framework_route_parameter_names(context.route_name.as_deref()?)
        } else {
            self.framework_symfony_symbol_names(context.kind)
        };
        if candidates.is_empty() {
            return None;
        }

        let prefix = context.prefix.to_ascii_lowercase();
        let range = Range {
            start: offset_to_position(content, context.content_start),
            end: position,
        };
        let items = candidates
            .into_iter()
            .filter(|name| prefix.is_empty() || name.to_ascii_lowercase().starts_with(&prefix))
            .enumerate()
            .map(|(index, name)| {
                let inserted = if context.escape_backslashes {
                    name.replace('\\', "\\\\")
                } else {
                    name.clone()
                };
                CompletionItem {
                    label: name,
                    kind: Some(match context.kind {
                        SymfonySymbolKind::Parameter => CompletionItemKind::PROPERTY,
                        SymfonySymbolKind::Service => CompletionItemKind::REFERENCE,
                        SymfonySymbolKind::Route => CompletionItemKind::VALUE,
                        SymfonySymbolKind::RouteParameter => CompletionItemKind::FIELD,
                        SymfonySymbolKind::Template => CompletionItemKind::FILE,
                    }),
                    detail: Some(format!("Symfony {}", context.kind.label())),
                    sort_text: Some(format!("{index:05}")),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range,
                        new_text: inserted,
                    })),
                    ..Default::default()
                }
            })
            .collect::<Vec<_>>();

        (!items.is_empty()).then_some(CompletionResponse::Array(items))
    }
}

fn detect_php_context(content: &str, position: Position) -> Option<SymfonyCompletionContext> {
    let cursor = position_to_offset(content, position) as usize;
    let (quote_start, quote) = opening_quote(content, cursor)?;
    let raw_prefix = content.get(quote_start + 1..cursor)?;

    if let Some(percent) = raw_prefix.rfind('%')
        && !raw_prefix[percent + 1..].contains('%')
    {
        return Some(SymfonyCompletionContext {
            kind: SymfonySymbolKind::Parameter,
            prefix: raw_prefix[percent + 1..].to_string(),
            content_start: quote_start + percent + 2,
            escape_backslashes: false,
            route_name: None,
        });
    }

    let service_prefix = raw_prefix
        .bytes()
        .take_while(|byte| matches!(byte, b'@' | b'?' | b'!'))
        .count();
    let (call_name, argument_index, args_start) = php_call_context(content, quote_start)?;
    let call_name = call_name.to_ascii_lowercase();
    let named_argument = named_argument_before(content, args_start, quote_start);
    if argument_index > 0
        && is_route_reference_call(&call_name, content, quote_start)
        && content[args_start..quote_start].contains('[')
        && let Some(route_name) = first_string_argument(content, args_start, quote_start)
    {
        return Some(SymfonyCompletionContext {
            kind: SymfonySymbolKind::RouteParameter,
            prefix: raw_prefix.to_string(),
            content_start: quote_start + 1,
            escape_backslashes: false,
            route_name: Some(route_name),
        });
    }
    let service_context = (matches!(call_name.as_str(), "service" | "decorate" | "target")
        && argument_index == 0)
        || (call_name == "alias" && argument_index == 1)
        || (matches!(call_name.as_str(), "get" | "has")
            && argument_index == 0
            && looks_like_container_call(content, quote_start))
        || (call_name == "autowire"
            && named_argument.is_some_and(|name| name.eq_ignore_ascii_case("service")))
        || service_prefix > 0;
    let parameter_context = (matches!(
        call_name.as_str(),
        "param" | "getparameter" | "hasparameter"
    ) && argument_index == 0)
        || (call_name == "autowire"
            && named_argument.is_some_and(|name| name.eq_ignore_ascii_case("param")));
    let route_context = (matches!(call_name.as_str(), "generateurl" | "redirecttoroute")
        && argument_index == 0)
        || (call_name == "generate"
            && argument_index == 0
            && looks_like_route_generator_call(content, quote_start));
    let template_context = argument_index == 0
        && (matches!(
            call_name.as_str(),
            "render" | "renderview" | "renderblock" | "htmltemplate" | "texttemplate"
        ) || (call_name == "template"
            && named_argument.is_none_or(|name| name.eq_ignore_ascii_case("template"))));
    let kind = if service_context {
        SymfonySymbolKind::Service
    } else if parameter_context {
        SymfonySymbolKind::Parameter
    } else if route_context {
        SymfonySymbolKind::Route
    } else if template_context {
        SymfonySymbolKind::Template
    } else {
        return None;
    };

    Some(SymfonyCompletionContext {
        kind,
        prefix: raw_prefix[service_prefix..].replace("\\\\", "\\"),
        content_start: quote_start + 1 + service_prefix,
        escape_backslashes: quote == b'\'' || quote == b'"',
        route_name: None,
    })
}

fn detect_resource_context(
    uri: &str,
    content: &str,
    position: Position,
) -> Option<SymfonyCompletionContext> {
    let cursor = position_to_offset(content, position) as usize;
    let line_start = content[..cursor].rfind('\n').map_or(0, |idx| idx + 1);
    let prefix = &content[line_start..cursor];

    if let Some((quote_start, _)) = opening_quote(content, cursor)
        && let Some((call_name, argument_index, args_start)) =
            php_call_context(content, quote_start)
    {
        if matches!(call_name, "path" | "url") && argument_index == 0 {
            return Some(SymfonyCompletionContext {
                kind: SymfonySymbolKind::Route,
                prefix: content[quote_start + 1..cursor].to_string(),
                content_start: quote_start + 1,
                escape_backslashes: false,
                route_name: None,
            });
        }
        if matches!(call_name, "path" | "url")
            && content[args_start..quote_start].contains('{')
            && let Some(route_name) = first_string_argument(content, args_start, quote_start)
        {
            return Some(SymfonyCompletionContext {
                kind: SymfonySymbolKind::RouteParameter,
                prefix: content[quote_start + 1..cursor].to_string(),
                content_start: quote_start + 1,
                escape_backslashes: false,
                route_name: Some(route_name),
            });
        }
        if is_twig_uri(uri)
            && matches!(
                call_name.to_ascii_lowercase().as_str(),
                "include" | "source"
            )
            && argument_index == 0
        {
            return template_context(content, quote_start, cursor);
        }
    }

    if is_twig_uri(uri)
        && let Some((quote_start, _)) = opening_quote(content, cursor)
    {
        let statement = content[line_start..quote_start].trim_end();
        let keyword = statement
            .rsplit_once("{%")
            .map(|(_, tail)| tail.trim_start())
            .and_then(|tail| tail.split_whitespace().next())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(
            keyword.as_str(),
            "extends" | "include" | "embed" | "use" | "import" | "from"
        ) {
            return template_context(content, quote_start, cursor);
        }
    }

    if let Some(percent) = prefix.rfind('%')
        && !prefix[percent + 1..].contains('%')
    {
        return Some(SymfonyCompletionContext {
            kind: SymfonySymbolKind::Parameter,
            prefix: prefix[percent + 1..].to_string(),
            content_start: line_start + percent + 1,
            escape_backslashes: false,
            route_name: None,
        });
    }

    if let Some(at) = prefix.rfind('@') {
        let typed = prefix[at + 1..].trim_start_matches(['?', '!']);
        let adjust = prefix[at + 1..].len() - typed.len();
        if typed.bytes().all(is_symbol_char) {
            return Some(SymfonyCompletionContext {
                kind: SymfonySymbolKind::Service,
                prefix: typed.to_string(),
                content_start: line_start + at + 1 + adjust,
                escape_backslashes: false,
                route_name: None,
            });
        }
    }

    let lower = prefix.to_ascii_lowercase();
    let service_attribute = ["alias=\"", "decorates=\"", "parent=\"", "service=\""]
        .iter()
        .find_map(|needle| lower.rfind(needle).map(|start| (needle.len(), start)));
    if let Some((needle_len, start)) = service_attribute {
        let typed_start = start + needle_len;
        let typed = &prefix[typed_start..];
        if !typed.contains('"') && typed.bytes().all(is_symbol_char) {
            return Some(SymfonyCompletionContext {
                kind: SymfonySymbolKind::Service,
                prefix: typed.to_string(),
                content_start: line_start + typed_start,
                escape_backslashes: false,
                route_name: None,
            });
        }
    }

    None
}

fn template_context(
    content: &str,
    quote_start: usize,
    cursor: usize,
) -> Option<SymfonyCompletionContext> {
    Some(SymfonyCompletionContext {
        kind: SymfonySymbolKind::Template,
        prefix: content.get(quote_start + 1..cursor)?.to_string(),
        content_start: quote_start + 1,
        escape_backslashes: false,
        route_name: None,
    })
}

fn is_twig_uri(uri: &str) -> bool {
    uri.split('?')
        .next()
        .is_some_and(|path| path.to_ascii_lowercase().ends_with(".twig"))
}

fn opening_quote(content: &str, cursor: usize) -> Option<(usize, u8)> {
    let bytes = content.as_bytes();
    let mut index = cursor;
    while index > 0 {
        index -= 1;
        let byte = bytes[index];
        if byte == b'\n' || byte == b'\r' {
            return None;
        }
        if matches!(byte, b'\'' | b'"') {
            let mut backslashes = 0usize;
            let mut previous = index;
            while previous > 0 && bytes[previous - 1] == b'\\' {
                previous -= 1;
                backslashes += 1;
            }
            if backslashes.is_multiple_of(2) {
                return Some((index, byte));
            }
        }
    }
    None
}

fn php_call_context(content: &str, quote_start: usize) -> Option<(&str, usize, usize)> {
    let search_start = quote_start.saturating_sub(2048);
    let open = content[search_start..quote_start].rfind('(')? + search_start;
    let bytes = content.as_bytes();
    let mut name_end = open;
    while name_end > 0 && bytes[name_end - 1].is_ascii_whitespace() {
        name_end -= 1;
    }
    let mut name_start = name_end;
    while name_start > 0 && is_identifier_char(bytes[name_start - 1]) {
        name_start -= 1;
    }
    if name_start == name_end {
        return None;
    }

    let mut argument_index = 0usize;
    let mut depth = 0u32;
    for byte in bytes[open + 1..quote_start].iter().copied() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => argument_index += 1,
            _ => {}
        }
    }
    Some((&content[name_start..name_end], argument_index, open + 1))
}

fn named_argument_before(content: &str, args_start: usize, quote_start: usize) -> Option<&str> {
    let segment = content[args_start..quote_start]
        .rsplit_once(',')
        .map_or(&content[args_start..quote_start], |(_, tail)| tail)
        .trim();
    let colon = segment.rfind(':')?;
    let name = segment[..colon].trim();
    (!name.is_empty() && name.bytes().all(is_identifier_char)).then_some(name)
}

fn looks_like_container_call(content: &str, quote_start: usize) -> bool {
    let start = quote_start.saturating_sub(160);
    let prefix = &content[start..quote_start];
    if prefix.contains("$container->")
        || prefix.contains("$serviceLocator->")
        || prefix.contains("$locator->")
        || prefix.contains("container->")
    {
        return true;
    }

    let Some(arrow) = prefix.rfind("->") else {
        return false;
    };
    let receiver_prefix = prefix[..arrow].trim_end();
    let receiver_start = receiver_prefix
        .rfind(|character: char| {
            !(character == '$' || character == '_' || character.is_ascii_alphanumeric())
        })
        .map_or(0, |index| index + 1);
    let receiver = &receiver_prefix[receiver_start..];
    !receiver.is_empty()
        && [
            format!("ContainerInterface {receiver}"),
            format!("ServiceLocator {receiver}"),
            format!("ContainerBagInterface {receiver}"),
        ]
        .iter()
        .any(|typed| content.contains(typed))
}

fn looks_like_route_generator_call(content: &str, quote_start: usize) -> bool {
    let start = quote_start.saturating_sub(192);
    let prefix = &content[start..quote_start];
    prefix.contains("$router->generate(")
        || prefix.contains("$urlGenerator->generate(")
        || content.contains("UrlGeneratorInterface")
        || content.contains("RouterInterface")
}

fn is_route_reference_call(call_name: &str, content: &str, quote_start: usize) -> bool {
    matches!(call_name, "generateurl" | "redirecttoroute")
        || (call_name == "generate" && looks_like_route_generator_call(content, quote_start))
}

fn first_string_argument(content: &str, args_start: usize, before: usize) -> Option<String> {
    let bytes = content.as_bytes();
    let mut cursor = args_start;
    while cursor < before && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let quote @ (b'\'' | b'"') = bytes.get(cursor).copied()? else {
        return None;
    };
    cursor += 1;
    let start = cursor;
    while cursor < before {
        if bytes[cursor] == b'\\' {
            cursor = (cursor + 2).min(before);
            continue;
        }
        if bytes[cursor] == quote {
            return Some(content[start..cursor].replace("\\\\", "\\"));
        }
        cursor += 1;
    }
    None
}

fn is_identifier_char(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn is_symbol_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b':' | b'/' | b'\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_container_service_call() {
        let php = "<?php\nfunction f(ContainerInterface $container) { $container->get('app.'); }\n";
        let offset = php.find("app.").unwrap() + 4;
        let position = offset_to_position(php, offset);
        let context = detect_php_context(php, position).unwrap();
        assert_eq!(context.kind, SymfonySymbolKind::Service);
        assert_eq!(context.prefix, "app.");
    }

    #[test]
    fn detects_autowire_parameter() {
        let php = "<?php\n#[Autowire(param: 'app.na')]\n";
        let offset = php.find("app.na").unwrap() + 6;
        let position = offset_to_position(php, offset);
        let context = detect_php_context(php, position).unwrap();
        assert_eq!(context.kind, SymfonySymbolKind::Parameter);
        assert_eq!(context.prefix, "app.na");
    }

    #[test]
    fn detects_yaml_service_reference() {
        let yaml = "services:\n  app.foo:\n    arguments: ['@app.ba']\n";
        let offset = yaml.find("app.ba").unwrap() + 6;
        let position = offset_to_position(yaml, offset);
        let context =
            detect_resource_context("file:///config/services.yaml", yaml, position).unwrap();
        assert_eq!(context.kind, SymfonySymbolKind::Service);
        assert_eq!(context.prefix, "app.ba");
    }
}
