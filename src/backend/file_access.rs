//! Per-file content and context accessors on [`Backend`].
//!
//! These read (and, for `clear_file_maps`, clear) the per-URI state that
//! backs every feature's "what do we know about this file?" query:
//! open-file content, the class index, and the [`FileState`] each parse
//! publishes (symbol map, imports, namespaces). Centralising them here removes the repeated
//! lock-and-unwrap boilerplate that used to be duplicated across the
//! completion handler, definition resolver, and other consumers.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tower_lsp::lsp_types::Position;

use crate::Backend;
use crate::file_state::FileState;
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

    /// Get the content of a file by URI, trying open files first, then the
    /// embedded stubs, then disk.
    ///
    /// An owned copy of [`get_file_content_arc`](Self::get_file_content_arc)'s
    /// answer, for callers that need a `String`.
    pub(crate) fn get_file_content(&self, uri: &str) -> Option<String> {
        self.get_file_content_arc(uri).map(Arc::unwrap_or_clone)
    }

    /// The text a request against `uri` analyses: the virtual PHP a
    /// template lowers to when one has been recorded, else the file's own
    /// content.
    ///
    /// A template's symbol map and types describe the virtual PHP, so every
    /// feature that reads them has to read the same text, and answer in the
    /// template's coordinates through `src/blade/translate.rs` afterwards.
    ///
    /// Shared rather than copied: a template's virtual PHP is fetched on
    /// every completion, hover, go-to-definition, code-action and
    /// diagnostic request against it.
    pub(crate) fn analysable_content(&self, uri: &str) -> Option<Arc<String>> {
        self.blade_virtual_php_arc(uri)
            .or_else(|| self.get_file_content_arc(uri))
    }

    /// [`Self::analysable_content`] with `position` carried into the text:
    /// a template's position is translated into its virtual PHP.
    pub(crate) fn analysable_content_at(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<(Arc<String>, Position)> {
        match self.blade_virtual_php_arc(uri) {
            Some(virtual_php) => Some((virtual_php, self.translate_blade_to_php(uri, position))),
            None => Some((self.get_file_content_arc(uri)?, position)),
        }
    }

    /// [`Self::analysable_content`] for a caller that already holds the
    /// file's content and only needs a template swapped for its virtual PHP.
    pub(crate) fn analysable_content_or<'a>(
        &self,
        uri: &str,
        content: &'a str,
    ) -> AnalysableContent<'a> {
        match self.blade_virtual_php_arc(uri) {
            Some(virtual_php) => AnalysableContent::Shared(virtual_php),
            None => AnalysableContent::Borrowed(content),
        }
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

        // Embedded stubs have no on-disk file.  A class stub's URI path is
        // the key of its source in `stub_index`, and a function stub's the
        // key in `stub_function_index`.
        if let Some(class_name) = uri.strip_prefix(crate::stubs::STUB_URI_PREFIX) {
            let stub_idx = self.stub_index.read();
            return stub_idx.get(class_name).map(|s| Arc::new(s.to_string()));
        }
        if let Some(func_name) = uri.strip_prefix(crate::stubs::STUB_FUNCTION_URI_PREFIX) {
            let stub_fn_idx = self.stub_function_index.read();
            return stub_fn_idx.get(func_name).map(|s| Arc::new(s.to_string()));
        }

        let path = crate::util::uri_to_path(uri)?;
        crate::util::read_php_source(&path).map(Arc::new)
    }

    /// Public helper for tests: get the uri_classes_index entry for a given URI.
    pub fn get_classes_for_uri(&self, uri: &str) -> Option<Vec<ClassInfo>> {
        self.symbols
            .uri_classes_index
            .read()
            .get(uri)
            .map(|classes| classes.iter().map(|c| ClassInfo::clone(c)).collect())
    }

    /// The classes `uri` declares, sharing the index's `Arc`s instead of
    /// copying each body the way [`get_classes_for_uri`](Self::get_classes_for_uri)
    /// does.  For the scanners that ask this of every candidate file.
    pub(crate) fn shared_classes_for_uri(&self, uri: &str) -> Option<Vec<Arc<ClassInfo>>> {
        self.symbols.uri_classes_index.read().get(uri).cloned()
    }

    /// The innermost class `uri` declares around `offset` (see
    /// [`find_class_at_offset`](crate::class_lookup::find_class_at_offset)),
    /// unlinked, without copying the file's class list.
    pub(crate) fn class_at_offset(&self, uri: &str, offset: u32) -> Option<Arc<ClassInfo>> {
        let index = self.symbols.uri_classes_index.read();
        crate::class_lookup::find_shared_class_at_offset(index.get(uri)?, offset).cloned()
    }

    /// The classes `uri` declares, linked the way the class loader links a
    /// class it loads (see [`link_loaded_class`](Self::link_loaded_class)).
    ///
    /// The file's own classes are found in this list before the class
    /// loader is asked, so they have to be linked here the way the loader
    /// links a class declared in any other file.
    fn linked_classes_for_uri(&self, uri: &str) -> Vec<Arc<ClassInfo>> {
        let mut classes = self
            .symbols
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        for class in &mut classes {
            if crate::resolution::needs_linking(class) {
                *class = self.link_loaded_class(Arc::clone(class));
            }
        }
        classes
    }

    /// The short names of the classes `uri` declares in `namespace`, for
    /// asking whether a name written there is one of the file's own
    /// classes without cloning their bodies.
    ///
    /// A file with several `namespace` blocks can declare a class in one
    /// that a bare name in another does not reach, so the classes are
    /// filtered to the namespace the name is written in.
    pub(crate) fn local_class_names(&self, uri: &str, namespace: Option<&str>) -> HashSet<String> {
        self.symbols
            .uri_classes_index
            .read()
            .get(uri)
            .map(|classes| {
                classes
                    .iter()
                    .filter(|c| match (c.file_namespace.as_deref(), namespace) {
                        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                        (a, b) => a.is_none() && b.is_none(),
                    })
                    .map(|c| c.name.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Gather the per-file context (classes, use-map, namespace) in one call.
    pub(crate) fn file_context(&self, uri: &str) -> FileContext {
        let classes = self.linked_classes_for_uri(uri);
        let state = self.file_state(uri);

        // The legacy use_map (short name → FQN from `use` statements)
        // remains the canonical import table.  `resolved_names` is a
        // supplementary data source for consumers that can query by
        // byte offset — it must NOT replace the use_map because
        // `to_use_map()` only contains names that are actually
        // *referenced* in the code, not all *declared* imports.
        // The unused-imports diagnostic relies on seeing declared-but-
        // unreferenced imports.
        let Some(state) = state else {
            return FileContext {
                classes,
                pest_uses: self.pest_uses_for(uri),
                ..FileContext::default()
            };
        };
        let (namespace, namespace_spans) = namespace_and_spans(&state.namespaces);
        FileContext {
            classes,
            use_map: state.use_map.clone(),
            symbol_imports: state.symbol_imports.clone(),
            namespace,
            namespace_spans,
            resolved_names: state.resolved_names.clone(),
            pest_uses: self.pest_uses_for(uri),
        }
    }

    /// What the last parse of `uri` published, or `None` when it has never
    /// been parsed.
    pub(crate) fn file_state(&self, uri: &str) -> Option<Arc<FileState>> {
        self.files.read().get(uri).cloned()
    }

    /// Like [`file_context`](Self::file_context) but resolves the namespace
    /// and imports of the namespace block that contains `byte_offset`.
    ///
    /// In single-namespace files this returns the same result as
    /// `file_context`.  In multi-namespace files it picks the correct
    /// namespace block for the cursor position.
    pub(crate) fn file_context_at(&self, uri: &str, byte_offset: u32) -> FileContext {
        let ctx = self.file_context(uri);
        // A file with only one namespace has nothing to pick between, so
        // the namespace and imports `file_context` already found stand.
        if ctx.namespace_spans.is_none() {
            return ctx;
        }
        ctx.at(byte_offset)
    }

    /// Subset of [`file_context_at`](Self::file_context_at) for callers
    /// that only need the `use`-map and namespace (e.g. resolving a bare
    /// name to its FQN). Skips the class-list and `resolved_names` clones.
    pub(crate) fn use_map_and_namespace_at(
        &self,
        uri: &str,
        byte_offset: u32,
    ) -> (HashMap<String, String>, Option<String>) {
        let files = self.files.read();
        let Some(state) = files.get(uri) else {
            return (HashMap::new(), None);
        };
        if state.namespaces.len() > 1
            && let Some(span) = NamespaceSpan::containing(&state.namespaces, byte_offset)
        {
            return (span.use_map.clone(), span.namespace.clone());
        }
        let namespace = namespace_in_spans(&state.namespaces, byte_offset).map(str::to_string);
        (state.use_map.clone(), namespace)
    }

    /// Return the namespace that contains the given byte offset in a file.
    ///
    /// For single-namespace files (the common case) this returns the file's
    /// only namespace.  For multi-namespace files it finds the namespace
    /// block whose byte range contains `byte_offset`.  Returns `None` when
    /// the offset is in the global namespace or the file has no namespace.
    pub(crate) fn namespace_at_offset(&self, uri: &str, byte_offset: u32) -> Option<String> {
        let files = self.files.read();
        namespace_in_spans(&files.get(uri)?.namespaces, byte_offset).map(str::to_string)
    }

    /// A copy of a file's namespace blocks, for callers that resolve many
    /// offsets against the same file and would otherwise take the
    /// file-state lock once per offset.
    pub(crate) fn namespace_spans_for_uri(&self, uri: &str) -> Vec<NamespaceSpan> {
        self.files
            .read()
            .get(uri)
            .map(|state| state.namespaces.clone())
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
        self.files
            .read()
            .get(uri)?
            .namespaces
            .first()
            .and_then(|s| s.namespace.clone())
    }

    /// Each `namespace` block of a file with the imports it declares, for
    /// consumers that read or write a file's `use` statements.
    ///
    /// PHP scopes an import to its block, so an edit that adds, checks,
    /// or rewrites one has to work against the block the code it serves
    /// is written in.  A file with a single block yields one entry with
    /// the file-wide import table and no range.
    pub(crate) fn import_blocks(&self, uri: &str) -> Vec<ImportBlock> {
        {
            let files = self.files.read();
            if let Some(state) = files.get(uri)
                && state.namespaces.len() > 1
            {
                return state.namespaces.iter().map(ImportBlock::of_span).collect();
            }
        }
        vec![self.whole_file_import_block(uri)]
    }

    /// The [`import_blocks`](Self::import_blocks) entry for the block
    /// containing `offset`.
    pub(crate) fn import_block_at(&self, uri: &str, offset: usize) -> ImportBlock {
        {
            let files = self.files.read();
            if let Some(state) = files.get(uri)
                && state.namespaces.len() > 1
                && let Some(span) = NamespaceSpan::containing(&state.namespaces, offset as u32)
            {
                return ImportBlock::of_span(span);
            }
        }
        self.whole_file_import_block(uri)
    }

    /// The one [`ImportBlock`] of a file that does not declare several
    /// `namespace` blocks.
    fn whole_file_import_block(&self, uri: &str) -> ImportBlock {
        ImportBlock {
            namespace: self.first_file_namespace(uri),
            use_map: self.file_use_map(uri),
            symbol_imports: self.symbol_imports_for(uri),
            range: None,
        }
    }

    /// The byte range of the block containing `offset`, as
    /// [`import_block_at`](Self::import_block_at) would report it, without
    /// copying any import table.
    pub(crate) fn import_block_range_at(&self, uri: &str, offset: usize) -> Option<(usize, usize)> {
        let files = self.files.read();
        let spans = &files.get(uri)?.namespaces;
        if spans.len() <= 1 {
            return None;
        }
        NamespaceSpan::containing(spans, offset as u32)
            .map(|span| (span.start as usize, span.end as usize))
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
        self.files
            .read()
            .get(uri)
            .map(|state| state.use_map.clone())
            .unwrap_or_default()
    }

    /// The `use function` and `use const` imports of a file.
    pub(crate) fn symbol_imports_for(&self, uri: &str) -> crate::types::SymbolImports {
        self.files
            .read()
            .get(uri)
            .map(|state| state.symbol_imports.clone())
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
        self.files.read().get(uri)?.symbol_map.clone()
    }

    /// The URIs of every file a parse has published a symbol map for.
    pub(crate) fn uris_with_symbol_maps(&self) -> Vec<String> {
        self.files
            .read()
            .iter()
            .filter(|(_, state)| state.symbol_map.is_some())
            .map(|(uri, _)| uri.clone())
            .collect()
    }

    /// Pair a file's cached symbol map with `content`, for a caller that
    /// needs to slice a span but does not hold the map itself.
    ///
    /// `None` when the file has never been parsed, or when the map was
    /// extracted from different text than `content` — see
    /// [`MappedSource`](crate::symbol_map::MappedSource).
    pub(crate) fn symbol_map_source<'a>(
        &self,
        uri: &str,
        content: &'a str,
    ) -> Option<crate::symbol_map::MappedSource<'a>> {
        self.files
            .read()
            .get(uri)?
            .symbol_map
            .as_ref()?
            .source(content)
    }

    /// Remove what is recorded about a file: its [`FileState`], its
    /// classes in `uri_classes_index`, its reference-index entries, and the
    /// caches and indexes derived from it.
    ///
    /// Called from `did_close` to clean up state when a file the workspace
    /// index does not cover is closed.
    pub(crate) fn clear_file_maps(&self, uri: &str) {
        self.laravel
            .string_key_cache
            .write()
            .invalidate_for_uri(uri, "");
        // uri_classes_index is redundant with fqn_class_index once indexing
        // is complete — GTD falls back to fqn_uri_index + parse_and_cache_file
        // when the uri_classes_index entry is missing.
        self.symbols.uri_classes_index.write().remove(uri);
        // The symbol map, imports, namespaces, resolved names, Pest
        // bindings, parse errors, and a template's lowered PHP and source
        // map.  Parse errors in particular are otherwise kept for every
        // file ever opened (or deleted from disk) for the whole session,
        // and a template's lowering is recorded for every indexed template,
        // not just open ones, so a deleted or renamed one would stay in the
        // template lists the Blade refresh passes enumerate.
        self.files.write().remove(uri);
        self.evict_typed_receiver_view_spans(uri);
        // Another file's entry is stamped with the class-lookup generation,
        // which a class this file withdraws bumps, so only this file's own
        // entry goes stale here.
        self.evict_morph_column_spans(uri);
        self.evict_reference_index_uri(uri);
        // Likewise a template's injected variables.
        self.blade.uris.write().remove(uri);
        self.blade.injected_vars.write().remove(uri);
        // The config keys a file declares at runtime (`Config::set(...)`)
        // are otherwise only refreshed when the file is re-parsed, which a
        // deleted file never is.
        self.laravel.runtime_config_keys.write().remove(uri);
        // What the file declared is gone for every other file's diagnostics.
        self.diag.epoch.note_change();
        // NOTE: We intentionally keep fqn_uri_index and fqn_class_index intact.
        // fqn_uri_index maps FQN → URI so GTD can locate the file, and
        // fqn_class_index keeps the full ClassInfo for cross-file resolution.
        // The file will be re-parsed from disk on next access via
        // parse_and_cache_file when needed (issue #99).
    }

    /// [`clear_file_maps`](Self::clear_file_maps) for a closed file the
    /// workspace index does not cover, which also gives up the functions
    /// and constants it declared and the record that it was parsed.  A
    /// vendor or autoload file is parsed again on its next lookup.
    ///
    /// Its classes stay in `fqn_class_index` (see `clear_file_maps`), and
    /// so do their inheritance edges, which describe those classes.
    pub(crate) fn forget_closed_file(&self, uri: &str) {
        self.clear_file_maps(uri);
        self.parsed_uris.write().remove(uri);
        let uris = std::collections::HashSet::from([uri.to_string()]);
        self.withdraw_functions_for_uris(&uris);
        self.symbols
            .global_defines
            .write()
            .retain(|_, d| d.file_uri.as_str() != uri);
        self.symbols.uri_globals_index.write().remove(uri);
    }

    /// The on-disk path of `uri` when it is a PHP file the workspace index
    /// covers: inside the workspace root, outside the vendor directories,
    /// not excluded by `[indexing] exclude`, and with a PHP extension.
    ///
    /// Such a file has to stay indexed after the editor closes it, since
    /// the reference index and the reference-count lenses read it whether
    /// or not it is open, and the completed workspace index is never walked
    /// again to put it back.
    pub(crate) fn workspace_index_path(&self, uri: &str) -> Option<std::path::PathBuf> {
        let path = crate::util::uri_to_path(uri)?;
        let root = self.workspace.workspace_root.read().clone()?;
        if !path.starts_with(&root) {
            return None;
        }
        if self
            .workspace
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
        {
            return None;
        }
        let filters = self.index_filters();
        (filters.is_php_file(&path) && !filters.is_excluded_path(&path, false)).then_some(path)
    }
}

/// The text a caller should analyse: either the buffer it already holds,
/// or the shared virtual PHP a template lowers to.
///
/// Returned by [`Backend::analysable_content_or`] so that swapping a
/// template for its virtual PHP costs an `Arc` bump rather than a copy of
/// the whole lowered file. Deref to `&str` to read it.
pub(crate) enum AnalysableContent<'a> {
    Borrowed(&'a str),
    Shared(Arc<String>),
}

impl std::ops::Deref for AnalysableContent<'_> {
    type Target = str;

    fn deref(&self) -> &str {
        match self {
            AnalysableContent::Borrowed(text) => text,
            AnalysableContent::Shared(text) => text,
        }
    }
}

