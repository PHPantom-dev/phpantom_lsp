//! Path-segment completion inside Laravel's path helpers.
//!
//! `base_path('|')`, `app_path('|')`, `config_path('|')`, `database_path('|')`,
//! `lang_path('|')`, `public_path('|')`, `resource_path('|')` and
//! `storage_path('|')` each anchor their argument to a conventional directory
//! under the project root, so the candidates for the segment being typed are
//! simply the entries of the directory the argument has reached so far.

use std::path::PathBuf;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::completion::source::helpers::split_trailing_ident;
use crate::text_position::{offset_to_position, position_to_offset};
use crate::virtual_members::laravel::{is_path_helper, path_helper_base};

/// How many entries of one directory are offered.
///
/// Bounds the work per keystroke in directories nobody browses by hand —
/// `storage/framework/views` holds one file per compiled template.  Segments
/// already typed narrow the listing before the cap applies, so reaching it
/// means the answer was never going to be found by scrolling.
const MAX_ENTRIES: usize = 500;

/// The directory whose entries the cursor is completing, and the text typed
/// so far.
struct PathHelperContext {
    /// Directory the segment being typed is relative to.
    directory: PathBuf,
    /// The segments already typed and terminated with `/`, kept so the
    /// replacement text can carry them back into the literal.
    typed_directories: String,
    /// The partial entry name after the last `/`.
    prefix: String,
    /// Byte offset just after the opening quote of the argument.
    content_start_offset: usize,
}

/// Detect the cursor inside the first argument of a path helper call.
///
/// The argument must be a string literal the cursor sits in, and the call a
/// plain function call: `$disk->base_path('…')` and `Foo::app_path('…')` are
/// somebody else's methods that happen to share a name.
fn detect_context(
    backend: &Backend,
    content: &str,
    cursor_offset: usize,
    code: &CodeContext<'_>,
) -> Option<PathHelperContext> {
    let (quote_offset, _) = code.open_string?;

    // The innermost open bracket, so the literal has to sit in the argument
    // list itself: `base_path(['…'])` hands the helper an array, not a path.
    let paren = code.open_brackets.last().filter(|b| b.byte == b'(')?;
    if paren.commas != 0 || paren.callee_operator.is_some() || quote_offset < paren.offset {
        return None;
    }
    let (helper, before_helper) = split_trailing_ident(content[..paren.code_before].trim_end());
    if !is_path_helper(helper) {
        return None;
    }
    // `\base_path()` is the global helper written explicitly, but
    // `Acme\base_path()` is a namespaced function of somebody else's that
    // happens to end in the same word.
    if let Some(namespace) = before_helper.strip_suffix('\\')
        && namespace
            .as_bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return None;
    }

    let root = backend.workspace.workspace_root.read().clone()?;
    let base = path_helper_base(&root, helper)?;

    // A leading separator is concatenation in PHP rather than a new root, so
    // it belongs to neither the directories walked nor the name being typed.
    let typed = content
        .get(quote_offset + 1..cursor_offset)?
        .trim_start_matches(['/', '\\']);
    let (typed_directories, prefix) = match typed.rfind(['/', '\\']) {
        Some(last) => (&typed[..=last], &typed[last + 1..]),
        None => ("", typed),
    };

    Some(PathHelperContext {
        directory: base.join(typed_directories),
        typed_directories: typed_directories.to_string(),
        prefix: prefix.to_string(),
        content_start_offset: quote_offset + 1,
    })
}

/// One candidate entry of the directory being completed.
struct Entry {
    name: String,
    is_directory: bool,
}

/// The entries of `ctx.directory` whose names start with what has been typed,
/// directories first and alphabetical within each group, and whether the
/// listing was cut short by [`MAX_ENTRIES`].
///
/// Directories lead because they are the only entries a further segment can
/// be typed under, so the ordering matches the order the path is written in.
fn matching_entries(ctx: &PathHelperContext) -> (Vec<Entry>, bool) {
    let Ok(read_dir) = std::fs::read_dir(&ctx.directory) else {
        return (Vec::new(), false);
    };

    let prefix = ctx.prefix.to_ascii_lowercase();
    let mut entries: Vec<Entry> = read_dir
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if !prefix.is_empty() && !name.to_ascii_lowercase().starts_with(&prefix) {
                return None;
            }
            Some(Entry {
                name,
                is_directory: entry.file_type().is_ok_and(|kind| kind.is_dir()),
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| a.name.cmp(&b.name))
    });
    let truncated = entries.len() > MAX_ENTRIES;
    entries.truncate(MAX_ENTRIES);
    (entries, truncated)
}

