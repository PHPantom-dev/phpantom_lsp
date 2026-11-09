//! Data providers PHPUnit refuses to call.
//!
//! PHPUnit reaches a data provider through reflection and calls it before
//! the test case is constructed, so the method must be `public`, and from
//! PHPUnit 10 on also `static` (a deprecation in 10, an error from 11).
//! A provider name that matches no method at all is the unknown-member
//! check's to report.
//!
//! Whether PHPUnit 10 is in use is read from `composer.lock`.  The
//! `#[DataProvider]` and `#[DataProviderExternal]` attributes settle it
//! on their own: only PHPUnit 10 and later read them.

use std::sync::atomic::Ordering;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::find_class_at_offset;
use crate::symbol_map::{DocblockMemberRef, SymbolKind};
use crate::type_engine::resolver::{SubjectOutcome, resolve_subject_outcome};
use crate::types::{AccessKind, ClassInfo, Visibility};
use crate::virtual_members::resolve_class_fully_cached;

use super::helpers::{FileDiagnosticContext, make_diagnostic};

pub(crate) const INVALID_DATA_PROVIDER_CODE: &str = "invalid_phpunit_data_provider";

/// The modifier a data provider lacks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MissingModifier {
    Public,
    Static,
}

impl MissingModifier {
    pub(crate) fn keyword(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Static => "static",
        }
    }
}

/// What is wrong with the method one data-provider reference names.
pub(crate) enum ProviderProblem {
    /// The method exists but lacks a modifier PHPUnit requires.
    Modifier {
        missing: MissingModifier,
        /// The display name of the class the method was found on.
        class_name: String,
        /// The offset of the method's name, when the method is declared in
        /// a class of this file and so can be fixed here.
        local_name_offset: Option<u32>,
    },
    /// No method of the name exists on the test class itself.
    Missing {
        /// The test class's closing brace, which a new provider is
        /// inserted before.
        class_end: u32,
    },
}

/// One data-provider reference and what is wrong with the method it names.
pub(crate) struct ProviderFinding {
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) method_name: String,
    pub(crate) problem: ProviderProblem,
    /// Whether PHPUnit 11 or later is known to be in use, where a
    /// non-static provider is an error rather than a deprecation.
    pub(crate) static_is_error: bool,
}

impl Backend {
    /// The PHPUnit major version the project's `composer.lock` installs.
    pub(crate) fn phpunit_major(&self) -> Option<u8> {
        match self.phpunit_major.load(Ordering::Relaxed) {
            0 => None,
            major => Some(major),
        }
    }

    /// Record the PHPUnit major version the workspace's `composer.lock`
    /// files install, the lowest when they disagree.
    pub(crate) fn set_phpunit_major(&self, major: Option<u8>) {
        self.phpunit_major
            .store(major.unwrap_or(0), Ordering::Relaxed);
    }

