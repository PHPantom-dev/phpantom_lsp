//! What one parse of a file produced, kept as one record per URI.
//!
//! A parse publishes the file's symbol map, parse errors, imports,
//! namespaces, resolved names, Pest bindings and (for a template) its
//! lowering together, by replacing the file's [`FileState`] as a whole, and
//! closing or deleting the file removes it in one call.  A reader therefore
//! never sees one parse's symbol map next to another parse's imports, and a
//! field added here is published and removed with the rest.

use std::collections::HashMap;
use std::sync::Arc;

use crate::ParseErrorEntry;
use crate::blade::source_map::BladeSourceMap;
use crate::names::OwnedResolvedNames;
use crate::symbol_map::SymbolMap;
use crate::types::{NamespaceSpan, SymbolImports};

/// Everything the last parse of one file left behind.
#[derive(Clone, Default)]
pub(crate) struct FileState {
    /// The file's precomputed symbol spans.  `None` until a parse
    /// succeeds: a file the parser panics on keeps only its error, so the
    /// next request parses it again.
    pub(crate) symbol_map: Option<Arc<SymbolMap>>,
    /// Errors from the Mago parser, as `(message, start, end)`.  When the
    /// parser panics, a single "Parse failed" entry.
    pub(crate) parse_errors: Vec<ParseErrorEntry>,
    /// The file's `use` statement mappings (short name → fully qualified
    /// name).  For example, `use Klarna\Rest\Resource;` produces
    /// `"Resource" → "Klarna\Rest\Resource"`.
    pub(crate) use_map: HashMap<String, String>,
    /// The file's `use function` and `use const` imports, kept apart from
    /// the class imports in [`Self::use_map`].
    pub(crate) symbol_imports: SymbolImports,
    /// Byte-offset → FQN lookups for every identifier in the file,
    /// produced by `mago-names`.
    pub(crate) resolved_names: Option<Arc<OwnedResolvedNames>>,
    /// The namespace blocks declared in the file.  Single-namespace files
    /// have exactly one entry; multi-namespace files (using
    /// `namespace Foo { }` blocks) have one entry per block.
    pub(crate) namespaces: Vec<NamespaceSpan>,
    /// The classes and traits a test file binds to itself with a `uses(…)`
    /// or `pest()->extends(…)` that has no `->in(…)`.
    pub(crate) pest_uses: Option<Arc<[String]>>,
    /// What a Blade template lowers to.
    pub(crate) blade: Option<BladeFileState>,
}

/// A template's lowering.
#[derive(Clone)]
pub(crate) struct BladeFileState {
    /// The virtual PHP the template lowers to.  Shared rather than owned:
    /// every request against a template reads this text, and a template's
    /// virtual PHP is several times the size of the template itself.
    pub(crate) virtual_php: Arc<String>,
    /// Maps virtual PHP positions back to the template's.
    pub(crate) source_map: Arc<BladeSourceMap>,
}

/// Every file's state, keyed by URI.
pub(crate) type FileStates = HashMap<String, Arc<FileState>>;

impl FileState {
    /// A copy of this state with `symbol_map` in place of its own.
    pub(crate) fn with_symbol_map(&self, symbol_map: Arc<SymbolMap>) -> Self {
        Self {
            symbol_map: Some(symbol_map),
            ..self.clone()
        }
    }
}
