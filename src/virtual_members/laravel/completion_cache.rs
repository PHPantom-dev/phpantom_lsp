//! Warming Laravel's shared builder classes ahead of the first completion.
//!
//! Pre-resolving the framework-level builder classes keeps the first
//! `Model::query()->` completion off the full inheritance + mixin + patch
//! cost on the editor hot path.

use crate::Backend;
use crate::class_lookup::Fqn;

impl Backend {
    /// Pre-resolve Laravel's shared builder classes so the first
    /// `Model::query()->` or `Model::with()->` completion does not pay
    /// the full inheritance + mixin + patch cost on the editor hot path.
    ///
    /// This intentionally warms only framework-level classes. Per-model
    /// generic specialisations like `Builder<User>` still depend on the
    /// concrete model and are resolved on demand.
    pub(crate) fn warm_laravel_completion_cache(&self) -> usize {
        let loader = |name: Fqn<'_>| self.find_or_load_class(&name);
        let mut warmed = 0usize;

        for fqn in [
            crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN,
            "Illuminate\\Database\\Query\\Builder",
        ] {
            let Some(class_info) = self.find_or_load_class(fqn) else {
                continue;
            };
            crate::virtual_members::resolve_class_fully_cached(
                &class_info,
                &loader,
                &self.resolved_class_cache,
            );
            warmed += 1;
        }

        warmed
    }
}
