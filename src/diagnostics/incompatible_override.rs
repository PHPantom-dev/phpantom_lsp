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
        let class_loaders = self.resolved_class_loaders(&ctx.file);

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

/// The declaration `method_name` overrides, with the name its class was
/// loaded by: one up the class chain or in an interface, which the override
/// has to stay compatible with.
///
/// A class method displaces the concrete trait method it collides with,
/// and PHP runs no compatibility check on that pair. Only an abstract
/// trait method states a requirement the class has to satisfy.
fn find_parent_method(
    class: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<(Arc<MethodInfo>, String)> {
    let declares = |candidate: &ClassInfo| match candidate.kind {
        ClassLikeKind::Trait => candidate
            .get_method(method_name)
            .is_some_and(|method| method.is_abstract),
        _ => candidate.has_method(method_name),
    };
    let (name, declaring) =
        crate::inheritance::find_declaring_ancestor(class, class_loader, &declares)?;
    Some((declaring.get_method_arc(method_name)?, name.to_string()))
}
