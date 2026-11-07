//! Configuration loaded from `.phpantom.toml`.
//!
//! Settings are read from two locations (in order of precedence):
//!
//! 1. **Project** — `.phpantom.toml` in the workspace root (next to
//!    `composer.json`).
//! 2. **Global** — `$XDG_CONFIG_HOME/phpantom_lsp/.phpantom.toml`
//!    (typically `~/.config/phpantom_lsp/.phpantom.toml` on Linux),
//!    which `phpantom_lsp init --global` creates.
//!
//! The two are merged key by key rather than one replacing the other,
//! so a project only has to spell out the settings where it differs
//! from the user's defaults.  When neither file exists, all settings
//! use their defaults.

use std::path::{Path, PathBuf};

use etcetera::BaseStrategy as _;
use serde::Deserialize;

mod diagnostics;
mod indexing;
mod inlay_hints;
mod laravel;
mod stubs;
#[cfg(test)]
mod tests;
mod tools;

pub use diagnostics::{DiagnosticsConfig, IgnoreRule};
pub use indexing::{ClientIndexingOptions, IndexingConfig, IndexingStrategy};
pub use inlay_hints::{ClientInlayHintOptions, InlayHintKinds, InlayHintsConfig};
pub use laravel::{LaravelConfig, LaravelMigrationsConfig, LaravelSchemaConfig};
pub use stubs::{ClientStubOptions, StubsConfig};
pub use tools::{FormattingConfig, MagoConfig, PhpStanConfig, PhpcsConfig, PhpmdConfig};

/// Top-level configuration parsed from `.phpantom.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// PHP version and language settings.
    pub php: PhpConfig,
    /// Diagnostic toggles.
    pub diagnostics: DiagnosticsConfig,
    /// Indexing strategy and file discovery settings.
    pub indexing: IndexingConfig,
    /// Stub files whose declarations win.
    pub stubs: StubsConfig,
    /// Semantic token highlighting settings.
    #[serde(rename = "semantic-tokens")]
    pub semantic_tokens: SemanticTokensConfig,
    /// Inlay hint toggles.
    #[serde(rename = "inlay-hints")]
    pub inlay_hints: InlayHintsConfig,
    /// Formatting proxy settings.
    pub formatting: FormattingConfig,
    /// PHPStan proxy settings.
    pub phpstan: PhpStanConfig,
    /// PHPCS (PHP_CodeSniffer) proxy settings.
    pub phpcs: PhpcsConfig,
    /// PHPMD (PHP Mess Detector) proxy settings.
    pub phpmd: PhpmdConfig,
    /// Mago proxy settings.
    pub mago: MagoConfig,
    /// Laravel-specific analysis settings.
    pub laravel: LaravelConfig,
}

/// `[semantic-tokens]` section — controls LSP semantic highlighting.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SemanticTokensConfig {
    /// Semantic token emission mode.
    ///
    /// - `"contextual"` (default) — emit only context-sensitive tokens
    ///   that syntax grammars usually cannot infer.
    /// - `"full"` — emit the complete semantic token stream.
    /// - `"off"` — return no semantic tokens.
    pub mode: Option<SemanticTokensMode>,
}

impl SemanticTokensConfig {
    pub fn mode(&self) -> SemanticTokensMode {
        self.mode.unwrap_or_default()
    }
}

/// Semantic token emission mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SemanticTokensMode {
    /// Emit only context-sensitive tokens that complement editor syntax highlighting.
    #[default]
    Contextual,
    /// Emit every token PHPantom can classify.
    Full,
    /// Disable semantic tokens.
    Off,
}

impl<'de> Deserialize<'de> for SemanticTokensMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "contextual" => Ok(SemanticTokensMode::Contextual),
            "full" => Ok(SemanticTokensMode::Full),
            "off" => Ok(SemanticTokensMode::Off),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["contextual", "full", "off"],
            )),
        }
    }
}

impl std::fmt::Display for SemanticTokensMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SemanticTokensMode::Contextual => write!(f, "contextual"),
            SemanticTokensMode::Full => write!(f, "full"),
            SemanticTokensMode::Off => write!(f, "off"),
        }
    }
}

