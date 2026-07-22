//! Function and global-constant reference finders.
//!
//! Both scan the cross-file symbol-map snapshot, preferring mago-names
//! resolved names for FQN resolution and falling back to the file's
//! use-map for identifiers mago-names does not track (e.g. docblock
//! references).

use super::*;

use tower_lsp::lsp_types::{Location, Range};

use crate::symbol_map::SymbolKind;
use crate::util::{offset_to_position, push_unique_location};

impl Backend {
    /// Find all references to a function across all files.
    pub(super) fn find_function_references(
        &self,
        target_fqn: &str,
        target_short: &str,
        include_declaration: bool,
    ) -> Vec<Location> {
        let mut locations = Vec::new();

        // Input boundary: callers may pass FQNs with a leading `\`.
        let target = strip_fqn_prefix(target_fqn);

        let candidate_keys = function_candidate_keys(target, target_short);
        let snapshot = self.user_file_symbol_maps_for_reference_keys(&candidate_keys);
        self.begin_request_scan_window(snapshot.len(), "Scanning for function references");

        for (file_uri, symbol_map) in &snapshot {
            self.request_scan_file_done();
            // Prefer mago-names resolved_names; lazy-load use_map only
            // when an offset is not tracked (e.g. docblock references).
            let resolved_names = self.resolved_names.read().get(file_uri).cloned();
            let file_namespace = self.first_file_namespace(file_uri);
            let file_use_map = std::cell::OnceCell::new();

            // First pass: resolved-name check. Function imports can be aliased
            // (`use function Foo\bar as baz; baz()`), so the call-site text
            // alone is not enough to decide whether this file can match.
            let has_potential_match = symbol_map.spans.iter().any(|span| {
                if let SymbolKind::FunctionCall { name, .. } = &span.kind {
                    if name == target_short {
                        true
                    } else {
                        let resolved = if let Some(fqn) =
                            resolved_names.as_ref().and_then(|rn| rn.get(span.start))
                        {
                            fqn.to_string()
                        } else {
                            let use_map = file_use_map.get_or_init(|| {
                                self.file_imports
                                    .read()
                                    .get(file_uri)
                                    .cloned()
                                    .unwrap_or_default()
                            });
                            Self::resolve_to_fqn(name, use_map, &file_namespace)
                        };
                        let resolved_normalized = strip_fqn_prefix(&resolved);
                        resolved_normalized == target
                            || crate::util::short_name(resolved_normalized) == target_short
                    }
                } else {
                    false
                }
            });

            if !has_potential_match {
                continue;
            }

            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let mut file_content: Option<Arc<String>> = None;

            for span in &symbol_map.spans {
                if let SymbolKind::FunctionCall {
                    name,
                    is_definition,
                } = &span.kind
                {
                    if *is_definition && !include_declaration {
                        continue;
                    }

                    let resolved = if let Some(fqn) =
                        resolved_names.as_ref().and_then(|rn| rn.get(span.start))
                    {
                        fqn.to_string()
                    } else {
                        let use_map = file_use_map.get_or_init(|| {
                            self.file_imports
                                .read()
                                .get(file_uri)
                                .cloned()
                                .unwrap_or_default()
                        });
                        Self::resolve_to_fqn(name, use_map, &file_namespace)
                    };

                    // Input boundary: resolve_to_fqn may return a leading `\`.
                    let resolved_normalized = strip_fqn_prefix(&resolved);
                    if resolved_normalized == target
                        || crate::util::short_name(resolved_normalized) == target_short
                    {
                        if file_content.is_none() {
                            file_content = self.reference_file_content_arc(file_uri);
                        }
                        if let Some(ref content) = file_content {
                            let start = offset_to_position(content, span.start as usize);
                            let end = offset_to_position(content, span.end as usize);
                            locations.push(Location {
                                uri: parsed_uri.clone(),
                                range: Range { start, end },
                            });
                        }
                    }
                }
            }
        }

        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        locations
    }

    /// Find all references to a constant across all files.
    pub(super) fn find_constant_references(
        &self,
        target_name: &str,
        include_declaration: bool,
    ) -> Vec<Location> {
        let mut locations = Vec::new();

        let snapshot =
            self.user_file_symbol_maps_for_reference_keys(&[ReferenceIndexKey::Constant(
                target_name.to_string(),
            )]);

        for (file_uri, symbol_map) in &snapshot {
            // First pass: name-only check.
            let has_potential_match = symbol_map.spans.iter().any(|span| match &span.kind {
                SymbolKind::ConstantReference { name } => name == target_name,
                SymbolKind::MemberDeclaration { name, is_static }
                    if include_declaration && name == target_name && *is_static =>
                {
                    true
                }
                _ => false,
            });

            if !has_potential_match {
                continue;
            }

            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let mut file_content: Option<Arc<String>> = None;

            for span in &symbol_map.spans {
                let matched = match &span.kind {
                    SymbolKind::ConstantReference { name } => name == target_name,
                    SymbolKind::MemberDeclaration { name, is_static }
                        if include_declaration && name == target_name && *is_static =>
                    {
                        true
                    }
                    _ => false,
                };

                if matched {
                    if file_content.is_none() {
                        file_content = self.reference_file_content_arc(file_uri);
                    }
                    if let Some(ref content) = file_content {
                        let start = offset_to_position(content, span.start as usize);
                        let end = offset_to_position(content, span.end as usize);
                        push_unique_location(&mut locations, &parsed_uri, start, end);
                    }
                }
            }
        }

        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        locations
    }
}
