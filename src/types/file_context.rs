//! The file-level metadata most handlers need, and per-`namespace`-block
//! views of it.

use super::*;

/// Bundles the three pieces of file-level metadata that almost every
/// handler needs: the parsed classes, the `use` statement import table,
/// and the declared namespace.  Constructed by
/// [`Backend::file_context`](crate::Backend) to replace the repeated
/// lock-and-unwrap boilerplate that was duplicated across completion,
/// definition, and implementation handlers.
#[derive(Default)]
pub(crate) struct FileContext {
    /// Classes extracted from the file's AST (from `uri_classes_index`).
    pub classes: Vec<Arc<ClassInfo>>,
    /// Class import table mapping short names to fully-qualified names
    /// (from the file's `FileState`).
    pub use_map: HashMap<String, String>,
    /// The file's function and constant imports.  Like `use_map`, every
    /// block's imports;
    /// see [`symbol_imports_at`](Self::symbol_imports_at).
    pub symbol_imports: SymbolImports,
    /// The file's declared namespace, if any.
    ///
    /// This is the *first* `namespace` block's name. A file may declare
    /// several, so consumers that know the byte offset they are resolving
    /// a name for should ask [`namespace_at`](Self::namespace_at) instead.
    pub namespace: Option<String>,
    /// Every `namespace` block in the file, but only when there is more
    /// than one.
    ///
    /// `None` for the overwhelmingly common single-namespace (or
    /// namespace-less) file, where `namespace` is already the right answer
    /// at every offset — populating it there would cost an allocation on
    /// every `file_context` call for nothing.
    pub namespace_spans: Option<Vec<NamespaceSpan>>,
    /// Per-file resolved names from `mago-names` (byte offset → FQN).
    ///
    /// `None` for files that were loaded via `parse_and_cache_content`
    /// (vendor/stub files) which don't run the name resolver.
    pub resolved_names: Option<Arc<crate::names::OwnedResolvedNames>>,
    /// The classes and traits `tests/Pest.php` binds to this file when it
    /// is a Pest test, which decide what `$this` is inside its test
    /// closures.  `None` for every other file.
    pub pest_uses: Option<Arc<[String]>>,
}

/// A class loader per `namespace` block of a file (see
/// [`Backend::class_loaders`](crate::Backend::class_loaders)), as trait
/// objects so it can be handed through non-generic walkers.
pub type BlockClassLoaders<'a> = PerBlock<'a, &'a crate::class_lookup::SourceClassLoader<'a>>;

/// [`BlockClassLoaders`] for already-resolved names (see
/// [`Backend::resolved_class_loaders`](crate::Backend::resolved_class_loaders)).
pub type BlockResolvedClassLoaders<'a> =
    PerBlock<'a, &'a crate::class_lookup::ResolvedClassLoader<'a>>;

/// The `use` imports of each `namespace` block of a file, built by
/// [`FileContext::block_imports`].
pub type BlockImports<'a> = PerBlock<'a, &'a HashMap<String, String>>;

/// One value per `namespace` block of a file, built by
/// [`FileContext::per_block`].
///
/// PHP scopes both the namespace and the `use` imports to a block, so
/// anything that resolves a source name (a class loader, above all) has to
/// be the one built for the block the name is written in.
pub struct PerBlock<'a, T> {
    /// The file's blocks, or empty when it has only one.
    spans: &'a [NamespaceSpan],
    /// One entry per span, or a single entry when `spans` is empty.
    items: Vec<T>,
}

impl<'a, T> PerBlock<'a, T> {
    /// One value for every offset, for a consumer that only reads code in
    /// the block it was built for.
    pub fn single(item: T) -> Self {
        PerBlock {
            spans: &[],
            items: vec![item],
        }
    }

    /// The value for the block containing `offset`.
    pub fn at(&self, offset: u32) -> &T {
        let index = self
            .spans
            .iter()
            .position(|span| offset >= span.start && offset <= span.end)
            // Past the last block (e.g. code after its closing brace).
            .unwrap_or(self.items.len() - 1);
        &self.items[index]
    }

