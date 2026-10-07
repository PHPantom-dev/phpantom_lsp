use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// PHP parsing and AST extraction.
///
/// This module contains the logic for parsing PHP source text using the
/// mago_syntax parser and extracting class information (methods, properties,
/// constants), `use` statement mappings, and namespace declarations from
/// the resulting AST.
///
/// Sub-modules:
/// - [`classes`]: Class, interface, trait, and enum extraction
/// - [`anonymous`]: Anonymous class (`new class { ... }`) extraction
/// - [`attributes`]: Native PHP `#[Attribute]` target bitmask extraction
/// - [`functions`]: Standalone function and `define()` constant extraction
/// - [`use_statements`]: `use` statement and namespace extraction
/// - [`ast_update`]: The `update_ast` orchestrator and name resolution
/// - [`error_format`]: Human-readable formatting for Mago parse errors
mod anonymous;
mod ast_update;
mod attributes;
mod classes;
pub(crate) mod error_format;
mod functions;
mod use_statements;

use crate::atom::{atom, bytes_to_str, last_segment};

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::php_type::{PhpType, TypeKind};
use crate::types::*;
use crate::util::strip_fqn_prefix;

/// Context for resolving PHPDoc type annotations from docblock comments.
///
/// Bundles the program's trivia (comments/whitespace) and the raw source
/// text so that extraction functions can look up the `/** ... */` comment
/// preceding any AST node and parse `@return` / `@var` tags from it.
pub(crate) struct DocblockCtx<'a> {
    pub trivias: &'a [Trivia<'a>],
    pub content: &'a str,
    /// Target PHP version for version-aware stub filtering.
    ///
    /// When `Some`, elements annotated with
    /// `#[PhpStormStubsElementAvailable]` whose version range excludes
    /// this version are filtered out during extraction.  Set when
    /// parsing phpstorm-stubs; left as `None` for user code (where the
    /// attribute is never used).
    pub php_version: Option<PhpVersion>,
    /// Use-statement map for the file being parsed.
    ///
    /// Maps short (imported or aliased) names to their fully-qualified
    /// equivalents, e.g. `"Available"` → `"JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable"`.
    /// Used to resolve attribute names that appear under an alias.
    pub use_map: HashMap<String, String>,
    /// The file-level namespace, if any.
    ///
    /// Used by [`is_known_deprecated_attr`] to distinguish unqualified
    /// `#[Deprecated]` in the global namespace (which is the native PHP
    /// 8.4 attribute) from `#[Deprecated]` inside a user namespace (which
    /// would resolve to `App\Deprecated`, not the built-in).
    pub namespace: Option<String>,
}

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
/// The matching rules mirror PHP's attribute name resolution:
///
/// - **Fully-qualified** (`\Deprecated`, `\JetBrains\PhpStorm\Deprecated`):
///   strip the leading `\` and compare against [`DEPRECATED_FQNS`].
/// - **Qualified** (`JetBrains\PhpStorm\Deprecated`): resolve the first
///   segment through the use-map, then compare the resolved FQN.  If the
///   first segment is not in the use-map, prepend the file namespace.
/// - **Local/unqualified** (`Deprecated`): look up the short name in the
///   use-map.  If found, compare the resolved FQN.  If not found, the
///   name is in the current namespace. Only matches if the file is in the
///   global namespace (i.e. the bare name equals a known FQN).
fn is_known_deprecated_attr(name: &Identifier<'_>, ctx: &DocblockCtx<'_>) -> bool {
    match name {
        Identifier::FullyQualified(fq) => {
            // Input boundary: AST fully-qualified names include the leading `\`.
            let stripped = strip_fqn_prefix(bytes_to_str(fq.value));
            DEPRECATED_FQNS.contains(&stripped)
        }
        Identifier::Qualified(q) => {
            // Resolve the first segment via the use-map, then rebuild.
            let q_str = bytes_to_str(q.value);
            let first_seg = q_str.split('\\').next().unwrap_or(q_str);
            if let Some(resolved_prefix) = ctx.use_map.get(first_seg) {
                let rest = &q_str[first_seg.len()..]; // includes leading '\'
                let fqn = format!("{}{}", resolved_prefix, rest);
                DEPRECATED_FQNS.contains(&fqn.as_str())
            } else {
                // No use-map entry — prepend file namespace if present.
                let fqn = if let Some(ns) = &ctx.namespace {
                    format!("{}\\{}", ns, q_str)
                } else {
                    q_str.to_string()
                };
                DEPRECATED_FQNS.contains(&fqn.as_str())
            }
        }
        Identifier::Local(local) => {
            // Check use-map first (e.g. `use JetBrains\PhpStorm\Deprecated;`)
            let local_str = bytes_to_str(local.value);
            if let Some(fqn) = ctx.use_map.get(local_str) {
                DEPRECATED_FQNS.contains(&fqn.as_str())
            } else {
                // No import — the name lives in the current namespace.
                // Only matches if the file is in the global namespace.
                let fqn = if let Some(ns) = &ctx.namespace {
                    format!("{}\\{}", ns, local_str)
                } else {
                    local_str.to_string()
                };
                DEPRECATED_FQNS.contains(&fqn.as_str())
            }
        }
    }
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

// ─── Thread-local parse cache ───────────────────────────────────────────────
//
// During a single pass (an LSP request, or diagnostics over one file) the
// file content is immutable and `with_parsed_program` may be called dozens
// of times for the same content string (once per unique variable subject,
// plus secondary helpers).  Each call would otherwise allocate a fresh
// `LocalArena` and re-parse the entire file from scratch.
//
// The cache below eliminates that redundancy.  [`with_parse_cache`] stores
// only the content (no parsing).  The first [`with_parsed_program`] call
// whose content matches then lazily parses the file and keeps the arena
// and the resulting `Program`.  Subsequent calls reuse the cached AST.
//
// A pass also reads other files: inferring the return type of a method
// declared elsewhere walks that method's body, and every resolution inside
// the walk parses the declaring file.  A nested [`with_parse_cache`] for a
// different file therefore registers it alongside the pass's own file, so
// all of those parses share one AST until the pass ends.  Only the most
// recently registered few are kept, which bounds what a pass that reaches
// into many files holds at once.
//
// The cache is activated by the outermost [`with_parse_cache`], which sets
// it up at the start of a block and tears it down (via `Drop`) when the
// block finishes.  Outside that scope `with_parsed_program` parses every
// time.
//
// ## Safety
//
// The `Program<'arena>` borrows from the `LocalArena` and the content
// string, both owned by the same [`CachedFile`].  The raw pointer is
// reconstituted to a reference only through a `&CachedFile`, and a caller
// holds an `Rc` to the file for as long as it uses the program, so neither
// eviction nor the guard clearing the cache can free them underneath it.

/// How many files besides the pass's own one the cache keeps parsed.
const MAX_CACHED_OTHER_FILES: usize = 4;

/// One file's content and, once something has asked for it, its AST.
struct CachedFile {
    content: Arc<String>,
    /// The arena that owns the AST nodes, and a pointer to the `Program`
    /// allocated in it.  Unset until the first parse of this file.
    parsed: std::cell::OnceCell<(mago_allocator::LocalArena, *const ())>,
}

impl CachedFile {
    fn new(content: Arc<String>) -> Rc<Self> {
        Rc::new(Self {
            content,
            parsed: std::cell::OnceCell::new(),
        })
    }

    fn holds(&self, content: &str) -> bool {
        // The pointer comparison includes the length, so a prefix of the
        // cached text, which starts at the same address, is not mistaken
        // for the whole of it.
        std::ptr::eq(self.content.as_str(), content) || self.content.as_str() == content
    }

    fn program(&self) -> &Program<'_> {
        let (_, program_ptr) = self.parsed.get_or_init(|| {
            let arena = mago_allocator::LocalArena::new();
            let file_id = mago_database::file::FileId::new(b"input.php");
            let program =
                mago_syntax::parser::parse_file_content(&arena, file_id, self.content.as_bytes());
            // The arena's chunks are heap-allocated and do not move when
            // the arena itself does, so the pointer outlives this move.
            let program_ptr: *const () = (program as *const Program<'_>).cast();
            (arena, program_ptr)
        });
        // SAFETY: the pointer came from a `&Program` that borrows only
        // the arena and `self.content`, both owned by `self`, which the
        // returned reference cannot outlive.
        unsafe { &*program_ptr.cast::<Program<'_>>() }
    }
}

/// The files of the active pass: its own file first, then the other files
/// it registered, least recently registered first.
struct ParseCache {
    files: Vec<Rc<CachedFile>>,
}

impl ParseCache {
    fn find(&self, content: &str) -> Option<Rc<CachedFile>> {
        self.files.iter().find(|file| file.holds(content)).cloned()
    }

    /// Make `content` available to the pass, evicting the oldest other
    /// file when that leaves too many.
    fn register(&mut self, content: &str, to_arc: impl FnOnce() -> Arc<String>) {
        match self.files.iter().position(|file| file.holds(content)) {
            Some(0) => {}
            Some(index) => {
                let file = self.files.remove(index);
                self.files.push(file);
            }
            None => {
                if self.files.len() > MAX_CACHED_OTHER_FILES {
                    self.files.remove(1);
                }
                self.files.push(CachedFile::new(to_arc()));
            }
        }
    }

    fn evict(&mut self, file: &Rc<CachedFile>) {
        self.files.retain(|cached| !Rc::ptr_eq(cached, file));
    }
}

thread_local! {
    static PARSE_CACHE: RefCell<Option<ParseCache>> = const { RefCell::new(None) };
}

/// RAII guard that clears the thread-local parse cache on drop.
///
/// Created by [`with_parse_cache`].  Must not be leaked (e.g. via
/// `std::mem::forget`) — doing so would leave the cache active past the
/// pass it belongs to.
pub(crate) struct ParseCacheGuard {
    /// `true` when this guard owns the cache and must clear it on drop.
    /// Nested guards have `owns_cache = false` and leave it untouched.
    owns_cache: bool,
}

impl Drop for ParseCacheGuard {
    fn drop(&mut self) {
        if self.owns_cache {
            PARSE_CACHE.with(|cell| {
                *cell.borrow_mut() = None;
            });
        }
    }
}

/// Unwrap parenthesized expressions to their inner expression.
pub(crate) fn unwrap_parens<'a>(
    expr: &'a mago_syntax::cst::Expression<'a>,
) -> &'a mago_syntax::cst::Expression<'a> {
    match expr {
        mago_syntax::cst::Expression::Parenthesized(p) => unwrap_parens(p.expression),
        other => other,
    }
}

/// Unwrap parentheses and any number of `!` prefixes from a condition,
/// returning the expression underneath and whether an odd number of `!`s
/// wrapped it.
pub(crate) fn unwrap_negation<'a>(
    expr: &'a mago_syntax::cst::Expression<'a>,
) -> (&'a mago_syntax::cst::Expression<'a>, bool) {
    use mago_syntax::cst::Expression;
    let mut negated = false;
    let mut inner = expr;
    loop {
        match inner {
            Expression::Parenthesized(p) => inner = p.expression,
            Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
                negated = !negated;
                inner = prefix.operand;
            }
            _ => return (inner, negated),
        }
    }
}

