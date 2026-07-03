//! Configuration loaded from `.phpantom.toml`.
//!
//! Settings are read from two locations (in order of precedence):
//!
//! 1. **Project** — `.phpantom.toml` in the workspace root (next to
//!    `composer.json`).
//! 2. **Global** — `$XDG_CONFIG_HOME/phpantom_lsp/.phpantom.toml`
//!    (typically `~/.config/phpantom_lsp/.phpantom.toml` on Linux).
//!
//! Project settings override global settings.  When neither file
//! exists, all settings use their defaults.

use std::path::{Path, PathBuf};

use etcetera::BaseStrategy as _;
use serde::Deserialize;

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
    /// Formatting proxy settings.
    pub formatting: FormattingConfig,
    /// PHPStan proxy settings.
    pub phpstan: PhpStanConfig,
    /// PHPCS (PHP_CodeSniffer) proxy settings.
    pub phpcs: PhpcsConfig,
    /// Mago proxy settings.
    pub mago: MagoConfig,
}

/// `[php]` section — PHP version override.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PhpConfig {
    /// Override the detected PHP version (e.g. `"8.3"`).
    /// When `None`, PHPantom infers from `composer.json`.
    pub version: Option<String>,
}

/// `[diagnostics]` section — toggle individual diagnostic providers.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DiagnosticsConfig {
    /// Report member access on subjects whose type could not be resolved.
    ///
    /// Off by default. When enabled, PHPantom emits a hint-level
    /// diagnostic on every `->`, `?->`, or `::` access where the
    /// subject type is unknown (e.g. `mixed`, untyped variable, or a
    /// return type PHPantom cannot infer). This is useful for
    /// discovering gaps in type coverage but produces too many
    /// diagnostics on codebases without comprehensive type annotations.
    #[serde(rename = "unresolved-member-access")]
    pub unresolved_member_access: Option<bool>,

    /// Report calls that pass more arguments than the function accepts.
    ///
    /// Off by default. PHP does not error on extra arguments to
    /// user-defined functions (the extras are silently ignored), and
    /// many libraries exploit this for flexible APIs. Enable this if
    /// you want stricter checking.
    #[serde(rename = "extra-arguments")]
    pub extra_arguments: Option<bool>,

    /// Report property access on classes with `__get` when virtual
    /// properties are defined.
    ///
    /// Off by default. When enabled, classes that have `__get` but
    /// also declare virtual properties (via `@property` docblock tags,
    /// Laravel Eloquent column inference, or any other virtual member
    /// provider) will flag unknown property access instead of
    /// suppressing it. This matches PHPStan's `reportMagicProperties`
    /// behaviour.
    #[serde(rename = "report-magic-properties")]
    pub report_magic_properties: Option<bool>,

    /// Rules that suppress matching diagnostics, similar to PHPStan's
    /// `ignoreErrors`.
    ///
    /// Each rule may constrain by `message` (regex), `path` (glob,
    /// relative to the workspace root), and/or `identifier` (the
    /// diagnostic code, e.g. `"unused_import"`). A diagnostic is
    /// suppressed when it matches every constraint present on a rule;
    /// omitted constraints match anything. A rule with no constraints
    /// at all is rejected (it would silently suppress every
    /// diagnostic in the project).
    ///
    /// ```toml
    /// [[diagnostics.ignore]]
    /// path = "tests/**"
    ///
    /// [[diagnostics.ignore]]
    /// identifier = "deprecated_usage"
    /// message = "^Call to deprecated function some_legacy_helper\\(\\)"
    /// ```
    pub ignore: Vec<IgnoreRule>,
}

/// A single `[[diagnostics.ignore]]` rule.
///
/// See [`DiagnosticsConfig::ignore`] for the matching semantics.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct IgnoreRule {
    /// Regex matched against the diagnostic message.
    pub message: Option<String>,
    /// Glob matched against the file path, relative to the workspace
    /// root. Follows gitignore-style glob syntax: `*` does not cross
    /// `/`, use `**` to match across directories.
    pub path: Option<String>,
    /// Diagnostic identifier (the diagnostic `code`, e.g.
    /// `"unused_import"`, `"deprecated_usage"`).
    pub identifier: Option<String>,
}

