//! Parse-time extraction of Eloquent model metadata from the AST.
//!
//! This module builds the [`LaravelMetadata`] attached to every parsed
//! class: factory `$model`, `$casts`/`casts()`, `$attributes`, `$dates`,
//! `$fillable`/`$guarded`/`$hidden`/`$visible`/`$appends`, `$timestamps` and the
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

use crate::atom::{Atom, atom, bytes_to_str, last_segment, literal_bytes_to_str};
use crate::parser::DocblockCtx;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{
    CastSources, FacadeAccessor, LaravelMetadata, MethodInfo, PivotAccessor, PivotRelation,
    model_declaration,
};
use crate::util::{short_name, strip_fqn_prefix};

use super::relationships::is_inferable_relationship_short_name;
use super::{
    extract_pivot_accessor, extract_pivot_using, extract_with_pivot_columns,
    infer_relationship_from_body, is_soft_deletes_trait,
};

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
/// This enables relationship property synthesis on models whose
/// relationship methods carry no generic `@return` annotation.
fn infer_relationship_from_method<'a>(
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

/// A method's return type, filled in from its body when that says more.
///
/// With no declared type, the body's `$this->hasMany(Post::class)` is the
/// only source.  A bare relationship class as the declared type
/// (`: HasMany`, the usual way to write a relationship without a
/// docblock) names the relationship but not the related model, which the
/// same body call supplies.
pub(crate) fn relationship_return_type<'a>(
    declared: Option<PhpType>,
    method: &class_like::method::Method<'a>,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> Option<PhpType> {
    let Some(declared) = declared else {
        return infer_relationship_from_method(method, doc_ctx);
    };
    let TypeKind::Named(name) = declared.kind() else {
        return Some(declared);
    };
    let declared_short = short_name(name);
    if !is_inferable_relationship_short_name(declared_short) {
        return Some(declared);
    }
    match infer_relationship_from_method(method, doc_ctx) {
        Some(inferred) if matches!(inferred.kind(), TypeKind::Generic(g) if short_name(&g.name) == declared_short) => {
            Some(inferred)
        }
        _ => Some(declared),
    }
}

/// Extract the policy class name from a `#[UsePolicy(X::class)]` attribute.
fn extract_use_policy_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
) -> Option<String> {
    extract_class_constant_attribute(attribute_lists, content, b"UsePolicy")
}

