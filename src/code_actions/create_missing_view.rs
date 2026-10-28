//! Quick-fixes for a template name nothing resolves: "Change to" the
//! closest declared names, and "Create missing view".
//!
//! `view('name')` / `View::make('name')` resolve against Blade templates on
//! disk, and `render('name.html.twig')` against a Symfony project's Twig
//! templates; `invalid_laravel_view` and `invalid_symfony_template`
//! (`diagnostics/laravel_string_keys.rs`) fire when a name matches none of
//! them. A typo is the likelier mistake, so the declared names within a
//! small edit distance are offered first. After them comes the offer to
//! create the missing file at the location the framework would actually
//! look for it, then open it: for a view, under
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
    /// Offer the closest declared names for a `view('name')` (or
    /// `View::make`, `@include`, …) call, or a `render('name.html.twig')`
    /// (or `#[Template]`, …), that names a template nothing on disk
    /// resolves, and offer to create the missing template.
    pub(crate) fn collect_create_missing_view_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        let symbol_map = match self.symbol_map_for(uri) {
            Some(sm) => sm,
            None => return,
        };
        let extra = self.typed_receiver_spans_for(uri, &symbol_map);

        // The create fix makes a file, which only a client that accepts the
        // `create` resource operation can apply.
        let can_create = self.caps().file_create;
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

            let (declared, file_path, code, label) = match kind {
                StringKeyKind::View => {
                    let views = known.views.get_or_insert_with(|| self.cached_view_names());
                    if is_declared(views, key) {
                        continue;
                    }
                    let path = can_create.then(|| self.missing_view_file(key)).flatten();
                    (&views[..], path, "invalid_laravel_view", "view")
                }
                _ => {
                    // Only a name of the project's own has a file the
                    // project could hold.
                    if !self.is_twig_project() {
                        continue;
                    }
                    let Some(path) = self.twig_template_file(key) else {
                        continue;
                    };
                    let templates = known.twig.get_or_insert_with(|| {
                        self.symfony_declared_names(StringKeyKind::TwigTemplate)
                    });
                    if is_declared(templates, key) {
                        continue;
                    }
                    (
                        &templates[..],
                        can_create.then_some(path),
                        "invalid_symfony_template",
                        "template",
                    )
                }
            };

            // The diagnostic the actions fix is reported in the template's
            // own coordinates when the call sits in one, which is also
            // where the name is replaced.
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

            let suggestions = close_names(key, declared);
            let suggestion_preferred = suggestions.first().is_some_and(|best| {
                best.0 <= 1 && suggestions.get(1).is_none_or(|next| next.0 > best.0)
            });
            if !suggestions.is_empty()
                && let Ok(doc_uri) = uri.parse::<Uri>()
            {
                for (i, (_, name)) in suggestions.iter().enumerate() {
                    out.push(CodeActionResponse::CodeAction(CodeAction {
                        title: format!("Change to '{}'", name),
                        kind: Some(CodeActionKind::QuickFix),
                        diagnostics: Some(vec![diagnostic.clone()]),
                        edit: Some(super::single_file_edit(
                            doc_uri.clone(),
                            vec![TextEdit {
                                range: diagnostic_range,
                                new_text: name.to_string(),
                            }],
                        )),
                        command: None,
                        is_preferred: (suggestion_preferred && i == 0).then_some(true),
                        disabled: None,
                        data: None,
                        tags: None,
                    }));
                }
            }

            let Some(file_path) = file_path else {
                continue;
            };
            if file_path.exists() {
                continue;
            }
            let Some(new_file_uri) = crate::util::path_to_url(&file_path) else {
                continue;
            };

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
                is_preferred: (!suggestion_preferred).then_some(true),
                disabled: None,
                data: None,
                tags: None,
            }));
        }
    }

    /// The Blade file a view named `view_key` would be.
    fn missing_view_file(&self, view_key: &str) -> Option<PathBuf> {
        let target_dir = self.laravel_view_create_target(view_key)?;
        let rel = match view_key.split_once("::") {
            Some((_, name)) => name,
            None => view_key,
        }
        .replace('.', "/");
        Some(target_dir.join(format!("{rel}.blade.php")))
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

fn is_declared(names: &[String], name: &str) -> bool {
    names
        .binary_search_by(|declared| declared.as_str().cmp(name))
        .is_ok()
}

/// The most suggestions offered for one unknown name.
const MAX_SUGGESTIONS: usize = 3;

/// The declared names close enough to `name` to be what was meant, by
/// case-insensitive edit distance, nearest first.  A short name allows
/// one edit and a longer one two, so an unrelated name of similar length
/// is not offered.
fn close_names<'a>(name: &str, declared: &'a [String]) -> Vec<(usize, &'a str)> {
    if name.is_empty() || name.len() > 255 {
        return Vec::new();
    }
    let threshold = if name.len() < 8 { 1 } else { 2 };
    let needle: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    let mut row = Vec::new();
    let mut matches: Vec<(usize, &str)> = declared
        .iter()
        .filter(|candidate| candidate.len().abs_diff(name.len()) <= threshold)
        .filter_map(|candidate| {
            let distance = edit_distance(&needle, candidate, &mut row);
            (distance <= threshold).then_some((distance, candidate.as_str()))
        })
        .collect();
    // `declared` is sorted, and the sort is stable, so equal distances
    // stay in name order.
    matches.sort_by_key(|&(distance, _)| distance);
    matches.truncate(MAX_SUGGESTIONS);
    matches
}

/// The Levenshtein distance between `needle`, already lowercased, and
/// `candidate`, compared lowercased.  `row` is scratch space reused across
/// calls.
fn edit_distance(needle: &[char], candidate: &str, row: &mut Vec<usize>) -> usize {
    row.clear();
    row.extend(0..=needle.len());
    for (j, c) in candidate.chars().flat_map(char::to_lowercase).enumerate() {
        let mut diagonal = row[0];
        row[0] = j + 1;
        for (i, &n) in needle.iter().enumerate() {
            let above = row[i + 1];
            row[i + 1] = if n == c {
                diagonal
            } else {
                1 + diagonal.min(above).min(row[i])
            };
            diagonal = above;
        }
    }
    row[needle.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        names.sort();
        names
    }

    #[test]
    fn close_names_rank_by_distance_then_name() {
        let declared = names(&[
            "emails.welcome",
            "emails.welcomes",
            "emails.goodbye",
            "blog/show.html.twig",
        ]);
        assert_eq!(
            close_names("emails.welcom", &declared),
            vec![(1, "emails.welcome"), (2, "emails.welcomes")]
        );
        assert_eq!(
            close_names("blog/shw.html.twig", &declared),
            vec![(1, "blog/show.html.twig")]
        );
    }

    #[test]
    fn a_short_name_allows_one_edit() {
        let declared = names(&["home", "help"]);
        assert_eq!(close_names("hom", &declared), vec![(1, "home")]);
        assert_eq!(close_names("hlp", &declared), vec![(1, "help")]);
        assert!(close_names("xyz", &declared).is_empty());
    }

    #[test]
    fn case_differences_are_not_edits() {
        let declared = names(&["Admin.Dashboard"]);
        assert_eq!(
            close_names("admin.dashboard", &declared),
            vec![(0, "Admin.Dashboard")]
        );
    }

    #[test]
    fn at_most_three_are_offered() {
        let declared = names(&["page1", "page2", "page3", "page4"]);
        assert_eq!(close_names("page", &declared).len(), MAX_SUGGESTIONS);
    }
}
