//! The workspace-wide Laravel string-key enumerations, cached on the
//! `Backend` until an edit invalidates them.

use std::sync::Arc;

/// The Laravel string keys (routes, config keys, views, translations,
/// abilities, …) enumerated from the workspace, each built on first use
/// and dropped again when an edit could change it.
#[derive(Default)]
pub(crate) struct LaravelStringKeyCache {
    /// Named routes with the URI each was registered with, plus any group
    /// prefixes whose full set of children is unknowable.  Shared behind an
    /// `Arc` because both the name list and the parameter names of one route
    /// are read from it, and cloning the whole table per read would be waste.
    pub routes: Option<std::sync::Arc<crate::virtual_members::laravel::RouteDiscovery>>,
    /// The config keys `config/` declares, sorted, so a lookup is a binary
    /// search rather than a set built per diagnostic pass.  Like the other
    /// key lists, shared behind an `Arc` so a read does not copy it.
    pub config_keys: Option<std::sync::Arc<[String]>>,
    /// Every Blade view name the project ships, sorted.
    pub view_names: Option<std::sync::Arc<[String]>>,
    /// The Blade view roots `config/view.php` configures, each with its
    /// canonical spelling.  Every template's view name is worked out
    /// against them, which would otherwise re-read and re-parse the config
    /// file once per template.
    pub view_roots: Option<std::sync::Arc<Vec<crate::blade::view_paths::ViewRoot>>>,
    /// Shared translation declarations, values, locales, and file locations.
    pub translations: Option<Arc<crate::virtual_members::laravel::TranslationCatalog>>,
    /// The Blade templates and component classes the project ships, keyed
    /// by the names Laravel addresses them under.  Shared behind an `Arc`
    /// because consumers look up a single name in one of its three maps and
    /// cloning the whole index per lookup would be waste.
    pub blade_discovery: Option<std::sync::Arc<crate::blade::discovery::BladeDiscovery>>,
    /// The section and stack names every template of the project writes,
    /// with what each one extends and includes.  Shared behind an `Arc`
    /// because an edit updates the entry of the one template that changed
    /// rather than replacing the whole index.
    pub blade_blocks: Option<std::sync::Arc<crate::blade::block_index::BladeBlockIndex>>,
    /// The parsed tree of every config file, framework defaults merged in,
    /// keyed by its prefix.  Shared behind an `Arc` because every
    /// `config('…')` call the type engine resolves reads it.
    pub config_trees: Option<
        std::sync::Arc<
            Vec<(
                String,
                crate::virtual_members::laravel::config_values::ConfigNode,
            )>,
        >,
    >,
    /// The variables service providers share into every template, and the
    /// ones their view composers add to the templates they target, with each
    /// value expression already resolved to a type.  Shared behind an `Arc`
    /// because every Blade template reads the whole set to pick the groups
    /// that reach it.
    pub shared_view_vars: Option<std::sync::Arc<Vec<crate::blade::shared_vars::SharedVarGroup>>>,
    /// Every authorization ability the project knows: `Gate::define()`
    /// registrations plus the methods of every policy class, sorted.
    pub gate_abilities: Option<std::sync::Arc<[String]>>,
}

/// Compute-once guards for the entries of [`LaravelStringKeyCache`].
///
/// Every enumeration behind that cache walks the workspace from disk.
/// A plain check-then-fill cache stampedes under the parallel
/// diagnostic pass: all workers miss the empty slot in the same
/// instant, and each one repeats the identical walk while the others
/// queue behind the workspace index lock. Holding the matching guard
/// across the build means one worker walks and the rest wait once,
/// then read the filled slot.
///
/// One guard per slot rather than one shared guard: the enumerations
/// are independent, and a shared guard would let a worker that needs
/// view names wait out an unrelated route-name build.
#[derive(Default)]
pub(crate) struct LaravelStringKeyBuildLocks {
    pub routes: parking_lot::Mutex<()>,
    pub config_keys: parking_lot::Mutex<()>,
    pub view_names: parking_lot::Mutex<()>,
    pub view_roots: parking_lot::Mutex<()>,
    pub translations: parking_lot::Mutex<()>,
    pub config_trees: parking_lot::Mutex<()>,
    pub blade_discovery: parking_lot::Mutex<()>,
    pub blade_blocks: parking_lot::Mutex<()>,
    pub shared_view_vars: parking_lot::Mutex<()>,
    pub gate_abilities: parking_lot::Mutex<()>,
}

impl LaravelStringKeyCache {
    pub(crate) fn invalidate_for_uri(&mut self, uri: &str, content: &str) {
        // Abilities come from `Gate::define()` calls and from the methods of
        // every policy class, so an edit to either invalidates the set.  The
        // token is `Gate::` rather than `Gate` so an unrelated `Gateway` does
        // not throw the enumeration away on every keystroke.
        if uri.ends_with("Policy.php")
            || memchr::memmem::find(content.as_bytes(), b"Gate::").is_some()
        {
            self.gate_abilities = None;
        }
        // A Folio page registers a route without ever touching `routes/`, so
        // its own edits have to invalidate the route cache too — gated on
        // the page mentioning `Folio` at all (its `name()` import or a
        // fully-qualified call), the same way the `Gate::` check above
        // avoids paying for every unrelated Blade edit.  `bootstrap/app.php`
        // is where a Folio mount is most commonly registered
        // (`withRouting(pages: ...)`), and it is not a service provider, so
        // it needs its own trigger rather than riding along with one.
        // Routes are also registered from files loaded outside `routes/`,
        // such as a module's route file a service provider loads, so an edit
        // to any file the last scan read drops them too.
        if uri.contains("/routes/")
            || uri.ends_with("/bootstrap/app.php")
            || (uri.ends_with(".blade.php")
                && memchr::memmem::find(content.as_bytes(), b"Folio").is_some())
            || self.routes.as_ref().is_some_and(|routes| routes.reads(uri))
        {
            self.routes = None;
        }
        if uri.contains("/config/") {
            self.config_keys = None;
            self.config_trees = None;
        }
        // View roots are configurable via `config/view.php`, so a Blade
        // file may live outside `resources/views/`. Invalidate on any
        // Blade file, any path containing a `views/` directory, and on
        // `config/view.php` itself (which changes the set of roots).
        let looks_like_view_file = uri.ends_with(".blade.php") || uri.contains("/views/");
        if looks_like_view_file || uri.contains("/config/view.php") {
            self.view_names = None;
        }
        // A file that is not covered by any already-known root may be the
        // first one to land under a directory that has just appeared (a
        // `resources/views` created after the roots were cached, or a
        // custom root from `config/view.php` that didn't exist yet):
        // recompute so the new directory is picked up without waiting for
        // `config/view.php` itself to change.
        if uri.contains("/config/view.php")
            || (looks_like_view_file
                && self
                    .view_roots
                    .as_deref()
                    .is_some_and(|roots| !crate::blade::view_paths::view_root_covers(roots, uri)))
        {
            self.view_roots = None;
        }
        if uri.contains("/lang/")
            || self
                .translations
                .as_ref()
                .is_some_and(|catalog| catalog.contains_uri(uri))
        {
            self.translations = None;
        }
    }
}