    /// A value derived from each block's value.
    pub fn map<'b, U>(&'b self, f: impl FnMut(&'b T) -> U) -> PerBlock<'a, U> {
        PerBlock {
            spans: self.spans,
            items: self.items.iter().map(f).collect(),
        }
    }
}

impl<L: Fn(&str) -> Option<Arc<ClassInfo>>> PerBlock<'_, L> {
    /// These loaders as trait objects.
    pub fn as_dyn(&self) -> BlockClassLoaders<'_> {
        self.map(|loader| loader as &crate::class_lookup::SourceClassLoader<'_>)
    }
}

impl<L: Fn(crate::class_lookup::Fqn<'_>) -> Option<Arc<ClassInfo>>> PerBlock<'_, L> {
    /// These resolved-name loaders as trait objects.
    pub fn as_dyn_resolved(&self) -> BlockResolvedClassLoaders<'_> {
        self.map(|loader| loader as &crate::class_lookup::ResolvedClassLoader<'_>)
    }
}

impl FileContext {
    /// The namespace in effect at `offset`.
    ///
    /// Equals [`namespace`](Self::namespace) for single-namespace files.
    /// In a file with several `namespace` blocks it returns the one whose
    /// span contains `offset`, so a name written in the second block is
    /// not resolved against the first block's namespace.
    pub fn namespace_at(&self, offset: u32) -> &Option<String> {
        self.span_at(offset)
            .map_or(&self.namespace, |span| &span.namespace)
    }

    /// The `use` imports in force at `offset`.
    ///
    /// Equals [`use_map`](Self::use_map) for single-namespace files.  In a
    /// file with several `namespace` blocks it is the table of the block
    /// containing `offset`, so an import declared in one block does not
    /// apply in another.
    pub fn use_map_at(&self, offset: u32) -> &HashMap<String, String> {
        self.span_at(offset)
            .map_or(&self.use_map, |span| &span.use_map)
    }

    /// The `use function` and `use const` imports in force at `offset`,
    /// scoped to a block the way [`use_map_at`](Self::use_map_at) is.
    pub fn symbol_imports_at(&self, offset: u32) -> &SymbolImports {
        self.span_at(offset)
            .map_or(&self.symbol_imports, |span| &span.symbol_imports)
    }

    /// The start of the `namespace` block containing `offset`, which tells
    /// a file's blocks apart: `0` throughout a file with only one.
    pub fn block_start_at(&self, offset: u32) -> u32 {
        self.span_at(offset).map_or(0, |span| span.start)
    }

    /// The namespace block containing `offset`, when the file has several.
    fn span_at(&self, offset: u32) -> Option<&NamespaceSpan> {
        NamespaceSpan::containing(self.namespace_spans.as_ref()?, offset)
    }

    /// Build one `T` per `namespace` block from that block's imports and
    /// namespace, for a consumer that resolves names all over the file.
    ///
    /// A single-namespace file builds one value from the file-wide
    /// [`use_map`](Self::use_map) and [`namespace`](Self::namespace).
    /// Look a value up with [`PerBlock::at`].
    pub fn per_block<'a, T>(
        &'a self,
        mut build: impl FnMut(&'a HashMap<String, String>, &'a Option<String>) -> T,
    ) -> PerBlock<'a, T> {
        self.per_block_with_symbol_imports(|use_map, _, namespace| build(use_map, namespace))
    }

    /// [`per_block`](Self::per_block), also handing `build` the block's
    /// `use function` and `use const` imports, for a function or constant
    /// loader.
    pub fn per_block_with_symbol_imports<'a, T>(
        &'a self,
        mut build: impl FnMut(&'a HashMap<String, String>, &'a SymbolImports, &'a Option<String>) -> T,
    ) -> PerBlock<'a, T> {
        match self.namespace_spans.as_deref() {
            Some(spans) => PerBlock {
                spans,
                items: spans
                    .iter()
                    .map(|span| build(&span.use_map, &span.symbol_imports, &span.namespace))
                    .collect(),
            },
            None => PerBlock {
                spans: &[],
                items: vec![build(&self.use_map, &self.symbol_imports, &self.namespace)],
            },
        }
    }

    /// Each block's `use` imports, to pair with the loaders
    /// [`per_block`](Self::per_block) builds from them.
    pub fn block_imports(&self) -> BlockImports<'_> {
        self.per_block(|use_map, _| use_map)
    }

    /// This context narrowed to the namespace block containing `offset`:
    /// that block's namespace and imports, for a consumer that resolves
    /// every name it sees against one block.
    pub fn at(&self, offset: u32) -> FileContext {
        FileContext {
            classes: self.classes.clone(),
            use_map: self.use_map_at(offset).clone(),
            symbol_imports: self.symbol_imports_at(offset).clone(),
            namespace: self.namespace_at(offset).clone(),
            namespace_spans: self.namespace_spans.clone(),
            resolved_names: self.resolved_names.clone(),
            pest_uses: self.pest_uses.clone(),
        }
    }

    /// This context with `classes` in place of the file's own, for
    /// resolving against a re-parse of a patched copy of the file.
    pub fn with_classes(&self, classes: Vec<Arc<ClassInfo>>) -> FileContext {
        FileContext {
            classes,
            use_map: self.use_map.clone(),
            symbol_imports: self.symbol_imports.clone(),
            namespace: self.namespace.clone(),
            namespace_spans: self.namespace_spans.clone(),
            resolved_names: self.resolved_names.clone(),
            pest_uses: self.pest_uses.clone(),
        }
    }

    /// Resolve a name to its FQN using the best available data source.
    ///
    /// When `resolved_names` is available and contains an entry at
    /// `offset`, returns the mago-names result directly (it applies
    /// PHP's full name resolution rules in a single pass).
    ///
    /// Falls back to [`resolve_to_fqn`](crate::names::resolve_to_fqn)
    /// (use-map + namespace prefix) when `resolved_names` is not
    /// populated or has no entry at the given offset.
    ///
    /// `name` is the raw identifier text (used for the fallback path).
    /// `offset` is the starting byte offset of the identifier in the
    /// source file.
    pub fn resolve_name_at(&self, name: &str, offset: u32) -> String {
        if let Some(ref rn) = self.resolved_names
            && let Some(fqn) = rn.get(offset)
        {
            return fqn.to_string();
        }
        crate::names::resolve_to_fqn(name, self.use_map_at(offset), self.namespace_at(offset))
    }
}

