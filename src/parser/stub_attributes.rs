//! The phpstorm-stubs attributes the parser reads while extracting a
//! declaration: `#[PhpStormStubsElementAvailable]` (elements that exist
//! only for some PHP versions), `#[LanguageLevelTypeAware]` (types that
//! change between versions), `#[ArrayShape]`, and `#[Deprecated]`, which
//! PHP 8.4 also defines natively.

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{bytes_to_str, last_segment};
use crate::php_type::{PhpType, TypeKind};
use crate::types::*;

use super::DocblockCtx;

/// FQN constants for the JetBrains stub attributes we recognise.
/// Matching is done on the last segment of the resolved FQN so that
/// `#[PhpStormStubsElementAvailable]`, `#[\JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable]`,
/// and any `use ... as Alias` form all work.
const ATTR_ELEMENT_AVAILABLE: &str = "PhpStormStubsElementAvailable";

/// Last segment of the `LanguageLevelTypeAware` attribute FQN.
const ATTR_LANGUAGE_LEVEL_TYPE_AWARE: &str = "LanguageLevelTypeAware";

/// Last segment of the `ArrayShape` attribute FQN.
const ATTR_ARRAY_SHAPE: &str = "ArrayShape";

/// Fully-qualified names (without leading `\`) that we recognise as
/// deprecation attributes.  Only the native PHP 8.4 `\Deprecated` and
/// the JetBrains stubs `\JetBrains\PhpStorm\Deprecated` should match.
const DEPRECATED_FQNS: &[&str] = &["Deprecated", "JetBrains\\PhpStorm\\Deprecated"];

impl DocblockCtx<'_> {
    /// Resolve an attribute's short name through the file's use-map and
    /// return the last segment of the resolved FQN.
    ///
    /// For example, if the file has
    /// `use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable as Available;`
    /// then `resolve_attr_last_segment("Available")` returns
    /// `"PhpStormStubsElementAvailable"`.
    ///
    /// When the name is not in the use-map, returns `None` (the caller
    /// should fall back to the original name).
    fn resolve_attr_last_segment(&self, short_name: &str) -> Option<&str> {
        let fqn = self.use_map.get(short_name)?;
        Some(fqn.rsplit('\\').next().unwrap_or(fqn))
    }

    /// Check whether `attr_short_name` resolves to `PhpStormStubsElementAvailable`.
    pub(crate) fn is_element_available_attr(&self, attr_short_name: &[u8]) -> bool {
        let name_str = bytes_to_str(attr_short_name);
        let canonical = self.resolve_attr_last_segment(name_str).unwrap_or(name_str);
        canonical == ATTR_ELEMENT_AVAILABLE
    }

    /// Check whether `attr_short_name` resolves to `LanguageLevelTypeAware`.
    fn is_language_level_type_aware_attr(&self, attr_short_name: &[u8]) -> bool {
        let name_str = bytes_to_str(attr_short_name);
        let canonical = self.resolve_attr_last_segment(name_str).unwrap_or(name_str);
        canonical == ATTR_LANGUAGE_LEVEL_TYPE_AWARE
    }

    /// Check whether `attr_short_name` resolves to `ArrayShape`.
    fn is_array_shape_attr(&self, attr_short_name: &[u8]) -> bool {
        let name_str = bytes_to_str(attr_short_name);
        let canonical = self.resolve_attr_last_segment(name_str).unwrap_or(name_str);
        canonical == ATTR_ARRAY_SHAPE
    }
}

// ─── PhpStormStubsElementAvailable Attribute Parsing ────────────────────────

/// Version range extracted from a `#[PhpStormStubsElementAvailable]` attribute.
///
/// Both bounds are inclusive.  `None` means unbounded in that direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VersionAvailability {
    /// Earliest PHP version where the element is available (inclusive).
    pub from: Option<PhpVersion>,
    /// Latest PHP version where the element is available (inclusive).
    pub to: Option<PhpVersion>,
}

/// Check whether a function or method is available for the given PHP version
/// based on its `#[PhpStormStubsElementAvailable]` attributes.
///
/// Returns `true` when:
///   - The element has no `PhpStormStubsElementAvailable` attribute (always available).
///   - The element has the attribute and the version falls within its range.
///
/// Returns `false` when the attribute is present and the version is outside the range.
pub(crate) fn is_available_for_version(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    ctx: &DocblockCtx<'_>,
    php_version: PhpVersion,
) -> bool {
    if let Some(avail) = extract_version_availability(attribute_lists, ctx) {
        php_version.matches_range(avail.from, avail.to)
    } else {
        true
    }
}