/// The elements of an array literal, in either spelling (`[…]` and the
/// legacy `array(…)`).
///
/// `None` for anything that is not an array literal.
pub(crate) fn array_literal_elements<'a, 'ast>(
    expr: &'a mago_syntax::cst::Expression<'ast>,
) -> Option<
    &'a mago_syntax::cst::sequence::TokenSeparatedSequence<
        'ast,
        mago_syntax::cst::array::ArrayElement<'ast>,
    >,
> {
    use mago_syntax::cst::Expression;
    match expr {
        Expression::Array(array) => Some(&array.elements),
        Expression::LegacyArray(array) => Some(&array.elements),
        _ => None,
    }
}

/// The value expression of an array element, or `None` for a hole in a
/// list (`[$a, , $c]`).
pub(crate) fn array_element_value<'b>(
    elem: &'b mago_syntax::cst::array::ArrayElement<'b>,
) -> Option<&'b mago_syntax::cst::Expression<'b>> {
    use mago_syntax::cst::array::ArrayElement;
    match elem {
        ArrayElement::KeyValue(kv) => Some(kv.value),
        ArrayElement::Value(v) => Some(v.value),
        ArrayElement::Variadic(v) => Some(v.value),
        ArrayElement::Missing(_) => None,
    }
}

/// The key expression of an array element written as `key => value`.
pub(crate) fn array_element_key<'b>(
    elem: &'b mago_syntax::cst::array::ArrayElement<'b>,
) -> Option<&'b mago_syntax::cst::Expression<'b>> {
    use mago_syntax::cst::array::ArrayElement;
    match elem {
        mago_syntax::cst::array::ArrayElement::KeyValue(kv) => Some(kv.key),
        ArrayElement::Value(_) | ArrayElement::Variadic(_) | ArrayElement::Missing(_) => None,
    }
}

