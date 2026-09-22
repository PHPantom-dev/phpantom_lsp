//! The macro index: `Target::macro()` and `Target::mixin()` registrations
//! found in the registered service providers and the classes they import.

use std::collections::HashMap;

use crate::Backend;
use crate::type_engine::resolver::CtxLoaders;
use crate::types::FileContext;

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
            index.files.set_file(uri, regs);
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
                drivers.files.set_file(uri.to_string(), regs);
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
        // A macro is attached without a class lookup naming the provider that
        // registered it, so a cached receiver resolution records no
        // dependency on this table.
        self.clear_resolved_member_files();

        tracing::info!(
            "PHPantom: scanned {} Laravel macro candidates ({} providers, {} imported classes), indexed {} macro targets",
            candidate_uris.len(),
            provider_uris.len(),
            imported_uris.len(),
            target_count,
        );
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
        let had = self.laravel_macros.read().files.has_uri(uri);
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
            index.files.set_file(uri.to_string(), regs);
            index.rebuild();
            self.laravel_has_macros
                .store(!index.is_empty(), std::sync::atomic::Ordering::Relaxed);
            targets.extend(index.target_fqns());
            targets
        };

        // Evict every class a macro attaches to so the next resolution picks
        // up the change instead of a stale cached merge.
        {
            let mut cache = self.resolved_class_cache.write();
            for fqn in targets {
                crate::virtual_members::evict_fqn(&mut cache, &fqn);
            }
        }
        // See the rebuild path above: the receiver layer cannot name this
        // table as a dependency, so it goes wholesale.
        self.clear_resolved_member_files();
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
            self.infer_macro_return_type(
                reg,
                content,
                &file_ctx,
                CtxLoaders::new(
                    &class_loader,
                    &function_loader,
                    &laravel_macro_this_resolver,
                ),
            );
        }
    }

    /// Infer a mixin-derived macro's return type from the closure its mixin
    /// method returns, resolving against the mixin class file (where the closure
    /// actually lives) rather than the `::mixin(...)` registration site.
    ///
    /// A no-op when the mixin file cannot be read.  `reg.name_offset` is an
    /// offset into `def_uri`'s content, so the file context and content must
    /// both come from that file.
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
        let file_ctx = self.file_context(def_uri);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader = self.function_loader(&file_ctx);
        let laravel_macro_this_resolver = self.laravel_macro_this_resolver(&class_loader);
        self.infer_macro_return_type(
            reg,
            &content,
            &file_ctx,
            CtxLoaders::new(
                &class_loader,
                &function_loader,
                &laravel_macro_this_resolver,
            ),
        );
    }

    /// Type `reg`'s macro from the closure it registers, read as the body of
    /// a method on the target class.  `content` is the file the closure is
    /// written in, and `file_ctx` and `loaders` describe that same file.
    ///
    /// A no-op when the registration has no closure or the target class
    /// cannot be resolved.
    fn infer_macro_return_type(
        &self,
        reg: &mut crate::virtual_members::laravel::MacroRegistration,
        content: &str,
        file_ctx: &FileContext,
        loaders: CtxLoaders<'_>,
    ) {
        let Some(closure_text) = reg.closure_text.as_deref() else {
            return;
        };
        let Some(target_class) = self.find_or_load_class(&reg.target) else {
            return;
        };
        let rctx = crate::type_engine::resolver::ResolutionCtx {
            preserve_static: true,
            ..self.resolution_ctx_at(
                Some(target_class.as_ref()),
                &file_ctx.classes,
                content,
                reg.name_offset,
                loaders,
            )
        };
        if let Some(ty) = Self::infer_closure_return_type(closure_text, &rctx) {
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
}
