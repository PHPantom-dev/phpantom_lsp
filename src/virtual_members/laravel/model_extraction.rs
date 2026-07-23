//! Parse-time extraction of Eloquent model metadata from the AST.
//!
//! This module builds the [`LaravelMetadata`] attached to every parsed
//! class: `$casts`/`casts()`, `$attributes`, `$dates`,
//! `$fillable`/`$guarded`/`$hidden`/`$appends`, `$timestamps` and the
//! `CREATED_AT`/`UPDATED_AT` constants, the `#[Connection]`/`#[Table]`
//! attributes (and their property fallbacks), custom builder/collection
//! overrides, and `belongsToMany`/`morphToMany` pivot configuration.
//!
//! This runs once per class during parsing (`parser::classes`), unlike the
//! rest of `virtual_members::laravel`, which resolves already-parsed
//! [`ClassInfo`]/[`LaravelMetadata`] into virtual members at completion
//! time.

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{Atom, bytes_to_str, last_segment};
use crate::parser::DocblockCtx;
use crate::php_type::PhpType;
use crate::types::{LaravelMetadata, MethodInfo, PivotRelation};
use crate::util::strip_fqn_prefix;

use super::{extract_pivot_using, extract_with_pivot_columns, infer_relationship_from_body};

/// Check whether a method has the `#[Scope]` attribute (Laravel 11+).
///
/// Scans the method's attribute lists for an attribute whose short name
/// is `Scope` (matching `#[Scope]`, `#[\Illuminate\Database\Eloquent\Attributes\Scope]`,
/// or any use-imported alias that ends with `Scope`).
pub(crate) fn has_scope_attribute(method: &class_like::method::Method<'_>) -> bool {
    for attr_list in method.attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            if last_segment(attr.name.value()) == b"Scope" {
                return true;
            }
        }
    }
    false
}

/// Try to infer an Eloquent relationship return type from a method's body.
///
/// When a method has no `@return` annotation and no native return type
/// hint, this function extracts the method body text and scans it for
/// patterns like `$this->hasMany(Post::class)`.  If found, it returns
/// a synthesized return type string (e.g. `HasMany<Post>`).
///
/// This enables relationship property synthesis on models that don't
/// use Larastan-style `@return` annotations.
pub(crate) fn infer_relationship_from_method<'a>(
    method: &class_like::method::Method<'a>,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> Option<PhpType> {
    let ctx = doc_ctx?;
    let class_like::method::MethodBody::Concrete(block) = &method.body else {
        return None;
    };
    let start = block.left_brace.start.offset as usize;
    let end = block.right_brace.end.offset as usize;
    if end > ctx.content.len() || start >= end {
        return None;
    }
    // Adjust to valid UTF-8 char boundaries.
    let start = ctx.content.floor_char_boundary(start);
    let end = ctx.content.floor_char_boundary(end);
    let body_text = &ctx.content[start..end];
    infer_relationship_from_body(body_text)
}

/// Extract the custom collection class name from a `#[CollectedBy(X::class)]` attribute.
///
/// Scans the class's attribute lists for an attribute whose short name is
/// `CollectedBy` and extracts the first argument's text with `::class` stripped.
/// Returns `None` if no such attribute exists.
fn extract_collected_by_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
) -> Option<String> {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            let short = last_segment(attr.name.value());
            if short != b"CollectedBy" {
                continue;
            }
            let arg_list = attr.argument_list.as_ref()?;
            let first_arg = arg_list.arguments.first()?;
            let span = first_arg.span();
            let start = span.start.offset as usize;
            let end = span.end.offset as usize;
            let text = content.get(start..end)?;
            let class_name = text.trim_end_matches("::class").trim();
            if !class_name.is_empty() {
                return Some(class_name.to_string());
            }
        }
    }
    None
}

/// Extract the custom builder class name from a `#[UseEloquentBuilder(X::class)]` attribute.
fn extract_use_eloquent_builder_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
) -> Option<String> {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            let short = last_segment(attr.name.value());
            if short != b"UseEloquentBuilder" {
                continue;
            }
            let arg_list = attr.argument_list.as_ref()?;
            let first_arg = arg_list.arguments.first()?;
            let span = first_arg.span();
            let start = span.start.offset as usize;
            let end = span.end.offset as usize;
            let text = content.get(start..end)?;
            let class_name = text.trim_end_matches("::class").trim();
            if !class_name.is_empty() {
                return Some(class_name.to_string());
            }
        }
    }
    None
}

