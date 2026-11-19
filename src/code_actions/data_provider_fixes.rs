//! Quick-fixes for a PHPUnit data provider: add the `public` or `static`
//! modifier the `invalid_phpunit_data_provider` diagnostic reports missing, and
//! create a provider the test names but does not declare.
//!
//! Both edit this file only: a provider declared in another file (an
//! external provider, or one inherited from a base test case) is reported
//! but not fixed here.

use mago_span::HasSpan;
use mago_syntax::cst::modifier::Modifier;
use tower_lsp_server::gen_lsp_types::*;

use super::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::Backend;
use crate::class_lookup::find_class_at_offset;
use crate::diagnostics::data_providers::{MissingModifier, ProviderFinding, ProviderProblem};
use crate::diagnostics::helpers::FileDiagnosticContext;
use crate::signature_render::detect_class_indent;
use crate::symbol_map::{DocblockMemberRef, SymbolKind};
use crate::text_position::{find_line_start, offset_to_position, ranges_overlap};

impl Backend {
    pub(crate) fn collect_data_provider_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        let Ok(doc_uri) = uri.parse::<Uri>() else {
            return;
        };
        let start = crate::text_position::position_to_offset(content, params.range.start);
        let end = crate::text_position::position_to_offset(content, params.range.end);
        // Most files name no data provider; settle that before gathering
        // the file's context.
        let names_provider = self.symbol_map_for(uri).is_some_and(|map| {
            map.spans.iter().any(|span| {
                span.start <= end
                    && span.end >= start
                    && matches!(
                        span.kind,
                        SymbolKind::MemberAccess {
                            docblock_ref: DocblockMemberRef::PhpUnitDataProvider { .. },
                            ..
                        }
                    )
            })
        });
        if !names_provider {
            return;
        }
        let ctx = FileDiagnosticContext::gather(self, uri);
        // Only the references the request touches are resolved; the exact
        // overlap is checked on the LSP range below.
        let findings = self.data_provider_findings(&ctx, content, |span_start, span_end| {
            span_start <= end && span_end >= start
        });

        for finding in &findings {
            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                finding.start as usize,
                finding.end as usize,
            ) else {
                continue;
            };
            // A bare cursor touching either end of the name is on it.
            let touches = if params.range.start == params.range.end {
                range.start <= params.range.start && params.range.start <= range.end
            } else {
                ranges_overlap(&range, &params.range)
            };
            if !touches {
                continue;
            }
            let (title, edit, diagnostic) = match &finding.problem {
                ProviderProblem::Modifier {
                    missing,
                    local_name_offset: Some(name_offset),
                    ..
                } => {
                    let Some(edit) = modifier_edit(content, *name_offset, *missing) else {
                        continue;
                    };
                    (
                        format!(
                            "Make data provider '{}' {}",
                            finding.method_name,
                            missing.keyword()
                        ),
                        edit,
                        self.data_provider_diagnostic(uri, content, finding),
                    )
                }
                ProviderProblem::Missing { class_end } => {
                    let Some(class) = find_class_at_offset(&ctx.file.classes, finding.start) else {
                        continue;
                    };
                    (
                        format!("Create data provider '{}'", finding.method_name),
                        create_provider_edit(content, class, *class_end, finding),
                        None,
                    )
                }
                ProviderProblem::Modifier { .. } => continue,
            };
            out.push(CodeActionResponse::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QuickFix),
                diagnostics: diagnostic.map(|diagnostic| vec![diagnostic]),
                edit: Some(super::single_file_edit(doc_uri.clone(), vec![edit])),
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: None,
                tags: None,
            }));
        }
    }
}

/// The edit that gives the method whose name starts at `name_offset` the
/// `missing` modifier.
fn modifier_edit(content: &str, name_offset: u32, missing: MissingModifier) -> Option<TextEdit> {
    let (start, end, new_text) = crate::parser::with_parsed_program(
        content,
        "data_provider_modifier",
        |program, _| -> Option<(u32, u32, String)> {
            let CursorContext::InClassLike {
                member: MemberContext::Method(method, _),
                ..
            } = find_cursor_context(&program.statements, name_offset)
            else {
                return None;
            };
            if method.name.span.start.offset != name_offset {
                return None;
            }
            let visibility = method.modifiers.iter().find_map(|modifier| match modifier {
                Modifier::Public(keyword)
                | Modifier::Protected(keyword)
                | Modifier::Private(keyword) => Some(keyword.span),
                _ => None,
            });
            match missing {
                MissingModifier::Public => {
                    let span = visibility?;
                    Some((span.start.offset, span.end.offset, "public".to_owned()))
                }
                // `public static function`, the order PSR-12 asks for.
                MissingModifier::Static => Some(match visibility {
                    Some(span) => (span.end.offset, span.end.offset, " static".to_owned()),
                    None => {
                        let at = method.function.span().start.offset;
                        (at, at, "static ".to_owned())
                    }
                }),
            }
        },
    )?;
    Some(TextEdit {
        range: Range {
            start: offset_to_position(content, start as usize),
            end: offset_to_position(content, end as usize),
        },
        new_text,
    })
}

/// The edit that declares the provider `finding` names as the last member
/// of `class`, whose closing brace is at `class_end`.
fn create_provider_edit(
    content: &str,
    class: &crate::types::ClassInfo,
    class_end: u32,
    finding: &ProviderFinding,
) -> TextEdit {
    let indent = detect_class_indent(content, class);
    let unit = super::indent_unit(content);
    let brace = (class_end as usize).min(content.len());
    // Keep the closing brace's own indentation by inserting at the start
    // of its line, when nothing else shares that line.
    let line_start = find_line_start(content, brace);
    let (at, lead) = if content[line_start..brace].trim().is_empty() {
        (line_start, "")
    } else {
        (brace, "\n")
    };
    let new_text = format!(
        "{lead}\n{indent}public static function {name}(): iterable\n{indent}{{\n{indent}{unit}return [];\n{indent}}}\n",
        name = finding.method_name,
    );
    let position = offset_to_position(content, at);
    TextEdit {
        range: Range {
            start: position,
            end: position,
        },
        new_text,
    }
}
