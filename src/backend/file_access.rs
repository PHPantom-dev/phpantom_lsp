//! Per-file content and context accessors on [`Backend`].
//!
//! These read (and, for `clear_file_maps`, clear) the per-URI maps that
//! back every feature's "what do we know about this file?" query:
//! open-file content, the class index, the import table, and the
//! namespace map. Centralising them here removes the repeated
//! lock-and-unwrap boilerplate that used to be duplicated across the
//! completion handler, definition resolver, and other consumers.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tower_lsp::lsp_types::{Position, Url};

use crate::Backend;
use crate::types::{ClassInfo, FileContext, NamespaceSpan};

impl Backend {
    /// Look up a class by its (possibly namespace-qualified) name via
    /// `fqn_class_index`, without triggering any disk I/O.
    ///
    /// The `class_name` can be:
    ///   - A simple name like `"Customer"`
    ///   - A namespace-qualified name like `"Klarna\\Customer"`
    ///   - A fully-qualified name like `"\\Klarna\\Customer"` (leading `\` is stripped)
    ///
    /// Returns a shared `Arc<ClassInfo>` if found, or `None`.
    pub(crate) fn find_class_in_uri_classes_index(
        &self,
        class_name: &str,
    ) -> Option<Arc<ClassInfo>> {
        // For namespace-qualified names the FQN is the normalized name
        // itself.  For bare names (no backslash) the FQN equals the
        // short name, which is also stored in the index.
        if let Some(cls) = self.symbols.fqn_class_index.read().get(class_name) {
            return Some(Arc::clone(cls));
        }

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

    /// The text a request against `uri` analyses: the virtual PHP a
    /// template lowers to when one has been recorded, else the file's own
    /// content.
    ///
    /// A template's symbol map and types describe the virtual PHP, so every
    /// feature that reads them has to read the same text, and answer in the
    /// template's coordinates through `src/blade/translate.rs` afterwards.
    pub(crate) fn analysable_content(&self, uri: &str) -> Option<String> {
        self.blade_virtual_php(uri)
            .or_else(|| self.get_file_content(uri))
    }

    /// [`Self::analysable_content`] with `position` carried into the text:
    /// a template's position is translated into its virtual PHP.
    pub(crate) fn analysable_content_at(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<(String, Position)> {
        match self.blade_virtual_php(uri) {
            Some(virtual_php) => Some((virtual_php, self.translate_blade_to_php(uri, position))),
            None => Some((self.get_file_content(uri)?, position)),
        }
    }

    /// [`Self::analysable_content`] for a caller that already holds the
    /// file's content and only needs a template swapped for its virtual PHP.
    pub(crate) fn analysable_content_or<'a>(&self, uri: &str, content: &'a str) -> Cow<'a, str> {
        self.blade_virtual_php(uri)
            .map_or(Cow::Borrowed(content), Cow::Owned)
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
        self.symbols
            .uri_classes_index
            .read()
            .get(uri)
            .map(|classes| classes.iter().map(|c| ClassInfo::clone(c)).collect())
    }

    /// The short names of the classes `uri` declares, for asking whether a
    /// name is one of the file's own classes without cloning their bodies.
    pub(crate) fn local_class_names(&self, uri: &str) -> HashSet<String> {
        self.symbols
            .uri_classes_index
            .read()
            .get(uri)
            .map(|classes| classes.iter().map(|c| c.name.to_string()).collect())
            .unwrap_or_default()
    }

