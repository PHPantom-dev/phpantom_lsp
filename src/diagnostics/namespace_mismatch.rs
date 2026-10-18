use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::composer;
use crate::parser::with_parsed_program;

use super::helpers::make_diagnostic;

use mago_span::HasSpan;
use mago_syntax::cst::*;

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
    let (expected_ns, _) = psr4_expectation(backend, uri, content)?;

    let (actual_ns, range) = namespace_decl_from_content(content)?;

    if expected_ns.as_deref() == actual_ns.as_deref() {
        return None;
    }

    let expected_display = expected_ns.as_deref().unwrap_or("<global>");
    let actual_display = actual_ns.as_deref().unwrap_or("<global>");

    Some(make_diagnostic(
        range,
        DiagnosticSeverity::WARNING,
        "namespace_mismatch",
        format!("Namespace `{actual_display}` does not match PSR-4 expected `{expected_display}`"),
    ))
}

/// What PSR-4 expects of `uri`: the namespace its directory maps to and
/// the class name its filename maps to.
///
/// `None` when the file is exempt from both checks: it declares more
/// than the one class-like they are about, it is not a file on disk, the
/// project has no PSR-4 mappings (a standalone script, or no
/// `composer.json` at all), or no mapping covers its path.
pub(crate) fn psr4_expectation(
    backend: &Backend,
    uri: &str,
    content: &str,
) -> Option<(Option<String>, String)> {
    if !is_structural_single_classlike_file(content) {
        return None;
    }

    let workspace_root = backend.workspace_root().read().clone()?;
    let file_path = Url::parse(uri).ok().and_then(|u| u.to_file_path().ok())?;

    let mappings = backend.psr4_mappings().read().clone();
    if mappings.is_empty() {
        return None;
    }

    composer::resolve_namespace_from_path(&mappings, &workspace_root, &file_path)
}

/// The namespace the file's first `namespace` declaration names, with the
/// range of its name, or `None` with an empty range at the start of the
/// file when it declares none (an unnamed `namespace { }` block names none
/// either, with the range just past its keyword).
pub(crate) fn namespace_decl_from_content(content: &str) -> Option<(Option<String>, Range)> {
    with_parsed_program(content, "namespace_decl", |program, _| {
        let namespace = program.statements.iter().find_map(|stmt| match stmt {
            Statement::Namespace(namespace) => Some(namespace),
            _ => None,
        });
        Some(match namespace {
            Some(namespace) => match &namespace.name {
                Some(name) => {
                    let span = name.span();
                    (
                        Some(crate::atom::bytes_to_str(name.value()).to_string()),
                        crate::text_position::byte_range_to_lsp_range(
                            content,
                            span.start.offset as usize,
                            span.end.offset as usize,
                        ),
                    )
                }
                None => {
                    let at = crate::text_position::offset_to_position(
                        content,
                        namespace.namespace.span().end.offset as usize,
                    );
                    (None, Range { start: at, end: at })
                }
            },
            None => (None, Range::default()),
        })
    })
}

pub(crate) fn is_structural_single_classlike_file(content: &str) -> bool {
    with_parsed_program(content, "psr4_file_shape", |program, _| {
        let mut classlikes = 0usize;
        let mut has_other_top_level = false;
        collect_file_shape(
            program.statements.iter(),
            &mut classlikes,
            &mut has_other_top_level,
        );
        classlikes == 1 && !has_other_top_level
    })
}

fn collect_file_shape<'a>(
    statements: impl Iterator<Item = &'a Statement<'a>>,
    classlikes: &mut usize,
    has_other_top_level: &mut bool,
) {
    for stmt in statements {
        match stmt {
            Statement::Namespace(ns) => {
                collect_file_shape(ns.statements().iter(), classlikes, has_other_top_level);
            }
            Statement::Declare(declare) => match &declare.body {
                DeclareBody::Statement(body) => {
                    classify_statement(body, classlikes, has_other_top_level);
                }
                DeclareBody::ColonDelimited(body) => {
                    collect_file_shape(body.statements.iter(), classlikes, has_other_top_level);
                }
            },
            other => {
                classify_statement(other, classlikes, has_other_top_level);
            }
        }
    }
}

fn classify_statement(
    stmt: &Statement<'_>,
    classlikes: &mut usize,
    has_other_top_level: &mut bool,
) {
    match stmt {
        Statement::OpeningTag(_) | Statement::ClosingTag(_) | Statement::Noop(_) => {}
        Statement::Use(_) => {}
        Statement::Class(_)
        | Statement::Interface(_)
        | Statement::Trait(_)
        | Statement::Enum(_) => {
            *classlikes += 1;
        }
        // A bare top-level function call (e.g. Pest's `it(...)`,
        // `describe(...)`, `uses(...)`) is the signature of a test/script
        // file that happens to declare a local fixture class-like, rather
        // than a normal PSR-4 source file whose sole purpose is that
        // declaration. Other statement kinds (`if`, `declare`, `require`,
        // `return`, ...) are common in ordinary production files and must
        // not disqualify the mismatch checks, or genuine PSR-4 violations
        // would go undetected.
        Statement::Expression(expr_stmt)
            if matches!(expr_stmt.expression, Expression::Call(Call::Function(_))) =>
        {
            *has_other_top_level = true;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{is_structural_single_classlike_file, namespace_mismatch_diagnostic};
    use crate::Backend;
    use crate::composer::Psr4Mapping;
    use std::path::PathBuf;

    #[test]
    fn structural_helper_rejects_inline_fixture_file() {
        let php = "<?php\n\nit('demo', function (): void {});\n\nenum ExampleState: int {\n    case One = 1;\n}\n";
        assert!(!is_structural_single_classlike_file(php));
    }

    #[test]
    fn namespace_mismatch_skipped_for_inline_fixture_file() {
        let backend = Backend::new_test_with_workspace(
            PathBuf::from("/project"),
            vec![Psr4Mapping {
                prefix: "App\\Models\\".to_string(),
                base_path: "app/Models/".to_string(),
            }],
        );
        let uri = "file:///project/app/Models/ExampleState.php";
        let php = "<?php\nnamespace App\\Wrong;\n\nit('demo', function (): void {});\n\nenum ExampleState: int {\n    case One = 1;\n}\n";

        assert!(namespace_mismatch_diagnostic(&backend, uri, php).is_none());
    }
}
