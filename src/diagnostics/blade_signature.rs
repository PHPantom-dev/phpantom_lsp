//! What a Blade template's own signature has to agree with.
//!
//! A template and the layouts it `@extends` render from one data array, so
//! their signatures describe the same variables. [`crate::blade::contract`]
//! merges them nearest-declaration-first, which lets a template narrow what
//! a layout declared: a layout asking for `\App\Models\User` is satisfied by
//! a child that declares `\App\Models\Admin`, and the child's type is the one
//! its body is read with.
//!
//! The other direction is an error. A child that declares `string|int` where
//! its layout declared `string` promises its callers less than the layout
//! will get, and nothing downstream ever catches it: the merge keeps the
//! child's type and the layout renders with a value it did not ask for. This
//! is the check Bladestan (the PHPStan extension for Blade) performs when it
//! merges a signature chain, reported here against the declaration that
//! widens rather than against every call site that renders the template.
//!
//! A template also has exactly one contract, so a second
//! `@bladestan-signature` block is a mistake: everything below the first is
//! silently unread.

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::Backend;
use crate::blade::signature;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::PhpType;

use super::helpers::make_diagnostic;

/// A declaration the layout above it does not accept.
const COVARIANCE_CODE: &str = "blade_signature_widens_layout";
/// A second `@bladestan-signature` block in one template.
const DUPLICATE_CODE: &str = "duplicate_blade_signature";

impl Backend {
    /// Check the signature the template at `uri` declares against itself
    /// and against the layouts it renders through.
    ///
    /// Unlike the other collectors this reads the raw Blade source rather
    /// than the virtual PHP: the declarations are docblock text, and the
    /// ranges are the template's own from the start, so nothing has to go
    /// back through the source map.
    pub(super) fn collect_blade_signature_diagnostics(
        &self,
        ctx: &super::helpers::FileDiagnosticContext,
        uri: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        if !self.is_blade_file(uri) {
            return;
        }
        // The open buffer is shared rather than copied: this runs on every
        // pass over the template, and the source is only read from.
        let Some(source) = self.get_file_content_arc(uri) else {
            return;
        };
        let range_of = |span: std::ops::Range<usize>| {
            crate::text_position::byte_range_to_lsp_range(&source, span.start, span.end)
        };

        if source.contains("@bladestan-signature") {
            for block in signature::explicit_signature_docblocks(&source)
                .into_iter()
                .skip(1)
            {
                out.push(make_diagnostic(
                    range_of(block),
                    DiagnosticSeverity::ERROR,
                    DUPLICATE_CODE,
                    "This template already declares a @bladestan-signature; only the first one is read".to_string(),
                ));
            }
        }

        let declarations = signature::declarations(&source);
        if declarations.is_empty() {
            return;
        }
        // Nearest layout first, each with the declarations it contributes
        // and a resolver for the class names *it* wrote them with.
        let chain: Vec<(String, Vec<(String, PhpType)>)> = self
            .blade_layout_chain(&source)
            .into_iter()
            .map(|(name, layout_source)| (name, signature::declarations(&layout_source)))
            .filter(|(_, declared)| !declared.is_empty())
            .collect();
        if chain.is_empty() {
            return;
        }

        let view_names = self.view_names_for_blade_uri(uri);
        let qualify = self.blade_type_qualifier(view_names.first().map_or("", String::as_str));
        let class_loader = self.resolved_class_loader(&ctx.file);

        for (name, declared) in declarations {
            let Some((layout, inherited)) = chain.iter().find_map(|(layout, declarations)| {
                declarations
                    .iter()
                    .find(|(candidate, _)| candidate == &name)
                    .map(|(_, ty)| (layout, ty))
            }) else {
                continue;
            };
            let declared = qualify(&declared);
            let inherited = self.blade_type_qualifier(layout)(inherited);
            // A class neither side can load leaves the relationship
            // unknowable: `Admin` is a subtype of `User` only if both can
            // be read, and an unread one is not evidence of a mismatch.
            if !classes_resolve(&declared, &class_loader)
                || !classes_resolve(&inherited, &class_loader)
                || crate::class_lookup::is_subtype_of_typed(&declared, &inherited, &class_loader)
            {
                continue;
            }
            let Some(span) = signature::declaration_span(&source, &name) else {
                continue;
            };
            out.push(make_diagnostic(
                range_of(span),
                DiagnosticSeverity::ERROR,
                COVARIANCE_CODE,
                format!(
                    "Layout '{layout}' declares ${name} as {inherited}, which {declared} does not satisfy: a template may narrow its layout's declaration, never widen it",
                ),
            ));
        }
    }
}

/// Whether every class name the type mentions names a class that can be
/// read.
fn classes_resolve(ty: &PhpType, class_loader: &ResolvedClassLoader<'_>) -> bool {
    // `resolve_names` visits exactly the class-like names in the type tree,
    // leaving keywords (`string`, `array`, …) alone.
    let resolved = std::cell::Cell::new(true);
    ty.resolve_names(&|name: &str| {
        if class_loader(Fqn::new(name)).is_none() {
            resolved.set(false);
        }
        name.to_string()
    });
    resolved.get()
}