/// The member list of a class-like node, in any of the four spellings.
///
/// `None` for an interface (whose methods have no body) and for every
/// other node kind, which a walker treats as "nothing to descend into
/// here".
pub(crate) fn class_like_members<'ast>(
    node: mago_syntax::cst::node::Node<'ast, 'ast>,
) -> Option<&'ast [mago_syntax::cst::class_like::member::ClassLikeMember<'ast>]> {
    use mago_syntax::cst::node::Node;
    match node {
        Node::Class(class) => Some(class.members.as_slice()),
        Node::AnonymousClass(class) => Some(class.members.as_slice()),
        Node::Trait(trait_) => Some(trait_.members.as_slice()),
        Node::Enum(enum_) => Some(enum_.members.as_slice()),
        _ => None,
    }
}

/// Call `visit` for each method of `members` that has a body, handing it
/// the method and the block.
pub(crate) fn for_each_concrete_method<'ast>(
    members: &'ast [mago_syntax::cst::class_like::member::ClassLikeMember<'ast>],
    mut visit: impl FnMut(
        &'ast mago_syntax::cst::class_like::method::Method<'ast>,
        &'ast mago_syntax::cst::Block<'ast>,
    ),
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;
    for member in members {
        if let ClassLikeMember::Method(method) = member
            && let MethodBody::Concrete(body) = &method.body
        {
            visit(method, body);
        }
    }
}