/// `[php]` section — PHP version override.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PhpConfig {
    /// Override the detected PHP version (e.g. `"8.3"`).
    /// When `None`, PHPantom infers from `composer.json`.
    pub version: Option<String>,
}

/// Sections and keys an earlier release spelled differently, as
/// `(parent table path, old name, current name)`.
///
/// From 1.0 a key that once worked has to keep working, so a rename
/// lands here rather than breaking the configs that use the old name.
const RENAMED_KEYS: &[(&[&str], &str, &str)] = &[
    (&[], "semantic_tokens", "semantic-tokens"),
    (&[], "inlay_hints", "inlay-hints"),
    (
        &["diagnostics"],
        "report-magic-properties",
        "magic-properties",
    ),
];

/// Rewrite the old spellings in [`RENAMED_KEYS`] to the current ones,
/// recording a warning for each one `path` uses.
///
/// This runs on each file before the layers are merged, rather than
/// through a serde alias, so a global config written with the old name
/// and a project config written with the new one still merge key by
/// key instead of colliding as duplicate fields.  When one file spells
/// a key both ways, the current spelling wins.
fn upgrade_renamed_keys(table: &mut toml::Table, path: &Path, warnings: &mut Vec<ConfigWarning>) {
    for &(parent_path, old, new) in RENAMED_KEYS {
        let mut parent = Some(&mut *table);
        for segment in parent_path {
            parent = parent.and_then(|t| t.get_mut(*segment)?.as_table_mut());
        }
        let Some(parent) = parent else {
            continue;
        };
        if let Some(value) = parent.remove(old) {
            parent.entry(new).or_insert(value);
            let prefix: String = parent_path.iter().map(|s| format!("{s}.")).collect();
            warnings.push(ConfigWarning::RenamedKey {
                file: path.to_path_buf(),
                old: format!("{prefix}{old}"),
                new: format!("{prefix}{new}"),
            });
        }
    }
}

/// Recursively merge `overlay` into `base`.  Keys in `overlay` take
/// precedence; sub-tables are merged recursively rather than replaced
/// wholesale so that a project config section inherits individual
/// keys from the global config.
fn merge_toml(base: &mut toml::Table, overlay: toml::Table) {
    for (key, overlay_val) in overlay {
        match overlay_val {
            toml::Value::Table(overlay_table)
                if matches!(base.get(&key), Some(toml::Value::Table(_))) =>
            {
                if let Some(toml::Value::Table(base_table)) = base.get_mut(&key) {
                    merge_toml(base_table, overlay_table);
                }
            }
            val => {
                base.insert(key, val);
            }
        }
    }
}

/// The config file name that PHPantom looks for in the project root.
pub const CONFIG_FILE_NAME: &str = ".phpantom.toml";

/// The subdirectory under the user's XDG config directory.
const CONFIG_APP_DIR: &str = "phpantom_lsp";

/// Header shared by every generated `.phpantom.toml`: enables
/// schema-aware editor tooling and points at the docs. `init_wizard`
/// reuses this so an interactively-built config still starts the same
/// way as the blank one below.
pub(crate) const CONFIG_HEADER: &str = r#"#:schema https://github.com/PHPantom-dev/phpantom_lsp/raw/main/config-schema.json

# PHPantom configuration: only add settings you want to override.
# Editors with TOML schema support (Zed, VS Code + Even Better TOML, Neovim)
# provide autocomplete and hover documentation for all available options.
# Full reference: https://phpantom-dev.github.io/phpantom_lsp/configuration/
"#;

/// Default content for a newly created `.phpantom.toml` file.
pub const DEFAULT_CONFIG_CONTENT: &str = CONFIG_HEADER;

