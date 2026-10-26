use mago_span::HasSpan;

use crate::atom::{atom, bytes_to_str, literal_bytes_to_str};
use crate::php_type::PhpType;
use crate::types::{FacadeAccessor, PivotAccessor, PivotRelation, model_declaration};
use crate::virtual_members::laravel::cast_literals::{
    array_literal_entries, extract_string_literal, parse_casts_array,
};
use crate::virtual_members::laravel::{
    extract_pivot_accessor, extract_pivot_using, extract_with_pivot_columns,
};

use super::*;

/// A property item's name without the leading `$`.
fn property_item_name<'a>(item: &class_like::property::PropertyItem<'a>) -> &'a str {
    let name = bytes_to_str(item.variable().name);
    name.strip_prefix('$').unwrap_or(name)
}

/// The source text `node` spans, if it lies within `content`.
fn span_text<'c>(node: &impl HasSpan, content: &'c str) -> Option<&'c str> {
    let span = node.span();
    content.get(span.start.offset as usize..span.end.offset as usize)
}

/// The source text of a property item's initializer, `None` for an item
/// declared without one.
fn initializer_text<'c>(
    item: &class_like::property::PropertyItem<'_>,
    content: &'c str,
) -> Option<&'c str> {
    let class_like::property::PropertyItem::Concrete(concrete) = item else {
        return None;
    };
    span_text(concrete.value, content)
}

/// The plain property items among `members` named `name` (without the
/// `$`), each with the declaration it belongs to, in source order.
pub(crate) fn plain_properties<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    name: &str,
) -> impl Iterator<
    Item = (
        &'a class_like::property::PlainProperty<'a>,
        &'a class_like::property::PropertyItem<'a>,
    ),
> {
    members
        .filter_map(|member| match member {
            class_like::member::ClassLikeMember::Property(
                class_like::property::Property::Plain(plain),
            ) => Some(plain),
            _ => None,
        })
        .flat_map(move |plain| {
            plain
                .items
                .iter()
                .filter(move |item| property_item_name(item) == name)
                .map(move |item| (plain, item))
        })
}

/// The initializer text of the first plain property named `name` (without
/// the `$`) that has one.
fn property_initializer_text<'a, 'c>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &'c str,
    name: &str,
) -> Option<&'c str> {
    plain_properties(members, name).find_map(|(_, item)| initializer_text(item, content))
}

/// The value of the first top-level `return` in `method`'s body.
fn first_return_value<'a>(
    method: &'a class_like::method::Method<'a>,
) -> Option<&'a Expression<'a>> {
    let class_like::method::MethodBody::Concrete(block) = &method.body else {
        return None;
    };
    block.statements.iter().find_map(|stmt| match stmt {
        Statement::Return(ret) => ret.value,
        _ => None,
    })
}

/// The value of the first top-level `return` in the first method named
/// `name` (case-insensitively).
pub(crate) fn method_return_value<'a>(
    mut members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    name: &str,
) -> Option<&'a Expression<'a>> {
    let method = members.find_map(|member| match member {
        class_like::member::ClassLikeMember::Method(method)
            if method.name.value.eq_ignore_ascii_case(name.as_bytes()) =>
        {
            Some(method)
        }
        _ => None,
    })?;
    first_return_value(method)
}

/// Where a model's attribute arrays name `attribute`: the offset of the
/// first character inside the quotes of the string literal, in `$casts`,
/// `$attributes` or a `casts()` method's returned array as a key, or in
/// `$fillable`, `$guarded`, `$hidden`, `$visible`, `$appends` or `$dates`
/// as an entry.
pub(crate) fn attribute_entry_offset(
    members: &[class_like::member::ClassLikeMember<'_>],
    attribute: &str,
) -> Option<u32> {
    let names = |literal: Option<&Expression<'_>>| -> Option<u32> {
        let Some(Expression::Literal(Literal::String(string))) = literal else {
            return None;
        };
        (crate::atom::literal_bytes_to_str(string.value?)? == attribute)
            .then(|| string.span().start.offset + 1)
    };
    let in_array = |array: &Expression<'_>, keyed: bool| {
        crate::parser::array_literal_elements(array)?
            .iter()
            .find_map(|element| {
                names(if keyed {
                    crate::parser::array_element_key(element)
                } else {
                    crate::parser::array_element_value(element)
                })
            })
    };
    let property_entry = |name: &str, keyed: bool| {
        plain_properties(members.iter(), name).find_map(|(_, item)| match item {
            class_like::property::PropertyItem::Concrete(concrete) => {
                in_array(concrete.value, keyed)
            }
            class_like::property::PropertyItem::Abstract(_) => None,
        })
    };
    property_entry("casts", true)
        .or_else(|| {
            method_return_value(members.iter(), "casts").and_then(|ret| in_array(ret, true))
        })
        .or_else(|| property_entry("attributes", true))
        .or_else(|| {
            [
                "fillable", "guarded", "hidden", "visible", "appends", "dates",
            ]
            .into_iter()
            .find_map(|name| property_entry(name, false))
        })
}

