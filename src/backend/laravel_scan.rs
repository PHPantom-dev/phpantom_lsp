//! Laravel self-scan: the index-building passes that read the project's
//! own source (service providers, macro registrations, morph maps, gates,
//! console commands, published resources, migrations) and the incremental
//! refreshes that re-run one of them when a file the pass depends on
//! changes.
//!
//! The indexes these passes fill are consumed by
//! [`crate::virtual_members::laravel`]; this module is only the scanner.

use std::collections::HashMap;
use std::path::PathBuf;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::type_engine::resolver::CtxLoaders;

impl Backend {
    /// Build the Laravel macro index by scanning the project's own source
    /// service providers, plus one level of classes they import, for
    /// `Target::macro('name', closure)` registrations.
    ///
    /// Vendor macros are recovered from the service providers packages register
    /// (via `extra.laravel.providers` in `installed.json`) plus any providers
    /// the app registers in `bootstrap/providers.php` / `config/app.php`,
    /// rather than re-reading the whole vendor tree. Project macros follow the
    /// same provider-rooted shape: each provider file is scanned directly and
    /// each imported class is scanned as a one-level helper candidate. Called
    /// once after indexing for Laravel projects. Files are byte-prefiltered for
    /// `macro(` so only candidates are parsed.
    ///
    /// `Storage::extend('driver', closure)` registrations are collected in the
    /// same pass: they live in exactly these files, and reading each one twice
    /// to build two indexes would double the scan for no gain.
    pub(crate) fn build_laravel_macro_index(&self) {
        let php_version = Some(*self.workspace.php_version.lock());

        let mut index = crate::virtual_members::laravel::LaravelMacroIndex::default();
        let mut drivers = crate::virtual_members::laravel::LaravelStorageDriverIndex::default();
        let mut candidate_uris: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut provider_uris: Vec<String> = Vec::new();
        let mut imported_uris: Vec<String> = Vec::new();
        // Seed URI → the class references it contributed to this build.
        // `refresh_laravel_macros` compares an edited seed's references
        // against this snapshot and only rebuilds when they changed.
        let mut seeds: HashMap<String, Vec<String>> = HashMap::new();

        // The app's provider registration files are seeds too: adding a
        // provider there must trigger a rebuild.  Their reference
        // fingerprint is the provider class list itself.
        if let Some(root) = self.workspace.workspace_root.read().clone() {
            for rel in ["bootstrap/providers.php", "config/app.php"] {
                let path = root.join(rel);
                let uri = crate::util::path_to_uri(&path);
                let refs = self
                    .get_file_content(&uri)
                    .or_else(|| std::fs::read_to_string(&path).ok())
                    .map(|c| crate::virtual_members::laravel::parse_provider_class_list(&c))
                    .unwrap_or_default();
                seeds.insert(uri, refs);
            }
        }

        // Every mixin-class file the scan pulled macros from, so an edit to one
        // triggers a rebuild even though it holds no `macro(`/`mixin(` token.
        let mut mixin_uris: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Scan a single file's content into the index, keyed by its URI.
        let scan_content = |index: &mut crate::virtual_members::laravel::LaravelMacroIndex,
                            mixin_uris: &mut std::collections::HashSet<String>,
                            uri: String,
                            content: &str| {
            let has_macro = memchr::memmem::find(content.as_bytes(), b"macro(").is_some();
            let has_mixin = memchr::memmem::find(content.as_bytes(), b"mixin(").is_some();
            if !has_macro && !has_mixin {
                return;
            }
            let mut regs = if has_macro {
                crate::virtual_members::laravel::extract_macro_registrations(content, php_version)
            } else {
                Vec::new()
            };
            if has_mixin {
                for reg in self.synthesize_mixin_registrations(content, php_version) {
                    if let Some(mixin_uri) = &reg.definition_uri {
                        mixin_uris.insert(mixin_uri.clone());
                    }
                    regs.push(reg);
                }
            }
            if regs.is_empty() {
                return;
            }
            self.infer_laravel_macro_return_types(&mut regs, &uri, content);
            // A macro registered through a facade also attaches to the
            // facade's concrete container-bound class.
            self.expand_facade_macros(&mut regs);
            index.set_file(uri, regs);
        };

        // Scan a single file's `Storage::extend()` registrations into the
        // driver index, keyed by its URI.
        let scan_storage_drivers =
            |drivers: &mut crate::virtual_members::laravel::LaravelStorageDriverIndex,
             uri: &str,
             content: &str| {
                let mut regs =
                    crate::virtual_members::laravel::extract_storage_driver_registrations(content);
                if regs.is_empty() {
                    return;
                }
                self.infer_storage_driver_return_types(&mut regs, uri, content);
                drivers.set_file(uri.to_string(), regs);
            };

        // Vendor- and app-registered service providers seed macro discovery.
        for fqn in self.laravel_provider_fqns() {
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            if candidate_uris.insert(uri.clone()) {
                provider_uris.push(uri);
            }
        }

        for uri in &provider_uris {
            let Some(content) = self.get_file_content(uri) else {
                seeds.insert(uri.clone(), Vec::new());
                continue;
            };
            scan_content(&mut index, &mut mixin_uris, uri.clone(), &content);
            scan_storage_drivers(&mut drivers, uri, &content);

            let referenced =
                crate::virtual_members::laravel::parse_provider_referenced_classes(&content);
            for imported_fqn in &referenced {
                let Some(imported_uri) = self.resolve_class_uri(imported_fqn) else {
                    continue;
                };
                if !self.is_macro_helper_uri_allowed(uri, &imported_uri) {
                    continue;
                }
                if candidate_uris.insert(imported_uri.clone()) {
                    imported_uris.push(imported_uri);
                }
            }
            seeds.insert(uri.clone(), referenced);
        }

        for uri in &imported_uris {
            let Some(content) = self.get_file_content(uri) else {
                continue;
            };
            scan_content(&mut index, &mut mixin_uris, uri.clone(), &content);
            scan_storage_drivers(&mut drivers, uri, &content);
        }

        drivers.rebuild();
        self.store_laravel_storage_drivers(drivers);

        index.rebuild();
        let has_macros = !index.is_empty();
        let new_targets = index.target_fqns();
        let target_count = new_targets.len();
        let old_targets = self.laravel_macros.read().target_fqns();
        *self.laravel_macros.write() = index;
        self.laravel_has_macros
            .store(has_macros, std::sync::atomic::Ordering::Relaxed);
        *self.laravel_macro_seeds.write() = seeds;
        *self.laravel_macro_mixin_uris.write() = mixin_uris;

        // Evict every class that had macros before or has them now, so a
        // rebuild triggered by a provider edit replaces stale cached merges
        // (both for added and for removed macros).
        {
            let mut cache = self.resolved_class_cache.write();
            for fqn in old_targets.iter().chain(new_targets.iter()) {
                crate::virtual_members::evict_fqn(&mut cache, fqn);
            }
        }

        tracing::info!(
            "PHPantom: scanned {} Laravel macro candidates ({} providers, {} imported classes), indexed {} macro targets",
            candidate_uris.len(),
            provider_uris.len(),
            imported_uris.len(),
            target_count,
        );
    }

