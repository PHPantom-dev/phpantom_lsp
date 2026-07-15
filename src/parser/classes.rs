use std::sync::Arc;

use crate::php_type::PhpType;
use crate::types::TypeAliasDef;
use crate::util::strip_fqn_prefix;
use mago_span::HasSpan;
use mago_syntax::cst::attribute::AttributeList;
use mago_syntax::cst::class_like::enum_case::EnumCaseItem;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::method::{Method, MethodBody};
use mago_syntax::cst::class_like::property::{Property, PropertyItem};
use mago_syntax::cst::class_like::trait_use::{
    TraitUseAdaptation, TraitUseMethodReference, TraitUseSpecification,
};
use mago_syntax::cst::sequence::Sequence;

/// Check whether a method has the `#[Scope]` attribute (Laravel 11+).
///
/// Scans the method's attribute lists for an attribute whose short name
/// is `Scope` (matching `#[Scope]`, `#[\Illuminate\Database\Eloquent\Attributes\Scope]`,
/// or any use-imported alias that ends with `Scope`).
fn has_scope_attribute(method: &Method<'_>) -> bool {
    for attr_list in method.attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            if last_segment(attr.name.value()) == b"Scope" {
                return true;
            }
        }
    }
    false
}
/// Extract the PHP attribute target bitmask from a class's attribute lists.
///
/// Scans for `#[\Attribute]` or `#[\Attribute(flags)]` and returns the
/// target bitmask.  Returns `0` when the class is not an attribute class.
///
/// Recognises these patterns:
/// - `#[Attribute]` / `#[\Attribute]` → `TARGET_ALL` (default)
/// - `#[Attribute(Attribute::TARGET_CLASS)]` → `TARGET_CLASS`
/// - `#[Attribute(Attribute::TARGET_CLASS | Attribute::TARGET_METHOD)]` → bitwise OR
/// - `#[Attribute(TARGET_CLASS | TARGET_METHOD)]` → short-form constants
/// - Numeric literals (e.g. `#[Attribute(1)]`, `#[Attribute(63)]`)
fn extract_attribute_targets(
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
    content: &str,
) -> u8 {
    use crate::types::attribute_target;

    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            let short = last_segment(attr.name.value());
            if short != b"Attribute" {
                continue;
            }

            // `#[\Attribute]` without arguments → TARGET_ALL.
            let Some(arg_list) = attr.argument_list.as_ref() else {
                return attribute_target::TARGET_ALL;
            };

            // No arguments inside parentheses → TARGET_ALL.
            let Some(first_arg) = arg_list.arguments.first() else {
                return attribute_target::TARGET_ALL;
            };

            // Extract the raw text of the first argument and parse
            // the target flags from it.
            let span = first_arg.span();
            let start = span.start.offset as usize;
            let end = span.end.offset as usize;
            let Some(text) = content.get(start..end) else {
                return attribute_target::TARGET_ALL;
            };

            return parse_attribute_target_flags(text);
        }
    }

    0
}

/// Parse a target-flag expression from the argument to `#[\Attribute(…)]`.
///
/// Handles `|`-separated lists of `Attribute::TARGET_*` or bare
/// `TARGET_*` constants, as well as plain integer literals.
fn parse_attribute_target_flags(text: &str) -> u8 {
    use crate::types::attribute_target;

    let text = text.trim();

    // Try plain integer literal first.
    if let Ok(n) = text.parse::<u8>() {
        return n;
    }

    let mut flags: u8 = 0;
    for part in text.split('|') {
        let part = part.trim();
        // Strip optional `Attribute::` or `self::` prefix.
        let constant = part
            .strip_prefix("Attribute::")
            .or_else(|| part.strip_prefix("\\Attribute::"))
            .or_else(|| part.strip_prefix("self::"))
            .unwrap_or(part);

        flags |= match constant {
            "TARGET_CLASS" => attribute_target::TARGET_CLASS,
            "TARGET_FUNCTION" => attribute_target::TARGET_FUNCTION,
            "TARGET_METHOD" => attribute_target::TARGET_METHOD,
            "TARGET_PROPERTY" => attribute_target::TARGET_PROPERTY,
            "TARGET_CLASS_CONSTANT" => attribute_target::TARGET_CLASS_CONSTANT,
            "TARGET_PARAMETER" => attribute_target::TARGET_PARAMETER,
            "TARGET_ALL" => attribute_target::TARGET_ALL,
            _ => {
                // Unrecognised constant — try parsing as an integer.
                constant.trim().parse::<u8>().unwrap_or_default()
            }
        };
    }

    // If we matched the `#[Attribute]` name but couldn't parse any
    // flags, default to TARGET_ALL.
    if flags == 0 {
        attribute_target::TARGET_ALL
    } else {
        flags
    }
}

/// Class, interface, trait, and enum extraction.
///
/// Each class-like declaration is tagged with a [`ClassLikeKind`] so that
/// downstream consumers (e.g. `throw new` completion) can distinguish
/// concrete classes from interfaces, traits, and enums.
///
/// This module handles extracting `ClassInfo` from the PHP AST for all
/// class-like declarations: `class`, `interface`, `trait`, and `enum`.
/// It also extracts class-like members (methods, properties, constants,
/// trait uses) and merges in PHPDoc `@property`, `@method`, `@mixin`,
/// and `@deprecated` annotations from docblocks.
///
/// Anonymous classes (`new class { ... }`) are also extracted.  They are
/// given synthetic names of the form `__anonymous@<offset>` so that
/// [`find_class_at_offset`](crate::util::find_class_at_offset) can resolve
/// `$this` inside their bodies.
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::{Atom, AtomMap, atom, atom_bytes, bytes_to_str, last_segment};
use crate::docblock;
use crate::types::*;
use crate::virtual_members::laravel::infer_relationship_from_body;

use super::{
    DeprecationInfo, DocblockCtx, extract_hint_type, extract_parameters, extract_property_info,
    extract_visibility, is_available_for_version, is_removed_for_version, merge_deprecation_info,
};

/// Docblock-derived metadata common to all class-like declarations.
///
/// Produced by [`extract_class_docblock`] and consumed by each match arm
/// in [`Backend::extract_classes_from_statements`] to avoid repeating
/// the same extraction calls for classes, interfaces, traits, and enums.
#[derive(Default)]
struct ClassDocblockInfo {
    /// Deprecation message from `@deprecated`, or `None` if not deprecated.
    deprecation_message: Option<String>,
    /// `@template` parameters declared on the class-like.
    template_params: Vec<Atom>,
    /// Upper bounds for template parameters (`@template T of Bound`).
    template_param_bounds: AtomMap<PhpType>,
    /// Default values for template parameters (`@template T of bool = false`).
    template_param_defaults: AtomMap<PhpType>,
    /// Generic arguments from `@extends` / `@phpstan-extends`.
    extends_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Generic arguments from `@implements` / `@phpstan-implements`.
    implements_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Generic arguments from `@use` / `@phpstan-use`.
    use_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Type aliases from `@phpstan-type` / `@psalm-type`.
    type_aliases: AtomMap<TypeAliasDef>,
    /// Mixin class names from `@mixin` tags.
    mixins: Vec<Atom>,
    /// Generic type arguments from `@mixin` tags.
    ///
    /// Each entry is `(MixinClassName, [TypeArg1, TypeArg2, …])`.
    /// Only populated for mixins that have generic arguments.
    mixin_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Required base class from a `@phpstan-require-extends` tag (traits only).
    require_extends: Option<Atom>,
    /// URLs from `@link` and `@see` tags in the class-level docblock.
    links: Vec<String>,
    /// `@see` references from the class-level docblock.
    see_refs: Vec<String>,
    /// Raw class-level docblock text, preserved for deferred `@method` /
    /// `@property` parsing by the `PHPDocProvider`.
    raw_docblock: Option<String>,
}

/// Extract all docblock-derived metadata from a class-like AST node.
///
/// Returns [`ClassDocblockInfo::default()`] when no docblock context is
/// available or when the node has no preceding doc comment.
fn extract_class_docblock<'a>(
    node: &impl HasSpan,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> ClassDocblockInfo {
    let Some(ctx) = doc_ctx else {
        return ClassDocblockInfo::default();
    };
    let Some(doc_text) = docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, node)
    else {
        return ClassDocblockInfo::default();
    };
    let Some(info) = docblock::parse_docblock_for_tags(doc_text) else {
        return ClassDocblockInfo::default();
    };

    let params_full = docblock::extract_template_params_full_from_info(&info);
    let template_params: Vec<Atom> = params_full.iter().map(|(n, _, _, _)| atom(n)).collect();
    let template_param_bounds: AtomMap<PhpType> = params_full
        .iter()
        .filter_map(|(name, bound, _, _)| bound.as_ref().map(|b| (atom(name), b.clone())))
        .collect();
    let template_param_defaults: AtomMap<PhpType> = params_full
        .into_iter()
        .filter_map(|(name, _, _, default)| default.map(|d| (atom(&name), d)))
        .collect();

    let mixin_data = docblock::extract_mixin_tags_from_info(&info);
    let mixins: Vec<Atom> = mixin_data.iter().map(|(name, _)| atom(name)).collect();
    let mixin_generics: Vec<(Atom, Vec<PhpType>)> = mixin_data
        .into_iter()
        .filter(|(_, args)| !args.is_empty())
        .map(|(name, args)| (atom(&name), args))
        .collect();

    ClassDocblockInfo {
        deprecation_message: docblock::extract_deprecation_message_from_info(&info),
        template_params,
        template_param_bounds,
        template_param_defaults,
        extends_generics: docblock::extract_generics_tag_from_info(&info, "@extends")
            .into_iter()
            .map(|(n, args)| (atom(&n), args))
            .collect(),
        implements_generics: docblock::extract_generics_tag_from_info(&info, "@implements")
            .into_iter()
            .map(|(n, args)| (atom(&n), args))
            .collect(),
        use_generics: docblock::extract_generics_tag_from_info(&info, "@use")
            .into_iter()
            .map(|(n, args)| (atom(&n), args))
            .collect(),
        type_aliases: docblock::extract_type_aliases_from_info(&info)
            .into_iter()
            .map(|(k, v)| (atom(&k), v))
            .collect(),
        mixins,
        mixin_generics,
        require_extends: docblock::extract_require_extends_from_info(&info).map(|n| atom(&n)),
        links: docblock::extract_link_urls_from_info(&info),
        see_refs: docblock::extract_see_references_from_info(&info),
        raw_docblock: Some(doc_text.to_string()),
    }
}

