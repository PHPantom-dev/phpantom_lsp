//! Unknown class diagnostics.
//!
//! Walk the precomputed [`SymbolMap`] for a file and flag every
//! `ClassReference` that cannot be resolved through any of PHPantom's
//! resolution phases (use-map → local classes → same-namespace →
//! fqn_uri_index → PSR-4 → stubs).
//!
//! Diagnostics use `Severity::Warning` because the code may still run
//! (e.g. the class exists but hasn't been indexed yet), but the user
//! benefits from knowing that PHPantom can't resolve it.
//!
//! The logic closely mirrors `collect_import_class_actions` in the
//! `code_actions::import_class` module — both need to determine whether
//! a class reference is unresolved.  The difference is that the code
//! action offers to *fix* it, while this diagnostic *reports* it.
//!
//! `ClassReference` spans that fall on `use` statement lines are skipped
//! because they are import declarations, not actual usages.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::{ClassRefContext, SymbolKind};

use super::existence_guards::compute_existence_guards;
use super::helpers::{
    ByteRange, FileDiagnosticContext, is_offset_in_ranges, make_diagnostic, resolve_to_fqn,
};
use super::use_statements::compute_use_line_ranges;

/// Diagnostic code used for unknown-class diagnostics so that code
/// actions can match on it.
pub(crate) const UNKNOWN_CLASS_CODE: &str = "unknown_class";

