//! Sections and stacks nothing renders.
//!
//! A `@section('sidebar')` whose layout yields `sidebar_content`, or a
//! `@push('script')` under a layout that stacks `scripts`, is not an error
//! Blade reports: the content is collected, nothing asks for it, and the
//! page renders without it. The template looks right and the browser shows
//! nothing, which is the hardest kind of mistake to find.
//!
//! The check runs only where the answer is knowable. A template has to say
//! what renders it (`@extends`), and the walk of that chain — layouts, and
//! the partials they include — has to have read every file in it. A view
//! name built at runtime, a component tag whose template this cannot
//! follow, or a layout no view root holds all leave a hole the missing
//! `@yield` could be in, so the template is left alone rather than
//! reported against a set of names known to be incomplete.

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::Backend;
use crate::blade::blocks::{self, BlockKind, BlockRole};

/// A section filled under a name nothing in the render tree yields.
const UNRENDERED_SECTION_CODE: &str = "unrendered_blade_section";
/// A stack pushed to under a name nothing in the render tree renders.
const UNRENDERED_STACK_CODE: &str = "unrendered_blade_stack";

impl Backend {
    /// Check that every section and stack the template at `uri` fills is
    /// rendered by something above it.
    pub(super) fn collect_blade_section_diagnostics(&self, uri: &str, out: &mut Vec<Diagnostic>) {
        if !self.is_blade_file(uri) {
            return;
        }
        let Some(source) = self.get_file_content_arc(uri) else {
            return;
        };
        let own = blocks::analyse(&source, &self.blade_directives.read());
        if own.extends.is_empty() || !own.blocks.iter().any(|block| block.role == BlockRole::Fill) {
            // A template that names no layout is rendered by something
            // this cannot see — a controller, an `@include` from anywhere —
            // and what renders its sections is that caller's business.
            return;
        }

        let (scope, complete) = self.blade_render_scope(uri, &source);
        if !complete {
            return;
        }

        for block in own.blocks.iter().filter(|b| b.role == BlockRole::Fill) {
            if scope
                .iter()
                .any(|entry| entry.blocks.consumes(block.kind, &block.name))
            {
                continue;
            }
            let (code, renders) = match block.kind {
                BlockKind::Section => (UNRENDERED_SECTION_CODE, "@yield"),
                BlockKind::Stack => (UNRENDERED_STACK_CODE, "@stack"),
            };
            out.push(super::helpers::make_diagnostic(
                crate::text_position::byte_range_to_lsp_range(
                    &source,
                    block.name_span.start,
                    block.name_span.end,
                ),
                DiagnosticSeverity::WARNING,
                code,
                format!(
                    "Nothing renders the {} '{}': no {renders}('{}') in the layouts this template extends",
                    block.kind.label(),
                    block.name,
                    block.name
                ),
            ));
        }
    }
}