/// Call `visit` with each statement list an `if` can run: the body, the
/// body of every `elseif`, and the `else`.
///
/// Both spellings of an `if` (braced and `:`-delimited) are covered, since
/// `IfBody` already unifies them.
pub(crate) fn for_each_if_branch(
    if_stmt: &mago_syntax::cst::If<'_>,
    mut visit: impl FnMut(&[mago_syntax::cst::Statement<'_>]),
) {
    let _ = try_for_each_if_branch(if_stmt, |statements| {
        visit(statements);
        std::ops::ControlFlow::<()>::Continue(())
    });
}

/// [`for_each_if_branch`] for a walker that stops early: the first branch
/// `visit` breaks on ends the walk and its value comes back.
pub(crate) fn try_for_each_if_branch<B>(
    if_stmt: &mago_syntax::cst::If<'_>,
    mut visit: impl FnMut(&[mago_syntax::cst::Statement<'_>]) -> std::ops::ControlFlow<B>,
) -> std::ops::ControlFlow<B> {
    visit(if_stmt.body.statements())?;
    for statements in if_stmt.body.else_if_statements() {
        visit(statements)?;
    }
    if let Some(statements) = if_stmt.body.else_statements() {
        visit(statements)?;
    }
    std::ops::ControlFlow::Continue(())
}

/// Activate the thread-local parse cache for `content`.
///
/// While the returned [`ParseCacheGuard`] is alive, every call to
/// [`with_parsed_program`] whose `content` argument is byte-equal to
/// the cached content will reuse the already-parsed `Program` instead
/// of re-parsing.
///
/// Typical usage:
///
/// ```ignore
/// let _guard = with_parse_cache(content);
/// // … many calls to resolve_variable_types / resolve_variable_type / etc.
/// // All of them hit the cache instead of re-parsing.
/// // Guard is dropped here, clearing the cache.
/// ```
///
/// Called while a cache is already active, this adds `content` to it for
/// the rest of the outer pass and returns a guard that does nothing.
pub(crate) fn with_parse_cache(content: &str) -> ParseCacheGuard {
    install_parse_cache(content, || Arc::new(content.to_string()))
}

/// [`with_parse_cache`] for a caller that already holds the content
/// behind an `Arc`, which the cache then shares instead of copying.
///
/// Every LSP request passes through here, so the copy would otherwise be
/// paid once per request whether or not anything goes on to parse.
pub(crate) fn with_parse_cache_arc(content: Arc<String>) -> ParseCacheGuard {
    let text = Arc::clone(&content);
    install_parse_cache(&text, move || content)
}

fn install_parse_cache(content: &str, to_arc: impl FnOnce() -> Arc<String>) -> ParseCacheGuard {
    PARSE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        match cache.as_mut() {
            Some(cache) => {
                cache.register(content, to_arc);
                ParseCacheGuard { owns_cache: false }
            }
            None => {
                // Store only the content.  The parse is deferred until the
                // first `with_parsed_program` call that asks for it.
                *cache = Some(ParseCache {
                    files: vec![CachedFile::new(to_arc())],
                });
                ParseCacheGuard { owns_cache: true }
            }
        }
    })
}

/// Parse `content` with the mago-syntax parser and pass the resulting
/// `Program` (plus the content string) to `f`.
///
/// Handles the boilerplate that every parse entry-point needs:
/// allocating a `LocalArena` arena, creating a `FileId`, calling
/// `parse_file_content`, and wrapping the whole thing in
/// `catch_unwind` so that a parser panic doesn't crash the LSP
/// server.  On panic the error is logged (using `method_name` for
/// context) and `T::default()` is returned.
///
/// When the thread-local parse cache is active (see
/// [`with_parse_cache`]) and holds `content`, the previously parsed
/// `Program` is reused — no allocation or parsing occurs.
pub(crate) fn with_parsed_program<T: Default>(
    content: &str,
    method_name: &str,
    f: impl FnOnce(&Program<'_>, &str) -> T,
) -> T {
    // ── Fast path: check the thread-local cache ─────────────────
    // The borrow ends before `f` runs, so `f` is free to register more
    // files or parse others through the cache.
    let cached = PARSE_CACHE.with(|cell| cell.borrow().as_ref().and_then(|c| c.find(content)));

    if let Some(file) = cached {
        // The fast path runs both the lazy mago parse and the extraction
        // closure `f`.  Wrap them in `catch_unwind` — like the slow path
        // below — so a parser or extraction panic doesn't escape and
        // violate this function's "a parser panic doesn't crash the LSP
        // server" contract.  On panic the poisoned entry is evicted so the
        // next call re-parses from scratch.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f(file.program(), file.content.as_str())
        }));

        return match outcome {
            Ok(value) => value,
            Err(_) => {
                PARSE_CACHE.with(|cell| {
                    if let Some(cache) = cell.borrow_mut().as_mut() {
                        cache.evict(&file);
                    }
                });
                tracing::error!("PHPantom: parser panicked in {}", method_name);
                T::default()
            }
        };
    }

    // ── Slow path: parse from scratch ───────────────────────────
    let content_owned = content.to_string();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program =
            mago_syntax::parser::parse_file_content(&arena, file_id, content_owned.as_bytes());
        f(program, &content_owned)
    }));

    match result {
        Ok(value) => value,
        Err(_) => {
            tracing::error!("PHPantom: parser panicked in {}", method_name);
            T::default()
        }
    }
}

/// Convert a Mago [`Hint`] AST node directly to a [`PhpType`].
///
/// Since `Hint` only represents native PHP type syntax (no
/// generics, shapes, or callables), the mapping is straightforward.
pub(crate) fn extract_hint_type(hint: &Hint) -> PhpType {
    match hint {
        Hint::Identifier(ident) => PhpType::named(atom(bytes_to_str(ident.value()))),
        Hint::Nullable(nullable) => PhpType::nullable(extract_hint_type(nullable.hint)),
        Hint::Union(union) => {
            let mut members = Vec::new();
            collect_union_members(union.left, &mut members);
            collect_union_members(union.right, &mut members);
            PhpType::union(members)
        }
        Hint::Intersection(intersection) => {
            let mut members = Vec::new();
            collect_intersection_members(intersection.left, &mut members);
            collect_intersection_members(intersection.right, &mut members);
            PhpType::intersection(members)
        }
        Hint::Null(_) => PhpType::null(),
        Hint::True(_) => PhpType::true_(),
        Hint::False(_) => PhpType::false_(),
        Hint::Array(_) => PhpType::array(),
        Hint::Callable(_) => PhpType::callable(),
        Hint::Static(_) => PhpType::static_(),
        Hint::Self_(_) => PhpType::self_(),
        Hint::Parent(_) => PhpType::parent_(),
        Hint::Void(_) => PhpType::void(),
        Hint::Never(_) => PhpType::never(),
        Hint::Float(_) => PhpType::float(),
        Hint::Bool(_) => PhpType::bool(),
        Hint::Integer(_) => PhpType::int(),
        Hint::String(_) => PhpType::string(),
        Hint::Object(_) => PhpType::object(),
        Hint::Mixed(_) => PhpType::mixed(),
        Hint::Iterable(_) => PhpType::iterable(),
        Hint::Parenthesized(paren) => extract_hint_type(paren.hint),
    }
}

