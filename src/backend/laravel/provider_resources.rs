//! The resources the registered service providers publish: config files,
//! view and translation directories, route files, container bindings,
//! component namespaces, and Blade directives.

use crate::Backend;

impl Backend {
    pub(crate) fn build_provider_resources(&self, providers: &super::LaravelProviders) {
        let mut scans = crate::virtual_members::laravel::ProviderScans::default();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (fqn, origin) in providers.with_origin() {
            scans.record_registered(fqn);
            let Some(uri) = self.resolve_class_uri(fqn) else {
                continue;
            };
            if !seen.insert(uri.clone()) {
                continue;
            }
            let Some((identity, resources)) =
                self.scan_provider_resources(&uri, None, fqn, *origin)
            else {
                continue;
            };
            scans.record(crate::virtual_members::laravel::ProviderScan {
                uri,
                identity,
                resources,
            });
        }
        scans.mark_built();

        let resources = scans.merged();
        *self.laravel_provider_scans.write() = scans;

        let config_count = resources.config_files.len();
        let view_count = resources.view_dirs.len();
        let trans_count = resources.trans_dirs.len();
        let route_count = resources.route_files.len();
        let binding_count = resources.bindings.len();
        self.publish_provider_resources(resources);

        tracing::info!(
            "PHPantom: discovered {} package config files, {} view dirs, {} translation dirs, {} route files, {} container bindings from service providers",
            config_count,
            view_count,
            trans_count,
            route_count,
            binding_count,
        );
    }

    /// Scan one provider file for the resources it registers, returning them
    /// alongside the identity they were recorded under.
    ///
    /// `content` is the edited buffer when the caller has one; otherwise the
    /// file is read through the normal content path.
    fn scan_provider_resources(
        &self,
        uri: &str,
        content: Option<&str>,
        fqn: &str,
        origin: crate::virtual_members::laravel::ProviderOrigin,
    ) -> Option<(
        std::sync::Arc<crate::virtual_members::laravel::ProviderIdentity>,
        crate::virtual_members::laravel::ProviderResources,
    )> {
        let owned;
        let content = match content {
            Some(content) => content,
            None => {
                owned = self.get_file_content(uri)?;
                &owned
            }
        };
        let file_path = tower_lsp::lsp_types::Url::parse(uri)
            .ok()
            .and_then(|u| u.to_file_path().ok())?;
        let workspace_root = self
            .workspace
            .workspace_root
            .read()
            .clone()
            .unwrap_or_default();

        let identity = std::sync::Arc::new(crate::virtual_members::laravel::ProviderIdentity {
            ancestors: self.provider_ancestors(fqn),
            fqn: fqn.to_string(),
            origin,
        });
        let resources = crate::virtual_members::laravel::extract_provider_resources(
            content,
            &file_path,
            &workspace_root,
            self.provider_class_context(fqn),
            std::sync::Arc::clone(&identity),
        );
        Some((identity, resources))
    }

    /// Drop a deleted provider file's registrations from the merged table.
    pub(super) fn forget_laravel_provider_resources(&self, uri: &str) {
        let merged = {
            let mut scans = self.laravel_provider_scans.write();
            if !scans.is_built() || !scans.remove(uri) {
                return;
            }
            scans.merged()
        };
        self.publish_provider_resources(merged);
    }

    /// Publish a freshly merged provider-resource table, dropping the caches
    /// that were derived from the previous one.
    fn publish_provider_resources(
        &self,
        resources: crate::virtual_members::laravel::ProviderResources,
    ) {
        let has_string_key_sources = resources.config_files.len()
            + resources.view_dirs.len()
            + resources.trans_dirs.len()
            + resources.route_files.len()
            + resources.class_component_namespaces.len()
            + resources.folio_mounts.len()
            > 0;
        let has_bindings = !resources.bindings.is_empty();
        let directives = crate::blade::directives::CustomDirectives::from_registrations(
            &resources.custom_directives,
        );
        let directives_changed = *self.blade_custom_directives.read() != directives;
        *self.blade_custom_directives.write() = directives;
        *self.laravel_provider_resources.write() = resources;

        // The shared and composed template variables are resolved from these
        // registrations, so the previous scan's set is stale whether or not
        // any other resource count moved.
        self.laravel_string_key_cache.write().shared_view_vars = None;

        if has_string_key_sources {
            let mut cache = self.laravel_string_key_cache.write();
            cache.config_keys = None;
            cache.config_trees = None;
            cache.view_names = None;
            cache.trans_keys = None;
            cache.trans_key_shapes = None;
            cache.routes = None;
            cache.blade_discovery = None;
        }

        // The provider bindings overlay the core container alias table, which
        // an earlier resolution may already have built without them.
        if has_bindings {
            self.laravel_aliases.invalidate();
            self.clear_class_not_found_cache();
            // A binding is followed without a class lookup naming the
            // provider that registered it, so a cached receiver resolution
            // records no dependency on the alias table.
            self.clear_resolved_member_files();
        }

        // Which directives exist decides what the preprocessor lowers rather
        // than masks, and the providers registering them are scanned after
        // the workspace index has already preprocessed every template — so
        // the templates have to be preprocessed again against the set that
        // just arrived.
        if directives_changed {
            self.reparse_blade_templates();
        }
    }

