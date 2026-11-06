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

#[cfg(test)]
mod tests;
