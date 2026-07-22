//! Invalid class-like kind in context diagnostics.
//!
//! Flags class-like names that appear in syntactic positions where their
//! kind (class, interface, trait, enum) is guaranteed to fail at runtime
//! or be silently useless.  For example, `new` on an abstract class,
//! `implements` with a trait, or `instanceof` with a trait.
//!
//! The rule table mirrors the completion system's
//! [`ClassNameContext`](crate::completion::context::class_completion::ClassNameContext)
//! filtering — completion prevents inserting a wrong kind; this
//! diagnostic catches wrong kinds already in the code.
//!
//! Only references where the target class is loaded (in `uri_classes_index` or
//! stubs) are flagged.  Unknown classes are not reported here (that is
//! the unknown-class diagnostic's job).

use std::collections::HashSet;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::names::OwnedResolvedNames;
use crate::symbol_map::{ClassRefContext, SymbolKind};
use crate::types::{ClassInfo, ClassLikeKind};

use super::helpers::{
    compute_use_line_ranges, is_offset_in_ranges, make_diagnostic, resolve_to_fqn,
};

/// Diagnostic code used for invalid-class-kind diagnostics.
pub(crate) const INVALID_CLASS_KIND_CODE: &str = "invalid_class_kind";

impl Backend {
    /// Collect invalid-class-kind diagnostics for a single file.
    ///
    /// Walks the precomputed [`SymbolMap`] and checks every
    /// `ClassReference` whose [`ClassRefContext`] is not `Other`.
    /// When the referenced class is loaded and its kind does not match
    /// the position, a diagnostic is emitted.
    ///
    /// Appends diagnostics to `out`.
    pub fn collect_invalid_class_kind_diagnostics(
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

        let file_resolved_names: Option<Arc<OwnedResolvedNames>> =
            self.resolved_names.read().get(uri).cloned();

        let file_use_map = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);

        let local_classes: Vec<Arc<ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();

        let use_line_ranges = compute_use_line_ranges(content);

        let ctx = self.file_context(uri);
        let class_loader = self.class_loader(&ctx);

        for span in &symbol_map.spans {
            let (ref_name, is_fqn, ref_ctx) = match &span.kind {
                SymbolKind::ClassReference {
                    name,
                    is_fqn,
                    context,
                } => (name.as_str(), *is_fqn, *context),
                _ => continue,
            };

            // Only check references with a known context.  Attribute
            // usages (`#[Foo]`) are valid on any instantiable class, so
            // they are skipped just like `Other`.
            if ref_ctx == ClassRefContext::Other || ref_ctx == ClassRefContext::Attribute {
                continue;
            }

            // Skip use-import lines.
            if is_offset_in_ranges(span.start, &use_line_ranges) {
                continue;
            }

            // Skip template parameters.
            if !is_fqn
                && !ref_name.contains('\\')
                && symbol_map.find_template_def(ref_name, span.start).is_some()
            {
                continue;
            }

            // Resolve to FQN.
            let fqn = if is_fqn {
                ref_name.to_string()
            } else if let Some(ref rn) = file_resolved_names {
                rn.get(span.start)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| resolve_to_fqn(ref_name, &file_use_map, &file_namespace))
            } else {
                resolve_to_fqn(ref_name, &file_use_map, &file_namespace)
            };

            // Try to load the class.  If it's not found, skip — the
            // unknown-class diagnostic handles that case.
            let class_info = if let Some(ci) = local_classes
                .iter()
                .find(|c| c.name == ref_name || c.fqn() == fqn)
            {
                Arc::clone(ci)
            } else if let Some(ci) = self.find_or_load_class(&fqn) {
                ci
            } else {
                continue;
            };

            // Check the class kind against the context and build a
            // diagnostic if it's invalid.
            if let Some((severity, message)) =
                check_kind_in_context(&class_info, ref_ctx, &fqn, &class_loader)
            {
                let range = match self.offset_range_to_lsp_range(
                    uri,
                    content,
                    span.start as usize,
                    span.end as usize,
                ) {
                    Some(r) => r,
                    None => continue,
                };

                out.push(make_diagnostic(
                    range,
                    severity,
                    INVALID_CLASS_KIND_CODE,
                    message,
                ));
            }
        }
    }
}

