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
        let symbol_map = {
            let maps = self.symbol_maps.read();
            match maps.get(uri) {
                Some(sm) => sm.clone(),
                None => return,
            }
        };

        let ctx = self.file_context(uri);
        let class_loader = self.class_loader(&ctx);

        // Iterate all ClassDeclaration spans in the symbol map.
        for span in &symbol_map.spans {
            let class_name = match &span.kind {
                SymbolKind::ClassDeclaration { name } => name,
                _ => continue,
            };

            // Find the matching ClassInfo in the uri_classes_index.
            let class_info = match ctx
                .classes
                .iter()
                .find(|c| c.name == *class_name || self.class_fqn_matches(c, class_name, &ctx))
            {
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

            // Skip classes with no interfaces and no parent class — they
            // cannot have missing method implementations.
            if class_info.interfaces.is_empty() && class_info.parent_class.is_none() {
                continue;
            }

            let missing = collect_missing_methods(&class_info, &class_loader);

            if missing.is_empty() {
                continue;
            }

            // Build the diagnostic range from the class name span.
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

            out.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(NumberOrString::String("missing_implementation".to_string())),
                code_description: None,
                source: Some("phpantom".to_string()),
                message,
                related_information: None,
                tags: None,
                data: None,
            });
        }
    }

    /// Check if a ClassInfo's fully-qualified name matches the given name.
    ///
    /// The symbol map stores the short class name, but classes in the
    /// uri_classes_index may have their FQN stored differently.  This handles the
    /// common case where the class name is unqualified.
    fn class_fqn_matches(
        &self,
        class: &crate::types::ClassInfo,
        name: &str,
        ctx: &crate::types::FileContext,
    ) -> bool {
        // Build FQN from namespace + class name and compare.
        if let Some(ref ns) = ctx.namespace {
            let fqn = format!("{}\\{}", ns, class.name);
            fqn == name || class.name == name
        } else {
            class.name == name
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
    // Check interfaces first.
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

    // Fallback — shouldn't happen if collect_missing_methods found it.
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

    // Check parent interfaces.
    for iface_name in &class.interfaces {
        if let Some(iface) = class_loader(iface_name)
            && has_method_in_chain(&iface, method_name, class_loader, visited)
        {
            return true;
        }
    }

    // Check parent class.
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
