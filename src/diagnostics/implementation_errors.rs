//! Implementation error diagnostic.
//!
//! Flags concrete classes that fail to implement all required methods
//! from their interfaces or abstract parents.  Reuses the same
//! missing-method detection logic as the "Implement missing methods"
//! code action (`code_actions::implement_methods::collect_missing_methods`).

use std::collections::HashSet;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
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
        let Some(ctx) = FileDiagnosticContext::gather(self, uri) else {
            return;
        };
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
        let class_loader = self.class_loader(&ctx.file);

        for span in &symbol_map.spans {
            let class_name = match &span.kind {
                SymbolKind::ClassDeclaration { name } => name,
                _ => continue,
            };

            // Find the matching ClassInfo in the uri_classes_index.
            let class_info = match ctx.declared_class(class_name) {
                Some(c) => Arc::clone(c),
                None => continue,
            };

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

            let missing = collect_missing_methods(&class_info, &class_loader);

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
                let source = method_source_description(&class_info, &m.name, &class_loader);
                format!(
                    "{} '{}' must implement method '{}()' from {}",
                    kind_label, class_info.name, m.name, source
                )
            } else {
                let method_list: Vec<String> = missing
                    .iter()
                    .map(|m| {
                        let source = method_source_description(&class_info, &m.name, &class_loader);
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
                DiagnosticSeverity::ERROR,
                "missing_implementation",
                message,
            ));
        }
    }
}

/// Describe where a missing method was required from (interface or
/// abstract parent class).
fn method_source_description(
    class: &crate::types::ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> String {
    for iface_name in &class.interfaces {
        if let Some(iface) = class_loader(iface_name)
            && has_method_in_chain(&iface, method_name, class_loader, &mut HashSet::new())
        {
            return format!("interface '{}'", iface_name);
        }
    }

    // Check parent chain for abstract methods.
    if let Some(ref parent_name) = class.parent_class
        && let Some(parent) = class_loader(parent_name)
        && has_abstract_method_in_chain(&parent, method_name, class_loader, &mut HashSet::new())
    {
        return format!("class '{}'", parent_name);
    }

    // Check used traits for abstract methods.
    for trait_name in &class.used_traits {
        if let Some(trait_info) = class_loader(trait_name)
            && has_abstract_method_in_chain(
                &trait_info,
                method_name,
                class_loader,
                &mut HashSet::new(),
            )
        {
            return format!("trait '{}'", trait_name);
        }
    }

    "its hierarchy".to_string()
}

/// Check if a class or its parent chain declares a method (abstract or not).
fn has_method_in_chain(
    class: &crate::types::ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
    visited: &mut HashSet<String>,
) -> bool {
    if !visited.insert(class.name.to_string()) {
        return false;
    }

    let lower = method_name.to_lowercase();
    if class.methods.iter().any(|m| m.name.to_lowercase() == lower) {
        return true;
    }

    for iface_name in &class.interfaces {
        if let Some(iface) = class_loader(iface_name)
            && has_method_in_chain(&iface, method_name, class_loader, visited)
        {
            return true;
        }
    }

    if let Some(ref parent_name) = class.parent_class
        && let Some(parent) = class_loader(parent_name)
        && has_method_in_chain(&parent, method_name, class_loader, visited)
    {
        return true;
    }

    false
}

/// Check if a class or its parent chain declares an abstract method.
fn has_abstract_method_in_chain(
    class: &crate::types::ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
    visited: &mut HashSet<String>,
) -> bool {
    if !visited.insert(class.name.to_string()) {
        return false;
    }

    let lower = method_name.to_lowercase();
    if class
        .methods
        .iter()
        .any(|m| m.name.to_lowercase() == lower && m.is_abstract)
    {
        return true;
    }

    if let Some(ref parent_name) = class.parent_class
        && let Some(parent) = class_loader(parent_name)
        && has_abstract_method_in_chain(&parent, method_name, class_loader, visited)
    {
        return true;
    }

    false
}
