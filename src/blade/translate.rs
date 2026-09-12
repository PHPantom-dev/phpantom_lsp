//! Moving positions, ranges, and edits between a Blade template and the
//! virtual PHP the preprocessor lowers it to.
//!
//! Every feature plans its answer against the virtual PHP, because that
//! is what the symbol map and the type engine describe, but the editor
//! only knows the template. The helpers here translate through the
//! file's recorded [`super::source_map::BladeSourceMap`], and drop what
//! lands in the injected prologue, which has no template text behind it.

use crate::Backend;

impl Backend {
    /// Check whether a URI refers to a Blade template file.
    /// Returns true if the URI ends with `.blade.php` OR was opened with `languageId == "blade"`.
    pub(crate) fn is_blade_file(&self, uri: &str) -> bool {
        crate::blade::is_blade_file(uri) || self.blade_uris.read().contains(uri)
    }

    /// Translate a position from an original Blade file to the virtual PHP file.
    pub(crate) fn translate_blade_to_php(
        &self,
        uri: &str,
        pos: tower_lsp::lsp_types::Position,
    ) -> tower_lsp::lsp_types::Position {
        if let Some(map) = self.blade_source_maps.read().get(uri) {
            map.blade_to_php(pos)
        } else {
            pos
        }
    }

    /// Translate a position from a virtual PHP file back to the original Blade file.
    pub(crate) fn translate_php_to_blade(
        &self,
        uri: &str,
        pos: tower_lsp::lsp_types::Position,
    ) -> tower_lsp::lsp_types::Position {
        if let Some(map) = self.blade_source_maps.read().get(uri) {
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
        pos: tower_lsp::lsp_types::Position,
    ) -> Option<tower_lsp::lsp_types::Position> {
        match self.blade_source_maps.read().get(uri) {
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
        range: tower_lsp::lsp_types::Range,
    ) -> tower_lsp::lsp_types::Range {
        tower_lsp::lsp_types::Range {
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
        range: tower_lsp::lsp_types::Range,
    ) -> tower_lsp::lsp_types::Range {
        tower_lsp::lsp_types::Range {
            start: self.translate_php_to_blade(uri, range.start),
            end: self.translate_php_to_blade(uri, range.end),
        }
    }

    /// Translate a range from virtual PHP coordinates back to original Blade
    /// coordinates, dropping it when either end lies in the prologue.
    pub(crate) fn try_translate_blade_range(
        &self,
        uri: &str,
        range: tower_lsp::lsp_types::Range,
    ) -> Option<tower_lsp::lsp_types::Range> {
        Some(tower_lsp::lsp_types::Range {
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
        mut item: tower_lsp::lsp_types::CompletionItem,
    ) -> Option<tower_lsp::lsp_types::CompletionItem> {
        use tower_lsp::lsp_types::CompletionTextEdit;

        if let Some(edit) = item.text_edit.take() {
            item.text_edit = Some(match edit {
                CompletionTextEdit::Edit(mut e) => {
                    e.range = self.try_translate_blade_range(uri, e.range)?;
                    CompletionTextEdit::Edit(e)
                }
                CompletionTextEdit::InsertAndReplace(mut e) => {
                    e.insert = self.try_translate_blade_range(uri, e.insert)?;
                    e.replace = self.try_translate_blade_range(uri, e.replace)?;
                    CompletionTextEdit::InsertAndReplace(e)
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
        response: tower_lsp::lsp_types::CompletionResponse,
    ) -> tower_lsp::lsp_types::CompletionResponse {
        use tower_lsp::lsp_types::{CompletionList, CompletionResponse};

        match response {
            CompletionResponse::Array(items) => CompletionResponse::Array(
                items
                    .into_iter()
                    .filter_map(|item| self.translate_completion_item(uri, item))
                    .collect(),
            ),
            CompletionResponse::List(list) => CompletionResponse::List(CompletionList {
                is_incomplete: list.is_incomplete,
                items: list
                    .items
                    .into_iter()
                    .filter_map(|item| self.translate_completion_item(uri, item))
                    .collect(),
            }),
        }
    }

    /// Translate a location from virtual PHP coordinates back to original Blade
    /// coordinates if the location points into a Blade file.
    pub(crate) fn translate_location(
        &self,
        mut location: tower_lsp::lsp_types::Location,
    ) -> tower_lsp::lsp_types::Location {
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
        mut location: tower_lsp::lsp_types::Location,
    ) -> Option<tower_lsp::lsp_types::Location> {
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
        edits: &mut Vec<tower_lsp::lsp_types::TextEdit>,
    ) -> bool {
        if !self.is_blade_file(uri) {
            return !edits.is_empty();
        }
        edits.retain_mut(
            |edit| match self.try_translate_blade_range(uri, edit.range) {
                Some(range) => {
                    edit.range = range;
                    true
                }
                None => false,
            },
        );
        !edits.is_empty()
    }

    /// Move every template edit in a workspace edit from the virtual PHP it
    /// was planned against back into the template's own coordinates, the
    /// way [`Self::translate_template_edits`] does for one file's edits.
    ///
    /// Edits to files that are not templates are left alone, and an edit
    /// that targets a template's injected prologue is dropped, whichever
    /// shape (`changes` or `document_changes`) carries it.
    ///
    /// Returns whether the workspace edit still applies anything: a text
    /// edit in any file, or a resource operation (which no translation
    /// touches).  `false` means every edit was dropped and the caller has
    /// nothing left to offer.
    pub(crate) fn translate_workspace_edit(
        &self,
        edit: &mut tower_lsp::lsp_types::WorkspaceEdit,
    ) -> bool {
        use tower_lsp::lsp_types::{DocumentChangeOperation, DocumentChanges};

        let mut applies = false;
        if let Some(changes) = edit.changes.as_mut() {
            for (uri, edits) in changes.iter_mut() {
                applies |= self.translate_template_edits(uri.as_str(), edits);
            }
        }
        let Some(document_changes) = edit.document_changes.as_mut() else {
            return applies;
        };
        match document_changes {
            DocumentChanges::Edits(edits) => {
                for document in edits {
                    applies |= self.translate_document_edit(document);
                }
            }
            DocumentChanges::Operations(operations) => {
                for operation in operations {
                    match operation {
                        DocumentChangeOperation::Edit(document) => {
                            applies |= self.translate_document_edit(document);
                        }
                        DocumentChangeOperation::Op(_) => applies = true,
                    }
                }
            }
        }
        applies
    }

    /// [`Self::translate_template_edits`] for the edits of one
    /// `TextDocumentEdit`, whichever of the two edit shapes each carries.
    fn translate_document_edit(
        &self,
        document: &mut tower_lsp::lsp_types::TextDocumentEdit,
    ) -> bool {
        use tower_lsp::lsp_types::OneOf;

        let uri = document.text_document.uri.as_str();
        if !self.is_blade_file(uri) {
            return !document.edits.is_empty();
        }
        document.edits.retain_mut(|edit| {
            let range = match edit {
                OneOf::Left(edit) => &mut edit.range,
                OneOf::Right(edit) => &mut edit.text_edit.range,
            };
            match self.try_translate_blade_range(uri, *range) {
                Some(translated) => {
                    *range = translated;
                    true
                }
                None => false,
            }
        });
        !document.edits.is_empty()
    }
}