    /// Gather the per-file context (classes, use-map, namespace) in one call.
    ///
    /// This replaces the repeated lock-and-unwrap boilerplate that was
    /// duplicated across the completion handler, definition resolver,
    /// implementation resolver, and variable definition modules.  Each of
    /// those sites used to have three nearly-identical blocks acquiring
    /// `uri_classes_index`, `file_imports`, and `file_namespaces` locks
    /// and extracting the entry for a given URI.
    pub(crate) fn file_context(&self, uri: &str) -> FileContext {
        let classes = self
            .symbols
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

        let (namespace, namespace_spans) = self.namespace_and_spans(uri);
        let resolved_names = self.resolved_names.read().get(uri).cloned();

        FileContext {
            classes,
            use_map,
            namespace,
            namespace_spans,
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
            .symbols
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
        let (first_namespace, namespace_spans) = self.namespace_and_spans(uri);
        let namespace = match namespace_spans.as_ref() {
            Some(_) => self.namespace_at_offset(uri, byte_offset),
            None => first_namespace,
        };
        let resolved_names = self.resolved_names.read().get(uri).cloned();

        FileContext {
            classes,
            use_map,
            namespace,
            namespace_spans,
            resolved_names,
        }
    }

    /// Subset of [`file_context_at`](Self::file_context_at) for callers
    /// that only need the enclosing class list and namespace (e.g.
    /// resolving `self`/`static`/`parent`). Skips the `use`-map and
    /// `resolved_names` clones.
    pub(crate) fn classes_and_namespace_at(
        &self,
        uri: &str,
        byte_offset: u32,
    ) -> (Vec<Arc<ClassInfo>>, Option<String>) {
        let classes = self
            .symbols
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let namespace = self.namespace_at_offset(uri, byte_offset);
        (classes, namespace)
    }

    /// Subset of [`file_context_at`](Self::file_context_at) for callers
    /// that only need the `use`-map and namespace (e.g. resolving a bare
    /// name to its FQN). Skips the class-list and `resolved_names` clones.
    pub(crate) fn use_map_and_namespace_at(
        &self,
        uri: &str,
        byte_offset: u32,
    ) -> (HashMap<String, String>, Option<String>) {
        let use_map = self
            .file_imports
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let namespace = self.namespace_at_offset(uri, byte_offset);
        (use_map, namespace)
    }

    /// Return a file's first namespace plus, for files that declare more
    /// than one `namespace` block, every block's span.
    ///
    /// The span list stays `None` for the single-namespace case so that
    /// building a [`FileContext`] costs no extra allocation there; see
    /// [`FileContext::namespace_spans`].
    fn namespace_and_spans(&self, uri: &str) -> (Option<String>, Option<Vec<NamespaceSpan>>) {
        let nmap = self.file_namespaces.read();
        let Some(spans) = nmap.get(uri) else {
            return (None, None);
        };
        let first = spans.first().and_then(|s| s.namespace.clone());
        if spans.len() > 1 {
            (first, Some(spans.clone()))
        } else {
            (first, None)
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
        namespace_in_spans(nmap.get(uri)?, byte_offset).map(str::to_string)
    }

    /// A copy of a file's namespace blocks, for callers that resolve many
    /// offsets against the same file and would otherwise take the
    /// `file_namespaces` lock once per offset.
    pub(crate) fn namespace_spans_for_uri(&self, uri: &str) -> Vec<NamespaceSpan> {
        self.file_namespaces
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default()
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

    /// Look up the precomputed [`SymbolMap`](crate::symbol_map::SymbolMap)
    /// for a file, without triggering any parsing.
    ///
    /// Returns `None` when the file has never been parsed (no `did_open`
    /// / `update_ast` yet). Centralises the read-lock-and-clone
    /// boilerplate that used to be duplicated at the top of every
    /// diagnostic collector.
    pub(crate) fn symbol_map_for(&self, uri: &str) -> Option<Arc<crate::symbol_map::SymbolMap>> {
        self.symbol_maps.read().get(uri).cloned()
    }

    /// Remove a file's entries from every per-URI map populated while it
    /// was open (`uri_classes_index`, `symbol_maps`, `file_imports`,
    /// `resolved_names`, `file_namespaces`, `parse_errors`), plus the
    /// reference index.
    ///
    /// Called from `did_close` to clean up state when a file is closed.
    pub(crate) fn clear_file_maps(&self, uri: &str) {
        // uri_classes_index is redundant with fqn_class_index once indexing
        // is complete — GTD falls back to fqn_uri_index + parse_and_cache_file
        // when the uri_classes_index entry is missing.
        self.symbols.uri_classes_index.write().remove(uri);
        self.symbol_maps.write().remove(uri);
        self.evict_typed_receiver_view_spans(uri);
        self.morph_column_spans_cache.write().clear();
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

/// The namespace in effect at `byte_offset` among a file's namespace
/// blocks, or `None` for the global namespace.
///
/// An offset past every block (code after the last closing brace) belongs
/// to the last one.
pub(crate) fn namespace_in_spans(spans: &[NamespaceSpan], byte_offset: u32) -> Option<&str> {
    for span in spans {
        if byte_offset >= span.start && byte_offset <= span.end {
            return span.namespace.as_deref();
        }
    }
    spans.last().and_then(|s| s.namespace.as_deref())
}
