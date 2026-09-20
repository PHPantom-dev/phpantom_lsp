//! The Artisan command index: every class declaring a console command,
//! found by name convention, directory, or a `$signature` declaration.

use crate::Backend;

impl Backend {
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
            index.files.set_file(uri.clone(), entries);
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

    /// Refresh the command index after a single file edit.
    ///
    /// Cheap: re-scans only the edited file when it is a command candidate
    /// (or was contributing before), replacing just that file's entries.
    pub(crate) fn refresh_laravel_command_index(&self, uri: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let was_contributor = self.laravel_commands.read().files.has_uri(uri);
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
        index.files.set_file(uri.to_string(), entries);
        index.rebuild();
        let has_commands = !index.is_empty();
        drop(index);
        self.laravel_has_commands
            .store(has_commands, std::sync::atomic::Ordering::Relaxed);
    }
}
