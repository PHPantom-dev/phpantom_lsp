//! Formatting for `.blade.php` templates.
//!
//! A Blade template is markup with PHP inside it, so the PHP formatting
//! strategy does not apply: the built-in PHP formatter cannot parse it,
//! and of the external tools only Laravel Pint knows what to do with one.
//! Two strategies remain:
//!
//! - **Pint**, when the project has opted into its `Pint/laravel_blade`
//!   rule. Pint formats Blade through prettier and its Blade and Tailwind
//!   plugins, reflowing markup and sorting classes. It is only used when
//!   the rule is on, because Pint echoes a file the rule excludes back
//!   unchanged with a successful exit code, which is indistinguishable
//!   from "already formatted"; the decision has to be read from
//!   `pint.json` (or from `pint-blade = true` in `.phpantom.toml`, which
//!   also passes `--blade`).
//! - **The built-in reindenter** ([`reindent`]) otherwise. It changes
//!   leading whitespace only, so it never reflows a line, wraps an
//!   attribute, or touches the CSS, JavaScript, and PHP embedded in the
//!   template. A project that sets `blade-php = true` in `.phpantom.toml`
//!   gets one more pass first ([`php`]), which formats the PHP the
//!   template carries through the built-in PHP formatter.
//!
//! Some templates are output where indentation means something, and are
//! left alone by both: Envoy task files, Markdown mail templates, and
//! Laravel Boost guidelines.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use tower_lsp::lsp_types::FormattingOptions;

use crate::Backend;
use crate::composer;
use crate::config::FormattingConfig;

use super::{FormattingStrategy, ResolvedTool, Tool, external, mago, pint, resolve_strategy};

mod php;
pub mod reindent;
#[cfg(test)]
mod tests;

pub use reindent::Options as BladeFormatOptions;

/// How a Blade template is formatted.
#[derive(Debug)]
pub enum BladeFormattingStrategy {
    /// Laravel Pint with its Blade rule. `flag` passes `--blade`, which
    /// turns the rule on for the run when `pint.json` does not.
    Pint { tool: ResolvedTool, flag: bool },
    /// The built-in reindenter, carrying the settings for the
    /// embedded-PHP pass when the project opted into it.
    BuiltIn(Option<BladePhpFormatting>),
    /// Formatting is explicitly disabled.
    Disabled,
}

/// What the embedded-PHP pass ([`php`]) runs with, resolved once with the
/// strategy rather than once per template.
///
/// The workspace `mago.toml` is read at format time, the way the PHP
/// strategy reads its own, so a template's PHP and the classes behind it
/// are written the same way.
#[derive(Debug)]
pub struct BladePhpFormatting {
    version: crate::types::PhpVersion,
    mago_config: Option<PathBuf>,
}

impl BladePhpFormatting {
    /// The pass's settings, or the error a malformed `mago.toml` reports.
    fn settings(&self) -> Result<php::PhpSettings, String> {
        let settings = match &self.mago_config {
            Some(path) => mago::load_mago_format_settings(path)?,
            None => mago_formatter::settings::FormatSettings::default(),
        };
        Ok(php::PhpSettings {
            version: mago::to_mago_php_version(self.version),
            settings,
        })
    }
}

/// Resolve how a Blade template is formatted, from the same inputs as
/// [`resolve_strategy`].
///
/// Pint is chosen when the PHP strategy resolved it (explicit path or
/// `require-dev`) and the project has turned its Blade rule on: through
/// `pint-blade = true` in `.phpantom.toml`, or through
/// `rules["Pint/laravel_blade"]` in the workspace `pint.json` when
/// `pint-blade` is unset. `pint-blade = false` keeps Blade files on the
/// built-in reindenter whatever `pint.json` says.
///
/// A project on the built-in reindenter that also sets `blade-php = true`
/// gets the settings for the embedded-PHP pass resolved with it, so a run
/// over a whole project reads the workspace metadata once rather than
/// once per template.
pub fn resolve_blade_strategy(
    workspace_root: Option<&Path>,
    config: &FormattingConfig,
    composer_json: Option<&composer::ComposerPackage>,
    bin_dir: Option<&str>,
    php_version: crate::types::PhpVersion,
) -> BladeFormattingStrategy {
    let built_in = || {
        BladeFormattingStrategy::BuiltIn(config.blade_php.unwrap_or(false).then(|| {
            BladePhpFormatting {
                version: php_version,
                mago_config: workspace_root
                    .filter(|root| crate::mago::has_mago_config(root))
                    .map(|root| root.join("mago.toml")),
            }
        }))
    };
    let tools = match resolve_strategy(workspace_root, config, composer_json, bin_dir) {
        FormattingStrategy::Disabled => return BladeFormattingStrategy::Disabled,
        FormattingStrategy::BuiltIn(_) => return built_in(),
        FormattingStrategy::External(tools) => tools,
    };
    let Some(tool) = tools.into_iter().find(|tool| tool.tool == Tool::Pint) else {
        return built_in();
    };
    match config.pint_blade {
        Some(true) => BladeFormattingStrategy::Pint { tool, flag: true },
        Some(false) => built_in(),
        None if workspace_root.is_some_and(pint::blade_rule_enabled) => {
            BladeFormattingStrategy::Pint { tool, flag: false }
        }
        None => built_in(),
    }
}

