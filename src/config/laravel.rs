//! `[laravel]` section: Laravel-specific analysis settings.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LaravelConfig {
    pub schema: LaravelSchemaConfig,
    pub migrations: LaravelMigrationsConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LaravelSchemaConfig {
    /// Enable schema dump scanning. Defaults to enabled.
    pub enabled: Option<bool>,
    /// Optional files or directories to scan. Defaults to `database/schema`.
    pub paths: Vec<String>,
}

impl LaravelSchemaConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LaravelMigrationsConfig {
    /// Enable migration scanning. Defaults to enabled.
    pub enabled: Option<bool>,
    /// Optional files or directories to scan. Defaults to every direct
    /// `database/migrations/*.php` directory outside vendor.
    pub paths: Vec<String>,
}

impl LaravelMigrationsConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}
