//! Translation hover across all statically known locales.

use crate::Backend;

impl Backend {
    /// Describe each locale's translation and link to its exact declaration.
    pub(super) fn translation_hover_detail(&self, key: &str) -> String {
        let catalog = self.cached_translations();
        let mut parts = Vec::new();
        if let Some(entries) = catalog.entries.get(key) {
            for entry in entries {
                let file = &catalog.files[entry.file];
                parts.push(locale_detail(
                    &file.locale,
                    &file.uri,
                    entry.range.start.line,
                    entry.value.as_deref(),
                ));
            }
        } else {
            for file in catalog
                .files
                .iter()
                .filter(|file| file.group.as_deref() == Some(key))
            {
                parts.push(locale_detail(&file.locale, &file.uri, 0, None));
            }
        }
        if parts.is_empty() {
            "Translation key".to_string()
        } else {
            parts.join("\n\n")
        }
    }
}

fn locale_detail(
    locale: &str,
    uri: &tower_lsp::lsp_types::Url,
    line: u32,
    value: Option<&str>,
) -> String {
    let path = uri.path();
    let short_path = path
        .find("/resources/lang/")
        .or_else(|| path.find("/lang/"))
        .map_or(path, |offset| &path[offset + 1..]);
    let mut detail = super::inline_code(locale);
    if let Some(value) = value {
        detail.push_str(&format!(": {}", super::inline_code(value)));
    }
    detail.push_str(&format!(
        "\n\nDefined in [{}](<{}#L{}>)",
        super::inline_code(short_path),
        uri,
        line + 1
    ));
    detail
}

#[cfg(test)]
#[path = "laravel_trans_tests.rs"]
mod tests;