/// The `X` of the first `#[Name(X::class)]` attribute whose short name is
/// `attr_name`.
fn extract_class_constant_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
    attr_name: &[u8],
) -> Option<String> {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            let short = last_segment(attr.name.value());
            if short != attr_name {
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
/// 1. A `newEloquentBuilder()` override with a concrete return type.
/// 2. `#[UseEloquentBuilder(CustomBuilder::class)]` on the class.
/// 3. `/** @use HasBuilder<CustomBuilder> */` in `use_generics`.
fn extract_custom_builder(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    use_generics: &[(Atom, Vec<PhpType>)],
    methods: &[MethodInfo],
    content: &str,
) -> Option<PhpType> {
    extract_customisation(
        attribute_lists,
        use_generics,
        methods,
        content,
        b"UseEloquentBuilder",
        &["HasBuilder", "CustomizeQueryBuilder"],
        "newEloquentBuilder",
        &["Illuminate\\Database\\Eloquent\\Builder", "Builder"],
    )
}

/// Determine the custom collection class for an Eloquent model.
///
/// Checks the same three sources [`extract_custom_builder`] does, in the
/// same order: `newCollection()`, `#[CollectedBy(CustomCollection::class)]`,
/// and `/** @use HasCollection<CustomCollection> */`.
fn extract_custom_collection(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    use_generics: &[(Atom, Vec<PhpType>)],
    methods: &[MethodInfo],
    content: &str,
) -> Option<PhpType> {
    extract_customisation(
        attribute_lists,
        use_generics,
        methods,
        content,
        b"CollectedBy",
        &["HasCollection"],
        "newCollection",
        &["Illuminate\\Database\\Eloquent\\Collection", "Collection"],
    )
}

/// The class a model substitutes for one of Eloquent's defaults, in the
/// three ways a model can name it.
///
/// `attr_name` is the `#[…(X::class)]` attribute, `traits` the generic
/// traits whose first argument names it, `factory_method` the `new…()`
/// override that returns it, and `defaults` the base classes that method
/// returns when nothing is customised.
#[allow(clippy::too_many_arguments)]
fn extract_customisation(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    use_generics: &[(Atom, Vec<PhpType>)],
    methods: &[MethodInfo],
    content: &str,
    attr_name: &[u8],
    traits: &[&str],
    factory_method: &str,
    defaults: &[&str],
) -> Option<PhpType> {
    if let Some(method) = methods.iter().find(|m| m.name == factory_method)
        && let Some(return_type) = method.return_type.as_ref()
        && let Some(base) = return_type.base_name()
        && !base.is_empty()
        && !defaults.contains(&base)
    {
        return Some(return_type.clone());
    }

    if let Some(name) = extract_class_constant_attribute(attribute_lists, content, attr_name) {
        return Some(PhpType::named(atom(&name)));
    }

    for (trait_name, args) in use_generics {
        let short = crate::util::short_name(trait_name);
        if traits.contains(&short) && !args.is_empty() {
            return Some(args[0].clone());
        }
    }

    None
}

/// Extract the model class explicitly configured on a Laravel factory.
///
/// A `SomeModel::class` initializer follows PHP class-name resolution, so
/// its written name is left for the normal name-resolution pass. A quoted
/// class name is already a runtime string and is therefore marked absolute
/// before that pass, preventing the factory's namespace from being prepended.
fn extract_factory_model<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
) -> Option<PhpType> {
    for member in members {
        let class_like::member::ClassLikeMember::Property(class_like::property::Property::Plain(
            plain,
        )) = member
        else {
            continue;
        };

        // `Factory::modelName()` reads `$this->model`, which a static
        // property of the same name never answers.
        if plain.modifiers.iter().any(|modifier| modifier.is_static()) {
            continue;
        }

        for item in plain.items.iter() {
            let var_name = bytes_to_str(item.variable().name);
            if var_name.strip_prefix('$').unwrap_or(var_name) != "model" {
                continue;
            }
            let class_like::property::PropertyItem::Concrete(concrete) = item else {
                continue;
            };

            if let Expression::Access(Access::ClassConstant(access)) = concrete.value
                && matches!(
                    &access.constant,
                    ClassLikeConstantSelector::Identifier(constant)
                        if bytes_to_str(constant.value).eq_ignore_ascii_case("class")
                )
                && let Expression::Identifier(identifier) = access.class
            {
                let name = bytes_to_str(identifier.value()).trim();
                return (!name.is_empty()
                    && !name.eq_ignore_ascii_case("self")
                    && !name.eq_ignore_ascii_case("static")
                    && !name.eq_ignore_ascii_case("parent"))
                .then(|| PhpType::named(atom(name)));
            }

            let Expression::Literal(Literal::String(literal)) = concrete.value else {
                continue;
            };
            let Some(name) = literal.value.and_then(literal_bytes_to_str) else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let absolute = if name.starts_with('\\') {
                name.to_string()
            } else {
                format!("\\{name}")
            };
            return Some(PhpType::named(atom(&absolute)));
        }
    }

    None
}

fn extract_class_property<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    property: &str,
) -> Option<PhpType> {
    for member in members {
        let class_like::member::ClassLikeMember::Property(class_like::property::Property::Plain(
            plain,
        )) = member
        else {
            continue;
        };
        if !plain.modifiers.iter().any(|modifier| modifier.is_static()) {
            continue;
        }
        for item in plain.items.iter() {
            if bytes_to_str(item.variable().name).trim_start_matches('$') != property {
                continue;
            }
            let class_like::property::PropertyItem::Concrete(concrete) = item else {
                continue;
            };
            if let Expression::Access(Access::ClassConstant(access)) = concrete.value
                && matches!(&access.constant, ClassLikeConstantSelector::Identifier(identifier)
                    if bytes_to_str(identifier.value).eq_ignore_ascii_case("class"))
                && let Expression::Identifier(identifier) = access.class
            {
                return Some(PhpType::named(atom(bytes_to_str(identifier.value()))));
            }
            if let Expression::Literal(Literal::String(literal)) = concrete.value
                && let Some(name) = literal.value.and_then(literal_bytes_to_str)
            {
                return Some(PhpType::named(atom(&format!(
                    "\\{}",
                    name.trim_start_matches('\\')
                ))));
            }
        }
    }
    None
}

fn extract_class_return_from_body<'a>(
    mut members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    method_name: &str,
) -> Option<PhpType> {
    let method = members.find_map(|member| match member {
        class_like::member::ClassLikeMember::Method(method)
            if bytes_to_str(method.name.value).eq_ignore_ascii_case(method_name) =>
        {
            Some(method)
        }
        _ => None,
    })?;
    let class_like::method::MethodBody::Concrete(block) = &method.body else {
        return None;
    };
    let value = block.statements.iter().find_map(|stmt| match stmt {
        Statement::Return(ret) => ret.value,
        _ => None,
    })?;
    let name = match value {
        Expression::Instantiation(inst) => match inst.class {
            Expression::Identifier(id) => Some(bytes_to_str(id.value())),
            _ => None,
        },
        Expression::Call(Call::StaticMethod(call)) => match (&call.class, &call.method) {
            (Expression::Identifier(id), ClassLikeMemberSelector::Identifier(method))
                if bytes_to_str(method.value).eq_ignore_ascii_case("new") =>
            {
                Some(bytes_to_str(id.value()))
            }
            _ => None,
        },
        _ => None,
    }?;
    Some(PhpType::named(atom(name)))
}

