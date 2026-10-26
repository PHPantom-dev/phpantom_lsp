use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::diagnostics::helpers::make_diagnostic;
use crate::diagnostics::namespace_mismatch::psr4_expectation;
use crate::text_position::byte_range_to_lsp_range;
use crate::types::ClassInfo;

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
        DiagnosticSeverity::Warning,
        "class_name_mismatch",
        format!(
            "Class name `{}` does not match filename `{expected_name}`",
            class.name
        ),
    ))
}

/// The range of `class`'s name in its declaration.
pub(crate) fn class_name_range(content: &str, class: &ClassInfo) -> Option<Range> {
    let (name_start, name_end) =
        crate::parser::with_parsed_program(content, "class_name_range", |program, _| {
            use mago_syntax::cst::Node;
            crate::parser::innermost_node_at(Node::Program(program), class.keyword_offset, |node| {
                let name = match node {
                    Node::Class(class) => class.name.span,
                    Node::Interface(interface) => interface.name.span,
                    Node::Trait(trait_) => trait_.name.span,
                    Node::Enum(enum_) => enum_.name.span,
                    _ => return None,
                };
                Some((name.start.offset as usize, name.end.offset as usize))
            })
        })?;
    Some(byte_range_to_lsp_range(content, name_start, name_end))
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