/// Check whether a function, method, or class has been removed in the
/// target PHP version based on a `@removed X.Y` docblock tag.
///
/// Returns `true` when the element has `@removed X.Y` and the target
/// PHP version is >= X.Y (meaning it was removed before or at the
/// target version).
///
/// Returns `false` when there is no `@removed` tag or the target
/// version is older than the removal version.
pub(crate) fn is_removed_for_version(
    node: &impl HasSpan,
    ctx: &DocblockCtx<'_>,
    php_version: PhpVersion,
) -> bool {
    let docblock_text = crate::docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, node);
    if let Some(text) = docblock_text
        && let Some(removed_ver) = crate::docblock::extract_removed_version(text)
    {
        return php_version >= removed_ver;
    }
    false
}

/// Check whether a parameter is available for the given PHP version
/// based on its `#[PhpStormStubsElementAvailable]` attributes.
///
/// Same logic as [`is_available_for_version`] but operates on a single
/// parameter's attribute lists.
pub(crate) fn is_param_available_for_version(
    param: &function_like::parameter::FunctionLikeParameter<'_>,
    ctx: &DocblockCtx<'_>,
    php_version: PhpVersion,
) -> bool {
    if let Some(avail) = extract_version_availability(&param.attribute_lists, ctx) {
        php_version.matches_range(avail.from, avail.to)
    } else {
        true
    }
}

/// Extract the `from` / `to` version range from a
/// `#[PhpStormStubsElementAvailable(...)]` attribute, if present.
///
/// Supports both named and positional argument forms:
///   - `#[PhpStormStubsElementAvailable(from: '8.0')]`
///   - `#[PhpStormStubsElementAvailable(from: '8.0', to: '8.4')]`
///   - `#[PhpStormStubsElementAvailable(to: '7.4')]`
///   - `#[PhpStormStubsElementAvailable('8.1')]` (positional → treated as `from`)
///
/// Attribute names are resolved through the [`DocblockCtx`] use-map so
/// that aliases like `ElementAvailable` or `Available` (used in some
/// stub files) are recognised.
///
/// Returns `None` when the attribute is not present.
fn extract_version_availability(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    ctx: &DocblockCtx<'_>,
) -> Option<VersionAvailability> {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            if !ctx.is_element_available_attr(last_segment(attr.name.value())) {
                continue;
            }

            let arg_list = attr.argument_list.as_ref()?;
            let mut from: Option<PhpVersion> = None;
            let mut to: Option<PhpVersion> = None;

            for arg in arg_list.arguments.iter() {
                match arg {
                    argument::PartialArgument::Named(named) => {
                        let name = bytes_to_str(named.name.value).to_string();
                        let value = extract_string_literal_value(named.value, ctx.content);
                        if let Some(ver_str) = value {
                            let ver = PhpVersion::from_composer_constraint(&ver_str);
                            match name.as_str() {
                                "from" => from = ver,
                                "to" => to = ver,
                                _ => {}
                            }
                        }
                    }
                    argument::PartialArgument::Positional(positional) => {
                        // Positional argument is treated as `from`.
                        let value = extract_string_literal_value(positional.value, ctx.content);
                        if let Some(ver_str) = value {
                            from = PhpVersion::from_composer_constraint(&ver_str);
                        }
                    }
                    _ => {}
                }
            }

            return Some(VersionAvailability { from, to });
        }
    }

    None
}

// ─── LanguageLevelTypeAware Attribute Parsing ───────────────────────────────

/// Extract the type override from a `#[LanguageLevelTypeAware]` attribute
/// on a function, method, or property.
///
/// The attribute maps PHP version strings to type annotations:
/// ```php
/// #[LanguageLevelTypeAware(['8.0' => 'string|false', '8.1' => 'string'], default: 'string')]
/// ```
///
/// For the given `php_version`, selects the entry with the highest version
/// key that is `<=` the target.  Falls back to `default` when no entry
/// matches.  Returns `None` when the attribute is absent.
pub(crate) fn extract_language_level_type(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    ctx: &DocblockCtx<'_>,
    php_version: PhpVersion,
) -> Option<PhpType> {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            if !ctx.is_language_level_type_aware_attr(last_segment(attr.name.value())) {
                continue;
            }

            let arg_list = attr.argument_list.as_ref()?;
            let mut default_type: Option<String> = None;
            let mut version_map: Vec<(PhpVersion, String)> = Vec::new();

            for arg in arg_list.arguments.iter() {
                match arg {
                    argument::PartialArgument::Named(named) => {
                        let name = bytes_to_str(named.name.value).to_string();
                        if name == "default" {
                            default_type = extract_string_literal_value(named.value, ctx.content);
                        }
                    }
                    argument::PartialArgument::Positional(positional) => {
                        // The positional argument is the version → type array.
                        extract_version_type_pairs(positional.value, ctx.content, &mut version_map);
                    }
                    _ => {}
                }
            }

            // Select the best match: highest version key <= target.
            let mut best: Option<(PhpVersion, &str)> = None;
            for (ver, type_str) in &version_map {
                if *ver <= php_version && (best.is_none() || best.is_some_and(|(b, _)| *ver > b)) {
                    best = Some((*ver, type_str));
                }
            }

            if let Some((_, type_str)) = best {
                // Empty string means "no type" (untyped in older PHP).
                return if type_str.is_empty() {
                    None
                } else {
                    Some(PhpType::parse(type_str))
                };
            }

            // No version matched — use the default.
            if let Some(ref d) = default_type {
                return if d.is_empty() {
                    None
                } else {
                    Some(PhpType::parse(d))
                };
            }

            // Attribute present but couldn't parse — return None to keep
            // the native type hint unchanged.
            return None;
        }
    }

    None
}

