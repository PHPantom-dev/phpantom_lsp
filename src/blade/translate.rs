//! Moving positions, ranges, and edits between a Blade template and the
//! virtual PHP the preprocessor lowers it to.
//!
//! Every feature plans its answer against the virtual PHP, because that
//! is what the symbol map and the type engine describe, but the editor
//! only knows the template. The helpers here translate through the
//! file's recorded [`super::source_map::BladeSourceMap`], and drop what
//! lands in the injected prologue, which has no template text behind it.

use super::template_edits::TemplateText;
use crate::Backend;
use crate::code_actions::{FileTextEdits, visit_file_text_edits};

impl Backend {
    /// Check whether a URI refers to a Blade template file.
    /// Returns true if the URI ends with `.blade.php` OR was opened with `languageId == "blade"`.
    pub(crate) fn is_blade_file(&self, uri: &str) -> bool {
        crate::blade::is_blade_file(uri) || self.blade.uris.read().contains(uri)
    }

    /// A Blade template's own text, or `None` for a file that is not one.
    pub(crate) fn blade_template_content(&self, uri: &str) -> Option<std::sync::Arc<String>> {
        if self.is_blade_file(uri) {
            self.get_file_content_arc(uri)
        } else {
            None
        }
    }

    /// Translate a position from an original Blade file to the virtual PHP file.
    pub(crate) fn translate_blade_to_php(
        &self,
        uri: &str,
        pos: tower_lsp_server::gen_lsp_types::Position,
    ) -> tower_lsp_server::gen_lsp_types::Position {
        if let Some(map) = self.blade_source_map(uri) {
            map.blade_to_php(pos)
        } else {
            pos
        }
    }

    /// Translate a position from a virtual PHP file back to the original Blade file.
    pub(crate) fn translate_php_to_blade(
        &self,
        uri: &str,
        pos: tower_lsp_server::gen_lsp_types::Position,
    ) -> tower_lsp_server::gen_lsp_types::Position {
        if let Some(map) = self.blade_source_map(uri) {
            map.php_to_blade(pos)
        } else {
            pos
        }
    }

    /// Translate a position from a virtual PHP file back to the original Blade
    /// file, or `None` when the position lies in the injected prologue.
    ///
    /// Use this instead of [`Self::translate_php_to_blade`] wherever the
    /// result becomes a text edit or a range the editor navigates to: the
    /// prologue has no template text behind it, and clamping a prologue
    /// position to `0:0` produces an edit at the very start of the template.
    pub(crate) fn try_translate_php_to_blade(
        &self,
        uri: &str,
        pos: tower_lsp_server::gen_lsp_types::Position,
    ) -> Option<tower_lsp_server::gen_lsp_types::Position> {
        match self.blade_source_map(uri) {
            Some(map) => map.try_php_to_blade(pos),
            None => Some(pos),
        }
    }

    /// Translate a range from a Blade template into the virtual PHP it
    /// lowers to, the coordinate system every feature plans against.
    ///
    /// The identity for a file that is not a template, or one whose
    /// source map has not been recorded.
    pub(crate) fn translate_blade_range_to_php(
        &self,
        uri: &str,
        range: tower_lsp_server::gen_lsp_types::Range,
    ) -> tower_lsp_server::gen_lsp_types::Range {
        tower_lsp_server::gen_lsp_types::Range {
            start: self.translate_blade_to_php(uri, range.start),
            end: self.translate_blade_to_php(uri, range.end),
        }
    }

    /// Translate a range from virtual PHP coordinates back to original Blade
    /// coordinates, clamping an end that lies in the prologue to the start of
    /// the template.
    ///
    /// For callers that must always answer with a range, such as the item a
    /// hierarchy or an outline is built around.  Prefer
    /// [`Self::try_translate_blade_range`] wherever dropping the range is an
    /// option: a clamped prologue position points at template text that has
    /// nothing to do with the symbol.
    pub(crate) fn translate_blade_range(
        &self,
        uri: &str,
        range: tower_lsp_server::gen_lsp_types::Range,
    ) -> tower_lsp_server::gen_lsp_types::Range {
        tower_lsp_server::gen_lsp_types::Range {
            start: self.translate_php_to_blade(uri, range.start),
            end: self.translate_php_to_blade(uri, range.end),
        }
    }

    /// Translate a range from virtual PHP coordinates back to original Blade
    /// coordinates, dropping it when either end lies in the prologue.
    pub(crate) fn try_translate_blade_range(
        &self,
        uri: &str,
        range: tower_lsp_server::gen_lsp_types::Range,
    ) -> Option<tower_lsp_server::gen_lsp_types::Range> {
        Some(tower_lsp_server::gen_lsp_types::Range {
            start: self.try_translate_php_to_blade(uri, range.start)?,
            end: self.try_translate_php_to_blade(uri, range.end)?,
        })
    }

    /// Translate one completion item's edit ranges from virtual PHP
    /// coordinates back to Blade, dropping the item if any range falls in
    /// the injected prologue.
    fn translate_completion_item(
        &self,
        uri: &str,
        mut item: tower_lsp_server::gen_lsp_types::CompletionItem,
    ) -> Option<tower_lsp_server::gen_lsp_types::CompletionItem> {
        use tower_lsp_server::gen_lsp_types::CompletionItemTextEdit;

        if let Some(edit) = item.text_edit.take() {
            item.text_edit = Some(match edit {
                CompletionItemTextEdit::TextEdit(mut e) => {
                    e.range = self.try_translate_blade_range(uri, e.range)?;
                    CompletionItemTextEdit::TextEdit(e)
                }
                CompletionItemTextEdit::InsertReplaceEdit(mut e) => {
                    e.insert = self.try_translate_blade_range(uri, e.insert)?;
                    e.replace = self.try_translate_blade_range(uri, e.replace)?;
                    CompletionItemTextEdit::InsertReplaceEdit(e)
                }
            });
        }

        if let Some(edits) = item.additional_text_edits.take() {
            let mut translated = Vec::with_capacity(edits.len());
            for mut e in edits {
                e.range = self.try_translate_blade_range(uri, e.range)?;
                translated.push(e);
            }
            item.additional_text_edits = Some(translated);
        }

        Some(item)
    }

