use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::composer;

impl Backend {
    pub fn collect_namespace_mismatch_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(diag) = namespace_mismatch_diagnostic(self, uri, content) else {
            return;
        };
        out.push(diag);
    }
}

pub(crate) fn namespace_mismatch_diagnostic(
    backend: &Backend,
    uri: &str,
    content: &str,
) -> Option<Diagnostic> {
    let workspace_root = backend.workspace_root().read().clone()?;
    let file_path = Url::parse(uri).ok().and_then(|u| u.to_file_path().ok())?;

    let mappings = backend.psr4_mappings().read().clone();
    if mappings.is_empty() {
        return None;
    }

    let (expected_ns, _) =
        composer::resolve_namespace_from_path(&mappings, &workspace_root, &file_path)?;

    let (actual_ns, range) = namespace_decl_from_content(content)?;

    if expected_ns.as_deref() == actual_ns.as_deref() {
        return None;
    }

    let expected_display = expected_ns.as_deref().unwrap_or("<global>");
    let actual_display = actual_ns.as_deref().unwrap_or("<global>");

    Some(Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String("namespace_mismatch".to_string())),
        source: Some("phpantom".to_string()),
        message: format!(
            "Namespace `{}` does not match PSR-4 expected `{}`",
            actual_display, expected_display,
        ),
        ..Default::default()
    })
}

pub(crate) fn namespace_decl_from_content(content: &str) -> Option<(Option<String>, Range)> {
    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("namespace ") {
            continue;
        }

        let indent = line.len() - trimmed.len();
        let name_start_char = indent + "namespace ".len();
        let rest = &trimmed["namespace ".len()..];
        let name_len = rest.find([';', '{']).unwrap_or(rest.len());
        let ns = rest[..name_len].trim().to_string();
        let leading_ws = rest[..name_len].len() - rest[..name_len].trim_start().len();
        let start_char = (name_start_char + leading_ws) as u32;
        let end_char = start_char + ns.len() as u32;

        return Some((
            if ns.is_empty() { None } else { Some(ns) },
            Range {
                start: Position {
                    line: line_idx as u32,
                    character: start_char,
                },
                end: Position {
                    line: line_idx as u32,
                    character: end_char,
                },
            },
        ));
    }

    Some((
        None,
        Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        },
    ))
}