impl DiagnosticsConfig {
    /// Whether the unresolved-member-access diagnostic is enabled.
    ///
    /// Defaults to `false` (off) when not explicitly set.
    pub fn unresolved_member_access_enabled(&self) -> bool {
        self.unresolved_member_access.unwrap_or(false)
    }

    /// Whether the extra-arguments diagnostic is enabled.
    ///
    /// Defaults to `false` (off) when not explicitly set.
    pub fn extra_arguments_enabled(&self) -> bool {
        self.extra_arguments.unwrap_or(false)
    }

    /// Whether magic property reporting is enabled.
    ///
    /// Defaults to `false` (off) when not explicitly set.
    pub fn report_magic_properties_enabled(&self) -> bool {
        self.report_magic_properties.unwrap_or(false)
    }
}

/// `[formatting]` section — controls the formatting strategy.
///
/// PHPantom ships a built-in PHP formatter (mago-formatter) that works
/// out of the box with PER-CS 2.0 defaults.  Projects that list
/// `friendsofphp/php-cs-fixer` or `squizlabs/php_codesniffer` in their
/// `composer.json` `require-dev` automatically use those external tools
/// instead (resolved via Composer's bin-dir).
///
/// Explicit configuration in `.phpantom.toml` always takes priority:
/// set a tool path to use it, or set it to `""` to disable it.
/// When no external tool is configured or detected, the built-in
/// formatter is used.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FormattingConfig {
    /// Command (path or name) to run php-cs-fixer.
    ///
    /// - `None` (default) — check `require-dev` in `composer.json`;
    ///   if absent, fall back to the built-in formatter.
    /// - `""` — disable php-cs-fixer.
    /// - Any other value — use as the command (e.g.
    ///   `"/usr/local/bin/php-cs-fixer"` or `"php-cs-fixer"`).
    #[serde(rename = "php-cs-fixer")]
    pub php_cs_fixer: Option<String>,
    /// Command (path or name) to run phpcbf.
    ///
    /// - `None` (default) — check `require-dev` in `composer.json`;
    ///   if absent, fall back to the built-in formatter.
    /// - `""` — disable phpcbf.
    /// - Any other value — use as the command.
    pub phpcbf: Option<String>,
    /// Command (path or name) to run Laravel Pint.
    ///
    /// - `None` (default) — check `require-dev` in `composer.json`;
    ///   if absent, fall back to the built-in formatter.
    /// - `""` — disable pint.
    /// - Any other value — use as the command.
    pub pint: Option<String>,
    /// Maximum runtime in milliseconds before each formatter is killed.
    /// Defaults to 10 000 ms (10 seconds).  Applied per tool, not
    /// for the combined pipeline.
    pub timeout: Option<u64>,
}

impl FormattingConfig {
    /// Return the configured timeout in milliseconds, falling back to
    /// 10 000 ms when unset.
    pub fn timeout_ms(&self) -> u64 {
        self.timeout.unwrap_or(10_000)
    }

    /// Whether formatting is entirely disabled (all tools explicitly
    /// set to empty strings).
    pub fn is_disabled(&self) -> bool {
        self.php_cs_fixer.as_deref() == Some("")
            && self.phpcbf.as_deref() == Some("")
            && self.pint.as_deref() == Some("")
    }
}

/// `[phpstan]` section — controls the external PHPStan proxy.
///
/// PHPantom can run PHPStan in "editor mode" (`--tmp-file` /
/// `--instead-of`) on each file save to surface static analysis
/// errors as LSP diagnostics.
///
/// When `command` is unset (`None`), PHPantom auto-detects via
/// `vendor/bin/phpstan` then `$PATH`.  Set to `""` (empty string)
/// to explicitly disable PHPStan integration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PhpStanConfig {
    /// Command (path or name) to run PHPStan.
    ///
    /// - `None` (default) — auto-detect `vendor/bin/phpstan`,
    ///   then `phpstan` on `$PATH`.
    /// - `""` — disable PHPStan.
    /// - Any other value — use as the command (e.g.
    ///   `"/usr/local/bin/phpstan"` or `"phpstan"`).
    pub command: Option<String>,
    /// Memory limit passed to PHPStan via `--memory-limit`.
    /// Defaults to `"1G"` when unset.
    #[serde(rename = "memory-limit")]
    pub memory_limit: Option<String>,
    /// Maximum runtime in milliseconds before PHPStan is killed.
    /// Defaults to 60 000 ms (60 seconds).
    pub timeout: Option<u64>,
}

