//! Hover for a Twig filter or function: the signature of the PHP method or
//! function behind it, and the extension that declares it.

use crate::Backend;
use crate::class_lookup::Fqn;
use crate::symbol_map::{StringKeyKind, TwigCallableTarget};

use super::formatting::{format_native_params, format_visibility};

impl Backend {
    /// What the hover of the `kind` callable `name` says below its name.
    pub(super) fn twig_callable_detail(&self, kind: StringKeyKind, name: &str) -> String {
        let callables = self.twig_callables();
        let Some(callable) = callables.matching(kind, name).into_iter().next() else {
            return "Not declared by any extension".to_string();
        };
        let declared_in = self
            .workspace_relative_path(&callable.uri)
            .unwrap_or_else(|| callable.uri.clone());
        let (signature, description, runs) = match &callable.target {
            TwigCallableTarget::Method { class, method } => {
                let found = self.find_or_load_class(class).and_then(|owner| {
                    let loader = |name: Fqn<'_>| self.find_or_load_class(&name);
                    let merged = crate::virtual_members::resolve_class_fully_cached(
                        &owner,
                        &loader,
                        &self.resolved_class_cache,
                    );
                    merged.get_method(method).cloned()
                });
                let signature = found.as_ref().map(|found| {
                    format!(
                        "{}{}function {}({}){}",
                        format_visibility(found.visibility),
                        if found.is_static { "static " } else { "" },
                        found.name,
                        format_native_params(&found.parameters),
                        found
                            .native_return_type
                            .as_ref()
                            .map(|ret| format!(": {ret}"))
                            .unwrap_or_default()
                    )
                });
                let description = found.and_then(|found| found.description.clone());
                (
                    signature,
                    description,
                    Some(format!("`{class}::{method}()`")),
                )
            }
            TwigCallableTarget::Function(function) => {
                let found = self.find_or_load_function(&[function.as_ref()]);
                let signature = found.as_ref().map(|found| {
                    format!(
                        "function {}({}){}",
                        found.name,
                        format_native_params(&found.parameters),
                        found
                            .native_return_type
                            .as_ref()
                            .map(|ret| format!(": {ret}"))
                            .unwrap_or_default()
                    )
                });
                let description = found.and_then(|found| found.description);
                (signature, description, Some(format!("`{function}()`")))
            }
            TwigCallableTarget::Inline => (None, None, None),
        };
        let mut lines = Vec::new();
        if let Some(signature) = signature {
            lines.push(format!("```php\n<?php\n{signature}\n```"));
        }
        if let Some(description) = description {
            lines.push(description);
        }
        lines.push(match runs {
            Some(runs) => format!("Runs {runs}, declared in `{declared_in}`"),
            None => format!("Declared in `{declared_in}`"),
        });
        lines.join("\n\n")
    }
}