/// Extract the model class explicitly configured on a Laravel factory.
pub(super) fn extract_factory_model<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
) -> Option<PhpType> {
    // `Factory::modelName()` reads `$this->model`, which a static
    // property of the same name never answers.
    plain_properties(members, "model")
        .filter(|(plain, _)| !plain.modifiers.iter().any(|modifier| modifier.is_static()))
        .find_map(|(_, item)| class_named_by_default(item))
}

pub(super) fn extract_class_property<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    property: &str,
) -> Option<PhpType> {
    plain_properties(members, property)
        .filter(|(plain, _)| plain.modifiers.iter().any(|modifier| modifier.is_static()))
        .find_map(|(_, item)| class_named_by_default(item))
}

/// The class a property's default value names.
///
/// A `SomeModel::class` initializer follows PHP class-name resolution, so
/// its written name is left for the normal name-resolution pass. A quoted
/// class name is already a runtime string and is therefore marked absolute
/// before that pass, preventing the declaring namespace from being prepended.
fn class_named_by_default(item: &class_like::property::PropertyItem<'_>) -> Option<PhpType> {
    let class_like::property::PropertyItem::Concrete(concrete) = item else {
        return None;
    };
    if let Some(class) = crate::parser::class_fetch_identifier(concrete.value) {
        return Some(PhpType::named(atom(bytes_to_str(class.value()))));
    }
    let Expression::Literal(Literal::String(literal)) = concrete.value else {
        return None;
    };
    let name = literal_bytes_to_str(literal.value?)?
        .trim()
        .trim_start_matches('\\');
    (!name.is_empty()).then(|| PhpType::named(atom(&format!("\\{name}"))))
}