/// The namespace in effect at `byte_offset` among a file's namespace
/// blocks, or `None` for the global namespace.
///
/// An offset past every block (code after the last closing brace) belongs
/// to the last one.
pub(crate) fn namespace_in_spans(spans: &[NamespaceSpan], byte_offset: u32) -> Option<&str> {
    NamespaceSpan::containing(spans, byte_offset).and_then(|s| s.namespace.as_deref())
}

/// One `namespace` block's imports, from [`Backend::import_blocks`].
pub(crate) struct ImportBlock {
    /// The block's namespace, or `None` for the global namespace.
    pub(crate) namespace: Option<String>,
    /// The class imports the block declares (alias → FQN).
    pub(crate) use_map: HashMap<String, String>,
    /// The `use function` and `use const` imports the block declares.
    pub(crate) symbol_imports: crate::types::SymbolImports,
    /// The block's byte range, or `None` when the file has only one
    /// block and its imports apply to the whole file.
    pub(crate) range: Option<(usize, usize)>,
}

impl ImportBlock {
    /// The block `span` covers, in a file that declares several.
    fn of_span(span: &NamespaceSpan) -> ImportBlock {
        ImportBlock {
            namespace: span.namespace.clone(),
            use_map: span.use_map.clone(),
            symbol_imports: span.symbol_imports.clone(),
            range: Some((span.start as usize, span.end as usize)),
        }
    }

    /// The index of the block containing `offset`, or of the last block
    /// for an offset past every block (e.g. code after its closing brace).
    pub(crate) fn index_at(blocks: &[ImportBlock], offset: usize) -> usize {
        blocks
            .iter()
            .position(|block| block.range.is_none_or(|(s, e)| offset >= s && offset <= e))
            .unwrap_or(blocks.len().saturating_sub(1))
    }
}

/// A file's first namespace plus, for files that declare more than one
/// `namespace` block, every block's span.
///
/// The span list stays `None` for the single-namespace case so that
/// building a [`FileContext`] costs no extra allocation there; see
/// [`FileContext::namespace_spans`].
fn namespace_and_spans(spans: &[NamespaceSpan]) -> (Option<String>, Option<Vec<NamespaceSpan>>) {
    let first = spans.first().and_then(|s| s.namespace.clone());
    if spans.len() > 1 {
        (first, Some(spans.to_vec()))
    } else {
        (first, None)
    }
}