/// Extract the type override from `#[LanguageLevelTypeAware]` on a
/// function-like parameter's attribute lists.
pub(crate) fn extract_language_level_type_for_param(
    param: &function_like::parameter::FunctionLikeParameter<'_>,
    ctx: &DocblockCtx<'_>,
    php_version: PhpVersion,
) -> Option<PhpType> {
    extract_language_level_type(&param.attribute_lists, ctx, php_version)
}

/// Extract an `array{key: type, ...}` shape from a `#[ArrayShape]` attribute.
///
/// phpstorm-stubs annotate functions and methods with
/// `#[ArrayShape(["key" => "type", ...])]` to declare the structure of
/// their array return values.  This function extracts that information
/// and returns it as a `TypeKind::ArrayShape`.
pub(crate) fn extract_array_shape_type(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    ctx: &DocblockCtx<'_>,
) -> Option<PhpType> {
    use crate::php_type::ShapeEntry;

    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            if !ctx.is_array_shape_attr(last_segment(attr.name.value())) {
                continue;
            }

            let arg_list = attr.argument_list.as_ref()?;
            // The ArrayShape attribute takes a single positional argument:
            // an associative array literal ["key" => "type", ...].
            let first_arg = arg_list.arguments.first()?;
            let expr = match first_arg {
                argument::PartialArgument::Positional(p) => p.value,
                argument::PartialArgument::Named(n) => n.value,
                _ => return None,
            };

            let elements: Box<dyn Iterator<Item = &ArrayElement<'_>>> = match expr {
                Expression::Array(arr) => Box::new(arr.elements.iter()),
                Expression::LegacyArray(arr) => Box::new(arr.elements.iter()),
                _ => return None,
            };

            let mut entries = Vec::new();
            for elem in elements {
                if let ArrayElement::KeyValue(kv) = elem {
                    let key = extract_string_literal_value(kv.key, ctx.content)?;
                    let value_str = extract_string_literal_value(kv.value, ctx.content)?;
                    let value_type = PhpType::parse(&value_str);
                    entries.push(ShapeEntry {
                        key: Some(key),
                        value_type,
                        optional: false,
                    });
                }
            }

            if !entries.is_empty() {
                return Some(PhpType::array_shape(entries));
            }
        }
    }

    None
}

/// Replace bare `array` components in `ty` with the shape from an
/// `#[ArrayShape]` attribute, if present.  Handles `array`, `?array`,
/// and `array|false` patterns.
pub(crate) fn apply_array_shape_override(
    ty: PhpType,
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    ctx: &DocblockCtx<'_>,
) -> PhpType {
    let Some(shape) = extract_array_shape_type(attribute_lists, ctx) else {
        return ty;
    };

    match &ty.kind() {
        // Exact `array` → replace with shape.
        TypeKind::Named(n) if n == "array" => shape,
        // `?array` → `?array{...}`
        TypeKind::Nullable(inner) => {
            if matches!(inner.kind(), TypeKind::Named(n) if n == "array") {
                PhpType::nullable(shape)
            } else {
                ty
            }
        }
        // `array|false` or other unions containing bare `array`.
        TypeKind::Union(members) => {
            let new_members: Vec<PhpType> = members
                .iter()
                .map(|m| {
                    if matches!(m.kind(), TypeKind::Named(n) if n == "array") {
                        shape.clone()
                    } else {
                        m.clone()
                    }
                })
                .collect();
            PhpType::union(new_members)
        }
        _ => ty,
    }
}

