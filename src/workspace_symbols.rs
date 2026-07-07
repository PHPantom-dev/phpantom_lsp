//! Workspace Symbols (`workspace/symbol`).
//!
//! Returns a flat list of symbols across the entire workspace so that
//! editors can display a "Go to Symbol in Workspace" picker (typically
//! triggered via Ctrl+T / Cmd+T).
//!
//! The handler builds the list from five data sources:
//!
//! 1. **`uri_classes_index`** — provides `ClassInfo` records for every class,
//!    interface, trait, and enum across all indexed files.  Class members
//!    (methods, properties, constants) are also emitted with
//!    `container_name` set to the owning class FQN.
//!
//! 2. **`global_functions`** — provides `FunctionInfo` records keyed by
//!    name with associated file URIs.
//!
//! 3. **`global_defines`** — provides `DefineInfo` records for
//!    `define()` / top-level `const` declarations.
//!
//! 4. **`fqn_uri_index`** — maps fully-qualified class names to file URIs
//!    for classes discovered during parsing but not necessarily open.
//!    Paired with `fqn_index` for rich metadata when available.
//!
//! 5. **`fqn_uri_index`** — maps fully-qualified class names to file URIs,
//!    covering vendor classes from Composer's classmap and other sources.

use std::collections::HashSet;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::types::{ClassLikeKind, DefineInfo, FunctionInfo};
use crate::util::offset_to_position;

/// Maximum number of symbols returned for a single workspace/symbol request.
///
/// When the query is empty (or very short) the result set can be enormous.
/// We cap it to keep the response snappy and avoid overwhelming the client.
const MAX_RESULTS: usize = 500;

/// Relevance tier for sorting workspace symbol results.
///
/// Lower numeric values sort first (higher relevance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchTier {
    /// The symbol name exactly equals the query (case-insensitive).
    Exact = 0,
    /// The symbol name starts with the query (case-insensitive).
    Prefix = 1,
    /// The symbol name contains the query as a substring.
    Substring = 2,
}

/// A workspace symbol paired with its relevance tier for sorting.
struct RankedSymbol {
    symbol: SymbolInformation,
    tier: MatchTier,
}

/// Determine the match tier of `name` against `query_lower`.
///
/// `query_lower` must already be lowercased.  Returns `None` when
/// there is no match at all.
fn match_tier(name: &str, query_lower: &str) -> Option<MatchTier> {
    if query_lower.is_empty() {
        // Empty query matches everything at the lowest tier so that
        // alphabetical ordering is the only tiebreaker.
        return Some(MatchTier::Substring);
    }
    let name_lower = name.to_lowercase();
    if name_lower == query_lower {
        Some(MatchTier::Exact)
    } else if name_lower.starts_with(query_lower) {
        Some(MatchTier::Prefix)
    } else if name_lower.contains(query_lower) {
        Some(MatchTier::Substring)
    } else {
        None
    }
}

/// Extract the short name from a symbol name for relevance ranking.
///
/// For namespaced names like `"App\\Models\\User"`, returns `"User"`.
/// For member-qualified names like `"App\\Models\\User::findByEmail"`,
/// returns `"findByEmail"`.  For unqualified names, returns the input
/// as-is.
fn short_name(full_name: &str) -> &str {
    // Check for `::` first (class member notation).
    if let Some(idx) = full_name.rfind("::") {
        return &full_name[idx + 2..];
    }
    // Then check for `\` (namespace separator).
    if let Some(idx) = full_name.rfind('\\') {
        return &full_name[idx + 1..];
    }
    full_name
}

