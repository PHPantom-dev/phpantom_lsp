//! Class-reference case mismatch diagnostics.
//!
//! PHP resolves class *names* case-insensitively, but PSR-4 autoloading
//! maps a fully-qualified name to a file *path*, and path lookups are
//! case-sensitive on Linux (and other case-sensitive filesystems). So a
//! reference like `use App\Models\user;` or `new App\Models\user()`
//! loads fine on a case-insensitive filesystem (macOS default, Windows)
//! but fatals with "class not found" in production on Linux.
//!
//! This pass walks the precomputed [`SymbolMap`] and, for every class
//! reference in a context that actually triggers autoloading, resolves
//! it to its declaration (case-insensitively, via the shared resolution
//! pipeline) and compares the referenced spelling against the canonical
//! declared name. A case-only difference is flagged with a quick fix
//! that rewrites the reference to the canonical casing.
//!
//! Only references to PSR-4 file-backed classes in *other* files are
//! flagged. Built-in classes (stubs) are case-insensitive and always
//! available, and a reference to a class declared in the same file is
//! already loaded, so neither reaches the autoloader.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::names::OwnedResolvedNames;
use crate::symbol_map::{ClassRefContext, SymbolKind};
use crate::util::resolve_to_fqn;

use super::helpers::make_diagnostic;

/// Diagnostic code for class-reference case mismatches so that the
/// quick-fix code action can match on it.
pub(crate) const CLASS_CASE_MISMATCH_CODE: &str = "class_case_mismatch";

/// A single detected case mismatch, shared between the diagnostic
/// collector and the quick-fix code action so both agree on the range,
/// the correction, and the message.
pub(crate) struct ClassCaseMismatch {
    /// LSP range of the mis-cased reference.
    pub range: Range,
    /// The corrected spelling with canonical casing, ready to drop into
    /// the reference's span in place of the current text.
    pub corrected: String,
    /// User-facing diagnostic message.
    pub message: String,
}

impl Backend {
    /// Collect class-reference case-mismatch diagnostics for a file.
    pub fn collect_class_case_mismatch_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        for m in self.class_case_mismatches(uri, content) {
            out.push(make_diagnostic(
                m.range,
                DiagnosticSeverity::WARNING,
                CLASS_CASE_MISMATCH_CODE,
                m.message,
            ));
        }
    }

    /// Compute every class-reference case mismatch in a file.
    ///
    /// Shared by the diagnostic collector and the quick-fix code action.
    pub(crate) fn class_case_mismatches(&self, uri: &str, content: &str) -> Vec<ClassCaseMismatch> {
        let symbol_map = match self.symbol_maps.read().get(uri) {
            Some(sm) => sm.clone(),
            None => return Vec::new(),
        };

        let file_resolved_names: Option<Arc<OwnedResolvedNames>> =
            self.resolved_names.read().get(uri).cloned();
        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);

        let mut out = Vec::new();

        for span in &symbol_map.spans {
            let (ref_name, is_fqn, context) = match &span.kind {
                SymbolKind::ClassReference {
                    name,
                    is_fqn,
                    context,
                } => (name.as_str(), *is_fqn, context),
                _ => continue,
            };

            // Only contexts that actually reach the autoloader. Docblock
            // references (which never autoload) use `ClassRefContext::Other`
            // and are intentionally excluded.
            if !is_autoloading_context(context) {
                continue;
            }

            // Determine the fully-qualified name exactly as the autoloader
            // would see it.
            let referenced_fqn: String = if matches!(context, ClassRefContext::UseImport) {
                // Use-import targets are already fully qualified.
                ref_name.to_string()
            } else {
                // An *unqualified* name imported via a `use` statement
                // inherits its casing from that import (reported on the
                // import itself), so skip it here to avoid double-reporting.
                // Qualified and fully-qualified references carry literal
                // casing at the reference site, so they are always checked.
                if !is_fqn && !ref_name.contains('\\') {
                    let is_imported = file_resolved_names
                        .as_ref()
                        .map(|rn| rn.is_imported(span.start))
                        .unwrap_or_else(|| file_use_map.contains_key(ref_name));
                    if is_imported {
                        continue;
                    }
                }

                if is_fqn {
                    ref_name.to_string()
                } else if let Some(ref rn) = file_resolved_names {
                    rn.get(span.start)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| resolve_to_fqn(ref_name, &file_use_map, &file_namespace))
                } else {
                    resolve_to_fqn(ref_name, &file_use_map, &file_namespace)
                }
            };

            let referenced_fqn = referenced_fqn.trim_start_matches('\\');
            if referenced_fqn.is_empty() {
                continue;
            }

            // Resolve to the canonical declaration (case-insensitive).
            let Some(class) = self.find_or_load_class(referenced_fqn) else {
                continue;
            };
            let canonical = class.fqn();
            let canonical = canonical.as_str();

            // Already correctly cased.
            if referenced_fqn == canonical {
                continue;
            }
            // Guard: only a case-only difference is an autoloading hazard.
            if !referenced_fqn.eq_ignore_ascii_case(canonical) {
                continue;
            }

            // Only PSR-4 file-backed classes autoload by path.
            let decl_uri = match self.fqn_uri_index.read().get(canonical) {
                Some(u) => u.clone(),
                None => continue,
            };
            // Built-in classes (phpstorm-stubs) are case-insensitive and
            // always available; they are indexed under a `phpantom-stub://`
            // URI and never reach the PSR-4 autoloader.
            if decl_uri.starts_with("phpantom-stub://") {
                continue;
            }
            // A class declared in the same file is already loaded, so its
            // casing never reaches the autoloader.
            if decl_uri == uri {
                continue;
            }

            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                span.start as usize,
                span.end as usize,
            ) else {
                continue;
            };

            let written = match content.get(span.start as usize..span.end as usize) {
                Some(s) => s,
                None => continue,
            };
            let corrected = correct_casing(written, canonical);
            if corrected == written {
                continue;
            }

            let message = format!(
                "Class `{}` differs in case from its declaration `{}`. PSR-4 autoloading is \
                 case-sensitive, so this loads on a case-insensitive filesystem but fails on Linux.",
                written, canonical,
            );

            out.push(ClassCaseMismatch {
                range,
                corrected,
                message,
            });
        }

        out
    }
}