/// A `(column_name, cast_type)` pair.
type CastEntry = (String, String);

/// Extract Eloquent cast definitions from a class's members.
///
/// Scans the class members for:
/// 1. A `$casts` property with an array initializer (`protected $casts = [...]`)
/// 2. A `casts()` method whose body contains a `return [...]` statement
///
/// Returns the `(column_name, cast_type)` pairs of each source, `None`
/// for a source the class does not declare. [`overlay_casts`] merges
/// them the way Laravel does at runtime.
fn extract_cast_sources<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> (Option<Vec<CastEntry>>, Option<Vec<CastEntry>>) {
    let mut property_text: Option<&str> = None;
    let mut method_text: Option<&str> = None;
    let mut has_property = false;
    let mut has_method = false;

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
                    has_property = true;
                    if let class_like::property::PropertyItem::Concrete(concrete) = item {
                        let span = concrete.value.span();
                        let start = span.start.offset as usize;
                        let end = span.end.offset as usize;
                        property_text = content.get(start..end);
                    }
                }
            }
            class_like::member::ClassLikeMember::Method(method)
                if method.name.value.eq_ignore_ascii_case(b"casts") =>
            {
                has_method = true;
                if let class_like::method::MethodBody::Concrete(block) = &method.body
                    && let Some(value) = block.statements.iter().find_map(|stmt| match stmt {
                        Statement::Return(ret) => ret.value,
                        _ => None,
                    })
                {
                    let span = value.span();
                    method_text = content.get(span.start.offset as usize..span.end.offset as usize);
                }
            }
            _ => {}
        }
    }

    let property = has_property.then(|| property_text.map(parse_casts_array).unwrap_or_default());
    let method = has_method.then(|| method_text.map(parse_casts_array).unwrap_or_default());
    (property, method)
}

/// Merge a model's `$casts` property entries with its `casts()` method
/// entries.
///
/// Method entries override property entries for the same column,
/// matching Laravel's runtime behaviour where `Model::casts()` is merged
/// over `$casts`.
pub(crate) fn overlay_casts(
    property: &[(String, String)],
    method: &[(String, String)],
) -> Vec<(String, String)> {
    let mut merged = property.to_vec();
    for (key, value) in method {
        if let Some(existing) = merged.iter_mut().find(|(k, _)| k == key) {
            existing.1.clone_from(value);
        } else {
            merged.push((key.clone(), value.clone()));
        }
    }
    merged
}

/// The trimmed, non-empty entries of a PHP array literal written as `[…]`.
///
/// Splitting on commas is safe for the arrays this reads: a model's
/// `$casts`, `$fillable`, `$hidden`, and their siblings hold string
/// literals and `::class` constants, never a nested array or a call whose
/// own arguments would carry a comma. Text that is not an array literal
/// yields nothing.
fn array_literal_entries(text: &str) -> impl Iterator<Item = &str> {
    text.trim()
        .strip_prefix('[')
        .map(|s| s.strip_suffix(']').unwrap_or(s))
        .into_iter()
        .flat_map(|inner| inner.split(','))
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
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
    for segment in array_literal_entries(text) {
        let Some(arrow_pos) = segment.find("=>") else {
            continue;
        };
        let key = extract_string_literal(segment[..arrow_pos].trim());
        let value = extract_string_literal(segment[arrow_pos + 2..].trim());

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
/// Reads four sources:
///
/// - `$timestamps` property — `true` (default) or `false`.
/// - `CREATED_AT` constant — column name string or `null`.
/// - `UPDATED_AT` constant — column name string or `null`.
/// - `DELETED_AT` constant — the `SoftDeletes` column name string.
///
/// Each field uses the same `Option` semantics as the `LaravelMetadata`
/// field of that name: outer `None` means "not declared", `Some(None)`
/// means "explicitly `null`".
fn extract_timestamp_config<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> TimestampConfig {
    let mut timestamps: Option<bool> = None;
    let mut created_at: Option<Option<String>> = None;
    let mut updated_at: Option<Option<String>> = None;
    let mut deleted_at: Option<String> = None;

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
                    if name != "CREATED_AT" && name != "UPDATED_AT" && name != "DELETED_AT" {
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
                        match name.as_str() {
                            "CREATED_AT" => created_at = Some(val),
                            "UPDATED_AT" => updated_at = Some(val),
                            _ => deleted_at = val,
                        }
                    }
                }
            }
            _ => {}
        }
    }

    TimestampConfig {
        timestamps,
        created_at_name: created_at,
        updated_at_name: updated_at,
        deleted_at_name: deleted_at,
    }
}

