//! Per-file content and context accessors on [`Backend`].
//!
//! These read (and, for `clear_file_maps`, clear) the per-URI maps that
//! back every feature's "what do we know about this file?" query:
//! open-file content, the class index, the import table, and the
//! namespace map. Centralising them here removes the repeated
//! lock-and-unwrap boilerplate that used to be duplicated across the
//! completion handler, definition resolver, and other consumers.

use std::sync::Arc;

use tower_lsp::lsp_types::Url;

use crate::Backend;
use crate::types::{ClassInfo, FileContext};

impl Backend {
    /// Look up a class by its (possibly namespace-qualified) name in the
    /// in-memory `uri_classes_index`, without triggering any disk I/O.
    ///
    /// The `class_name` can be:
    ///   - A simple name like `"Customer"`
    ///   - A namespace-qualified name like `"Klarna\\Customer"`
    ///   - A fully-qualified name like `"\\Klarna\\Customer"` (leading `\` is stripped)
    ///
    /// When a namespace prefix is present, the file's namespace (from
    /// `namespace_map`) must match for the class to be returned.  This
    /// prevents `"Demo\\PDO"` from matching the global `PDO` stub.
    ///
    /// Returns a shared `Arc<ClassInfo>` if found, or `None`.
    pub(crate) fn find_class_in_uri_classes_index(
        &self,
        class_name: &str,
    ) -> Option<Arc<ClassInfo>> {
        // ── Fast path: O(1) lookup via fqn_index ──
        // For namespace-qualified names the FQN is the normalized name
        // itself.  For bare names (no backslash) the FQN equals the
        // short name, which is also stored in the index.
        if let Some(cls) = self.fqn_class_index.read().get(class_name) {
            return Some(Arc::clone(cls));
        }

        // The fqn_class_index is always populated before (or at the
        // same time as) uri_classes_index, so if the O(1) lookup above
        // missed, a linear scan would not find it either.
        None
    }

    /// Get the content of a file by URI, trying open files first then disk.
    ///
    /// This replaces the repeated pattern of locking `open_files`, looking
    /// up the URI, and falling back to reading from disk via
    /// `Url::to_file_path` + `std::fs::read_to_string`.  Three call sites
    /// in the definition modules used this exact sequence.
    pub(crate) fn get_file_content(&self, uri: &str) -> Option<String> {
        if let Some(content) = self.open_files.read().get(uri) {
            return Some(String::clone(content));
        }

        // Embedded class stubs live under synthetic `phpantom-stub://`
        // URIs and have no on-disk file.  Retrieve the raw source from
        // the stub_index keyed by the class short name (the URI path).
        if let Some(class_name) = uri.strip_prefix("phpantom-stub://") {
            let stub_idx = self.stub_index.read();
            return stub_idx.get(class_name).map(|s| s.to_string());
        }

        // Embedded function stubs use `phpantom-stub-fn://` URIs.
        // The path component is the function name used as key in
        // stub_function_index.
        if let Some(func_name) = uri.strip_prefix("phpantom-stub-fn://") {
            let stub_fn_idx = self.stub_function_index.read();
            return stub_fn_idx.get(func_name).map(|s| s.to_string());
        }

        let path = Url::parse(uri).ok()?.to_file_path().ok()?;
        std::fs::read_to_string(path).ok()
    }

    /// Retrieve file content as a cheap `Arc<String>` reference when the
    /// file is in `open_files`.  Falls back to reading from disk (which
    /// wraps the result in a new `Arc`).
    ///
    /// Prefer this over [`get_file_content`] in hot paths where the
    /// content will be shared across tasks or stored for the duration
    /// of a request, since it avoids deep-cloning the file string.
    pub(crate) fn get_file_content_arc(&self, uri: &str) -> Option<Arc<String>> {
        if let Some(content) = self.open_files.read().get(uri) {
            return Some(Arc::clone(content));
        }

        // Embedded class stubs live under synthetic `phpantom-stub://`
        // URIs and have no on-disk file.
        if let Some(class_name) = uri.strip_prefix("phpantom-stub://") {
            let stub_idx = self.stub_index.read();
            return stub_idx.get(class_name).map(|s| Arc::new(s.to_string()));
        }

        // Embedded function stubs use `phpantom-stub-fn://` URIs.
        if let Some(func_name) = uri.strip_prefix("phpantom-stub-fn://") {
            let stub_fn_idx = self.stub_function_index.read();
            return stub_fn_idx.get(func_name).map(|s| Arc::new(s.to_string()));
        }

        let path = Url::parse(uri).ok()?.to_file_path().ok()?;
        std::fs::read_to_string(path).ok().map(Arc::new)
    }

    /// Public helper for tests: get the uri_classes_index entry for a given URI.
    pub fn get_classes_for_uri(&self, uri: &str) -> Option<Vec<ClassInfo>> {
        self.uri_classes_index
            .read()
            .get(uri)
            .map(|classes| classes.iter().map(|c| ClassInfo::clone(c)).collect())
    }