/// Check whether a class's kind is valid for the given context.
///
/// Returns `Some((severity, message))` when the kind is invalid, or
/// `None` when the usage is valid.
fn check_kind_in_context(
    class: &ClassInfo,
    ctx: ClassRefContext,
    fqn: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<(DiagnosticSeverity, String)> {
    let kind = class.kind;

    match ctx {
        ClassRefContext::New => {
            // Cannot instantiate abstract classes, interfaces, traits, or enums.
            match kind {
                ClassLikeKind::Interface => Some((
                    DiagnosticSeverity::ERROR,
                    format!("Cannot instantiate interface '{}'", fqn),
                )),
                ClassLikeKind::Trait => Some((
                    DiagnosticSeverity::ERROR,
                    format!("Cannot instantiate trait '{}'", fqn),
                )),
                ClassLikeKind::Enum => Some((
                    DiagnosticSeverity::ERROR,
                    format!("Cannot instantiate enum '{}'", fqn),
                )),
                ClassLikeKind::Class if class.is_abstract => Some((
                    DiagnosticSeverity::ERROR,
                    format!("Cannot instantiate abstract class '{}'", fqn),
                )),
                _ => None,
            }
        }
        ClassRefContext::ExtendsClass => {
            // `class A extends X` — X must be a non-final class.
            match kind {
                ClassLikeKind::Interface => Some((
                    DiagnosticSeverity::ERROR,
                    format!(
                        "'{}' is an interface and cannot be used in 'extends' for a class (use 'implements' instead)",
                        fqn
                    ),
                )),
                ClassLikeKind::Trait => Some((
                    DiagnosticSeverity::ERROR,
                    format!(
                        "'{}' is a trait and cannot be used in 'extends' (use 'use' inside the class body instead)",
                        fqn
                    ),
                )),
                ClassLikeKind::Enum => Some((
                    DiagnosticSeverity::ERROR,
                    format!("'{}' is an enum and cannot be extended", fqn),
                )),
                ClassLikeKind::Class if class.is_final => Some((
                    DiagnosticSeverity::ERROR,
                    format!("Cannot extend final class '{}'", fqn),
                )),
                _ => None,
            }
        }
        ClassRefContext::ExtendsInterface => {
            // `interface A extends X` — X must be an interface.
            match kind {
                ClassLikeKind::Class => Some((
                    DiagnosticSeverity::ERROR,
                    format!(
                        "'{}' is a class, but interfaces can only extend other interfaces",
                        fqn
                    ),
                )),
                ClassLikeKind::Trait => Some((
                    DiagnosticSeverity::ERROR,
                    format!(
                        "'{}' is a trait, but interfaces can only extend other interfaces",
                        fqn
                    ),
                )),
                ClassLikeKind::Enum => Some((
                    DiagnosticSeverity::ERROR,
                    format!(
                        "'{}' is an enum, but interfaces can only extend other interfaces",
                        fqn
                    ),
                )),
                _ => None,
            }
        }
        ClassRefContext::Implements => {
            // `class A implements X` / `enum A implements X` — X must be an interface.
            match kind {
                ClassLikeKind::Class => Some((
                    DiagnosticSeverity::ERROR,
                    format!(
                        "'{}' is a class, not an interface (use 'extends' to inherit from a class)",
                        fqn
                    ),
                )),
                ClassLikeKind::Trait => Some((
                    DiagnosticSeverity::ERROR,
                    format!(
                        "'{}' is a trait, not an interface (use 'use' inside the class body for traits)",
                        fqn
                    ),
                )),
                ClassLikeKind::Enum => Some((
                    DiagnosticSeverity::ERROR,
                    format!("'{}' is an enum, not an interface", fqn),
                )),
                _ => None,
            }
        }
        ClassRefContext::TraitUse => {
            // `class A { use X; }` — X must be a trait.
            match kind {
                ClassLikeKind::Class => Some((
                    DiagnosticSeverity::ERROR,
                    format!(
                        "'{}' is a class, not a trait (use 'extends' to inherit from a class)",
                        fqn
                    ),
                )),
                ClassLikeKind::Interface => Some((
                    DiagnosticSeverity::ERROR,
                    format!(
                        "'{}' is an interface, not a trait (use 'implements' for interfaces)",
                        fqn
                    ),
                )),
                ClassLikeKind::Enum => Some((
                    DiagnosticSeverity::ERROR,
                    format!("'{}' is an enum, not a trait", fqn),
                )),
                _ => None,
            }
        }
        ClassRefContext::Instanceof => {
            // `$x instanceof X` — traits always evaluate to false.
            if kind == ClassLikeKind::Trait {
                Some((
                    DiagnosticSeverity::WARNING,
                    format!(
                        "'instanceof' with trait '{}' always evaluates to false",
                        fqn
                    ),
                ))
            } else {
                None
            }
        }
        ClassRefContext::Catch => {
            // `catch (X $e)` — traits can never catch, non-Throwable is an error.
            match kind {
                ClassLikeKind::Trait => Some((
                    DiagnosticSeverity::WARNING,
                    format!("Trait '{}' in catch block will never catch anything", fqn),
                )),
                ClassLikeKind::Enum => Some((
                    DiagnosticSeverity::ERROR,
                    format!(
                        "Enum '{}' cannot be caught (only classes and interfaces that implement Throwable can be caught)",
                        fqn
                    ),
                )),
                ClassLikeKind::Class | ClassLikeKind::Interface => {
                    // Check if the class/interface implements Throwable.
                    if !is_throwable(class, class_loader) {
                        Some((
                            DiagnosticSeverity::ERROR,
                            format!(
                                "'{}' does not implement Throwable and cannot be caught",
                                fqn
                            ),
                        ))
                    } else {
                        None
                    }
                }
            }
        }
        ClassRefContext::TypeHint => {
            // Traits in type-hint positions — type check always fails.
            if kind == ClassLikeKind::Trait {
                Some((
                    DiagnosticSeverity::WARNING,
                    format!(
                        "Trait '{}' used as a type hint will always fail type checking",
                        fqn
                    ),
                ))
            } else {
                None
            }
        }
        ClassRefContext::Other | ClassRefContext::UseImport | ClassRefContext::Attribute => None,
    }
}

/// Check whether a class or interface is (or extends/implements)
/// `Throwable`.
///
/// Walks the parent class chain and interface hierarchy, using the
/// provided class loader to resolve names.  A visited set prevents
/// infinite loops from cyclic hierarchies.
fn is_throwable(class: &ClassInfo, class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>) -> bool {
    let mut visited = HashSet::new();
    is_throwable_inner(class, class_loader, &mut visited)
}

fn is_throwable_inner(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    visited: &mut HashSet<String>,
) -> bool {
    let fqn = class.fqn().to_string();
    if !visited.insert(fqn.clone()) {
        return false;
    }

    // Direct match on well-known throwable types.
    let fqn_lower = fqn.to_lowercase();
    if fqn_lower == "throwable"
        || fqn_lower == "exception"
        || fqn_lower == "error"
        || fqn_lower == "runtimeexception"
        || fqn_lower == "logicexception"
    {
        return true;
    }

    // Check interfaces.
    for iface_name in &class.interfaces {
        let iface_lower = iface_name.to_lowercase();
        let iface_short = short_name(&iface_lower);
        if iface_short == "throwable" {
            return true;
        }
        if let Some(iface) = class_loader(iface_name)
            && is_throwable_inner(&iface, class_loader, visited)
        {
            return true;
        }
    }

    // Check parent class.
    if let Some(ref parent_name) = class.parent_class {
        let parent_lower = parent_name.to_lowercase();
        let parent_short = short_name(&parent_lower);
        if parent_short == "exception"
            || parent_short == "error"
            || parent_short == "throwable"
            || parent_short == "runtimeexception"
            || parent_short == "logicexception"
        {
            return true;
        }
        if let Some(parent) = class_loader(parent_name)
            && is_throwable_inner(&parent, class_loader, visited)
        {
            return true;
        }
    }

    false
}

/// Extract the short name from a potentially namespaced class name.
fn short_name(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}
