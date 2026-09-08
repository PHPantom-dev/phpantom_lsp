//! Built-in formatting through the embedded mago-formatter, including
//! loading the `[formatter]` table of a workspace `mago.toml`.

use std::borrow::Cow;
use std::path::Path;

use serde::Deserialize;

use crate::atom::bytes_to_str;

/// Format PHP source code using the built-in mago-formatter.
///
/// Returns the formatted source string, or an error if parsing fails.
/// The caller supplies the effective formatter settings.
pub(super) fn format_with_mago(
    content: &str,
    php_version: mago_php_version::PHPVersion,
    settings: mago_formatter::settings::FormatSettings,
) -> Result<String, String> {
    let arena = mago_allocator::LocalArena::new();
    let formatter = mago_formatter::Formatter::new(&arena, php_version, settings);

    let formatted = formatter
        .format_code(
            Cow::Borrowed(b"phpantom-fmt"),
            Cow::Owned(content.as_bytes().to_vec()),
        )
        .map_err(|e| format!("Built-in formatter failed to parse PHP: {}", e))?;

    Ok(bytes_to_str(formatted).to_string())
}

#[derive(Deserialize)]
struct MagoToml {
    formatter: Option<MagoFormatterToml>,
}

#[derive(Deserialize)]
struct MagoFormatterToml {
    preset: Option<mago_formatter::presets::FormatterPreset>,
    #[serde(flatten)]
    settings: mago_formatter::settings::RawFormatSettings,
}

/// Load the `[formatter]` table from a workspace `mago.toml`.
///
/// `RawFormatSettings` is Mago's own deserialization type, so settings added
/// by the embedded formatter are accepted without duplicating its schema.
pub(super) fn load_mago_format_settings(
    config_path: &Path,
) -> Result<mago_formatter::settings::FormatSettings, String> {
    let source = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read {}: {}", config_path.display(), e))?;
    let config: MagoToml = toml::from_str(&source)
        .map_err(|e| format!("Failed to parse {}: {}", config_path.display(), e))?;

    let Some(formatter) = config.formatter else {
        return Ok(mago_formatter::settings::FormatSettings::default());
    };

    let base = formatter.preset.unwrap_or_default().settings();
    Ok(formatter.settings.merge_with(base))
}

/// Convert a project [`PhpVersion`](crate::types::PhpVersion) into the
/// mago-formatter's [`PHPVersion`](mago_php_version::PHPVersion).
pub(super) fn to_mago_php_version(v: crate::types::PhpVersion) -> mago_php_version::PHPVersion {
    mago_php_version::PHPVersion::new(v.major as u32, v.minor as u32, 0)
}
