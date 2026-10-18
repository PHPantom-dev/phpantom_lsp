//! Primary-key types supplied by Laravel's UUID and ULID traits.

use std::sync::Arc;

use crate::inheritance::{ancestors, find_declaring_trait};
use crate::types::ClassInfo;

use super::ELOQUENT_MODEL_FQN;

/// Whether a model uses `HasUuids` or `HasUlids`, including through
/// composed traits and parent models. Base resolution retains only the
/// leaf class's trait names, so inspect the original declarations too.
pub(super) fn uses_unique_string_ids(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    // The trait is recognised by the name a class uses it by, so it is
    // found even when its own source cannot be loaded.
    let uses_directly = |c: &ClassInfo| {
        c.used_traits.iter().any(|name| {
            let fqn = name.trim_start_matches('\\');
            fqn.eq_ignore_ascii_case("Illuminate\\Database\\Eloquent\\Concerns\\HasUuids")
                || fqn.eq_ignore_ascii_case("Illuminate\\Database\\Eloquent\\Concerns\\HasUlids")
        })
    };
    let uses_through_traits = |c: &ClassInfo| {
        uses_directly(c)
            || find_declaring_trait(&c.used_traits, class_loader, &uses_directly).is_some()
    };
    uses_through_traits(class)
        || model_ancestors(class, class_loader).any(|parent| uses_through_traits(&parent))
}

/// Whether the traits generate `key`, mirroring their `getKeyType()`
/// check of `in_array($this->getKeyName(), $this->uniqueIds())`. The
/// nearest `uniqueIds()` override decides; without a statically known
/// one, the traits' own `uniqueIds()` returns the primary key.
pub(super) fn unique_ids_include(
    class: &ClassInfo,
    key: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    let unique_ids = |c: &ClassInfo| {
        c.laravel()
            .and_then(|l| l.unique_ids.as_ref())
            .map(|ids| ids.iter().any(|id| id == key))
    };
    unique_ids(class)
        .or_else(|| model_ancestors(class, class_loader).find_map(|parent| unique_ids(&parent)))
        .unwrap_or(true)
}

/// The parents of a model up to the framework's base model, which has no
/// UUID/ULID trait: walking its internal concerns for every ordinary
/// application model would find nothing.
fn model_ancestors<'a>(
    class: &ClassInfo,
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> impl Iterator<Item = Arc<ClassInfo>> + use<'a> {
    ancestors(class, class_loader)
        .take_while(|(name, _)| !name.eq_ignore_ascii_case(ELOQUENT_MODEL_FQN))
        .map(|(_, parent)| parent)
}

#[cfg(test)]
#[path = "unique_ids_tests.rs"]
mod tests;