impl Backend {
    /// Try path-segment completion inside a Laravel path helper.
    pub(crate) fn try_path_helper_completion(
        &self,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let cursor_offset = position_to_offset(content, position) as usize;
        let ctx = detect_context(self, content, cursor_offset, code)?;

        // The whole literal so far is replaced, so a `/` in the middle of a
        // completion does not break the editor's word-based filtering.
        let edit_range = Range {
            start: offset_to_position(content, ctx.content_start_offset),
            end: position,
        };

        let (entries, truncated) = matching_entries(&ctx);
        let items: Vec<CompletionItem> = entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                // A directory completes to a path that is still being typed,
                // so it keeps its separator and the next segment follows on.
                let new_text = if entry.is_directory {
                    format!("{}{}/", ctx.typed_directories, entry.name)
                } else {
                    format!("{}{}", ctx.typed_directories, entry.name)
                };
                CompletionItem {
                    label: entry.name,
                    kind: Some(if entry.is_directory {
                        CompletionItemKind::Folder
                    } else {
                        CompletionItemKind::File
                    }),
                    sort_text: Some(format!("{:05}", index)),
                    filter_text: Some(new_text.clone()),
                    text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                        range: edit_range,
                        new_text,
                    })),
                    ..Default::default()
                }
            })
            .collect();

        if items.is_empty() {
            return None;
        }
        // A cut-short listing is reported as such, so the editor asks again
        // as the name narrows rather than presenting the cap as the answer.
        Some(CompletionResponse::CompletionList(CompletionList {
            is_incomplete: truncated,
            items,
            apply_kind: None,
            item_defaults: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::source::code_context::code_context_at;

    fn backend_rooted_at(root: &std::path::Path) -> Backend {
        Backend::new_test_with_workspace(root.to_path_buf(), Vec::new())
    }

    fn context_at(backend: &Backend, content: &str, marker: &str) -> Option<PathHelperContext> {
        let cursor = content.find(marker).unwrap() + marker.len();
        let code = code_context_at(content, cursor)?;
        detect_context(backend, content, cursor, &code)
    }

    #[test]
    fn detects_a_helper_argument_being_typed() {
        let dir = tempfile::tempdir().unwrap();
        let backend = backend_rooted_at(dir.path());

        let ctx = context_at(&backend, "<?php\nresource_path('views/wel", "views/wel")
            .expect("resource_path() names a directory");
        assert_eq!(ctx.directory, dir.path().join("resources").join("views"));
        assert_eq!(ctx.typed_directories, "views/");
        assert_eq!(ctx.prefix, "wel");
    }

    #[test]
    fn ignores_a_method_of_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let backend = backend_rooted_at(dir.path());

        assert!(context_at(&backend, "<?php\n$app->base_path('rou", "rou").is_none());
        assert!(context_at(&backend, "<?php\nApp::base_path('rou", "rou").is_none());
    }

    #[test]
    fn ignores_a_namespaced_function_of_the_same_short_name() {
        let dir = tempfile::tempdir().unwrap();
        let backend = backend_rooted_at(dir.path());

        assert!(
            context_at(&backend, "<?php\nAcme\\base_path('rou", "rou").is_none(),
            "a namespaced function only shares the helper's last word"
        );
        assert!(
            context_at(&backend, "<?php\n\\base_path('rou", "rou").is_some(),
            "a root-namespaced call is the helper itself"
        );
    }

    #[test]
    fn ignores_an_argument_nested_in_an_array() {
        let dir = tempfile::tempdir().unwrap();
        let backend = backend_rooted_at(dir.path());

        assert!(
            context_at(&backend, "<?php\nbase_path(['rou", "rou").is_none(),
            "an array argument names no path"
        );
    }

    #[test]
    fn ignores_a_function_that_is_not_a_path_helper() {
        let dir = tempfile::tempdir().unwrap();
        let backend = backend_rooted_at(dir.path());

        assert!(context_at(&backend, "<?php\nsprintf('rou", "rou").is_none());
        assert!(
            context_at(&backend, "<?php\nview_path('rou", "rou").is_none(),
            "a name shaped like a helper is still not one of them"
        );
    }

    #[test]
    fn a_directory_that_is_not_there_offers_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let backend = backend_rooted_at(dir.path());

        let content = "<?php\nresource_path('nowhere/wel');\n";
        let cursor = content.find("nowhere/wel").unwrap() + "nowhere/wel".len();
        let code = code_context_at(content, cursor).unwrap();
        let ctx = detect_context(&backend, content, cursor, &code)
            .expect("the call is still a path helper");
        assert_eq!(matching_entries(&ctx).0.len(), 0);

        assert!(
            backend
                .try_path_helper_completion(content, offset_to_position(content, cursor), &code)
                .is_none(),
            "an unreadable directory offers nothing rather than an empty list"
        );
    }

    #[test]
    fn ignores_later_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let backend = backend_rooted_at(dir.path());

        assert!(
            context_at(&backend, "<?php\nbase_path('a', 'b", "'a', 'b").is_none(),
            "only the first argument is a path"
        );
    }

    #[test]
    fn lists_directories_before_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("app/Models")).unwrap();
        std::fs::write(dir.path().join("app/Model.php"), "<?php").unwrap();
        let backend = backend_rooted_at(dir.path());

        let ctx = context_at(&backend, "<?php\napp_path('Mo", "Mo").unwrap();
        let (entries, truncated) = matching_entries(&ctx);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, vec!["Models", "Model.php"]);
        assert!(entries[0].is_directory);
        assert!(!truncated, "two entries are well under the cap");
    }

    #[test]
    fn offers_directory_entries_as_completions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("resources/views")).unwrap();
        std::fs::write(
            dir.path().join("resources/views/welcome.blade.php"),
            "hello",
        )
        .unwrap();
        let backend = backend_rooted_at(dir.path());

        let content = "<?php\nresource_path('views/wel');\n";
        let cursor = content.find("views/wel").unwrap() + "views/wel".len();
        let position = offset_to_position(content, cursor);
        let code = code_context_at(content, cursor).unwrap();
        let response = backend
            .try_path_helper_completion(content, position, &code)
            .expect("the views directory has a matching entry");

        let CompletionResponse::CompletionList(list) = response else {
            panic!("expected a completion list");
        };
        assert!(!list.is_incomplete);
        let items = list.items;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "welcome.blade.php");
        let Some(CompletionItemTextEdit::TextEdit(edit)) = &items[0].text_edit else {
            panic!("expected a text edit");
        };
        assert_eq!(
            edit.new_text, "views/welcome.blade.php",
            "the edit replaces the whole literal, segments included"
        );
    }
}
