//! The storage-driver index: `Storage::extend()` registrations, collected
//! by the macro scan from the same provider files, and the memoized disk
//! type they decide.

use crate::Backend;
use crate::type_engine::resolver::CtxLoaders;
use crate::virtual_members::laravel::file_contributions::refresh_file;

impl Backend {
    /// Publish a freshly built storage-driver index, invalidating the memoized
    /// disk type when the set of custom drivers is (or was) non-empty.
    ///
    /// A build resolves classes as it goes, which can compute and cache the
    /// disk type against the previous index; dropping it here means the first
    /// read after the build sees the drivers this scan found.
    pub(super) fn store_laravel_storage_drivers(
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
        let touched = refresh_file(&self.laravel_storage_drivers, uri, true, || {
            let mut regs =
                crate::virtual_members::laravel::extract_storage_driver_registrations(content);
            self.infer_storage_driver_return_types(&mut regs, uri, content);
            regs
        });
        if config_changed || touched {
            self.invalidate_storage_disk_type();
        }
    }

    /// Fill in the type each `Storage::extend()` closure builds when it does
    /// not annotate one.
    ///
    /// The documented registration shape ends in an unannotated `return new
    /// FilesystemAdapter(...)`, so the body is the ordinary source of a custom
    /// driver's type rather than a fallback for sloppy code.
    pub(super) fn infer_storage_driver_return_types(
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
}