impl PhpStanConfig {
    /// Return the configured timeout in milliseconds, falling back to
    /// 60 000 ms when unset.
    pub fn timeout_ms(&self) -> u64 {
        self.timeout.unwrap_or(60_000)
    }

    /// Whether PHPStan is explicitly disabled (command set to empty
    /// string).
    pub fn is_disabled(&self) -> bool {
        self.command.as_deref() == Some("")
    }
}

/// `[mago]` section — Mago proxy settings.
///
/// Mago is only activated when `mago.toml` exists at the workspace
/// root.  When `command` is unset (`None`), PHPantom auto-detects via
/// `vendor/bin/mago`, then `mago` on `$PATH`.  Set to `""` (empty
/// string) to explicitly disable Mago integration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MagoConfig {
    /// Command (path or name) to run Mago.
    ///
    /// - `None` (default) — auto-detect `vendor/bin/mago`,
    ///   then `mago` on `$PATH`.
    /// - `""` — disable Mago.
    /// - Any other value — use as the command.
    pub command: Option<String>,
    /// Maximum runtime in milliseconds before `mago lint` is killed.
    /// Defaults to 30 000 ms (30 seconds).
    #[serde(rename = "lint-timeout")]
    pub lint_timeout: Option<u64>,
    /// Maximum runtime in milliseconds before `mago analyze` is killed.
    /// Defaults to 60 000 ms (60 seconds).
    #[serde(rename = "analyze-timeout")]
    pub analyze_timeout: Option<u64>,
}

impl MagoConfig {
    /// Return the configured lint timeout in milliseconds, falling back
    /// to 30 000 ms when unset.
    pub fn lint_timeout_ms(&self) -> u64 {
        self.lint_timeout.unwrap_or(30_000)
    }

    /// Return the configured analyze timeout in milliseconds, falling
    /// back to 60 000 ms when unset.
    pub fn analyze_timeout_ms(&self) -> u64 {
        self.analyze_timeout.unwrap_or(60_000)
    }

    /// Whether Mago is explicitly disabled (command set to empty
    /// string).
    pub fn is_disabled(&self) -> bool {
        self.command.as_deref() == Some("")
    }
}

/// `[phpcs]` section — PHP_CodeSniffer proxy settings.
///
/// When `command` is unset (`None`), PHPantom auto-detects via
/// `vendor/bin/phpcs` then `$PATH`.  Set to `""` (empty string)
/// to explicitly disable PHPCS integration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PhpcsConfig {
    /// Command (path or name) to run PHPCS.
    ///
    /// - `None` (default) — auto-detect `vendor/bin/phpcs`,
    ///   then `phpcs` on `$PATH`.
    /// - `""` — disable PHPCS.
    /// - Any other value — use as the command (e.g.
    ///   `"vendor/bin/phpcs"` or `"phpcs"`).
    pub command: Option<String>,
    /// Coding standard to enforce (e.g. `"PSR12"`).
    ///
    /// When unset, PHPCS uses its own default detection
    /// (`phpcs.xml` / `phpcs.xml.dist` in the project root,
    /// then its built-in default).
    pub standard: Option<String>,
    /// Maximum runtime in milliseconds before PHPCS is killed.
    /// Defaults to 30 000 ms (30 seconds).
    pub timeout: Option<u64>,
}

impl PhpcsConfig {
    /// Return the configured timeout in milliseconds, falling back to
    /// 30 000 ms when unset.
    pub fn timeout_ms(&self) -> u64 {
        self.timeout.unwrap_or(30_000)
    }

