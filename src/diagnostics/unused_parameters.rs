//! Parameters a function never reads.
//!
//! ```php
//! function greet(string $name, bool $loud) {   // $loud is never read
//!     return "Hello $name";
//! }
//! ```
//!
//! Only the parameters after the last one the body reads are reported:
//! an unused parameter before a used one still holds that one's position,
//! so callers (and callbacks handed to `array_map()` and the like) cannot
//! drop it.  A name starting with `$_` marks a parameter as deliberately
//! unused and is skipped without ending the run.
//!
//! A method's parameters are only judged when nothing else dictates its
//! signature:
//!
//! - a method with a prototype (a parent class, interface, or abstract
//!   trait method it implements) keeps the parameters that prototype
//!   declares;
//! - a method a subclass can override is part of a contract the override
//!   may read the parameter in, so only private and final methods (and
//!   those of final classes, enums, and anonymous classes) are judged;
//! - a constructor is judged whatever its class, since a subclass's
//!   constructor replaces it rather than sharing its parameters;
//! - PHP fixes the signature of the other magic methods.
//!
//! Promoted constructor parameters declare properties and are always
//! used.  A body that reads its arguments without naming them
//! (`func_get_args()`, `compact()`, `get_defined_vars()`, a variable
//! variable, or an `include` that runs in its scope) is left alone.
//!
//! Off unless `unused-parameters` is enabled: an unused parameter is
//! often deliberate, so flagging it is a matter of style.
//!
//! Severity: `Hint` with `DiagnosticTag::Unnecessary`, like an unused
//! variable.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::ResolvedClassLoader;
use crate::diagnostics::undefined_variables::collect_import_offsets;
use crate::scope_collector::{
    AccessKind, AccessRole, Frame, FrameKind, ScopeBody, ScopeMap,
    collect_function_scope_with_kind_and_resolver,
};
use crate::types::{ANONYMOUS_CLASS_PREFIX, ClassInfo, ClassLikeKind, MethodInfo, Visibility};

use super::helpers::{FileDiagnosticContext, make_tagged_diagnostic};
use super::override_attribute::method_has_prototype;

/// Diagnostic code used for unused-parameter diagnostics.
pub(crate) const UNUSED_PARAMETER_CODE: &str = "unused_parameter";

/// A function-like with a body.
struct Candidate<'ast, 'arena> {
    params: &'ast FunctionLikeParameterList<'arena>,
    body: ScopeBody<'ast, 'arena>,
    body_start: u32,
    body_end: u32,
    kind: FrameKind,
    /// The offset of a method's name, which finds its `MethodInfo`.
    method_name_offset: Option<u32>,
}

struct CollectCandidates;

impl<'ast, 'arena> Walker<'ast, 'arena, Vec<Candidate<'ast, 'arena>>> for CollectCandidates {
    fn walk_in_function(
        &self,
        function: &'ast Function<'arena>,
        found: &mut Vec<Candidate<'ast, 'arena>>,
    ) {
        found.push(Candidate {
            params: &function.parameter_list,
            body: ScopeBody::Statements(function.body.statements.as_slice()),
            body_start: function.body.left_brace.start.offset,
            body_end: function.body.right_brace.end.offset,
            kind: FrameKind::Function,
            method_name_offset: None,
        });
    }

    fn walk_in_method(
        &self,
        method: &'ast Method<'arena>,
        found: &mut Vec<Candidate<'ast, 'arena>>,
    ) {
        if let MethodBody::Concrete(block) = &method.body {
            found.push(Candidate {
                params: &method.parameter_list,
                body: ScopeBody::Statements(block.statements.as_slice()),
                body_start: block.left_brace.start.offset,
                body_end: block.right_brace.end.offset,
                kind: FrameKind::Method,
                method_name_offset: Some(method.name.span.start.offset),
            });
        }
    }