fn extract_laravel_model_string_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
    doc_ctx: Option<&DocblockCtx<'_>>,
    fqns: &[&str],
) -> Option<String> {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            let attr_fqn = resolve_attribute_fqn(bytes_to_str(attr.name.value()), doc_ctx);
            if !fqns.iter().any(|fqn| attr_fqn == *fqn) {
                continue;
            }
            let arg_list = attr.argument_list.as_ref()?;
            let first_arg = arg_list.arguments.first()?;
            let span = first_arg.span();
            let start = span.start.offset as usize;
            let end = span.end.offset as usize;
            let text = content.get(start..end)?.trim();
            if let Some(value) = extract_string_literal(text) {
                return Some(value);
            }
        }
    }
    None
}

fn resolve_attribute_fqn(name: &str, doc_ctx: Option<&DocblockCtx<'_>>) -> String {
    let name = name.trim_start_matches('\\');
    if name.contains('\\') {
        return name.to_string();
    }
    let Some(ctx) = doc_ctx else {
        return name.to_string();
    };
    if let Some(imported) = ctx.use_map.get(name) {
        return imported.trim_start_matches('\\').to_string();
    }
    if let Some(ns) = &ctx.namespace {
        return format!("{}\\{}", ns, name);
    }
    name.to_string()
}

fn extract_laravel_connection_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> Option<String> {
    extract_laravel_model_string_attribute(
        attribute_lists,
        content,
        doc_ctx,
        &["Illuminate\\Database\\Eloquent\\Attributes\\Connection"],
    )
}

fn extract_laravel_table_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> Option<String> {
    extract_laravel_model_string_attribute(
        attribute_lists,
        content,
        doc_ctx,
        &["Illuminate\\Database\\Eloquent\\Attributes\\Table"],
    )
}

/// Determine the custom builder class for an Eloquent model.
///
/// Checks three sources in priority order:
///
/// 1. `#[UseEloquentBuilder(CustomBuilder::class)]` attribute on the class.
/// 2. `/** @use HasBuilder<CustomBuilder> */` in `use_generics`.
/// 3. A `newEloquentBuilder()` method override whose return type names the
///    custom builder class.
fn extract_custom_builder(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    use_generics: &[(Atom, Vec<PhpType>)],
    methods: &[MethodInfo],
    content: &str,
) -> Option<PhpType> {
    // 1. Try the #[UseEloquentBuilder] attribute first.
    if let Some(name) = extract_use_eloquent_builder_attribute(attribute_lists, content) {
        return Some(PhpType::Named(name));
    }

    // 2. Fall back to @use HasBuilder<X>.
    for (trait_name, args) in use_generics {
        let short = trait_name.rsplit('\\').next().unwrap_or(trait_name);
        if (short == "HasBuilder" || short == "CustomizeQueryBuilder") && !args.is_empty() {
            return Some(args[0].clone());
        }
    }

    // 3. Fall back to newEloquentBuilder() return type override.
    let method = methods.iter().find(|m| m.name == "newEloquentBuilder")?;
    let return_type = method.return_type.as_ref()?;
    let base = return_type.base_name()?;
    if base == "Illuminate\\Database\\Eloquent\\Builder" || base == "Builder" || base.is_empty() {
        return None;
    }

    Some(return_type.clone())
}

/// Determine the custom collection class for an Eloquent model.
///
/// Checks three sources in priority order:
///
/// 1. `#[CollectedBy(CustomCollection::class)]` attribute on the class.
/// 2. `/** @use HasCollection<CustomCollection> */` in `use_generics`.
/// 3. A `newCollection()` method override whose return type names the
///    custom collection class.
///
/// The attribute takes priority because it is the newer Laravel API.
fn extract_custom_collection(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    use_generics: &[(Atom, Vec<PhpType>)],
    methods: &[MethodInfo],
    content: &str,
) -> Option<PhpType> {
    // 1. Try the #[CollectedBy] attribute first.
    if let Some(name) = extract_collected_by_attribute(attribute_lists, content) {
        return Some(PhpType::Named(name));
    }

    // 2. Fall back to @use HasCollection<X>.
    for (trait_name, args) in use_generics {
        let short = trait_name.rsplit('\\').next().unwrap_or(trait_name);
        if short == "HasCollection" && !args.is_empty() {
            return Some(args[0].clone());
        }
    }

    // 3. Fall back to newCollection() return type override.
    extract_custom_collection_from_new_collection(methods)
}