pub(super) fn extract_class_return_from_body<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    method_name: &str,
) -> Option<PhpType> {
    let value = method_return_value(members, method_name)?;
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
pub(super) type CastEntry = (String, String);

/// Extract Eloquent cast definitions from a class's members.
///
/// Scans the class members for:
/// 1. A `$casts` property with an array initializer (`protected $casts = [...]`)
/// 2. A `casts()` method whose body contains a `return [...]` statement
///
/// Returns the `(column_name, cast_type)` pairs of each source, `None`
/// for a source the class does not declare. [`overlay_casts`] merges
/// them the way Laravel does at runtime.
pub(super) fn extract_cast_sources<'a>(
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
                    if property_item_name(item) != "casts" {
                        continue;
                    }
                    has_property = true;
                    if let class_like::property::PropertyItem::Concrete(concrete) = item {
                        property_text = span_text(concrete.value, content);
                    }
                }
            }
            class_like::member::ClassLikeMember::Method(method)
                if method.name.value.eq_ignore_ascii_case(b"casts") =>
            {
                has_method = true;
                if let Some(value) = first_return_value(method) {
                    method_text = span_text(value, content);
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

/// Extract Eloquent attribute defaults from a class's `$attributes` property.
///
/// Scans the class members for a `$attributes` property with an array
/// initializer (`protected $attributes = [...]`) and infers PHP types
/// from the literal default values.
///
/// Returns a list of `(column_name, php_type, default_text)` triples.  For
/// example, `'role' => 'user'` produces `("role", "string", "'user'")` and
/// `'is_active' => true` produces `("is_active", "bool", "true")`.
pub(super) fn extract_attributes<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Vec<(String, PhpType, String)> {
    property_initializer_text(members, content, "attributes")
        .map(parse_attributes_array)
        .unwrap_or_default()
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
    for segment in array_literal_entries(text) {
        let Some((key_part, value_part)) = crate::text_scan::split_top_level_arrow(segment) else {
            continue;
        };
        let key_part = key_part.trim();
        let value_part = value_part.trim();

        let Some(key) = extract_string_literal(key_part) else {
            continue;
        };
        if key.is_empty() {
            continue;
        }

        if let Some(php_type) = attribute_default_type(value_part) {
            results.push((key, php_type, value_part.to_string()));
        }
    }

    results
}

/// The type an attribute default written as `value` gives its column: the
/// base type of a scalar literal (`'user'` is a `string`), `null`, or
/// `array` for an empty `[]`.  A populated array literal gives none.
fn attribute_default_type(value: &str) -> Option<PhpType> {
    if value == "[]" {
        return Some(PhpType::array());
    }
    crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(value)
        .filter(|ty| !ty.is_array_like())
        .map(|ty| ty.widen_scalar_literals())
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
pub(super) fn extract_timestamp_config<'a>(
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
                    if property_item_name(item) != "timestamps" {
                        continue;
                    }
                    match initializer_text(item, content).map(str::trim) {
                        Some("false") => timestamps = Some(false),
                        Some("true") => timestamps = Some(true),
                        _ => {}
                    }
                }
            }
            class_like::member::ClassLikeMember::Constant(constant) => {
                for item in constant.items.iter() {
                    let name = bytes_to_str(item.name.value);
                    if name != "CREATED_AT" && name != "UPDATED_AT" && name != "DELETED_AT" {
                        continue;
                    }
                    let value = span_text(item.value, content).map(str::trim);
                    let parsed = match value {
                        Some("null") | Some("NULL") => Some(None),
                        Some(v) => extract_string_literal(v).map(Some),
                        None => None,
                    };
                    if let Some(val) = parsed {
                        match name {
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
pub(super) struct TimestampConfig {
    pub(super) timestamps: Option<bool>,
    pub(super) created_at_name: Option<Option<String>>,
    pub(super) updated_at_name: Option<Option<String>>,
    pub(super) deleted_at_name: Option<String>,
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
pub(super) fn extract_column_names<'a>(
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
                let stripped = property_item_name(item);
                let Some(&(_, flag)) = targets.iter().find(|(name, _)| *name == stripped) else {
                    continue;
                };
                declared |= flag;
                if let Some(text) = initializer_text(item, content) {
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

    (names, sources, declared)
}

/// Whether the class declares a plain property named `target` (without
/// the `$`), whatever its initializer.
pub(super) fn declares_property<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    target: &str,
) -> bool {
    plain_properties(members, target).next().is_some()
}

/// Extract column names from the deprecated `$dates` property array.
///
/// Before `$casts`, Laravel used `protected $dates = [...]` to mark
/// columns as Carbon instances. Each column listed here should be
/// typed as `\Carbon\Carbon` by the virtual member provider.
pub(super) fn extract_dates_definitions<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Vec<String> {
    let mut names = Vec::new();
    let texts =
        plain_properties(members, "dates").filter_map(|(_, item)| initializer_text(item, content));
    for text in texts {
        for name in parse_string_list(text) {
            if !names.contains(&name) {
                names.push(name);
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
pub(super) fn extract_facade_accessor<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Option<FacadeAccessor> {
    let value = method_return_value(members, "getFacadeAccessor")?;

    if let Some((text, _, _)) =
        crate::virtual_members::laravel::helpers::extract_string_literal(value, content)
    {
        return Some(FacadeAccessor::Alias(atom(text)));
    }
    // `self::class` / `static::class` name the facade itself, which is
    // never the class it forwards to.
    let identifier = crate::parser::class_fetch_identifier(value)?;
    let name = bytes_to_str(identifier.value());
    (!name.is_empty()).then(|| FacadeAccessor::Class(atom(name)))
}

/// Extract the columns a model's own `uniqueIds()` override returns.
///
/// Only a `return [...]` of string literals is statically knowable. Any
/// other shape (`[$this->getKeyName(), 'uuid']`, a spread of
/// `parent::uniqueIds()`, a computed value) yields `None`, as does a
/// model that does not declare the method.
pub(super) fn extract_unique_ids<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
) -> Option<Vec<String>> {
    let value = method_return_value(members, "uniqueIds")?;
    let elements = match value {
        Expression::Array(arr) => &arr.elements,
        Expression::LegacyArray(arr) => &arr.elements,
        _ => return None,
    };
    elements
        .iter()
        .map(|element| match element {
            ArrayElement::Value(v) => {
                crate::virtual_members::laravel::helpers::extract_string_literal(v.value, content)
                    .map(|(text, _, _)| text.to_string())
            }
            _ => None,
        })
        .collect()
}

pub(super) fn extract_string_property<'a>(
    members: impl Iterator<Item = &'a class_like::member::ClassLikeMember<'a>>,
    content: &str,
    target: &str,
) -> Option<String> {
    plain_properties(members, target)
        .filter_map(|(_, item)| initializer_text(item, content))
        .find_map(|text| extract_string_literal(text.trim()))
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
pub(super) fn extract_pivot_relations<'a>(
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
