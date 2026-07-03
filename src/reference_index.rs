//! Cross-file reference candidate index.
//!
//! The precise find-references logic still lives in `references`: it resolves
//! aliases, class hierarchies, `self/static/parent`, and Laravel declarations.
//! This index is intentionally a coarse candidate index keyed by symbol name so
//! those scanners can skip files that cannot contain a match once the workspace
//! has been fully parsed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use parking_lot::RwLock;

use crate::Backend;
use crate::symbol_map::{LaravelStringKind, SelfStaticParentKind, SymbolKind, SymbolMap};
use crate::util::{build_fqn, find_class_at_offset, short_name, strip_fqn_prefix};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ReferenceIndexKey {
    Class(String),
    Function(String),
    Constant(String),
    Member {
        name: String,
        is_static: bool,
    },
    LaravelString {
        kind: LaravelStringKind,
        key: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReferenceIndexEntry {
    pub(crate) uri: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) is_declaration: bool,
}

pub(crate) type ReferenceIndex = Arc<RwLock<HashMap<ReferenceIndexKey, Vec<ReferenceIndexEntry>>>>;

pub(crate) fn new_reference_index() -> ReferenceIndex {
    Arc::new(RwLock::new(HashMap::new()))
}

impl Backend {
    pub(crate) fn evict_reference_index_uri(&self, uri: &str) {
        let mut index = self.reference_index.write();
        evict_reference_index_uri_locked(&mut index, uri);
    }

    pub(crate) fn reference_candidate_uris_for_keys(
        &self,
        keys: &[ReferenceIndexKey],
    ) -> Option<HashSet<String>> {
        if !self.workspace_indexed.load(Ordering::Acquire) {
            return None;
        }

        let index = self.reference_index.read();
        let mut uris = HashSet::new();
        for key in keys {
            if let Some(entries) = index.get(key) {
                uris.extend(entries.iter().map(|entry| entry.uri.clone()));
            }
        }
        Some(uris)
    }

    pub(crate) fn reindex_references_for_symbol_maps_batch(
        &self,
        items: Vec<(String, Arc<SymbolMap>)>,
    ) {
        if items.is_empty() {
            return;
        }

        let mut rebuilt = Vec::with_capacity(items.len());
        for (uri, symbol_map) in items {
            if self.is_reference_indexable_uri(&uri) {
                rebuilt.push((
                    uri.clone(),
                    self.reference_entries_for_symbol_map(&uri, &symbol_map),
                ));
            } else {
                rebuilt.push((uri, Vec::new()));
            }
        }

        let mut keep = vec![true; rebuilt.len()];
        let mut seen_uris = HashSet::new();
        for (idx, (uri, _)) in rebuilt.iter().enumerate().rev() {
            if !seen_uris.insert(uri.clone()) {
                keep[idx] = false;
            }
        }
        rebuilt = rebuilt
            .into_iter()
            .enumerate()
            .filter_map(|(idx, item)| keep[idx].then_some(item))
            .collect();

        let batch_uris: HashSet<String> = rebuilt.iter().map(|(uri, _)| uri.clone()).collect();
        let mut index = self.reference_index.write();
        evict_reference_index_uris_locked(&mut index, &batch_uris);
        for (_uri, entries) in rebuilt {
            for (key, entry) in entries {
                index.entry(key).or_default().push(entry);
            }
        }
    }

    fn reference_entries_for_symbol_map(
        &self,
        uri: &str,
        symbol_map: &SymbolMap,
    ) -> Vec<(ReferenceIndexKey, ReferenceIndexEntry)> {
        if !self.is_reference_indexable_uri(uri) {
            return Vec::new();
        }

        let mut entries: Vec<(ReferenceIndexKey, ReferenceIndexEntry)> = Vec::new();
        for span in &symbol_map.spans {
            let is_declaration = matches!(
                &span.kind,
                SymbolKind::ClassDeclaration { .. }
                    | SymbolKind::FunctionCall {
                        is_definition: true,
                        ..
                    }
                    | SymbolKind::MemberDeclaration { .. }
            );

            for key in self.reference_keys_for_span(uri, span) {
                entries.push((
                    key,
                    ReferenceIndexEntry {
                        uri: uri.to_string(),
                        start: span.start,
                        end: span.end,
                        is_declaration,
                    },
                ));
            }
        }

        if let Some(classes) = self.uri_classes_index.read().get(uri).cloned() {
            for class in classes {
                for prop in &class.properties {
                    let Some((start, end)) = member_range(prop.name_offset, &prop.name, true)
                    else {
                        continue;
                    };
                    entries.push((
                        ReferenceIndexKey::Member {
                            name: prop
                                .name
                                .strip_prefix('$')
                                .unwrap_or(&prop.name)
                                .to_string(),
                            is_static: prop.is_static,
                        },
                        ReferenceIndexEntry {
                            uri: uri.to_string(),
                            start,
                            end,
                            is_declaration: true,
                        },
                    ));
                }
            }
        }

        entries
    }

    fn reference_keys_for_span(
        &self,
        uri: &str,
        span: &crate::symbol_map::SymbolSpan,
    ) -> Vec<ReferenceIndexKey> {
        match &span.kind {
            SymbolKind::ClassReference { name, is_fqn, .. } => {
                let resolved = if *is_fqn {
                    normalize_symbol_name(name)
                } else if let Some(fqn) = self.resolved_name_at(uri, span.start) {
                    fqn
                } else {
                    let ctx = self.file_context_at(uri, span.start);
                    normalize_symbol_name(Self::resolve_to_fqn(name, &ctx.use_map, &ctx.namespace))
                };
                class_keys(&resolved, name)
            }
            SymbolKind::ClassDeclaration { name } => {
                let namespace = self.namespace_at_offset(uri, span.start);
                let fqn = build_fqn(name, namespace.as_deref());
                class_keys(&fqn, name)
            }
            SymbolKind::SelfStaticParent(kind) if *kind != SelfStaticParentKind::This => {
                let ctx = self.file_context_at(uri, span.start);
                let Some(current_class) = find_class_at_offset(&ctx.classes, span.start) else {
                    return Vec::new();
                };
                let fqn = match kind {
                    SelfStaticParentKind::Parent => {
                        current_class.parent_class.map(normalize_symbol_name)
                    }
                    _ => Some(build_fqn(&current_class.name, ctx.namespace.as_deref())),
                };
                fqn.map(|name| class_keys(&name, short_name(&name)))
                    .unwrap_or_default()
            }
            SymbolKind::FunctionCall {
                name,
                is_definition: _,
            } => {
                let resolved = if let Some(fqn) = self.resolved_name_at(uri, span.start) {
                    fqn
                } else {
                    let ctx = self.file_context_at(uri, span.start);
                    normalize_symbol_name(Self::resolve_to_fqn(name, &ctx.use_map, &ctx.namespace))
                };
                function_keys(&resolved, name)
            }
            SymbolKind::ConstantReference { name } => {
                vec![ReferenceIndexKey::Constant(name.to_string())]
            }
            SymbolKind::MemberAccess {
                member_name,
                is_static,
                ..
            }
            | SymbolKind::MemberDeclaration {
                name: member_name,
                is_static,
            } => {
                vec![ReferenceIndexKey::Member {
                    name: member_name.to_string(),
                    is_static: *is_static,
                }]
            }
            SymbolKind::LaravelStringKey { kind, key } => {
                vec![ReferenceIndexKey::LaravelString {
                    kind: kind.clone(),
                    key: key.to_string(),
                }]
            }
            _ => Vec::new(),
        }
    }

    fn is_reference_indexable_uri(&self, uri: &str) -> bool {
        if uri.starts_with("phpantom-stub://") || uri.starts_with("phpantom-stub-fn://") {
            return false;
        }
        !self
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
    }

    fn resolved_name_at(&self, uri: &str, offset: u32) -> Option<String> {
        self.resolved_names
            .read()
            .get(uri)
            .and_then(|rn| rn.get(offset).map(normalize_symbol_name))
    }
}

fn evict_reference_index_uri_locked(
    index: &mut HashMap<ReferenceIndexKey, Vec<ReferenceIndexEntry>>,
    uri: &str,
) {
    index.retain(|_, entries| {
        entries.retain(|entry| entry.uri != uri);
        !entries.is_empty()
    });
}

fn evict_reference_index_uris_locked(
    index: &mut HashMap<ReferenceIndexKey, Vec<ReferenceIndexEntry>>,
    uris: &HashSet<String>,
) {
    if uris.is_empty() {
        return;
    }

    index.retain(|_, entries| {
        entries.retain(|entry| !uris.contains(entry.uri.as_str()));
        !entries.is_empty()
    });
}

fn normalize_symbol_name(name: impl AsRef<str>) -> String {
    strip_fqn_prefix(name.as_ref()).to_string()
}

fn class_keys(resolved: &str, source_name: &str) -> Vec<ReferenceIndexKey> {
    symbol_name_keys(resolved, source_name)
        .into_iter()
        .map(ReferenceIndexKey::Class)
        .collect()
}

fn function_keys(resolved: &str, source_name: &str) -> Vec<ReferenceIndexKey> {
    symbol_name_keys(resolved, source_name)
        .into_iter()
        .map(ReferenceIndexKey::Function)
        .collect()
}

fn symbol_name_keys(resolved: &str, source_name: &str) -> Vec<String> {
    let mut keys = vec![
        normalize_symbol_name(resolved),
        normalize_symbol_name(source_name),
    ];
    keys.push(short_name(resolved).to_string());
    keys.sort();
    keys.dedup();
    keys
}

fn member_range(name_offset: u32, name: &str, has_dollar_prefix: bool) -> Option<(u32, u32)> {
    if name_offset == 0 {
        return None;
    }
    let len = name.len() as u32 + u32::from(has_dollar_prefix);
    Some((name_offset, name_offset.saturating_add(len)))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use super::*;
    use crate::Backend;
    use crate::symbol_map::{SymbolMap, SymbolSpan};

    #[test]
    fn candidate_lookup_is_disabled_until_workspace_is_indexed() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Foo.php";
        backend.update_ast(
            uri,
            "<?php\nnamespace App;\nclass Foo {}\n$foo = new Foo();\n",
        );

        let candidates = backend
            .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Class("App\\Foo".to_string())]);

        assert!(candidates.is_none());
    }

    #[test]
    fn reference_index_populates_candidates_from_parsed_file() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Foo.php";
        backend.update_ast(
            uri,
            "<?php\nnamespace App;\nclass Foo { public string $name; public function save(): void {} }\nfunction helper(): void {}\n$foo = new Foo();\n$foo->save();\nconfig('app.name');\n",
        );
        backend.workspace_indexed.store(true, Ordering::Release);

        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Class("App\\Foo".to_string()),
            uri,
        );
        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Function("App\\helper".to_string()),
            uri,
        );
        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Member {
                name: "save".to_string(),
                is_static: false,
            },
            uri,
        );
        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Member {
                name: "name".to_string(),
                is_static: false,
            },
            uri,
        );
        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::LaravelString {
                kind: LaravelStringKind::Config,
                key: "app.name".to_string(),
            },
            uri,
        );
    }

    #[test]
    fn reference_index_evicts_candidates_when_file_maps_clear() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Foo.php";
        backend.update_ast(uri, "<?php\nnamespace App;\nclass Foo {}\nnew Foo();\n");
        backend.workspace_indexed.store(true, Ordering::Release);

        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Class("App\\Foo".to_string()),
            uri,
        );

        backend.clear_file_maps(uri);

        let candidates = backend
            .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Class("App\\Foo".to_string())]);
        assert!(candidates.unwrap().is_empty());
    }

    #[test]
    fn batch_reindex_replaces_existing_uri_and_keeps_last_duplicate() {
        let backend = Backend::new_test();
        backend.workspace_indexed.store(true, Ordering::Release);
        let uri = "file:///project/src/Foo.php".to_string();

        backend.reindex_references_for_symbol_maps_batch(vec![(
            uri.clone(),
            class_declaration_symbol_map("Old"),
        )]);
        assert_candidate_contains(&backend, ReferenceIndexKey::Class("Old".to_string()), &uri);

        backend.reindex_references_for_symbol_maps_batch(vec![
            (uri.clone(), class_declaration_symbol_map("First")),
            (uri.clone(), class_declaration_symbol_map("Second")),
        ]);

        assert_candidate_not_contains(&backend, ReferenceIndexKey::Class("Old".to_string()), &uri);
        assert_candidate_not_contains(
            &backend,
            ReferenceIndexKey::Class("First".to_string()),
            &uri,
        );
        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Class("Second".to_string()),
            &uri,
        );
    }

    #[test]
    fn batch_reindex_empty_input_is_noop() {
        let backend = Backend::new_test();
        backend.reindex_references_for_symbol_maps_batch(Vec::new());
        assert!(backend.reference_index.read().is_empty());
    }

    #[test]
    fn batch_reindex_skips_vendor_and_stub_uris() {
        let dir = tempfile::tempdir().expect("temp dir");
        let vendor = dir.path().join("vendor");
        std::fs::create_dir_all(&vendor).expect("vendor dir");

        let backend = Backend::new_test();
        backend.add_vendor_dir(&vendor);
        backend.workspace_indexed.store(true, Ordering::Release);

        let user_uri = "file:///project/src/User.php".to_string();
        let vendor_uri = crate::util::path_to_uri(&vendor.join("Package.php"));
        backend.reindex_references_for_symbol_maps_batch(vec![
            (user_uri.clone(), class_declaration_symbol_map("User")),
            (vendor_uri.clone(), class_declaration_symbol_map("Package")),
            (
                "phpantom-stub://core.php".to_string(),
                class_declaration_symbol_map("StubClass"),
            ),
            (
                "phpantom-stub-fn://core.php".to_string(),
                class_declaration_symbol_map("StubFunctionClass"),
            ),
        ]);

        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Class("User".to_string()),
            &user_uri,
        );
        assert_candidate_not_contains(
            &backend,
            ReferenceIndexKey::Class("Package".to_string()),
            &vendor_uri,
        );
        assert_candidate_not_contains(
            &backend,
            ReferenceIndexKey::Class("StubClass".to_string()),
            "phpantom-stub://core.php",
        );
        assert_candidate_not_contains(
            &backend,
            ReferenceIndexKey::Class("StubFunctionClass".to_string()),
            "phpantom-stub-fn://core.php",
        );
    }

    #[test]
    fn self_static_parent_without_enclosing_class_is_not_indexed() {
        let backend = Backend::new_test();
        backend.workspace_indexed.store(true, Ordering::Release);
        let uri = "file:///project/src/Loose.php".to_string();

        backend.reindex_references_for_symbol_maps_batch(vec![(
            uri,
            Arc::new(SymbolMap {
                spans: vec![SymbolSpan {
                    start: 6,
                    end: 10,
                    kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::Self_),
                }],
                ..SymbolMap::default()
            }),
        )]);

        let candidates = backend
            .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Class("self".to_string())])
            .expect("workspace should be marked indexed");
        assert!(candidates.is_empty());
    }

    #[test]
    fn zero_offset_property_declaration_is_not_indexed() {
        let backend = Backend::new_test();
        backend.workspace_indexed.store(true, Ordering::Release);
        let uri = "file:///project/src/Foo.php".to_string();

        let mut class = crate::types::ClassInfo {
            name: crate::atom::atom("Foo"),
            ..crate::types::ClassInfo::default()
        };
        class.properties =
            crate::types::SharedVec::from_vec(vec![crate::types::PropertyInfo::virtual_property(
                "name", None,
            )]);
        backend
            .uri_classes_index
            .write()
            .insert(uri.clone(), vec![Arc::new(class)]);

        backend
            .reindex_references_for_symbol_maps_batch(vec![(uri, Arc::new(SymbolMap::default()))]);

        let candidates = backend
            .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Member {
                name: "name".to_string(),
                is_static: false,
            }])
            .expect("workspace should be marked indexed");
        assert!(candidates.is_empty());
    }

    #[test]
    fn direct_entry_build_skips_non_indexable_uri() {
        let backend = Backend::new_test();
        let entries = backend.reference_entries_for_symbol_map(
            "phpantom-stub://core.php",
            &class_declaration_symbol_map("StubClass"),
        );
        assert!(entries.is_empty());
    }

    #[test]
    fn empty_batch_evict_and_zero_member_offset_are_noops() {
        let mut index = HashMap::new();
        index.insert(
            ReferenceIndexKey::Class("Foo".to_string()),
            vec![ReferenceIndexEntry {
                uri: "file:///project/src/Foo.php".to_string(),
                start: 0,
                end: 3,
                is_declaration: true,
            }],
        );

        evict_reference_index_uris_locked(&mut index, &HashSet::new());
        assert!(index.contains_key(&ReferenceIndexKey::Class("Foo".to_string())));
        assert_eq!(member_range(0, "name", true), None);
    }

    fn assert_candidate_contains(backend: &Backend, key: ReferenceIndexKey, uri: &str) {
        let candidates = backend
            .reference_candidate_uris_for_keys(&[key])
            .expect("workspace should be marked indexed");
        assert!(
            candidates.contains(uri),
            "expected candidates to contain {uri}, got {candidates:?}"
        );
    }

    fn assert_candidate_not_contains(backend: &Backend, key: ReferenceIndexKey, uri: &str) {
        let candidates = backend
            .reference_candidate_uris_for_keys(&[key])
            .expect("workspace should be marked indexed");
        assert!(
            !candidates.contains(uri),
            "expected candidates not to contain {uri}, got {candidates:?}"
        );
    }

    fn class_declaration_symbol_map(name: &str) -> Arc<SymbolMap> {
        Arc::new(SymbolMap {
            spans: vec![SymbolSpan {
                start: 0,
                end: name.len() as u32,
                kind: SymbolKind::ClassDeclaration {
                    name: name.to_string(),
                },
            }],
            ..SymbolMap::default()
        })
    }
}
