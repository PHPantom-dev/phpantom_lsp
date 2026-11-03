//! The middleware alias and group index: the framework's defaults, the
//! application's HTTP kernels and `bootstrap/app.php`, and the router
//! registrations in the service providers.

use crate::Backend;
use crate::virtual_members::laravel::file_contributions::refresh_file;
use crate::virtual_members::laravel::middleware::{
    MIDDLEWARE_CONFIGURATION_FQN, may_register_middleware, scan_middleware,
};

impl Backend {
    /// Build the middleware index.
    ///
    /// The files read are the framework's `Middleware` configurator (for
    /// its defaults), `bootstrap/app.php`, every class of the project's own
    /// named like an HTTP kernel (an application may have more than one),
    /// and every registered service provider (where a package aliases its
    /// middleware on the router).
    pub(crate) fn build_laravel_middleware_index(&self, providers: &super::LaravelProviders) {
        let mut index = crate::virtual_members::laravel::LaravelMiddlewareIndex::default();

        let mut candidates: Vec<String> = Vec::new();
        if let Some(uri) = self.resolve_class_uri(MIDDLEWARE_CONFIGURATION_FQN) {
            candidates.push(uri);
        }
        if let Some(root) = self.workspace.workspace_root.read().clone() {
            candidates.push(crate::util::path_to_uri(&root.join("bootstrap/app.php")));
        }
        {
            let fqn_index = self.symbols.fqn_uri_index.read();
            for (fqn, uri) in fqn_index.iter() {
                if fqn.ends_with("Kernel") && !self.is_vendor_uri(uri) {
                    candidates.push(uri.to_string());
                }
            }
        }

        let mut scanned = 0usize;
        for uri in candidates {
            if index.files.has_uri(&uri) {
                continue;
            }
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            scanned += 1;
            index.files.set_file(uri, scan_middleware(&content));
        }
        scanned += self.scan_providers_into(providers, &mut index.files, scan_middleware);

        index.rebuild();
        let count = index.all_names().len();
        *self.laravel.middleware.write() = index;

        tracing::info!(
            "PHPantom: scanned {} Laravel middleware files, indexed {} middleware aliases and groups",
            scanned,
            count,
        );
    }

    /// Re-scan a single file's middleware registrations after an edit.
    ///
    /// A cheap no-op unless the file currently contributes registrations or
    /// its new content mentions middleware.  Only runs for Laravel projects.
    pub(crate) fn refresh_laravel_middleware(&self, uri: &str, content: &str) {
        if !self.resolved_class_cache.read().laravel().is_enabled() {
            return;
        }
        refresh_file(
            &self.laravel.middleware,
            uri,
            may_register_middleware(content),
            || scan_middleware(content),
        );
    }

    fn is_vendor_uri(&self, uri: &str) -> bool {
        self.workspace
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
    }
}
