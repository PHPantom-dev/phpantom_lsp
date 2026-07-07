/// Standalone function name completions.
///
/// This module builds completion items for global and namespaced
/// functions (both user-defined and built-in PHP stubs).
use std::collections::HashSet;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::util::{short_name, strip_fqn_prefix};

use crate::completion::builder::{
    analyze_use_block, build_callable_label, build_callable_snippet, deprecation_tag,
};
use crate::completion::resolve::CompletionItemData;
use crate::completion::use_edit::build_use_function_edit;

/// Builder for function `CompletionItem`s with the standard layout.
///
/// This is the single code path for all function completion items so
/// that the detail / label_details style stays consistent:
///
/// - `label`: function name with parameter list (when signature is known)
/// - `detail`: return type (when known), using short class names
struct FunctionItemBuilder {
    label: String,
    insert_text: String,
    filter_text: String,
    sort_text: String,
    /// The FQN used to look up the function during `completionItem/resolve`.
    fqn: String,
    /// The file URI where the completion was triggered.
    uri: String,
    return_type: Option<String>,
    insert_text_format: Option<InsertTextFormat>,
    is_deprecated: bool,
    additional_text_edits: Option<Vec<TextEdit>>,
}

impl FunctionItemBuilder {
    fn new(
        label: String,
        insert_text: String,
        filter_text: String,
        sort_text: String,
        fqn: String,
        uri: String,
    ) -> Self {
        Self {
            label,
            insert_text,
            filter_text,
            sort_text,
            fqn,
            uri,
            return_type: None,
            insert_text_format: None,
            is_deprecated: false,
            additional_text_edits: None,
        }
    }

    fn return_type(mut self, rt: Option<String>) -> Self {
        self.return_type = rt;
        self
    }

    fn snippet(mut self) -> Self {
        self.insert_text_format = Some(InsertTextFormat::SNIPPET);
        self
    }

    fn deprecated(mut self, is_deprecated: bool) -> Self {
        self.is_deprecated = is_deprecated;
        self
    }

    fn additional_edits(mut self, edits: Option<Vec<TextEdit>>) -> Self {
        self.additional_text_edits = edits;
        self
    }

    fn build(self) -> CompletionItem {
        let detail = self.return_type;
        let data = serde_json::to_value(CompletionItemData {
            class_name: String::new(),
            member_name: self.fqn,
            kind: "function".to_string(),
            uri: self.uri,
            extra_class_names: vec![],
        })
        .ok();
        CompletionItem {
            label: self.label,
            kind: Some(CompletionItemKind::FUNCTION),
            detail,
            insert_text: Some(self.insert_text),
            insert_text_format: self.insert_text_format,
            filter_text: Some(self.filter_text),
            sort_text: Some(self.sort_text),
            tags: deprecation_tag(self.is_deprecated),
            additional_text_edits: self.additional_text_edits,
            data,
            ..CompletionItem::default()
        }
    }
}

/// Build a minimal function item for use-import context where only
/// the FQN matters and no signature information is shown.
fn build_use_import_item(
    label: String,
    fqn: &str,
    sort_prefix: &str,
    is_deprecated: bool,
    uri: &str,
) -> CompletionItem {
    let data = serde_json::to_value(CompletionItemData {
        class_name: String::new(),
        member_name: fqn.to_string(),
        kind: "function".to_string(),
        uri: uri.to_string(),
        extra_class_names: vec![],
    })
    .ok();
    CompletionItem {
        label,
        kind: Some(CompletionItemKind::FUNCTION),
        insert_text: Some(fqn.to_string()),
        filter_text: Some(fqn.to_string()),
        sort_text: Some(format!("{}_{}", sort_prefix, fqn.to_lowercase())),
        tags: deprecation_tag(is_deprecated),
        data,
        ..CompletionItem::default()
    }
}

