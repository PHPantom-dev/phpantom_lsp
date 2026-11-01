//! Create the registration named by an unresolved Laravel route reference.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::names::OwnedResolvedNames;
use crate::symbol_map::{StringKeyKind, SymbolKind};
use crate::text_position::{byte_range_to_lsp_range, offset_to_position, ranges_overlap};
use crate::virtual_members::laravel::singularize_english_word;

impl Backend {
    /// Offer a route registration for an unknown route diagnosed at the cursor.
    pub(crate) fn collect_create_missing_route_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        if !self.string_key_kind_enabled(StringKeyKind::Route) {
            return;
        }
        let diagnostics: Vec<_> = params.context.diagnostics.iter().filter_map(|diagnostic| {
            if !matches!(&diagnostic.code, Some(Code::String(code)) if code == "invalid_laravel_route") {
                return None;
            }
            let range = self.translate_blade_range_to_php(uri, diagnostic.range);
            ranges_overlap(&range, &params.range).then_some((diagnostic, range))
        }).collect();
        if diagnostics.is_empty() {
            return;
        }
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return;
        };
        let Some(source_path) =
            crate::util::uri_to_path(uri).filter(|path| path.starts_with(&root))
        else {
            return;
        };
        let Some(state) = self.file_state(uri) else {
            return;
        };
        let Some(symbol_map) = state.symbol_map.as_ref().filter(|map| {
            map.matches_source(content) && (map.source_hash.is_none() || map.built_from(content))
        }) else {
            return;
        };
        let routes = self.cached_routes();
        for span in &symbol_map.spans {
            let SymbolKind::StringKey {
                kind: StringKeyKind::Route,
                key,
                is_write: false,
                is_optional: false,
            } = &span.kind
            else {
                continue;
            };
            if key.is_empty()
                || key.contains('*')
                || routes
                    .names
                    .binary_search_by(|name| name.as_str().cmp(key))
                    .is_ok()
            {
                continue;
            }
            let literal = content
                .get(span.start.saturating_sub(1) as usize..span.end as usize + 1)
                .and_then(crate::text_scan::decode_php_string_literal);
            if literal.as_deref() != Some(key.as_str()) {
                continue;
            }
            let range = byte_range_to_lsp_range(content, span.start as usize, span.end as usize);
            let Some((diagnostic, _)) = diagnostics
                .iter()
                .find(|(_, diagnostic_range)| ranges_overlap(&range, diagnostic_range))
            else {
                continue;
            };
            let api = source_path == root.join("routes/api.php")
                || api_resource_context(
                    content,
                    span.start,
                    state.resolved_names.as_deref(),
                    || !self.has_indexed_class("Route"),
                );
            let target_path = root.join(if api {
                "routes/api.php"
            } else {
                "routes/web.php"
            });
            let Some(target) = crate::util::path_to_url(&target_path) else {
                continue;
            };
            let existing = self.get_file_content_arc(target.as_str());
            if existing.is_none() && (target_path.exists() || !self.caps().file_create) {
                continue;
            }
            let source = existing.as_deref().map_or("", String::as_str);
            let Some((edit, position)) = route_insertion(source, &route_stub(key)) else {
                continue;
            };
            let edit = if existing.is_some() {
                super::single_file_edit(target.clone(), vec![edit])
            } else {
                super::create_file_edit(target.clone(), edit.new_text, Vec::new())
            };
            out.push(CodeActionResponse::CodeAction(CodeAction {
                title: format!("Create missing route '{key}'"),
                kind: Some(CodeActionKind::QuickFix),
                diagnostics: Some(vec![(*diagnostic).clone()]),
                edit: Some(edit),
                command: Some(self.build_code_lens_command(
                    "Open new route".into(),
                    target,
                    position,
                )),
                ..Default::default()
            }));
        }
    }
}

fn api_resource_context(
    content: &str,
    offset: u32,
    names: Option<&OwnedResolvedNames>,
    root_alias_available: impl Fn() -> bool,
) -> bool {
    let Some(names) = names else {
        return false;
    };
    crate::parser::with_parsed_program(content, "create_missing_route", |program, _| {
        super::cursor_context::innermost_at(program, offset, |node| {
            let Node::Expression(mut expression) = node else {
                return None;
            };
            loop {
                expression = match expression {
                    Expression::Call(Call::Method(call)) => call.object,
                    Expression::Parenthesized(parenthesized) => parenthesized.expression,
                    Expression::Call(Call::StaticMethod(call)) => {
                        let ClassLikeMemberSelector::Identifier(method) = &call.method else {
                            return None;
                        };
                        let method = crate::atom::bytes_to_str(method.value);
                        if !method.eq_ignore_ascii_case("apiResource")
                            && !method.eq_ignore_ascii_case("apiResources")
                        {
                            return None;
                        }
                        let class = names.get(call.class.span().start.offset)?;
                        return (class.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Route")
                            || class.eq_ignore_ascii_case("Route") && root_alias_available())
                        .then_some(());
                    }
                    _ => return None,
                };
            }
        })
        .is_some()
    })
}