    /// Translate every completion item in a response from virtual PHP
    /// coordinates back to Blade, dropping items whose edits target the
    /// preprocessor's injected prologue rather than clamping them to the
    /// start of the template.
    pub(crate) fn translate_completion_response(
        &self,
        uri: &str,
        response: tower_lsp_server::gen_lsp_types::CompletionResponse,
    ) -> tower_lsp_server::gen_lsp_types::CompletionResponse {
        use tower_lsp_server::gen_lsp_types::{CompletionList, CompletionResponse};

        match response {
            CompletionResponse::CompletionItemList(items) => {
                CompletionResponse::CompletionItemList(
                    items
                        .into_iter()
                        .filter_map(|item| self.translate_completion_item(uri, item))
                        .collect(),
                )
            }
            CompletionResponse::CompletionList(list) => {
                CompletionResponse::CompletionList(CompletionList {
                    is_incomplete: list.is_incomplete,
                    items: list
                        .items
                        .into_iter()
                        .filter_map(|item| self.translate_completion_item(uri, item))
                        .collect(),
                    apply_kind: None,
                    item_defaults: None,
                })
            }
        }
    }

    /// Translate a location from virtual PHP coordinates back to original Blade
    /// coordinates if the location points into a Blade file.
    pub(crate) fn translate_location(
        &self,
        mut location: tower_lsp_server::gen_lsp_types::Location,
    ) -> tower_lsp_server::gen_lsp_types::Location {
        let uri_str = location.uri.to_string();
        if self.is_blade_file(&uri_str) {
            location.range.start = self.translate_php_to_blade(&uri_str, location.range.start);
            location.range.end = self.translate_php_to_blade(&uri_str, location.range.end);
        }
        location
    }

    /// Like [`Self::translate_location`], but drops a Blade location whose
    /// range falls inside the preprocessor's prologue.
    pub(crate) fn try_translate_location(
        &self,
        mut location: tower_lsp_server::gen_lsp_types::Location,
    ) -> Option<tower_lsp_server::gen_lsp_types::Location> {
        let uri_str = location.uri.to_string();
        if self.is_blade_file(&uri_str) {
            location.range = self.try_translate_blade_range(&uri_str, location.range)?;
        }
        Some(location)
    }

    /// Move a file's edits from the virtual PHP they were planned against
    /// back into the file's own coordinates, dropping the ones that target
    /// a template's injected prologue.
    ///
    /// A no-op for every file that is not a template.  A template is
    /// planned against the virtual PHP it lowers to, because that is what
    /// its symbol map describes, but the editor applies the edits to the
    /// template; the prologue holds declarations no template line stands
    /// behind, so an edit landing there has no position to be applied at.
    ///
    /// Returns whether any edit is left to apply.  A caller offering the
    /// edits as an action has nothing to offer when every one was dropped.
    pub(crate) fn translate_template_edits(
        &self,
        uri: &str,
        edits: &mut Vec<tower_lsp_server::gen_lsp_types::TextEdit>,
    ) -> bool {
        self.retain_translated(uri, FileTextEdits::Plain(edits))
    }

    /// Translate the range of every edit in `edits` back into `uri`'s own
    /// coordinates, dropping the ones that land where the template has no
    /// position, and report whether any edit is left.
    fn retain_translated(&self, uri: &str, mut edits: FileTextEdits<'_>) -> bool {
        if self.is_blade_file(uri) {
            edits.retain_mut(
                |edit| match self.try_translate_blade_range(uri, edit.range) {
                    Some(translated) => {
                        edit.range = translated;
                        true
                    }
                    None => false,
                },
            );
        }
        !edits.is_empty()
    }

    /// Move every template edit of a code action from the virtual PHP it
    /// was planned against back into the template's own coordinates, all
    /// or nothing.
    ///
    /// Edits to files that are not templates are left alone, whichever
    /// shape (`changes` or `document_changes`) carries them.  `request` is
    /// the template the action was asked for, already read, and
    /// `planned_in_template` says its edits were planned against its own
    /// text rather than the virtual PHP, so they stay as they are.
    ///
    /// Returns `false` when any template edit touches text the template did
    /// not write (see [`TemplateText::translate_edit`]), or targets a
    /// template edited since it was lowered: applying the rest would leave
    /// the action half done, so the caller drops it.
    pub(crate) fn translate_action_edit(
        &self,
        edit: &mut tower_lsp_server::gen_lsp_types::WorkspaceEdit,
        request: Option<&TemplateText>,
        planned_in_template: bool,
    ) -> bool {
        let mut applies = true;
        visit_file_text_edits(edit, |uri, mut edits| {
            if !applies || !self.is_blade_file(uri.as_str()) {
                return;
            }
            let read;
            let template = match request.filter(|request| request.uri() == uri.as_str()) {
                Some(_) if planned_in_template => return,
                Some(request) => request,
                None => match self.template_text(uri.as_str()) {
                    Some(template) => {
                        read = template;
                        &read
                    }
                    None => {
                        applies = false;
                        return;
                    }
                },
            };
            applies = edits.try_for_each_mut(|edit| {
                match template.translate_edit(edit.range, &edit.new_text) {
                    Some(range) => {
                        edit.range = range;
                        true
                    }
                    None => false,
                }
            });
        });
        applies
    }
}
