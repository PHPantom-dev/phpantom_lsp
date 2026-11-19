//! `setUp()` and `tearDown()` that skip the parent's.
//!
//! A PHPUnit test case whose base class does work in `setUp()`,
//! `tearDown()`, `setUpBeforeClass()`, or `tearDownAfterClass()` (Laravel's
//! and Symfony's both do) breaks when a subclass overrides the method and
//! never calls the parent's. PHPStan reports it as `phpunit.callParent`.
//!
//! Only a parent implementation that does something is worth calling:
//! `PHPUnit\Framework\TestCase`'s own are empty, and an abstract one cannot
//! be called. A call to the parent's method anywhere in the body counts,
//! so a call made conditionally or after some setup of the test's own is
//! accepted.

use mago_span::HasSpan;
use mago_syntax::cst::{ClassLikeMemberSelector, Expression, MethodBody, Node};
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::types::{ClassInfo, ClassLikeKind, MAX_INHERITANCE_DEPTH, MAX_TRAIT_DEPTH, Visibility};

use super::helpers::{FileDiagnosticContext, make_diagnostic};

pub(crate) const MISSING_PARENT_CALL_CODE: &str = "missing_phpunit_parent_call";

const PHPUNIT_TEST_CASE: &str = "PHPUnit\\Framework\\TestCase";

/// The hooks PHPUnit runs around a test or a test class.
const HOOKS: [&str; 4] = [
    "setUp",
    "tearDown",
    "setUpBeforeClass",
    "tearDownAfterClass",
];

/// One hook that never calls the parent's.
pub(crate) struct MissingParentCall {
    /// The method's name as declared.
    pub(crate) name: String,
    /// The offset of the method's name.
    pub(crate) name_offset: u32,
    /// The offset of the method body's `{`.
    pub(crate) body_open: u32,
    /// The offset of the method body's `}`.
    pub(crate) body_close: u32,
    /// The offset of the body's first statement and the end of its last,
    /// `None` for an empty body.
    pub(crate) statements: Option<(u32, u32)>,
}

impl MissingParentCall {
    /// Whether the call belongs before the method's own work (set-up
    /// hooks) rather than after it (tear-down hooks).
    pub(crate) fn calls_first(&self) -> bool {
        self.name.to_ascii_lowercase().starts_with("setup")
    }
}

impl Backend {
    /// Collect missing-parent-call diagnostics for a single file.
    pub fn collect_parent_call_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_parent_call_diagnostics_with_context(&ctx, uri, content, out);
    }

    pub(crate) fn collect_parent_call_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        out.extend(
            self.missing_parent_calls(ctx, content, |offset| {
                super::incremental::reports_site(offset)
            })
            .iter()
            .filter_map(|finding| self.parent_call_diagnostic(uri, content, finding)),
        );
    }

    /// The diagnostic a hook that skips the parent's reports.
    pub(crate) fn parent_call_diagnostic(
        &self,
        uri: &str,
        content: &str,
        finding: &MissingParentCall,
    ) -> Option<Diagnostic> {
        let start = finding.name_offset as usize;
        let range =
            self.offset_range_to_lsp_range(uri, content, start, start + finding.name.len())?;
        Some(make_diagnostic(
            range,
            DiagnosticSeverity::Warning,
            MISSING_PARENT_CALL_CODE,
            format!("This override does not call parent::{}()", finding.name),
        ))
    }

    /// Every hook in the file, whose name offset `wanted` accepts, that
    /// overrides a parent implementation it never calls.
    pub(crate) fn missing_parent_calls(
        &self,
        ctx: &FileDiagnosticContext,
        content: &str,
        wanted: impl Fn(u32) -> bool,
    ) -> Vec<MissingParentCall> {
        // Most classes override no hook; settle that before loading any
        // ancestor or parsing the file.
        let candidates: Vec<(&ClassInfo, &str, u32)> = ctx
            .file
            .classes
            .iter()
            .filter(|class| class.kind == ClassLikeKind::Class && class.parent_class.is_some())
            .flat_map(|class| {
                class
                    .methods
                    .iter()
                    .filter(|m| !m.is_virtual && !m.is_abstract && m.name_offset != 0)
                    .filter(|m| HOOKS.iter().any(|h| m.name.eq_ignore_ascii_case(h)))
                    .filter(|m| wanted(m.name_offset))
                    .map(move |m| (&**class, m.name.as_str(), m.name_offset))
            })
            .collect();
        if candidates.is_empty() {
            return Vec::new();
        }

        let class_loaders = self.resolved_class_loaders(&ctx.file);
        let overriding: Vec<(&str, u32)> = candidates
            .into_iter()
            .filter(|(class, name, offset)| {
                overrides_callable_hook(class, name, class_loaders.at(*offset))
            })
            .map(|(_, name, offset)| (name, offset))
            .collect();
        if overriding.is_empty() {
            return Vec::new();
        }

        crate::parser::with_parsed_program(content, "parent_calls", |program, _| {
            overriding
                .iter()
                .filter_map(|&(name, offset)| {
                    let method =
                        crate::parser::innermost_node_at(Node::Program(program), offset, |node| {
                            match node {
                                Node::Method(method) if method.name.span.start.offset == offset => {
                                    Some(method)
                                }
                                _ => None,
                            }
                        })?;
                    let MethodBody::Concrete(block) = &method.body else {
                        return None;
                    };
                    if calls_parent(Node::Method(method), name) {
                        return None;
                    }
                    let statements =
                        block.statements.first().zip(block.statements.last()).map(
                            |(first, last)| (first.span().start.offset, last.span().end.offset),
                        );
                    Some(MissingParentCall {
                        name: name.to_owned(),
                        name_offset: offset,
                        body_open: block.left_brace.start.offset,
                        body_close: block.right_brace.start.offset,
                        statements,
                    })
                })
                .collect()
        })
    }
}