/// Parse the version → type array inside `LanguageLevelTypeAware`.
///
/// Handles both `['8.0' => 'string']` (short array) and
/// `array('8.0' => 'string')` (legacy array) syntax.
fn extract_version_type_pairs(
    expr: &Expression<'_>,
    content: &str,
    out: &mut Vec<(PhpVersion, String)>,
) {
    let elements: Box<dyn Iterator<Item = &ArrayElement<'_>>> = match expr {
        Expression::Array(arr) => Box::new(arr.elements.iter()),
        Expression::LegacyArray(arr) => Box::new(arr.elements.iter()),
        _ => return,
    };

    for elem in elements {
        if let ArrayElement::KeyValue(kv) = elem {
            let key = extract_string_literal_value(kv.key, content);
            let value = extract_string_literal_value(kv.value, content);
            if let (Some(ver_str), Some(type_str)) = (key, value)
                && let Some(ver) = PhpVersion::from_composer_constraint(&ver_str)
            {
                out.push((ver, type_str));
            }
        }
    }
}

/// Deprecation metadata extracted from a `#[Deprecated]` attribute.
///
/// phpstorm-stubs annotate hundreds of elements with this attribute.  The three
/// fields mirror the attribute's named arguments:
///
/// - `reason` — human-readable explanation (may also appear as a positional arg).
/// - `since` — PHP version when the element was deprecated.
/// - `replacement` — code template for auto-replacement, wired to the
///   "Replace deprecated call" code action.
///
/// When only a bare `#[Deprecated]` is present, all three fields are `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DeprecatedAttribute {
    pub reason: Option<String>,
    pub since: Option<String>,
    /// Code template for auto-replacement, e.g.
    /// `"exif_read_data(%parametersList%)"`.
    ///
    /// Template variables (`%parametersList%`, `%parameter0%`, `%class%`)
    /// are expanded at call sites by the "Replace deprecated call" code
    /// action.
    pub replacement: Option<String>,
}

/// Combined deprecation metadata returned by [`merge_deprecation_info`].
///
/// Bundles the human-readable message with the optional replacement
/// template so callers can populate both `deprecation_message` and
/// `deprecated_replacement` on the type structs in a single pass.
pub(crate) struct DeprecationInfo {
    /// Human-readable message (may be empty for bare `@deprecated` / `#[Deprecated]`).
    pub message: Option<String>,
    /// Replacement code template from `#[Deprecated(replacement: "...")]`.
    pub replacement: Option<String>,
}

impl DeprecatedAttribute {
    /// Build a deprecation message string suitable for storing in
    /// `deprecation_message` on `ClassInfo`, `MethodInfo`, etc.
    ///
    /// Combines `reason` and `since` into a single human-readable line.
    /// Returns an empty string when neither field is set (bare
    /// `#[Deprecated]`).
    pub fn to_message(&self) -> String {
        match (&self.reason, &self.since) {
            (Some(reason), Some(since)) => format!("{} (since PHP {})", reason, since),
            (Some(reason), None) => reason.clone(),
            (None, Some(since)) => format!("since PHP {}", since),
            (None, None) => String::new(),
        }
    }
}

