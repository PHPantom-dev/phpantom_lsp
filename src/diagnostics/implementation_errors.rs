//! Implementation error diagnostic.
//!
//! Flags concrete classes that fail to implement all required methods
//! from their interfaces or abstract parents.  Reuses the same
//! missing-method detection logic as the "Implement missing methods"
//! code action (`code_actions::implement_methods::collect_missing_methods`).

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::code_actions::implement_methods::collect_missing_methods;
use crate::symbol_map::SymbolKind;
use crate::types::ClassLikeKind;

use super::helpers::{FileDiagnosticContext, make_diagnostic};

impl Backend {
    /// Collect implementation-error diagnostics for a single file.
    ///
    /// For each concrete (non-abstract) class in the file, checks whether
    /// all required methods from interfaces and abstract parents are
    /// implemented.  Emits an Error-severity diagnostic on the class name
    /// span for each class that has missing methods.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing or returning them.
    pub fn collect_implementation_error_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_implementation_error_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// Same as [`Self::collect_implementation_error_diagnostics`] but
    /// reuses an already-gathered [`FileDiagnosticContext`] instead of
    /// re-reading the per-file locks. Used by `collect_slow_diagnostics`
    /// so all slow collectors in the same pass share one consistent
    /// snapshot.
    pub(crate) fn collect_implementation_error_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let symbol_map = &ctx.symbol_map;
        let class_loaders = self.resolved_class_loaders(&ctx.file);

        for span in &symbol_map.spans {
            let class_name = match &span.kind {
                SymbolKind::ClassDeclaration { name } => name,
                _ => continue,
            };

            // Find the matching ClassInfo in the uri_classes_index.
            let class_info = match ctx.declared_class(class_name, span.start) {
                Some(c) => Arc::clone(c),
                None => continue,
            };
            let class_loader = class_loaders.at(span.start);

            // Only concrete classes and enums can have implementation errors.
            // Abstract classes, interfaces, and traits are skipped.
            let is_concrete_class =
                class_info.kind == ClassLikeKind::Class && !class_info.is_abstract;
            let is_enum = class_info.kind == ClassLikeKind::Enum;
            if !is_concrete_class && !is_enum {
                continue;
            }

            if class_info.interfaces.is_empty()
                && class_info.parent_class.is_none()
                && class_info.used_traits.is_empty()
            {
                continue;
            }

            let missing = collect_missing_methods(&class_info, class_loader);

            if missing.is_empty() {
                continue;
            }

            let range = match self.offset_range_to_lsp_range(
                uri,
                content,
                span.start as usize,
                span.end as usize,
            ) {
                Some(r) => r,
                None => continue,
            };

            // Build a single diagnostic listing all missing methods.
            let kind_label = if class_info.kind == ClassLikeKind::Enum {
                "Enum"
            } else {
                "Class"
            };

            let message = if missing.len() == 1 {
                let m = &missing[0];
                let source = method_source_description(&class_info, &m.name, class_loader);
                format!(
                    "{} '{}' must implement method '{}()' from {}",
                    kind_label, class_info.name, m.name, source
                )
            } else {
                let method_list: Vec<String> = missing
                    .iter()
                    .map(|m| {
                        let source = method_source_description(&class_info, &m.name, class_loader);
                        format!("'{}()' from {}", m.name, source)
                    })
                    .collect();
                format!(
                    "{} '{}' must implement {} methods: {}",
                    kind_label,
                    class_info.name,
                    missing.len(),
                    method_list.join(", ")
                )
            };

            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::Error,
                "missing_implementation",
                message,
            ));
        }
    }
}

/// Describe where a missing method was required from: the interface,
/// parent class, or trait the class names directly that leads to the
/// declaration.
fn method_source_description(
    class: &crate::types::ClassInfo,
    method_name: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> String {
    use crate::inheritance::{ancestors, find_declaring_ancestor, find_declaring_trait};

    let declares = |c: &crate::types::ClassInfo| c.has_method(method_name);
    let declares_abstract = |c: &crate::types::ClassInfo| {
        c.get_method(method_name)
            .is_some_and(|method| method.is_abstract)
    };

    for iface_name in &class.interfaces {
        if let Some(iface) = class_loader(Fqn::new(iface_name))
            && (declares(&iface)
                || find_declaring_ancestor(&iface, class_loader, &declares).is_some())
        {
            return format!("interface '{}'", iface_name);
        }
    }

    // Check the parent chain, and the traits each parent uses, for abstract
    // methods.
    if let Some(ref parent_name) = class.parent_class
        && ancestors(class, class_loader).any(|(_, ancestor)| {
            declares_abstract(&ancestor)
                || find_declaring_trait(&ancestor.used_traits, class_loader, &declares_abstract)
                    .is_some()
        })
    {
        return format!("class '{}'", parent_name);
    }

    // Check used traits for abstract methods.
    for trait_name in &class.used_traits {
        if find_declaring_trait(
            std::slice::from_ref(trait_name),
            class_loader,
            &declares_abstract,
        )
        .is_some()
        {
            return format!("trait '{}'", trait_name);
        }
    }

    "its hierarchy".to_string()
}