impl Backend {
    /// Handle a `workspace/symbol` request.
    ///
    /// Searches classes, interfaces, traits, enums, their members
    /// (methods, properties, class constants), standalone functions,
    /// and global constants across all indexed files plus vendor classes
    /// from the fqn_uri_index.  The `query` string
    /// is matched as a case-insensitive substring against symbol names.
    /// An empty query returns symbols from parsed files only (not the
    /// full fqn_uri_index) to avoid flooding the picker.
    ///
    /// Results are sorted by relevance: exact matches first, then prefix
    /// matches, then substring matches. Within each tier, symbols are
    /// sorted alphabetically by name.
    #[allow(deprecated)] // SymbolInformation::deprecated is deprecated in the LSP types crate
    pub fn handle_workspace_symbol(&self, query: &str) -> Option<Vec<SymbolInformation>> {
        let query_lower = query.to_lowercase();
        let mut ranked: Vec<RankedSymbol> = Vec::new();

        // Track FQNs already emitted so that fqn_uri_index doesn't
        // produce duplicates for classes already in the uri_classes_index.
        let mut seen_fqns: HashSet<String> = HashSet::new();

        // ── Classes, interfaces, traits, enums (from uri_classes_index) ───────
        // Also emits methods, properties, and class constants.
        {
            let uri_classes = self.uri_classes_index.read();
            for (file_uri, classes) in uri_classes.iter() {
                for class in classes {
                    // Skip anonymous classes (empty name or name starting with
                    // "anonymous@" which the parser uses for anonymous classes).
                    if class.name.is_empty() || class.name.starts_with("anonymous@") {
                        continue;
                    }

                    let fqn = class.fqn().to_string();

                    let content = match self.get_file_content_arc(file_uri) {
                        Some(c) => c,
                        None => continue,
                    };

                    // ── The class itself ─────────────────────────────
                    // Match against both the FQN and the short class name.
                    let class_tier = match_tier(&fqn, &query_lower)
                        .or_else(|| match_tier(&class.name, &query_lower));

                    if let Some(tier) = class_tier
                        && class.keyword_offset != 0
                    {
                        let pos = offset_to_position(&content, class.keyword_offset as usize);
                        let kind = match class.kind {
                            ClassLikeKind::Class => SymbolKind::CLASS,
                            ClassLikeKind::Interface => SymbolKind::INTERFACE,
                            ClassLikeKind::Trait => SymbolKind::CLASS,
                            ClassLikeKind::Enum => SymbolKind::ENUM,
                        };

                        let tags = class
                            .deprecation_message
                            .as_ref()
                            .map(|_| vec![SymbolTag::DEPRECATED]);

                        seen_fqns.insert(fqn.clone());

                        ranked.push(RankedSymbol {
                            symbol: SymbolInformation {
                                name: fqn.clone(),
                                kind,
                                tags,
                                deprecated: None,
                                location: Location {
                                    uri: Url::parse(file_uri)
                                        .unwrap_or_else(|_| Url::parse("file:///unknown").unwrap()),
                                    range: Range::new(pos, pos),
                                },
                                container_name: class.file_namespace.map(|a| a.to_string()),
                            },
                            tier,
                        });
                    }

                    // ── Methods ──────────────────────────────────────
                    for method in &class.methods {
                        // Skip virtual methods — they have no real source position.
                        if method.is_virtual {
                            continue;
                        }
                        if method.name_offset == 0 {
                            continue;
                        }

                        let tier = match match_tier(&method.name, &query_lower) {
                            Some(t) => t,
                            None => continue,
                        };

                        let pos = offset_to_position(&content, method.name_offset as usize);

                        let tags = method
                            .deprecation_message
                            .as_ref()
                            .map(|_| vec![SymbolTag::DEPRECATED]);

                        ranked.push(RankedSymbol {
                            symbol: SymbolInformation {
                                name: format!("{}::{}", fqn, method.name),
                                kind: SymbolKind::METHOD,
                                tags,
                                deprecated: None,
                                location: Location {
                                    uri: Url::parse(file_uri)
                                        .unwrap_or_else(|_| Url::parse("file:///unknown").unwrap()),
                                    range: Range::new(pos, pos),
                                },
                                container_name: Some(fqn.clone()),
                            },
                            tier,
                        });
                    }

                    // ── Properties ───────────────────────────────────
                    for prop in &class.properties {
                        if prop.is_virtual {
                            continue;
                        }
                        if prop.name_offset == 0 {
                            continue;
                        }

                        // Match against the property name (without $).
                        let match_name = format!("${}", prop.name);
                        let tier = match_tier(&prop.name, &query_lower)
                            .or_else(|| match_tier(&match_name, &query_lower));
                        let tier = match tier {
                            Some(t) => t,
                            None => continue,
                        };

                        let pos = offset_to_position(&content, prop.name_offset as usize);

                        let tags = prop
                            .deprecation_message
                            .as_ref()
                            .map(|_| vec![SymbolTag::DEPRECATED]);

                        ranked.push(RankedSymbol {
                            symbol: SymbolInformation {
                                name: format!("{}::${}", fqn, prop.name),
                                kind: SymbolKind::PROPERTY,
                                tags,
                                deprecated: None,
                                location: Location {
                                    uri: Url::parse(file_uri)
                                        .unwrap_or_else(|_| Url::parse("file:///unknown").unwrap()),
                                    range: Range::new(pos, pos),
                                },
                                container_name: Some(fqn.clone()),
                            },
                            tier,
                        });
                    }

                    // ── Class constants ──────────────────────────────
                    for constant in &class.constants {
                        if constant.is_virtual {
                            continue;
                        }
                        if constant.name_offset == 0 {
                            continue;
                        }

                        let tier = match match_tier(&constant.name, &query_lower) {
                            Some(t) => t,
                            None => continue,
                        };

                        let pos = offset_to_position(&content, constant.name_offset as usize);

                        let tags = constant
                            .deprecation_message
                            .as_ref()
                            .map(|_| vec![SymbolTag::DEPRECATED]);

                        // Use ENUM_MEMBER for enum cases, CONSTANT for class constants.
                        let kind = if constant.is_enum_case {
                            SymbolKind::ENUM_MEMBER
                        } else {
                            SymbolKind::CONSTANT
                        };

                        ranked.push(RankedSymbol {
                            symbol: SymbolInformation {
                                name: format!("{}::{}", fqn, constant.name),
                                kind,
                                tags,
                                deprecated: None,
                                location: Location {
                                    uri: Url::parse(file_uri)
                                        .unwrap_or_else(|_| Url::parse("file:///unknown").unwrap()),
                                    range: Range::new(pos, pos),
                                },
                                container_name: Some(fqn.clone()),
                            },
                            tier,
                        });
                    }
                }
            }
        }

        // ── Standalone functions ────────────────────────────────────
        {
            let fmap = self.global_functions.read();
            for (_name, (file_uri, func)) in fmap.iter() {
                let display_name = function_display_name(func);

                let func_short = short_name(&display_name);
                let tier = match match_tier(&display_name, &query_lower)
                    .or_else(|| match_tier(func_short, &query_lower))
                {
                    Some(t) => t,
                    None => continue,
                };

                // Skip functions with no usable offset.
                if func.name_offset == 0 {
                    continue;
                }

                let content = match self.get_file_content_arc(file_uri) {
                    Some(c) => c,
                    None => continue,
                };

                let pos = offset_to_position(&content, func.name_offset as usize);

                let tags = func
                    .deprecation_message
                    .as_ref()
                    .map(|_| vec![SymbolTag::DEPRECATED]);

                ranked.push(RankedSymbol {
                    symbol: SymbolInformation {
                        name: display_name,
                        kind: SymbolKind::FUNCTION,
                        tags,
                        deprecated: None,
                        location: Location {
                            uri: Url::parse(file_uri)
                                .unwrap_or_else(|_| Url::parse("file:///unknown").unwrap()),
                            range: Range::new(pos, pos),
                        },
                        container_name: func.namespace.clone(),
                    },
                    tier,
                });
            }
        }

        // ── Global defines / constants ──────────────────────────────
        {
            let dmap = self.global_defines.read();
            for (name, info) in dmap.iter() {
                let tier = match match_tier(name, &query_lower) {
                    Some(t) => t,
                    None => continue,
                };

                // Skip constants with no usable offset.
                if info.name_offset == 0 {
                    continue;
                }

                let content = match self.get_file_content_arc(&info.file_uri) {
                    Some(c) => c,
                    None => continue,
                };

                let pos = offset_to_position(&content, info.name_offset as usize);

                ranked.push(RankedSymbol {
                    symbol: make_constant_symbol(name, info, pos),
                    tier,
                });
            }
        }

        // ── fqn_uri_index (discovered classes not yet in uri_classes_index) ─────
        // Only searched when the user has typed a query — an empty query
        // would dump thousands of vendor classes into the picker.
        if !query_lower.is_empty() {
            // Grab the fqn_index for rich metadata (kind, deprecation).
            let fqn_idx = self.fqn_class_index.read();
            let idx = self.fqn_uri_index.read();
            for (fqn, file_uri) in idx.iter() {
                if seen_fqns.contains(fqn) {
                    continue;
                }

                let fqn_short = short_name(fqn);
                let tier = match match_tier(fqn, &query_lower)
                    .or_else(|| match_tier(fqn_short, &query_lower))
                {
                    Some(t) => t,
                    None => continue,
                };

                let (kind, tags, container_name) = if let Some(class_info) = fqn_idx.get(fqn) {
                    let k = match class_info.kind {
                        ClassLikeKind::Class => SymbolKind::CLASS,
                        ClassLikeKind::Interface => SymbolKind::INTERFACE,
                        ClassLikeKind::Trait => SymbolKind::CLASS,
                        ClassLikeKind::Enum => SymbolKind::ENUM,
                    };
                    let t = class_info
                        .deprecation_message
                        .as_ref()
                        .map(|_| vec![SymbolTag::DEPRECATED]);
                    (k, t, class_info.file_namespace.map(|a| a.to_string()))
                } else {
                    (SymbolKind::CLASS, None, namespace_from_fqn(fqn))
                };

                // Try to compute a precise position from file content.
                let pos = if let Some(class_info) = fqn_idx.get(fqn) {
                    if class_info.keyword_offset > 0 {
                        if let Some(content) = self.get_file_content_arc(file_uri) {
                            offset_to_position(&content, class_info.keyword_offset as usize)
                        } else {
                            Position::new(0, 0)
                        }
                    } else {
                        Position::new(0, 0)
                    }
                } else {
                    Position::new(0, 0)
                };

                seen_fqns.insert(fqn.to_owned());

                ranked.push(RankedSymbol {
                    symbol: SymbolInformation {
                        name: fqn.to_owned(),
                        kind,
                        tags,
                        deprecated: None,
                        location: Location {
                            uri: Url::parse(file_uri)
                                .unwrap_or_else(|_| Url::parse("file:///unknown").unwrap()),
                            range: Range::new(pos, pos),
                        },
                        container_name,
                    },
                    tier,
                });
            }
        }

        // ── class index (Composer vendor classes) ───────────────────
        // Only searched when the user has typed a query, same rationale
        // as above.
        if !query_lower.is_empty() {
            let cmap = self.fqn_uri_index.read();
            for (fqn, file_uri) in cmap.iter() {
                if seen_fqns.contains(fqn) {
                    continue;
                }

                let fqn_short = short_name(fqn);
                let tier = match match_tier(fqn, &query_lower)
                    .or_else(|| match_tier(fqn_short, &query_lower))
                {
                    Some(t) => t,
                    None => continue,
                };

                let uri = match Url::parse(file_uri) {
                    Ok(u) => u,
                    Err(_) => continue,
                };

                seen_fqns.insert(fqn.to_owned());

                ranked.push(RankedSymbol {
                    symbol: SymbolInformation {
                        name: fqn.to_owned(),
                        kind: SymbolKind::CLASS,
                        tags: None,
                        deprecated: None,
                        location: Location {
                            uri,
                            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                        },
                        container_name: namespace_from_fqn(fqn),
                    },
                    tier,
                });
            }
        }

        // ── Sort by relevance then alphabetically ───────────────────
        ranked.sort_by(|a, b| {
            a.tier
                .cmp(&b.tier)
                .then_with(|| a.symbol.name.cmp(&b.symbol.name))
        });

        // ── Cap at MAX_RESULTS ──────────────────────────────────────
        ranked.truncate(MAX_RESULTS);

        let symbols: Vec<SymbolInformation> = ranked.into_iter().map(|r| r.symbol).collect();

        if symbols.is_empty() {
            None
        } else {
            Some(symbols)
        }
    }
}

