//! Primary-key types supplied by Laravel's UUID and ULID traits.

use std::sync::Arc;

use crate::atom::AtomSet;
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH, MAX_TRAIT_DEPTH};

use super::ELOQUENT_MODEL_FQN;

/// Whether a model uses `HasUuids` or `HasUlids`, including through
/// composed traits and parent models. Base resolution retains only the
/// leaf class's trait names, so inspect the original declarations too.
pub(super) fn uses_unique_string_ids(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    let mut visited = AtomSet::default();
    if traits_use_unique_string_ids(class, class_loader, &mut visited, 0) {
        return true;
    }

    let mut parent_name = class.parent_class;
    for _ in 0..MAX_INHERITANCE_DEPTH {
        let Some(name) = parent_name else {
            break;
        };
        // The framework base model has no UUID/ULID trait. Avoid walking
        // its internal concerns for every ordinary application model.
        if name.eq_ignore_ascii_case(ELOQUENT_MODEL_FQN) {
            break;
        }
        let Some(parent) = class_loader(&name) else {
            break;
        };
        if traits_use_unique_string_ids(&parent, class_loader, &mut visited, 0) {
            return true;
        }
        parent_name = parent.parent_class;
    }
    false
}

fn traits_use_unique_string_ids(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    visited: &mut AtomSet,
    depth: u32,
) -> bool {
    if depth >= MAX_TRAIT_DEPTH {
        return false;
    }
    for name in &class.used_traits {
        let fqn = name.trim_start_matches('\\');
        if fqn.eq_ignore_ascii_case("Illuminate\\Database\\Eloquent\\Concerns\\HasUuids")
            || fqn.eq_ignore_ascii_case("Illuminate\\Database\\Eloquent\\Concerns\\HasUlids")
        {
            return true;
        }
        if visited.insert(*name)
            && let Some(trait_class) = class_loader(name)
            && traits_use_unique_string_ids(&trait_class, class_loader, visited, depth + 1)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[path = "unique_ids_tests.rs"]
mod tests;
