//! Unknown member access diagnostics.
//!
//! Walk the precomputed [`SymbolMap`] for a file and flag every
//! `MemberAccess` span where the member does not exist on the resolved
//! class after full resolution (inheritance + virtual member providers).
//!
//! Diagnostics use `Severity::Warning` because the code may still run
//! (e.g. via `__call` / `__get` magic methods that we cannot see), but
//! the user benefits from knowing that PHPantom can't resolve the member.
//!
//! We suppress diagnostics when:
//!
//! - The subject type cannot be resolved (we can't know what members it has).
//! - Any resolved class in a union type has the member (the member is
//!   valid for at least one branch of the union).  The opt-in
//!   `partial-union-members` setting reports the branches that lack it.
//! - Any resolved class has `__call` / `__callStatic` (for method calls)
//!   or `__get` (for property access) magic methods — these accept
//!   arbitrary member names at runtime.
//! - Any resolved class is `stdClass` — it is a universal object
//!   container that accepts arbitrary properties at runtime.
//! - The member name is `class` (the magic `::class` constant).
//! - The subject is an enum and the member is a case name (enum cases
//!   are accessed via `::` but stored as constants).
//! - The subject is `$this`, `self`, `static`, or `parent` inside a
//!   trait method.  Traits are incomplete by nature — they expect to
//!   be mixed into classes that provide the missing members.  Flagging
//!   accesses that only exist on host classes produces a high rate of
//!   false positives.
//!
//! ## Performance: subject resolution cache
//!
//! A single file can contain hundreds of member access spans that share
//! the same subject text (e.g. 60 occurrences of `$this->assertEquals`,
//! `$this->assertTrue`, etc.).  Without caching, each span triggers the
//! full resolution pipeline including `resolve_variable_types` which
//! re-parses the entire file via `with_parsed_program`.
//!
//! To avoid this, we cache the resolution outcome per
//! [`SubjectCacheKey`], the shared key that scopes a subject resolution
//! to one function/method/closure body.  The cache lives for a single
//! `collect_unknown_member_diagnostics` call and is not shared across
//! files or invocations.
//!
//! ## Performance: narrowing re-resolution fallback
//!
//! The cache key intentionally omits per-access byte offsets to keep
//! the cache effective.  A large service file with 200 accesses to
//! `$model->` should resolve the variable type ONCE, not 200 times.
//!
//! Expression-level narrowing (ternary `instanceof`, inline `&&`
//! chains) can change a variable's type at a specific byte offset
//! without creating a narrowing block.  To handle this without
//! busting the cache, we use a two-phase approach:
//!
//! 1. **Coarse resolution** (cached): resolve the subject WITHOUT
//!    per-access discrimination.  If the member exists on the
//!    resolved classes, we're done — no diagnostic, no re-resolution.
//!
//! 2. **Narrowing fallback** (uncached, rare): when the member is
//!    NOT found on the coarsely-resolved classes, re-resolve the
//!    subject with the exact cursor position.  If the re-resolution
//!    finds the member (because ternary/`&&` narrowing refined the
//!    type), suppress the diagnostic.
//!
//! This makes the common case (member exists) O(1) per unique
//! subject+scope, while preserving correctness for the rare case
//! where expression-level narrowing matters.  The re-resolution runs
//! only where a diagnostic would otherwise be reported, so a file
//! with nothing wrong in it never pays for it.

mod access;
mod chains;
mod member_check;

use std::collections::HashMap;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::ResolvedClassLoader;
use crate::parser::with_parse_cache;
use crate::resolution::FileLoaders;
use crate::type_engine::resolver::{
    ResolutionCtx, SubjectOutcome, resolve_subject_outcome, with_chain_resolution_cache,
};

use super::existence_guards::{compute_existence_guards, compute_isset_empty_argument_ranges};
use super::helpers::{FileDiagnosticContext, make_diagnostic};
use super::subject_cache::SubjectCacheKey;
use super::unresolved_member_access::UNRESOLVED_MEMBER_ACCESS_CODE;

use access::{AccessFilters, MemberAccessSite};
use chains::BrokenChains;

/// Diagnostic code used for unknown-member diagnostics so that code
/// actions can match on it.
pub(crate) const UNKNOWN_MEMBER_CODE: &str = "unknown_member";

/// Diagnostic code for a member that some classes of a union declare and
/// others do not.  Opt-in through `partial-union-members`.
pub(crate) const PARTIAL_UNION_MEMBER_CODE: &str = "partial_union_member";

