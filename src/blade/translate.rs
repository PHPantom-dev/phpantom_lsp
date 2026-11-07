//! Blade document identification.

use crate::Backend;

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
}