    /// Scan project and vendor Artisan command classes and build the
    /// [`laravel_commands`](Backend::laravel_commands) index.
    ///
    /// Candidate files are those declaring a class whose short name ends in
    /// `Command` (the near-universal Laravel/Symfony convention), those living
    /// under a `Console/`, `Commands/` or `Command/` directory (so commands
    /// with unconventional names are still found), and every other non-vendor
    /// project class.  That last group is what makes `withCommands()` work:
    /// `bootstrap/app.php` can register a command directory anywhere (say
    /// `app/Actions/Sync`), and the registration is not statically recoverable
    /// in general, so project classes are all offered as candidates rather
    /// than guessed at.  Vendor classes keep the narrow filter, which is where
    /// the bulk of the classmap lives.
    ///
    /// Each candidate is read once, gated by a cheap byte pre-filter for a
    /// `signature`/`AsCommand`/`$name` declaration before parsing, then
    /// scanned by
    /// [`scan_command_file`](crate::virtual_members::laravel::scan_command_file),
    /// whose extends-`Command` / attribute checks decide whether the file
    /// really declares a command.
    pub(crate) fn build_laravel_command_index(&self) {
        let mut candidate_uris: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        {
            let idx = self.symbols.fqn_uri_index.read();
            for (fqn, uri) in idx.iter() {
                let short = fqn.rsplit('\\').next().unwrap_or(fqn);
                if !uri.contains("/vendor/")
                    || short.ends_with("Command")
                    || crate::virtual_members::laravel::is_command_directory_uri(uri)
                {
                    candidate_uris.insert(uri.to_string());
                }
            }
        }

        let mut index = crate::virtual_members::laravel::LaravelCommandIndex::default();
        for uri in &candidate_uris {
            let Some(content) = self.get_file_content(uri) else {
                continue;
            };
            let bytes = content.as_bytes();
            // `ignature` matches both the `$signature` property and the
            // `#[Signature]` attribute.
            let looks_like_command = memchr::memmem::find(bytes, b"ignature").is_some()
                || memchr::memmem::find(bytes, b"AsCommand").is_some()
                || memchr::memmem::find(bytes, b"$name").is_some();
            if !looks_like_command {
                continue;
            }
            let entries = crate::virtual_members::laravel::scan_command_file(&content, uri);
            index.set_file(uri.clone(), entries);
        }
        index.rebuild();

        let has_commands = !index.is_empty();
        let count = index.all_names().len();
        *self.laravel_commands.write() = index;
        self.laravel_has_commands
            .store(has_commands, std::sync::atomic::Ordering::Relaxed);

        tracing::info!(
            "PHPantom: scanned {} Laravel command candidates, indexed {} commands",
            candidate_uris.len(),
            count,
        );
    }