    fn walk_in_closure(
        &self,
        closure: &'ast Closure<'arena>,
        found: &mut Vec<Candidate<'ast, 'arena>>,
    ) {
        found.push(Candidate {
            params: &closure.parameter_list,
            body: ScopeBody::Statements(closure.body.statements.as_slice()),
            body_start: closure.body.left_brace.start.offset,
            body_end: closure.body.right_brace.end.offset,
            kind: FrameKind::Closure,
            method_name_offset: None,
        });
    }

    fn walk_in_arrow_function(
        &self,
        arrow: &'ast ArrowFunction<'arena>,
        found: &mut Vec<Candidate<'ast, 'arena>>,
    ) {
        found.push(Candidate {
            params: &arrow.parameter_list,
            body: ScopeBody::Expression(arrow.expression),
            body_start: arrow.arrow.start.offset,
            body_end: arrow.expression.span().end.offset,
            kind: FrameKind::ArrowFunction,
            method_name_offset: None,
        });
    }
}

impl Backend {
    /// Collect unused-parameter diagnostics for a single file.
    pub fn collect_unused_parameter_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_unused_parameter_diagnostics_with_context(&ctx, uri, content, out);
    }

    pub(crate) fn collect_unused_parameter_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        if !self.config().diagnostics.unused_parameters_enabled() {
            return;
        }
        let _parse_guard = crate::parser::with_parse_cache(content);
        let class_loaders = self.resolved_class_loaders(&ctx.file);

        // Every method this file declares, by the offset of its name.
        let methods: HashMap<u32, (&ClassInfo, &MethodInfo)> = ctx
            .file
            .classes
            .iter()
            .flat_map(|class| {
                class
                    .methods
                    .iter()
                    .filter(|m| {
                        !m.is_virtual
                            && m.name_offset >= class.start_offset
                            && m.name_offset <= class.end_offset
                    })
                    .map(move |m| (m.name_offset, (class.as_ref(), m.as_ref())))
            })
            .collect();

        crate::parser::with_parsed_program(content, "unused_parameter", |program, content| {
            let mut found = Vec::new();
            for statement in program.statements.iter() {
                CollectCandidates.walk_statement(statement, &mut found);
            }

            for candidate in found {
                if !candidate.params.parameters.iter().any(|p| {
                    may_report(p) && super::incremental::reports_site(p.span().start.offset)
                }) {
                    continue;
                }
                if let Some(name_offset) = candidate.method_name_offset {
                    let Some(&(class, method)) = methods.get(&name_offset) else {
                        continue;
                    };
                    if !method_signature_is_free(class, method, class_loaders.at(name_offset)) {
                        continue;
                    }
                }

                for param in unused_parameters(&candidate) {
                    let start = param.variable.span().start.offset;
                    if !super::incremental::reports_site(start) {
                        continue;
                    }
                    let name = bytes_to_str(param.variable.name);
                    let Some(range) = self.offset_range_to_lsp_range(
                        uri,
                        content,
                        start as usize,
                        start as usize + name.len(),
                    ) else {
                        continue;
                    };
                    out.push(make_tagged_diagnostic(
                        range,
                        DiagnosticSeverity::Hint,
                        UNUSED_PARAMETER_CODE,
                        format!("Unused parameter {name}"),
                        Some(DiagnosticTag::Unnecessary),
                    ));
                }
            }
        });
    }
}

/// Whether `param` could be reported at all: it is neither promoted to a
/// property nor marked as deliberately unused.
fn may_report(param: &FunctionLikeParameter<'_>) -> bool {
    !param.is_promoted_property() && !is_deliberately_unused(bytes_to_str(param.variable.name))
}

/// A `$_` prefix marks a parameter the author knows is unused.
fn is_deliberately_unused(name: &str) -> bool {
    name.starts_with("$_")
}

/// Whether the parameters of `method` are the method's own to choose,
/// rather than fixed by PHP, a prototype, or the overrides it may have.
fn method_signature_is_free(
    class: &ClassInfo,
    method: &MethodInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    let name = method.name.as_str();
    let is_constructor = name.eq_ignore_ascii_case("__construct");
    if name.starts_with("__") && !is_constructor && !name.eq_ignore_ascii_case("__invoke") {
        return false;
    }
    if !is_constructor && can_be_overridden(class, method) {
        return false;
    }
    !method_has_prototype(class, name, class_loader)
}