/// Diagnostic code used when member access is attempted on a scalar
/// type (int, string, bool, float, null, void, never, array).  This
/// is always a runtime crash, so the severity is `Error`.
pub(crate) const SCALAR_MEMBER_ACCESS_CODE: &str = "scalar_member_access";

/// Result of checking whether a member exists on resolved classes.
///
/// Tells the caller whether a diagnostic was emitted and whether the
/// chain should be considered broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemberCheckResult {
    /// No issue — member exists or access is fully suppressed (e.g. `__get`).
    Ok,
    /// Diagnostic emitted; the chain is broken because the type
    /// cannot be recovered.
    Break,
    /// Diagnostic emitted; a magic method (`__call` / `__callStatic`)
    /// can recover the return type so the chain continues resolving.
    MagicFallback,
}

/// Per-pass cache mapping subject keys to their resolution outcomes.
type SubjectCache = HashMap<SubjectCacheKey, SubjectOutcome>;

/// What one unknown-member pass over a file reads for every access.
struct UnknownMemberPass<'a> {
    backend: &'a Backend,
    ctx: &'a FileDiagnosticContext,
    uri: &'a str,
    content: &'a str,
    loaders: FileLoaders<'a>,
    /// Whether a magic method stands in for undeclared members
    /// (`report_magic_properties`).  Read once for the whole file:
    /// `config()` clones the config behind a lock, and the flags cannot
    /// change mid-pass.
    report_magic: bool,
    /// Whether the opt-in `unresolved-member-access` diagnostic is on.
    report_unresolved: bool,
    /// Whether the opt-in `partial-union-members` diagnostic is on.
    report_partial_union: bool,
}

impl Backend {
    /// Collect unknown-member diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_unknown_member_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_unknown_member_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// Same as [`Self::collect_unknown_member_diagnostics`] but reuses
    /// an already-gathered [`FileDiagnosticContext`] instead of
    /// re-reading the per-file locks. Used by `collect_slow_diagnostics`
    /// so all slow collectors in the same pass share one consistent
    /// snapshot.
    pub(crate) fn collect_unknown_member_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(source) = ctx.symbol_map.source(content) else {
            return;
        };
        let config = self.config();
        let pass = UnknownMemberPass {
            backend: self,
            ctx,
            uri,
            content,
            loaders: self.file_loaders(&ctx.file),
            report_magic: config.diagnostics.report_magic_properties_enabled(),
            report_unresolved: config.diagnostics.unresolved_member_access_enabled(),
            report_partial_union: config.diagnostics.partial_union_members_enabled(),
        };
        let filters = AccessFilters {
            source,
            local_classes: &ctx.file.classes,
            existence_guards: compute_existence_guards(content),
            isset_empty_ranges: compute_isset_empty_argument_ranges(content),
        };

        // ── Parse cache for this diagnostic pass ────────────────────────
        // The file content is immutable during a single diagnostic pass.
        // Activating the thread-local parse cache means every call to
        // `with_parsed_program(content, …)` in the resolution pipeline
        // (resolve_variable_types, resolve_variable_type, etc.)
        // will reuse the same parsed AST instead of re-parsing the
        // entire file from scratch.
        let _parse_guard = with_parse_cache(content);

        // ── Chain resolution cache for this diagnostic pass ─────────────
        let _chain_guard = with_chain_resolution_cache();

        let mut subject_cache: SubjectCache = HashMap::new();
        let mut broken_chains = BrokenChains::default();

        // ── Walk every symbol span ──────────────────────────────────────
        for span in &ctx.symbol_map.spans {
            if !super::incremental::reports_site(span.start) {
                continue;
            }
            let Some(access) = MemberAccessSite::of_span(span, &filters) else {
                continue;
            };

            // ── Look up or populate the subject cache ───────────────────
            let cache_key = SubjectCacheKey::build(
                ctx,
                access.current_class,
                access.subject_text,
                access.access_kind(),
                access.start,
            );
            let scope = cache_key.scope();
            let outcome = subject_cache
                .entry(cache_key)
                .or_insert_with(|| pass.resolve_subject(&access));

            // ── Chain error propagation: suppress downstream links ──────
            if broken_chains.is_downstream(access.subject_text, scope) {
                continue;
            }

            // Only a member that is truly missing breaks the chain: when
            // `__call`/`__callStatic` exists, the diagnostic is emitted
            // but the chain continues because the magic method's return
            // type recovers the type.
            if pass.report(&access, outcome, out) == MemberCheckResult::Break {
                broken_chains.record(scope, &access);
            }
        }
    }
}