fn route_stub(name: &str) -> String {
    let mut path = String::from("/path");
    let mut controller = String::from("\\App\\Http\\Controllers\\PlaceholderController");
    let mut method = String::from("method");
    let parts: Vec<_> = name.split('.').collect();
    let conventional = parts.iter().all(|part| {
        part.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    });
    if conventional {
        path = format!("/{}", parts.join("/"));
        if parts.len() > 1 {
            let action = parts[parts.len() - 1];
            let resource = parts[parts.len() - 2];
            let singular = singularize_english_word(resource);
            let singular = if singular.is_empty() {
                resource
            } else {
                &singular
            };
            controller = String::from("\\App\\Http\\Controllers\\");
            for namespace in &parts[..parts.len() - 2] {
                controller.push_str(&identifier(namespace));
                controller.push('\\');
            }
            controller.push_str(&identifier(singular));
            controller.push_str("Controller");
            method = identifier(action);
            method[..1].make_ascii_lowercase();
            let base = parts[..parts.len() - 1].join("/");
            path = match action {
                "index" | "store" => format!("/{base}"),
                "show" | "update" | "destroy" => {
                    format!("/{base}/{{{}}}", singular.replace('-', "_"))
                }
                "edit" => format!("/{base}/{{{}}}/edit", singular.replace('-', "_")),
                _ => path,
            };
        }
    }
    // Keep literal line breaks out of edits: their line-ending conversion
    // must not change the route's name.
    let escaped = name
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\r', "' . \"\\r\" . '")
        .replace('\n', "' . \"\\n\" . '");
    format!(
        "\\Illuminate\\Support\\Facades\\Route::get('{path}', [{controller}::class, '{method}'])->name('{escaped}');"
    )
}

fn identifier(value: &str) -> String {
    value
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(super::naming::capitalise)
        .collect()
}

/// Keep registrations inside the final PHP/namespace block, ahead of a
/// closing tag; a closing tag may itself have terminated the last statement.
fn insertion_site(statements: &[Statement<'_>], end: usize) -> Option<(usize, bool, bool)> {
    if statements
        .iter()
        .any(|statement| matches!(statement, Statement::HaltCompiler(_) | Statement::Return(_)))
    {
        return None;
    }
    if let Some(Statement::Namespace(namespace)) = statements
        .iter()
        .rev()
        .find(|statement| !matches!(statement, Statement::Inline(_) | Statement::ClosingTag(_)))
    {
        return match &namespace.body {
            NamespaceBody::Implicit(body)
                if body
                    .statements
                    .iter()
                    .all(|statement| matches!(statement, Statement::Inline(_)))
                    && body.terminator.is_closing_tag() =>
            {
                Some((body.terminator.span().start.offset as usize, true, false))
            }
            NamespaceBody::Implicit(body) => insertion_site(body.statements.as_slice(), end),
            NamespaceBody::BraceDelimited(body) => insertion_site(
                body.statements.as_slice(),
                body.right_brace.start.offset as usize,
            )
            .map(|(offset, semicolon, _)| (offset, semicolon, true)),
        };
    }
    let last = statements
        .iter()
        .rev()
        .find(|statement| !matches!(statement, Statement::Inline(_)));
    match last {
        Some(statement) if statement.terminates_scripting() => Some((
            statement.span().end.offset as usize - 2,
            !statement.is_closing_tag(),
            false,
        )),
        _ => Some((end, false, false)),
    }
}

fn route_insertion(content: &str, stub: &str) -> Option<(TextEdit, Position)> {
    if content.trim().is_empty() {
        let position = offset_to_position(content, content.len());
        return Some((
            TextEdit {
                range: Range::new(position, position),
                new_text: format!("<?php\n\n{stub}\n"),
            },
            Position::new(position.line + 2, 0),
        ));
    }
    crate::parser::with_parsed_program(content, "create_missing_route_target", |program, _| {
        if !program.errors.is_empty()
            || !program
                .statements
                .iter()
                .any(|statement| matches!(statement, Statement::OpeningTag(_)))
        {
            return None;
        }
        let (offset, semicolon, namespace) =
            insertion_site(program.statements.as_slice(), content.len())?;
        let before = &content[..offset];
        let separator = if before.ends_with("\n\n") {
            ""
        } else if before.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        let indent = if namespace {
            super::indent_unit(content)
        } else {
            ""
        };
        let prefix = format!("{}{separator}{indent}", if semicolon { ";" } else { "" });
        let position = offset_to_position(content, offset);
        let line = position.line + prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
        Some((
            TextEdit {
                range: Range::new(position, position),
                new_text: format!("{prefix}{stub}\n"),
            },
            Position::new(line, indent.len() as u32),
        ))
    })
}

#[cfg(test)]
#[path = "create_missing_route_tests.rs"]
mod tests;
