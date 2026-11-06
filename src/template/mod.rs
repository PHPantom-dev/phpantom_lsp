//! Shared virtual-PHP source mapping for template languages.

pub mod source_map;
pub mod source_spans;
mod translate;

impl crate::Backend {
    /// Whether a document is lowered to PHP before analysis.
    pub(crate) fn is_php_template(&self, uri: &str) -> bool {
        crate::tempest::is_view(uri) || self.is_blade_file(uri)
    }
}