    /// Build the Eloquent morph-map index by scanning the project's registered
    /// service providers for `Relation::morphMap()` /
    /// `Relation::enforceMorphMap()` calls.
    ///
    /// Uses the same provider set as the macro scan (vendor packages'
    /// auto-discovered providers plus those the app lists in
    /// `bootstrap/providers.php` / `config/app.php`), since a morph map is
    /// registered from a provider's `boot()`.  Files are byte-prefiltered for
    /// the `orphMap(` token so only candidates are parsed.
    pub(crate) fn build_laravel_morph_map_index(&self) {
        let mut index = crate::virtual_members::laravel::LaravelMorphMapIndex::default();
        let mut scanned = 0usize;

        for fqn in self.laravel_provider_fqns() {
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            if index.has_uri(&uri) {
                continue;
            }
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            scanned += 1;
            let mut scan = crate::virtual_members::laravel::scan_morph_map(&content);
            if scan.is_empty() {
                continue;
            }
            self.resolve_morph_map_table_aliases(&mut scan);
            index.set_file(uri, scan);
        }

        index.rebuild();
        let alias_count = index.all_aliases().len();
        *self.laravel_morph_map.write() = index;

        tracing::info!(
            "PHPantom: scanned {} Laravel provider files, indexed {} morph aliases",
            scanned,
            alias_count,
        );
    }

    /// Turn a `Relation::morphMap([Post::class, …])` list registration into
    /// `alias => model` entries by resolving each model's table name, which is
    /// the alias Laravel derives for it.
    ///
    /// A model whose table cannot be determined statically (it overrides
    /// `getTable()`) is dropped rather than guessed, so no wrong alias enters
    /// the index.
    fn resolve_morph_map_table_aliases(
        &self,
        scan: &mut crate::virtual_members::laravel::MorphMapScan,
    ) {
        for target in std::mem::take(&mut scan.table_keyed) {
            let Some(class) = self.find_or_load_class(&target.target_fqn) else {
                continue;
            };
            let Some(table) = crate::virtual_members::laravel::model_table_name(&class) else {
                continue;
            };
            scan.entries
                .push(crate::virtual_members::laravel::MorphMapEntry {
                    alias: table,
                    target_fqn: target.target_fqn,
                    alias_offset: target.offset,
                });
        }
    }

    /// Build the authorization gate index by scanning the project's
    /// registered service providers for `Gate::define()` / `Gate::policy()`
    /// calls and `$policies` arrays.
    ///
    /// Uses the same provider set as the macro and morph-map scans (vendor
    /// packages' auto-discovered providers plus those the app lists in
    /// `bootstrap/providers.php` / `config/app.php`), since abilities and the
    /// policy map are registered from a provider's `boot()`.  Files are
    /// byte-prefiltered inside
    /// [`scan_gate_registrations`](crate::virtual_members::laravel::scan_gate_registrations)
    /// so only candidates are parsed.
    pub(crate) fn build_laravel_gate_index(&self) {
        let mut index = crate::virtual_members::laravel::LaravelGateIndex::default();
        // Read from `composer.json` during init and not recoverable from the
        // provider scan below, so it has to survive the fresh index.
        index
            .set_runtime_permission_package(self.laravel_gates.read().runtime_permission_package());
        let mut scanned = 0usize;

        for fqn in self.laravel_provider_fqns() {
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            if index.has_uri(&uri) {
                continue;
            }
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            scanned += 1;
            let scan = crate::virtual_members::laravel::scan_gate_registrations(&content);
            if scan.is_empty() {
                continue;
            }
            index.set_file(uri, scan);
        }

        index.rebuild();
        let ability_count = index.definition_names().len();
        *self.laravel_gates.write() = index;
        self.laravel_string_key_cache.write().gate_abilities = None;

        tracing::info!(
            "PHPantom: scanned {} Laravel provider files, indexed {} gate abilities",
            scanned,
            ability_count,
        );
    }

    /// Re-scan a single file's gate registrations after an edit.
    ///
    /// A cheap no-op unless the file currently contributes registrations or
    /// its new content mentions `Gate` or a `$policies` property.  Only runs
    /// for Laravel projects.
    pub(crate) fn refresh_laravel_gates(&self, uri: &str, content: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let was_contributor = self.laravel_gates.read().has_uri(uri);
        let bytes = content.as_bytes();
        let has_token = memchr::memmem::find(bytes, b"Gate").is_some()
            || memchr::memmem::find(bytes, b"$policies").is_some();
        if !was_contributor && !has_token {
            return;
        }

        let scan = crate::virtual_members::laravel::scan_gate_registrations(content);
        if !was_contributor && scan.is_empty() {
            return;
        }

        let mut index = self.laravel_gates.write();
        index.set_file(uri.to_string(), scan);
        index.rebuild();
    }

