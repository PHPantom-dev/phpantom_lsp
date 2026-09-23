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
        // Input boundary: callers may pass FQNs with a leading `\`.
        let target = strip_fqn_prefix(target_fqn);

        let candidate_keys = function_candidate_keys(target, target_short);
        self.scan_reference_candidates(
            &candidate_keys,
            "Scanning for function references",
            |file, symbol_map, locations| {
                let fqn_resolver = SpanFqnResolver::new(self, file.uri());

                // Function imports can be aliased (`use function Foo\bar as
                // baz; baz()`), so the call-site text alone is not enough to
                // decide whether this file can match.
                let function_matches = |name: &str, offset: u32| -> bool {
                    let resolved = fqn_resolver.fqn(name, false, offset);
                    // Function names are case-insensitive in PHP, so
                    // `HELPER()` is a call to `helper` and has to compare
                    // equal to it.
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
                            && let Some(location) = file.location(span.start, span.end)
                        {
                            locations.push(location);
                        }
                    }
                }
            },
        )
    }

    /// Find all references to a constant across all files.
    pub(super) fn find_constant_references(
        &self,
        target_fqn: &str,
        target_short: &str,
        include_declaration: bool,
    ) -> Vec<Location> {
        // Input boundary: callers may pass FQNs with a leading `\`.
        let target = strip_fqn_prefix(target_fqn);

        let candidate_keys = constant_candidate_keys(target, target_short);
        self.scan_reference_candidates(
            &candidate_keys,
            "Scanning for constant references",
            |file, symbol_map, locations| {
                // An import names the constant qualified and a use of it
                // names it plainly, so the span text alone cannot decide
                // whether this file matches -- the resolved name can.
                let resolved_names = self.resolved_names.read().get(file.uri()).cloned();
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

                for span in &symbol_map.spans {
                    let matched = match &span.kind {
                        SymbolKind::ConstantReference {
                            name,
                            is_definition,
                        } => {
                            (!*is_definition || include_declaration)
                                && constant_matches(name, span.start)
                        }
                        _ => false,
                    };

                    if matched && let Some(location) = file.location(span.start, span.end) {
                        locations.push(location);
                    }
                }
            },
        )
    }
}