impl UnknownMemberPass<'_> {
    fn class_loader(&self) -> &ResolvedClassLoader<'_> {
        self.loaders.resolved_class_loader()
    }

    fn range(&self, access: &MemberAccessSite) -> Option<Range> {
        self.backend.offset_range_to_lsp_range(
            self.uri,
            self.content,
            access.start as usize,
            access.end as usize,
        )
    }

    /// Resolve the access's subject at the access's own position.
    fn resolve_subject(&self, access: &MemberAccessSite) -> SubjectOutcome {
        let rctx = ResolutionCtx {
            is_in_static_method: self.ctx.symbol_map.is_in_static_method(access.start),
            ..self.loaders.resolution_ctx_at(self.content, access.start)
        };
        resolve_subject_outcome(access.subject_text, access.access_kind(), &rctx)
    }

    /// Push what the subject's resolution `outcome` says about `access`.
    /// Returns `Break` when the diagnostic leaves the rest of the chain
    /// without a type.
    fn report(
        &self,
        access: &MemberAccessSite,
        outcome: &SubjectOutcome,
        out: &mut Vec<Diagnostic>,
    ) -> MemberCheckResult {
        match outcome {
            // `?->` short-circuits to `null` without touching the
            // member when its subject is `null` — it never crashes,
            // unlike every other scalar this arm flags for `->`.
            SubjectOutcome::Scalar(scalar) if access.is_nullsafe && scalar.is_null() => {
                MemberCheckResult::Ok
            }
            SubjectOutcome::Scalar(scalar) => self.push_chain_break(
                access,
                DiagnosticSeverity::Error,
                SCALAR_MEMBER_ACCESS_CODE,
                format!(
                    "Cannot access {} '{}' on type '{}'",
                    access.kind_label(),
                    access.member_name,
                    scalar,
                ),
                out,
            ),
            SubjectOutcome::UnresolvableClass(unresolved) => {
                // SoapClient is a SOAP proxy where any method is
                // valid.  Even if we cannot fully resolve the class,
                // suppress the diagnostic.
                let type_str = unresolved.to_string();
                if type_str == "SoapClient" || type_str == "\\SoapClient" {
                    return MemberCheckResult::Ok;
                }
                self.push_chain_break(
                    access,
                    DiagnosticSeverity::Warning,
                    UNKNOWN_MEMBER_CODE,
                    format!(
                        "Cannot verify {} '{}' — subject type '{}' could not be resolved",
                        access.kind_label(),
                        access.member_name,
                        unresolved,
                    ),
                    out,
                )
            }
            SubjectOutcome::Mixed | SubjectOutcome::Untyped => {
                // When the opt-in `unresolved-member-access` diagnostic
                // is enabled, report every member access the subject
                // type cannot answer for — regardless of whether the
                // subject is a bare variable, a chain, an array access,
                // or a function call result.
                if !self.report_unresolved {
                    return MemberCheckResult::Ok;
                }
                let subject_display = access.subject_text.trim();
                // A `mixed` subject is not a resolution failure:
                // the type engine answered, and the answer was the
                // type that admits every value. Saying so points at
                // the annotation that is missing from the codebase
                // instead of implying a gap in the type engine.
                let message = if matches!(outcome, SubjectOutcome::Mixed) {
                    format!(
                        "Cannot verify {} '{}' — type of '{}' is 'mixed'",
                        access.kind_label(),
                        access.member_name,
                        subject_display,
                    )
                } else {
                    format!(
                        "Cannot verify {} '{}' — type of '{}' could not be resolved",
                        access.kind_label(),
                        access.member_name,
                        subject_display,
                    )
                };
                self.push_chain_break(
                    access,
                    DiagnosticSeverity::Hint,
                    UNRESOLVED_MEMBER_ACCESS_CODE,
                    message,
                    out,
                )
            }
            SubjectOutcome::Resolved { classes, branches } => {
                self.report_on_resolved(access, classes, branches, out)
            }
        }
    }

    /// Push a diagnostic on the access after which the chain has no type.
    fn push_chain_break(
        &self,
        access: &MemberAccessSite,
        severity: DiagnosticSeverity,
        code: &str,
        message: String,
        out: &mut Vec<Diagnostic>,
    ) -> MemberCheckResult {
        let Some(range) = self.range(access) else {
            return MemberCheckResult::Ok;
        };
        out.push(make_diagnostic(range, severity, code, message));
        MemberCheckResult::Break
    }
}