/// Extract the custom collection class name from a `#[CollectedBy(X::class)]` attribute.
///
/// Scans the class's attribute lists for an attribute whose short name is
/// `CollectedBy` and extracts the first argument's text with `::class` stripped.
/// Returns `None` if no such attribute exists.
fn extract_collected_by_attribute(
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
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
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
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

/// Determine the custom builder class for an Eloquent model.
///
/// Checks three sources in priority order:
///
/// 1. `#[UseEloquentBuilder(CustomBuilder::class)]` attribute on the class.
/// 2. `/** @use HasBuilder<CustomBuilder> */` in `use_generics`.
/// 3. A `newEloquentBuilder()` method override whose return type names the
///    custom builder class.
fn extract_custom_builder(
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
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
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
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
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    content: &str,
) -> Vec<(String, String)> {
    let mut property_text: Option<String> = None;
    let mut method_text: Option<String> = None;

    for member in members {
        match member {
            ClassLikeMember::Property(Property::Plain(plain)) => {
                for item in plain.items.iter() {
                    let var_name = bytes_to_str(item.variable().name).to_string();
                    let stripped = var_name.strip_prefix('$').unwrap_or(&var_name);
                    if stripped != "casts" {
                        continue;
                    }
                    if let PropertyItem::Concrete(concrete) = item {
                        let span = concrete.value.span();
                        let start = span.start.offset as usize;
                        let end = span.end.offset as usize;
                        if let Some(text) = content.get(start..end) {
                            property_text = Some(text.to_string());
                        }
                    }
                }
            }
            ClassLikeMember::Method(method) if method.name.value == b"casts" => {
                if let MethodBody::Concrete(block) = &method.body {
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
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    content: &str,
) -> Vec<(String, PhpType)> {
    for member in members {
        if let ClassLikeMember::Property(Property::Plain(plain)) = member {
            for item in plain.items.iter() {
                let var_name = bytes_to_str(item.variable().name).to_string();
                let stripped = var_name.strip_prefix('$').unwrap_or(&var_name);
                if stripped != "attributes" {
                    continue;
                }
                if let PropertyItem::Concrete(concrete) = item {
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
fn parse_attributes_array(text: &str) -> Vec<(String, PhpType)> {
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
            results.push((key, php_type));
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
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    content: &str,
) -> (Option<bool>, Option<Option<String>>, Option<Option<String>>) {
    let mut timestamps: Option<bool> = None;
    let mut created_at: Option<Option<String>> = None;
    let mut updated_at: Option<Option<String>> = None;

    for member in members {
        match member {
            ClassLikeMember::Property(Property::Plain(plain)) => {
                for item in plain.items.iter() {
                    let var_name = bytes_to_str(item.variable().name).to_string();
                    let stripped = var_name.strip_prefix('$').unwrap_or(&var_name);
                    if stripped != "timestamps" {
                        continue;
                    }
                    if let PropertyItem::Concrete(concrete) = item {
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
            ClassLikeMember::Constant(constant) => {
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
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    content: &str,
) -> Vec<String> {
    let mut names = Vec::new();
    let targets = ["fillable", "guarded", "hidden", "visible", "appends"];

    for member in members {
        if let ClassLikeMember::Property(Property::Plain(plain)) = member {
            for item in plain.items.iter() {
                let var_name = bytes_to_str(item.variable().name).to_string();
                let stripped = var_name.strip_prefix('$').unwrap_or(&var_name);
                if !targets.contains(&stripped) {
                    continue;
                }
                if let PropertyItem::Concrete(concrete) = item {
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
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    content: &str,
) -> Vec<String> {
    let mut names = Vec::new();

    for member in members {
        if let ClassLikeMember::Property(Property::Plain(plain)) = member {
            for item in plain.items.iter() {
                let var_name = bytes_to_str(item.variable().name).to_string();
                let stripped = var_name.strip_prefix('$').unwrap_or(&var_name);
                if stripped != "dates" {
                    continue;
                }
                if let PropertyItem::Concrete(concrete) = item {
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

/// Try to infer an Eloquent relationship return type from a method's body.
///
/// When a method has no `@return` annotation and no native return type
/// hint, this function extracts the method body text and scans it for
/// patterns like `$this->hasMany(Post::class)`.  If found, it returns
/// a synthesized return type string (e.g. `HasMany<Post>`).
///
/// This enables relationship property synthesis on models that don't
/// use Larastan-style `@return` annotations.
fn infer_relationship_from_method<'a>(
    method: &Method<'a>,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> Option<PhpType> {
    let ctx = doc_ctx?;
    let MethodBody::Concrete(block) = &method.body else {
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

impl Backend {
    /// De-duplicate parsed class-likes by `(name, namespace)`, keeping the
    /// first declaration in source order.
    ///
    /// A class-like declared in more than one branch of a conditional —
    /// e.g. Doctrine's `ServiceEntityRepository`, defined differently for
    /// ORM2 vs ORM3 inside an `if`/`else` version guard — yields one
    /// [`ClassInfo`] per branch once we descend into conditional bodies.
    /// Keeping the first declaration makes resolution deterministic and
    /// matches PHPantom's existing first-occurrence-wins convention
    /// (`classmap_scanner`, `find_class_by_name`, `fqn_uri_index`) as well
    /// as PHPStan/Psalm. This must run before the classes reach
    /// `fqn_class_index`, whose insert is last-wins and would otherwise pick
    /// the wrong (later) branch.
    pub(crate) fn dedup_class_likes_first_wins(items: &mut Vec<(ClassInfo, Option<String>)>) {
        let mut seen: std::collections::HashSet<(Atom, Option<String>)> =
            std::collections::HashSet::new();
        items.retain(|(cls, ns)| seen.insert((cls.name, ns.clone())));
    }

    /// Recursively walk statements and extract class information.
    /// This handles classes at the top level as well as classes nested
    /// inside namespace declarations.
    pub(crate) fn extract_classes_from_statements<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        for statement in statements {
            match statement {
                Statement::Class(class) => {
                    // Skip classes whose docblock has `@removed X.Y`
                    // where X.Y <= the target PHP version.
                    if let Some(ctx) = doc_ctx
                        && let Some(ver) = ctx.php_version
                        && is_removed_for_version(class, ctx, ver)
                    {
                        continue;
                    }

                    let class_name = atom_bytes(class.name.value);

                    let parent_class = class
                        .extends
                        .as_ref()
                        .and_then(|ext| ext.types.first().map(|ident| atom_bytes(ident.value())));

                    let interfaces: Vec<Atom> = class
                        .implements
                        .as_ref()
                        .map(|imp| {
                            imp.types
                                .iter()
                                .map(|ident| atom_bytes(ident.value()))
                                .collect()
                        })
                        .unwrap_or_default();

                    let doc_info = extract_class_docblock(class, doc_ctx);

                    let ExtractedMembers {
                        methods,
                        properties,
                        constants,
                        used_traits,
                        trait_precedences,
                        trait_aliases,
                        inline_use_generics,
                    } = Self::extract_class_like_members(
                        class.members.iter(),
                        doc_ctx,
                        &doc_info.template_params,
                    );

                    let mut use_generics: Vec<(Atom, Vec<PhpType>)> = doc_info.use_generics;
                    use_generics.extend(inline_use_generics);

                    let keyword_offset = class.class.span.start.offset;
                    let decl_start_offset = class
                        .attribute_lists
                        .first()
                        .map_or(keyword_offset, |a| a.span().start.offset);
                    let start_offset = class.left_brace.start.offset;
                    let end_offset = class.right_brace.end.offset;

                    let content = doc_ctx.map(|c| c.content).unwrap_or("");
                    let custom_collection = extract_custom_collection(
                        &class.attribute_lists,
                        &use_generics,
                        &methods,
                        content,
                    );

                    let custom_builder = extract_custom_builder(
                        &class.attribute_lists,
                        &use_generics,
                        &methods,
                        content,
                    );

                    let casts_definitions =
                        extract_casts_definitions(class.members.iter(), content);

                    let attributes_definitions =
                        extract_attributes_definitions(class.members.iter(), content);

                    let column_names = extract_column_names(class.members.iter(), content);

                    let dates_definitions =
                        extract_dates_definitions(class.members.iter(), content);

                    let (timestamps, created_at_name, updated_at_name) =
                        extract_timestamp_config(class.members.iter(), content);

                    let attr_targets = extract_attribute_targets(&class.attribute_lists, content);

                    let class_depr = merge_deprecation_info(
                        doc_info.deprecation_message.clone(),
                        &class.attribute_lists,
                        doc_ctx,
                    );
                    classes.push(ClassInfo {
                        kind: ClassLikeKind::Class,
                        name: class_name,
                        methods: methods.into_iter().map(Arc::new).collect::<Vec<_>>().into(),
                        properties: properties.into(),
                        constants: constants.into(),
                        start_offset,
                        end_offset,
                        keyword_offset,
                        decl_start_offset,
                        parent_class,
                        interfaces,
                        used_traits,
                        mixins: doc_info.mixins,
                        mixin_generics: doc_info.mixin_generics,
                        require_extends: doc_info.require_extends,
                        is_final: class.modifiers.contains_final(),
                        is_abstract: class.modifiers.contains_abstract(),
                        deprecation_message: class_depr.message,
                        deprecated_replacement: class_depr.replacement,
                        links: doc_info.links,
                        see_refs: doc_info.see_refs,
                        template_params: doc_info.template_params,
                        template_param_bounds: doc_info.template_param_bounds,
                        template_param_defaults: doc_info.template_param_defaults,
                        extends_generics: doc_info.extends_generics,
                        implements_generics: doc_info.implements_generics,
                        use_generics,
                        type_aliases: doc_info.type_aliases,
                        trait_precedences,
                        trait_aliases,
                        class_docblock: doc_info.raw_docblock,
                        file_namespace: None,
                        backed_type: None,
                        attribute_targets: attr_targets,
                        method_index: Default::default(),
                        indexed_method_count: 0,
                        laravel: Some(Box::new(LaravelMetadata {
                            custom_collection,
                            casts_definitions,
                            dates_definitions,
                            attributes_definitions,
                            column_names,
                            timestamps,
                            created_at_name,
                            updated_at_name,
                            custom_builder,
                        })),
                    });

                    // Walk method bodies for anonymous classes.
                    Self::find_anonymous_classes_in_members(class.members.iter(), classes, doc_ctx);
                }
                Statement::Interface(iface) => {
                    // Skip interfaces whose docblock has `@removed X.Y`
                    // where X.Y <= the target PHP version.
                    if let Some(ctx) = doc_ctx
                        && let Some(ver) = ctx.php_version
                        && is_removed_for_version(iface, ctx, ver)
                    {
                        continue;
                    }

                    let iface_name = atom_bytes(iface.name.value);

                    // Interfaces can extend multiple parent interfaces.
                    // Store the first one in `parent_class` for backward
                    // compatibility with single-inheritance resolution,
                    // and all of them in `interfaces` so that transitive
                    // interface inheritance checks work correctly.
                    let all_parents: Vec<Atom> = iface
                        .extends
                        .as_ref()
                        .map(|ext| {
                            ext.types
                                .iter()
                                .map(|ident| atom_bytes(ident.value()))
                                .collect()
                        })
                        .unwrap_or_default();

                    let parent_class = all_parents.first().copied();

                    let doc_info = extract_class_docblock(iface, doc_ctx);

                    let ExtractedMembers {
                        methods,
                        properties,
                        constants,
                        used_traits,
                        trait_precedences,
                        trait_aliases,
                        inline_use_generics,
                    } = Self::extract_class_like_members(
                        iface.members.iter(),
                        doc_ctx,
                        &doc_info.template_params,
                    );

                    let keyword_offset = iface.interface.span.start.offset;
                    let decl_start_offset = iface
                        .attribute_lists
                        .first()
                        .map_or(keyword_offset, |a| a.span().start.offset);
                    let start_offset = iface.left_brace.start.offset;
                    let end_offset = iface.right_brace.end.offset;

                    let iface_depr = merge_deprecation_info(
                        doc_info.deprecation_message.clone(),
                        &iface.attribute_lists,
                        doc_ctx,
                    );
                    classes.push(ClassInfo {
                        kind: ClassLikeKind::Interface,
                        name: iface_name,
                        methods: methods.into_iter().map(Arc::new).collect::<Vec<_>>().into(),
                        properties: properties.into(),
                        constants: constants.into(),
                        start_offset,
                        end_offset,
                        keyword_offset,
                        decl_start_offset,
                        parent_class,
                        interfaces: all_parents,
                        used_traits,
                        mixins: doc_info.mixins,
                        mixin_generics: doc_info.mixin_generics,
                        require_extends: doc_info.require_extends,
                        is_final: false,
                        is_abstract: false,
                        deprecation_message: iface_depr.message,
                        deprecated_replacement: iface_depr.replacement,
                        links: doc_info.links,
                        see_refs: doc_info.see_refs,
                        template_params: doc_info.template_params,
                        template_param_bounds: doc_info.template_param_bounds,
                        template_param_defaults: doc_info.template_param_defaults,
                        extends_generics: doc_info.extends_generics,
                        implements_generics: doc_info.implements_generics,
                        use_generics: {
                            let mut ug = doc_info.use_generics;
                            ug.extend(inline_use_generics);
                            ug
                        },
                        type_aliases: doc_info.type_aliases,
                        trait_precedences,
                        trait_aliases,
                        class_docblock: doc_info.raw_docblock,
                        file_namespace: None,
                        backed_type: None,
                        attribute_targets: 0,
                        method_index: Default::default(),
                        indexed_method_count: 0,
                        laravel: None,
                    });

                    // Walk method bodies for anonymous classes.
                    Self::find_anonymous_classes_in_members(iface.members.iter(), classes, doc_ctx);
                }
                Statement::Trait(trait_def) => {
                    // Skip traits whose docblock has `@removed X.Y`
                    // where X.Y <= the target PHP version.
                    if let Some(ctx) = doc_ctx
                        && let Some(ver) = ctx.php_version
                        && is_removed_for_version(trait_def, ctx, ver)
                    {
                        continue;
                    }

                    let trait_name = atom_bytes(trait_def.name.value);

                    let doc_info = extract_class_docblock(trait_def, doc_ctx);

                    let ExtractedMembers {
                        methods,
                        properties,
                        constants,
                        used_traits,
                        trait_precedences,
                        trait_aliases,
                        inline_use_generics,
                    } = Self::extract_class_like_members(
                        trait_def.members.iter(),
                        doc_ctx,
                        &doc_info.template_params,
                    );

                    let keyword_offset = trait_def.r#trait.span.start.offset;
                    let decl_start_offset = trait_def
                        .attribute_lists
                        .first()
                        .map_or(keyword_offset, |a| a.span().start.offset);
                    let start_offset = trait_def.left_brace.start.offset;
                    let end_offset = trait_def.right_brace.end.offset;

                    let trait_depr = merge_deprecation_info(
                        doc_info.deprecation_message.clone(),
                        &trait_def.attribute_lists,
                        doc_ctx,
                    );
                    classes.push(ClassInfo {
                        kind: ClassLikeKind::Trait,
                        name: trait_name,
                        methods: methods.into_iter().map(Arc::new).collect::<Vec<_>>().into(),
                        properties: properties.into(),
                        constants: constants.into(),
                        start_offset,
                        end_offset,
                        keyword_offset,
                        decl_start_offset,
                        parent_class: None,
                        interfaces: vec![],
                        used_traits,
                        mixins: doc_info.mixins,
                        mixin_generics: doc_info.mixin_generics,
                        require_extends: doc_info.require_extends,
                        is_final: false,
                        is_abstract: false,
                        deprecation_message: trait_depr.message,
                        deprecated_replacement: trait_depr.replacement,
                        links: doc_info.links,
                        see_refs: doc_info.see_refs,
                        template_params: doc_info.template_params,
                        template_param_bounds: doc_info.template_param_bounds,
                        template_param_defaults: doc_info.template_param_defaults,
                        extends_generics: vec![],
                        implements_generics: vec![],
                        use_generics: inline_use_generics,
                        type_aliases: AtomMap::default(),
                        trait_precedences,
                        trait_aliases,
                        class_docblock: doc_info.raw_docblock,
                        file_namespace: None,
                        backed_type: None,
                        attribute_targets: 0,
                        method_index: Default::default(),
                        indexed_method_count: 0,
                        laravel: None,
                    });

                    // Walk method bodies for anonymous classes.
                    Self::find_anonymous_classes_in_members(
                        trait_def.members.iter(),
                        classes,
                        doc_ctx,
                    );
                }
                Statement::Enum(enum_def) => {
                    // Skip enums whose docblock has `@removed X.Y`
                    // where X.Y <= the target PHP version.
                    if let Some(ctx) = doc_ctx
                        && let Some(ver) = ctx.php_version
                        && is_removed_for_version(enum_def, ctx, ver)
                    {
                        continue;
                    }

                    let enum_name = atom_bytes(enum_def.name.value);

                    let ExtractedMembers {
                        methods,
                        mut properties,
                        constants,
                        mut used_traits,
                        ..
                    } = Self::extract_class_like_members(enum_def.members.iter(), doc_ctx, &[]);

                    // Every enum case exposes a readonly `name` property, and
                    // backed enums additionally expose a `value` property whose
                    // type is the backing type.  These are real instance
                    // properties in PHP (declared on the UnitEnum/BackedEnum
                    // interfaces via stubs, but interface properties are not
                    // merged into implementors), so synthesize them here.
                    properties.push(crate::types::PropertyInfo::virtual_property_typed(
                        "name",
                        Some(&PhpType::Named("string".to_string())),
                    ));
                    if let Some(hint) = enum_def.backing_type_hint.as_ref() {
                        let value_type = crate::parser::extract_hint_type(&hint.hint);
                        properties.push(crate::types::PropertyInfo::virtual_property_typed(
                            "value",
                            Some(&value_type),
                        ));
                    }

                    // Enums implicitly implement UnitEnum or BackedEnum.
                    // We add the interface with a leading backslash so that
                    // `resolve_name` treats it as fully-qualified and does not
                    // prepend the current namespace.  `resolve_name` then
                    // strips the `\` to produce the canonical form (`BackedEnum`
                    // / `UnitEnum`).  The class_loader / merge_traits_into path
                    // will pick up the interface from the SPL stubs and merge
                    // its methods (cases, from, tryFrom, …) automatically.
                    let implicit_interface = if enum_def.backing_type_hint.is_some() {
                        "\\BackedEnum"
                    } else {
                        "\\UnitEnum"
                    };
                    used_traits.push(atom(implicit_interface));

                    let doc_info = extract_class_docblock(enum_def, doc_ctx);

                    let mut interfaces: Vec<Atom> = enum_def
                        .implements
                        .as_ref()
                        .map(|imp| {
                            imp.types
                                .iter()
                                .map(|ident| atom_bytes(ident.value()))
                                .collect()
                        })
                        .unwrap_or_default();

                    // Also add the implicit interface to the interfaces
                    // list so that hierarchy checks (`is_subtype_of`)
                    // recognise backed enums as subtypes of `BackedEnum`
                    // and all enums as subtypes of `UnitEnum`.
                    if !interfaces.iter().any(|i| {
                        i.trim_start_matches('\\') == implicit_interface.trim_start_matches('\\')
                    }) {
                        interfaces.push(atom(implicit_interface));
                    }

                    let keyword_offset = enum_def.r#enum.span.start.offset;
                    let decl_start_offset = enum_def
                        .attribute_lists
                        .first()
                        .map_or(keyword_offset, |a| a.span().start.offset);
                    let start_offset = enum_def.left_brace.start.offset;
                    let end_offset = enum_def.right_brace.end.offset;

                    // Enums are implicitly final and cannot be extended.
                    let enum_depr = merge_deprecation_info(
                        doc_info.deprecation_message,
                        &enum_def.attribute_lists,
                        doc_ctx,
                    );
                    classes.push(ClassInfo {
                        kind: ClassLikeKind::Enum,
                        name: enum_name,
                        methods: methods.into_iter().map(Arc::new).collect::<Vec<_>>().into(),
                        properties: properties.into(),
                        constants: constants.into(),
                        start_offset,
                        end_offset,
                        keyword_offset,
                        decl_start_offset,
                        parent_class: None,
                        interfaces,
                        used_traits,
                        mixins: doc_info.mixins,
                        mixin_generics: doc_info.mixin_generics,
                        require_extends: doc_info.require_extends,
                        is_final: true,
                        is_abstract: false,
                        deprecation_message: enum_depr.message,
                        deprecated_replacement: enum_depr.replacement,
                        links: doc_info.links,
                        see_refs: doc_info.see_refs,
                        template_params: vec![],
                        template_param_bounds: AtomMap::default(),
                        template_param_defaults: AtomMap::default(),
                        extends_generics: vec![],
                        implements_generics: doc_info.implements_generics,
                        use_generics: vec![],
                        type_aliases: AtomMap::default(),
                        trait_precedences: vec![],
                        trait_aliases: vec![],
                        class_docblock: doc_info.raw_docblock,
                        file_namespace: None,
                        backed_type: enum_def.backing_type_hint.as_ref().and_then(|h| {
                            let ty = crate::parser::extract_hint_type(&h.hint);
                            if ty.is_string_type() {
                                Some(crate::types::BackedEnumType::String)
                            } else if ty.is_int() {
                                Some(crate::types::BackedEnumType::Int)
                            } else {
                                None
                            }
                        }),
                        attribute_targets: 0,
                        method_index: Default::default(),
                        indexed_method_count: 0,
                        laravel: None,
                    });

                    // Walk method bodies for anonymous classes.
                    Self::find_anonymous_classes_in_members(
                        enum_def.members.iter(),
                        classes,
                        doc_ctx,
                    );
                }
                Statement::Namespace(namespace) => {
                    Self::extract_classes_from_statements(
                        namespace.statements().iter(),
                        classes,
                        doc_ctx,
                    );
                }
                // Named class-likes can be declared inside conditional and
                // control-flow blocks — most notably Doctrine's
                // `ServiceEntityRepository`, defined inside an
                // `if (! property_exists(EntityRepository::class, '_entityName'))`
                // guard that selects the ORM2 vs ORM3 base class. Descend into
                // these container bodies so such declarations are indexed with
                // their parent class and `@extends` generics, not merely
                // discovered by name. Anonymous classes nested in the
                // non-container statements within these bodies are still
                // collected via the `_` arm when the recursion reaches them.
                Statement::If(if_stmt) => {
                    Self::extract_classes_from_statements(
                        if_stmt.body.statements().iter(),
                        classes,
                        doc_ctx,
                    );
                    for else_if in if_stmt.body.else_if_statements() {
                        Self::extract_classes_from_statements(else_if.iter(), classes, doc_ctx);
                    }
                    if let Some(else_stmts) = if_stmt.body.else_statements() {
                        Self::extract_classes_from_statements(else_stmts.iter(), classes, doc_ctx);
                    }
                }
                Statement::Block(block) => {
                    Self::extract_classes_from_statements(
                        block.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
                Statement::Try(try_stmt) => {
                    Self::extract_classes_from_statements(
                        try_stmt.block.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                    for catch in try_stmt.catch_clauses.iter() {
                        Self::extract_classes_from_statements(
                            catch.block.statements.iter(),
                            classes,
                            doc_ctx,
                        );
                    }
                    if let Some(finally) = &try_stmt.finally_clause {
                        Self::extract_classes_from_statements(
                            finally.block.statements.iter(),
                            classes,
                            doc_ctx,
                        );
                    }
                }
                Statement::Switch(switch_stmt) => {
                    for case in switch_stmt.body.cases() {
                        Self::extract_classes_from_statements(
                            case.statements().iter(),
                            classes,
                            doc_ctx,
                        );
                    }
                }
                Statement::While(while_stmt) => {
                    Self::extract_classes_from_statements(
                        while_stmt.body.statements().iter(),
                        classes,
                        doc_ctx,
                    );
                }
                Statement::DoWhile(do_while) => {
                    Self::extract_classes_from_statements(
                        std::iter::once(do_while.statement),
                        classes,
                        doc_ctx,
                    );
                }
                Statement::For(for_stmt) => {
                    Self::extract_classes_from_statements(
                        for_stmt.body.statements().iter(),
                        classes,
                        doc_ctx,
                    );
                }
                Statement::Foreach(foreach_stmt) => {
                    Self::extract_classes_from_statements(
                        foreach_stmt.body.statements().iter(),
                        classes,
                        doc_ctx,
                    );
                }
                _ => {
                    // Walk into all other statement types to find anonymous
                    // classes nested inside expressions, control flow, method
                    // bodies, closures, etc.
                    Self::find_anonymous_classes_in_statement(statement, classes, doc_ctx);
                }
            }
        }
    }

    // ─── Anonymous class extraction ─────────────────────────────────────

    /// Extract an anonymous class node into a [`ClassInfo`] with a
    /// synthetic name `__anonymous@<offset>`.
    fn extract_anonymous_class_info<'a>(
        anon: &AnonymousClass<'a>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) -> ClassInfo {
        let parent_class = anon
            .extends
            .as_ref()
            .and_then(|ext| ext.types.first().map(|ident| atom_bytes(ident.value())));

        let interfaces: Vec<Atom> = anon
            .implements
            .as_ref()
            .map(|imp| {
                imp.types
                    .iter()
                    .map(|ident| atom_bytes(ident.value()))
                    .collect()
            })
            .unwrap_or_default();

        let ExtractedMembers {
            methods,
            properties,
            constants,
            used_traits,
            trait_precedences,
            trait_aliases,
            ..
        } = Self::extract_class_like_members(anon.members.iter(), doc_ctx, &[]);

        let start_offset = anon.left_brace.start.offset;
        let end_offset = anon.right_brace.end.offset;
        // Anonymous classes don't have a meaningful keyword_offset for
        // go-to-definition purposes — use 0 ("not available").
        let keyword_offset = 0;
        let name = atom(&format!("__anonymous@{}", start_offset));

        ClassInfo {
            kind: ClassLikeKind::Class,
            name,
            methods: methods.into_iter().map(Arc::new).collect::<Vec<_>>().into(),
            properties: properties.into(),
            constants: constants.into(),
            start_offset,
            end_offset,
            keyword_offset,
            decl_start_offset: start_offset,
            parent_class,
            interfaces,
            used_traits,
            mixins: vec![],
            mixin_generics: vec![],
            require_extends: None,
            is_final: false,
            is_abstract: false,
            deprecation_message: None,
            deprecated_replacement: None,
            template_params: vec![],
            template_param_bounds: AtomMap::default(),
            template_param_defaults: AtomMap::default(),
            extends_generics: vec![],
            implements_generics: vec![],
            use_generics: vec![],
            type_aliases: AtomMap::default(),
            trait_precedences,
            trait_aliases,
            links: Vec::new(),
            see_refs: Vec::new(),
            class_docblock: None,
            file_namespace: None,
            backed_type: None,
            attribute_targets: 0,
            method_index: Default::default(),
            indexed_method_count: 0,
            laravel: None,
        }
    }

    /// Recursively walk a statement looking for anonymous classes in
    /// expressions and nested statement blocks.
    pub(crate) fn find_anonymous_classes_in_statement<'a>(
        statement: &'a Statement<'a>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        match statement {
            Statement::Expression(expr_stmt) => {
                Self::find_anonymous_classes_in_expression(expr_stmt.expression, classes, doc_ctx);
            }
            Statement::Return(ret) => {
                if let Some(value) = &ret.value {
                    Self::find_anonymous_classes_in_expression(value, classes, doc_ctx);
                }
            }
            Statement::Block(block) => {
                Self::walk_statements_for_anonymous_classes(
                    block.statements.iter(),
                    classes,
                    doc_ctx,
                );
            }
            Statement::If(if_stmt) => {
                Self::find_anonymous_classes_in_if_body(&if_stmt.body, classes, doc_ctx);
            }
            Statement::While(while_stmt) => match &while_stmt.body {
                WhileBody::Statement(stmt) => {
                    Self::find_anonymous_classes_in_statement(stmt, classes, doc_ctx);
                }
                WhileBody::ColonDelimited(body) => {
                    Self::walk_statements_for_anonymous_classes(
                        body.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
            },
            Statement::DoWhile(do_while) => {
                Self::find_anonymous_classes_in_statement(do_while.statement, classes, doc_ctx);
            }
            Statement::For(for_stmt) => match &for_stmt.body {
                ForBody::Statement(stmt) => {
                    Self::find_anonymous_classes_in_statement(stmt, classes, doc_ctx);
                }
                ForBody::ColonDelimited(body) => {
                    Self::walk_statements_for_anonymous_classes(
                        body.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
            },
            Statement::Foreach(foreach_stmt) => match &foreach_stmt.body {
                ForeachBody::Statement(stmt) => {
                    Self::find_anonymous_classes_in_statement(stmt, classes, doc_ctx);
                }
                ForeachBody::ColonDelimited(body) => {
                    Self::walk_statements_for_anonymous_classes(
                        body.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
            },
            Statement::Switch(switch_stmt) => {
                let cases = match &switch_stmt.body {
                    SwitchBody::BraceDelimited(b) => &b.cases,
                    SwitchBody::ColonDelimited(b) => &b.cases,
                };
                for case in cases.iter() {
                    let stmts = match case {
                        SwitchCase::Expression(c) => &c.statements,
                        SwitchCase::Default(c) => &c.statements,
                    };
                    Self::walk_statements_for_anonymous_classes(stmts.iter(), classes, doc_ctx);
                }
            }
            Statement::Try(try_stmt) => {
                Self::walk_statements_for_anonymous_classes(
                    try_stmt.block.statements.iter(),
                    classes,
                    doc_ctx,
                );
                for catch in try_stmt.catch_clauses.iter() {
                    Self::walk_statements_for_anonymous_classes(
                        catch.block.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
                if let Some(finally) = &try_stmt.finally_clause {
                    Self::walk_statements_for_anonymous_classes(
                        finally.block.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
            }
            Statement::Function(func) => {
                Self::walk_statements_for_anonymous_classes(
                    func.body.statements.iter(),
                    classes,
                    doc_ctx,
                );
            }
            // Named class-like declarations: walk method bodies to find
            // anonymous classes used inside methods.
            Statement::Class(class) => {
                Self::find_anonymous_classes_in_members(class.members.iter(), classes, doc_ctx);
            }
            Statement::Interface(iface) => {
                Self::find_anonymous_classes_in_members(iface.members.iter(), classes, doc_ctx);
            }
            Statement::Trait(trait_def) => {
                Self::find_anonymous_classes_in_members(trait_def.members.iter(), classes, doc_ctx);
            }
            Statement::Enum(enum_def) => {
                Self::find_anonymous_classes_in_members(enum_def.members.iter(), classes, doc_ctx);
            }
            Statement::Namespace(ns) => {
                Self::walk_statements_for_anonymous_classes(
                    ns.statements().iter(),
                    classes,
                    doc_ctx,
                );
            }
            Statement::Echo(echo) => {
                for expr in echo.values.iter() {
                    Self::find_anonymous_classes_in_expression(expr, classes, doc_ctx);
                }
            }
            _ => {}
        }
    }

    /// Walk class-like member method bodies to find anonymous classes.
    fn find_anonymous_classes_in_members<'a>(
        members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        for member in members {
            if let ClassLikeMember::Method(method) = member
                && let MethodBody::Concrete(block) = &method.body
            {
                Self::walk_statements_for_anonymous_classes(
                    block.statements.iter(),
                    classes,
                    doc_ctx,
                );
            }
        }
    }

    /// Walk a sequence of statements, dispatching each to the
    /// anonymous-class finder.
    fn walk_statements_for_anonymous_classes<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        for stmt in statements {
            Self::find_anonymous_classes_in_statement(stmt, classes, doc_ctx);
        }
    }

    /// Helper: recurse into an `if` statement body for anonymous classes.
    fn find_anonymous_classes_in_if_body<'a>(
        body: &'a IfBody<'a>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        match body {
            IfBody::Statement(body) => {
                Self::find_anonymous_classes_in_statement(body.statement, classes, doc_ctx);
                for else_if in body.else_if_clauses.iter() {
                    Self::find_anonymous_classes_in_statement(else_if.statement, classes, doc_ctx);
                }
                if let Some(else_clause) = &body.else_clause {
                    Self::find_anonymous_classes_in_statement(
                        else_clause.statement,
                        classes,
                        doc_ctx,
                    );
                }
            }
            IfBody::ColonDelimited(body) => {
                Self::walk_statements_for_anonymous_classes(
                    body.statements.iter(),
                    classes,
                    doc_ctx,
                );
                for else_if in body.else_if_clauses.iter() {
                    Self::walk_statements_for_anonymous_classes(
                        else_if.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
                if let Some(else_clause) = &body.else_clause {
                    Self::walk_statements_for_anonymous_classes(
                        else_clause.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
            }
        }
    }

    /// Recursively walk an expression tree looking for
    /// `Expression::AnonymousClass` nodes.
    fn find_anonymous_classes_in_expression<'a>(
        expr: &'a Expression<'a>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        match expr {
            Expression::AnonymousClass(anon) => {
                let info = Self::extract_anonymous_class_info(anon, doc_ctx);
                classes.push(info);
                // Also recurse into the anonymous class's method bodies
                // to find nested anonymous classes.
                Self::find_anonymous_classes_in_members(anon.members.iter(), classes, doc_ctx);
            }
            Expression::Assignment(assignment) => {
                Self::find_anonymous_classes_in_expression(assignment.lhs, classes, doc_ctx);
                Self::find_anonymous_classes_in_expression(assignment.rhs, classes, doc_ctx);
            }
            Expression::Parenthesized(paren) => {
                Self::find_anonymous_classes_in_expression(paren.expression, classes, doc_ctx);
            }
            Expression::Binary(binary) => {
                Self::find_anonymous_classes_in_expression(binary.lhs, classes, doc_ctx);
                Self::find_anonymous_classes_in_expression(binary.rhs, classes, doc_ctx);
            }
            Expression::UnaryPrefix(unary) => {
                Self::find_anonymous_classes_in_expression(unary.operand, classes, doc_ctx);
            }
            Expression::UnaryPostfix(unary) => {
                Self::find_anonymous_classes_in_expression(unary.operand, classes, doc_ctx);
            }
            Expression::Conditional(cond) => {
                Self::find_anonymous_classes_in_expression(cond.condition, classes, doc_ctx);
                if let Some(then) = &cond.then {
                    Self::find_anonymous_classes_in_expression(then, classes, doc_ctx);
                }
                Self::find_anonymous_classes_in_expression(cond.r#else, classes, doc_ctx);
            }
            Expression::Call(call) => {
                Self::find_anonymous_classes_in_argument_list(
                    call.get_argument_list(),
                    classes,
                    doc_ctx,
                );
                // Also walk the object/class/function expression
                match call {
                    Call::Function(fc) => {
                        Self::find_anonymous_classes_in_expression(fc.function, classes, doc_ctx);
                    }
                    Call::Method(mc) => {
                        Self::find_anonymous_classes_in_expression(mc.object, classes, doc_ctx);
                    }
                    Call::NullSafeMethod(nmc) => {
                        Self::find_anonymous_classes_in_expression(nmc.object, classes, doc_ctx);
                    }
                    Call::StaticMethod(smc) => {
                        Self::find_anonymous_classes_in_expression(smc.class, classes, doc_ctx);
                    }
                }
            }
            Expression::Instantiation(inst) => {
                Self::find_anonymous_classes_in_expression(inst.class, classes, doc_ctx);
                if let Some(args) = &inst.argument_list {
                    Self::find_anonymous_classes_in_argument_list(args, classes, doc_ctx);
                }
            }
            Expression::Throw(throw) => {
                Self::find_anonymous_classes_in_expression(throw.exception, classes, doc_ctx);
            }
            Expression::Clone(clone) => {
                Self::find_anonymous_classes_in_expression(clone.object, classes, doc_ctx);
            }
            Expression::Yield(yld) => match yld {
                Yield::Value(yv) => {
                    if let Some(value) = &yv.value {
                        Self::find_anonymous_classes_in_expression(value, classes, doc_ctx);
                    }
                }
                Yield::Pair(yp) => {
                    Self::find_anonymous_classes_in_expression(yp.key, classes, doc_ctx);
                    Self::find_anonymous_classes_in_expression(yp.value, classes, doc_ctx);
                }
                Yield::From(yf) => {
                    Self::find_anonymous_classes_in_expression(yf.iterator, classes, doc_ctx);
                }
            },
            Expression::Match(match_expr) => {
                Self::find_anonymous_classes_in_expression(match_expr.expression, classes, doc_ctx);
                for arm in match_expr.arms.iter() {
                    let arm_expr = arm.expression();
                    Self::find_anonymous_classes_in_expression(arm_expr, classes, doc_ctx);
                }
            }
            Expression::Array(array) => {
                for element in array.elements.iter() {
                    Self::find_anonymous_classes_in_array_element(element, classes, doc_ctx);
                }
            }
            Expression::LegacyArray(array) => {
                for element in array.elements.iter() {
                    Self::find_anonymous_classes_in_array_element(element, classes, doc_ctx);
                }
            }
            Expression::ArrayAccess(access) => {
                Self::find_anonymous_classes_in_expression(access.array, classes, doc_ctx);
                Self::find_anonymous_classes_in_expression(access.index, classes, doc_ctx);
            }
            Expression::Access(access) => match access {
                Access::Property(pa) => {
                    Self::find_anonymous_classes_in_expression(pa.object, classes, doc_ctx);
                }
                Access::NullSafeProperty(npa) => {
                    Self::find_anonymous_classes_in_expression(npa.object, classes, doc_ctx);
                }
                Access::StaticProperty(spa) => {
                    Self::find_anonymous_classes_in_expression(spa.class, classes, doc_ctx);
                }
                Access::ClassConstant(cca) => {
                    Self::find_anonymous_classes_in_expression(cca.class, classes, doc_ctx);
                }
            },
            Expression::Closure(closure) => {
                Self::walk_statements_for_anonymous_classes(
                    closure.body.statements.iter(),
                    classes,
                    doc_ctx,
                );
            }
            Expression::ArrowFunction(arrow) => {
                Self::find_anonymous_classes_in_expression(arrow.expression, classes, doc_ctx);
            }
            // Terminal expressions that cannot contain anonymous classes.
            Expression::Literal(_)
            | Expression::Variable(_)
            | Expression::Identifier(_)
            | Expression::ConstantAccess(_)
            | Expression::MagicConstant(_)
            | Expression::Parent(_)
            | Expression::Static(_)
            | Expression::Self_(_)
            | Expression::Error(_) => {}
            // Catch-all for less common expression types (Construct,
            // CompositeString, List, Pipe, ArrayAppend, PartialApplication).
            // These rarely contain anonymous classes, but if they do,
            // we'll miss them — acceptable for a first implementation.
            _ => {}
        }
    }

    /// Walk an argument list to find anonymous classes in argument values.
    fn find_anonymous_classes_in_argument_list<'a>(
        args: &'a ArgumentList<'a>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        for arg in args.arguments.iter() {
            let expr = match arg {
                Argument::Positional(pos) => pos.value,
                Argument::Named(named) => named.value,
            };
            Self::find_anonymous_classes_in_expression(expr, classes, doc_ctx);
        }
    }

    /// Walk an array element to find anonymous classes in values/keys.
    fn find_anonymous_classes_in_array_element<'a>(
        element: &'a ArrayElement<'a>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        match element {
            ArrayElement::KeyValue(kv) => {
                Self::find_anonymous_classes_in_expression(kv.key, classes, doc_ctx);
                Self::find_anonymous_classes_in_expression(kv.value, classes, doc_ctx);
            }
            ArrayElement::Value(v) => {
                Self::find_anonymous_classes_in_expression(v.value, classes, doc_ctx);
            }
            ArrayElement::Variadic(v) => {
                Self::find_anonymous_classes_in_expression(v.value, classes, doc_ctx);
            }
            ArrayElement::Missing(_) => {}
        }
    }

    /// Extract methods, properties, constants, and used trait names from
    /// class-like members.
    ///
    /// This is shared between `Statement::Class`, `Statement::Interface`,
    /// and `Statement::Trait` since all use the same `ClassLikeMember`
    /// representation.
    ///
    /// When `doc_ctx` is provided, PHPDoc `@return` and `@var` tags are used
    /// to refine (or supply) type information for methods and properties.
    pub(crate) fn extract_class_like_members<'a>(
        members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
        doc_ctx: Option<&DocblockCtx<'a>>,
        class_template_params: &[Atom],
    ) -> ExtractedMembers {
        /// Resolve a short class name to its FQN using the file's use-map
        /// and namespace from [`DocblockCtx`].  When no context is
        /// available the name is returned as-is.
        fn resolve_name_via_ctx(name: &str, doc_ctx: Option<&DocblockCtx<'_>>) -> String {
            // Already fully-qualified — strip the leading `\`.
            if let Some(stripped) = name.strip_prefix('\\') {
                return stripped.to_string();
            }
            let Some(ctx) = doc_ctx else {
                return name.to_string();
            };
            // Check use-map (handles both unqualified and qualified names).
            if let Some(pos) = name.find('\\') {
                let first = &name[..pos];
                let rest = &name[pos..];
                if let Some(fqn) = ctx.use_map.get(first) {
                    return format!("{fqn}{rest}");
                }
            } else if let Some(fqn) = ctx.use_map.get(name) {
                return fqn.clone();
            }
            // Prepend current namespace if available.
            if let Some(ref ns) = ctx.namespace {
                format!("{ns}\\{name}")
            } else {
                name.to_string()
            }
        }

        let mut methods = Vec::new();
        let mut properties = Vec::new();
        let mut constants = Vec::new();
        let mut used_traits: Vec<Atom> = Vec::new();
        let mut trait_precedences = Vec::new();
        let mut trait_aliases = Vec::new();
        let mut inline_use_generics: Vec<(Atom, Vec<PhpType>)> = Vec::new();
        let mut constructor_body: Option<&MethodBody<'_>> = None;

        for member in members {
            match member {
                ClassLikeMember::Method(method) => {
                    // Skip methods whose #[PhpStormStubsElementAvailable]
                    // range excludes the target PHP version.
                    if let Some(ctx) = doc_ctx
                        && let Some(ver) = ctx.php_version
                        && !is_available_for_version(&method.attribute_lists, ctx, ver)
                    {
                        continue;
                    }

                    // Skip methods whose docblock has `@removed X.Y`
                    // where X.Y <= the target PHP version.
                    if let Some(ctx) = doc_ctx
                        && let Some(ver) = ctx.php_version
                        && is_removed_for_version(method, ctx, ver)
                    {
                        continue;
                    }

                    let name = atom_bytes(method.name.value);
                    let name_offset = method.name.span.start.offset;
                    let php_version = doc_ctx.and_then(|ctx| ctx.php_version);
                    let mut parameters = extract_parameters(
                        &method.parameter_list,
                        doc_ctx.map(|ctx| ctx.content),
                        php_version,
                        doc_ctx,
                    );
                    let raw_native_return_type = method
                        .return_type_hint
                        .as_ref()
                        .map(|rth| extract_hint_type(&rth.hint));

                    // Check for a #[LanguageLevelTypeAware] override on the
                    // method's return type.  When present, it replaces the
                    // native type hint with the version-appropriate string.
                    let lang_level_return = if let Some(ctx) = doc_ctx
                        && let Some(ver) = ctx.php_version
                    {
                        super::extract_language_level_type(&method.attribute_lists, ctx, ver)
                    } else {
                        None
                    };
                    // The `#[LanguageLevelTypeAware]` attribute is JetBrains'
                    // authoritative, version-resolved return type, so the
                    // accompanying legacy `@return` docblock must not widen it.
                    let return_from_lang_level = lang_level_return.is_some();
                    let native_return_type = lang_level_return.or(raw_native_return_type);
                    let is_static = method.modifiers.iter().any(|m| m.is_static());
                    let visibility = extract_visibility(method.modifiers.iter());

                    // Parse the method's docblock once and reuse the
                    // structured `DocblockInfo` across all extraction
                    // helpers below instead of re-parsing the raw text
                    // for every tag kind.
                    let method_docblock_text = doc_ctx.and_then(|ctx| {
                        docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, method)
                    });
                    let method_docblock_info =
                        method_docblock_text.and_then(docblock::parse_docblock_for_tags);

                    // Look up the PHPDoc `@return` tag (if any) and apply
                    // type override logic.  Also extract PHPStan conditional
                    // return types if present.  Also check for `@deprecated`.
                    // Additionally extract method-level `@template` params
                    // and their `@param` bindings for general template
                    // substitution at call sites.
                    let (
                        return_type,
                        conditional_return,
                        deprecation_message,
                        method_deprecated_replacement,
                        method_template_params,
                        method_template_param_bounds,
                        method_template_bindings,
                    ) = if let Some(ref info) = method_docblock_info {
                        let parsed_doc_type = docblock::extract_return_type_from_info(info);
                        let effective = if return_from_lang_level {
                            native_return_type.clone()
                        } else {
                            docblock::resolve_effective_type_typed(
                                native_return_type.as_ref(),
                                parsed_doc_type.as_ref(),
                            )
                        };

                        // Apply #[ArrayShape] override if present.
                        let effective = if let Some(ctx) = doc_ctx {
                            effective.map(|ty| {
                                super::apply_array_shape_override(ty, &method.attribute_lists, ctx)
                            })
                        } else {
                            effective
                        };

                        let conditional = docblock::extract_conditional_return_type_from_info(info);

                        // Extract method-level @template params, their bounds,
                        // and @param bindings for generic type substitution.
                        let tpl_params_with_bounds =
                            docblock::extract_template_params_with_bounds_from_info(info);
                        let tpl_params: Vec<Atom> = tpl_params_with_bounds
                            .iter()
                            .map(|(n, _)| atom(n))
                            .collect();
                        let tpl_param_bounds: AtomMap<PhpType> = tpl_params_with_bounds
                            .into_iter()
                            .filter_map(|(n, b)| b.map(|b| (atom(&n), b)))
                            .collect();
                        let tpl_bindings: Vec<(Atom, Atom)> = if !tpl_params.is_empty() {
                            let tpl_strs: Vec<String> =
                                tpl_params.iter().map(|a| a.to_string()).collect();
                            docblock::extract_template_param_bindings_from_info(info, &tpl_strs)
                                .into_iter()
                                .map(|(a, b)| (atom(&a), atom(&b)))
                                .collect()
                        } else {
                            vec![]
                        };

                        // For constructors, also check for bindings between
                        // the *class-level* template params and the
                        // constructor's `@param` annotations.  This handles
                        // the common pattern:
                        //   /** @template T */
                        //   class Foo {
                        //       /** @param T $bar */
                        //       public function __construct($bar) {}
                        //   }
                        // where `T` is declared on the class but bound via
                        // the constructor's `@param T $bar`.
                        let (tpl_params, tpl_bindings): (Vec<Atom>, Vec<(Atom, Atom)>) = if name
                            == "__construct"
                            && tpl_bindings.is_empty()
                            && !class_template_params.is_empty()
                        {
                            let class_tpl_str: Vec<String> = class_template_params
                                .iter()
                                .map(|a| a.to_string())
                                .collect();
                            let class_bindings =
                                docblock::extract_template_param_bindings_from_info(
                                    info,
                                    &class_tpl_str,
                                );
                            if !class_bindings.is_empty() {
                                (
                                    class_template_params.to_vec(),
                                    class_bindings
                                        .into_iter()
                                        .map(|(a, b)| (atom(&a), atom(&b)))
                                        .collect(),
                                )
                            } else {
                                (tpl_params, tpl_bindings)
                            }
                        } else {
                            (tpl_params, tpl_bindings)
                        };

                        // If no explicit conditional return type was found,
                        // try to synthesize one from method-level @template
                        // annotations.  For example:
                        //   @template T
                        //   @param class-string<T> $class
                        //   @return T
                        // becomes a conditional that resolves T from the
                        // call-site argument (e.g. find(User::class) → User).
                        let conditional = conditional.or_else(|| {
                            let tpl_param_strings: Vec<String> =
                                tpl_params.iter().map(|a| a.to_string()).collect();
                            docblock::synthesize_template_conditional_from_info(
                                info,
                                &tpl_param_strings,
                                effective.as_ref(),
                                false,
                            )
                        });

                        let depr_info = merge_deprecation_info(
                            docblock::extract_deprecation_message_from_info(info),
                            &method.attribute_lists,
                            doc_ctx,
                        );
                        let deprecation_message = depr_info.message;

                        (
                            effective,
                            conditional,
                            deprecation_message,
                            depr_info.replacement,
                            tpl_params,
                            tpl_param_bounds,
                            tpl_bindings,
                        )
                    } else {
                        // No docblock, but we still need to check for
                        // #[Deprecated] attribute on the method itself.
                        let depr_info =
                            merge_deprecation_info(None, &method.attribute_lists, doc_ctx);

                        // Apply #[ArrayShape] override even without a docblock.
                        let effective_ret = if let Some(ctx) = doc_ctx {
                            native_return_type.clone().map(|ty| {
                                super::apply_array_shape_override(ty, &method.attribute_lists, ctx)
                            })
                        } else {
                            native_return_type.clone()
                        };

                        (
                            effective_ret,
                            None,
                            depr_info.message,
                            depr_info.replacement,
                            Vec::<Atom>::new(),
                            AtomMap::<PhpType>::default(),
                            Vec::<(Atom, Atom)>::new(),
                        )
                    };

                    // Extract promoted properties from constructor parameters.
                    // A promoted property is a constructor parameter with a
                    // visibility modifier (e.g. `public`, `private`, `protected`).
                    //
                    // When the constructor has a docblock, `@param` annotations
                    // can provide a more specific type than the native hint
                    // (e.g. `@param list<User> $users` vs native `array $users`).
                    // We apply `resolve_effective_type()` to pick the winner.
                    if name == "__construct" {
                        constructor_body = Some(&method.body);
                        for param in method.parameter_list.parameters.iter() {
                            if param.is_promoted_property() {
                                let raw_name = bytes_to_str(param.variable.name).to_string();
                                let prop_name =
                                    atom(raw_name.strip_prefix('$').unwrap_or(&raw_name));
                                let saved_native_hint =
                                    param.hint.as_ref().map(|h| extract_hint_type(h));
                                let prop_visibility = extract_visibility(param.modifiers.iter());

                                // Check for a docblock type override.
                                //
                                // 1. Inline `@var` on the parameter itself
                                //    (e.g. `/** @var array<User> */ public array $users`).
                                // 2. `@param` on the constructor docblock
                                //    (e.g. `@param list<User> $users`).
                                let inline_var_type = doc_ctx.and_then(|ctx| {
                                    let doc = docblock::get_docblock_text_for_node(
                                        ctx.trivias,
                                        ctx.content,
                                        param,
                                    )?;
                                    docblock::extract_var_type(doc)
                                });

                                let type_hint = if let Some(ref var_type) = inline_var_type {
                                    docblock::resolve_effective_type_typed(
                                        saved_native_hint.as_ref(),
                                        Some(var_type),
                                    )
                                } else if let Some(ref info) = method_docblock_info {
                                    let parsed =
                                        docblock::extract_param_raw_type_from_info(info, &raw_name);
                                    docblock::resolve_effective_type_typed(
                                        saved_native_hint.as_ref(),
                                        parsed.as_ref(),
                                    )
                                } else {
                                    saved_native_hint.clone()
                                };

                                // When no type hint is available, infer from `new ClassName()`
                                // default values (PHP 8.1+: `private $repo = new Repo()`).
                                // Resolve to FQN eagerly so downstream code does not
                                // need short-name resolution logic.
                                let type_hint = type_hint.or_else(|| {
                                    let dv = param.default_value.as_ref()?;
                                    if let Expression::Instantiation(inst) = dv.value
                                        && let Expression::Identifier(ident) = inst.class
                                    {
                                        let raw = bytes_to_str(ident.value()).to_string();
                                        let fqn = resolve_name_via_ctx(&raw, doc_ctx);
                                        return Some(PhpType::Named(fqn));
                                    }
                                    None
                                });

                                let prop_name_offset = param.variable.span.start.offset;
                                properties.push(PropertyInfo {
                                    name: prop_name,
                                    name_offset: prop_name_offset,
                                    native_type_hint: saved_native_hint,
                                    type_hint,
                                    description: None,
                                    is_static: false,
                                    visibility: prop_visibility,
                                    deprecation_message: None,
                                    deprecated_replacement: None,
                                    see_refs: Vec::new(),
                                    is_virtual: false,
                                });
                            }
                        }
                    }

                    // When no return type was resolved from docblocks or
                    // native type hints, try to infer an Eloquent
                    // relationship type from the method body text.
                    // For example, `$this->hasMany(Post::class)` produces
                    // a return type of `HasMany<Post>`.
                    let return_type = if return_type.is_none() {
                        infer_relationship_from_method(method, doc_ctx)
                    } else {
                        return_type
                    };

                    // Merge `@param` docblock types into parameter type
                    // hints so that callable signatures like
                    // `callable(User): void` are preserved.  This mirrors
                    // the promoted-property logic already used for
                    // constructor parameters.
                    if let Some(ref info) = method_docblock_info {
                        for param in &mut parameters {
                            let param_doc_type =
                                docblock::extract_param_raw_type_from_info(info, &param.name);
                            if let Some(ref doc_type) = param_doc_type {
                                let effective = docblock::resolve_effective_type_typed(
                                    param.type_hint.as_ref(),
                                    Some(doc_type),
                                );
                                if effective.is_some() {
                                    param.type_hint = effective;
                                }
                            }
                        }

                        // Populate `closure_this_type` from
                        // `@param-closure-this` tags so that `$this`
                        // inside a closure argument resolves to the
                        // declared type instead of the lexical class.
                        for (this_type, param_name) in
                            docblock::extract_param_closure_this_from_info(info)
                        {
                            if let Some(param) =
                                parameters.iter_mut().find(|p| p.name == param_name)
                            {
                                param.closure_this_type = Some(this_type);
                            }
                        }

                        // Append extra `@param` tags that don't match any
                        // native parameter.  These document parameters
                        // accessed via `func_get_args()` or similar
                        // mechanisms and should appear in hover/signature.
                        for (tag_name, tag_type) in docblock::extract_all_param_tags_from_info(info)
                        {
                            if !parameters.iter().any(|p| p.name == tag_name) {
                                let description =
                                    docblock::extract_param_description_from_info(info, &tag_name);
                                parameters.push(ParameterInfo {
                                    name: atom(&tag_name),
                                    is_required: false,
                                    type_hint: Some(tag_type),
                                    native_type_hint: None,
                                    description,
                                    default_value: None,
                                    is_variadic: false,
                                    is_reference: false,
                                    closure_this_type: None,
                                });
                            }
                        }
                    }

                    // A docblock `@param` merge above may have overwritten
                    // `type_hint` with a non-nullable docblock type. Re-fold
                    // null for parameters whose default value is `null`.
                    for param in &mut parameters {
                        param.apply_null_default();
                    }

                    let has_scope_attr = has_scope_attribute(method);

                    // Extract description, return description, link, and
                    // per-parameter descriptions from the method's docblock.
                    let method_description = method_docblock_info
                        .as_ref()
                        .and_then(crate::hover::extract_description_from_info);

                    let return_description = method_docblock_info
                        .as_ref()
                        .and_then(docblock::extract_return_description_from_info);

                    let links = method_docblock_info
                        .as_ref()
                        .map(docblock::extract_link_urls_from_info)
                        .unwrap_or_default();

                    let see_refs = method_docblock_info
                        .as_ref()
                        .map(docblock::extract_see_references_from_info)
                        .unwrap_or_default();

                    // Populate per-parameter descriptions from `@param` tags.
                    if let Some(ref info) = method_docblock_info {
                        for param in &mut parameters {
                            param.description =
                                docblock::extract_param_description_from_info(info, &param.name);
                        }
                    }

                    // Extract `@phpstan-assert` / `@psalm-assert` type
                    // assertion tags from the method's docblock so that
                    // the narrowing engine can apply type guards from
                    // static method calls like `Assert::instanceOf($v, Foo::class)`.
                    let type_assertions = method_docblock_info
                        .as_ref()
                        .map(docblock::extract_type_assertions_from_info)
                        .unwrap_or_default();

                    // Extract `@throws` tags so that cross-file throws
                    // propagation can look up which exceptions a method
                    // declares without needing access to the source text.
                    let throws = method_docblock_info
                        .as_ref()
                        .map(docblock::extract_throws_tags_from_info)
                        .unwrap_or_default();

                    methods.push(MethodInfo {
                        name,
                        name_offset,
                        parameters,
                        native_return_type: native_return_type.clone(),
                        return_type,
                        description: method_description,
                        return_description,
                        links,
                        see_refs,
                        is_static,
                        visibility,
                        conditional_return,
                        deprecation_message,
                        deprecated_replacement: method_deprecated_replacement,
                        template_params: method_template_params,
                        template_param_bounds: method_template_param_bounds,
                        template_bindings: method_template_bindings,
                        has_scope_attribute: has_scope_attr,
                        is_abstract: method.is_abstract(),
                        is_virtual: false,
                        is_macro: false,
                        is_inferred_return: false,
                        type_assertions,
                        throws,
                        if_this_is: method_docblock_text
                            .and_then(crate::docblock::extract_if_this_is_type),
                    });
                }
                ClassLikeMember::Property(property) => {
                    let mut prop_infos = extract_property_info(property);

                    // Extract the attribute lists from the property variant
                    // so we can check for #[Deprecated] below.
                    let prop_attr_lists: Option<&Sequence<'_, AttributeList<'_>>> = match property {
                        Property::Plain(p) => Some(&p.attribute_lists),
                        Property::Hooked(h) => Some(&h.attribute_lists),
                    };

                    // Apply #[LanguageLevelTypeAware] override to property types.
                    // When present, the attribute's version-appropriate type
                    // string replaces the native type hint.
                    if let Some(ctx) = doc_ctx
                        && let Some(ver) = ctx.php_version
                        && let Some(attr_lists) = prop_attr_lists
                        && let Some(override_type) =
                            super::extract_language_level_type(attr_lists, ctx, ver)
                    {
                        for prop in &mut prop_infos {
                            prop.type_hint = Some(override_type.clone());
                            prop.native_type_hint = Some(override_type.clone());
                        }
                    }

                    // Apply PHPDoc `@var` override, `@deprecated`, `@see`, and
                    // description for each property.
                    if let Some(ctx) = doc_ctx
                        && let Some(doc_text) =
                            docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, member)
                    {
                        let info = docblock::parse_docblock_for_tags(doc_text);
                        let docblock_msg = info
                            .as_ref()
                            .and_then(docblock::extract_deprecation_message_from_info);
                        let see_refs = info
                            .as_ref()
                            .map(docblock::extract_see_references_from_info)
                            .unwrap_or_default();
                        if !see_refs.is_empty() {
                            for prop in &mut prop_infos {
                                prop.see_refs = see_refs.clone();
                            }
                        }
                        // Use merge_deprecation_info for version-aware suppression
                        // and replacement extraction.  Re-use the attribute lists
                        // from the property variant.
                        let depr_info = if let Some(attr_lists) = prop_attr_lists {
                            merge_deprecation_info(docblock_msg, attr_lists, doc_ctx)
                        } else {
                            DeprecationInfo {
                                message: docblock_msg,
                                replacement: None,
                            }
                        };
                        if let Some(ref msg) = depr_info.message {
                            for prop in &mut prop_infos {
                                prop.deprecation_message = Some(msg.clone());
                            }
                        }
                        if let Some(ref repl) = depr_info.replacement {
                            for prop in &mut prop_infos {
                                prop.deprecated_replacement = Some(repl.clone());
                            }
                        }
                        if let Some(parsed_doc) =
                            info.as_ref().and_then(docblock::extract_var_type_from_info)
                        {
                            for prop in &mut prop_infos {
                                let effective = docblock::resolve_effective_type_typed(
                                    prop.type_hint.as_ref(),
                                    Some(&parsed_doc),
                                );
                                prop.type_hint = effective;
                            }
                        }
                        let description = info
                            .as_ref()
                            .and_then(crate::hover::extract_description_from_info)
                            .or_else(|| {
                                info.as_ref()
                                    .and_then(crate::hover::extract_var_description_from_info)
                            });
                        if description.is_some() {
                            for prop in &mut prop_infos {
                                prop.description = description.clone();
                            }
                        }
                    }

                    // If no deprecation was found from the docblock, check the
                    // #[Deprecated] attribute.  This covers properties that have
                    // an attribute but no docblock at all.
                    if prop_infos.iter().all(|p| p.deprecation_message.is_none())
                        && let Some(ctx) = doc_ctx
                        && let Some(attr_lists) = prop_attr_lists
                    {
                        let depr_info = merge_deprecation_info(None, attr_lists, Some(ctx));
                        if let Some(ref msg) = depr_info.message {
                            for prop in &mut prop_infos {
                                prop.deprecation_message = Some(msg.clone());
                            }
                        }
                        if let Some(ref repl) = depr_info.replacement {
                            for prop in &mut prop_infos {
                                prop.deprecated_replacement = Some(repl.clone());
                            }
                        }
                    }

                    properties.append(&mut prop_infos);
                }
                ClassLikeMember::Constant(constant) => {
                    let type_hint = constant.hint.as_ref().map(|h| extract_hint_type(h));
                    let visibility = extract_visibility(constant.modifiers.iter());
                    let const_docblock_text = doc_ctx.and_then(|ctx| {
                        docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, member)
                    });
                    let const_docblock_info =
                        const_docblock_text.and_then(docblock::parse_docblock_for_tags);
                    let depr_info = {
                        let docblock_msg = const_docblock_info
                            .as_ref()
                            .and_then(docblock::extract_deprecation_message_from_info);
                        merge_deprecation_info(docblock_msg, &constant.attribute_lists, doc_ctx)
                    };
                    let deprecation_message = depr_info.message;
                    let constant_deprecated_replacement = depr_info.replacement;
                    let const_see_refs = const_docblock_info
                        .as_ref()
                        .map(docblock::extract_see_references_from_info)
                        .unwrap_or_default();
                    let const_description = const_docblock_info
                        .as_ref()
                        .and_then(crate::hover::extract_description_from_info);
                    for item in constant.items.iter() {
                        let value = doc_ctx.and_then(|ctx| {
                            let start = item.value.span().start.offset as usize;
                            let end = item.value.span().end.offset as usize;
                            ctx.content.get(start..end).map(|s| s.to_string())
                        });
                        constants.push(ConstantInfo {
                            name: atom_bytes(item.name.value),
                            name_offset: item.name.span.start.offset,
                            type_hint: type_hint.clone(),
                            visibility,
                            deprecation_message: deprecation_message.clone(),
                            deprecated_replacement: constant_deprecated_replacement.clone(),
                            see_refs: const_see_refs.clone(),
                            description: const_description.clone(),
                            is_enum_case: false,
                            enum_value: None,
                            value,
                            is_virtual: false,
                        });
                    }
                }
                ClassLikeMember::EnumCase(enum_case) => {
                    let case_name = atom_bytes(enum_case.item.name().value);
                    let case_name_offset = enum_case.item.name().span.start.offset;
                    let enum_value = if let EnumCaseItem::Backed(backed) = &enum_case.item {
                        let start = backed.value.span().start.offset as usize;
                        let end = backed.value.span().end.offset as usize;
                        doc_ctx
                            .and_then(|ctx| ctx.content.get(start..end))
                            .map(|s| s.to_string())
                    } else {
                        None
                    };
                    constants.push(ConstantInfo {
                        name: case_name,
                        name_offset: case_name_offset,
                        type_hint: None,
                        visibility: Visibility::Public,
                        deprecation_message: None,
                        deprecated_replacement: None,
                        see_refs: Vec::new(),
                        description: None,
                        is_enum_case: true,
                        enum_value,
                        value: None,
                        is_virtual: false,
                    });
                }
                ClassLikeMember::TraitUse(trait_use) => {
                    for trait_name_ident in trait_use.trait_names.iter() {
                        used_traits.push(atom_bytes(trait_name_ident.value()));
                    }

                    // Extract `@use` generics from the docblock on the
                    // trait `use` statement itself.  In Laravel, the
                    // Eloquent Builder declares:
                    //
                    //   /** @use BuildsQueries<TModel> */
                    //   use BuildsQueries;
                    //
                    // This binds the trait's template parameter to the
                    // class's own template parameter.
                    if let Some(ctx) = doc_ctx
                        && let Some(doc_text) = docblock::get_docblock_text_for_node(
                            ctx.trivias,
                            ctx.content,
                            trait_use,
                        )
                    {
                        let tags = docblock::extract_generics_tag(doc_text, "@use");
                        inline_use_generics
                            .extend(tags.into_iter().map(|(n, args)| (atom(&n), args)));
                    }

                    // Parse trait adaptation block (`{ ... }`) if present.
                    // This handles `insteadof` (precedence) and `as` (alias)
                    // declarations for resolving trait method conflicts.
                    if let TraitUseSpecification::Concrete(spec) = &trait_use.specification {
                        for adaptation in spec.adaptations.iter() {
                            match adaptation {
                                TraitUseAdaptation::Precedence(prec) => {
                                    let trait_name =
                                        atom_bytes(prec.method_reference.trait_name.value());
                                    let method_name =
                                        atom_bytes(prec.method_reference.method_name.value);
                                    let insteadof: Vec<Atom> = prec
                                        .trait_names
                                        .iter()
                                        .map(|id| atom_bytes(id.value()))
                                        .collect();
                                    trait_precedences.push(TraitPrecedence {
                                        trait_name,
                                        method_name,
                                        insteadof,
                                    });
                                }
                                TraitUseAdaptation::Alias(alias_adapt) => {
                                    let (trait_name, method_name) =
                                        match &alias_adapt.method_reference {
                                            TraitUseMethodReference::Identifier(ident) => {
                                                (None, atom_bytes(ident.value))
                                            }
                                            TraitUseMethodReference::Absolute(abs) => (
                                                Some(atom_bytes(abs.trait_name.value())),
                                                atom_bytes(abs.method_name.value),
                                            ),
                                        };
                                    let alias =
                                        alias_adapt.alias.as_ref().map(|a| atom_bytes(a.value));
                                    let visibility = alias_adapt.modifier.as_ref().map(|m| {
                                        if m.is_private() {
                                            Visibility::Private
                                        } else if m.is_protected() {
                                            Visibility::Protected
                                        } else {
                                            Visibility::Public
                                        }
                                    });
                                    trait_aliases.push(TraitAlias {
                                        trait_name,
                                        method_name,
                                        alias,
                                        visibility,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // Infer types for untyped properties from constructor assignments.
        // Scans the constructor body for `$this->prop = new ClassName()`
        // and fills in the type_hint when not set by declaration or docblock.
        // Only applies when neither native type hint nor docblock type is present.
        // Resolves to FQN eagerly so downstream code does not need
        // short-name resolution logic.
        if let Some(MethodBody::Concrete(concrete)) = constructor_body {
            for stmt in concrete.statements.iter() {
                if let Statement::Expression(expr_stmt) = stmt
                    && let Expression::Assignment(assign) = expr_stmt.expression
                    && let Expression::Access(Access::Property(pa)) = assign.lhs
                    && let Expression::Variable(Variable::Direct(dv)) = pa.object
                    && dv.name == b"$this"
                    && let ClassLikeMemberSelector::Identifier(ident) = &pa.property
                    && let Expression::Instantiation(inst) = assign.rhs
                    && let Expression::Identifier(class_ident) = inst.class
                {
                    let prop_name = bytes_to_str(ident.value).to_string();
                    if let Some(prop) = properties.iter_mut().find(|p| {
                        p.name == prop_name && p.type_hint.is_none() && p.native_type_hint.is_none()
                    }) {
                        let raw = bytes_to_str(class_ident.value()).to_string();
                        let fqn = resolve_name_via_ctx(&raw, doc_ctx);
                        prop.type_hint = Some(PhpType::Named(fqn));
                    }
                }
            }
        }

        ExtractedMembers {
            methods,
            properties,
            constants,
            used_traits,
            trait_precedences,
            trait_aliases,
            inline_use_generics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::attribute_target;

    #[test]
    fn parse_target_class_qualified() {
        assert_eq!(
            parse_attribute_target_flags("\\Attribute::TARGET_CLASS"),
            attribute_target::TARGET_CLASS,
        );
    }

    #[test]
    fn parse_target_class_unqualified() {
        assert_eq!(
            parse_attribute_target_flags("Attribute::TARGET_CLASS"),
            attribute_target::TARGET_CLASS,
        );
    }

    #[test]
    fn parse_target_method_self() {
        assert_eq!(
            parse_attribute_target_flags("self::TARGET_METHOD"),
            attribute_target::TARGET_METHOD,
        );
    }

    #[test]
    fn parse_target_bare_constant() {
        assert_eq!(
            parse_attribute_target_flags("TARGET_PROPERTY"),
            attribute_target::TARGET_PROPERTY,
        );
    }

    #[test]
    fn parse_target_numeric_literal() {
        assert_eq!(parse_attribute_target_flags("1"), 1);
        assert_eq!(parse_attribute_target_flags("63"), 63);
    }

    #[test]
    fn parse_target_bitwise_or() {
        let expected = attribute_target::TARGET_CLASS | attribute_target::TARGET_METHOD;
        assert_eq!(
            parse_attribute_target_flags("\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD"),
            expected,
        );
    }

    #[test]
    fn parse_target_all() {
        assert_eq!(
            parse_attribute_target_flags("Attribute::TARGET_ALL"),
            attribute_target::TARGET_ALL,
        );
    }

    #[test]
    fn parse_target_unrecognised_defaults_to_all() {
        // Completely unrecognisable text falls back to TARGET_ALL
        // because the class IS marked with #[Attribute(...)].
        assert_eq!(
            parse_attribute_target_flags("SOME_UNKNOWN_CONST"),
            attribute_target::TARGET_ALL,
        );
    }

    /// A class declared inside an `if` block (e.g. a version guard, like
    /// Doctrine's `ServiceEntityRepository`) must be indexed with its native
    /// parent and its `@extends Parent<Concrete>` generics — not merely
    /// discovered by name.
    #[test]
    fn conditional_class_inside_if_is_extracted_with_parent_and_generics() {
        let src = r#"<?php
/** @template T of object */
class Repo {}
class Entity {}
if (\PHP_VERSION_ID >= 80000) {
    /** @extends Repo<Entity> */
    class ConditionalRepo extends Repo {}
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        let conditional = classes
            .iter()
            .find(|(c, _)| c.name == atom("ConditionalRepo"))
            .map(|(c, _)| c)
            .expect("class declared inside `if` should be indexed");

        assert_eq!(
            conditional.parent_class,
            Some(atom("Repo")),
            "conditional class should carry its native parent",
        );
        assert!(
            conditional
                .extends_generics
                .iter()
                .any(|(parent, args)| *parent == atom("Repo") && !args.is_empty()),
            "conditional class should carry its `@extends Repo<Entity>` generics, got {:?}",
            conditional.extends_generics,
        );
    }

    /// When the same class name is declared in both branches of a conditional
    /// (the Doctrine ORM2-vs-ORM3 shape), the first declaration in source
    /// order wins and exactly one `ClassInfo` is produced.
    #[test]
    fn conditional_class_in_both_branches_keeps_first() {
        let src = r#"<?php
if (\defined('SOME_FLAG')) {
    class Dup extends First {}
} else {
    class Dup extends Second {}
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        let dups: Vec<_> = classes
            .iter()
            .filter(|(c, _)| c.name == atom("Dup"))
            .collect();

        assert_eq!(
            dups.len(),
            1,
            "duplicate-branch class must be de-duplicated"
        );
        assert_eq!(
            dups[0].0.parent_class,
            Some(atom("First")),
            "first (source-order) branch should win",
        );
    }

    #[test]
    fn parse_target_mixed_qualified_and_bare() {
        let expected = attribute_target::TARGET_FUNCTION | attribute_target::TARGET_PARAMETER;
        assert_eq!(
            parse_attribute_target_flags("Attribute::TARGET_FUNCTION | TARGET_PARAMETER"),
            expected,
        );
    }

    #[test]
    fn parse_target_whitespace_handling() {
        assert_eq!(
            parse_attribute_target_flags("  Attribute::TARGET_CLASS  "),
            attribute_target::TARGET_CLASS,
        );
    }
}