/// Build the display name for a function, including its namespace prefix
/// when present (e.g. `"Amp\\delay"`).
fn function_display_name(func: &FunctionInfo) -> String {
    match &func.namespace {
        Some(ns) if !ns.is_empty() => format!("{}\\{}", ns, func.name),
        _ => func.name.to_string(),
    }
}

/// Extract the namespace portion from a fully-qualified class name.
///
/// Returns `Some("App\\Models")` for `"App\\Models\\User"`, or `None`
/// for a class with no namespace (e.g. `"stdClass"`).
fn namespace_from_fqn(fqn: &str) -> Option<String> {
    fqn.rfind('\\').map(|i| fqn[..i].to_string())
}

/// Build a `SymbolInformation` for a global constant.
#[allow(deprecated)] // SymbolInformation::deprecated is deprecated in the LSP types crate
fn make_constant_symbol(name: &str, info: &DefineInfo, pos: Position) -> SymbolInformation {
    SymbolInformation {
        name: name.to_string(),
        kind: SymbolKind::CONSTANT,
        tags: None,
        deprecated: None,
        location: Location {
            uri: Url::parse(&info.file_uri)
                .unwrap_or_else(|_| Url::parse("file:///unknown").unwrap()),
            range: Range::new(pos, pos),
        },
        container_name: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_name_no_separator() {
        assert_eq!(short_name("Foo"), "Foo");
    }

    #[test]
    fn short_name_with_namespace() {
        assert_eq!(short_name("App\\Models\\User"), "User");
    }

    #[test]
    fn short_name_with_member() {
        assert_eq!(short_name("App\\Models\\User::findByEmail"), "findByEmail");
    }

    #[test]
    fn short_name_member_takes_precedence() {
        assert_eq!(short_name("Ns\\Cls::method"), "method");
    }

    #[test]
    fn match_tier_exact() {
        assert_eq!(match_tier("Foo", "foo"), Some(MatchTier::Exact));
    }

    #[test]
    fn match_tier_prefix() {
        assert_eq!(match_tier("FooBar", "foo"), Some(MatchTier::Prefix));
    }

    #[test]
    fn match_tier_substring() {
        assert_eq!(match_tier("MyFooBar", "foo"), Some(MatchTier::Substring));
    }

    #[test]
    fn match_tier_no_match() {
        assert_eq!(match_tier("Bar", "foo"), None);
    }

    #[test]
    fn match_tier_empty_query() {
        assert_eq!(match_tier("Anything", ""), Some(MatchTier::Substring));
    }

    #[test]
    fn tier_ordering() {
        assert!(MatchTier::Exact < MatchTier::Prefix);
        assert!(MatchTier::Prefix < MatchTier::Substring);
    }
}
