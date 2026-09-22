//! Function and global-constant reference finders.
//!
//! Both scan the cross-file symbol-map snapshot, preferring mago-names
//! resolved names for FQN resolution and falling back to the file's
//! use-map for identifiers mago-names does not track (e.g. docblock
//! references).

use super::*;

use tower_lsp::lsp_types::Location;

use crate::symbol_map::SymbolKind;

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

            // Function imports can be aliased (`use function Foo\bar as
            // baz; baz()`), so the call-site text alone is not enough to
            // decide whether this file can match.
            let function_matches = |name: &str, offset: u32| -> bool {
                let resolved =
                    if let Some(fqn) = resolved_names.as_ref().and_then(|rn| rn.get(offset)) {
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
                // Function names are case-insensitive in PHP, so `HELPER()`
                // is a call to `helper` and has to compare equal to it.
                let resolved_normalized = strip_fqn_prefix(&resolved);
                if resolved_normalized.eq_ignore_ascii_case(target) {
                    return true;
                }
                // PHP falls back from an unqualified call to the global
                // function of the same name only once the namespace-
                // qualified guess above turns out not to be declared
                // anywhere; a qualified call, or a target that itself
                // lives in a namespace, can never be reached this way,
                // so a sibling-namespace function of the same short name
                // is never treated as a match.
                !name.contains('\\')
                    && !target.contains('\\')
                    && crate::util::short_name(resolved_normalized)
                        .eq_ignore_ascii_case(target_short)
                    && !self
                        .symbols
                        .global_functions
                        .read()
                        .contains_key(resolved_normalized)
            };

            let has_potential_match = symbol_map.spans.iter().any(|span| {
                if let SymbolKind::FunctionCall { name, .. } = &span.kind {
                    function_matches(name, span.start)
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

            let file = CandidateFile::new(self, file_uri);

            for span in &symbol_map.spans {
                if let SymbolKind::FunctionCall {
                    name,
                    is_definition,
                    ..
                } = &span.kind
                {
                    if *is_definition && !include_declaration {
                        continue;
                    }

                    if function_matches(name, span.start)
                        && let Some(range) = file.range(span.start, span.end)
                    {
                        locations.push(Location {
                            uri: parsed_uri.clone(),
                            range,
                        });
                    }
                }
            }
        }

        super::sort_locations_for_references(&mut locations);

        locations
    }

    /// Find all references to a constant across all files.
    pub(super) fn find_constant_references(
        &self,
        target_fqn: &str,
        target_short: &str,
        include_declaration: bool,
    ) -> Vec<Location> {
        let mut locations = Vec::new();

        // Input boundary: callers may pass FQNs with a leading `\`.
        let target = strip_fqn_prefix(target_fqn);

        let candidate_keys = constant_candidate_keys(target, target_short);
        let snapshot = self.user_file_symbol_maps_for_reference_keys(&candidate_keys);

        for (file_uri, symbol_map) in &snapshot {
            // An import names the constant qualified and a use of it
            // names it plainly, so the span text alone cannot decide
            // whether this file matches -- the resolved name can.
            let resolved_names = self.resolved_names.read().get(file_uri).cloned();
            let constant_matches = |name: &str, offset: u32| -> bool {
                let resolved = resolved_names
                    .as_ref()
                    .and_then(|rn| rn.get(offset))
                    .map(strip_fqn_prefix)
                    .unwrap_or(strip_fqn_prefix(name));
                if resolved == target {
                    return true;
                }
                // PHP falls back from an unqualified reference to the
                // global constant of the same name only once the
                // namespace-qualified guess above turns out not to be
                // declared anywhere; a qualified reference, or a target
                // that itself lives in a namespace, can never be reached
                // this way, so a sibling-namespace constant of the same
                // short name is never treated as a match.
                !name.contains('\\')
                    && !target.contains('\\')
                    && crate::util::short_name(resolved) == target_short
                    && !self.symbols.global_defines.read().contains_key(resolved)
            };

            let has_potential_match = symbol_map.spans.iter().any(|span| match &span.kind {
                SymbolKind::ConstantReference { name, .. } => constant_matches(name, span.start),
                _ => false,
            });

            if !has_potential_match {
                continue;
            }

            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let file = CandidateFile::new(self, file_uri);

            for span in &symbol_map.spans {
                let matched = match &span.kind {
                    SymbolKind::ConstantReference {
                        name,
                        is_definition,
                    } => {
                        if *is_definition && !include_declaration {
                            false
                        } else {
                            constant_matches(name, span.start)
                        }
                    }
                    _ => false,
                };

                if matched && let Some(range) = file.range(span.start, span.end) {
                    locations.push(Location {
                        uri: parsed_uri.clone(),
                        range,
                    });
                }
            }
        }

        super::sort_locations_for_references(&mut locations);

        locations
    }
}
