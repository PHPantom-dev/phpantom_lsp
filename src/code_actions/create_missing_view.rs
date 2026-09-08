//! "Create missing view" quick-fix for an unresolved view name.
//!
//! `view('name')` / `View::make('name')` resolve against Blade templates on
//! disk; `invalid_laravel_view` (`diagnostics/mod.rs`) fires when a name
//! matches none of them. This offers to create the missing `.blade.php`
//! file at the location Laravel would actually look for it, under the
//! project's configured view roots (or the matching package namespace's
//! own view directory), then opens it.

use std::path::PathBuf;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::diagnostics::helpers::make_diagnostic;
use crate::diagnostics::offset_range_to_lsp_range;
use crate::symbol_map::{LaravelStringKind, SymbolKind};
use crate::text_position::ranges_overlap;

impl Backend {
    /// Offer to create the Blade template a `view('name')` (or
    /// `View::make`, `@include`, …) call names when nothing on disk
    /// resolves it.
    pub(crate) fn collect_create_missing_view_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let symbol_map = match self.symbol_maps.read().get(uri).cloned() {
            Some(sm) => sm,
            None => return,
        };
        let extra = self.typed_receiver_view_spans_for(uri, &symbol_map);

        let mut view_names: Option<std::collections::HashSet<String>> = None;

        for span in symbol_map.spans.iter().chain(extra.iter()) {
            let SymbolKind::LaravelStringKey {
                kind: LaravelStringKind::View,
                key,
                is_write: false,
                is_optional: false,
            } = &span.kind
            else {
                continue;
            };

            let Some(range) =
                offset_range_to_lsp_range(content, span.start as usize, span.end as usize)
            else {
                continue;
            };
            if !ranges_overlap(&range, &params.range) {
                continue;
            }

            let known =
                view_names.get_or_insert_with(|| self.cached_view_names().into_iter().collect());
            if known.contains(key) {
                continue;
            }

            let Some(target_dir) = self.laravel_view_create_target(key) else {
                continue;
            };
            let rel = match key.split_once("::") {
                Some((_, name)) => name,
                None => key.as_str(),
            }
            .replace('.', "/");
            let file_path = target_dir.join(format!("{rel}.blade.php"));
            if file_path.exists() {
                continue;
            }
            let Ok(new_file_uri) = Url::from_file_path(&file_path) else {
                continue;
            };

            let diagnostic = make_diagnostic(
                range,
                DiagnosticSeverity::WARNING,
                "invalid_laravel_view",
                format!("Unknown view: '{}'", key),
            );

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Create missing view '{}'", key),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diagnostic]),
                edit: Some(WorkspaceEdit {
                    changes: None,
                    document_changes: Some(DocumentChanges::Operations(vec![
                        DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                            uri: new_file_uri.clone(),
                            options: Some(CreateFileOptions {
                                overwrite: Some(false),
                                ignore_if_exists: Some(true),
                            }),
                            annotation_id: None,
                        })),
                    ])),
                    change_annotations: None,
                }),
                command: Some(self.build_code_lens_command(
                    "Open new view".to_string(),
                    new_file_uri,
                    Position::new(0, 0),
                )),
                is_preferred: Some(true),
                disabled: None,
                data: None,
            }));
        }
    }

    /// Where a missing view named `view_key` should be created.
    ///
    /// A `package::name` view belongs under the package's own registered
    /// view directory. A plain name goes under the first configured view
    /// root, falling back to Laravel's conventional `resources/views` even
    /// when it does not exist yet, so the very first view in a fresh
    /// project can still be created.
    fn laravel_view_create_target(&self, view_key: &str) -> Option<PathBuf> {
        if let Some((namespace, _)) = view_key.split_once("::") {
            return self
                .laravel_provider_resources
                .read()
                .view_dirs
                .iter()
                .find(|res| res.namespace == namespace)
                .map(|res| res.path.clone());
        }

        if let Some(root) = self.laravel_view_roots().into_iter().next() {
            return Some(root);
        }

        self.workspace
            .workspace_root
            .read()
            .clone()
            .map(|root| root.join("resources/views"))
    }
}