/// Recursively flatten nested `Hint::Union` nodes into a flat member list.
fn collect_union_members(hint: &Hint, members: &mut Vec<PhpType>) {
    match hint {
        Hint::Union(union) => {
            collect_union_members(union.left, members);
            collect_union_members(union.right, members);
        }
        other => members.push(extract_hint_type(other)),
    }
}

/// Recursively flatten nested `Hint::Intersection` nodes into a flat member list.
fn collect_intersection_members(hint: &Hint, members: &mut Vec<PhpType>) {
    match hint {
        Hint::Intersection(intersection) => {
            collect_intersection_members(intersection.left, members);
            collect_intersection_members(intersection.right, members);
        }
        other => members.push(extract_hint_type(other)),
    }
}

/// Extract parameter information from a method's parameter list.
///
/// When `content` is provided, default value expressions are extracted
/// from the source text using AST span offsets.  Pass `None` when the
/// source text is not available (the `default_value` field will be `None`
/// for every parameter in that case).
///
/// When `php_version` is `Some`, parameters annotated with
/// `#[PhpStormStubsElementAvailable]` whose version range excludes the
/// target version are filtered out.  When `None`, all parameters are
/// included.
pub(crate) fn extract_parameters(
    parameter_list: &FunctionLikeParameterList,
    content: Option<&str>,
    php_version: Option<PhpVersion>,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> Vec<ParameterInfo> {
    parameter_list
        .parameters
        .iter()
        .filter(|param| {
            // When a PHP version is configured, skip parameters that are
            // not available for that version.
            if let Some(ver) = php_version
                && let Some(ctx) = doc_ctx
            {
                is_param_available_for_version(param, ctx, ver)
            } else {
                true
            }
        })
        .map(|param| {
            let name = atom(bytes_to_str(param.variable.name));
            let is_variadic = param.ellipsis.is_some();
            let is_reference = param.ampersand.is_some();
            let has_default = param.default_value.is_some();
            let is_required = !has_default && !is_variadic;

            let native_type_hint = param.hint.as_ref().map(|h| extract_hint_type(h));

            // Check for a #[LanguageLevelTypeAware] override on the
            // parameter.  When present, it replaces the native type hint
            // with the version-appropriate type string.
            let mut type_hint = if let Some(ver) = php_version
                && let Some(ctx) = doc_ctx
            {
                extract_language_level_type_for_param(param, ctx, ver)
                    .or_else(|| native_type_hint.clone())
            } else {
                native_type_hint.clone()
            };

            // A parameter with a literal `null` default accepts null
            // regardless of its type hint. This covers the pre-8.4
            // implicit-nullable form `Type $x = null`, which PHP treats
            // as `?Type`. Fold null into the effective type so callers
            // passing null are not flagged. Re-applied after any docblock
            // `@param` merge overwrites `type_hint` (see the callers).
            if param_default_is_null(param) {
                type_hint = type_hint.map(PhpType::or_null);
            }

            let default_value = content.and_then(|src| {
                let dv = param.default_value.as_ref()?;
                let span = dv.value.span();
                let start = span.start.offset as usize;
                let end = span.end.offset as usize;
                let raw = src.get(start..end)?.trim().to_string();
                // Resolve `ClassName::class` to its FQN so that
                // downstream template substitution can find the class.
                if let Some(class_name) = raw.strip_suffix("::class") {
                    let fqn = resolve_default_class_name(class_name, doc_ctx);
                    Some(format!("{}::class", fqn))
                } else {
                    Some(raw)
                }
            });

            ParameterInfo {
                name,
                is_required,
                native_type_hint: native_type_hint.clone(),
                type_hint,
                description: None,
                default_value,
                is_variadic,
                is_reference,
                closure_this_type: None,
                param_out_type: None,
            }
        })
        .collect()
}

/// Whether a parameter's default value is the literal `null`.
///
/// Used to detect the implicit-nullable form `Type $x = null`, which PHP
/// treats as `?Type` (mandatory before 8.4, still permitted after).
pub(crate) fn param_default_is_null(
    param: &function_like::parameter::FunctionLikeParameter<'_>,
) -> bool {
    let Some(dv) = param.default_value.as_ref() else {
        return false;
    };
    match dv.value {
        Expression::Literal(Literal::Null(_)) => true,
        // `Null`, `NULL` and `\null` are the same constant.
        Expression::ConstantAccess(ca) => {
            let name = bytes_to_str(ca.name.value());
            name.strip_prefix('\\')
                .unwrap_or(name)
                .eq_ignore_ascii_case("null")
        }
        _ => false,
    }
}

/// Resolve a class name from a parameter default (`Foo::class`) to its FQN
/// using the file's use-map and namespace from [`DocblockCtx`].
fn resolve_default_class_name(name: &str, doc_ctx: Option<&DocblockCtx<'_>>) -> String {
    // Already fully-qualified.
    if let Some(stripped) = name.strip_prefix('\\') {
        return stripped.to_string();
    }
    let Some(ctx) = doc_ctx else {
        return name.to_string();
    };
    // Check use-map (handles `use App\Application;` → "Application" → "App\Application").
    if let Some(fqn) = ctx.use_map.get(name) {
        return fqn.clone();
    }
    // If the name has a namespace prefix (e.g. `Sub\Foo`), check the first segment.
    if let Some(first_seg) = name.split('\\').next()
        && first_seg != name
        && let Some(base) = ctx.use_map.get(first_seg)
    {
        let rest = &name[first_seg.len() + 1..];
        return format!("{}\\{}", base, rest);
    }
    // Prepend file namespace if present.
    if let Some(ref ns) = ctx.namespace {
        return format!("{}\\{}", ns, name);
    }
    name.to_string()
}

/// Extract visibility from a set of modifiers.
/// Defaults to `Public` if no visibility modifier is present.
pub(crate) fn extract_visibility<'a>(
    modifiers: impl Iterator<Item = &'a Modifier<'a>>,
) -> Visibility {
    for m in modifiers {
        if m.is_private() {
            return Visibility::Private;
        }
        if m.is_protected() {
            return Visibility::Protected;
        }
        if m.is_public() {
            return Visibility::Public;
        }
    }
    Visibility::Public
}

