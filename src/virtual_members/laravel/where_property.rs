//! Eloquent `where{PropertyName}()` dynamic method synthesis.
//!
//! Laravel's `Builder::__call()` translates calls like
//! `whereBrandId($value)` into `where('brand_id', $value)` and returns
//! `$this`.  This module synthesizes those dynamic methods from a
//! model's known column names so that PHPantom resolves them instead of
//! reporting `unknown_member` diagnostics.
//!
//! Column names are gathered directly from the raw `ClassInfo` and its
//! `LaravelMetadata` (without full resolution) to avoid recursive
//! cycles with `LaravelModelProvider::provide()`.  The sources are:
//!
//! - `$casts` definitions
//! - `$dates` definitions
//! - `$attributes` defaults
//! - `$fillable`/`$guarded`/`$hidden`/`$appends` column names
//! - Timestamp columns (`created_at`, `updated_at` unless disabled)
//! - `@property` / `@property-read` / `@property-write` docblock tags (parsed from raw docblock)
//! - Declared (non-static, non-private) properties on the class itself
//!
//! Each column `foo_bar` produces a method `whereFooBar($value)` that
//! accepts one parameter and returns `Builder<ConcreteModel>`.

use std::collections::HashSet;

use crate::atom::atom;
use crate::docblock;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, MethodInfo, ParameterInfo};

use super::ELOQUENT_BUILDER_FQN;
use super::helpers::snake_to_pascal;

/// Collect all known column names from a raw (unresolved) model class.
///
/// This reads directly from `LaravelMetadata` fields and from the
/// class's own declared/virtual properties, avoiding a full
/// `resolve_class_fully` call (which would recurse through
/// `LaravelModelProvider`).
fn collect_column_names(class: &ClassInfo) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut columns = Vec::new();

    // Helper to insert a column if not already seen.
    let mut push = |name: &str| {
        if seen.insert(name.to_string()) {
            columns.push(name.to_string());
        }
    };

    // ── LaravelMetadata sources ─────────────────────────────────────
    if let Some(laravel) = class.laravel() {
        // $casts
        for (col, _) in &laravel.casts_definitions {
            push(col);
        }

        // $dates
        for col in &laravel.dates_definitions {
            push(col);
        }

        // $attributes defaults
        for (col, _) in &laravel.attributes_definitions {
            push(col);
        }

        // $fillable, $guarded, $hidden, $appends
        for col in &laravel.column_names {
            push(col);
        }

        // Timestamp columns (unless explicitly disabled).
        let timestamps_enabled = laravel.timestamps.unwrap_or(true);
        if timestamps_enabled {
            let created_col = match &laravel.created_at_name {
                Some(Some(name)) => Some(name.as_str()),
                Some(None) => None,         // explicitly null
                None => Some("created_at"), // default
            };
            let updated_col = match &laravel.updated_at_name {
                Some(Some(name)) => Some(name.as_str()),
                Some(None) => None,         // explicitly null
                None => Some("updated_at"), // default
            };
            for col in [created_col, updated_col].into_iter().flatten() {
                push(col);
            }
        }
    }

    // ── Properties already on the class ─────────────────────────────
    // This catches any explicitly declared properties and virtual
    // properties that were already added to the class.
    for prop in class.properties.iter() {
        push(&prop.name);
    }

    // ── @property tags from the class docblock ──────────────────────
    // Virtual @property tags are not on `class.properties` yet (they
    // are added by PHPDocProvider during full resolution, which runs
    // after this function).  Extract them directly from the raw
    // docblock text.
    if let Some(ref doc_text) = class.class_docblock {
        for (name, _type_str) in docblock::extract_property_tags(doc_text) {
            push(&name);
        }
    }

    columns
}

/// Build `where{PropertyName}()` virtual methods for a model's columns.
///
/// Reads column names directly from the raw `class` (no recursive
/// resolution) and synthesizes a `where{StudlyCase}()` method for each.
/// Each method accepts a single `$value` parameter (typed `mixed`) and
/// returns `Builder<ConcreteModel>`.
///
/// Methods whose name would collide with an entry in
/// `existing_method_names` are skipped.
pub fn build_where_property_methods_for_class(
    class: &ClassInfo,
    existing_method_names: &HashSet<String>,
) -> Vec<MethodInfo> {
    let columns = collect_column_names(class);

    if columns.is_empty() {
        return Vec::new();
    }

    // Build the return type: Builder<ConcreteModel>.
    let builder_fqn = class
        .laravel()
        .and_then(|l| l.custom_builder.as_ref())
        .and_then(|t| t.base_name())
        .unwrap_or(ELOQUENT_BUILDER_FQN);

    let return_type = PhpType::Generic(
        builder_fqn.to_string(),
        vec![PhpType::Named(class.name.to_string())],
    );

    let value_param = ParameterInfo {
        name: atom("$value"),
        is_required: true,
        type_hint: Some(PhpType::mixed()),
        native_type_hint: None,
        description: None,
        default_value: None,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    };

    let mut methods = Vec::new();
    let mut seen_methods = HashSet::new();

    for col in &columns {
        let method_name = format!("where{}", snake_to_pascal(col));

        // Skip if a method with this name already exists on the target.
        if existing_method_names.contains(&method_name.to_ascii_lowercase()) {
            continue;
        }

        // Skip if we already synthesized a method with this name
        // (can happen when a column appears in multiple sources).
        if !seen_methods.insert(method_name.clone()) {
            continue;
        }

        let method = MethodInfo {
            parameters: vec![value_param.clone()],
            description: Some(format!("Find models where `{col}` equals the given value.",)),
            return_type: Some(return_type.clone()),
            ..MethodInfo::virtual_method(&method_name, None)
        };

        methods.push(method);
    }

    methods
}

/// Collect lowercase method names from a slice for dedup lookups.
pub fn lowercase_method_names<M: std::borrow::Borrow<MethodInfo>>(
    methods: &[M],
) -> HashSet<String> {
    methods
        .iter()
        .map(|m| m.borrow().name.to_ascii_lowercase())
        .collect()
}

/// Reverse-map a `where{Property}` method name back to its column name.
///
/// `whereFlour` → `"flour"`, `whereKitchenId` → `"kitchen_id"`.
/// Returns `None` if the name does not start with `"where"` or if what
/// follows is empty.
pub fn where_property_method_to_column(method_name: &str) -> Option<String> {
    let suffix = method_name.strip_prefix("where")?;
    if suffix.is_empty() {
        return None;
    }
    // The suffix is PascalCase. Run camel_to_snake on it to recover the
    // original column name.  The first character is already uppercase
    // (PascalCase), so camel_to_snake produces the right result.
    Some(super::helpers::camel_to_snake(suffix))
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "where_property_tests.rs"]
mod tests;