    /// Re-scan a single file's morph-map registrations after an edit.
    ///
    /// A cheap no-op unless the file currently contributes registrations or its
    /// new content contains a `morphMap(` call.  Only runs for Laravel projects.
    pub(crate) fn refresh_laravel_morph_map(&self, uri: &str, content: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let was_contributor = self.laravel_morph_map.read().has_uri(uri);
        let has_token = memchr::memmem::find(content.as_bytes(), b"orphMap(").is_some();
        if !was_contributor && !has_token {
            return;
        }

        let mut scan = crate::virtual_members::laravel::scan_morph_map(content);
        self.resolve_morph_map_table_aliases(&mut scan);

        let mut index = self.laravel_morph_map.write();
        index.set_file(uri.to_string(), scan);
        index.rebuild();
    }

    /// Refresh the command index after a single file edit.
    ///
    /// Cheap: re-scans only the edited file when it is a command candidate
    /// (or was contributing before), replacing just that file's entries.
    pub(crate) fn refresh_laravel_command_index(&self, uri: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let was_contributor = self.laravel_commands.read().has_uri(uri);
        // Same candidate rule as the full build: a contributor file, a
        // conventionally-named command file, or any non-vendor project file
        // (commands registered via `withCommands()` may live anywhere).
        let looks_like_command_file = uri.ends_with("Command.php")
            || crate::virtual_members::laravel::is_command_directory_uri(uri)
            || (!uri.contains("/vendor/") && uri.ends_with(".php"));
        if !was_contributor && !looks_like_command_file {
            return;
        }

        let entries = self
            .get_file_content(uri)
            .filter(|content| {
                let bytes = content.as_bytes();
                // `ignature` matches both the `$signature` property and the
                // `#[Signature]` attribute.
                memchr::memmem::find(bytes, b"ignature").is_some()
                    || memchr::memmem::find(bytes, b"AsCommand").is_some()
                    || memchr::memmem::find(bytes, b"$name").is_some()
            })
            .map(|content| crate::virtual_members::laravel::scan_command_file(&content, uri))
            .unwrap_or_default();

        if !was_contributor && entries.is_empty() {
            return;
        }

        let mut index = self.laravel_commands.write();
        index.set_file(uri.to_string(), entries);
        index.rebuild();
        let has_commands = !index.is_empty();
        drop(index);
        self.laravel_has_commands
            .store(has_commands, std::sync::atomic::Ordering::Relaxed);
    }

    /// Find the date class selected by project service providers. Laravel's
    /// helpers use this factory, so `now()` and `today()` return this class
    /// rather than their broad `CarbonInterface` declaration.
    ///
    /// Runs in both the LSP `initialized` handler and the headless `analyze`
    /// pipeline so every consumer resolves the date helpers to a concrete
    /// class. Until this has run, `laravel_date_class` stays `None` and the
    /// helpers resolve to nothing rather than a stale default.
    pub(crate) fn build_laravel_date_class(&self) {
        let mut configured = None;
        // Track every file this scan reads so the single-file refresh can tell
        // whether an edit could change the configured class.  The app's
        // provider-registration files are seeds too: editing them changes which
        // providers are registered, so a `Date::use()` in a newly added (or
        // removed) provider is picked up on the next scan.
        let mut seed_uris: std::collections::HashSet<String> = std::collections::HashSet::new();
        if let Some(root) = self.workspace.workspace_root.read().clone() {
            for rel in ["bootstrap/providers.php", "config/app.php"] {
                seed_uris.insert(crate::util::path_to_uri(&root.join(rel)));
            }
        }
        let providers = self.laravel_provider_fqns();
        for fqn in providers {
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            let Ok(url) = tower_lsp::lsp_types::Url::parse(&uri) else {
                continue;
            };
            let Ok(path) = url.to_file_path() else {
                continue;
            };
            if self.is_in_vendor_dir(&path) {
                continue;
            }
            seed_uris.insert(uri.clone());
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            if let Some(class) =
                crate::virtual_members::laravel::extract_date_factory_class(&content)
            {
                configured = Some(class);
            }
        }
        *self.laravel_date_seed_uris.write() = seed_uris;
        *self.laravel_date_class.write() = Some(configured);
    }

    /// Collect the FQNs of every Laravel service provider that could register a
    /// macro: those installed vendor packages auto-discover (via
    /// `extra.laravel.providers` in each vendor's `installed.json`) plus those
    /// the app lists in `bootstrap/providers.php` / `config/app.php`.
    fn laravel_provider_fqns(&self) -> Vec<String> {
        self.laravel_providers_with_origin()
            .into_iter()
            .map(|(fqn, _)| fqn)
            .collect()
    }