/// Extract property information from a class member Property node.
pub(crate) fn extract_property_info(
    property: &Property,
    content: Option<&str>,
) -> Vec<PropertyInfo> {
    use mago_syntax::cst::class_like::property::PropertyItem;

    let is_static = property.modifiers().iter().any(|m| m.is_static());
    let is_readonly = property.modifiers().iter().any(|m| m.is_readonly());
    let visibility = extract_visibility(property.modifiers().iter());

    let native_hint = property.hint().map(|h| extract_hint_type(h));

    let mut props = Vec::with_capacity(property.variables().len());
    let mut push_item = |item: &PropertyItem| {
        let default_value = match item {
            PropertyItem::Concrete(c) => content.and_then(|src| {
                let start = c.value.span().start.offset as usize;
                let end = c.value.span().end.offset as usize;
                src.get(start..end).map(Box::from)
            }),
            PropertyItem::Abstract(_) => None,
        };

        let var = item.variable();
        let raw_name = bytes_to_str(var.name).to_string();
        let name = if let Some(stripped) = raw_name.strip_prefix('$') {
            atom(stripped)
        } else {
            atom(&raw_name)
        };

        props.push(PropertyInfo {
            name,
            name_offset: var.span.start.offset,
            type_hint: native_hint.clone(),
            native_type_hint: native_hint.clone(),
            description: None,
            is_static,
            is_readonly,
            visibility,
            deprecation_message: None,
            deprecated_replacement: None,
            see_refs: Vec::new(),
            is_virtual: false,
            source: default_value.map(|value| PropertySource::DeclaredDefault { value }),
        });
    };

    match property {
        Property::Plain(plain) => {
            for item in &plain.items {
                push_item(item);
            }
        }
        Property::Hooked(hooked) => push_item(&hooked.item),
    }

    props
}

use crate::Backend;

impl Backend {
    /// Parse PHP source text and extract class information.
    /// Returns a Vec of ClassInfo for all classes found in the file.
    pub fn parse_php(&self, content: &str) -> Vec<ClassInfo> {
        Self::parse_php_versioned(content, None)
    }

    /// Version-aware variant of [`parse_php`].
    ///
    /// When `php_version` is `Some`, elements annotated with
    /// `#[PhpStormStubsElementAvailable]` whose version range excludes
    /// the target version are filtered out during extraction.
    pub fn parse_php_versioned(content: &str, php_version: Option<PhpVersion>) -> Vec<ClassInfo> {
        Self::parse_php_versioned_with_namespaces(content, php_version)
            .into_iter()
            .map(|(cls, _ns)| cls)
            .collect()
    }

    /// Like [`parse_php_versioned`] but returns each class together with
    /// the namespace block it was declared in.
    ///
    /// This is needed by [`parse_and_cache_content_versioned`](crate::resolution)
    /// so that multi-namespace stub files (e.g. PDO.php with both
    /// `namespace { }` and `namespace Pdo { }`) resolve parent class
    /// names against the correct namespace context.
    pub fn parse_php_versioned_with_namespaces(
        content: &str,
        php_version: Option<PhpVersion>,
    ) -> Vec<(ClassInfo, Option<String>)> {
        Self::parse_php_classes_by_block(content, php_version)
            .0
            .into_iter()
            .map(|(cls, ns, _)| (cls, ns))
            .collect()
    }

