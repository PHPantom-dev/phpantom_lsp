//! External tool sections: `[formatting]`, `[phpstan]`, `[mago]`,
//! `[phpcs]` and `[phpmd]`.

use serde::Deserialize;

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
    /// Explicitly disable all formatting
    pub disabled: Option<bool>,
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
    /// Whether Blade templates are formatted through Pint's
    /// `Pint/laravel_blade` rule.
    ///
    /// - `None` (default) — follow the workspace `pint.json`: Blade files
    ///   go to Pint when its `rules` turn the rule on, and to
    ///   blade-formatter or the built-in reindenter otherwise.
    /// - `true` — send Blade files to Pint with `--blade`, which turns the
    ///   rule on for the run.
    /// - `false` — never send Blade files to Pint.
    #[serde(rename = "pint-blade")]
    pub pint_blade: Option<bool>,
    /// Command (path or name) to run shufo/blade-formatter on Blade
    /// templates.
    ///
    /// - `None` (default) — use `node_modules/.bin/blade-formatter` when
    ///   `package.json` lists `blade-formatter`, the workspace root has a
    ///   `.bladeformatterrc.json`/`.bladeformatterrc`, and Pint does not
    ///   format Blade.
    /// - `""` — never use blade-formatter.
    /// - Any other value — use as the command, ahead of Pint.
    #[serde(rename = "blade-formatter")]
    pub blade_formatter: Option<String>,
    /// Whether the built-in Blade formatter also formats the PHP the
    /// template carries: `@php` bodies, `<?php` islands, echoes, and
    /// directive arguments, along with the spacing that is Blade's own
    /// (`@if(` to `@if (`, `{{$x}}` to `{{ $x }}`).
    ///
    /// - `None` (default) — the reindenter changes leading whitespace
    ///   only, and every fragment keeps the spacing its author typed.
    /// - `true` — format each fragment through the built-in PHP
    ///   formatter, with the same `mago.toml` settings a `.php` file gets.
    ///
    /// Has no effect on a project whose Blade files go to Pint or
    /// blade-formatter, which format them themselves.
    #[serde(rename = "blade-php")]
    pub blade_php: Option<bool>,
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

    /// Whether formatting is explicitly disabled
    pub fn is_disabled(&self) -> bool {
        self.disabled == Some(true)
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
    /// Whether to proxy `mago lint` diagnostics.
    ///
    /// - `None` (default) — proxy them when the workspace `mago.toml`
    ///   configures the linter (it carries a `[linter]` table).
    /// - `true` / `false` — always / never, whatever `mago.toml` says.
    pub lint: Option<bool>,
    /// Whether to proxy `mago analyze` diagnostics.
    ///
    /// Same three states as [`lint`](Self::lint), keyed on an
    /// `[analyzer]` table in `mago.toml` when unset.
    pub analyze: Option<bool>,
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
/// `vendor/bin/phpcs` then `$PATH`, for projects that list
/// `squizlabs/php_codesniffer` in `require-dev`, have a PHPCS ruleset
/// file, or set `standard`.  Set to `""` (empty string) to explicitly
/// disable PHPCS integration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PhpcsConfig {
    /// Command (path or name) to run PHPCS.
    ///
    /// - `None` (default) — auto-detect `vendor/bin/phpcs`,
    ///   then `phpcs` on `$PATH`, when the project uses PHPCS.
    /// - `""` — disable PHPCS.
    /// - Any other value — use as the command (e.g.
    ///   `"vendor/bin/phpcs"` or `"phpcs"`).
    pub command: Option<String>,
    /// Coding standard to enforce (e.g. `"PSR12"`).
    /// phpcbf fixes against the same standard.
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

/// `[phpmd]` section — PHP Mess Detector proxy settings.
///
/// When `command` is unset (`None`), PHPantom auto-detects via
/// `vendor/bin/phpmd` then `$PATH`, but only for a project with a PHPMD
/// config file or a configured `ruleset`.  Set to `""` (empty string)
/// to explicitly disable PHPMD integration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PhpmdConfig {
    /// Command (path or name) to run PHPMD.
    ///
    /// - `None` (default) — auto-detect `vendor/bin/phpmd`,
    ///   then `phpmd` on `$PATH`.
    /// - `""` — disable PHPMD.
    /// - Any other value — use as the command (e.g.
    ///   `"vendor/bin/phpmd"` or `"phpmd"`).
    pub command: Option<String>,
    /// Ruleset name or file passed via `--ruleset` (e.g. `"cleancode"`).
    ///
    /// When unset, PHPMD uses the config file it auto-detects in the
    /// project root (`phpmd.yml`, `phpmd.xml`, ...).
    pub ruleset: Option<String>,
    /// Maximum runtime in milliseconds before PHPMD is killed.
    /// Defaults to 30 000 ms (30 seconds).
    pub timeout: Option<u64>,
}

impl PhpmdConfig {
    /// Return the configured timeout in milliseconds, falling back to
    /// 30 000 ms when unset.
    pub fn timeout_ms(&self) -> u64 {
        self.timeout.unwrap_or(30_000)
    }

    /// Whether PHPMD is explicitly disabled (command set to empty
    /// string).
    pub fn is_disabled(&self) -> bool {
        self.command.as_deref() == Some("")
    }
}