    /// The same providers, each tagged with how it was registered so a
    /// container key two of them bind can be settled the way the container
    /// settles it.
    ///
    /// A provider reached both ways keeps the origin it was first found under:
    /// the container registers it once, at the first point it is named.
    fn laravel_providers_with_origin(
        &self,
    ) -> Vec<(String, crate::virtual_members::laravel::ProviderOrigin)> {
        use crate::virtual_members::laravel::ProviderOrigin;

        let mut providers: Vec<(String, ProviderOrigin)> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut push =
            |providers: &mut Vec<(String, ProviderOrigin)>, fqn: String, origin: ProviderOrigin| {
                if seen.insert(fqn.clone()) {
                    providers.push((fqn, origin));
                }
            };

        for vendor_dir in self.workspace.vendor_dir_paths.lock().iter() {
            let installed = vendor_dir.join("composer").join("installed.json");
            if let Ok(content) = std::fs::read_to_string(&installed) {
                for fqn in crate::virtual_members::laravel::parse_installed_providers(&content) {
                    push(&mut providers, fqn, ProviderOrigin::Package);
                }
            }
        }

        if let Some(root) = self.workspace.workspace_root.read().clone() {
            for rel in ["bootstrap/providers.php", "config/app.php"] {
                let path = root.join(rel);
                let uri = crate::util::path_to_uri(&path);
                let content = self
                    .get_file_content(&uri)
                    .or_else(|| std::fs::read_to_string(&path).ok());
                if let Some(content) = content {
                    for fqn in crate::virtual_members::laravel::parse_provider_class_list(&content)
                    {
                        // The configured list is registered `Illuminate\*`
                        // first, then the auto-discovered packages, then the
                        // rest, which is the order
                        // `registerConfiguredProviders()` partitions it into.
                        let origin = if fqn.starts_with("Illuminate\\") {
                            ProviderOrigin::Framework
                        } else {
                            ProviderOrigin::Application
                        };
                        push(&mut providers, fqn, origin);
                    }
                }
            }
        }

        providers
    }

    /// Resolve a class FQN to the URI of the file that declares it, loading the
    /// class if it is not yet in the FQN → URI index.  Used to locate provider
    /// source files for the macro scan.
    pub(crate) fn resolve_class_uri(&self, fqn: &str) -> Option<String> {
        if let Some(uri) = self.symbols.fqn_uri_index.read().get(fqn).cloned() {
            return Some(uri);
        }
        // Not indexed yet: loading the class populates its FQN → URI entry.
        self.find_or_load_class(fqn);
        self.symbols.fqn_uri_index.read().get(fqn).cloned()
    }

    /// Expand every `Target::mixin(new X)` / `Target::mixin(X::class)`
    /// registration found in `content` into the concrete macros the mixin class
    /// `X` contributes.
    ///
    /// The mixin class's methods live in a different file than the `::mixin(…)`
    /// call, so this resolves `X` to its source (via the class index, preferring
    /// an open editor buffer over disk) and parses each qualifying method's
    /// returned closure.  Each resulting registration records the mixin file's
    /// URI as its go-to-definition target.  Returns an empty vector when the
    /// file registers no mixins or the mixin classes cannot be located.
    fn synthesize_mixin_registrations(
        &self,
        content: &str,
        php_version: Option<crate::types::PhpVersion>,
    ) -> Vec<crate::virtual_members::laravel::MacroRegistration> {
        let mixins = crate::virtual_members::laravel::extract_mixin_registrations(content);
        let mut out = Vec::new();
        for mixin in mixins {
            let Some(uri) = self.resolve_class_uri(&mixin.mixin_fqn) else {
                continue;
            };
            let Some(mixin_source) = self.get_file_content(&uri).or_else(|| {
                let path = tower_lsp::lsp_types::Url::parse(&uri)
                    .ok()?
                    .to_file_path()
                    .ok()?;
                std::fs::read_to_string(path).ok()
            }) else {
                continue;
            };
            out.extend(crate::virtual_members::laravel::synthesize_mixin_macros(
                &mixin_source,
                &mixin.mixin_fqn,
                &uri,
                &mixin.target,
                php_version,
            ));
        }
        out
    }