    /// Re-preprocess every Blade template already in the virtual-PHP cache.
    ///
    /// A no-op before any template has been preprocessed, and only reached
    /// when the registered directive set actually moved, so the pass cannot
    /// repeat itself: the second scan publishes the same set.
    fn reparse_blade_templates(&self) {
        let uris: Vec<String> = self.blade_virtual_content.read().keys().cloned().collect();
        for uri in uris {
            if let Some(content) = self.get_file_content(&uri) {
                self.update_ast(&uri, &content);
            }
        }
    }

    /// Re-scan a single service provider's registrations after an edit.
    ///
    /// A binding written now has to resolve now: nothing else records that a
    /// container key names a class, and the same goes for the view and
    /// translation directories, route and config files, and component
    /// namespaces a provider registers.
    ///
    /// The merged table is rebuilt from the cached per-provider scans rather
    /// than patched, because a key two providers bind belongs to whichever of
    /// them the container would let win, and only the merge knows that.  A
    /// cheap no-op for every file that is not a registered provider, and until
    /// the full scan has run.
    pub(crate) fn refresh_laravel_provider_resources(
        &self,
        uri: &str,
        content: &str,
        providers: &super::ProvidersOnce<'_>,
    ) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        if !self.laravel_provider_scans.read().is_built() {
            return;
        }

        // The app's provider list decides which providers are registered and
        // in what order, both of which the merge depends on, so a change to it
        // rebuilds from scratch.
        if self.is_laravel_provider_list_uri(uri) {
            self.build_provider_resources(providers.get());
            return;
        }

        let known = {
            let scans = self.laravel_provider_scans.read();
            scans
                .scan_for(uri)
                .map(|scan| (scan.identity.fqn.clone(), scan.identity.origin))
        };
        let Some((fqn, origin)) = known else {
            // A registered provider whose class could not be resolved when the
            // full scan ran, because the file is written after the list that
            // names it.  It joins the table the moment it parses.
            if self.declares_registered_provider(uri) {
                self.build_provider_resources(providers.get());
            }
            return;
        };

        let Some((identity, resources)) =
            self.scan_provider_resources(uri, Some(content), &fqn, origin)
        else {
            return;
        };

        let merged = {
            let mut scans = self.laravel_provider_scans.write();
            // An edit that leaves the registrations alone (any edit outside
            // them, which is most of them) changes nothing downstream, so the
            // derived caches are left intact.
            if scans
                .scan_for(uri)
                .is_some_and(|scan| scan.resources == resources && scan.identity == identity)
            {
                return;
            }
            if !scans.replace(uri, identity, resources) {
                return;
            }
            scans.merged()
        };
        self.publish_provider_resources(merged);
    }

    /// Whether the file at `uri` declares a registered service provider that a
    /// rebuild would now read it for.
    ///
    /// The class also has to resolve back to this file, so that a provider the
    /// rebuild would still not reach (its name indexed against another file)
    /// cannot make every keystroke rebuild the whole table.
    fn declares_registered_provider(&self, uri: &str) -> bool {
        let candidates: Vec<String> = {
            let scans = self.laravel_provider_scans.read();
            let classes = self.symbols.uri_classes_index.read();
            let Some(classes) = classes.get(uri) else {
                return false;
            };
            classes
                .iter()
                .map(|class| class.fqn())
                .filter(|fqn| scans.is_registered(fqn))
                .map(|fqn| fqn.to_string())
                .collect()
        };
        candidates
            .iter()
            .any(|fqn| self.resolve_class_uri(fqn).as_deref() == Some(uri))
    }

    /// The provider classes `fqn` extends, nearest first.
    ///
    /// Subclassing a provider and re-binding one of its keys is how a
    /// replacement is written, and the two providers can be registered in
    /// either order, so the subclass has to be recognizable as the later
    /// registration regardless of which one is scanned first.
    fn provider_ancestors(&self, fqn: &str) -> Vec<String> {
        let mut ancestors: Vec<String> = Vec::new();
        let mut current = self.find_or_load_class(fqn);
        while let Some(class) = current {
            let Some(parent) = class.parent_class.as_ref() else {
                break;
            };
            let parent = parent.trim_start_matches('\\').to_string();
            if ancestors.contains(&parent) {
                break;
            }
            current = self.find_or_load_class(&parent);
            ancestors.push(parent);
        }
        ancestors
    }

    /// The constants and static-property defaults a provider's own source
    /// folds `self::` and `static::` against.
    ///
    /// Merged over the parent chain, because a package commonly declares the
    /// container key on a base provider (`public static $abstract = 'sentry';`)
    /// and binds `static::$abstract` from the subclass the application
    /// registers.  Only the base resolution is used: the virtual member
    /// providers add nothing a declared constant is read from, and one of them
    /// is the very table being built here.
    fn provider_class_context(&self, fqn: &str) -> crate::virtual_members::laravel::ClassContext {
        let Some(class) = self.find_or_load_class(fqn) else {
            return Default::default();
        };
        let loader = |name: &str| self.find_or_load_class(name);
        let merged = crate::inheritance::resolve_class_with_inheritance(&class, &loader);
        crate::virtual_members::laravel::ClassContext::from_class(&merged)
    }
}