    /// Whether PHPCS is explicitly disabled (command set to empty
    /// string).
    pub fn is_disabled(&self) -> bool {
        self.command.as_deref() == Some("")
    }
}

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
}

impl IndexingConfig {
    pub fn strategy(&self) -> IndexingStrategy {
        self.strategy.unwrap_or_default()
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

/// Default content for a newly created `.phpantom.toml` file.
pub const DEFAULT_CONFIG_CONTENT: &str = r#"# $schema: https://github.com/PHPantom-dev/phpantom_lsp/raw/main/config-schema.json
"#;

/// Return the path to the global config file, if the platform's config
/// directory can be determined.
///
/// On Linux this is typically `$XDG_CONFIG_HOME/phpantom/.phpantom.toml`
/// (defaulting to `~/.config/phpantom_lsp/.phpantom.toml`).
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
    let config_path = workspace_root.join(CONFIG_FILE_NAME);

    if config_path.exists() {
        return Ok(false);
    }

    std::fs::write(&config_path, DEFAULT_CONFIG_CONTENT).map_err(|e| ConfigError::Io {
        path: config_path.display().to_string(),
        source: e,
    })?;

    Ok(true)
}

fn load_toml_table(path: &Path) -> Result<Option<toml::Table>, ConfigError> {
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.display().to_string(),
        source: e,
    })?;

    let table: toml::Table = content.parse().map_err(|e| ConfigError::Parse {
        path: path.display().to_string(),
        source: e,
    })?;

    Ok(Some(table))
}

