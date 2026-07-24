//! Go-to-definition and find-references for Laravel string-key spans.
//!
//! Laravel encodes several kinds of navigable references as plain string
//! literals: `config('app.name')`, `view('emails.welcome')`,
//! `route('users.index')`, `__('messages.saved')`.  The symbol map records
//! these as [`crate::symbol_map::LaravelStringKey`] spans; this module turns
//! a span (kind + key) into concrete definition/reference [`Location`]s.

use super::{find_all_config_references, resolve_config_key_declaration};
use super::{route_names, trans_keys, view_names};

use tower_lsp::lsp_types::Location;

/// Unified go-to-definition entry point for all Laravel string-key spans.
///
/// Dispatches on [`crate::symbol_map::LaravelStringKind`] so callers in
/// `definition/resolve.rs` only need one import and one call site.  Adding a
/// new Laravel navigation feature only requires a new match arm here, not a
/// new `pub(crate) use` in the parent module.
pub(crate) fn resolve_laravel_string_key(
    backend: &crate::Backend,
    kind: &crate::symbol_map::LaravelStringKind,
    key: &str,
) -> Vec<Location> {
    use crate::symbol_map::LaravelStringKind;
    match kind {
        LaravelStringKind::Config => resolve_config_key_declaration(backend, key)
            .into_iter()
            .collect(),
        LaravelStringKind::View => view_names::resolve_view_definitions(backend, key),
        LaravelStringKind::Route => route_names::resolve_route_definitions(backend, key),
        LaravelStringKind::Trans => trans_keys::resolve_trans_definitions(backend, key),
    }
}

/// Unified find-references entry point for all Laravel string-key spans.
///
/// Dispatches on [`crate::symbol_map::LaravelStringKind`] — see
/// [`resolve_laravel_string_key`] for the same rationale.
pub(crate) fn find_laravel_string_key_references(
    backend: &crate::Backend,
    kind: &crate::symbol_map::LaravelStringKind,
    key: &str,
    snapshot: &[(String, std::sync::Arc<crate::symbol_map::SymbolMap>)],
    include_declaration: bool,
) -> Vec<Location> {
    use crate::symbol_map::LaravelStringKind;
    let mut locations = match kind {
        LaravelStringKind::Config => {
            find_all_config_references(backend, key, snapshot, include_declaration)
        }
        LaravelStringKind::View | LaravelStringKind::Route | LaravelStringKind::Trans => {
            find_string_key_usages(kind, key, backend, snapshot)
        }
    };

    if include_declaration && kind != &LaravelStringKind::Config {
        for decl in resolve_laravel_string_key(backend, kind, key) {
            crate::references::push_unique_location(
                &mut locations,
                &decl.uri,
                decl.range.start,
                decl.range.end,
            );
        }
    }

    locations
}

/// Scan pre-built [`crate::symbol_map::SymbolMap`] spans for all call sites
/// matching `kind` + `key` — zero file re-parses, O(total spans) memory walk.
fn find_string_key_usages(
    kind: &crate::symbol_map::LaravelStringKind,
    key: &str,
    backend: &crate::Backend,
    snapshot: &[(String, std::sync::Arc<crate::symbol_map::SymbolMap>)],
) -> Vec<Location> {
    use crate::references::push_unique_location;
    use crate::symbol_map::SymbolKind;
    use crate::text_position::offset_to_position;
    use tower_lsp::lsp_types::Url;

    let mut locations = Vec::new();
    for (file_uri, symbol_map) in snapshot {
        // First pass: check if this file even has ANY LaravelStringKey matches.
        // This avoids reading file content from disk for thousands of unrelated files.
        let has_match = symbol_map.spans.iter().any(|span| {
            if let SymbolKind::LaravelStringKey {
                kind: span_kind,
                key: span_key,
            } = &span.kind
            {
                span_kind == kind && span_key == key
            } else {
                false
            }
        });

        if !has_match {
            continue;
        }

        let Ok(parsed_uri) = Url::parse(file_uri) else {
            continue;
        };
        let Some(content) = backend.get_file_content_arc(file_uri) else {
            continue;
        };
        for span in &symbol_map.spans {
            if let SymbolKind::LaravelStringKey {
                kind: span_kind,
                key: span_key,
            } = &span.kind
                && span_kind == kind
                && span_key == key
            {
                let start = offset_to_position(&content, span.start as usize);
                let end = offset_to_position(&content, span.end as usize);
                push_unique_location(&mut locations, &parsed_uri, start, end);
            }
        }
    }
    locations
}