/// Whether a subclass (or, for a trait, a class using it) can override
/// `method`.
fn can_be_overridden(class: &ClassInfo, method: &MethodInfo) -> bool {
    if method.visibility == Visibility::Private {
        return false;
    }
    match class.kind {
        ClassLikeKind::Enum => false,
        ClassLikeKind::Trait | ClassLikeKind::Interface => true,
        ClassLikeKind::Class => {
            !(method.is_final || class.is_final || class.name.starts_with(ANONYMOUS_CLASS_PREFIX))
        }
    }
}

/// The trailing parameters of `candidate` its body never reads, last
/// first.
fn unused_parameters<'ast, 'arena>(
    candidate: &Candidate<'ast, 'arena>,
) -> Vec<&'ast FunctionLikeParameter<'arena>> {
    if crate::parser::scope_body_reads_func_args(candidate.body) {
        return Vec::new();
    }
    let scope = collect_function_scope_with_kind_and_resolver(
        candidate.params,
        candidate.body,
        candidate.body_start,
        candidate.body_end,
        candidate.kind,
        None,
        None,
    );
    let Some(frame) = scope
        .frames
        .iter()
        .find(|f| f.start == candidate.body_start && f.kind == candidate.kind)
    else {
        return Vec::new();
    };

    // Code that names variables at runtime can read any of them; one
    // that names a single variable reads that one.
    let mut dynamically_read: Vec<&str> = Vec::new();
    for access in &scope.dynamic_scope_accesses {
        if !shares_scope(access.offset, frame, &scope.frames) {
            continue;
        }
        match &access.name {
            Some(name) => dynamically_read.push(name),
            None => return Vec::new(),
        }
    }
    // An included file runs in this scope and may read anything in it.
    if collect_import_offsets(candidate.body)
        .into_iter()
        .any(|offset| shares_scope(offset, frame, &scope.frames))
    {
        return Vec::new();
    }

    let mut unused = Vec::new();
    for param in candidate.params.parameters.iter().rev() {
        let name = bytes_to_str(param.variable.name);
        if is_deliberately_unused(name) {
            continue;
        }
        if param.is_promoted_property()
            || dynamically_read.contains(&name)
            || is_used(name, param.ampersand.is_some(), frame, &scope)
        {
            break;
        }
        unused.push(param);
    }
    unused
}

/// Whether the parameter `name` of `frame` is read in its body.  A write
/// to a by-reference parameter reaches the caller, so it counts too.
fn is_used(name: &str, by_reference: bool, frame: &Frame, scope: &ScopeMap) -> bool {
    scope.accesses.iter().any(|a| {
        a.name == name
            && a.offset >= frame.start
            && a.offset <= frame.end
            && match a.kind {
                AccessKind::Read | AccessKind::ReadWrite => true,
                AccessKind::Write => by_reference && a.role != AccessRole::Parameter,
            }
            && sees_parameter(name, a.offset, frame, &scope.frames)
    })
}

/// Whether the code at `offset` runs in `frame`'s own variable scope: it
/// is not inside a closure nested in it.  Arrow functions and catch blocks
/// share the scope they sit in.
fn shares_scope(offset: u32, frame: &Frame, frames: &[Frame]) -> bool {
    offset >= frame.start
        && offset <= frame.end
        && !frame
            .nested_frames_at(frames, offset)
            .any(|f| f.kind == FrameKind::Closure)
}

/// Whether `$name` at `offset` is `frame`'s parameter, rather than one of
/// a nested closure's variables or a nested arrow function's parameter of
/// the same name.
fn sees_parameter(name: &str, offset: u32, frame: &Frame, frames: &[Frame]) -> bool {
    !frame.nested_frames_at(frames, offset).any(|f| {
        f.kind == FrameKind::Closure
            || (f.kind == FrameKind::ArrowFunction && f.parameters.iter().any(|p| p == name))
    })
}
