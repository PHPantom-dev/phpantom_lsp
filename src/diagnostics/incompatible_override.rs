use std::collections::HashSet;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::php_type::{PhpType, TypeKind};
use crate::symbol_map::SymbolKind;
use crate::types::{ClassInfo, ClassLikeKind, MethodInfo};

use super::helpers::{FileDiagnosticContext, make_diagnostic};

impl Backend {
    pub fn collect_incompatible_override_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(ctx) = FileDiagnosticContext::gather(self, uri) else {
            return;
        };
        self.collect_incompatible_override_diagnostics_with_context(&ctx, uri, content, out);
    }

    pub(crate) fn collect_incompatible_override_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let class_loaders = self.class_loaders(&ctx.file);

        for span in &ctx.symbol_map.spans {
            let class_name = match &span.kind {
                SymbolKind::ClassDeclaration { name } => name,
                _ => continue,
            };

            let class_info = match ctx.declared_class(class_name, span.start) {
                Some(c) => c,
                None => continue,
            };
            let class_loader = class_loaders.at(span.start);

            if class_info.kind == ClassLikeKind::Interface
                || class_info.kind == ClassLikeKind::Trait
            {
                continue;
            }

            for method in &class_info.methods {
                if method.is_virtual || method.name_offset == 0 {
                    continue;
                }

                let child_native = match &method.native_return_type {
                    Some(t) => t,
                    None => continue,
                };

                // `never` is the bottom type, so it narrows any return type
                // the ancestor declared. A method that only ever throws is
                // free to say so.
                if matches!(child_native.kind(), TypeKind::Named(n) if *n == "never") {
                    continue;
                }

                if let Some((parent_method, source_name)) =
                    find_parent_method(class_info, &method.name, class_loader)
                {
                    let parent_native = match &parent_method.native_return_type {
                        Some(t) => t,
                        None => continue,
                    };

                    if contains_static_type(parent_native) && !contains_static_type(child_native) {
                        let range = match self.offset_range_to_lsp_range(
                            uri,
                            content,
                            method.name_offset as usize,
                            method.name_offset as usize + method.name.len(),
                        ) {
                            Some(r) => r,
                            None => continue,
                        };
                        out.push(make_diagnostic(
                            range,
                            DiagnosticSeverity::ERROR,
                            "incompatible_override",
                            format!(
                                "Declaration of {}::{}() must be compatible with {}::{}(): return type '{}' must be 'static'",
                                class_info.name, method.name, source_name, method.name, child_native
                            ),
                        ));
                    }
                }
            }
        }
    }
}

fn contains_static_type(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::StaticType(_) | TypeKind::ThisType(_) => true,
        TypeKind::Named(name) => *name == "static",
        TypeKind::Nullable(inner) => contains_static_type(inner),
        TypeKind::Union(types) | TypeKind::Intersection(types) => {
            types.iter().any(contains_static_type)
        }
        _ => false,
    }
}

fn find_parent_method(
    class: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<(Arc<MethodInfo>, String)> {
    let lower = method_name.to_lowercase();
    let mut visited = HashSet::new();

    if let Some(ref parent_name) = class.parent_class
        && let Some(result) = find_method_in_chain(parent_name, &lower, class_loader, &mut visited)
    {
        return Some(result);
    }

    for iface_name in &class.interfaces {
        if let Some(result) = find_method_in_chain(iface_name, &lower, class_loader, &mut visited) {
            return Some(result);
        }
    }

    // A class method displaces the concrete trait method it collides with,
    // and PHP runs no compatibility check on that pair. Only an abstract
    // trait method states a requirement the class has to satisfy.
    for trait_name in &class.used_traits {
        if let Some(result) =
            find_abstract_method_in_traits(trait_name, &lower, class_loader, &mut visited)
        {
            return Some(result);
        }
    }

    None
}

fn find_abstract_method_in_traits(
    trait_name: &str,
    method_lower: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    visited: &mut HashSet<String>,
) -> Option<(Arc<MethodInfo>, String)> {
    if !visited.insert(trait_name.to_string()) {
        return None;
    }

    let trait_info = class_loader(trait_name)?;

    if let Some(method) = trait_info
        .methods
        .iter()
        .find(|m| m.is_abstract && m.name.to_lowercase() == *method_lower)
    {
        return Some((Arc::clone(method), trait_name.to_string()));
    }

    for nested in &trait_info.used_traits {
        if let Some(result) =
            find_abstract_method_in_traits(nested, method_lower, class_loader, visited)
        {
            return Some(result);
        }
    }

    None
}

fn find_method_in_chain(
    class_name: &str,
    method_lower: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    visited: &mut HashSet<String>,
) -> Option<(Arc<MethodInfo>, String)> {
    if !visited.insert(class_name.to_string()) {
        return None;
    }

    let class = class_loader(class_name)?;

    if let Some(method) = class
        .methods
        .iter()
        .find(|m| m.name.to_lowercase() == *method_lower)
    {
        return Some((Arc::clone(method), class_name.to_string()));
    }

    if let Some(ref parent) = class.parent_class
        && let Some(result) = find_method_in_chain(parent, method_lower, class_loader, visited)
    {
        return Some(result);
    }

    for iface in &class.interfaces {
        if let Some(result) = find_method_in_chain(iface, method_lower, class_loader, visited) {
            return Some(result);
        }
    }

    None
}
