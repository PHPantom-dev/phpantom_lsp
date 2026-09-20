//! Reading the parts of a project's `pint.json` that steer formatting.

use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
struct PintJson {
    #[serde(default)]
    rules: serde_json::Map<String, serde_json::Value>,
}

/// Whether the workspace `pint.json` turns the `Pint/laravel_blade` rule
/// on. Pint treats any value but `false` as on, since the rule takes an
/// options object as well as a boolean.
///
/// Pint reads `pint.json` from its working directory, which is the
/// workspace root when PHPantom runs it, so that is the only file that
/// counts.
pub(super) fn blade_rule_enabled(workspace_root: &Path) -> bool {
    let Ok(source) = std::fs::read_to_string(workspace_root.join("pint.json")) else {
        return false;
    };
    let Ok(config) = serde_json::from_str::<PintJson>(&source) else {
        return false;
    };
    config
        .rules
        .get("Pint/laravel_blade")
        .is_some_and(|value| *value != serde_json::Value::Bool(false))
}