/// Whether a class reference in this context triggers PSR-4 autoloading
/// (and therefore must be correctly cased to load on Linux).
fn is_autoloading_context(context: &ClassRefContext) -> bool {
    matches!(
        context,
        ClassRefContext::UseImport
            | ClassRefContext::New
            | ClassRefContext::ExtendsClass
            | ClassRefContext::ExtendsInterface
            | ClassRefContext::Implements
            | ClassRefContext::TraitUse
            | ClassRefContext::Instanceof
            | ClassRefContext::Catch
            | ClassRefContext::TypeHint
            | ClassRefContext::Attribute
    )
}

/// Rewrite `written` so its `\`-separated segments carry the canonical
/// casing drawn from the tail of `canonical`.
///
/// `written` may be a short name (`user`), a partially-qualified name
/// (`Models\user`), or fully qualified with or without a leading `\`
/// (`App\Models\user`, `\App\Models\user`). Only the segments present in
/// `written` are corrected; a leading `\` is preserved.
fn correct_casing(written: &str, canonical: &str) -> String {
    let has_leading = written.starts_with('\\');
    let core = written.trim_start_matches('\\');
    let written_segs: Vec<&str> = core.split('\\').collect();
    let canonical_segs: Vec<&str> = canonical.split('\\').collect();

    if written_segs.len() > canonical_segs.len() {
        return written.to_string();
    }

    let tail = &canonical_segs[canonical_segs.len() - written_segs.len()..];
    let corrected_core = tail.join("\\");
    if has_leading {
        format!("\\{}", corrected_core)
    } else {
        corrected_core
    }
}

#[cfg(test)]
mod tests {
    use super::correct_casing;

    #[test]
    fn corrects_short_name() {
        assert_eq!(correct_casing("user", "App\\Models\\User"), "User");
    }

    #[test]
    fn corrects_full_name() {
        assert_eq!(
            correct_casing("app\\models\\user", "App\\Models\\User"),
            "App\\Models\\User"
        );
    }

    #[test]
    fn corrects_partial_name() {
        assert_eq!(
            correct_casing("models\\user", "App\\Models\\User"),
            "Models\\User"
        );
    }

    #[test]
    fn preserves_leading_backslash() {
        assert_eq!(
            correct_casing("\\app\\models\\user", "App\\Models\\User"),
            "\\App\\Models\\User"
        );
    }
}