impl Backend {
    /// Build completion items for standalone functions from all known sources.
    ///
    /// Sources (in priority order):
    ///   1. Functions discovered from parsed files (`global_functions`)
    ///   2. Functions from the autoload index (`autoload_function_index`,
    ///      non-Composer projects only — not yet parsed, name only)
    ///   3. Built-in PHP functions from embedded stubs (`stub_function_index`)
    ///
    /// For user-defined functions (source 1), the full signature is shown in
    /// the label because we already have a parsed `FunctionInfo`.  For
    /// autoload index functions (source 2) and stub functions (source 3),
    /// only the function name is shown to avoid the cost of parsing every
    /// matching file at completion time.
    ///
    /// Returns `(items, is_incomplete)`.  When the total number of
    /// matching functions exceeds [`MAX_FUNCTION_COMPLETIONS`], the result
    /// is truncated and `is_incomplete` is `true`.
    const MAX_FUNCTION_COMPLETIONS: usize = 100;

    /// Build completion items for standalone functions matching `prefix`.
    ///
    /// When `for_use_import` is `true` the items are tailored for a
    /// `use function` statement: the insert text is the FQN (so that
    /// `use function FQN;` is produced) and no parentheses are appended.
    ///
    /// When `for_use_import` is `false`, namespaced functions get an
    /// `additional_text_edits` entry that inserts `use function FQN;`
    /// at the correct position, mirroring how class auto-import works.
    /// The `content` and `file_namespace` parameters are required for
    /// this auto-import; pass `None` / empty when not needed.
    pub(crate) fn build_function_completions(
        &self,
        prefix: &str,
        for_use_import: bool,
        content: Option<&str>,
        file_namespace: &Option<String>,
        uri: &str,
    ) -> (Vec<CompletionItem>, bool) {
        let prefix_lower = strip_fqn_prefix(prefix).to_lowercase();
        let mut seen: HashSet<String> = HashSet::new();
        let mut items: Vec<CompletionItem> = Vec::new();

        // Pre-compute use-block info for auto-import insertion.
        let use_block = content.map(analyze_use_block);

        // ── 1. User-defined functions (from parsed files) ───────────
        {
            let fmap = self.global_functions.read();
            for (key, (_uri, info)) in fmap.iter() {
                // Match against both the FQN (key) and the short name so
                // that typing either finds the function.
                if !key.to_lowercase().contains(&prefix_lower)
                    && !info.name.to_lowercase().contains(&prefix_lower)
                {
                    continue;
                }
                // Deduplicate on the map key (FQN for namespaced
                // functions, bare name for global ones).  User-defined
                // functions run first, so they shadow same-named stubs.
                if !seen.insert(key.to_owned()) {
                    continue;
                }

                let is_namespaced = info.namespace.is_some();
                let fqn = key.to_owned();
                let is_deprecated = info.deprecation_message.is_some();

                let return_type = info
                    .return_type
                    .as_ref()
                    .or(info.native_return_type.as_ref())
                    .map(crate::hover::shorten_php_type);

                if for_use_import {
                    let label = if is_namespaced {
                        fqn.clone()
                    } else {
                        build_callable_label(&info.name, &info.parameters)
                    };
                    items.push(build_use_import_item(label, &fqn, "4", is_deprecated, uri));
                } else {
                    let label = build_callable_label(&info.name, &info.parameters);

                    // No import needed when the function lives in the
                    // same namespace as the current file.
                    let same_ns = file_namespace
                        .as_ref()
                        .zip(info.namespace.as_ref())
                        .is_some_and(|(file_ns, func_ns)| file_ns.eq_ignore_ascii_case(func_ns));

                    // Check for import conflict: another function with
                    // the same short name is already imported.
                    let has_conflict = is_namespaced
                        && !same_ns
                        && use_block
                            .as_ref()
                            .is_some_and(|ub| ub.function_import_conflicts(&fqn));

                    let additional_text_edits = if is_namespaced && !same_ns && !has_conflict {
                        use_block
                            .as_ref()
                            .and_then(|ub| build_use_function_edit(&fqn, ub))
                    } else {
                        None
                    };

                    // When there's a conflict, use the leading-backslash
                    // FQN so the call resolves without an import.
                    let (insert_text, filter_text) = if has_conflict {
                        let fqn_snippet =
                            format!("\\{}", build_callable_snippet(&fqn, &info.parameters));
                        (fqn_snippet, format!("\\{}", fqn))
                    } else {
                        (
                            build_callable_snippet(&info.name, &info.parameters),
                            info.name.to_string(),
                        )
                    };

                    items.push(
                        FunctionItemBuilder::new(
                            label,
                            insert_text,
                            filter_text,
                            format!("4_{}", info.name.to_lowercase()),
                            fqn.clone(),
                            uri.to_string(),
                        )
                        .return_type(return_type)
                        .snippet()
                        .deprecated(is_deprecated)
                        .additional_edits(additional_text_edits)
                        .build(),
                    );
                }
            }
        }

        // ── 2. Autoload function index (full-scan discovered functions) ──
        // The lightweight `find_symbols` byte-level scan discovers
        // function names at startup without a full AST parse, for both
        // non-Composer projects (workspace scan) and Composer projects
        // (autoload_files.php scan).  Show them in completion so the
        // user sees cross-file functions even before they're lazily
        // parsed.  Only the name is available; full signatures appear
        // after the first use triggers a lazy `update_ast` call.
        {
            let idx = self.autoload_function_index.read();
            for (fqn, _path) in idx.iter() {
                if !fqn.to_lowercase().contains(&prefix_lower) {
                    continue;
                }
                if !seen.insert(fqn.to_owned()) {
                    continue;
                }

                let is_namespaced = fqn.contains('\\');
                let sn = if is_namespaced { short_name(fqn) } else { fqn };

                if for_use_import {
                    items.push(build_use_import_item(fqn.to_owned(), fqn, "4", false, uri));
                } else {
                    let has_conflict = is_namespaced
                        && use_block
                            .as_ref()
                            .is_some_and(|ub| ub.function_import_conflicts(fqn));

                    let additional_text_edits = if is_namespaced && !has_conflict {
                        use_block
                            .as_ref()
                            .and_then(|ub| build_use_function_edit(fqn, ub))
                    } else {
                        None
                    };

                    let (insert_text, filter_text) = if has_conflict {
                        (format!("\\{fqn}()$0"), format!("\\{fqn}"))
                    } else {
                        (format!("{sn}()$0"), sn.to_string())
                    };

                    items.push(
                        FunctionItemBuilder::new(
                            sn.to_string(),
                            insert_text,
                            filter_text,
                            format!("4_{}", sn.to_lowercase()),
                            fqn.to_owned(),
                            uri.to_string(),
                        )
                        .snippet()
                        .additional_edits(additional_text_edits)
                        .build(),
                    );
                }
            }
        }

        // ── 3. Built-in PHP functions from stubs ────────────────────
        let stub_fn_idx = self.stub_function_index.read();
        for name in stub_fn_idx.keys() {
            if !name.to_lowercase().contains(&prefix_lower) {
                continue;
            }
            if !seen.insert(name.to_string()) {
                continue;
            }

            let is_namespaced = name.contains('\\');
            let sn = if is_namespaced {
                short_name(name)
            } else {
                name
            };

            if for_use_import {
                items.push(build_use_import_item(
                    name.to_string(),
                    name,
                    "5",
                    false,
                    uri,
                ));
            } else {
                let additional_text_edits = if is_namespaced {
                    use_block
                        .as_ref()
                        .and_then(|ub| build_use_function_edit(name, ub))
                } else {
                    None
                };

                items.push(
                    FunctionItemBuilder::new(
                        sn.to_string(),
                        format!("{sn}()$0"),
                        sn.to_string(),
                        format!("5_{}", sn.to_lowercase()),
                        name.to_string(),
                        uri.to_string(),
                    )
                    .snippet()
                    .additional_edits(additional_text_edits)
                    .build(),
                );
            }
        }

        let is_incomplete = items.len() > Self::MAX_FUNCTION_COMPLETIONS;
        if is_incomplete {
            items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            items.truncate(Self::MAX_FUNCTION_COMPLETIONS);
        }

        (items, is_incomplete)
    }
}
