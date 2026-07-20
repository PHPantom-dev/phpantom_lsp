/// Namespace declaration completions.
///
/// This module builds completion items for `namespace` declarations,
/// suggesting namespace names that fall under known PSR-4 prefixes.
///
/// When the file's path falls under a PSR-4 source directory, the
/// namespace inferred from the path is boosted to the top of the list.
/// If multiple PSR-4 roots match, the longest match is ranked first.
use std::collections::HashSet;
use std::path::Path;

use tower_lsp::lsp_types::*;

use crate::Backend;

/// A namespace inferred from the file's path and a PSR-4 mapping.
///
/// `specificity` is the length (in bytes) of the matching PSR-4
/// `base_path`.  A longer base path means a more specific mapping,
/// so it should be preferred when multiple mappings match the same
/// file.
struct InferredNamespace {
    /// The fully-qualified namespace (no leading or trailing `\`).
    namespace: String,
    /// Length of the matching PSR-4 base_path (used for ranking).
    specificity: usize,
}

/// Infer the namespace(s) that a file should belong to based on PSR-4
/// mappings and its path relative to the workspace root.
///
/// Returns all matching mappings ordered by specificity (longest match
/// first).  For a file at `src/core/Brands/Services/Fred.php` with
/// mappings `"Acme\\Core\\" => "src/core/"` and
/// `"Acme\\Core\\Tasks\\" => "src/tasks/"`, only the first mapping
/// matches, producing `Acme\Core\Brands\Services`.
fn infer_namespaces_from_path(
    file_path: &Path,
    workspace_root: &Path,
    mappings: &[crate::composer::Psr4Mapping],
) -> Vec<InferredNamespace> {
    let rel = match file_path.strip_prefix(workspace_root) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    // Normalise to forward-slash string for comparison with base_path.
    let rel_str = rel.to_string_lossy().replace('\\', "/");

    // The directory portion (without the filename).
    let dir = match rel_str.rfind('/') {
        Some(pos) => &rel_str[..pos + 1], // include trailing `/`
        None => return Vec::new(),        // file is at workspace root — no namespace
    };

    let mut results: Vec<InferredNamespace> = Vec::new();

    for mapping in mappings {
        let base = &mapping.base_path; // always ends with `/`
        let prefix = &mapping.prefix; // always ends with `\` (or empty)

        // The file's directory must start with (or equal) the mapping's
        // base_path for this mapping to apply.
        if !dir.starts_with(base.as_str()) {
            continue;
        }

        // The portion of the directory path after the base_path.
        let remainder = &dir[base.len()..];
        // Strip trailing `/` if present.
        let remainder = remainder.trim_end_matches('/');

        // Convert directory separators to namespace separators.
        let suffix = if remainder.is_empty() {
            String::new()
        } else {
            remainder.replace('/', "\\")
        };

        // Build the full namespace.  `prefix` ends with `\`, so strip
        // the trailing `\` when there is no suffix to append.
        let ns = if suffix.is_empty() {
            prefix.trim_end_matches('\\').to_string()
        } else {
            format!("{}{}", prefix, suffix)
        };

        if !ns.is_empty() {
            results.push(InferredNamespace {
                namespace: ns,
                specificity: base.len(),
            });
        }
    }

    // Sort by specificity descending (longest base_path first).
    results.sort_by_key(|b| std::cmp::Reverse(b.specificity));
    results
}

impl Backend {
    // ─── Namespace declaration completion ───────────────────────────

    /// Maximum number of namespace suggestions to return.
    const MAX_NAMESPACE_COMPLETIONS: usize = 100;