    /// Gather the per-file context (classes, use-map, namespace) in one call.
    ///
    /// This replaces the repeated lock-and-unwrap boilerplate that was
    /// duplicated across the completion handler, definition resolver,
    /// implementation resolver, and variable definition modules.  Each of
    /// those sites used to have three nearly-identical blocks acquiring
    /// `uri_classes_index`, `use_map`, and `namespace_map` locks and
    /// extracting the entry for a given URI.
    pub(crate) fn file_context(&self, uri: &str) -> FileContext {
        let classes = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();

        // The legacy use_map (short name → FQN from `use` statements)
        // remains the canonical import table.  `resolved_names` is a
        // supplementary data source for consumers that can query by
        // byte offset — it must NOT replace the use_map because
        // `to_use_map()` only contains names that are actually
        // *referenced* in the code, not all *declared* imports.
        // The unused-imports diagnostic relies on seeing declared-but-
        // unreferenced imports.
        let use_map = self
            .file_imports
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();

        let namespace = self
            .file_namespaces
            .read()
            .get(uri)
            .and_then(|spans| spans.first())
            .and_then(|s| s.namespace.clone());

        let resolved_names = self.resolved_names.read().get(uri).cloned();

        FileContext {
            classes,
            use_map,
            namespace,
            resolved_names,
        }
    }

    /// Like [`file_context`](Self::file_context) but resolves the namespace
    /// for the namespace block that contains `byte_offset`.
    ///
    /// In single-namespace files this returns the same result as
    /// `file_context`.  In multi-namespace files it picks the correct
    /// namespace block for the cursor position.
    pub(crate) fn file_context_at(&self, uri: &str, byte_offset: u32) -> FileContext {
        let classes = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let use_map = self
            .file_imports
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let namespace = self.namespace_at_offset(uri, byte_offset);
        let resolved_names = self.resolved_names.read().get(uri).cloned();

        FileContext {
            classes,
            use_map,
            namespace,
            resolved_names,
        }
    }

    /// Return the namespace that contains the given byte offset in a file.
    ///
    /// For single-namespace files (the common case) this returns the file's
    /// only namespace.  For multi-namespace files it finds the namespace
    /// block whose byte range contains `byte_offset`.  Returns `None` when
    /// the offset is in the global namespace or the file has no namespace.
    pub(crate) fn namespace_at_offset(&self, uri: &str, byte_offset: u32) -> Option<String> {
        let nmap = self.file_namespaces.read();
        let spans = nmap.get(uri)?;
        // Try to find the namespace block containing the offset.
        for span in spans {
            if byte_offset >= span.start && byte_offset <= span.end {
                return span.namespace.clone();
            }
        }
        // Fallback: if the offset is past all namespace blocks (e.g.
        // code after the last closing brace), return the last namespace.
        spans.last().and_then(|s| s.namespace.clone())
    }

    /// Return the first namespace declared in a file.
    ///
    /// For single-namespace files this is the file's namespace.  For
    /// multi-namespace files this returns the first block's namespace,
    /// which may not be correct for all positions in the file.  Prefer
    /// [`namespace_at_offset`](Self::namespace_at_offset) when a cursor
    /// position is available.
    pub(crate) fn first_file_namespace(&self, uri: &str) -> Option<String> {
        self.file_namespaces
            .read()
            .get(uri)
            .and_then(|spans| spans.first())
            .and_then(|s| s.namespace.clone())
    }

    /// Return the import table (short name → FQN) for a file.
    ///
    /// Returns the legacy `use_map` which contains all *declared*
    /// imports from `use` statements, regardless of whether they are
    /// actually referenced in the code.  This is the correct source
    /// for consumers that need the full import table (unused-import
    /// detection, import-class code actions, name resolution helpers).
    ///
    /// For consumers that can resolve names by byte offset, prefer
    /// querying `resolved_names` directly via [`file_context`] instead.
    pub(crate) fn file_use_map(&self, uri: &str) -> std::collections::HashMap<String, String> {
        self.file_imports
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default()
    }

    /// Remove a file's entries from `uri_classes_index`, `use_map`, and `namespace_map`.
    ///
    /// This is the mirror of [`file_context`](Self::file_context): where that
    /// method *reads* the three maps, this method *clears* them for a given URI.
    /// Called from `did_close` to clean up state when a file is closed.
    pub(crate) fn clear_file_maps(&self, uri: &str) {
        // Drop per-file maps that are only needed while the file is
        // open.  uri_classes_index is redundant with fqn_class_index once indexing is
        // complete — GTD falls back to fqn_uri_index + parse_and_cache_file
        // when the uri_classes_index entry is missing.
        self.uri_classes_index.write().remove(uri);
        self.symbol_maps.write().remove(uri);
        self.evict_reference_index_uri(uri);
        self.file_imports.write().remove(uri);
        self.resolved_names.write().remove(uri);
        self.file_namespaces.write().remove(uri);
        // Parse errors are stored per file during update_ast and consumed
        // by the syntax-error diagnostic. Without this removal the last
        // parse-error vector for every file ever opened (or deleted from
        // disk) stays resident for the whole session.
        self.parse_errors.write().remove(uri);
        // NOTE: We intentionally keep fqn_uri_index and fqn_class_index intact.
        // fqn_uri_index maps FQN → URI so GTD can locate the file, and
        // fqn_class_index keeps the full ClassInfo for cross-file resolution.
        // The file will be re-parsed from disk on next access via
        // parse_and_cache_file when needed (issue #99).
    }
}
