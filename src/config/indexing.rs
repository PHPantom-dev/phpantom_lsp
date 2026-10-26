//! `[indexing]` section and the editor-supplied indexing filters.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// `[indexing]` section — controls how PHPantom discovers classes across
/// the workspace.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct IndexingConfig {
    /// The indexing strategy.
    ///
    /// - `"full"` (default) — same discovery as `"self"`, then
    ///   background-parse every user PHP file to populate symbol and
    ///   reference indexes.
    /// - `"composer"` — use Composer's classmap when available,
    ///   fall back to self-scan when it is missing or incomplete.
    /// - `"self"` — scan every PHP file under the workspace root,
    ///   ignoring Composer's generated classmap and PSR-4 mappings.
    ///   Vendor packages are still scanned via `installed.json`.
    /// - `"none"` — no proactive scanning. Still uses Composer's classmap
    ///   if present, still resolves on demand, but never falls back to
    ///   self-scan.
    pub strategy: Option<IndexingStrategy>,
    /// Paths the workspace scanners must skip, in gitignore syntax
    /// relative to the workspace root: a bare name matches at any
    /// depth, a pattern containing `/` anchors to the root, a trailing
    /// `/` restricts to directories, and a leading `!` re-includes.
    ///
    /// Excludes apply to background discovery and to the directories
    /// `analyze` walks. A file the editor opens, or one named outright
    /// on the `analyze` command line, is always served.
    pub exclude: Option<Vec<String>>,
    /// Extra file extensions (without the dot) treated as PHP source
    /// during workspace discovery, e.g. `["module", "inc", "theme"]`
    /// for Drupal. `.php` is always included. Drupal projects get the
    /// Drupal extensions inside the detected web root automatically;
    /// this setting extends discovery elsewhere.
    pub extensions: Option<Vec<String>>,

    /// Extra files and directories to index, relative to the workspace
    /// root (absolute paths are also accepted).
    ///
    /// Listing a path here indexes it even when the workspace walk
    /// would skip it as hidden or gitignored. Files are scanned
    /// directly; directories are walked recursively, with
    /// [`exclude`](Self::exclude) still pruning entries below them.
    ///
    /// ```toml
    /// [indexing]
    /// include = [".lib", ".lib.stub.php"]
    /// ```
    pub include: Option<Vec<String>>,
}

impl IndexingConfig {
    pub fn strategy(&self) -> IndexingStrategy {
        self.strategy.unwrap_or_default()
    }

    pub fn exclude(&self) -> &[String] {
        self.exclude.as_deref().unwrap_or_default()
    }

    pub fn extensions(&self) -> &[String] {
        self.extensions.as_deref().unwrap_or_default()
    }

    /// Resolve [`include`](Self::include) against the workspace root,
    /// dropping entries that do not exist.
    ///
    /// Relative entries are joined to `workspace_root`; absolute ones
    /// are taken as-is.
    pub fn include_paths(&self, workspace_root: &Path) -> Vec<PathBuf> {
        self.include
            .iter()
            .flatten()
            .map(|entry| {
                let path = Path::new(entry);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    workspace_root.join(path)
                }
            })
            .filter(|path| path.exists())
            .collect()
    }
}

/// File filters supplied by the editor rather than by `.phpantom.toml`.
///
/// An editor already knows which paths the user hides and which
/// extensions they treat as PHP; mirroring that into `.phpantom.toml`
/// by hand is duplicated work. A client passes the same two lists
/// [`IndexingConfig`] accepts through `initializationOptions` (and
/// again through `workspace/didChangeConfiguration` when the user
/// edits their settings mid-session):
///
/// ```json
/// { "indexing": { "exclude": ["generated"], "extensions": ["module"] } }
/// ```
///
/// The shape is deliberately generic. It is a list of gitignore-style
/// globs and a list of extensions, never an editor's own setting
/// names, so every client can translate its native settings into it.
/// This layer sits beside the config-file layer rather than replacing
/// it: [`Backend::index_filters`](crate::Backend::index_filters)
/// compiles the union, so a `.phpantom.toml` reload keeps the client's
/// contribution and a client update keeps the file's.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ClientIndexingOptions {
    /// Paths to skip, in the same gitignore syntax as
    /// [`IndexingConfig::exclude`].
    pub exclude: Vec<String>,
    /// Extra file extensions treated as PHP source, as in
    /// [`IndexingConfig::extensions`].
    pub extensions: Vec<String>,
}

impl ClientIndexingOptions {
    /// Read the `indexing` filters out of a client-supplied settings
    /// blob, from either `initializationOptions` or the `settings` of a
    /// `workspace/didChangeConfiguration` notification.
    ///
    /// Accepts the filters at the top level (`{"indexing": …}`, what an
    /// extension passes as its own initialization options) or nested
    /// under a `phpantom` key (`{"phpantom": {"indexing": …}}`, the
    /// shape a client sends when it pushes its whole settings tree).
    ///
    /// Returns `None` when the blob carries no readable `indexing`
    /// block, meaning "this notification is not about file filters"
    /// rather than "the user cleared them". Clients re-push settings
    /// for reasons of their own (VS Code's client sends the whole
    /// `phpantom` section whenever any key in it changes), and reading
    /// those as an empty filter set would silently discard what the
    /// editor forwarded at startup. Clearing is still expressible: an
    /// `indexing` block that is present but empty parses as no filters.
    /// A malformed block is ignored for the same reason.
    pub fn from_client_settings(settings: &serde_json::Value) -> Option<Self> {
        let scoped = settings.get("phpantom").unwrap_or(settings);
        serde_json::from_value(scoped.get("indexing")?.clone()).ok()
    }

    /// Whether the client supplied no filters at all.
    pub fn is_empty(&self) -> bool {
        self.exclude.is_empty() && self.extensions.is_empty()
    }
}

/// The indexing strategy that controls class discovery behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndexingStrategy {
    /// Background-parse every PHP file for rich intelligence.
    #[default]
    Full,
    /// Merged classmap + self-scan.  Load Composer's classmap (if it
    /// exists) as a skip set, then self-scan all PSR-4 and vendor
    /// directories for anything the classmap missed.  Whatever the
    /// classmap already covers is a free performance win; whatever it's
    /// missing, we find ourselves.  No completeness heuristic needed.
    Composer,
    /// Scan every PHP file under the workspace root, ignoring
    /// Composer's generated classmap and PSR-4 mappings entirely.
    /// The vendor directory is scanned separately (via
    /// `installed.json`) since it is typically gitignored.
    SelfScan,
    /// No proactive scanning.  Uses Composer's classmap if present but
    /// never self-scans to fill gaps.
    None,
}

impl<'de> Deserialize<'de> for IndexingStrategy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "composer" => Ok(IndexingStrategy::Composer),
            "self" => Ok(IndexingStrategy::SelfScan),
            "full" => Ok(IndexingStrategy::Full),
            "none" => Ok(IndexingStrategy::None),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["composer", "self", "full", "none"],
            )),
        }
    }
}

impl std::fmt::Display for IndexingStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexingStrategy::Composer => write!(f, "composer"),
            IndexingStrategy::SelfScan => write!(f, "self"),
            IndexingStrategy::Full => write!(f, "full"),
            IndexingStrategy::None => write!(f, "none"),
        }
    }
}
