//! The types Drupal's own declarations give: a service `\Drupal::service()`
//! looks up.
//!
//! Shared by the two places the type engine resolves a call: an assignment
//! (`$x = …`) and a chain (`…->method()`).

use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::types::ClassInfo;

/// The classes `\Drupal::service(id)` returns, when `class` is the global
/// `Drupal` class and the project declares `id`.
pub(crate) fn service_classes(
    backend: &Backend,
    class: &str,
    method_name: &str,
    id: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Vec<Arc<ClassInfo>>> {
    if !method_name.eq_ignore_ascii_case("service")
        || class.strip_prefix('\\').unwrap_or(class) != "Drupal"
        || !backend.is_drupal_project()
    {
        return None;
    }
    let classes: Vec<Arc<ClassInfo>> = backend
        .symfony_service_classes(id)
        .iter()
        .filter_map(|class| class_loader(Fqn::new(class)))
        .collect();
    (!classes.is_empty()).then_some(classes)
}
