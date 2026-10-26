//! Building one class name completion item: the text to insert, the `use`
//! import it needs (or why it cannot have one), its sort text, and how it
//! is labelled in the popup.

use std::collections::HashMap;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::builder::{build_use_edit, use_import_conflicts};
use crate::completion::use_edit;
use crate::types::*;

use super::ranking::class_sort_text;

/// Try to shorten a FQN using the file's use-map.
///
/// Checks whether any existing `use` import provides a prefix (or exact
/// match) for the given FQN.  Returns the shortest reference that is
/// valid given the imports, or `None` if no shortening is possible.
///
/// Examples (given `use Cassandra\Exception;`):
///   - `Cassandra\Exception\AlreadyExistsException` → `Exception\AlreadyExistsException`
///   - `Cassandra\Exception` → `Exception`
pub(super) fn shorten_fqn_via_use_map(
    fqn: &str,
    use_map: &HashMap<String, String>,
) -> Option<String> {
    let mut best: Option<String> = None;
    for (alias, import_fqn) in use_map {
        let shortened = if fqn == import_fqn {
            // Exact match: the full FQN is directly imported.
            Some(alias.clone())
        } else {
            // Prefix match: a parent namespace is imported.
            fqn.strip_prefix(&format!("{}\\", import_fqn))
                .map(|suffix| format!("{}\\{}", alias, suffix))
        };
        if let Some(ref s) = shortened
            && best.as_ref().is_none_or(|b| s.len() < b.len())
        {
            best = shortened;
        }
    }
    best
}

/// Compute the insert-text base, filter-text, and optional use-import
/// FQN for a class completion item.
///
/// This function handles **editing concerns only** (what text to insert
/// and whether a `use` statement is needed).  The completion item's
/// visual presentation (label, label_details, filter_text) is set by
/// [`ClassItemCtx::build_item`], the single authority for how items
/// appear in the editor popup.
///
/// In FQN-prefix mode the insert text is the namespace-qualified
/// reference.  When the FQN belongs to the current namespace the
/// reference is simplified to a relative name (e.g. typing `\Demo\` in
/// namespace `Demo` for class `Demo\Box` inserts just `Box`).
///
/// In non-FQN mode the short name is inserted with a `use` import.
///
/// Returns `(insert_base, filter_text, use_import_fqn)`.
/// `use_import_fqn` is `None` when no `use` statement is needed (FQN
/// mode or same-namespace class).
pub(in crate::completion) fn class_edit_texts(
    short_name: &str,
    fqn: &str,
    is_fqn: bool,
    has_leading_backslash: bool,
    file_namespace: &Option<String>,
) -> (String, String, Option<String>) {
    if is_fqn {
        // When the FQN belongs to the current namespace, simplify to a
        // relative reference so that `\Demo\` + `Demo\Box` → `Box`.
        if let Some(ns) = file_namespace {
            let ns_prefix = format!("{}\\", ns);
            if let Some(relative) = fqn.strip_prefix(&ns_prefix) {
                // Filter text keeps the full typed form so the editor's
                // fuzzy matcher still finds the item.
                let filter = if has_leading_backslash {
                    format!("\\{}", fqn)
                } else {
                    fqn.to_string()
                };
                return (relative.to_string(), filter, None);
            }
        }

        let insert = if has_leading_backslash {
            format!("\\{}", fqn)
        } else {
            fqn.to_string()
        };
        (insert.clone(), insert, None)
    } else {
        // Non-FQN mode: insert the short name and import the full FQN.
        // Use the short name as filter_text so the editor's fuzzy
        // matcher scores candidates by short-name relevance, not by
        // accidental substring hits inside the namespace path.  The
        // FQN is still visible in `label` and `detail`.
        let filter = short_name.to_string();
        (short_name.to_string(), filter, Some(fqn.to_string()))
    }
}

