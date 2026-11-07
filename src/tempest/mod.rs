//! Tempest View templates, lowered to PHP for the shared type engine.

mod components;
mod discovery;
mod paths;
pub(crate) mod preprocessor;
mod syntax;

pub(crate) use discovery::TempestState;

/// Tempest discovers views by their `.view.php` suffix, independently of
/// whether its renderer is used standalone or through the full framework.
pub(crate) fn is_view(uri: &str) -> bool {
    uri.ends_with(".view.php")
}

impl crate::Backend {
    /// Whether the project renders Tempest views; see
    /// [`Backend::is_tempest`](crate::Backend::is_tempest).
    pub(crate) fn is_tempest_project(&self) -> bool {
        self.is_tempest.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Record whether the project renders Tempest views, once the
    /// workspace's `composer.json` files have been read.
    pub(crate) fn set_is_tempest(&self, is_tempest: bool) {
        self.is_tempest
            .store(is_tempest, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether `uri` is a Tempest view.  Outside a Tempest project a
    /// `.view.php` file is ordinary PHP: other renderers use the suffix
    /// too, and their markup (an Alpine `:class` binding, say) is not PHP.
    pub(crate) fn is_tempest_view(&self, uri: &str) -> bool {
        is_view(uri) && self.is_tempest_project()
    }
}

#[cfg(test)]
mod tests;