    /// Re-scan a single file's macro registrations after an edit, keeping the
    /// index and the resolved-class cache coherent.
    ///
    /// A cheap no-op unless the file currently contributes macros or its new
    /// content contains a `macro(` call.  Only runs for Laravel projects.
    pub(crate) fn refresh_laravel_macros(&self, uri: &str, content: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        // Re-run the full date-factory scan when the edited file is one that
        // could configure it: a registered service provider, or one of the
        // app's provider-registration files.  Scanning (rather than a one-way
        // set on any `::use` call) means adding, changing, or *removing* a
        // `Date::use()` / `DateFactory::use()` call is reflected, and an edit
        // to an unrelated file can neither override the configured class nor
        // leave a stale one behind.
        if self.laravel_date_seed_uris.read().contains(uri) {
            self.build_laravel_date_class();
        }
        // A `Macroable::mixin()` registration pulls its macros from another
        // file and records that file as a dependency.  Because those macros are
        // keyed under the registration site (not the mixin class) and the mixin
        // class carries no `macro(`/`mixin(` token of its own, the single-file
        // path below cannot keep them coherent.  So any edit that touches a
        // `mixin(` call site, or a file a mixin was read from, rebuilds the
        // whole index (which also refreshes the dependency set).  Mixin
        // registrations are rare, so the occasional full rebuild is cheap.
        if memchr::memmem::find(content.as_bytes(), b"mixin(").is_some()
            || self.laravel_macro_mixin_uris.read().contains(uri)
        {
            self.build_laravel_macro_index();
            return;
        }
        // An edit to a seed file (a service provider or the app's provider
        // registration files) that changes its class references alters which
        // files feed the index, so the index is rebuilt.  When the references
        // are unchanged the edit can only affect the seed's own
        // registrations, which the single-file path below picks up.
        let prev_refs = self.laravel_macro_seeds.read().get(uri).cloned();
        if let Some(prev_refs) = prev_refs {
            let refs = if self.is_laravel_provider_list_uri(uri) {
                crate::virtual_members::laravel::parse_provider_class_list(content)
            } else {
                crate::virtual_members::laravel::parse_provider_referenced_classes(content)
            };
            if refs != prev_refs {
                self.build_laravel_macro_index();
                return;
            }
        }
        let had = self.laravel_macros.read().has_uri(uri);
        let has_token = memchr::memmem::find(content.as_bytes(), b"macro(").is_some();
        if !had && !has_token {
            return;
        }

        let php_version = Some(*self.workspace.php_version.lock());
        let mut regs =
            crate::virtual_members::laravel::extract_macro_registrations(content, php_version);
        self.infer_laravel_macro_return_types(&mut regs, uri, content);
        // A macro registered through a facade also attaches to the facade's
        // concrete container-bound class.
        self.expand_facade_macros(&mut regs);

        let targets = {
            let mut index = self.laravel_macros.write();
            // Capture the pre-edit targets too, so a class whose last macro
            // this edit removed is also evicted below.
            let mut targets = index.target_fqns();
            index.set_file(uri.to_string(), regs);
            index.rebuild();
            self.laravel_has_macros
                .store(!index.is_empty(), std::sync::atomic::Ordering::Relaxed);
            targets.extend(index.target_fqns());
            targets
        };

        // Evict every class a macro attaches to so the next resolution picks
        // up the change instead of a stale cached merge.
        let mut cache = self.resolved_class_cache.write();
        for fqn in targets {
            crate::virtual_members::evict_fqn(&mut cache, &fqn);
        }
    }

