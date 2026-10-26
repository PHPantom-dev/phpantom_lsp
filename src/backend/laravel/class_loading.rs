//! What Laravel adds to class loading: names only the framework gives a
//! meaning, the alias tables a lookup falls back to, and the project facts
//! that refine a loaded class.  Registered with the loader through
//! [`CLASS_LOAD_HOOKS`].

use std::sync::Arc;

use crate::Backend;
use crate::resolution::ClassLoadHooks;
use crate::types::{ClassInfo, FacadeAccessor};
use crate::virtual_members::laravel::{
    AUTH_FACADE_FQN, CONFIGURED_DATE_CLASS_FQN, FILESYSTEM_MANAGER_FQN, GUARD_FQN, REQUEST_FQN,
    STORAGE_FACADE_FQN, SUPPORT_CARBON_FQN,
};

/// Laravel's part in class loading.
pub(crate) const CLASS_LOAD_HOOKS: ClassLoadHooks = ClassLoadHooks {
    claim_name: claim_laravel_name,
    fallback: Backend::resolve_laravel_alias,
    refine: refine_laravel_class,
    refine_generation: laravel_refine_generation,
};

/// The class a name only Laravel gives a meaning names, or `None` when the
/// ordinary lookup should answer.
fn claim_laravel_name(backend: &Backend, name: &str) -> Option<Option<Arc<ClassInfo>>> {
    if name == CONFIGURED_DATE_CLASS_FQN {
        let configured = backend.laravel.date_class.read().clone();
        return Some(configured.and_then(|configured| {
            backend.find_or_load_class(configured.as_deref().unwrap_or(SUPPORT_CARBON_FQN))
        }));
    }
    // A dot can never appear in a PHP identifier or namespace separator,
    // so a name containing one (a Laravel container key like
    // `demo.bakery` or `view.engine.resolver`) is never a class name.
    // `PhpType::parse` stops at the first character an identifier cannot
    // contain, so handing such a key to it would silently truncate
    // `demo.bakery` down to `demo` and let the ordinary lookup phases match
    // an unrelated class of that short name before the alias table is ever
    // consulted. Go there directly instead.
    if name.contains('.') {
        return Some(backend.resolve_laravel_alias(name));
    }
    None
}

/// The sum of the generations of every index [`refine_laravel_class`] reads.
/// Each only grows, so the sum moves whenever any one of them does.
fn laravel_refine_generation(backend: &Backend) -> u64 {
    let laravel = &backend.laravel;
    // Rebuilding bumps the index's generation, so this comes first.
    if laravel.pivots.is_dirty() && laravel.pivots.take_dirty() {
        backend.rebuild_laravel_pivot_index();
    }
    laravel
        .config_facts_generation()
        .wrapping_add(laravel.aliases.generation())
        .wrapping_add(laravel.macros.generation())
        .wrapping_add(laravel.pivots.generation())
}

/// `loaded` with what the project's Laravel facts add to it.
///
/// These stay outside the name-search memo: each depends on an index of
/// its own (the configured auth model, registered macros, many-to-many
/// targets) that keeps changing as files are indexed.  The loader memoises
/// the result against [`laravel_refine_generation`] instead.
fn refine_laravel_class(backend: &Backend, mut loaded: Arc<ClassInfo>) -> Arc<ClassInfo> {
    match loaded.fqn().as_str() {
        // Refine the auth entry points' `user()` return type to the
        // configured model.
        GUARD_FQN | REQUEST_FQN | AUTH_FACADE_FQN => {
            loaded = crate::virtual_members::laravel::patch_auth_user_class(backend, loaded);
        }
        // Refine `disk()`/`drive()`/`cloud()`/`build()` from the abstract
        // `Filesystem`/`Cloud` contract to the concrete `FilesystemAdapter`
        // when no configured disk uses a custom `Storage::extend()` driver.
        FILESYSTEM_MANAGER_FQN | STORAGE_FACADE_FQN => {
            loaded = crate::virtual_members::laravel::patch_storage_disk_type(backend, loaded);
        }
        _ => {}
    }
    // A facade whose `getFacadeAccessor()` names a container binding needs
    // the alias table to reach the class it forwards to, and the virtual
    // member provider that does the forwarding reads that table from the
    // resolved-class cache rather than from here.  Building it now, while
    // the `Backend` is in hand, is what puts it there: this runs before the
    // facade is ever fully resolved, because full resolution reloads the
    // class through this loader first.  Invalidating the table moves the
    // refinement generation, so a memoised facade comes back through here.
    if loaded
        .laravel()
        .is_some_and(|l| matches!(l.facade_accessor, Some(FacadeAccessor::Alias(_))))
    {
        backend.ensure_laravel_aliases();
    }
    // Add any Laravel macros registered on this class.  Gated on a cheap
    // atomic so the hot loader path is untouched when no macros exist.
    loaded = backend.inject_laravel_macros(loaded);
    // Add pivot accessors when this class is a many-to-many target.
    backend.inject_laravel_pivot(loaded)
}

impl Backend {
    /// Add Laravel macro methods registered on `class` (by FQN).
    ///
    /// A no-op unless the project registered at least one
    /// `Target::macro('name', closure)` on this class.  See
    /// [`macros`](crate::backend::laravel::LaravelState::macros).
    fn inject_laravel_macros(&self, class: Arc<ClassInfo>) -> Arc<ClassInfo> {
        if !self.laravel.macros.has_any() {
            return class;
        }
        let index = self.laravel.macros.read();
        crate::virtual_members::laravel::inject_macros(&index, class)
    }

    /// Add Eloquent pivot accessors to `class` when it is the target of a
    /// many-to-many relationship somewhere in the project.
    ///
    /// The reverse index is brought up to date by
    /// [`laravel_refine_generation`], which the loader reads first; a cheap
    /// atomic gates the lock so the hot loader path is untouched when the
    /// project has no such relations.
    fn inject_laravel_pivot(&self, class: Arc<ClassInfo>) -> Arc<ClassInfo> {
        if !self.laravel.pivots.has_any() {
            return class;
        }
        let index = self.laravel.pivots.read();
        crate::virtual_members::laravel::inject_pivot(&index, class)
    }
}