/// The timestamp configuration [`extract_timestamp_config`] reads.
struct TimestampConfig {
    timestamps: Option<bool>,
    created_at_name: Option<Option<String>>,
    updated_at_name: Option<Option<String>>,
    deleted_at_name: Option<String>,
}

/// Extract column names from `$fillable`, `$guarded`, `$hidden`, `$visible`,
/// and `$appends` arrays.
///
/// These properties contain simple string lists of column names without
/// type information.  The `LaravelModelProvider` uses them as a
/// last-resort fallback, synthesizing `mixed`-typed virtual properties
/// for columns not already covered by `$casts` or `$attributes`.
///
/// All five arrays are merged; duplicates are removed (first occurrence
/// wins). Returns the names, the [`model_declaration`] list flags of each
/// name (index for index), and the flags of every list the class declares.
fn extract_column_names<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> (Vec<String>, Vec<u16>, u16) {
    let mut names: Vec<String> = Vec::new();
    let mut sources: Vec<u16> = Vec::new();
    let mut declared = 0;
    let targets = [
        ("fillable", model_declaration::FILLABLE),
        ("guarded", model_declaration::GUARDED),
        ("hidden", model_declaration::HIDDEN),
        ("visible", model_declaration::VISIBLE),
        ("appends", model_declaration::APPENDS),
    ];

    for member in members {
        if let class_like::member::ClassLikeMember::Property(
            class_like::property::Property::Plain(plain),
        ) = member
        {
            for item in plain.items.iter() {
                let var_name = bytes_to_str(item.variable().name);
                let stripped = var_name.strip_prefix('$').unwrap_or(var_name);
                let Some(&(_, flag)) = targets.iter().find(|(name, _)| *name == stripped) else {
                    continue;
                };
                declared |= flag;
                if let class_like::property::PropertyItem::Concrete(concrete) = item {
                    let span = concrete.value.span();
                    let start = span.start.offset as usize;
                    let end = span.end.offset as usize;
                    if let Some(text) = content.get(start..end) {
                        for name in parse_string_list(text) {
                            if let Some(pos) = names.iter().position(|n| *n == name) {
                                sources[pos] |= flag;
                            } else {
                                names.push(name);
                                sources.push(flag);
                            }
                        }
                    }
                }
            }
        }
    }

    (names, sources, declared)
}