    /// Whether `uri` is one of the app's provider registration files
    /// (`bootstrap/providers.php` / `config/app.php`), whose macro-relevant
    /// references are the provider class list rather than method-body class
    /// references.
    fn is_laravel_provider_list_uri(&self, uri: &str) -> bool {
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return false;
        };
        ["bootstrap/providers.php", "config/app.php"]
            .iter()
            .any(|rel| crate::util::path_to_uri(&root.join(rel)) == uri)
    }

    pub(crate) fn build_provider_resources(&self) {
        let mut scans = crate::virtual_members::laravel::ProviderScans::default();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (fqn, origin) in self.laravel_providers_with_origin() {
            scans.record_registered(&fqn);
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            if !seen.insert(uri.clone()) {
                continue;
            }
            let Some((identity, resources)) =
                self.scan_provider_resources(&uri, None, &fqn, origin)
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
            cache.routes = None;
            cache.blade_discovery = None;
        }

        // The provider bindings overlay the core container alias table, which
        // an earlier resolution may already have built without them.
        if has_bindings {
            *self.laravel_aliases.write() = None;
            self.clear_class_not_found_cache();
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
    pub(crate) fn refresh_laravel_provider_resources(&self, uri: &str, content: &str) {
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
            self.build_provider_resources();
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
                self.build_provider_resources();
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

    /// The constants and static-property defaults a provider's own source
    /// folds `self::` and `static::` against.
    ///
    /// Merged over the parent chain, because a package commonly declares the
    /// container key on a base provider (`public static $abstract = 'sentry';`)
    /// and binds `static::$abstract` from the subclass the application
    /// registers.  Only the base resolution is used: the virtual member
    /// providers add nothing a declared constant is read from, and one of them
    /// is the very table being built here.
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

    fn provider_class_context(&self, fqn: &str) -> crate::virtual_members::laravel::ClassContext {
        let Some(class) = self.find_or_load_class(fqn) else {
            return Default::default();
        };
        let loader = |name: &str| self.find_or_load_class(name);
        let merged = crate::inheritance::resolve_class_with_inheritance(&class, &loader);
        crate::virtual_members::laravel::ClassContext::from_class(&merged)
    }

    fn infer_laravel_macro_return_types(
        &self,
        regs: &mut [crate::virtual_members::laravel::MacroRegistration],
        uri: &str,
        content: &str,
    ) {
        let file_ctx = self.file_context(uri);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader = self.function_loader(&file_ctx);
        let laravel_macro_this_resolver = self.laravel_macro_this_resolver(&class_loader);
        for reg in regs.iter_mut() {
            if reg.method.return_type.is_some() || reg.method.native_return_type.is_some() {
                continue;
            }
            if reg.closure_text.is_none() {
                continue;
            }
            // A mixin-derived macro's closure lives in the mixin class file, not
            // in the registration site, and its `name_offset` points into that
            // file.  Resolving the closure body against the registration file's
            // content would use a mismatched offset and the wrong scope, so
            // route it through its own file context instead.
            if let Some(def_uri) = reg.definition_uri.clone() {
                self.infer_mixin_macro_return_type(reg, &def_uri);
                continue;
            }
            let closure_text = reg.closure_text.as_deref().unwrap_or_default();
            let Some(target_class) = self.find_or_load_class(&reg.target) else {
                continue;
            };
            let rctx = crate::type_engine::resolver::ResolutionCtx {
                preserve_static: true,
                ..self.resolution_ctx_at(
                    Some(target_class.as_ref()),
                    &file_ctx.classes,
                    content,
                    reg.name_offset,
                    CtxLoaders::new(
                        &class_loader,
                        &function_loader,
                        &laravel_macro_this_resolver,
                    ),
                )
            };
            if let Some(ty) = Self::infer_closure_return_type(closure_text, &rctx) {
                reg.method.return_type = Some(ty);
                reg.method.is_inferred_return = true;
            }
        }
    }

    /// Publish a freshly built storage-driver index, invalidating the memoized
    /// disk type when the set of custom drivers is (or was) non-empty.
    ///
    /// A build resolves classes as it goes, which can compute and cache the
    /// disk type against the previous index; dropping it here means the first
    /// read after the build sees the drivers this scan found.
    fn store_laravel_storage_drivers(
        &self,
        drivers: crate::virtual_members::laravel::LaravelStorageDriverIndex,
    ) {
        let stale = !self.laravel_storage_drivers.read().is_empty() || !drivers.is_empty();
        *self.laravel_storage_drivers.write() = drivers;
        if stale {
            self.invalidate_storage_disk_type();
        }
    }

    /// Drop the memoized filesystem disk type and evict the two classes it was
    /// baked into, so the next load of either recomputes it.
    fn invalidate_storage_disk_type(&self) {
        *self.storage_disk_type_cache.write() = None;
        let mut cache = self.resolved_class_cache.write();
        for fqn in [
            crate::virtual_members::laravel::FILESYSTEM_MANAGER_FQN,
            crate::virtual_members::laravel::STORAGE_FACADE_FQN,
        ] {
            crate::virtual_members::evict_fqn(&mut cache, fqn);
        }
    }

    /// Keep the storage-driver index and the memoized disk type coherent with
    /// an edit.
    ///
    /// Two kinds of edit matter: a `config/` file (which decides what the disks
    /// name, and whose parsed tree the string-key cache drops on the same
    /// condition), and a file that registers — or used to register — a
    /// `Storage::extend()` driver.  Every other file is a byte-prefiltered
    /// no-op.
    pub(crate) fn refresh_laravel_storage_drivers(&self, uri: &str, content: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let config_changed = uri.contains("/config/");
        let had = self.laravel_storage_drivers.read().has_uri(uri);
        let mut regs =
            crate::virtual_members::laravel::extract_storage_driver_registrations(content);
        if !config_changed && !had && regs.is_empty() {
            return;
        }
        if had || !regs.is_empty() {
            self.infer_storage_driver_return_types(&mut regs, uri, content);
            let mut index = self.laravel_storage_drivers.write();
            index.set_file(uri.to_string(), regs);
            index.rebuild();
        }
        self.invalidate_storage_disk_type();
    }

    /// Fill in the type each `Storage::extend()` closure builds when it does
    /// not annotate one.
    ///
    /// The documented registration shape ends in an unannotated `return new
    /// FilesystemAdapter(...)`, so the body is the ordinary source of a custom
    /// driver's type rather than a fallback for sloppy code.
    fn infer_storage_driver_return_types(
        &self,
        regs: &mut [crate::virtual_members::laravel::StorageDriverRegistration],
        uri: &str,
        content: &str,
    ) {
        if regs.iter().all(|reg| reg.return_type.is_some()) {
            return;
        }
        let file_ctx = self.file_context(uri);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader = self.function_loader(&file_ctx);
        for reg in regs.iter_mut() {
            if reg.return_type.is_some() {
                continue;
            }
            let Some(closure_text) = reg.closure_text.as_deref() else {
                continue;
            };
            let rctx = self.resolution_ctx_at(
                crate::diagnostics::helpers::find_innermost_enclosing_class(
                    &file_ctx.classes,
                    reg.closure_offset,
                ),
                &file_ctx.classes,
                content,
                reg.closure_offset,
                CtxLoaders::without_macro_this(&class_loader, &function_loader),
            );
            reg.return_type = Self::infer_closure_return_type(closure_text, &rctx);
        }
    }

    /// Infer a mixin-derived macro's return type from the closure its mixin
    /// method returns, resolving against the mixin class file (where the closure
    /// actually lives) rather than the `::mixin(...)` registration site.
    ///
    /// A no-op when the mixin file cannot be read or the target class cannot be
    /// resolved.  `reg.name_offset` is an offset into `def_uri`'s content, so
    /// the file context and content must both come from that file.
    fn infer_mixin_macro_return_type(
        &self,
        reg: &mut crate::virtual_members::laravel::MacroRegistration,
        def_uri: &str,
    ) {
        let Some(content) = self.get_file_content(def_uri).or_else(|| {
            let path = tower_lsp::lsp_types::Url::parse(def_uri)
                .ok()?
                .to_file_path()
                .ok()?;
            std::fs::read_to_string(path).ok()
        }) else {
            return;
        };
        let Some(closure_text) = reg.closure_text.clone() else {
            return;
        };
        let Some(target_class) = self.find_or_load_class(&reg.target) else {
            return;
        };
        let file_ctx = self.file_context(def_uri);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader = self.function_loader(&file_ctx);
        let laravel_macro_this_resolver = self.laravel_macro_this_resolver(&class_loader);
        let rctx = crate::type_engine::resolver::ResolutionCtx {
            preserve_static: true,
            ..self.resolution_ctx_at(
                Some(target_class.as_ref()),
                &file_ctx.classes,
                &content,
                reg.name_offset,
                CtxLoaders::new(
                    &class_loader,
                    &function_loader,
                    &laravel_macro_this_resolver,
                ),
            )
        };
        if let Some(ty) = Self::infer_closure_return_type(&closure_text, &rctx) {
            reg.method.return_type = Some(ty);
            reg.method.is_inferred_return = true;
        }
    }

    fn is_macro_helper_uri_allowed(&self, provider_uri: &str, helper_uri: &str) -> bool {
        let Ok(provider_url) = tower_lsp::lsp_types::Url::parse(provider_uri) else {
            return false;
        };
        let Ok(helper_url) = tower_lsp::lsp_types::Url::parse(helper_uri) else {
            return false;
        };
        let Ok(provider_path) = provider_url.to_file_path() else {
            return false;
        };
        let Ok(helper_path) = helper_url.to_file_path() else {
            return false;
        };

        // Vendor providers may live under the workspace root, so classify
        // package-local vendor helpers before the broader app-root check.
        if let Some(root) = self.vendor_package_root(&provider_path) {
            return helper_path.starts_with(&root);
        }

        if let Some(root) = self.workspace.workspace_root.read().clone()
            && provider_path.starts_with(&root)
        {
            return helper_path.starts_with(&root) && !self.is_in_vendor_dir(&helper_path);
        }

        false
    }

    fn vendor_package_root(&self, path: &std::path::Path) -> Option<std::path::PathBuf> {
        for vendor_dir in self.workspace.vendor_dir_paths.lock().iter() {
            if let Ok(rel) = path.strip_prefix(vendor_dir)
                && let mut comps = rel.components()
                && let (Some(vendor), Some(package)) = (comps.next(), comps.next())
            {
                return Some(vendor_dir.join(vendor).join(package));
            }
        }
        None
    }

    fn is_in_vendor_dir(&self, path: &std::path::Path) -> bool {
        self.workspace
            .vendor_dir_paths
            .lock()
            .iter()
            .any(|vendor_dir| path.starts_with(vendor_dir))
    }

    pub(crate) fn reload_laravel_schema_index(&self, root: &std::path::Path) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }

        let laravel_config = self.config().laravel;
        let index = if laravel_config.schema.enabled() || laravel_config.migrations.enabled() {
            let bp_macros = self.laravel_macros.read().blueprint_macro_closures();
            match crate::virtual_members::laravel::database_schema::load_schema_index(
                root,
                &laravel_config,
                &bp_macros,
            ) {
                Ok(index) => index,
                Err(err) => {
                    tracing::warn!("Failed to reload Laravel schema dumps: {}", err);
                    return;
                }
            }
        } else {
            crate::virtual_members::laravel::database_schema::SchemaIndex::default()
        };

        self.resolved_class_cache
            .write()
            .set_schema_index(index.clone());
        *self.schema_index.write() = index;
        self.resolved_class_cache.write().clear();
        self.member_completion_cache.lock().clear();
    }

    pub(crate) fn update_laravel_migrations(&self, changes: &[(PathBuf, FileChangeType)]) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }

        let mut index = self.schema_index.write();
        let mut any_changed = false;
        for (path, change_type) in changes {
            if *change_type == FileChangeType::DELETED {
                if index.remove_migration_file(path) {
                    any_changed = true;
                }
            } else {
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        index.update_migration_file(path, content);
                        any_changed = true;
                    }
                    Err(err) => {
                        tracing::warn!("Failed to read migration file {}: {}", path.display(), err);
                    }
                }
            }
        }
        if any_changed {
            self.resolved_class_cache
                .write()
                .set_schema_index(index.clone());
            self.resolved_class_cache.write().clear();
            self.member_completion_cache.lock().clear();
        }
    }
}
