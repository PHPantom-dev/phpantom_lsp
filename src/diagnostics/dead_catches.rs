//! `catch` clauses nothing in their `try` body can throw into.
//!
//! ```php
//! try {
//!     $this->repo->save($user);   // @throws ORMException only
//! } catch (JsonException $e) {    // dead catch
//! }
//! ```
//!
//! PhpStorm's "Unnecessary catch" and PHPStan's `catch.neverThrown`.  The
//! forward walk works out what each `try` body can throw (see
//! `forward_walk::throw_points`) and records every class a `catch` names
//! along with whether an exception the body throws can land in it; this
//! reports the ones none can.  A body with a call to anything that does
//! not declare what it throws can throw anything, so it never rules a
//! catch out.  In a multi-catch (`catch (A | B $e)`) each class is judged
//! on its own.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;

use super::helpers::{FileDiagnosticContext, make_tagged_diagnostic};

pub(crate) const DEAD_CATCH_CODE: &str = "dead_catch";

impl Backend {
    pub(crate) fn collect_dead_catch_diagnostics(
        &self,
        _ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        for entry in crate::type_engine::variable::forward_walk::scope_dead_catch_types() {
            if !super::incremental::reports_site(entry.start) {
                continue;
            }
            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                entry.start as usize,
                entry.end as usize,
            ) else {
                continue;
            };
            out.push(make_tagged_diagnostic(
                range,
                DiagnosticSeverity::Warning,
                DEAD_CATCH_CODE,
                format!(
                    "Nothing in the try block throws {}",
                    entry.class.trim_start_matches('\\')
                ),
                Some(DiagnosticTag::Unnecessary),
            ));
        }
    }
}