    /// Like [`parse_php_versioned_with_namespaces`] but also returns the
    /// file's `namespace` blocks, each with its own imports, and tags every
    /// class with the index of the block that declared it (`None` outside
    /// any block).
    ///
    /// The blocks are only returned for a file with more than one; a file
    /// with a single block resolves everything against its file-wide
    /// imports.
    ///
    /// [`parse_php_versioned_with_namespaces`]: Self::parse_php_versioned_with_namespaces
    #[allow(clippy::type_complexity)]
    pub(crate) fn parse_php_classes_by_block(
        content: &str,
        php_version: Option<PhpVersion>,
    ) -> (
        Vec<(ClassInfo, Option<String>, Option<usize>)>,
        Vec<NamespaceSpan>,
    ) {
        with_parsed_program(content, "parse_php", |program, content| {
            let mut use_map = HashMap::new();
            Self::extract_use_statements_from_statements(program.statements.iter(), &mut use_map);
            let namespace = Self::extract_namespace_from_statements(program.statements.iter());

            let mut blocks: Vec<NamespaceSpan> = Vec::new();
            for statement in program.statements.iter() {
                if let Statement::Namespace(ns) = statement {
                    let mut block_use_map = HashMap::new();
                    Self::extract_use_statements_from_statements(
                        ns.statements().iter(),
                        &mut block_use_map,
                    );
                    let span = ns.span();
                    blocks.push(NamespaceSpan {
                        namespace: ns
                            .name
                            .as_ref()
                            .map(|ident| bytes_to_str(ident.value()).to_string())
                            .filter(|n| !n.is_empty()),
                        start: span.start.offset,
                        end: span.end.offset,
                        use_map: block_use_map,
                    });
                }
            }
            if blocks.len() <= 1 {
                blocks.clear();
            }

            let doc_ctx = DocblockCtx {
                trivias: program.trivia.as_slice(),
                content,
                php_version,
                use_map,
                namespace,
            };

            let mut result: Vec<(ClassInfo, Option<String>, Option<usize>)> = Vec::new();
            let mut block_index = 0;

            for statement in program.statements.iter() {
                match statement {
                    Statement::Namespace(ns) => {
                        let block_ns: Option<String> = ns
                            .name
                            .as_ref()
                            .map(|ident| bytes_to_str(ident.value()).to_string())
                            .filter(|n| !n.is_empty());

                        let block = blocks.get(block_index).map(|span| (block_index, span));
                        block_index += 1;
                        let block_doc_ctx = block.map(|(_, span)| DocblockCtx {
                            trivias: doc_ctx.trivias,
                            content: doc_ctx.content,
                            php_version: doc_ctx.php_version,
                            use_map: span.use_map.clone(),
                            namespace: span.namespace.clone(),
                        });

                        let mut block_classes = Vec::new();
                        Self::extract_classes_from_statements(
                            ns.statements().iter(),
                            &mut block_classes,
                            Some(block_doc_ctx.as_ref().unwrap_or(&doc_ctx)),
                        );
                        for cls in block_classes {
                            result.push((cls, block_ns.clone(), block.map(|(i, _)| i)));
                        }
                    }
                    statement if Self::is_classlike_extraction_candidate(statement) => {
                        let mut top_classes = Vec::new();
                        Self::extract_classes_from_statements(
                            std::iter::once(statement),
                            &mut top_classes,
                            Some(&doc_ctx),
                        );
                        for cls in top_classes {
                            result.push((cls, None, None));
                        }
                    }
                    _ => {}
                }
            }

            // A class-like declared in two branches of a conditional yields
            // one entry per branch; keep the first so resolution is
            // deterministic (see `dedup_class_likes_first_wins`).
            Self::dedup_class_likes_first_wins(&mut result, |(cls, ns, _)| (cls, ns));

            (result, blocks)
        })
    }

    /// Parse PHP source text and extract standalone function definitions.
    ///
    /// Returns a list of `FunctionInfo` for every `function` declaration
    /// found at the top level (or inside a namespace block).
    pub fn parse_functions(&self, content: &str) -> Vec<FunctionInfo> {
        self.parse_functions_versioned(content, None)
    }

    /// Version-aware variant of [`parse_functions`].
    ///
    /// When `php_version` is `Some`, functions and parameters annotated
    /// with `#[PhpStormStubsElementAvailable]` whose version range
    /// excludes the target version are filtered out.
    pub fn parse_functions_versioned(
        &self,
        content: &str,
        php_version: Option<PhpVersion>,
    ) -> Vec<FunctionInfo> {
        with_parsed_program(content, "parse_functions", |program, content| {
            let mut use_map = HashMap::new();
            Self::extract_use_statements_from_statements(program.statements.iter(), &mut use_map);
            let namespace = Self::extract_namespace_from_statements(program.statements.iter());

            let doc_ctx = DocblockCtx {
                trivias: program.trivia.as_slice(),
                content,
                php_version,
                use_map,
                namespace,
            };

            let mut functions = Vec::new();
            Self::extract_functions_from_statements(
                program.statements.iter(),
                &mut functions,
                &None,
                Some(&doc_ctx),
            );
            functions
        })
    }

    /// Parse PHP source text and extract constant names from `define()` calls
    /// and top-level `const` statements.
    ///
    /// Returns a list of `(name, offset, value)` tuples for every
    /// `define('NAME', value)` call or `const NAME = value;` statement
    /// found at the top level, inside namespace blocks, block statements,
    /// or `if` guards.  A `const` is named fully-qualified, a `define()`
    /// exactly as it was written.
    pub fn parse_defines(&self, content: &str) -> Vec<(String, u32, Option<String>)> {
        with_parsed_program(content, "parse_defines", |program, content| {
            let mut defines = Vec::new();
            Self::extract_defines_from_statements(
                program.statements.iter(),
                &mut defines,
                content,
                None,
            );
            defines
        })
    }