    /// Collect data-provider diagnostics for a single file.
    pub fn collect_data_provider_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_data_provider_diagnostics_with_context(&ctx, uri, content, out);
    }

    pub(crate) fn collect_data_provider_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        out.extend(
            self.data_provider_findings(ctx, content, |start, _| {
                super::incremental::reports_site(start)
            })
            .iter()
            .filter_map(|finding| self.data_provider_diagnostic(uri, content, finding)),
        );
    }

    /// The diagnostic a finding about a provider's modifiers reports.
    pub(crate) fn data_provider_diagnostic(
        &self,
        uri: &str,
        content: &str,
        finding: &ProviderFinding,
    ) -> Option<Diagnostic> {
        let ProviderProblem::Modifier {
            missing,
            class_name,
            ..
        } = &finding.problem
        else {
            return None;
        };
        let range = self.offset_range_to_lsp_range(
            uri,
            content,
            finding.start as usize,
            finding.end as usize,
        )?;
        let severity = if *missing == MissingModifier::Static && !finding.static_is_error {
            DiagnosticSeverity::Warning
        } else {
            DiagnosticSeverity::Error
        };
        Some(make_diagnostic(
            range,
            severity,
            INVALID_DATA_PROVIDER_CODE,
            format!(
                "Data provider method {}::{}() must be {}",
                class_name,
                finding.method_name,
                missing.keyword()
            ),
        ))
    }

    /// Judge every data-provider reference in the file whose start and end
    /// offsets `wanted` accepts.
    pub(crate) fn data_provider_findings(
        &self,
        ctx: &FileDiagnosticContext,
        content: &str,
        wanted: impl Fn(u32, u32) -> bool,
    ) -> Vec<ProviderFinding> {
        let mut findings = Vec::new();
        let Some(source) = ctx.symbol_map.source(content) else {
            return findings;
        };
        let mut loaders = None;
        let phpunit_major = self.phpunit_major();

        for span in &ctx.symbol_map.spans {
            let SymbolKind::MemberAccess {
                subject_text,
                member_name,
                docblock_ref: DocblockMemberRef::PhpUnitDataProvider { attribute },
                ..
            } = &span.kind
            else {
                continue;
            };
            if !wanted(span.start, span.end) {
                continue;
            }
            let loaders = loaders.get_or_insert_with(|| self.file_loaders(&ctx.file));
            let subject = subject_text.as_str(source);
            let rctx = loaders.resolution_ctx_at(content, span.start);
            let SubjectOutcome::Resolved { classes, .. } =
                resolve_subject_outcome(subject, AccessKind::of_member_access(true), &rctx)
            else {
                continue;
            };
            let [class] = classes.as_slice() else {
                continue;
            };
            let resolved = resolve_class_fully_cached(
                class,
                loaders.resolved_class_loader(),
                &self.resolved_class_cache,
            );
            let name = member_name.as_str();

            let static_is_error = phpunit_major.is_some_and(|m| m >= 11);
            let mut push = |problem| {
                findings.push(ProviderFinding {
                    start: span.start,
                    end: span.end,
                    method_name: name.to_owned(),
                    problem,
                    static_is_error,
                })
            };

            match resolved.get_method_ci(name) {
                // A `@method` tag declares no modifiers to judge.
                Some(method) if method.is_virtual => {}
                Some(method) => {
                    let requires_static = *attribute || phpunit_major.is_some_and(|m| m >= 10);
                    let missing = [
                        (method.visibility != Visibility::Public)
                            .then_some(MissingModifier::Public),
                        (requires_static && !method.is_static).then_some(MissingModifier::Static),
                    ];
                    let local_name_offset = local_declaration(&ctx.file.classes, class, name);
                    for missing in missing.into_iter().flatten() {
                        push(ProviderProblem::Modifier {
                            missing,
                            class_name: super::member_lookup::display_class_name(class),
                            local_name_offset,
                        });
                    }
                }
                // A missing provider can only be created on the test class
                // the reference sits in.
                None if subject == "static" => {
                    let Some(test_class) = find_class_at_offset(&ctx.file.classes, span.start)
                    else {
                        continue;
                    };
                    if test_class.fqn() != class.fqn() || test_class.end_offset == 0 {
                        continue;
                    }
                    push(ProviderProblem::Missing {
                        class_end: test_class.end_offset - 1,
                    });
                }
                None => {}
            }
        }
        findings
    }
}

/// The offset of the name of `method`, when `class` declares it in this
/// file.
fn local_declaration(
    local_classes: &[std::sync::Arc<ClassInfo>],
    class: &ClassInfo,
    method: &str,
) -> Option<u32> {
    let fqn = class.fqn();
    local_classes
        .iter()
        .filter(|local| local.fqn() == fqn)
        .find_map(|local| local.get_method_ci(method))
        .filter(|declared| !declared.is_virtual && declared.name_offset != 0)
        .map(|declared| declared.name_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANNOTATED: &str = r#"<?php
final class LegacyTest
{
    /**
     * @dataProvider cases
     */
    public function testIt(int $n): void {}

    public function cases(): array { return [[1]]; }
}
"#;

    fn messages(backend: &Backend) -> Vec<(DiagnosticSeverity, String)> {
        let uri = "file:///legacy.php";
        backend.update_ast(uri, ANNOTATED);
        let mut out = Vec::new();
        backend.collect_data_provider_diagnostics(uri, ANNOTATED, &mut out);
        out.into_iter()
            .map(|d| {
                assert_eq!(
                    d.code,
                    Some(Code::String(INVALID_DATA_PROVIDER_CODE.to_owned()))
                );
                let Message::String(message) = d.message else {
                    panic!("expected a plain-text message");
                };
                (d.severity.unwrap(), message)
            })
            .collect()
    }

    #[test]
    fn an_annotated_instance_provider_is_fine_before_phpunit_10() {
        let backend = Backend::new_test();
        assert!(messages(&backend).is_empty());
        backend.set_phpunit_major(Some(9));
        assert!(messages(&backend).is_empty());
    }

    #[test]
    fn an_annotated_instance_provider_must_be_static_from_phpunit_10() {
        let backend = Backend::new_test();
        backend.set_phpunit_major(Some(10));
        assert_eq!(
            messages(&backend),
            vec![(
                DiagnosticSeverity::Warning,
                "Data provider method LegacyTest::cases() must be static".to_owned()
            )]
        );
        backend.set_phpunit_major(Some(11));
        assert_eq!(messages(&backend)[0].0, DiagnosticSeverity::Error);
    }
}