/// Whether the class declares a plain property named `target` (without
/// the `$`), whatever its initializer.
fn declares_property<'a>(
    mut members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    target: &str,
) -> bool {
    members.any(|member| {
        let class_like::member::ClassLikeMember::Property(class_like::property::Property::Plain(
            plain,
        )) = member
        else {
            return false;
        };
        plain.items.iter().any(|item| {
            let name = bytes_to_str(item.variable().name);
            name.strip_prefix('$').unwrap_or(name) == target
        })
    })
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

/// Extract what a facade's `getFacadeAccessor()` returns.
///
/// Facades name the container binding they proxy either as a string
/// (`return 'view';`) or as a class reference (`return Factory::class;`).
/// The class name is taken as written and resolved to an FQN in the
/// name-resolution pass. Anything else (a computed value, a constant, a
/// `self::class`) is not statically knowable, so it yields `None`.
fn extract_facade_accessor<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Option<FacadeAccessor> {
    let method = members.into_iter().find_map(|member| match member {
        class_like::member::ClassLikeMember::Method(method)
            if bytes_to_str(method.name.value).eq_ignore_ascii_case("getFacadeAccessor") =>
        {
            Some(method)
        }
        _ => None,
    })?;
    let class_like::method::MethodBody::Concrete(block) = &method.body else {
        return None;
    };
    let value = block.statements.iter().find_map(|stmt| match stmt {
        Statement::Return(ret) => ret.value,
        _ => None,
    })?;

    if let Some((text, _, _)) = super::helpers::extract_string_literal(value, content) {
        return Some(FacadeAccessor::Alias(atom(text)));
    }
    let Expression::Access(Access::ClassConstant(access)) = value else {
        return None;
    };
    let ClassLikeConstantSelector::Identifier(constant) = &access.constant else {
        return None;
    };
    if !bytes_to_str(constant.value).eq_ignore_ascii_case("class") {
        return None;
    }
    // `self::class` / `static::class` name the facade itself, which is
    // never the class it forwards to.
    let Expression::Identifier(identifier) = access.class else {
        return None;
    };
    let name = bytes_to_str(identifier.value());
    (!name.is_empty()).then(|| FacadeAccessor::Class(atom(name)))
}

/// Extract the columns a model's own `uniqueIds()` override returns.
///
/// Only a `return [...]` of string literals is statically knowable. Any
/// other shape (`[$this->getKeyName(), 'uuid']`, a spread of
/// `parent::uniqueIds()`, a computed value) yields `None`, as does a
/// model that does not declare the method.
fn extract_unique_ids<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Option<Vec<String>> {
    let method = members.into_iter().find_map(|member| match member {
        class_like::member::ClassLikeMember::Method(method)
            if bytes_to_str(method.name.value).eq_ignore_ascii_case("uniqueIds") =>
        {
            Some(method)
        }
        _ => None,
    })?;
    let class_like::method::MethodBody::Concrete(block) = &method.body else {
        return None;
    };
    let value = block.statements.iter().find_map(|stmt| match stmt {
        Statement::Return(ret) => ret.value,
        _ => None,
    })?;
    let elements = match value {
        Expression::Array(arr) => &arr.elements,
        Expression::LegacyArray(arr) => &arr.elements,
        _ => return None,
    };
    elements
        .iter()
        .map(|element| match element {
            ArrayElement::Value(v) => super::helpers::extract_string_literal(v.value, content)
                .map(|(text, _, _)| text.to_string()),
            _ => None,
        })
        .collect()
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
    for segment in array_literal_entries(text) {
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
/// with `->as('name')`, `->using(CustomPivot::class)`, and/or
/// `->withPivot('col', …)`, and returns one [`PivotRelation`] per method that
/// declares any of them. Methods without a many-to-many call, or without any
/// pivot configuration, are skipped.
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

        // `as`/`using`/`withPivot` only appear on many-to-many relationships;
        // require the builder call so unrelated methods are not scanned.
        if !body.contains("belongsToMany(")
            && !body.contains("morphToMany(")
            && !body.contains("morphedByMany(")
        {
            continue;
        }

        let has_accessor_call = body.contains("->as(");
        let accessor = if has_accessor_call {
            extract_pivot_accessor(body)
                .map(PivotAccessor::Custom)
                .unwrap_or(PivotAccessor::Unknown)
        } else {
            PivotAccessor::Default
        };
        let using = extract_pivot_using(body);
        let columns = extract_with_pivot_columns(body);
        if !has_accessor_call && using.is_none() && columns.is_empty() {
            continue;
        }

        relations.push(PivotRelation {
            method: String::from_utf8_lossy(method.name.value).into_owned(),
            accessor,
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
/// `HasBuilder`/`HasCollection` detection, and `used_traits` the traits
/// the class body uses, needed for `SoftDeletes` detection.
pub(crate) fn extract_laravel_metadata<'a>(
    class: &class_like::Class<'a>,
    methods: &[MethodInfo],
    used_traits: &[Atom],
    use_generics: &[(Atom, Vec<PhpType>)],
    content: &str,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> LaravelMetadata {
    let factory_model = extract_factory_model(class.members.iter());

    let custom_collection = extract_class_return_from_body(class.members.iter(), "newCollection")
        .filter(|ty| {
            ty.base_name().is_some_and(|name| {
                name != "Collection" && name != "Illuminate\\Database\\Eloquent\\Collection"
            })
        })
        .or_else(|| {
            extract_custom_collection(&class.attribute_lists, use_generics, methods, content)
        });

    let custom_builder = extract_class_return_from_body(class.members.iter(), "newEloquentBuilder")
        .filter(|ty| {
            ty.base_name().is_some_and(|name| {
                name != "Builder" && name != "Illuminate\\Database\\Eloquent\\Builder"
            })
        })
        .or_else(|| extract_custom_builder(&class.attribute_lists, use_generics, methods, content));

    let custom_factory = methods
        .iter()
        .find(|m| m.name == "newFactory")
        .and_then(|m| m.return_type.as_ref())
        .filter(|ty| {
            ty.base_name().is_some_and(|name| {
                name != "Factory" && name != "Illuminate\\Database\\Eloquent\\Factories\\Factory"
            })
        })
        .cloned()
        .or_else(|| extract_class_return_from_body(class.members.iter(), "newFactory"))
        .or_else(|| extract_class_property(class.members.iter(), "factory"))
        .or_else(|| {
            extract_class_constant_attribute(&class.attribute_lists, content, b"UseFactory")
                .map(|name| PhpType::named(atom(&name)))
        });

    let policy_class = extract_use_policy_attribute(&class.attribute_lists, content);

    let (property_casts, method_casts) = extract_cast_sources(class.members.iter(), content);
    let mut declared = 0;
    if property_casts.is_some() {
        declared |= model_declaration::CASTS_PROPERTY;
    }
    if method_casts.is_some() {
        declared |= model_declaration::CASTS_METHOD;
    }
    let (casts_definitions, cast_sources) = match (property_casts, method_casts) {
        (Some(property), Some(method)) => {
            let merged = overlay_casts(&property, &method);
            (merged, Some(Box::new(CastSources { property, method })))
        }
        (Some(only), None) | (None, Some(only)) => (only, None),
        (None, None) => (Vec::new(), None),
    };

    let belongs_to_many_pivots = extract_pivot_relations(class.members.iter(), content);

    let attributes_definitions = extract_attributes_definitions(class.members.iter(), content);
    let attribute_defaults = extract_attribute_defaults(class.members.iter(), content);
    if declares_property(class.members.iter(), "attributes") {
        declared |= model_declaration::ATTRIBUTES;
    }
    if declares_property(class.members.iter(), "dates") {
        declared |= model_declaration::DATES;
    }

    let (column_names, column_sources, column_lists) =
        extract_column_names(class.members.iter(), content);
    declared |= column_lists;

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
    let unique_ids = methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case("uniqueIds"))
        .then(|| extract_unique_ids(class.members.iter(), content))
        .flatten();

    let dates_definitions = extract_dates_definitions(class.members.iter(), content);

    let TimestampConfig {
        timestamps,
        created_at_name,
        updated_at_name,
        deleted_at_name,
    } = extract_timestamp_config(class.members.iter(), content);
    let soft_deletes = used_traits.iter().any(|t| is_soft_deletes_trait(t));

    // Gate the member walk on the already-extracted method list: all but
    // the handful of facades in a project declare no `getFacadeAccessor()`,
    // and the slice is still warm from the checks above.
    let facade_accessor = methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case("getFacadeAccessor"))
        .then(|| extract_facade_accessor(class.members.iter(), content))
        .flatten();

    LaravelMetadata {
        custom_factory,
        factory_model,
        custom_collection,
        casts_definitions,
        cast_sources,
        declared,
        dates_definitions,
        attributes_definitions,
        attribute_defaults,
        column_names,
        column_sources,
        connection_name,
        table_name,
        has_get_connection_name_method,
        has_get_table_method,
        primary_key,
        key_type,
        has_get_key_name_method,
        unique_ids,
        timestamps,
        created_at_name,
        updated_at_name,
        soft_deletes,
        deleted_at_name,
        custom_builder,
        policy_class,
        belongs_to_many_pivots,
        facade_accessor,
    }
}

/// The [`model_declaration`] flags of `meta`, counting a non-empty list
/// with no flag as declared.
///
/// Metadata built by hand rather than parsed fills in the lists without
/// the flags; the lists alone are then the only record of a declaration.
fn declared_flags(meta: &LaravelMetadata) -> u16 {
    use model_declaration::*;
    let mut flags = meta.declared;
    if flags & (CASTS_PROPERTY | CASTS_METHOD) == 0 && !meta.casts_definitions.is_empty() {
        flags |= CASTS_PROPERTY;
    }
    if !meta.dates_definitions.is_empty() {
        flags |= DATES;
    }
    if !meta.attributes_definitions.is_empty() || !meta.attribute_defaults.is_empty() {
        flags |= ATTRIBUTES;
    }
    if flags & COLUMN_LISTS == 0 && !meta.column_names.is_empty() {
        flags |= FILLABLE;
    }
    flags
}

/// The `$casts` property and `casts()` method entries of `meta`, `None`
/// for a source it does not declare.
fn cast_parts(meta: &LaravelMetadata, flags: u16) -> (Option<&[CastEntry]>, Option<&[CastEntry]>) {
    let property = flags & model_declaration::CASTS_PROPERTY != 0;
    let method = flags & model_declaration::CASTS_METHOD != 0;
    match (property, method, meta.cast_sources.as_deref()) {
        (true, true, Some(sources)) => (Some(&sources.property), Some(&sources.method)),
        (true, true, None) => (Some(&meta.casts_definitions), Some(&[])),
        (true, false, _) => (Some(&meta.casts_definitions), None),
        (false, true, _) => (None, Some(&meta.casts_definitions)),
        (false, false, _) => (None, None),
    }
}

/// Whether `meta` declares anything a subclass could inherit through
/// [`inherit_model_metadata`].
pub(crate) fn has_inheritable_model_metadata(meta: &LaravelMetadata) -> bool {
    declared_flags(meta) != 0
        || meta.primary_key.is_some()
        || meta.key_type.is_some()
        || meta.timestamps.is_some()
        || meta.created_at_name.is_some()
        || meta.updated_at_name.is_some()
        || meta.soft_deletes
        || meta.deleted_at_name.is_some()
        || meta.connection_name.is_some()
        || meta.table_name.is_some()
        || meta.has_get_connection_name_method
        || meta.has_get_table_method
        || meta.has_get_key_name_method
}

/// Fill in the model configuration `child` does not declare from its
/// parent's.
///
/// PHP hands a subclass every property and constant it does not
/// redeclare, so `$fillable`, `$casts`, `$primaryKey`, `CREATED_AT` and
/// the rest each come from the nearest class in the parent chain that
/// declares them. Called once per parent while walking up the chain
/// nearest-first, so a declaration a closer class already supplied is
/// never overwritten. Each column list and each of `$casts` and `casts()`
/// is inherited on its own: a subclass that redeclares `$hidden` still
/// sees the parent's `$fillable`.
pub(crate) fn inherit_model_metadata(child: &mut LaravelMetadata, parent: &LaravelMetadata) {
    use model_declaration::*;
    let child_flags = declared_flags(child);
    let parent_flags = declared_flags(parent);
    let inherited = parent_flags & !child_flags;

    if inherited & (CASTS_PROPERTY | CASTS_METHOD) != 0 {
        let (own_property, own_method) = cast_parts(child, child_flags);
        let (parent_property, parent_method) = cast_parts(parent, parent_flags);
        let property = own_property.or(parent_property).map(<[_]>::to_vec);
        let method = own_method.or(parent_method).map(<[_]>::to_vec);
        match (property, method) {
            (Some(property), Some(method)) => {
                child.casts_definitions = overlay_casts(&property, &method);
                child.cast_sources = Some(Box::new(CastSources { property, method }));
            }
            (Some(only), None) | (None, Some(only)) => child.casts_definitions = only,
            (None, None) => {}
        }
    }

    if inherited & DATES != 0 {
        child
            .dates_definitions
            .clone_from(&parent.dates_definitions);
    }
    if inherited & ATTRIBUTES != 0 {
        child
            .attributes_definitions
            .clone_from(&parent.attributes_definitions);
        child
            .attribute_defaults
            .clone_from(&parent.attribute_defaults);
    }

    let inherited_lists = inherited & COLUMN_LISTS;
    if inherited_lists != 0 {
        child
            .column_sources
            .resize(child.column_names.len(), child_flags & COLUMN_LISTS);
        for (i, name) in parent.column_names.iter().enumerate() {
            let lists = parent
                .column_sources
                .get(i)
                .copied()
                .unwrap_or(parent_flags & COLUMN_LISTS)
                & inherited_lists;
            if lists == 0 {
                continue;
            }
            if let Some(pos) = child.column_names.iter().position(|n| n == name) {
                child.column_sources[pos] |= lists;
            } else {
                child.column_names.push(name.clone());
                child.column_sources.push(lists);
            }
        }
    }

    child.declared = child_flags | inherited;

    fn inherit<T: std::clone::Clone>(own: &mut Option<T>, parent: &Option<T>) {
        if own.is_none() {
            own.clone_from(parent);
        }
    }
    inherit(&mut child.primary_key, &parent.primary_key);
    inherit(&mut child.key_type, &parent.key_type);
    inherit(&mut child.timestamps, &parent.timestamps);
    inherit(&mut child.created_at_name, &parent.created_at_name);
    inherit(&mut child.updated_at_name, &parent.updated_at_name);
    inherit(&mut child.deleted_at_name, &parent.deleted_at_name);
    child.soft_deletes |= parent.soft_deletes;
    inherit(&mut child.connection_name, &parent.connection_name);
    inherit(&mut child.table_name, &parent.table_name);
    child.has_get_connection_name_method |= parent.has_get_connection_name_method;
    child.has_get_table_method |= parent.has_get_table_method;
    child.has_get_key_name_method |= parent.has_get_key_name_method;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::Backend;
    use crate::atom::atom;

    #[test]
    fn casts_method_returning_a_single_line_array_keeps_its_last_entry() {
        let src = r#"<?php
class User {
    protected function casts(): array {
        return ['nickname' => 'string', 'is_admin' => 'boolean'];
    }
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        let laravel = classes[0].0.laravel().unwrap();
        assert_eq!(
            laravel.casts_definitions,
            [
                ("nickname".to_string(), "string".to_string()),
                ("is_admin".to_string(), "boolean".to_string()),
            ]
        );
    }

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

    #[test]
    fn laravel_factory_model_property_is_extracted_and_resolved() {
        let src = r#"<?php
namespace Database\Factories;

use App\Models\Draft as DraftModel;

class ClassConstantFactory {
    protected $model = DraftModel::class;
}

class StringFactory {
    protected $model = 'Domain\\Models\\PublishedDraft';
}

class DoubleQuotedStringFactory {
    protected $model = "Domain\\Models\\PreviewDraft";
}

class DynamicFactory {
    protected $model = model_name();
}

class InterpolatedFactory {
    protected $model = "App\\Models\\$model";
}
"#;
        let mut classes: Vec<_> = Backend::parse_php_versioned_with_namespaces(src, None)
            .into_iter()
            .map(|(class, _)| class)
            .collect();
        let use_map = HashMap::from([("DraftModel".to_string(), "App\\Models\\Draft".to_string())]);
        Backend::resolve_parent_class_names(
            &mut classes,
            &use_map,
            &Some("Database\\Factories".to_string()),
        );

        let model_type = |class_name: &str| {
            classes
                .iter()
                .find(|class| class.name == atom(class_name))
                .and_then(|class| class.laravel())
                .and_then(|laravel| laravel.factory_model.as_ref())
                .map(ToString::to_string)
        };

        assert_eq!(
            model_type("ClassConstantFactory").as_deref(),
            Some("App\\Models\\Draft")
        );
        assert_eq!(
            model_type("StringFactory").as_deref(),
            Some("Domain\\Models\\PublishedDraft")
        );
        assert_eq!(
            model_type("DoubleQuotedStringFactory").as_deref(),
            Some("Domain\\Models\\PreviewDraft")
        );
        assert_eq!(model_type("DynamicFactory"), None);
        assert_eq!(model_type("InterpolatedFactory"), None);
    }

    #[test]
    fn laravel_factory_model_property_resolves_supported_class_name_forms() {
        let src = r#"<?php
namespace Database\Factories;

use App\Models as Models;

class LocalDraft {}

class NamespaceRelativeFactory {
    public function definition(): array { return []; }

    protected $unused = null, $model = LocalDraft::CLASS;
}

class QualifiedImportFactory {
    protected $model = Models\QualifiedDraft::class;
}

class FullyQualifiedFactory {
    protected $model = \App\Models\AbsoluteDraft::class;
}

class AbsoluteStringFactory {
    protected $model = '\\Domain\\Models\\AbsoluteDraft';
}

class BinaryStringFactory {
    protected $model = b'Domain\\Models\\BinaryDraft';
}

class EscapedDoubleQuotedStringFactory {
    protected $model = "\x44omain\\Models\\EscapedDraft";
}
"#;
        let mut classes: Vec<_> = Backend::parse_php_versioned_with_namespaces(src, None)
            .into_iter()
            .map(|(class, _)| class)
            .collect();
        let use_map = HashMap::from([("Models".to_string(), "App\\Models".to_string())]);
        Backend::resolve_parent_class_names(
            &mut classes,
            &use_map,
            &Some("Database\\Factories".to_string()),
        );

        let model_type = |class_name: &str| {
            classes
                .iter()
                .find(|class| class.name == atom(class_name))
                .and_then(|class| class.laravel())
                .and_then(|laravel| laravel.factory_model.as_ref())
                .map(ToString::to_string)
        };

        assert_eq!(
            model_type("NamespaceRelativeFactory").as_deref(),
            Some("Database\\Factories\\LocalDraft")
        );
        assert_eq!(
            model_type("QualifiedImportFactory").as_deref(),
            Some("App\\Models\\QualifiedDraft")
        );
        assert_eq!(
            model_type("FullyQualifiedFactory").as_deref(),
            Some("App\\Models\\AbsoluteDraft")
        );
        assert_eq!(
            model_type("AbsoluteStringFactory").as_deref(),
            Some("Domain\\Models\\AbsoluteDraft")
        );
        assert_eq!(
            model_type("BinaryStringFactory").as_deref(),
            Some("Domain\\Models\\BinaryDraft")
        );
        assert_eq!(
            model_type("EscapedDoubleQuotedStringFactory").as_deref(),
            Some("Domain\\Models\\EscapedDraft")
        );
    }

    #[test]
    fn laravel_factory_model_property_ignores_non_model_initializers() {
        let src = r#"<?php
namespace Database\Factories;

class FactoryParent {}

class UninitializedFactory {
    protected $model;
}

class EmptyStringFactory {
    protected $model = '   ';
}

class SelfFactory {
    protected $model = self::class;
}

class StaticFactory {
    protected $model = static::class;
}

class ParentFactory extends FactoryParent {
    protected $model = parent::class;
}

class OtherConstantFactory {
    private const MODEL_CLASS = 'App\\Models\\Draft';
    protected $model = self::MODEL_CLASS;
}

class UnrelatedPropertyFactory {
    protected $notModel = \App\Models\Draft::class;
}

class StaticPropertyFactory {
    protected static $model = \App\Models\Draft::class;
}

class NonUtf8StringFactory {
    protected $model = "\xff";
}

class InvalidEscapeFactory {
    protected $model = "\u{}";
}
"#;
        let mut classes: Vec<_> = Backend::parse_php_versioned_with_namespaces(src, None)
            .into_iter()
            .map(|(class, _)| class)
            .collect();
        Backend::resolve_parent_class_names(
            &mut classes,
            &HashMap::new(),
            &Some("Database\\Factories".to_string()),
        );

        for class_name in [
            "UninitializedFactory",
            "EmptyStringFactory",
            "SelfFactory",
            "StaticFactory",
            "ParentFactory",
            "OtherConstantFactory",
            "UnrelatedPropertyFactory",
            "StaticPropertyFactory",
            "NonUtf8StringFactory",
            "InvalidEscapeFactory",
        ] {
            let class = classes
                .iter()
                .find(|class| class.name == atom(class_name))
                .expect("factory model fixture should parse");
            let model = class
                .laravel()
                .and_then(|laravel| laravel.factory_model.as_ref());
            assert!(model.is_none(), "{class_name} must not declare a model");
        }
    }
}
