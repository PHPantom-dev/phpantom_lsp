//! What the resolved-class cache carries for Laravel.
//!
//! Virtual member providers receive the cache but never the `Backend`, so
//! the project facts the Laravel providers read travel inside it, in this
//! one slot the generic cache knows nothing about.

use super::database_schema::SchemaIndex;
use super::{LaravelAliasSlot, new_alias_slot};

/// The Laravel facts the resolved-class cache hands to the providers.
pub(crate) struct LaravelCacheState {
    /// Whether the current project uses Laravel (or a standalone
    /// Illuminate component).  When `false`, `resolve_class_fully_inner`
    /// skips the Laravel virtual member providers, the contract to
    /// concrete-class mixin bindings, and the post-resolution Laravel
    /// patches, so a non-Laravel project pays none of that scanning cost.
    ///
    /// Defaults to `true`: Laravel analysis stays enabled unless the server
    /// has parsed `composer.json` and confirmed no Laravel/Illuminate
    /// dependency.  This keeps behaviour unchanged for the CLI test
    /// backends and any code path that resolves without a project context.
    /// Not touched when the cache is cleared — it reflects the project, not
    /// the per-edit cache contents.
    enabled: bool,
    schema_index: SchemaIndex,
    /// Laravel's alias tables, shared by handle with the `Backend` that
    /// builds them (see [`LaravelAliasSlot`]).
    ///
    /// A facade whose `getFacadeAccessor()` names a container binding
    /// (`return 'sentry';`) needs the container table to reach the class it
    /// forwards to.  Sharing the slot rather than a snapshot means the
    /// provider always reads the table the `Backend` most recently built,
    /// including after a rebuild.  A cache created without a `Backend`
    /// (tests, the throwaway cache a cache-less resolution falls back to)
    /// holds an empty slot and simply forwards nothing.
    aliases: LaravelAliasSlot,
}

impl Default for LaravelCacheState {
    fn default() -> Self {
        Self {
            enabled: true,
            schema_index: SchemaIndex::default(),
            aliases: new_alias_slot(),
        }
    }
}

impl LaravelCacheState {
    /// Whether the project uses Laravel / Illuminate.  See the `enabled`
    /// field.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Record whether the project uses Laravel / Illuminate.
    ///
    /// Set once by the server after parsing `composer.json`.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn schema_index(&self) -> &SchemaIndex {
        &self.schema_index
    }

    pub fn set_schema_index(&mut self, schema_index: SchemaIndex) {
        self.schema_index = schema_index;
    }

    /// The concrete class FQN a Laravel container binding key is bound to,
    /// or `None` when the alias tables are not built or the key is bound
    /// only at runtime.
    ///
    /// The slot lock is only ever held for the length of a read or a
    /// single assignment (never across the build itself), so taking it
    /// under the cache lock cannot deadlock.
    pub(crate) fn container_alias_concrete_fqn(&self, key: &str) -> Option<String> {
        self.aliases.peek()?.container.get(key).cloned()
    }

    /// Share the `Backend`'s alias slot.  Called once at construction; the
    /// slot's contents are then always the tables the `Backend` most
    /// recently built.
    pub(crate) fn set_aliases(&mut self, slot: LaravelAliasSlot) {
        self.aliases = slot;
    }
}