    /// Parse PHP source text and extract `use` statement mappings.
    ///
    /// Returns a `HashMap` mapping short (imported) names to their
    /// fully-qualified equivalents.
    ///
    /// For example, `use Klarna\Rest\Resource;` produces
    /// `"Resource" → "Klarna\Rest\Resource"`.
    ///
    /// Handles:
    ///   - Simple use: `use Foo\Bar;`
    ///   - Aliased use: `use Foo\Bar as Baz;`
    ///   - Grouped use: `use Foo\{Bar, Baz};`
    ///   - Mixed grouped use: `use Foo\{Bar, function baz, const QUX};`
    ///     (function / const imports are skipped — we only track classes)
    ///   - Use statements inside namespace bodies
    pub(crate) fn parse_use_statements(&self, content: &str) -> HashMap<String, String> {
        with_parsed_program(content, "parse_use_statements", |program, _content| {
            let mut use_map = HashMap::new();
            Self::extract_use_statements_from_statements(program.statements.iter(), &mut use_map);
            use_map
        })
    }

    /// Parse PHP source text and extract the declared namespace (if any).
    ///
    /// Returns the namespace string (e.g. `"Klarna\Rest\Checkout"`) or
    /// `None` if the file has no namespace declaration.
    pub(crate) fn parse_namespace(&self, content: &str) -> Option<String> {
        with_parsed_program(content, "parse_namespace", |program, _content| {
            Self::extract_namespace_from_statements(program.statements.iter())
        })
    }
}

#[cfg(test)]
mod with_parsed_program_tests {
    //! A panic in the parser or extraction closure must be caught on the
    //! thread-local-cache fast path too (not just the slow path), and the
    //! poisoned cache entry must be evicted so the next call re-parses.

    use super::{with_parse_cache, with_parsed_program};

    #[test]
    fn fast_path_lazy_parse_recovers_from_panic() {
        let content = "<?php class Foo {}";
        let _guard = with_parse_cache(content);

        // First matching call lazily parses, then the closure panics.
        // The fast path must catch it and return the default.
        let caught: bool =
            with_parsed_program(content, "test", |_program, _content| panic!("boom"));
        assert!(
            !caught,
            "fast-path panic should be caught and default returned"
        );

        // The poisoned entry was evicted, so a later call re-parses and
        // runs the closure normally.
        let ok: bool = with_parsed_program(content, "test", |_program, _content| true);
        assert!(ok, "after eviction the next call must re-parse and succeed");
    }

    #[test]
    fn fast_path_ready_cache_recovers_from_panic() {
        let content = "<?php class Bar {}";
        let _guard = with_parse_cache(content);

        // Prime the cache so the program is already parsed (ready state).
        let primed: bool = with_parsed_program(content, "test", |_program, _content| true);
        assert!(primed);

        // A panic on the ready fast path is also caught.
        let caught: bool =
            with_parsed_program(content, "test", |_program, _content| panic!("boom"));
        assert!(
            !caught,
            "ready-cache panic should be caught and default returned"
        );

        // And the entry is evicted so subsequent calls still work.
        let ok: bool = with_parsed_program(content, "test", |_program, _content| true);
        assert!(ok, "after eviction the next call must re-parse and succeed");
    }

    fn program_address(content: &str) -> usize {
        with_parsed_program(content, "test", |program, _| {
            std::ptr::from_ref(program) as usize
        })
    }

    /// A prefix of the pass's file starts at the same address as the file
    /// itself, but it is different text and has to be parsed as itself.
    #[test]
    fn a_prefix_of_the_cached_file_is_not_handed_the_whole_files_program() {
        let content = "<?php class Foo {}\nclass Bar {}\n";
        let _guard = with_parse_cache(content);
        let prefix = &content[.."<?php class Foo {}".len()];

        let whole = with_parsed_program(content, "test", |program, _| program.statements.len());
        let (statements, text) = with_parsed_program(prefix, "test", |program, text| {
            (program.statements.len(), text.len())
        });
        assert_eq!(text, prefix.len());
        assert!(statements < whole, "{statements} statements of {whole}");
    }

    #[test]
    fn nested_scope_shares_one_parse_of_another_file_for_the_pass() {
        let own = "<?php class Own {}";
        let other = "<?php class Other {}";
        let _guard = with_parse_cache(own);

        // Registered from inside a parse of the pass's own file, the way a
        // body inference reaches into a declaring file mid-walk.
        let first = with_parsed_program(own, "test", |_, _| {
            let _nested = super::with_parse_cache(other);
            program_address(other)
        });
        assert_ne!(first, 0);
        assert_eq!(
            program_address(other),
            first,
            "the other file stays parsed after the nested scope ends"
        );
    }

    #[test]
    fn other_files_are_evicted_oldest_first_but_never_the_own_file() {
        let own = "<?php class Own {}";
        let _guard = with_parse_cache(own);
        let own_program = program_address(own);

        let others: Vec<String> = (0..=super::MAX_CACHED_OTHER_FILES)
            .map(|i| format!("<?php class Other{i} {{}}"))
            .collect();
        for other in &others {
            let _nested = super::with_parse_cache(other);
        }

        assert_eq!(program_address(own), own_program);
        let cached = |content: &str| {
            super::PARSE_CACHE.with(|cell| {
                cell.borrow()
                    .as_ref()
                    .is_some_and(|cache| cache.find(content).is_some())
            })
        };
        assert!(!cached(&others[0]), "the oldest other file was evicted");
        assert!(others[1..].iter().all(|other| cached(other)));
    }
}
