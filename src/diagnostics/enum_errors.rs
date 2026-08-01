use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolKind;
use crate::types::{ClassInfo, ClassLikeKind, ConstantInfo};

use super::helpers::{FileDiagnosticContext, make_diagnostic};

impl Backend {
    pub fn collect_enum_error_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(ctx) = FileDiagnosticContext::gather(self, uri) else {
            return;
        };
        self.collect_enum_error_diagnostics_with_context(&ctx, uri, content, out);
    }

    pub(crate) fn collect_enum_error_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        for span in &ctx.symbol_map.spans {
            let class_name = match &span.kind {
                SymbolKind::ClassDeclaration { name } => name,
                _ => continue,
            };

            let class_info = match find_class(&ctx.file.classes, class_name, &ctx.file.namespace) {
                Some(c) => c,
                None => continue,
            };

            if class_info.kind != ClassLikeKind::Enum {
                continue;
            }

            if detect_invalid_backing_type(self, uri, content, span.end as usize, out) {
                continue;
            }

            let enum_cases: Vec<_> = class_info
                .constants
                .iter()
                .filter(|c| c.is_enum_case)
                .collect();

            if enum_cases.is_empty() {
                continue;
            }

            let backed = class_info.backed_type.is_some();

            for case in &enum_cases {
                if backed && case.enum_value.is_none() {
                    let range = match self.offset_range_to_lsp_range(
                        uri,
                        content,
                        case.name_offset as usize,
                        case.name_offset as usize + case.name.len(),
                    ) {
                        Some(r) => r,
                        None => continue,
                    };
                    out.push(make_diagnostic(
                        range,
                        DiagnosticSeverity::ERROR,
                        "invalid_enum_case",
                        format!(
                            "Enum case '{}::{}' must have a value, enum '{}' is a backed enum",
                            class_info.name, case.name, class_info.name
                        ),
                    ));
                } else if !backed && case.enum_value.is_some() {
                    let range = match self.offset_range_to_lsp_range(
                        uri,
                        content,
                        case.name_offset as usize,
                        case.name_offset as usize + case.name.len(),
                    ) {
                        Some(r) => r,
                        None => continue,
                    };
                    out.push(make_diagnostic(
                        range,
                        DiagnosticSeverity::ERROR,
                        "invalid_enum_case",
                        format!(
                            "Enum case '{}::{}' must not have a value, enum '{}' is not a backed enum",
                            class_info.name, case.name, class_info.name
                        ),
                    ));
                }
            }

            if backed {
                check_duplicate_values(self, uri, content, class_info, &enum_cases, out);
            }
        }
    }
}

fn find_class<'a>(
    classes: &'a [std::sync::Arc<ClassInfo>],
    name: &str,
    namespace: &Option<String>,
) -> Option<&'a std::sync::Arc<ClassInfo>> {
    classes.iter().find(|c| {
        if c.name == name {
            return true;
        }
        if let Some(ns) = namespace {
            let fqn = format!("{}\\{}", ns, c.name);
            fqn == name
        } else {
            false
        }
    })
}

fn detect_invalid_backing_type(
    backend: &Backend,
    uri: &str,
    content: &str,
    name_end: usize,
    out: &mut Vec<Diagnostic>,
) -> bool {
    let search_end = (name_end + 150).min(content.len());
    let snippet = &content[name_end..search_end];

    let colon_pos = match snippet.find(':') {
        Some(p) => p,
        None => return false,
    };

    let brace_pos = snippet.find('{').unwrap_or(snippet.len());
    if colon_pos >= brace_pos {
        return false;
    }

    let after_colon = &snippet[colon_pos + 1..brace_pos];
    let type_name = after_colon.split_whitespace().next().unwrap_or("");

    if type_name.is_empty()
        || type_name.eq_ignore_ascii_case("int")
        || type_name.eq_ignore_ascii_case("string")
    {
        return false;
    }

    let leading_ws = after_colon.len() - after_colon.trim_start().len();
    let type_start = name_end + colon_pos + 1 + leading_ws;
    let type_end = type_start + type_name.len();

    let range = match backend.offset_range_to_lsp_range(uri, content, type_start, type_end) {
        Some(r) => r,
        None => return false,
    };

    out.push(make_diagnostic(
        range,
        DiagnosticSeverity::ERROR,
        "invalid_enum_backing_type",
        format!(
            "Enum backing type must be 'int' or 'string', got '{}'",
            type_name
        ),
    ));
    true
}

fn check_duplicate_values(
    backend: &Backend,
    uri: &str,
    content: &str,
    class_info: &ClassInfo,
    cases: &[&std::sync::Arc<ConstantInfo>],
    out: &mut Vec<Diagnostic>,
) {
    let mut seen: Vec<(&str, &str)> = Vec::new();

    for case in cases {
        let Some(ref value) = case.enum_value else {
            continue;
        };

        if let Some((first_name, _)) = seen.iter().find(|(_, v)| *v == value.as_str()) {
            let range = match backend.offset_range_to_lsp_range(
                uri,
                content,
                case.name_offset as usize,
                case.name_offset as usize + case.name.len(),
            ) {
                Some(r) => r,
                None => continue,
            };
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::ERROR,
                "invalid_enum_case",
                format!(
                    "Duplicate value {} in enum '{}': case '{}' and case '{}' share the same value",
                    value, class_info.name, case.name, first_name
                ),
            ));
        } else {
            seen.push((&case.name, value));
        }
    }
}