/// Shared context for building class completion items.
///
/// Bundles the parameters that are constant across all class sources
/// within a single completion request, so that `apply_import_fixups`
/// and `build_item` don't need seven-plus arguments each.
pub(in crate::completion) struct ClassItemCtx<'a> {
    pub(in crate::completion) is_fqn_prefix: bool,
    /// Whether the typed prefix starts with `\`, which the inserted
    /// fully-qualified name keeps.
    pub(in crate::completion) has_leading_backslash: bool,
    /// Whether a fully-qualified name may be shortened through one of the
    /// file's imports; never in a `use` statement, which spells it out.
    pub(in crate::completion) shorten_via_imports: bool,
    pub(in crate::completion) is_new: bool,
    /// Whether this completion is inside an attribute list (`#[…]`).
    ///
    /// When true, `build_item` generates constructor snippets with
    /// named arguments and smart default placeholders instead of
    /// inserting the bare class name.
    pub(in crate::completion) is_attribute: bool,
    pub(in crate::completion) fqn_replace_range: Option<Range>,
    pub(in crate::completion) file_use_map: &'a HashMap<String, String>,
    pub(in crate::completion) use_block: use_edit::UseBlockInfo,
    pub(in crate::completion) file_namespace: &'a Option<String>,
    /// Namespace prefix → occurrence count table used for affinity scoring.
    pub(in crate::completion) affinity_table: HashMap<String, u32>,
    /// The short-name portion of the typed prefix, used for match quality
    /// classification (e.g. `"Order"` from `"Illuminate\\Database\\Order"`).
    pub(in crate::completion) quality_prefix: String,
    /// Whether the typed prefix contains a `\` (after stripping a leading
    /// `\`).  When false in FQN mode (e.g. `use Order` rather than
    /// `use Illuminate\Order`), `build_item` uses the short name as
    /// `filter_text` so the editor's fuzzy scorer matches against class
    /// names rather than full namespace paths.
    pub(in crate::completion) prefix_has_namespace: bool,
    /// The file URI where the completion was triggered, stored in the
    /// `data` field for `completionItem/resolve`.
    pub(in crate::completion) uri: &'a str,
}

/// Per-item editing fields produced by `class_edit_texts` and
/// post-processed by `apply_import_fixups`.
///
/// These fields control *what gets inserted* when the user accepts the
/// completion.  The label and filter_text (what the user *sees* and
/// what the editor matches against) are set by
/// [`ClassItemCtx::build_item`].
pub(in crate::completion) struct ClassItemTexts {
    pub(in crate::completion) base_name: String,
    pub(in crate::completion) filter: String,
    pub(in crate::completion) use_import: Option<String>,
}