/// Whether `class` is a PHPUnit test case whose nearest ancestor
/// implementation of `name` is one worth calling: concrete, visible, and
/// not `TestCase`'s own empty hook.
///
/// Answers `false` when part of the hierarchy cannot be loaded, since the
/// class may then not be a test case at all.
fn overrides_callable_hook(
    class: &ClassInfo,
    name: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    // Whether the nearest declaration found so far is one to call.
    let mut callable = None;
    let mut next = class.parent_class;
    let mut depth = 0u32;
    while let Some(parent_name) = next {
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            return false;
        }
        let Some(parent) = class_loader(Fqn::new(&parent_name)) else {
            return false;
        };
        let is_test_case = parent.fqn().as_str() == PHPUNIT_TEST_CASE;
        if callable.is_none() {
            let declared = match parent.get_method_ci(name) {
                Some(method) if !method.is_virtual => Some((method.is_abstract, method.visibility)),
                _ => trait_declaration(&parent.used_traits, name, class_loader, 0),
            };
            if let Some((is_abstract, visibility)) = declared {
                callable = Some(!is_abstract && visibility != Visibility::Private && !is_test_case);
            }
        }
        if is_test_case {
            return callable == Some(true);
        }
        next = parent.parent_class;
    }
    false
}

/// Whether `name` is declared abstract and its visibility, by the first
/// of `trait_names` (or a trait they use) that declares it.
fn trait_declaration(
    trait_names: &[crate::atom::Atom],
    name: &str,
    class_loader: &ResolvedClassLoader<'_>,
    depth: u32,
) -> Option<(bool, Visibility)> {
    if depth > MAX_TRAIT_DEPTH {
        return None;
    }
    trait_names.iter().find_map(|trait_name| {
        let used = class_loader(Fqn::new(trait_name))?;
        match used.get_method_ci(name) {
            Some(method) if !method.is_virtual => Some((method.is_abstract, method.visibility)),
            _ => trait_declaration(&used.used_traits, name, class_loader, depth + 1),
        }
    })
}

/// Whether `method` calls `parent::name()` anywhere in its own body. A
/// call inside an anonymous class names that class's parent instead.
fn calls_parent(method: Node<'_, '_>, name: &str) -> bool {
    let mut found = false;
    crate::parser::for_each_node_with_ancestors(method, |node, ancestors| {
        if found {
            return;
        }
        let Node::StaticMethodCall(call) = node else {
            return;
        };
        if !matches!(call.class, Expression::Parent(_)) {
            return;
        }
        let ClassLikeMemberSelector::Identifier(method) = &call.method else {
            return;
        };
        if !method.value.eq_ignore_ascii_case(name.as_bytes()) {
            return;
        }
        found = !ancestors
            .iter()
            .any(|ancestor| matches!(ancestor, Node::AnonymousClass(_)));
    });
    found
}