/// Merge a docblock `@deprecated` message with a `#[Deprecated]` attribute.
///
/// The docblock tag takes priority (it is author-written and often more
/// specific).  When the docblock has no `@deprecated` tag, falls back to
/// the `#[Deprecated]` attribute if present.
///
/// **Version-aware suppression:** when the `#[Deprecated]` attribute has a
/// `since` field and the target PHP version (from `DocblockCtx`) is older
/// than `since`, the element is not considered deprecated and `None` is
/// returned for the message.  Docblock `@deprecated` tags have no
/// structured `since` data and are always honoured.
pub(crate) fn merge_deprecation_info(
    docblock_msg: Option<String>,
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> DeprecationInfo {
    // Docblock @deprecated always wins — it has no `since` field so
    // version-aware suppression does not apply.
    if docblock_msg.is_some() {
        return DeprecationInfo {
            message: docblock_msg,
            replacement: None,
        };
    }

    let Some(ctx) = doc_ctx else {
        return DeprecationInfo {
            message: None,
            replacement: None,
        };
    };

    let Some(attr) = extract_deprecated_attribute(attribute_lists, ctx) else {
        return DeprecationInfo {
            message: None,
            replacement: None,
        };
    };

    // Version-aware suppression: if the attribute declares `since` and
    // the project targets an older PHP version, this element is not yet
    // deprecated from the user's perspective.
    if let Some(since_str) = &attr.since
        && let Some(target) = ctx.php_version
        && let Some(since_ver) = PhpVersion::from_composer_constraint(since_str)
        && (target.major, target.minor) < (since_ver.major, since_ver.minor)
    {
        return DeprecationInfo {
            message: None,
            replacement: None,
        };
    }

    DeprecationInfo {
        message: Some(attr.to_message()),
        replacement: attr.replacement,
    }
}

/// Extract `#[Deprecated]` metadata from an element's attribute lists.
///
/// Supports the syntactic forms found in phpstorm-stubs:
///
/// - `#[Deprecated]` — bare, no arguments.
/// - `#[Deprecated("reason text")]` — positional reason.
/// - `#[Deprecated(reason: "...", since: "7.2")]` — named arguments.
/// - `#[Deprecated("reason", replacement: "...", since: "7.2")]` — mixed.
///
/// Attribute names are resolved through the [`DocblockCtx`] use-map so
/// that aliases (unlikely for `Deprecated` but technically possible) are
/// handled correctly.
///
/// Returns `None` when no `#[Deprecated]` attribute is present.
pub(crate) fn extract_deprecated_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    ctx: &DocblockCtx<'_>,
) -> Option<DeprecatedAttribute> {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            if !is_known_deprecated_attr(&attr.name, ctx) {
                continue;
            }

            // Bare #[Deprecated] — no argument list at all.
            let Some(arg_list) = attr.argument_list.as_ref() else {
                return Some(DeprecatedAttribute::default());
            };

            let mut reason: Option<String> = None;
            let mut since: Option<String> = None;
            let mut replacement: Option<String> = None;

            for arg in arg_list.arguments.iter() {
                match arg {
                    argument::PartialArgument::Named(named) => {
                        let name = bytes_to_str(named.name.value).to_string();
                        let value = extract_string_literal_value(named.value, ctx.content);
                        match name.as_str() {
                            // JetBrains stubs use `reason:`, native PHP 8.4
                            // `\Deprecated` uses `message:`.  Both mean the
                            // same thing — accept either.
                            "reason" | "message" => reason = value,
                            "since" => since = value,
                            "replacement" => replacement = value,
                            _ => {}
                        }
                    }
                    // First positional argument is the reason/message.
                    argument::PartialArgument::Positional(positional) if reason.is_none() => {
                        reason = extract_string_literal_value(positional.value, ctx.content);
                    }
                    _ => {}
                }
            }

            return Some(DeprecatedAttribute {
                reason,
                since,
                replacement,
            });
        }
    }

    None
}

/// Check whether an attribute identifier refers to one of the known
/// deprecation attributes (`\Deprecated` or `\JetBrains\PhpStorm\Deprecated`).
///
/// The name is read the way PHP reads an attribute's class name: through
/// the file's imports, else the current namespace.  An unimported bare
/// `Deprecated` therefore only matches in the global namespace.
fn is_known_deprecated_attr(name: &Identifier<'_>, ctx: &DocblockCtx<'_>) -> bool {
    let fqn =
        crate::names::resolve_to_fqn(bytes_to_str(name.value()), &ctx.use_map, &ctx.namespace);
    DEPRECATED_FQNS.contains(&fqn.as_str())
}

/// Whether an element carries PHP 8.5's native `#[\NoDiscard]` attribute.
///
/// The name is read through the file's imports and namespace like any
/// attribute class name, and compared case-insensitively as PHP compares
/// class names.  Without a [`DocblockCtx`] only the name as written is
/// available, which is taken to be global.
pub(crate) fn has_no_discard_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    ctx: Option<&DocblockCtx<'_>>,
) -> bool {
    attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
        .any(|attr| {
            let raw = bytes_to_str(attr.name.value());
            match ctx {
                Some(ctx) => crate::names::resolve_to_fqn(raw, &ctx.use_map, &ctx.namespace)
                    .eq_ignore_ascii_case("NoDiscard"),
                None => raw
                    .strip_prefix('\\')
                    .unwrap_or(raw)
                    .eq_ignore_ascii_case("NoDiscard"),
            }
        })
}

/// Extract the string value from a literal string expression by reading
/// the source text between the expression's span and stripping quotes.
fn extract_string_literal_value(
    expr: &expression::Expression<'_>,
    content: &str,
) -> Option<String> {
    let span = expr.span();
    let start = span.start.offset as usize;
    let end = span.end.offset as usize;
    let raw = content.get(start..end)?;
    // Strip surrounding quotes (single or double).
    let trimmed = raw.trim();
    if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('"') && trimmed.ends_with('"'))
    {
        Some(trimmed[1..trimmed.len() - 1].to_string())
    } else {
        Some(trimmed.to_string())
    }
}