/// A namespace block within a PHP file, tracking which byte range it covers.
///
/// Files with a single `namespace Foo;` declaration produce one span covering
/// the entire file.  Files with multiple `namespace Foo { ... }` blocks produce
/// one span per block.  Files without any namespace declaration produce a single
/// span with `namespace: None`.
#[derive(Debug, Clone)]
pub struct NamespaceSpan {
    /// The namespace name (e.g. `"App\Models"`), or `None` for the global namespace.
    pub namespace: Option<String>,
    /// Byte offset of the start of this namespace block (inclusive).
    pub start: u32,
    /// Byte offset of the end of this namespace block (inclusive).
    pub end: u32,
    /// The `use` imports declared inside this block.
    ///
    /// PHP scopes an import to the block that declares it, so a file with
    /// several blocks needs one table per block.  Left empty when the file
    /// has a single block, whose imports are the file-wide import table, so
    /// the common case stores them once.
    pub use_map: HashMap<String, String>,
    /// The `use function` and `use const` imports declared inside this
    /// block, stored the same way as [`use_map`](Self::use_map).
    pub symbol_imports: SymbolImports,
}

/// A file's `use function` and `use const` imports.
///
/// PHP keeps functions, constants and classes in separate symbol tables, so
/// `use function Foo\table;` and a class `Table` never collide.  These live
/// apart from the class import table (`use_map`) for the same reason: class
/// imports match case-insensitively, and a function or constant alias
/// sharing a short name with a class would otherwise claim it.
#[derive(Debug, Clone, Default)]
pub struct SymbolImports {
    /// `use function` imports, alias → function FQN.
    pub functions: HashMap<String, String>,
    /// `use const` imports, alias → constant FQN.
    pub constants: HashMap<String, String>,
}

impl SymbolImports {
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.constants.is_empty()
    }
}

impl NamespaceSpan {
    /// The block of `spans` that contains `offset`, or the last block for
    /// an offset past every block (e.g. code after its closing brace).
    pub fn containing(spans: &[NamespaceSpan], offset: u32) -> Option<&NamespaceSpan> {
        spans
            .iter()
            .find(|span| offset >= span.start && offset <= span.end)
            .or_else(|| spans.last())
    }
}