impl Backend {
    /// Collect unknown-class diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_unknown_class_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(ctx) = FileDiagnosticContext::gather(self, uri) else {
            return;
        };
        self.collect_unknown_class_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// Same as [`Self::collect_unknown_class_diagnostics`] but reuses an
    /// already-gathered [`FileDiagnosticContext`] instead of re-reading
    /// the per-file locks. Used by `collect_slow_diagnostics` so all
    /// slow collectors in the same pass share one consistent snapshot.
    pub(crate) fn collect_unknown_class_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let symbol_map = &ctx.symbol_map;
        let file_resolved_names = &ctx.file.resolved_names;
        let file_use_map = &ctx.file.use_map;
        let file_namespace = &ctx.file.namespace;
        let local_classes = &ctx.file.classes;

        // ── Collect type alias names from local classes ──────────────────
        // `@phpstan-type` / `@psalm-type` / `@phpstan-import-type` aliases
        // are not real classes — they are type-level definitions scoped to
        // the declaring class.  Collect all alias names so we can skip them.
        let type_alias_names: Vec<String> = local_classes
            .iter()
            .flat_map(|c| c.type_aliases.keys().map(|k| k.to_string()))
            .collect();

        // ── Compute byte ranges of `use` statement lines ────────────────
        // ClassReference spans that fall on these lines are import
        // declarations, not actual usages — skip them.
        let use_line_ranges = compute_use_line_ranges(content);

        // ── Compute byte ranges of `#[...]` attribute blocks ──────────
        // Attribute class names (e.g. `\JetBrains\PhpStorm\Deprecated`)
        // are a declaration concern — the PHP runtime resolves them, and
        // users don't expect "not found" warnings on attributes from
        // unindexed dependencies.
        let attribute_ranges = compute_attribute_ranges(content);

        // ── Compute existence guards ────────────────────────────────────
        let existence_guards = compute_existence_guards(content);

        // ── Walk every symbol span ──────────────────────────────────────
        for span in &symbol_map.spans {
            // Skip spans on `use` statement lines — those are the import
            // declarations themselves, not references to resolve.
            if is_offset_in_ranges(span.start, &use_line_ranges) {
                continue;
            }

            // Skip spans inside `#[...]` attribute blocks.
            if is_offset_in_ranges(span.start, &attribute_ranges) {
                continue;
            }

            let (ref_name, is_fqn, ref_context) = match &span.kind {
                SymbolKind::ClassReference {
                    name,
                    is_fqn,
                    context,
                } => (name.as_str(), *is_fqn, *context),
                _ => continue,
            };

            // `@see` legally carries URIs, prose, and naming suggestions in
            // addition to FQSENs, so a target that resolves to nothing is
            // not an error.  PHPUnit coverage metadata shares the emitter
            // but carries `CoversTarget`, so `@covers` is still checked.
            if ref_context == ClassRefContext::DocblockSee {
                continue;
            }

            // Resolve the name to a fully-qualified form, then check
            // whether PHPantom can find the class.
            //
            // Prefer the mago-names resolved name (byte-offset lookup)
            // when available — it applies PHP's full name resolution
            // rules in a single pass.  Fall back to the legacy
            // `resolve_to_fqn` helper for files without resolved names.
            let fqn = if is_fqn {
                ref_name.to_string()
            } else if let Some(rn) = file_resolved_names {
                rn.get(span.start)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| resolve_to_fqn(ref_name, file_use_map, file_namespace))
            } else {
                resolve_to_fqn(ref_name, file_use_map, file_namespace)
            };

            // ── Skip @phpstan-type / @psalm-type aliases ────────────────
            // Type aliases defined via `@phpstan-type`, `@psalm-type`, or
            // `@phpstan-import-type` are not real classes.  They appear as
            // ClassReference spans when used in `@return`, `@param`, etc.
            if !is_fqn && !ref_name.contains('\\') && type_alias_names.iter().any(|a| a == ref_name)
            {
                continue;
            }

            // ── Skip @template parameters ───────────────────────────────
            // Template type parameters (e.g. `TValue`, `TKey`) declared
            // via `@template` tags are not real classes — they are type
            // variables scoped to the class or method.  The symbol map
            // already tracks these with scope ranges, so we can check
            // whether the reference name matches an in-scope template def.
            if !is_fqn
                && !ref_name.contains('\\')
                && symbol_map.find_template_def(ref_name, span.start).is_some()
            {
                continue;
            }

            // ── Skip a constant read through a type operator ─────────────
            // `key-of<ID_TABLE>` / `ID_TABLE[K]` name the array the constant
            // holds, not a class. The type engine reads the constant's own
            // initializer there (see
            // `type_engine::call_resolution::constant_operand_shape`), so
            // insisting the name is a class would report every use of the
            // syntax PHPantom supports.
            if ref_context == ClassRefContext::TypeOperatorOperand
                && (self.lookup_indexed_global_constant(&fqn).is_some()
                    || self
                        .lookup_indexed_global_constant(crate::util::short_name(&fqn))
                        .is_some())
            {
                continue;
            }

            // ── Attempt resolution through all phases ───────────────────

            // 1. Local classes (same file)
            if local_classes
                .iter()
                .any(|c| c.name == ref_name || c.fqn() == fqn)
            {
                continue;
            }

            // 2. find_or_load_class covers: uri_classes_index → stubs →
            //    fqn_uri_index → PSR-4 → stubs (namespaced fallback)
            if self.find_or_load_class(&fqn).is_some() {
                continue;
            }

            // 3. For unqualified names without a use-map entry and without
            //    a namespace, try the raw name as a global class.
            //
            // When resolved_names is available, use `is_imported` to
            // check whether the name came from a `use` statement instead
            // of the legacy `contains_key` on the use_map.
            let is_imported = file_resolved_names
                .as_ref()
                .map(|rn| rn.is_imported(span.start))
                .unwrap_or_else(|| file_use_map.contains_key(ref_name));
            if !is_fqn
                && !ref_name.contains('\\')
                && !is_imported
                && file_namespace.is_none()
                && self.find_or_load_class(ref_name).is_some()
            {
                continue;
            }

            // 4. Check the stub index directly (global built-in classes).
            if self.stub_index.read().contains_key(fqn.as_str()) {
                continue;
            }

            // ── Skip classes guarded by class_exists() ─────────────────
            if existence_guards.is_class_guarded(&fqn, span.start)
                || existence_guards.is_class_guarded(ref_name, span.start)
            {
                continue;
            }

            // ── Name is unresolved — emit diagnostic ────────────────────
            let range = match self.offset_range_to_lsp_range(
                uri,
                content,
                span.start as usize,
                span.end as usize,
            ) {
                Some(r) => r,
                None => continue,
            };

            let message = format!("Class '{}' not found", fqn);

            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::WARNING,
                UNKNOWN_CLASS_CODE,
                message,
            ));
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Compute the byte ranges of `#[...]` attribute blocks in the source.
///
/// Returns a list of `(start, end)` byte offset pairs covering each
/// attribute list.  Handles nested brackets (e.g. `#[Attr([1,2])]`).
fn compute_attribute_ranges(content: &str) -> Vec<ByteRange> {
    let mut ranges = Vec::new();
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'#' && i + 1 < len && bytes[i + 1] == b'[' {
            let start = i;
            let mut depth: u32 = 1;
            i += 2;
            while i < len && depth > 0 {
                match bytes[i] {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    b'\'' | b'"' => {
                        // Skip string literals to avoid counting brackets inside them.
                        let quote = bytes[i];
                        i += 1;
                        while i < len && bytes[i] != quote {
                            if bytes[i] == b'\\' {
                                i += 1;
                            }
                            i += 1;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            ranges.push((start, i));
        } else {
            i += 1;
        }
    }

    ranges
}

// ─── Tests ──────────────────────────────────────────────────────────────────
//
// The bulk of this module's coverage lives in
// `tests/integration/diagnostics_unknown_classes.rs`. The one test below
// stays here because it asserts directly on `find_or_load_class` and
// `class_not_found_cache`, which are crate-private resolver internals not
// reachable from `tests/` (a separate crate that only sees the public API).

#[cfg(test)]
mod tests {
    use crate::Backend;

    /// When `find_or_load_class` runs before the classmap
    /// is populated (e.g. `did_open` during startup), the negative cache
    /// gets a stale entry.  Clearing the cache after init (as the server
    /// now does) must allow subsequent lookups to succeed.
    #[test]
    fn negative_cache_cleared_after_classmap_load() {
        let dir = tempfile::tempdir().expect("tempdir");

        let vendor_class_path = dir.path().join("vendor/filament/src/Panel.php");
        std::fs::create_dir_all(vendor_class_path.parent().unwrap()).unwrap();
        std::fs::write(
            &vendor_class_path,
            r#"<?php
namespace Filament;

class Panel {}
"#,
        )
        .unwrap();

        let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);

        // Lookup BEFORE classmap is loaded — fails and caches negative result.
        assert!(backend.find_or_load_class("Filament\\Panel").is_none());
        assert!(
            backend
                .symbols
                .class_not_found_cache
                .read()
                .contains("Filament\\Panel"),
            "negative cache should contain Filament\\Panel after failed lookup"
        );

        // Simulate init completing: load the classmap, then clear the
        // negative cache (mirrors the server.rs `initialized` handler).
        backend.symbols.fqn_uri_index.write().insert(
            "Filament\\Panel".to_string(),
            crate::util::path_to_uri(&vendor_class_path),
        );
        backend.clear_class_not_found_cache();

        // After the clear, the lookup must succeed.
        let result = backend.find_or_load_class("Filament\\Panel");
        assert!(
            result.is_some(),
            "Filament\\Panel should be found after classmap load + cache clear"
        );
    }
}