/// Return the path to the global config file, if the platform's config
/// directory can be determined.
///
/// `$XDG_CONFIG_HOME/phpantom_lsp/.phpantom.toml`, defaulting to
/// `~/.config/phpantom_lsp/.phpantom.toml`, on Linux and macOS alike, and
/// `%APPDATA%\phpantom_lsp\.phpantom.toml` on Windows.  macOS deliberately
/// follows the XDG path rather than `~/Library/Application Support`, which
/// is where a command-line tool's config is expected to be; changing it
/// would move every existing user's config out from under them, so
/// `choose_base_strategy` is the intended call here rather than
/// `choose_app_strategy`.
pub fn global_config_path() -> Option<PathBuf> {
    etcetera::choose_base_strategy()
        .ok()
        .map(|s| s.config_dir().join(CONFIG_APP_DIR).join(CONFIG_FILE_NAME))
}

/// Create a default `.phpantom.toml` in the given workspace root.
///
/// Returns `Ok(true)` if the file was created, `Ok(false)` if it
/// already exists, or `Err` on I/O failure.
pub fn create_default_config(workspace_root: &Path) -> Result<bool, ConfigError> {
    write_default_config(&workspace_root.join(CONFIG_FILE_NAME))
}

/// Create a default `.phpantom.toml` in the user's global config
/// directory, creating that directory when it does not exist yet.
///
/// Returns the path along with `true` if the file was created, or
/// `false` if it already existed.
pub fn create_global_config() -> Result<(bool, PathBuf), ConfigError> {
    let config_path = global_config_path().ok_or(ConfigError::NoConfigDir)?;
    let created = write_default_config(&config_path)?;
    Ok((created, config_path))
}

/// Same as [`create_default_config`], but with caller-supplied content
/// (e.g. the answers `init_wizard` collected) instead of the blank
/// starter config.
pub fn create_default_config_with_content(
    workspace_root: &Path,
    content: &str,
) -> Result<bool, ConfigError> {
    write_config_content(&workspace_root.join(CONFIG_FILE_NAME), content)
}

/// Same as [`create_global_config`], but with caller-supplied content.
pub fn create_global_config_with_content(content: &str) -> Result<(bool, PathBuf), ConfigError> {
    let config_path = global_config_path().ok_or(ConfigError::NoConfigDir)?;
    let created = write_config_content(&config_path, content)?;
    Ok((created, config_path))
}

/// Write the starter config to `config_path`, creating any missing
/// parent directories.  Returns `false` without touching anything when
/// the file is already there.
fn write_default_config(config_path: &Path) -> Result<bool, ConfigError> {
    write_config_content(config_path, DEFAULT_CONFIG_CONTENT)
}

/// Write `content` to `config_path`, creating any missing parent
/// directories.  Returns `false` without touching anything when the
/// file is already there.
fn write_config_content(config_path: &Path, content: &str) -> Result<bool, ConfigError> {
    if config_path.exists() {
        return Ok(false);
    }

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ConfigError::Io {
            path: parent.display().to_string(),
            source: e,
        })?;
    }

    std::fs::write(config_path, content).map_err(|e| ConfigError::Io {
        path: config_path.display().to_string(),
        source: e,
    })?;

    Ok(true)
}

fn load_toml_table(
    path: &Path,
    warnings: &mut Vec<ConfigWarning>,
) -> Result<Option<toml::Table>, ConfigError> {
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.display().to_string(),
        source: e,
    })?;

    let mut table: toml::Table = content.parse().map_err(|e| ConfigError::Parse {
        path: path.display().to_string(),
        source: e,
    })?;
    upgrade_renamed_keys(&mut table, path, warnings);

    Ok(Some(table))
}

/// Something in a config file worth telling the user about.  Never an
/// error: a config written for a newer release, or in an older
/// spelling, still loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigWarning {
    /// A key no setting reads, so it has no effect.
    UnknownKey {
        /// The file the key was written in.
        file: PathBuf,
        /// The key's dotted path, e.g. `diagnostics.unused-parameter`.
        key: String,
    },
    /// A key written in the spelling an earlier release used.  It still
    /// applies, under its current name.
    RenamedKey {
        /// The file the key was written in.
        file: PathBuf,
        /// The old dotted path, e.g. `semantic_tokens`.
        old: String,
        /// The current dotted path, e.g. `semantic-tokens`.
        new: String,
    },
}