/// Whether a template's indentation is output rather than layout, so
/// that no formatter may touch it: an Envoy task file, a mail template
/// (Markdown, where four leading spaces make a code block), or a Laravel
/// Boost guideline. The same files Pint's Blade rule skips.
pub fn is_whitespace_sensitive(file_path: &Path, content: &str) -> bool {
    let path = file_path.to_string_lossy().replace('\\', "/");
    let basename = path.rsplit('/').next().unwrap_or(&path);
    matches!(basename, "Envoy.blade.php" | "envoy.blade.php")
        || path.contains("/vendor/mail/")
        || path.contains("resources/views/mail/")
        || path.contains("resources/views/emails/")
        || path.contains("resources/boost/guidelines/")
        || content.contains("<x-mail::")
        || content.contains("@component('mail::")
        || content.contains("@component(\"mail::")
}

/// Run `strategy` on a Blade template and return the formatted text, or
/// `None` when formatting is disabled, the template is one whose
/// whitespace is output, or nothing changed.
pub fn format_blade_content(
    strategy: &BladeFormattingStrategy,
    content: &str,
    file_path: &Path,
    workspace_root: Option<&Path>,
    config: &FormattingConfig,
    options: &BladeFormatOptions,
    cancelled: &AtomicBool,
) -> Result<Option<String>, String> {
    let formatted = match strategy {
        BladeFormattingStrategy::Disabled => return Ok(None),
        BladeFormattingStrategy::Pint { tool, flag } => external::run_pint_on_blade(
            tool,
            content,
            file_path,
            workspace_root,
            *flag,
            config,
            cancelled,
        )?,
        BladeFormattingStrategy::BuiltIn(php) => {
            if is_whitespace_sensitive(file_path, content) {
                return Ok(None);
            }
            match php {
                None => reindent::reindent(content, options),
                // The embedded-PHP pass reads the column a block sits at
                // to decide how much of the print width its body has, so
                // it runs between two reindents: the first settles the
                // columns, the second lays out what the pass rewrote.
                Some(php) => {
                    let laid_out = reindent::reindent(content, options);
                    let rewritten =
                        php::format_embedded(&laid_out, &php.settings()?, &options.indent);
                    reindent::reindent(&rewritten, options)
                }
            }
        }
    };
    Ok((formatted != content).then_some(formatted))
}

/// The reindenter's options as an editor's formatting request states
/// them. A client that sends no tab size gets four spaces.
pub fn options_from_lsp(options: &FormattingOptions) -> BladeFormatOptions {
    let indent = if options.insert_spaces {
        " ".repeat(if options.tab_size == 0 {
            4
        } else {
            options.tab_size as usize
        })
    } else {
        "\t".to_string()
    };
    BladeFormatOptions {
        indent,
        trim_trailing_whitespace: options.trim_trailing_whitespace.unwrap_or(false),
        insert_final_newline: options.insert_final_newline.unwrap_or(false),
        trim_final_newlines: options.trim_final_newlines.unwrap_or(false),
    }
}

impl Backend {
    /// Resolve how the workspace formats Blade templates, from
    /// `.phpantom.toml`, the root `composer.json`, and `pint.json`.
    pub(crate) fn resolve_blade_formatting_strategy(&self) -> BladeFormattingStrategy {
        let config = self.config();
        let workspace_root = self.workspace.workspace_root.read().clone();
        let composer_json = workspace_root
            .as_deref()
            .and_then(composer::read_composer_package);
        let bin_dir = composer_json.as_ref().map(composer::get_bin_dir);
        resolve_blade_strategy(
            workspace_root.as_deref(),
            &config.formatting,
            composer_json.as_ref(),
            bin_dir.as_deref(),
            self.php_version(),
        )
    }

    /// Format one Blade template's `content` with `strategy`; see
    /// [`format_blade_content`].
    pub(crate) fn format_blade_content(
        &self,
        strategy: &BladeFormattingStrategy,
        file_path: &Path,
        content: &str,
        options: &BladeFormatOptions,
        cancelled: &AtomicBool,
    ) -> Result<Option<String>, String> {
        let config = self.config();
        let workspace_root = self.workspace.workspace_root.read().clone();
        format_blade_content(
            strategy,
            content,
            file_path,
            workspace_root.as_deref(),
            &config.formatting,
            options,
            cancelled,
        )
    }
}
