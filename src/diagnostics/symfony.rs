//! Conservative diagnostics for project-local Symfony container symbols.

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Range};

use crate::Backend;
use crate::framework::{FrameworkReferenceKind, SymfonySymbolKind};
use crate::text_position::offset_to_position;

impl Backend {
    pub(crate) fn collect_unknown_symfony_container_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(references) = self.framework_references.read().get(uri).cloned() else {
            return;
        };
        // The workspace's declarations are only gathered for a file that
        // uses a name, and in one pass over the index for every kind.
        let uses_a_name = references.iter().any(|reference| {
            matches!(
                reference.kind,
                FrameworkReferenceKind::SymfonySymbol {
                    declaration: false,
                    ..
                }
            )
        });
        if !uses_a_name {
            return;
        }
        let declared = self.framework_declared_symfony_symbols();
        let is_declared = |kind: SymfonySymbolKind, name: &str| {
            declared
                .get(&kind)
                .is_some_and(|names| names.contains(name))
        };

        for reference in references.iter() {
            let FrameworkReferenceKind::SymfonySymbol {
                kind,
                name,
                declaration: false,
            } = &reference.kind
            else {
                continue;
            };

            let known = match kind {
                SymfonySymbolKind::Service => {
                    is_declared(*kind, name)
                        || (name.starts_with("App\\") && self.find_or_load_class(name).is_some())
                }
                SymfonySymbolKind::Parameter => is_declared(*kind, name),
            };
            if known || !is_project_local_name(*kind, name) {
                continue;
            }

            let label = kind.label();
            out.push(Diagnostic {
                range: Range {
                    start: offset_to_position(content, reference.start as usize),
                    end: offset_to_position(content, reference.end as usize),
                },
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String(format!("unknown_symfony_{label}"))),
                source: Some("PHPantom".to_string()),
                message: format!("Symfony {label} '{}' is not declared", name),
                ..Default::default()
            });
        }
    }
}

fn is_project_local_name(kind: SymfonySymbolKind, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("app.") || (kind == SymfonySymbolKind::Service && name.starts_with("App\\"))
}
