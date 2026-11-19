//! Functions that can end without returning the value their type promises.
//!
//! ```php
//! function score(int $x): int {
//!     if ($x > 0) {
//!         return 1;
//!     }
//! }   // TypeError: score(): Return value must be of type int, none returned
//! ```
//!
//! Running off the closing brace returns `null`, so PHP throws a
//! `TypeError` there unless the native return type accepts it: `void`,
//! `never` (which has its own error), `mixed`, `null`, or anything
//! nullable.  A `@return` tag over an untyped signature is not enforced at
//! runtime, so only the native type counts.  Generators return a
//! `Generator` whatever their body does, and abstract methods have no body.
//!
//! The diagnostic sits on the closing brace, the point the error is raised
//! at.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::type_engine::type_ops::condition_facts::{
    FlowFacts, body_end_is_reachable, condition_always_holds_at, is_never_call_at,
    switch_covers_every_value_at,
};

use super::helpers::{FileDiagnosticContext, make_diagnostic};

/// Diagnostic code for a body that can end without returning a value.
pub(crate) const MISSING_RETURN_CODE: &str = "missing_return";

/// A function-like with a body and a native return type.
struct Candidate<'ast, 'arena> {
    kind: CandidateKind,
    /// Its name as declared, empty for a closure.
    name: &'ast str,
    /// Where its declaration starts, to find the namespace or class it
    /// belongs to.
    offset: u32,
    hint: &'ast Hint<'arena>,
    body: &'ast Block<'arena>,
}

#[derive(Clone, Copy)]
enum CandidateKind {
    Function,
    Method,
    Closure,
}

struct CollectCandidates;

impl<'ast, 'arena> Walker<'ast, 'arena, Vec<Candidate<'ast, 'arena>>> for CollectCandidates {
    fn walk_in_function(
        &self,
        function: &'ast Function<'arena>,
        found: &mut Vec<Candidate<'ast, 'arena>>,
    ) {
        if let Some(hint) = &function.return_type_hint {
            found.push(Candidate {
                kind: CandidateKind::Function,
                name: bytes_to_str(function.name.value),
                offset: function.span().start.offset,
                hint: &hint.hint,
                body: &function.body,
            });
        }
    }

    fn walk_in_method(
        &self,
        method: &'ast Method<'arena>,
        found: &mut Vec<Candidate<'ast, 'arena>>,
    ) {
        if let Some(hint) = &method.return_type_hint
            && let MethodBody::Concrete(body) = &method.body
        {
            found.push(Candidate {
                kind: CandidateKind::Method,
                name: bytes_to_str(method.name.value),
                offset: method.span().start.offset,
                hint: &hint.hint,
                body,
            });
        }
    }

    fn walk_in_closure(
        &self,
        closure: &'ast Closure<'arena>,
        found: &mut Vec<Candidate<'ast, 'arena>>,
    ) {
        if let Some(hint) = &closure.return_type_hint {
            found.push(Candidate {
                kind: CandidateKind::Closure,
                name: "",
                offset: closure.span().start.offset,
                hint: &hint.hint,
                body: &closure.body,
            });
        }
    }
}

/// How the message names `candidate`: `Function App\\score()`,
/// `Method App\\Scorer::get()`, or `Closure`.
fn candidate_label(candidate: &Candidate<'_, '_>, file: &crate::types::FileContext) -> String {
    match candidate.kind {
        CandidateKind::Function => match file.namespace_at(candidate.offset) {
            Some(ns) if !ns.is_empty() => format!("Function {ns}\\{}()", candidate.name),
            _ => format!("Function {}()", candidate.name),
        },
        CandidateKind::Method => {
            match crate::class_lookup::find_class_at_offset(&file.classes, candidate.offset) {
                Some(class) if !class.name.starts_with("__anonymous@") => {
                    format!("Method {}::{}()", class.fqn(), candidate.name)
                }
                _ => format!("Method {}()", candidate.name),
            }
        }
        CandidateKind::Closure => "Closure".to_string(),
    }
}

/// Whether running off the end of a body is a `TypeError` for this
/// declared type.
fn requires_a_value(declared: &PhpType) -> bool {
    !declared.is_void() && !declared.is_never() && !declared.accepts_null()
}

impl Backend {
    /// Collect missing-return diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub(crate) fn collect_missing_return_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let _parse_guard = crate::parser::with_parse_cache(content);
        let loaders = self.file_loaders(&ctx.file);
        crate::parser::with_parsed_program(content, "missing_return", |program, content| {
            let mut found = Vec::new();
            for statement in program.statements.iter() {
                CollectCandidates.walk_statement(statement, &mut found);
            }

            for candidate in found {
                let closing_brace = candidate.body.right_brace;
                if !super::incremental::reports_site(closing_brace.start.offset) {
                    continue;
                }
                let declared = crate::parser::extract_hint_type(candidate.hint);
                if !requires_a_value(&declared) {
                    continue;
                }
                let statements = candidate.body.statements.as_slice();
                // The structural answer settles almost every body, and
                // needs no types; only a body it leaves open has its calls
                // and `switch`es looked at.
                if !body_end_is_reachable(statements, &FlowFacts::STRUCTURAL)
                    || crate::parser::contains_own_yield(statements.iter())
                {
                    continue;
                }
                let var_ctx = loaders.var_ctx_at(content, candidate.body.span().start.offset);
                let typed = FlowFacts {
                    never_call: &|expr| is_never_call_at(expr, &var_ctx),
                    switch_is_exhaustive: &|switch| switch_covers_every_value_at(switch, &var_ctx),
                    condition_always_holds: &|condition| {
                        condition_always_holds_at(condition, &var_ctx)
                    },
                };
                if !body_end_is_reachable(statements, &typed) {
                    continue;
                }

                let Some(range) = self.offset_range_to_lsp_range(
                    uri,
                    content,
                    closing_brace.start.offset as usize,
                    closing_brace.end.offset as usize,
                ) else {
                    continue;
                };
                out.push(make_diagnostic(
                    range,
                    DiagnosticSeverity::Error,
                    MISSING_RETURN_CODE,
                    format!(
                        "{} should return {declared} but can reach its end without returning",
                        candidate_label(&candidate, &ctx.file),
                    ),
                ));
            }
        });
    }
}
