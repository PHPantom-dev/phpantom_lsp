//! "Create missing view" quick-fix for a template name nothing resolves.
//!
//! `view('name')` / `View::make('name')` resolve against Blade templates on
//! disk, and `render('name.html.twig')` against a Symfony project's Twig
//! templates; `invalid_laravel_view` and `invalid_symfony_template`
//! (`diagnostics/laravel_string_keys.rs`) fire when a name matches none of
//! them. This offers to create the missing file at the location the
//! framework would actually look for it, then opens it: for a view, under
//! the project's configured view roots (or the matching package namespace's
//! own view directory), and for a Twig template under `templates/`.

use std::path::PathBuf;
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::diagnostics::checked_byte_range_to_lsp_range;
use crate::diagnostics::helpers::make_diagnostic;
use crate::symbol_map::{StringKeyKind, SymbolKind};
use crate::text_position::ranges_overlap;

/// The template names the project holds, read for the first span that
/// needs them.
#[derive(Default)]
struct KnownTemplates {
    views: Option<Arc<[String]>>,
    twig: Option<Vec<String>>,
}

impl Backend {
    /// Offer to create the Blade template a `view('name')` (or
    /// `View::make`, `@include`, …) call names, or the Twig template a
    /// `render('name.html.twig')` (or `#[Template]`, …) names, when nothing
    /// on disk resolves it.
    pub(crate) fn collect_create_missing_view_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        // The fix creates a file, which only a client that accepts the
        // `create` resource operation can apply.
        if !self.caps().file_create {
            return;
        }

        let symbol_map = match self.symbol_map_for(uri) {
            Some(sm) => sm,
            None => return,
        };
        let extra = self.typed_receiver_spans_for(uri, &symbol_map);

        let mut known = KnownTemplates::default();

        for span in symbol_map.spans.iter().chain(extra.iter()) {
            let SymbolKind::StringKey {
                kind: kind @ (StringKeyKind::View | StringKeyKind::TwigTemplate),
                key,
                is_write: false,
                is_optional: false,
            } = &span.kind
            else {
                continue;
            };

            let Some(range) =
                checked_byte_range_to_lsp_range(content, span.start as usize, span.end as usize)
            else {
                continue;
            };
            if !ranges_overlap(&range, &params.range) {
                continue;
            }

            let (file_path, code, label) = match kind {
                StringKeyKind::View => {
                    let Some(path) = self.missing_view_file(key, &mut known) else {
                        continue;
                    };
                    (path, "invalid_laravel_view", "view")
                }
                _ => {
                    let Some(path) = self.missing_twig_template_file(key, &mut known) else {
                        continue;
                    };
                    (path, "invalid_symfony_template", "template")
                }
            };
            if file_path.exists() {
                continue;
            }
            let Some(new_file_uri) = crate::util::path_to_url(&file_path) else {
                continue;
            };

            // The diagnostic the action fixes is reported in the template's
            // own coordinates when the call sits in one.
            let Some(diagnostic_range) = self.offset_range_to_lsp_range(
                uri,
                content,
                span.start as usize,
                span.end as usize,
            ) else {
                continue;
            };
            let diagnostic = make_diagnostic(
                diagnostic_range,
                DiagnosticSeverity::Warning,
                code,
                format!("Unknown {}: '{}'", label, key),
            );

            out.push(CodeActionResponse::CodeAction(CodeAction {
                title: format!("Create missing {} '{}'", label, key),
                kind: Some(CodeActionKind::QuickFix),
                diagnostics: Some(vec![diagnostic]),
                edit: Some(super::create_file_edit(
                    new_file_uri.clone(),
                    String::new(),
                    Vec::new(),
                )),
                command: Some(self.build_code_lens_command(
                    format!("Open new {}", label),
                    new_file_uri,
                    Position::new(0, 0),
                )),
                is_preferred: Some(true),
                disabled: None,
                data: None,
                tags: None,
            }));
        }
    }

    /// The Blade file a view named `view_key` would be, when no view of
    /// that name exists.
    fn missing_view_file(&self, view_key: &str, known: &mut KnownTemplates) -> Option<PathBuf> {
        let views = known.views.get_or_insert_with(|| self.cached_view_names());
        if views
            .binary_search_by(|name| name.as_str().cmp(view_key))
            .is_ok()
        {
            return None;
        }
        let target_dir = self.laravel_view_create_target(view_key)?;
        let rel = match view_key.split_once("::") {
            Some((_, name)) => name,
            None => view_key,
        }
        .replace('.', "/");
        Some(target_dir.join(format!("{rel}.blade.php")))
    }

    /// The file a Twig template named `name` would be, when the project
    /// holds no template of that name and it is a name of the project's own.
    fn missing_twig_template_file(
        &self,
        name: &str,
        known: &mut KnownTemplates,
    ) -> Option<PathBuf> {
        if !self.is_twig_project() {
            return None;
        }
        let templates = known
            .twig
            .get_or_insert_with(|| self.symfony_declared_names(StringKeyKind::TwigTemplate));
        if templates
            .binary_search_by(|declared| declared.as_str().cmp(name))
            .is_ok()
        {
            return None;
        }
        self.twig_template_file(name)
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
                .laravel
                .provider_resources
                .read()
                .view_dirs
                .iter()
                .find(|res| res.namespace == namespace)
                .map(|res| res.path.clone());
        }

        if let Some(root) = self.laravel_view_roots().first() {
            return Some(root.path.clone());
        }

        self.workspace
            .workspace_root
            .read()
            .clone()
            .map(|root| root.join("resources/views"))
    }
}
