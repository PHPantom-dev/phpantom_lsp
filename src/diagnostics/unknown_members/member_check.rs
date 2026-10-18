//! Judging a member access whose subject resolved to classes: whether the
//! member exists on any of them, is reachable from the calling scope, or
//! is handled by a magic method.

use std::sync::Arc;

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::types::ClassInfo;
use crate::virtual_members::resolve_class_fully_cached;

use super::super::helpers::make_diagnostic;
use super::super::member_lookup::{
    display_class_name, has_magic_method_for_access, member_exists, member_exists_relaxed,
    member_is_public,
};
use super::super::member_visibility::{INVALID_MEMBER_ACCESS_CODE, inaccessible_member_message};
use super::access::MemberAccessSite;
use super::{MemberCheckResult, UNKNOWN_MEMBER_CODE, UnknownMemberPass};

impl UnknownMemberPass<'_> {
    /// Check the member on the classes the subject resolved to, and push
    /// what the check reports.
    pub(super) fn report_on_resolved(
        &self,
        access: &MemberAccessSite,
        base_classes: &[Arc<ClassInfo>],
        out: &mut Vec<Diagnostic>,
    ) -> MemberCheckResult {
        let (result, coarse_diags) = self.check_member_on_resolved_classes(access, base_classes);

        // ── Narrowing re-resolution fallback ────────────
        // When the member was not found on the coarsely-cached
        // type AND the subject is a bare variable, re-resolve
        // at the exact cursor position.  Expression-level
        // narrowing (ternary instanceof, inline && chains)
        // may refine the type so the member becomes visible.
        //
        // This is the rare path — most accesses find the
        // member on the coarse type and never reach here.
        let (result, diags) = if result != MemberCheckResult::Ok && access.is_narrowable_subject() {
            match self.resolve_subject(access) {
                // Use the fresh diagnostics instead of the coarse ones.
                crate::type_engine::resolver::SubjectOutcome::Resolved(fresh_classes) => {
                    self.check_member_on_resolved_classes(access, &fresh_classes)
                }
                // Re-resolution changed the outcome category (e.g.
                // became Untyped).  Keep the original diagnostic from
                // the coarse check.
                _ => (result, coarse_diags),
            }
        } else {
            (result, coarse_diags)
        };
        out.extend(diags);
        result
    }

    /// Emit an access diagnostic when the member exists but the calling
    /// scope may not reach it.  Returns whether one was emitted.
    ///
    /// Called wherever the member check has settled what the classes
    /// hold: at the two points that declare the member found, and once
    /// more before reporting it missing, since a parent's private member
    /// is unreachable rather than absent.
    ///
    /// A docblock reference is exempt — `@see Foo::$secret` documents a
    /// member, it does not read one.
    fn push_inaccessible_member(
        &self,
        access: &MemberAccessSite,
        classes: &[Arc<ClassInfo>],
        diagnostics: &mut Vec<Diagnostic>,
    ) -> bool {
        if access.is_docblock_ref {
            return false;
        }
        let Some(message) = inaccessible_member_message(
            classes,
            access.member_name,
            access.is_static,
            access.is_method_call,
            access.current_class,
            access.subject_binds_scope(),
            self.class_loader(),
        ) else {
            return false;
        };
        let Some(range) = self.range(access) else {
            return false;
        };
        diagnostics.push(make_diagnostic(
            range,
            DiagnosticSeverity::ERROR,
            INVALID_MEMBER_ACCESS_CODE,
            message,
        ));
        true
    }

    /// Check whether a member exists on the resolved classes and emit
    /// a diagnostic if it does not.
    ///
    /// Returns the check result (whether the access is fine, breaks the
    /// type chain, or is recoverable via a magic-method fallback) along
    /// with any diagnostics to emit.
    fn check_member_on_resolved_classes(
        &self,
        access: &MemberAccessSite,
        base_classes: &[Arc<ClassInfo>],
    ) -> (MemberCheckResult, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let report_magic = self.report_magic;
        let member_name = access.member_name;
        let is_static = access.is_static;
        let is_method_call = access.is_method_call;
        let is_docblock_ref = access.is_docblock_ref;

        // ── Quick check on pre-resolved base classes ────────────────
        // `resolve_target_classes` already returns fully-resolved
        // classes in many code paths (e.g. `type_hint_to_classes_typed`
        // calls `resolve_class_fully` and injects model-specific
        // scope methods onto Eloquent Builders).  Check the member
        // on these classes FIRST, before re-resolving through the
        // cache.  The cache is keyed by bare FQN and may hold a
        // stale entry that lacks context-specific virtual members
        // (e.g. Builder scope methods that depend on the concrete
        // model type).  Checking here avoids false positives when
        // the cache and the resolver disagree.

        // ── Suppress property access on __get classes ───────────────
        // `__get` handles arbitrary property names.  Unlike __call,
        // we suppress the diagnostic entirely because there is no
        // meaningful return-type recovery to perform.
        if !is_method_call
            && base_classes
                .iter()
                .any(|c| has_magic_method_for_access(c, is_static, false, report_magic))
        {
            return (MemberCheckResult::Ok, diagnostics);
        }
        if base_classes.iter().any(|c| c.name == "stdClass") {
            return (MemberCheckResult::Ok, diagnostics);
        }
        // This shortcut may be looking at a class that has not been
        // merged yet, which is enough to confirm a member is reachable
        // but not enough to judge one that is not: a magic handler or
        // the declaration's real owner can both live further up.  So it
        // settles only the public case and lets everything else fall
        // through to the merged classes below.
        if base_classes.iter().any(|c| {
            member_is_public(c, member_name, is_static, is_method_call)
                || (is_docblock_ref && member_exists_relaxed(c, member_name))
        }) {
            return (MemberCheckResult::Ok, diagnostics);
        }

        // ── Fully resolve each class (inheritance + virtual members) ─
        // Synthetic classes like `__object_shape` already carry all
        // their members and must NOT go through the cache (every
        // object shape shares the same name, so the cache would
        // return the wrong entry).
        let resolved_classes: Vec<Arc<ClassInfo>> = base_classes
            .iter()
            .map(|c| {
                if c.name == "__object_shape" {
                    Arc::clone(c)
                } else {
                    resolve_class_fully_cached(
                        c,
                        self.class_loader(),
                        &self.backend.resolved_class_cache,
                    )
                }
            })
            .collect();

        // ── Suppress property access on __get classes (resolved) ────
        if !is_method_call
            && resolved_classes
                .iter()
                .any(|c| has_magic_method_for_access(c, is_static, false, report_magic))
        {
            return (MemberCheckResult::Ok, diagnostics);
        }

        // ── Skip stdClass (universal object container) ──────────────
        if resolved_classes.iter().any(|c| c.name == "stdClass") {
            return (MemberCheckResult::Ok, diagnostics);
        }

        // ── Check whether the member exists on ANY branch ───────────
        if resolved_classes.iter().any(|c| {
            member_exists(c, member_name, is_static, is_method_call)
                || (is_docblock_ref && member_exists_relaxed(c, member_name))
        }) {
            self.push_inaccessible_member(access, &resolved_classes, &mut diagnostics);
            return (MemberCheckResult::Ok, diagnostics);
        }

        // ── Suppress method calls dispatched through __call / __callStatic ──
        // When any branch has a magic call handler, the method is
        // dispatched to it at runtime, so the call is valid PHP — there
        // is no fatal error and no undefined member.  This is how
        // proxies (SoapClient), mock/fluent APIs (Mockery's higher-order
        // messages), and dynamic query builders work.  We suppress the
        // diagnostic entirely (mirroring how `__get` suppresses
        // property-access diagnostics above) and return `MagicFallback`
        // so the chain keeps resolving through the magic method's return
        // type.  A speculative "we can't verify this" warning here is a
        // false positive: the receiver has explicitly opted into handling
        // arbitrary method names.
        let has_magic_call = is_method_call
            && (base_classes
                .iter()
                .any(|c| has_magic_method_for_access(c, is_static, true, report_magic))
                || resolved_classes
                    .iter()
                    .any(|c| has_magic_method_for_access(c, is_static, true, report_magic)));
        if has_magic_call {
            return (MemberCheckResult::MagicFallback, diagnostics);
        }

        // ── A parent's private member is unreachable, not missing ──
        // The inheritance merge drops it, so without this the access
        // would be reported as an unknown member and the real reason
        // — that PHP does not inherit private members — would be lost.
        if self.push_inaccessible_member(access, &resolved_classes, &mut diagnostics) {
            // The member is real, so its type still carries the chain.
            return (MemberCheckResult::Ok, diagnostics);
        }

        // ── Member is unresolved on ALL branches — emit diagnostic ──
        let Some(range) = self.range(access) else {
            return (MemberCheckResult::Ok, diagnostics);
        };

        let kind_label = if is_method_call {
            "Method"
        } else if is_static {
            // Static non-method could be a property ($prop) or constant
            "Member"
        } else {
            "Property"
        };

        // Show the first resolved class name for context.  For union
        // types we could list all of them, but keeping it short is
        // more useful in the editor gutter.
        let class_display = display_class_name(&resolved_classes[0]);

        let message = if resolved_classes.len() > 1 {
            format!(
                "{} '{}' not found on any of the {} possible types ({})",
                kind_label,
                member_name,
                resolved_classes.len(),
                resolved_classes
                    .iter()
                    .map(|c| display_class_name(c))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        } else {
            format!(
                "{} '{}' not found on class '{}'",
                kind_label, member_name, class_display,
            )
        };

        diagnostics.push(make_diagnostic(
            range,
            DiagnosticSeverity::WARNING,
            UNKNOWN_MEMBER_CODE,
            message,
        ));

        // The member exists on no branch and no branch has a magic call
        // handler, so the type cannot be recovered — break the chain.
        (MemberCheckResult::Break, diagnostics)
    }
}