/// Extract the custom collection class from a `newCollection()` method
/// override.
///
/// Laravel models can override `newCollection()` to return a custom
/// collection class.  If the method's return type is not the standard
/// `Illuminate\Database\Eloquent\Collection` (or its short name
/// `Collection`), it is treated as a custom collection class.
///
/// Returns `None` if no `newCollection` method exists, if it has no
/// return type, or if the return type is the standard Eloquent
/// Collection.
fn extract_custom_collection_from_new_collection(methods: &[MethodInfo]) -> Option<PhpType> {
    let method = methods.iter().find(|m| m.name == "newCollection")?;
    let return_type = method.return_type.as_ref()?;

    // `base_name()` strips leading `\` and generic parameters, giving a
    // clean class name suitable for comparison.
    let base = return_type.base_name()?;

    // Ignore the standard Eloquent Collection — that's the default, not
    // a custom override.
    if base == "Illuminate\\Database\\Eloquent\\Collection" || base == "Collection" {
        return None;
    }

    if base.is_empty() {
        return None;
    }

    Some(return_type.clone())
}

/// Extract Eloquent cast definitions from a class's members.
///
/// Scans the class members for:
/// 1. A `$casts` property with an array initializer (`protected $casts = [...]`)
/// 2. A `casts()` method whose body contains a `return [...]` statement
///
/// Returns a list of `(column_name, cast_type)` pairs extracted from the
/// array literal text.  Both sources are merged: entries from the
/// `casts()` method take priority over `$casts` property entries when
/// the same column appears in both.  This matches Laravel's runtime
/// behaviour where `Model::casts()` overrides `$casts`.
fn extract_casts_definitions<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Vec<(String, String)> {
    let mut property_text: Option<String> = None;
    let mut method_text: Option<String> = None;

    for member in members {
        match member {
            class_like::member::ClassLikeMember::Property(
                class_like::property::Property::Plain(plain),
            ) => {
                for item in plain.items.iter() {
                    let var_name = bytes_to_str(item.variable().name).to_string();
                    let stripped = var_name.strip_prefix('$').unwrap_or(&var_name);
                    if stripped != "casts" {
                        continue;
                    }
                    if let class_like::property::PropertyItem::Concrete(concrete) = item {
                        let span = concrete.value.span();
                        let start = span.start.offset as usize;
                        let end = span.end.offset as usize;
                        if let Some(text) = content.get(start..end) {
                            property_text = Some(text.to_string());
                        }
                    }
                }
            }
            class_like::member::ClassLikeMember::Method(method)
                if method.name.value == b"casts" =>
            {
                if let class_like::method::MethodBody::Concrete(block) = &method.body {
                    let start = block.left_brace.start.offset as usize;
                    let end = block.right_brace.end.offset as usize;
                    if let Some(text) = content.get(start..end) {
                        method_text = Some(text.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    // Start with $casts property entries as the base.
    let mut merged: Vec<(String, String)> = Vec::new();

    if let Some(ref text) = property_text {
        merged = parse_casts_array(text);
    }

    // Merge casts() method entries on top — method entries override
    // property entries for the same column, matching Laravel's runtime
    // behaviour.
    if let Some(ref text) = method_text
        && let Some(arr_start) = text.find("return")
    {
        let after_return = &text[arr_start + 6..];
        if let Some(bracket_pos) = after_return.find('[') {
            let array_text = &after_return[bracket_pos..];
            let method_defs = parse_casts_array(array_text);
            for (key, value) in method_defs {
                if let Some(existing) = merged.iter_mut().find(|(k, _)| *k == key) {
                    existing.1 = value;
                } else {
                    merged.push((key, value));
                }
            }
        }
    }

    merged
}

/// Parse key-value pairs from a PHP array literal text.
///
/// Accepts text starting with `[` and extracts `'key' => 'value'` pairs.
/// Both single-quoted and double-quoted strings are supported for keys
/// and values.  Handles multi-line arrays and trailing commas.
///
/// Returns a list of `(key, value)` string pairs.
fn parse_casts_array(text: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let trimmed = text.trim();

    // Must start with `[`
    let inner = if let Some(s) = trimmed.strip_prefix('[') {
        // Strip trailing `]` if present
        s.strip_suffix(']').unwrap_or(s)
    } else {
        return results;
    };

    // Split on commas, handling each `'key' => 'value'` pair.
    // This simple approach works because cast arrays contain only
    // string literals — no nested arrays or complex expressions.
    for segment in inner.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        // Look for the `=>` arrow.
        let Some(arrow_pos) = segment.find("=>") else {
            continue;
        };

        let key_part = segment[..arrow_pos].trim();
        let value_part = segment[arrow_pos + 2..].trim();

        let key = extract_string_literal(key_part);
        let value = extract_string_literal(value_part);

        if let (Some(k), Some(v)) = (key, value)
            && !k.is_empty()
            && !v.is_empty()
        {
            results.push((k, v));
        }
    }

    results
}

/// Extract the string content from a PHP string literal.
///
/// Strips surrounding quotes (single or double) and returns the inner
/// text.  Returns `None` if the text is not a quoted string.
///
/// Also handles:
/// - `SomeCast::class` — returns `"SomeCast"`
/// - `Address::class.':argument'` — strips the concatenated argument
///   suffix and returns `"Address"`
fn extract_string_literal(text: &str) -> Option<String> {
    let t = text.trim();
    if ((t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"')))
        && t.len() >= 2
    {
        return Some(t[1..t.len() - 1].to_string());
    }
    // For class-string cast values like `SomeCast::class` or
    // `SomeCast::class.':argument'`, extract the class name.
    // The concatenation dot may have surrounding whitespace, so
    // look for `::class` and take everything before it.
    if let Some(class_pos) = t.find("::class") {
        let before = t[..class_pos].trim();
        let name = strip_fqn_prefix(before);
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// Extract Eloquent attribute defaults from a class's `$attributes` property.
///
/// Scans the class members for a `$attributes` property with an array
/// initializer (`protected $attributes = [...]`) and infers PHP types
/// from the literal default values.
///
/// Returns a list of `(column_name, php_type)` pairs.  For example,
/// `'role' => 'user'` produces `("role", "string")` and
/// `'is_active' => true` produces `("is_active", "bool")`.
fn extract_attributes_definitions<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Vec<(String, PhpType)> {
    extract_attributes(members, content)
        .into_iter()
        .map(|(key, php_type, _)| (key, php_type))
        .collect()
}

fn extract_attribute_defaults<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Vec<(String, String)> {
    extract_attributes(members, content)
        .into_iter()
        .map(|(key, _, value)| (key, value))
        .collect()
}

fn extract_attributes<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Vec<(String, PhpType, String)> {
    for member in members {
        if let class_like::member::ClassLikeMember::Property(
            class_like::property::Property::Plain(plain),
        ) = member
        {
            for item in plain.items.iter() {
                let var_name = bytes_to_str(item.variable().name).to_string();
                let stripped = var_name.strip_prefix('$').unwrap_or(&var_name);
                if stripped != "attributes" {
                    continue;
                }
                if let class_like::property::PropertyItem::Concrete(concrete) = item {
                    let span = concrete.value.span();
                    let start = span.start.offset as usize;
                    let end = span.end.offset as usize;
                    if let Some(text) = content.get(start..end) {
                        return parse_attributes_array(text);
                    }
                }
            }
        }
    }
    Vec::new()
}

/// Parse key-value pairs from a PHP `$attributes` array literal and
/// infer types from the default values.
///
/// Accepts text starting with `[` and extracts `'key' => value` pairs
/// where `value` is a PHP literal (`true`, `false`, `null`, integer,
/// float, or string).
///
/// Returns a list of `(column_name, php_type)` pairs.
fn parse_attributes_array(text: &str) -> Vec<(String, PhpType, String)> {
    let mut results = Vec::new();
    let trimmed = text.trim();

    let inner = if let Some(s) = trimmed.strip_prefix('[') {
        s.strip_suffix(']').unwrap_or(s)
    } else {
        return results;
    };

    for segment in inner.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        let Some(arrow_pos) = segment.find("=>") else {
            continue;
        };

        let key_part = segment[..arrow_pos].trim();
        let value_part = segment[arrow_pos + 2..].trim();

        let Some(key) = extract_string_literal(key_part) else {
            continue;
        };
        if key.is_empty() {
            continue;
        }

        if let Some(php_type) = crate::util::infer_type_from_literal(value_part) {
            results.push((key, php_type, value_part.to_string()));
        }
    }

    results
}

/// Extract timestamp configuration from a model class.
///
/// Reads three sources:
///
/// - `$timestamps` property — `true` (default) or `false`.
/// - `CREATED_AT` constant — column name string or `null`.
/// - `UPDATED_AT` constant — column name string or `null`.
///
/// Returns `(timestamps, created_at_name, updated_at_name)` using the
/// same `Option` semantics as `LaravelMetadata`: outer `None` means
/// "not declared", `Some(None)` means "explicitly `null`".
fn extract_timestamp_config<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> (Option<bool>, Option<Option<String>>, Option<Option<String>>) {
    let mut timestamps: Option<bool> = None;
    let mut created_at: Option<Option<String>> = None;
    let mut updated_at: Option<Option<String>> = None;

    for member in members {
        match member {
            class_like::member::ClassLikeMember::Property(
                class_like::property::Property::Plain(plain),
            ) => {
                for item in plain.items.iter() {
                    let var_name = bytes_to_str(item.variable().name).to_string();
                    let stripped = var_name.strip_prefix('$').unwrap_or(&var_name);
                    if stripped != "timestamps" {
                        continue;
                    }
                    if let class_like::property::PropertyItem::Concrete(concrete) = item {
                        let span = concrete.value.span();
                        let start = span.start.offset as usize;
                        let end = span.end.offset as usize;
                        if let Some(text) = content.get(start..end) {
                            let trimmed = text.trim();
                            if trimmed == "false" {
                                timestamps = Some(false);
                            } else if trimmed == "true" {
                                timestamps = Some(true);
                            }
                        }
                    }
                }
            }
            class_like::member::ClassLikeMember::Constant(constant) => {
                for item in constant.items.iter() {
                    let name = bytes_to_str(item.name.value).to_string();
                    if name != "CREATED_AT" && name != "UPDATED_AT" {
                        continue;
                    }
                    let span = item.value.span();
                    let start = span.start.offset as usize;
                    let end = span.end.offset as usize;
                    let value = content.get(start..end).map(|t| t.trim());
                    let parsed = match value {
                        Some("null") | Some("NULL") => Some(None),
                        Some(v) => extract_string_literal(v).map(Some),
                        None => None,
                    };
                    if let Some(val) = parsed {
                        if name == "CREATED_AT" {
                            created_at = Some(val);
                        } else {
                            updated_at = Some(val);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    (timestamps, created_at, updated_at)
}

/// Extract column names from `$fillable`, `$guarded`, `$hidden`, and `$appends` arrays.
///
/// These properties contain simple string lists of column names without
/// type information.  The `LaravelModelProvider` uses them as a
/// last-resort fallback, synthesizing `mixed`-typed virtual properties
/// for columns not already covered by `$casts` or `$attributes`.
///
/// All four arrays are merged; duplicates are removed (first occurrence
/// wins).
fn extract_column_names<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Vec<String> {
    let mut names = Vec::new();
    let targets = ["fillable", "guarded", "hidden", "visible", "appends"];

    for member in members {
        if let class_like::member::ClassLikeMember::Property(
            class_like::property::Property::Plain(plain),
        ) = member
        {
            for item in plain.items.iter() {
                let var_name = bytes_to_str(item.variable().name).to_string();
                let stripped = var_name.strip_prefix('$').unwrap_or(&var_name);
                if !targets.contains(&stripped) {
                    continue;
                }
                if let class_like::property::PropertyItem::Concrete(concrete) = item {
                    let span = concrete.value.span();
                    let start = span.start.offset as usize;
                    let end = span.end.offset as usize;
                    if let Some(text) = content.get(start..end) {
                        for name in parse_string_list(text) {
                            if !names.contains(&name) {
                                names.push(name);
                            }
                        }
                    }
                }
            }
        }
    }

    names
}

/// Extract column names from the deprecated `$dates` property array.
///
/// Before `$casts`, Laravel used `protected $dates = [...]` to mark
/// columns as Carbon instances. Each column listed here should be
/// typed as `\Carbon\Carbon` by the virtual member provider.
fn extract_dates_definitions<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Vec<String> {
    let mut names = Vec::new();

    for member in members {
        if let class_like::member::ClassLikeMember::Property(
            class_like::property::Property::Plain(plain),
        ) = member
        {
            for item in plain.items.iter() {
                let var_name = bytes_to_str(item.variable().name).to_string();
                let stripped = var_name.strip_prefix('$').unwrap_or(&var_name);
                if stripped != "dates" {
                    continue;
                }
                if let class_like::property::PropertyItem::Concrete(concrete) = item {
                    let span = concrete.value.span();
                    let start = span.start.offset as usize;
                    let end = span.end.offset as usize;
                    if let Some(text) = content.get(start..end) {
                        for name in parse_string_list(text) {
                            if !names.contains(&name) {
                                names.push(name);
                            }
                        }
                    }
                }
            }
        }
    }

    names
}

fn extract_string_property<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
    target: &str,
) -> Option<String> {
    for member in members {
        if let class_like::member::ClassLikeMember::Property(
            class_like::property::Property::Plain(plain),
        ) = member
        {
            for item in plain.items.iter() {
                let var_name = bytes_to_str(item.variable().name).to_string();
                let stripped = var_name.strip_prefix('$').unwrap_or(&var_name);
                if stripped != target {
                    continue;
                }
                if let class_like::property::PropertyItem::Concrete(concrete) = item {
                    let span = concrete.value.span();
                    let start = span.start.offset as usize;
                    let end = span.end.offset as usize;
                    if let Some(text) = content.get(start..end)
                        && let Some(value) = extract_string_literal(text.trim())
                    {
                        return Some(value);
                    }
                }
            }
        }
    }
    None
}

/// Parse a PHP array literal containing only string values.
///
/// Accepts text starting with `[` and extracts bare string values
/// (no `=>` keys).  For example, `['name', 'email', 'password']`
/// returns `["name", "email", "password"]`.
fn parse_string_list(text: &str) -> Vec<String> {
    let mut results = Vec::new();
    let trimmed = text.trim();

    let inner = if let Some(s) = trimmed.strip_prefix('[') {
        s.strip_suffix(']').unwrap_or(s)
    } else {
        return results;
    };

    for segment in inner.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        // Skip key-value pairs (these belong to a different kind of array).
        if segment.contains("=>") {
            continue;
        }
        if let Some(s) = extract_string_literal(segment)
            && !s.is_empty()
        {
            results.push(s);
        }
    }

    results
}

/// Recover pivot configuration from `belongsToMany`/`morphToMany` relationship
/// method bodies in a class.
///
/// Scans each concrete method body for a many-to-many builder call chained
/// with `->using(CustomPivot::class)` and/or `->withPivot('col', …)`, and
/// returns one [`PivotRelation`] per method that declares either. Methods
/// without a many-to-many call, or without any pivot chain, are skipped.
///
/// Works from the method body text (via source offsets), so it covers both
/// annotated relationships (`@return BelongsToMany<…>`) and un-annotated ones.
fn extract_pivot_relations<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Vec<PivotRelation> {
    let mut relations = Vec::new();
    for member in members {
        let class_like::member::ClassLikeMember::Method(method) = member else {
            continue;
        };
        let class_like::method::MethodBody::Concrete(block) = &method.body else {
            continue;
        };
        let start = block.left_brace.start.offset as usize;
        let end = block.right_brace.end.offset as usize;
        if end > content.len() || start >= end {
            continue;
        }
        let start = content.floor_char_boundary(start);
        let end = content.floor_char_boundary(end);
        let body = &content[start..end];

        // `using`/`withPivot` only appear on many-to-many relationships;
        // require the builder call so unrelated methods are not scanned.
        if !body.contains("belongsToMany(")
            && !body.contains("morphToMany(")
            && !body.contains("morphedByMany(")
        {
            continue;
        }

        let using = extract_pivot_using(body);
        let columns = extract_with_pivot_columns(body);
        if using.is_none() && columns.is_empty() {
            continue;
        }

        relations.push(PivotRelation {
            method: String::from_utf8_lossy(method.name.value).into_owned(),
            using,
            columns,
        });
    }
    relations
}

/// Build the [`LaravelMetadata`] for a class from its AST node.
///
/// `methods` must already be extracted (via
/// `Backend::extract_class_like_members`) since several sources here
/// (custom builder/collection, relationship-derived timestamps) key off
/// already-resolved method return types. `use_generics` is the merged
/// docblock + inline `@use` generics list, needed for
/// `HasBuilder`/`HasCollection` detection.
pub(crate) fn extract_laravel_metadata<'a>(
    class: &class_like::Class<'a>,
    methods: &[MethodInfo],
    use_generics: &[(Atom, Vec<PhpType>)],
    content: &str,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> LaravelMetadata {
    let custom_collection =
        extract_custom_collection(&class.attribute_lists, use_generics, methods, content);

    let custom_builder =
        extract_custom_builder(&class.attribute_lists, use_generics, methods, content);

    let casts_definitions = extract_casts_definitions(class.members.iter(), content);

    let belongs_to_many_pivots = extract_pivot_relations(class.members.iter(), content);

    let attributes_definitions = extract_attributes_definitions(class.members.iter(), content);
    let attribute_defaults = extract_attribute_defaults(class.members.iter(), content);

    let column_names = extract_column_names(class.members.iter(), content);

    let connection_name =
        extract_laravel_connection_attribute(&class.attribute_lists, content, doc_ctx)
            .or_else(|| extract_string_property(class.members.iter(), content, "connection"));

    let table_name = extract_laravel_table_attribute(&class.attribute_lists, content, doc_ctx)
        .or_else(|| extract_string_property(class.members.iter(), content, "table"));

    let has_get_connection_name_method = methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case("getConnectionName"));
    let has_get_table_method = methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case("getTable"));

    let primary_key = extract_string_property(class.members.iter(), content, "primaryKey");
    let key_type = extract_string_property(class.members.iter(), content, "keyType");
    let has_get_key_name_method = methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case("getKeyName"));

    let dates_definitions = extract_dates_definitions(class.members.iter(), content);

    let (timestamps, created_at_name, updated_at_name) =
        extract_timestamp_config(class.members.iter(), content);

    LaravelMetadata {
        custom_collection,
        casts_definitions,
        dates_definitions,
        attributes_definitions,
        attribute_defaults,
        column_names,
        connection_name,
        table_name,
        has_get_connection_name_method,
        has_get_table_method,
        primary_key,
        key_type,
        has_get_key_name_method,
        timestamps,
        created_at_name,
        updated_at_name,
        custom_builder,
        belongs_to_many_pivots,
    }
}

#[cfg(test)]
mod tests {
    use crate::Backend;
    use crate::atom::atom;

    #[test]
    fn laravel_model_table_and_connection_attributes_are_extracted() {
        let src = r#"<?php
use Illuminate\Database\Eloquent\Attributes\Connection;
use Illuminate\Database\Eloquent\Attributes\Table;

#[Connection('analytics')]
#[Table('event_records')]
class EventRecord {}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        let class = classes
            .iter()
            .find(|(c, _)| c.name == atom("EventRecord"))
            .map(|(c, _)| c)
            .unwrap();
        let laravel = class.laravel().unwrap();
        assert_eq!(laravel.connection_name.as_deref(), Some("analytics"));
        assert_eq!(laravel.table_name.as_deref(), Some("event_records"));
    }

    #[test]
    fn local_connection_and_table_attributes_are_ignored() {
        let src = r#"<?php
namespace App\Models;

#[Connection('analytics')]
#[Table('event_records')]
class EventRecord {}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        let class = classes
            .iter()
            .find(|(c, _)| c.name == atom("EventRecord"))
            .map(|(c, _)| c)
            .unwrap();
        let laravel = class.laravel().unwrap();
        assert_eq!(laravel.connection_name, None);
        assert_eq!(laravel.table_name, None);
    }

    #[test]
    fn laravel_model_get_table_override_is_detected() {
        let src = r#"<?php
class ReportRow {
    public function getTable(): string { return 'dynamic_' . date('Y'); }
    public function getConnectionName(): string { return tenant_connection(); }
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        let class = classes
            .iter()
            .find(|(c, _)| c.name == atom("ReportRow"))
            .map(|(c, _)| c)
            .unwrap();
        let laravel = class.laravel().unwrap();
        assert!(laravel.has_get_table_method);
        assert!(laravel.has_get_connection_name_method);
    }

    #[test]
    fn laravel_model_primary_key_config_is_extracted() {
        let src = r#"<?php
class Passport {
    protected $primaryKey = 'passport_number';
    protected $keyType = 'string';
    public function getKeyName(): string { return $this->primaryKey; }
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        let class = classes
            .iter()
            .find(|(c, _)| c.name == atom("Passport"))
            .map(|(c, _)| c)
            .unwrap();
        let laravel = class.laravel().unwrap();
        assert_eq!(laravel.primary_key.as_deref(), Some("passport_number"));
        assert_eq!(laravel.key_type.as_deref(), Some("string"));
        assert!(laravel.has_get_key_name_method);
    }
}
