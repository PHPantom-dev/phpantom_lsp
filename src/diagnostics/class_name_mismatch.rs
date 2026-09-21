use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::diagnostics::helpers::make_diagnostic;
use crate::diagnostics::namespace_mismatch::psr4_expectation;
use crate::text_position::offset_to_position;
use crate::types::{ClassInfo, ClassLikeKind};

impl Backend {
    pub fn collect_class_name_mismatch_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(diag) = class_name_mismatch_diagnostic(self, uri, content) else {
            return;
        };
        out.push(diag);
    }
}

pub(crate) fn class_name_mismatch_diagnostic(
    backend: &Backend,
    uri: &str,
    content: &str,
) -> Option<Diagnostic> {
    // Only files that fall under a PSR-4 mapping are required to name
    // their single class after the file, which is what the shared gate
    // establishes; see [`psr4_expectation`].
    let (_, expected_name) = psr4_expectation(backend, uri, content)?;

    let classes = backend.parse_php(content);
    if classes.len() != 1 {
        return None;
    }
    let class = &classes[0];
    if class.name == expected_name {
        return None;
    }

    let range = class_name_range(content, class)?;

    Some(make_diagnostic(
        range,
        DiagnosticSeverity::WARNING,
        "class_name_mismatch",
        format!(
            "Class name `{}` does not match filename `{expected_name}`",
            class.name
        ),
    ))
}

pub(crate) fn class_name_range(content: &str, class: &ClassInfo) -> Option<Range> {
    let keyword = match class.kind {
        ClassLikeKind::Class => "class",
        ClassLikeKind::Interface => "interface",
        ClassLikeKind::Trait => "trait",
        ClassLikeKind::Enum => "enum",
    };
    let kw_off = class.keyword_offset as usize;
    let slice = content.get(kw_off..)?;
    let after_kw = slice.strip_prefix(keyword)?;
    let ws = after_kw.len() - after_kw.trim_start().len();
    let name_start = kw_off + keyword.len() + ws;
    let name_end = name_start + class.name.len();
    Some(Range {
        start: offset_to_position(content, name_start),
        end: offset_to_position(content, name_end),
    })
}

#[cfg(test)]
mod tests {
    use super::class_name_mismatch_diagnostic;
    use crate::Backend;
    use crate::composer::Psr4Mapping;
    use std::path::PathBuf;

    #[test]
    fn class_name_mismatch_skipped_for_inline_fixture_file() {
        let backend = Backend::new_test_with_workspace(
            PathBuf::from("/project"),
            vec![Psr4Mapping {
                prefix: "App\\Models\\".to_string(),
                base_path: "app/Models/".to_string(),
            }],
        );
        let uri = "file:///project/app/Models/ExampleState.php";
        let php = "<?php\nnamespace App\\Models;\n\nit('demo', function (): void {});\n\nenum WrongName: int {\n    case One = 1;\n}\n";

        assert!(class_name_mismatch_diagnostic(&backend, uri, php).is_none());
    }
}