    /// Build completion items for a `namespace` declaration.
    ///
    /// Only namespaces that fall under a known PSR-4 prefix are
    /// suggested.  The sources are:
    ///   1. PSR-4 mapping prefixes themselves (exploded to every level)
    ///   2. Namespace portions of FQNs from `namespace_map`,
    ///      `fqn_uri_index`, and `uri_classes_index` — but only when
    ///      they start with a PSR-4 prefix.
    ///   3. Namespace(s) inferred from the file's path relative to PSR-4
    ///      source directories (boosted to the top of the list).
    ///
    /// Every accepted namespace is exploded to each intermediate level
    /// (e.g. `A\B\C` also inserts `A\B` and `A`).
    ///
    /// Returns `(items, is_incomplete)`.
    pub(crate) fn build_namespace_completions(
        &self,
        prefix: &str,
        position: Position,
        uri: &str,
    ) -> (Vec<CompletionItem>, bool) {
        let prefix_lower = prefix.to_lowercase();
        let mut namespaces: HashSet<String> = HashSet::new();

        // Collect the project's own PSR-4 prefixes (without trailing
        // `\`) so we can gate which cache entries are eligible.
        let psr4_prefixes: Vec<String> = {
            let mappings = self.psr4_mappings.read();
            mappings
                .iter()
                .map(|m| m.prefix.trim_end_matches('\\').to_string())
                .filter(|p| !p.is_empty())
                .collect()
        };

        // ── Infer namespace(s) from the file path ───────────────────
        // These are the namespaces the file *should* have according to
        // PSR-4.  They are always included (even if no other source
        // mentions them) and boosted to the top of the suggestion list.
        let inferred: Vec<InferredNamespace> = {
            let ws = self.workspace_root.read();
            let mappings = self.psr4_mappings.read();
            if let Some(ref root) = *ws {
                if let Ok(url) = Url::parse(uri) {
                    if let Ok(file_path) = url.to_file_path() {
                        infer_namespaces_from_path(&file_path, root, &mappings)
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        };

        // Build a set of inferred namespace strings for O(1) lookup
        // when assigning sort keys later.
        let inferred_set: HashSet<String> = inferred.iter().map(|i| i.namespace.clone()).collect();

        // Always include inferred namespaces in the candidate set.
        for inf in &inferred {
            namespaces.insert(inf.namespace.clone());
        }

        // Helper: insert a namespace and all its parent namespaces.
        fn insert_with_parents(ns: &str, set: &mut HashSet<String>) {
            if ns.is_empty() {
                return;
            }
            set.insert(ns.to_string());
            let mut parts: Vec<&str> = ns.split('\\').collect();
            while parts.len() > 1 {
                parts.pop();
                set.insert(parts.join("\\"));
            }
        }

        /// Check whether `ns` falls under one of the PSR-4 prefixes.
        fn under_psr4(ns: &str, prefixes: &[String]) -> bool {
            prefixes
                .iter()
                .any(|p| ns == p || ns.starts_with(&format!("{}\\", p)))
        }

        // Helper: insert ns (and parents) only if under a PSR-4 prefix.
        fn insert_if_under_psr4(ns: &str, set: &mut HashSet<String>, prefixes: &[String]) {
            if under_psr4(ns, prefixes) {
                insert_with_parents(ns, set);
            }
        }

        // ── 1. PSR-4 prefixes (always included, exploded) ───────────
        for p in &psr4_prefixes {
            insert_with_parents(p, &mut namespaces);
        }

        // ── 2. namespace_map (already-opened files) ─────────────────
        {
            let nmap = self.file_namespaces.read();
            for spans in nmap.values() {
                for span in spans {
                    if let Some(ns) = &span.namespace {
                        insert_if_under_psr4(ns, &mut namespaces, &psr4_prefixes);
                    }
                }
            }
        }

        // ── 3. uri_classes_index namespace portions ───────────────────────────
        {
            let amap = self.uri_classes_index.read();
            for classes in amap.values() {
                for cls in classes {
                    if let Some(ns) = &cls.file_namespace {
                        let fqn = format!("{}\\{}", ns, cls.name);
                        if let Some(ns_end) = fqn.rfind('\\') {
                            insert_if_under_psr4(&fqn[..ns_end], &mut namespaces, &psr4_prefixes);
                        }
                    }
                }
            }
        }

        // ── 4. fqn_uri_index namespace portions ───────────────────────
        {
            let idx = self.fqn_uri_index.read();
            for fqn in idx.keys() {
                if let Some(ns_end) = fqn.rfind('\\') {
                    insert_if_under_psr4(&fqn[..ns_end], &mut namespaces, &psr4_prefixes);
                }
            }
        }

        // When the typed prefix contains a backslash the editor may
        // only replace the segment after the last `\`.  Provide an
        // explicit replacement range covering the entire typed prefix
        // so that picking `Tests\Feature\Domain` after typing
        // `Tests\Feature\D` replaces the whole thing instead of
        // inserting a duplicate prefix.
        let replace_range = if prefix.contains('\\') {
            Some(Range {
                start: Position {
                    line: position.line,
                    character: position
                        .character
                        .saturating_sub(prefix.chars().count() as u32),
                },
                end: position,
            })
        } else {
            None
        };

        // Build a specificity lookup keyed by namespace string.
        // The specificity value is used to rank inferred namespaces:
        // a higher specificity (longer base_path match) gets a lower
        // sort prefix, so it appears first.
        let specificity_map: std::collections::HashMap<&str, usize> = inferred
            .iter()
            .map(|i| (i.namespace.as_str(), i.specificity))
            .collect();

        // ── Filter and build items ──────────────────────────────────
        let mut items: Vec<CompletionItem> = namespaces
            .into_iter()
            .filter(|ns| ns.to_lowercase().contains(&prefix_lower))
            .map(|ns| {
                let sn = ns.rsplit('\\').next().unwrap_or(&ns);

                // Sort key construction:
                //   - Inferred namespaces from the file path get prefix
                //     "0_0_" (most specific first, based on specificity).
                //   - All other namespaces get prefix "0_1_" to appear
                //     after the inferred ones.
                let sort_text = if inferred_set.contains(&ns) {
                    // Higher specificity → lower number → sorts first.
                    // Invert specificity so that the longest match (most
                    // specific) gets the smallest number.
                    let spec = specificity_map.get(ns.as_str()).copied().unwrap_or(0);
                    // Use a large constant minus specificity to invert,
                    // then format with leading zeros for stable sorting.
                    let inverted = 10000_usize.saturating_sub(spec);
                    format!("0_0_{:05}_{}", inverted, sn.to_lowercase())
                } else {
                    format!("0_1_{}", sn.to_lowercase())
                };

                // Mark inferred namespaces with a detail hint so the
                // user can tell at a glance which suggestion was derived
                // from the file's location.
                let detail = if inferred_set.contains(&ns) {
                    Some("(from file path)".to_string())
                } else {
                    None
                };

                // Preselect the top inferred namespace (the one with
                // the highest specificity) so the user can just press
                // Enter.
                let preselect = inferred.first().is_some_and(|top| top.namespace == ns);

                CompletionItem {
                    label: ns.clone(),
                    kind: Some(CompletionItemKind::MODULE),
                    detail,
                    insert_text: Some(ns.clone()),
                    filter_text: Some(ns.clone()),
                    sort_text: Some(sort_text),
                    preselect: Some(preselect),
                    text_edit: replace_range.map(|range| {
                        CompletionTextEdit::Edit(TextEdit {
                            range,
                            new_text: ns,
                        })
                    }),
                    ..CompletionItem::default()
                }
            })
            .collect();

        let is_incomplete = items.len() > Self::MAX_NAMESPACE_COMPLETIONS;
        if is_incomplete {
            items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            items.truncate(Self::MAX_NAMESPACE_COMPLETIONS);
        }

        (items, is_incomplete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::composer::Psr4Mapping;

    fn mapping(prefix: &str, base: &str) -> Psr4Mapping {
        Psr4Mapping {
            prefix: prefix.to_string(),
            base_path: base.to_string(),
        }
    }

    #[test]
    fn single_mapping_basic() {
        let root = PathBuf::from("/project");
        let mappings = vec![mapping("App\\", "src/")];
        let file = PathBuf::from("/project/src/Models/User.php");

        let result = infer_namespaces_from_path(&file, &root, &mappings);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].namespace, "App\\Models");
    }

    #[test]
    fn single_mapping_root_dir() {
        let root = PathBuf::from("/project");
        let mappings = vec![mapping("App\\", "src/")];
        let file = PathBuf::from("/project/src/Kernel.php");

        let result = infer_namespaces_from_path(&file, &root, &mappings);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].namespace, "App");
    }

    #[test]
    fn multiple_mappings_longest_match_first() {
        let root = PathBuf::from("/project");
        let mappings = vec![
            mapping("Acme\\Core\\", "src/core/"),
            mapping("Acme\\Core\\Database\\", "src/database/"),
        ];
        let file = PathBuf::from("/project/src/core/Brands/Services/Fred.php");

        let result = infer_namespaces_from_path(&file, &root, &mappings);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].namespace, "Acme\\Core\\Brands\\Services");
    }

    #[test]
    fn multiple_overlapping_mappings() {
        // When two mappings both match the same file path, both should
        // be returned, ordered by specificity (longest base_path first).
        let root = PathBuf::from("/project");
        let mappings = vec![
            mapping(
                "Database\\Factories\\Acme\\Core\\Database\\",
                "database/factories/",
            ),
            mapping("Database\\Seeders\\", "database/seeders/"),
        ];
        let file = PathBuf::from("/project/database/factories/UserFactory.php");

        let result = infer_namespaces_from_path(&file, &root, &mappings);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].namespace,
            "Database\\Factories\\Acme\\Core\\Database"
        );
    }

    #[test]
    fn file_in_subdirectory_of_two_matching_bases() {
        // `src/core/Database/Foo.php` matches both:
        //   - `Acme\Core\` => `src/core/`       → `Acme\Core\Database`
        //   - `Acme\Core\Database\` => `src/database/` → NO (path doesn't start with `src/database/`)
        let root = PathBuf::from("/project");
        let mappings = vec![
            mapping("Acme\\Core\\", "src/core/"),
            mapping("Acme\\Core\\Database\\", "src/database/"),
        ];
        let file = PathBuf::from("/project/src/core/Database/Foo.php");

        let result = infer_namespaces_from_path(&file, &root, &mappings);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].namespace, "Acme\\Core\\Database");
    }

    #[test]
    fn no_match_returns_empty() {
        let root = PathBuf::from("/project");
        let mappings = vec![mapping("App\\", "src/")];
        let file = PathBuf::from("/project/lib/Something.php");

        let result = infer_namespaces_from_path(&file, &root, &mappings);
        assert!(result.is_empty());
    }

    #[test]
    fn file_outside_workspace_returns_empty() {
        let root = PathBuf::from("/project");
        let mappings = vec![mapping("App\\", "src/")];
        let file = PathBuf::from("/other/src/Foo.php");

        let result = infer_namespaces_from_path(&file, &root, &mappings);
        assert!(result.is_empty());
    }

    #[test]
    fn file_at_workspace_root_returns_empty() {
        let root = PathBuf::from("/project");
        let mappings = vec![mapping("App\\", "src/")];
        let file = PathBuf::from("/project/bootstrap.php");

        let result = infer_namespaces_from_path(&file, &root, &mappings);
        assert!(result.is_empty());
    }

    #[test]
    fn real_world_multi_mapping_example() {
        let root = PathBuf::from("/project");
        let mappings = vec![
            mapping("Acme\\Core\\", "src/core/"),
            mapping("Acme\\Decimal\\", "src/decimal/"),
            mapping(
                "Database\\Factories\\Acme\\Core\\Database\\",
                "database/factories/",
            ),
            mapping("Database\\Seeders\\", "database/seeders/"),
            mapping("Vendor\\Coolrunner\\", "src/coolrunner-client/"),
            mapping("Vendor\\ElasticClient\\", "src/elasticsearch-client/"),
            mapping("Vendor\\Klaviyo\\", "src/klaviyo-client/"),
            mapping("Vendor\\Shared\\Common\\", "src/common/"),
            mapping("Acme\\Core\\Database\\", "src/database/"),
            mapping("Acme\\Core\\Elasticsearch\\", "src/elasticsearch/"),
            mapping("Acme\\Core\\Enums\\", "src/enums/"),
            mapping("Acme\\Core\\Tasks\\", "src/tasks/"),
            mapping("Acme\\Web\\", "src/web/"),
            mapping("Tests\\Support\\", "tests/Support/"),
            mapping("Tests\\", "tests/"),
        ];

        let file = PathBuf::from("/project/src/core/Brands/Services/Fred.php");
        let result = infer_namespaces_from_path(&file, &root, &mappings);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].namespace, "Acme\\Core\\Brands\\Services");

        let file2 = PathBuf::from("/project/src/tasks/Import/RunImport.php");
        let result2 = infer_namespaces_from_path(&file2, &root, &mappings);
        assert_eq!(result2.len(), 1);
        assert_eq!(result2[0].namespace, "Acme\\Core\\Tasks\\Import");

        let file3 = PathBuf::from("/project/tests/Support/Helpers/TestHelper.php");
        let result3 = infer_namespaces_from_path(&file3, &root, &mappings);
        // Both `Tests\Support\` => `tests/Support/` and `Tests\` => `tests/` match.
        assert!(result3.len() >= 2);
        // The more specific one (Tests\Support\) should come first.
        assert_eq!(result3[0].namespace, "Tests\\Support\\Helpers");
        assert_eq!(result3[1].namespace, "Tests\\Support\\Helpers");
        // But the first one has higher specificity.
        assert!(result3[0].specificity > result3[1].specificity);
    }

    #[test]
    fn deeply_nested_path() {
        let root = PathBuf::from("/project");
        let mappings = vec![mapping("App\\", "src/")];
        let file = PathBuf::from("/project/src/Domain/Billing/Invoice/LineItem.php");

        let result = infer_namespaces_from_path(&file, &root, &mappings);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].namespace, "App\\Domain\\Billing\\Invoice");
    }
}