impl ClassItemCtx<'_> {
    /// What accepting `fqn` inserts, and the `use` import it adds, for a
    /// class in source `tier` (see [`build_item`](Self::build_item)).
    pub(in crate::completion) fn item_texts(&self, fqn: &str, tier: char) -> ClassItemTexts {
        let (mut base_name, filter, mut use_import) = class_edit_texts(
            crate::util::short_name(fqn),
            fqn,
            self.is_fqn_prefix,
            self.has_leading_backslash,
            self.file_namespace,
        );
        let mut was_shortened = false;
        if self.shorten_via_imports
            && let Some(shortened) = shorten_fqn_via_use_map(fqn, self.file_use_map)
        {
            base_name = shortened;
            use_import = None;
            was_shortened = true;
        }
        // Tier 0/1 never need a use-import.
        if tier <= '1' {
            use_import = None;
        }
        self.apply_import_fixups(&mut base_name, &mut use_import, was_shortened);
        ClassItemTexts {
            base_name,
            filter,
            use_import,
        }
    }

    /// Fix up `base_name` and `use_import` after `class_edit_texts`
    /// to handle import conflicts and FQN alias collisions.
    ///
    /// - If the short name conflicts with an existing import, falls back
    ///   to a fully-qualified reference (prepends `\`).
    /// - In FQN mode, if the first namespace segment matches an existing
    ///   alias (and the name wasn't intentionally shortened through that
    ///   alias), prepends `\` so PHP resolves from the global namespace.
    pub(in crate::completion) fn apply_import_fixups(
        &self,
        base_name: &mut String,
        use_import: &mut Option<String>,
        was_shortened: bool,
    ) {
        if let Some(ref import_fqn) = *use_import
            && use_import_conflicts(import_fqn, self.file_use_map)
        {
            *base_name = format!("\\{}", import_fqn);
            *use_import = None;
        }
        if self.is_fqn_prefix
            && !was_shortened
            && !base_name.starts_with('\\')
            && let Some(first_seg) = base_name.split('\\').next()
            && self
                .file_use_map
                .keys()
                .any(|a| a.eq_ignore_ascii_case(first_seg))
        {
            *base_name = format!("\\{}", base_name);
        }
    }

    /// Build a `CompletionItem` for a class name completion.
    ///
    /// This is the **single authority** for the visual presentation of
    /// class name completions.
    ///
    /// When the typed prefix contains no namespace separator (e.g.
    /// `Order`, `Exc`), the label is the **short name** and the
    /// namespace is shown via `label_details.description`, giving a
    /// clean two-column layout:
    ///
    /// ```text
    ///   Order            App\Models
    ///   Order            Acme\Database\Model\Orders
    /// ```
    ///
    /// The `filter_text` is also the short name so the editor's fuzzy
    /// scorer matches against class names, not accidental substrings
    /// inside namespace paths.
    ///
    /// When the prefix is namespace-qualified (e.g. `Illuminate\D`,
    /// `\App\Models\U`), the label and filter_text remain the full
    /// FQN so namespace drilling works as expected.
    ///
    /// The sort_text is computed internally from `sort_tiers`
    /// and `demoted` using the affinity table and quality prefix
    /// stored in `ClassItemCtx`.
    ///
    /// `sort_tiers` is `(origin_tier, source_tier)`.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::completion) fn build_item(
        &self,
        texts: ClassItemTexts,
        fqn: &str,
        source_tier: char,
        origin_tier: char,
        demoted: bool,
        ctor_params: Option<&[ParameterInfo]>,
        is_deprecated: bool,
    ) -> CompletionItem {
        let short_name = crate::util::short_name(fqn);
        let sort_text = class_sort_text(
            short_name,
            fqn,
            &self.quality_prefix,
            source_tier,
            origin_tier,
            demoted,
            &self.affinity_table,
        );
        let (insert_text, insert_text_format) = if self.is_attribute {
            let snippet = crate::completion::builder::build_attribute_snippet(
                &texts.base_name,
                ctor_params.unwrap_or(&[]),
            );
            if snippet.contains("$0") {
                (snippet, Some(InsertTextFormat::Snippet))
            } else {
                // No constructor params — bare name, no snippet syntax.
                (snippet, None)
            }
        } else if self.is_new {
            Backend::build_new_insert(&texts.base_name, ctor_params)
        } else {
            (texts.base_name, None)
        };
        // When the typed prefix is a simple name (no `\`), use the
        // short name as filter_text so the editor's fuzzy scorer
        // ranks candidates by short-name relevance.  With the FQN as
        // filter_text, the editor finds accidental substring hits
        // inside namespace paths (e.g. "Order" matching inside
        // "Mockery\HigherOrderMessage") and may promote them above
        // genuine prefix matches.  In FQN mode with a namespace-
        // qualified prefix (e.g. `use Illuminate\D`), keep the
        // original filter_text so namespace drilling works.
        //
        // In the same non-FQN case, set the label to the short name
        // and show the namespace in `label_details.description`.
        // This gives a clean two-column layout in the editor popup:
        //
        //   Order            App\Models
        //   Order            Acme\Database\Model\Orders
        //
        // so users can distinguish same-named classes without the
        // label being a long FQN that the editor truncates.
        let (label, filter_text, label_details) = if self.prefix_has_namespace {
            (fqn.to_string(), texts.filter, None)
        } else {
            let ns = fqn.rsplit_once('\\').map(|(ns, _)| ns.to_string());
            (
                short_name.to_string(),
                short_name.to_string(),
                ns.map(|desc| CompletionItemLabelDetails {
                    detail: None,
                    description: Some(desc),
                }),
            )
        };
        let data = serde_json::to_value(crate::completion::resolve::CompletionItemData {
            class_name: String::new(),
            member_name: fqn.to_string(),
            kind: "class".to_string(),
            uri: self.uri.to_string(),
            extra_class_names: vec![],
        })
        .ok();
        CompletionItem {
            label,
            label_details,
            kind: Some(CompletionItemKind::Class),
            detail: Some(fqn.to_string()),
            insert_text: Some(insert_text.clone()),
            insert_text_format,
            filter_text: Some(filter_text),
            sort_text: Some(sort_text),
            tags: if is_deprecated {
                Some(vec![CompletionItemTag::Deprecated])
            } else {
                None
            },
            text_edit: self.fqn_replace_range.map(|range| {
                CompletionItemTextEdit::TextEdit(TextEdit {
                    range,
                    new_text: insert_text.clone(),
                })
            }),
            additional_text_edits: texts.use_import.as_ref().and_then(|import_fqn| {
                build_use_edit(import_fqn, &self.use_block, self.file_namespace)
            }),
            data,
            ..CompletionItem::default()
        }
    }
}