/// Load the project configuration, merging the global config (from the
/// user's XDG config directory) with the project-level `.phpantom.toml`.
///
/// Project settings override global settings.  When neither file exists,
/// returns `Config::default()`.
pub fn load_config(workspace_root: &Path) -> Result<Config, ConfigError> {
    let mut table = global_config_path()
        .and_then(|p| load_toml_table(&p).transpose())
        .transpose()?
        .unwrap_or_default();

    let project_path = workspace_root.join(CONFIG_FILE_NAME);
    if let Some(project) = load_toml_table(&project_path)? {
        merge_toml(&mut table, project);
    }

    let config: Config = table.try_into().map_err(|e| ConfigError::Parse {
        path: project_path.display().to_string(),
        source: e,
    })?;

    Ok(config)
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
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn create_default_writes_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = create_default_config(dir.path()).unwrap();
        assert!(result, "should report that the file was created");
        let path = dir.path().join(CONFIG_FILE_NAME);
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("$schema"));
    }

    #[test]
    fn create_default_does_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "# custom\n").unwrap();
        let result = create_default_config(dir.path()).unwrap();
        assert!(!result, "should report that the file already exists");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            content, "# custom\n",
            "existing file must not be overwritten"
        );
    }

    #[test]
    fn default_content_parses_successfully() {
        let config: Config = toml::from_str(DEFAULT_CONFIG_CONTENT).unwrap();
        assert!(config.php.version.is_none());
        assert!(!config.diagnostics.unresolved_member_access_enabled());
        assert!(!config.diagnostics.extra_arguments_enabled());
        assert!(!config.diagnostics.report_magic_properties_enabled());
        assert!(config.diagnostics.ignore.is_empty());
        assert_eq!(config.indexing.strategy(), IndexingStrategy::Full);
        assert!(config.formatting.php_cs_fixer.is_none());
        assert!(config.formatting.phpcbf.is_none());
        assert!(config.formatting.timeout.is_none());
        assert_eq!(config.formatting.timeout_ms(), 10_000);
        assert!(config.phpstan.command.is_none());
        assert!(config.phpstan.memory_limit.is_none());
        assert!(config.phpstan.timeout.is_none());
        assert_eq!(config.phpstan.timeout_ms(), 60_000);
        assert!(config.phpcs.command.is_none());
        assert!(config.phpcs.standard.is_none());
        assert!(config.phpcs.timeout.is_none());
        assert_eq!(config.phpcs.timeout_ms(), 30_000);
        assert!(config.mago.command.is_none());
        assert!(config.mago.lint_timeout.is_none());
        assert!(config.mago.analyze_timeout.is_none());
        assert_eq!(config.mago.lint_timeout_ms(), 30_000);
        assert_eq!(config.mago.analyze_timeout_ms(), 60_000);
        assert!(!config.mago.is_disabled());
    }

    #[test]
    fn missing_file_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(config.php.version.is_none());
        assert!(!config.diagnostics.unresolved_member_access_enabled());
        assert!(!config.diagnostics.extra_arguments_enabled());
        assert!(!config.diagnostics.report_magic_properties_enabled());
        assert!(config.diagnostics.ignore.is_empty());
        assert_eq!(config.indexing.strategy(), IndexingStrategy::Full);
        assert!(config.formatting.php_cs_fixer.is_none());
        assert!(config.formatting.phpcbf.is_none());
        assert!(config.phpstan.command.is_none());
        assert!(config.phpcs.command.is_none());
        assert!(config.mago.command.is_none());
    }

    #[test]
    fn empty_file_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(config.php.version.is_none());
        assert!(!config.diagnostics.unresolved_member_access_enabled());
        assert!(!config.diagnostics.extra_arguments_enabled());
        assert!(!config.diagnostics.report_magic_properties_enabled());
        assert_eq!(config.indexing.strategy(), IndexingStrategy::Full);
        assert!(config.formatting.php_cs_fixer.is_none());
        assert!(config.formatting.phpcbf.is_none());
        assert!(config.phpstan.command.is_none());
        assert!(config.phpcs.command.is_none());
        assert!(config.mago.command.is_none());
    }

    #[test]
    fn parses_php_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[php]\nversion = \"8.3\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.php.version.as_deref(), Some("8.3"));
    }

    #[test]
    fn parses_diagnostics_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[diagnostics]\nunresolved-member-access = true\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(config.diagnostics.unresolved_member_access_enabled());
    }

    #[test]
    fn unresolved_member_access_defaults_to_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[diagnostics]\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(!config.diagnostics.unresolved_member_access_enabled());
    }

    #[test]
    fn parses_report_magic_properties() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[diagnostics]\nreport-magic-properties = true\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(config.diagnostics.report_magic_properties_enabled());
    }

    #[test]
    fn report_magic_properties_defaults_to_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[diagnostics]\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(!config.diagnostics.report_magic_properties_enabled());
    }

    #[test]
    fn extra_arguments_defaults_to_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[diagnostics]\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(!config.diagnostics.extra_arguments_enabled());
    }

    #[test]
    fn parses_extra_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[diagnostics]\nextra-arguments = true\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(config.diagnostics.extra_arguments_enabled());
    }

    #[test]
    fn diagnostics_ignore_defaults_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[diagnostics]\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert!(config.diagnostics.ignore.is_empty());
    }

    #[test]
    fn parses_diagnostics_ignore_rule() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(
            &path,
            r#"
[[diagnostics.ignore]]
message = "^Call to deprecated function some_legacy_helper\\(\\)"
path = "tests/**"
identifier = "deprecated_usage"
"#,
        )
        .unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.diagnostics.ignore.len(), 1);
        let rule = &config.diagnostics.ignore[0];
        assert_eq!(
            rule.message.as_deref(),
            Some("^Call to deprecated function some_legacy_helper\\(\\)")
        );
        assert_eq!(rule.path.as_deref(), Some("tests/**"));
        assert_eq!(rule.identifier.as_deref(), Some("deprecated_usage"));
    }

    #[test]
    fn parses_multiple_diagnostics_ignore_rules_with_partial_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(
            &path,
            r#"
[[diagnostics.ignore]]
path = "vendor/**"

[[diagnostics.ignore]]
identifier = "unused_variable"
"#,
        )
        .unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.diagnostics.ignore.len(), 2);
        assert_eq!(
            config.diagnostics.ignore[0].path.as_deref(),
            Some("vendor/**")
        );
        assert!(config.diagnostics.ignore[0].message.is_none());
        assert!(config.diagnostics.ignore[0].identifier.is_none());
        assert_eq!(
            config.diagnostics.ignore[1].identifier.as_deref(),
            Some("unused_variable")
        );
        assert!(config.diagnostics.ignore[1].path.is_none());
    }

    #[test]
    fn invalid_toml_returns_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[diagnostics\nbroken").unwrap();
        let result = load_config(dir.path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("failed to parse"));
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "[diagnostics]").unwrap();
        writeln!(f, "unresolved-member-access = true").unwrap();
        writeln!(f, "some-future-tool = false").unwrap();
        drop(f);
        // Unknown keys should NOT cause a parse error — forward compatibility.
        let config = load_config(dir.path()).unwrap();
        assert!(config.diagnostics.unresolved_member_access_enabled());
    }

    #[test]
    fn unknown_sections_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(
            &path,
            "[php]\nversion = \"8.4\"\n\n[some-future-section]\nkey = \"value\"\n",
        )
        .unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.php.version.as_deref(), Some("8.4"));
    }

    #[test]
    fn parses_phpstan_command() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[phpstan]\ncommand = \"/usr/bin/phpstan\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.phpstan.command.as_deref(), Some("/usr/bin/phpstan"));
    }

    #[test]
    fn parses_phpstan_memory_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[phpstan]\nmemory-limit = \"2G\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.phpstan.memory_limit.as_deref(), Some("2G"));
    }

    #[test]
    fn parses_phpstan_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[phpstan]\ntimeout = 30000\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.phpstan.timeout_ms(), 30_000);
    }

    #[test]
    fn phpstan_empty_string_disables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[phpstan]\ncommand = \"\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.phpstan.command.as_deref(), Some(""));
        assert!(config.phpstan.is_disabled());
    }

    #[test]
    fn phpstan_defaults() {
        let config = Config::default();
        assert!(config.phpstan.command.is_none());
        assert!(config.phpstan.memory_limit.is_none());
        assert!(config.phpstan.timeout.is_none());
        assert_eq!(config.phpstan.timeout_ms(), 60_000);
        assert!(!config.phpstan.is_disabled());
    }

    #[test]
    fn full_example_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(
            &path,
            r#"
[php]
version = "8.2"

[diagnostics]
unresolved-member-access = true
extra-arguments = true
report-magic-properties = true

[[diagnostics.ignore]]
path = "tests/**"

[[diagnostics.ignore]]
identifier = "deprecated_usage"
message = "^Call to deprecated function some_legacy_helper\\(\\)"

[indexing]
strategy = "self"

[formatting]
php-cs-fixer = ""
phpcbf = "/usr/local/bin/phpcbf"
timeout = 5000

[phpstan]
command = "/usr/local/bin/phpstan"
memory-limit = "2G"
timeout = 30000

[phpcs]
command = "/usr/local/bin/phpcs"
standard = "PSR12"
timeout = 15000

[mago]
command = "/usr/local/bin/mago"
lint-timeout = 15000
analyze-timeout = 45000
"#,
        )
        .unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.php.version.as_deref(), Some("8.2"));
        assert!(config.diagnostics.unresolved_member_access_enabled());
        assert!(config.diagnostics.extra_arguments_enabled());
        assert!(config.diagnostics.report_magic_properties_enabled());
        assert_eq!(config.diagnostics.ignore.len(), 2);
        assert_eq!(
            config.diagnostics.ignore[0].path.as_deref(),
            Some("tests/**")
        );
        assert_eq!(
            config.diagnostics.ignore[1].identifier.as_deref(),
            Some("deprecated_usage")
        );
        assert_eq!(config.indexing.strategy, Some(IndexingStrategy::SelfScan));
        assert_eq!(config.formatting.php_cs_fixer.as_deref(), Some(""));
        assert_eq!(
            config.formatting.phpcbf.as_deref(),
            Some("/usr/local/bin/phpcbf")
        );
        assert_eq!(config.formatting.timeout_ms(), 5000);
        assert_eq!(
            config.phpstan.command.as_deref(),
            Some("/usr/local/bin/phpstan")
        );
        assert_eq!(config.phpstan.memory_limit.as_deref(), Some("2G"));
        assert_eq!(config.phpstan.timeout_ms(), 30_000);
        assert_eq!(
            config.phpcs.command.as_deref(),
            Some("/usr/local/bin/phpcs")
        );
        assert_eq!(config.phpcs.standard.as_deref(), Some("PSR12"));
        assert_eq!(config.phpcs.timeout_ms(), 15_000);
        assert_eq!(config.mago.command.as_deref(), Some("/usr/local/bin/mago"));
        assert_eq!(config.mago.lint_timeout_ms(), 15_000);
        assert_eq!(config.mago.analyze_timeout_ms(), 45_000);
        assert!(!config.mago.is_disabled());
    }

    #[test]
    fn parses_indexing_strategy_composer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[indexing]\nstrategy = \"composer\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.indexing.strategy, Some(IndexingStrategy::Composer));
    }

    #[test]
    fn parses_indexing_strategy_self() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[indexing]\nstrategy = \"self\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.indexing.strategy, Some(IndexingStrategy::SelfScan));
    }

    #[test]
    fn parses_indexing_strategy_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[indexing]\nstrategy = \"full\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.indexing.strategy, Some(IndexingStrategy::Full));
    }

    #[test]
    fn parses_indexing_strategy_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[indexing]\nstrategy = \"none\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.indexing.strategy, Some(IndexingStrategy::None));
    }

    #[test]
    fn invalid_indexing_strategy_returns_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[indexing]\nstrategy = \"bogus\"\n").unwrap();
        let result = load_config(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn indexing_strategy_defaults_to_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[indexing]\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.indexing.strategy(), IndexingStrategy::Full);
    }

    #[test]
    fn indexing_strategy_display() {
        assert_eq!(IndexingStrategy::Composer.to_string(), "composer");
        assert_eq!(IndexingStrategy::SelfScan.to_string(), "self");
        assert_eq!(IndexingStrategy::Full.to_string(), "full");
        assert_eq!(IndexingStrategy::None.to_string(), "none");
    }

    #[test]
    fn parses_formatting_php_cs_fixer_command() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(
            &path,
            "[formatting]\nphp-cs-fixer = \"/usr/bin/php-cs-fixer\"\n",
        )
        .unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(
            config.formatting.php_cs_fixer.as_deref(),
            Some("/usr/bin/php-cs-fixer")
        );
        assert!(config.formatting.phpcbf.is_none());
    }

    #[test]
    fn parses_formatting_phpcbf_command() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[formatting]\nphpcbf = \"vendor/bin/phpcbf\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(
            config.formatting.phpcbf.as_deref(),
            Some("vendor/bin/phpcbf")
        );
        assert!(config.formatting.php_cs_fixer.is_none());
    }

    #[test]
    fn parses_formatting_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[formatting]\ntimeout = 3000\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.formatting.timeout_ms(), 3000);
    }

    #[test]
    fn formatting_empty_string_disables_tool() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(
            &path,
            "[formatting]\nphp-cs-fixer = \"\"\nphpcbf = \"\"\npint = \"\"\n",
        )
        .unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.formatting.php_cs_fixer.as_deref(), Some(""));
        assert_eq!(config.formatting.phpcbf.as_deref(), Some(""));
        assert_eq!(config.formatting.pint.as_deref(), Some(""));
        assert!(config.formatting.is_disabled());
    }

    #[test]
    fn formatting_defaults() {
        let config = Config::default();
        assert!(config.formatting.php_cs_fixer.is_none());
        assert!(config.formatting.phpcbf.is_none());
        assert!(config.formatting.timeout.is_none());
        assert_eq!(config.formatting.timeout_ms(), 10_000);
        assert!(!config.formatting.is_disabled());
    }

    #[test]
    fn parses_phpcs_command() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[phpcs]\ncommand = \"vendor/bin/phpcs\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.phpcs.command.as_deref(), Some("vendor/bin/phpcs"));
    }

    #[test]
    fn parses_phpcs_standard() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[phpcs]\nstandard = \"PSR1\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.phpcs.standard.as_deref(), Some("PSR1"));
    }

    #[test]
    fn parses_phpcs_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[phpcs]\ntimeout = 20000\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.phpcs.timeout_ms(), 20_000);
    }

    #[test]
    fn phpcs_empty_string_disables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, "[phpcs]\ncommand = \"\"\n").unwrap();
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.phpcs.command.as_deref(), Some(""));
        assert!(config.phpcs.is_disabled());
    }

    #[test]
    fn phpcs_defaults() {
        let config = Config::default();
        assert!(config.phpcs.command.is_none());
        assert!(config.phpcs.standard.is_none());
        assert!(config.phpcs.timeout.is_none());
        assert_eq!(config.phpcs.timeout_ms(), 30_000);
        assert!(!config.phpcs.is_disabled());
    }

    #[test]
    fn merge_toml_overlay_wins() {
        let mut base: toml::Table = toml::from_str("[php]\nversion = \"8.2\"\n").unwrap();
        let overlay: toml::Table = toml::from_str("[php]\nversion = \"8.4\"\n").unwrap();
        merge_toml(&mut base, overlay);
        let config: Config = base.try_into().unwrap();
        assert_eq!(config.php.version.as_deref(), Some("8.4"));
    }

    #[test]
    fn merge_toml_base_preserved_when_overlay_missing() {
        let mut base: toml::Table =
            toml::from_str("[php]\nversion = \"8.2\"\n\n[phpstan]\ntimeout = 30000\n").unwrap();
        let overlay: toml::Table = toml::from_str("[phpstan]\ncommand = \"phpstan\"\n").unwrap();
        merge_toml(&mut base, overlay);
        let config: Config = base.try_into().unwrap();
        assert_eq!(config.php.version.as_deref(), Some("8.2"));
        assert_eq!(config.phpstan.command.as_deref(), Some("phpstan"));
        assert_eq!(config.phpstan.timeout_ms(), 30_000);
    }

    #[test]
    fn merge_toml_deep_merge_within_section() {
        let mut base: toml::Table =
            toml::from_str("[formatting]\ntimeout = 5000\nphpcbf = \"/usr/bin/phpcbf\"\n").unwrap();
        let overlay: toml::Table =
            toml::from_str("[formatting]\nphp-cs-fixer = \"vendor/bin/php-cs-fixer\"\n").unwrap();
        merge_toml(&mut base, overlay);
        let config: Config = base.try_into().unwrap();
        assert_eq!(
            config.formatting.php_cs_fixer.as_deref(),
            Some("vendor/bin/php-cs-fixer")
        );
        assert_eq!(config.formatting.phpcbf.as_deref(), Some("/usr/bin/phpcbf"));
        assert_eq!(config.formatting.timeout_ms(), 5000);
    }

    #[test]
    fn merge_toml_empty_overlay() {
        let mut base: toml::Table = toml::from_str("[php]\nversion = \"8.3\"\n").unwrap();
        let overlay: toml::Table = toml::Table::new();
        merge_toml(&mut base, overlay);
        let config: Config = base.try_into().unwrap();
        assert_eq!(config.php.version.as_deref(), Some("8.3"));
    }

    #[test]
    fn merge_toml_empty_base() {
        let mut base = toml::Table::new();
        let overlay: toml::Table =
            toml::from_str("[diagnostics]\nextra-arguments = true\n").unwrap();
        merge_toml(&mut base, overlay);
        let config: Config = base.try_into().unwrap();
        assert!(config.diagnostics.extra_arguments_enabled());
    }

    #[test]
    fn merge_toml_overlay_replaces_non_table_with_value() {
        let mut base: toml::Table =
            toml::from_str("[indexing]\nstrategy = \"composer\"\n").unwrap();
        let overlay: toml::Table = toml::from_str("[indexing]\nstrategy = \"self\"\n").unwrap();
        merge_toml(&mut base, overlay);
        let config: Config = base.try_into().unwrap();
        assert_eq!(config.indexing.strategy, Some(IndexingStrategy::SelfScan));
    }
}
