/// Global constant name completions.
///
/// This module builds completion items for standalone constants
/// (`define()` constants and built-in PHP constants from stubs).
use std::collections::HashSet;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::builder::deprecation_tag;
use crate::completion::context::symbol_ranking::{flat_symbol_sort_text, origin_sort_tier};
use crate::completion::resolve::CompletionItemData;
use crate::util::strip_fqn_prefix;

/// Build a single constant `CompletionItem` with the standard layout.
///
/// This is the single code path for all constant completion items so
/// that the detail / label_details style stays consistent:
///
/// - `label`: constant name
/// - `detail`: value (when known)
fn build_constant_item(
    name: String,
    value: Option<String>,
    sort_text: String,
    is_deprecated: bool,
    uri: &str,
    replace_range: Option<Range>,
) -> CompletionItem {
    let data = serde_json::to_value(CompletionItemData {
        class_name: String::new(),
        member_name: name.clone(),
        kind: "global_constant".to_string(),
        uri: uri.to_string(),
        extra_class_names: vec![],
    })
    .ok();
    // Compute text_edit before `name` is moved into `filter_text`.
    let text_edit = replace_range.map(|range| {
        CompletionTextEdit::Edit(TextEdit {
            range,
            new_text: name.clone(),
        })
    });
    CompletionItem {
        label: name.clone(),
        kind: Some(CompletionItemKind::CONSTANT),
        detail: value,
        insert_text: Some(name.clone()),
        filter_text: Some(name),
        sort_text: Some(sort_text),
        tags: deprecation_tag(is_deprecated),
        text_edit,
        data,
        ..CompletionItem::default()
    }
}

impl Backend {
    // ─── Constant name completion ───────────────────────────────────

    /// Build completion items for standalone constants (`define()` constants)
    /// from all known sources.
    ///
    /// Sources (in priority order):
    ///   1. Constants discovered from parsed files (`global_defines`)
    ///   2. Constants from the autoload index (`autoload_constant_index`,
    ///      non-Composer projects only — not yet parsed, name only)
    ///   3. Built-in PHP constants from embedded stubs (`stub_constant_index`)
    ///
    /// Each item uses the constant name as `label` and the value (when
    /// known) as `detail`.  Items are deduplicated by name.
    ///
    /// Returns `(items, is_incomplete)`.  When the total number of
    /// matching constants exceeds [`MAX_CONSTANT_COMPLETIONS`], the result
    /// is truncated and `is_incomplete` is `true`.
    const MAX_CONSTANT_COMPLETIONS: usize = 100;

    /// Build completion items for global constants matching `prefix`.
    pub(crate) fn build_constant_completions(
        &self,
        prefix: &str,
        uri: &str,
        position: Position,
    ) -> (Vec<CompletionItem>, bool) {
        let prefix_lower = strip_fqn_prefix(prefix).to_lowercase();
        let mut seen: HashSet<String> = HashSet::new();
        let mut items: Vec<CompletionItem> = Vec::new();

        // When the user is typing a namespace-qualified constant
        // reference (e.g. `PHPStan\PHP`), the editor may treat `\` as
        // a word boundary and only replace the text after the last `\`.
        // Provide an explicit replacement range covering the entire
        // typed prefix so the editor replaces it in full, avoiding
        // duplicate namespace prefixes like `PHPStan\PHPStan\PHP_VERSION_ID`.
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

        // ── 1. User-defined constants (from parsed files) ───────────
        {
            let dmap = self.global_defines.read();
            for (name, info) in dmap.iter() {
                if !name.to_lowercase().contains(&prefix_lower) {
                    continue;
                }
                if !seen.insert(name.clone()) {
                    continue;
                }
                items.push(build_constant_item(
                    name.clone(),
                    info.value.clone(),
                    flat_symbol_sort_text(
                        name,
                        &prefix_lower,
                        origin_sort_tier(self.completion_origin_for_uri(&info.file_uri)),
                        '0',
                    ),
                    false,
                    uri,
                    replace_range,
                ));
            }
        }

        // ── 2. Autoload constant index (full-scan discovered constants) ──
        // The lightweight `find_symbols` byte-level scan discovers
        // constant names at startup without a full AST parse, for both
        // non-Composer projects (workspace scan) and Composer projects
        // (autoload_files.php scan).  Show them in completion so the
        // user sees cross-file constants even before they're lazily
        // parsed via `update_ast`.
        {
            let idx = self.autoload_constant_index.read();
            let dmap = self.global_defines.read();
            for name in idx.keys() {
                if !name.to_lowercase().contains(&prefix_lower) {
                    continue;
                }
                if !seen.insert(name.clone()) {
                    continue;
                }
                // If the constant has already been lazily parsed, use
                // its value.  Otherwise leave it as None — the resolve
                // handler will fill it in when the user selects the item.
                let value = dmap.get(name.as_str()).and_then(|info| info.value.clone());
                let origin = self
                    .autoload_constant_origin_index
                    .read()
                    .get(name)
                    .copied()
                    .unwrap_or(crate::ClassCompletionOrigin::Project);
                items.push(build_constant_item(
                    name.clone(),
                    value,
                    flat_symbol_sort_text(name, &prefix_lower, origin_sort_tier(origin), '1'),
                    false,
                    uri,
                    replace_range,
                ));
            }
        }

        // ── 3. Built-in PHP constants from stubs ────────────────────
        // Only show the name here — the value is resolved lazily on
        // hover / resolve, same as stub functions.
        let stub_const_idx = self.stub_constant_index.read();
        for &name in stub_const_idx.keys() {
            if !name.to_lowercase().contains(&prefix_lower) {
                continue;
            }
            if !seen.insert(name.to_string()) {
                continue;
            }
            items.push(build_constant_item(
                name.to_string(),
                None,
                flat_symbol_sort_text(
                    name,
                    &prefix_lower,
                    origin_sort_tier(crate::ClassCompletionOrigin::CoreStub),
                    '0',
                ),
                false,
                uri,
                replace_range,
            ));
        }

        let is_incomplete = items.len() > Self::MAX_CONSTANT_COMPLETIONS;
        if is_incomplete {
            items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            items.truncate(Self::MAX_CONSTANT_COMPLETIONS);
        }

        (items, is_incomplete)
    }
}