impl std::fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigWarning::UnknownKey { file, key } => {
                write!(f, "unknown key `{key}` in {} is ignored", file.display())
            }
            ConfigWarning::RenamedKey { file, old, new } => write!(
                f,
                "`{old}` in {} has been renamed to `{new}`",
                file.display()
            ),
        }
    }
}

/// A loaded configuration together with the warnings its files raised.
#[derive(Debug, Clone, Default)]
pub struct LoadedConfig {
    /// The merged configuration.
    pub config: Config,
    /// Warnings in the order they were found, global layer first.
    pub warnings: Vec<ConfigWarning>,
}

/// Deserialize one config layer, recording every key no setting reads
/// against `file`.
fn deserialize_layer(
    table: toml::Table,
    file: &Path,
    warnings: &mut Vec<ConfigWarning>,
) -> Result<Config, ConfigError> {
    serde_ignored::deserialize(toml::Value::Table(table), |path| {
        warnings.push(ConfigWarning::UnknownKey {
            file: file.to_path_buf(),
            key: path.to_string(),
        });
    })
    .map_err(|e| ConfigError::Parse {
        path: file.display().to_string(),
        source: e,
    })
}

/// Load the project configuration, merging the global config layer at
/// `global_path` with the project-level `.phpantom.toml`.
///
/// Project settings override global settings.  When neither file exists,
/// returns `Config::default()`.
///
/// `global_path` of `None` skips the global layer entirely, leaving the
/// project's own `.phpantom.toml` (or the built-in defaults) as the only
/// source of settings.  The location is always passed in rather than
/// read from [`global_config_path`] here, so that a test (or any other
/// isolated run) cannot be steered by whatever happens to sit in the
/// config directory of whoever is running it.
pub fn load_config_from(
    workspace_root: &Path,
    global_path: Option<&Path>,
) -> Result<Config, ConfigError> {
    load_config_with_warnings(workspace_root, global_path).map(|loaded| loaded.config)
}

/// [`load_config_from`], also returning the keys either file sets that
/// no setting reads or that use an old spelling, so the caller can warn
/// about them.
pub fn load_config_with_warnings(
    workspace_root: &Path,
    global_path: Option<&Path>,
) -> Result<LoadedConfig, ConfigError> {
    let mut warnings = Vec::new();

    // Each layer is deserialized on its own before merging, so a bad
    // value or an unknown key is reported against the file it was
    // actually written in.
    let mut table = toml::Table::new();
    let mut config = None;
    if let Some(path) = global_path
        && let Some(global) = load_toml_table(path, &mut warnings)?
        && !global.is_empty()
    {
        config = Some(deserialize_layer(global.clone(), path, &mut warnings)?);
        table = global;
    }

    let project_path = workspace_root.join(CONFIG_FILE_NAME);
    if let Some(project) = load_toml_table(&project_path, &mut warnings)? {
        if table.is_empty() {
            config = Some(deserialize_layer(project, &project_path, &mut warnings)?);
        } else {
            deserialize_layer(project.clone(), &project_path, &mut warnings)?;
            merge_toml(&mut table, project);
            config = Some(table.try_into().map_err(|e| ConfigError::Parse {
                path: project_path.display().to_string(),
                source: e,
            })?);
        }
    }

    Ok(LoadedConfig {
        config: config.unwrap_or_default(),
        warnings,
    })
}

/// Errors that can occur when loading the config file.
#[derive(Debug)]
pub enum ConfigError {
    /// Failed to read the config file from disk.
    Io {
        /// Path that was attempted.
        path: String,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The config file contains invalid TOML or does not match the schema.
    Parse {
        /// Path that was attempted.
        path: String,
        /// The underlying TOML parse error.
        source: toml::de::Error,
    },
    /// The platform's user config directory could not be determined, so
    /// the global config has no home.
    NoConfigDir,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io { path, source } => {
                write!(f, "failed to read {}: {}", path, source)
            }
            ConfigError::Parse { path, source } => {
                write!(f, "failed to parse {}: {}", path, source)
            }
            ConfigError::NoConfigDir => {
                write!(f, "cannot determine the user config directory")
            }
        }
    }
}

impl std::error::Error for ConfigError {}
