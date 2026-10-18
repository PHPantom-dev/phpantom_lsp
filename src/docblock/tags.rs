//! Core PHPDoc tag extraction.
//!
//! This submodule handles extracting type information from PHPDoc comments
//! (`/** ... */`), specifically `@return`, `@var`, `@param`, `@mixin`,
//! `@deprecated`, and `@phpstan-assert` / `@psalm-assert` tags.
//!
//! It also provides [`get_docblock_text_for_node`] to extract raw docblock
//! text from an AST node's preceding trivia.
//!
//! Template/generics/type-alias tags live in [`super::templates`].
//! Virtual member tags (`@property`, `@method`) live in
//! [`super::virtual_members`].  Choosing between a docblock type and a
//! native hint lives in [`super::type_override`], and scanning raw source
//! text for annotations in [`super::source_scan`].

use std::borrow::Cow;

use super::tag_kind::TagKind;
use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::atom;
use crate::symbol_map::docblock::get_docblock_text_with_offset;
use crate::types::{AssertionKind, CallableInvocation, ParameterInfo, PhpVersion, TypeAssertion};

use super::parser::{
    DocblockInfo, TagInfo, TagValueInfo, collapse_newlines, parse_docblock_for_tags,
};
use super::type_override::resolve_effective_type_with_template_bounds;
use super::type_strings::split_type_token;
use crate::php_type::{PhpType, TypeKind};

// ─── Public API ─────────────────────────────────────────────────────────────

/// Extract the type from a `@return` PHPDoc tag.
///
/// Handles common formats:
///   - `@return TypeName`
///   - `@return TypeName Some description text`
///   - `@return ?TypeName`
///   - `@return \Fully\Qualified\Name`
///   - `@return TypeName|null`
///
/// Returns the cleaned type string (leading `\` stripped) or `None` if no
/// `@return` tag is found.
pub fn extract_return_type(docblock: &str) -> Option<PhpType> {
    extract_type_via_mago(docblock, TagKind::Return)
}

/// Like [`extract_return_type`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_return_type_from_info(info: &DocblockInfo) -> Option<PhpType> {
    extract_type_via_mago_from_info(info, TagKind::Return)
}

/// Extract the deprecation message from a `@deprecated` PHPDoc tag.
///
/// Handles common formats:
///   - `@deprecated` → `Some("")`
///   - `@deprecated Some explanation text` → `Some("Some explanation text")`
///   - `@deprecated since 2.0` → `Some("since 2.0")`
///
/// Returns `None` when no `@deprecated` tag is present.
/// Returns `Some("")` when the tag is present but has no message.
/// Returns `Some("message")` when the tag includes explanatory text.
pub fn extract_deprecation_message(docblock: &str) -> Option<String> {
    extract_deprecation_message_from_info(&parse_docblock_for_tags(docblock)?)
}

/// Like [`extract_deprecation_message`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_deprecation_message_from_info(info: &DocblockInfo) -> Option<String> {
    let tag = info.first_tag_by_kind(TagKind::Deprecated)?;
    Some(tag.description.trim().to_owned())
}

/// Check whether a PHPDoc block contains an `@deprecated` tag.
///
/// Convenience wrapper around [`extract_deprecation_message`] for call
/// sites that only need a boolean check.
pub fn has_deprecated_tag(docblock: &str) -> bool {
    extract_deprecation_message(docblock).is_some()
}

/// Like [`has_deprecated_tag`], but operates on a pre-parsed [`DocblockInfo`].
pub fn has_deprecated_tag_from_info(info: &DocblockInfo) -> bool {
    extract_deprecation_message_from_info(info).is_some()
}

/// Extract the type from a `@psalm-if-this-is` or `@phpstan-if-this-is` tag.
///
/// `@psalm-if-this-is ArrayList<TOption|TEither>` returns
/// `Some(TypeKind::Generic("ArrayList", [Union(TOption, TEither)]))`.
pub fn extract_if_this_is_type(docblock: &str) -> Option<PhpType> {
    extract_if_this_is_type_from_info(&parse_docblock_for_tags(docblock)?)
}

/// Like [`extract_if_this_is_type`], but operates on a pre-parsed
/// [`DocblockInfo`].
pub fn extract_if_this_is_type_from_info(info: &DocblockInfo) -> Option<PhpType> {
    let tag = info
        .tags
        .iter()
        .find(|t| t.name == "psalm-if-this-is" || t.name == "phpstan-if-this-is")?;
    Some(PhpType::parse(&tag.type_text()?))
}

/// Extract the type from a `@psalm-this-out` or `@phpstan-self-out` tag.
///
/// `@psalm-this-out self<U>` returns `Some(PhpType::parse("self<U>"))`. When
/// both vendor variants are present, the higher-precedence one wins (see
/// [`DocblockInfo::first_tag_by_kind_vendor_first`]).
pub fn extract_self_out_type(docblock: &str) -> Option<PhpType> {
    extract_self_out_type_from_info(&parse_docblock_for_tags(docblock)?)
}

/// Like [`extract_self_out_type`], but operates on a pre-parsed
/// [`DocblockInfo`].
pub fn extract_self_out_type_from_info(info: &DocblockInfo) -> Option<PhpType> {
    let tag = info.first_tag_by_kind_vendor_first(TagKind::SelfOut)?;
    Some(PhpType::parse(&tag.type_text()?))
}

/// Whether the docblock declares the function or method side-effect free.
///
/// PHPStan and Psalm both spell it `@pure` and also accept their
/// vendor-prefixed forms.  None of the three is a tag the parser models, so
/// they arrive as `TagKind::Other` and are matched by name.
pub fn declares_pure(info: &DocblockInfo) -> bool {
    info.tags
        .iter()
        .any(|t| matches!(t.name.as_ref(), "pure" | "phpstan-pure" | "psalm-pure"))
}

/// Whether a docblock declares the declaration to have side effects via
/// `@impure`, `@phpstan-impure` or `@psalm-impure`.
///
/// The counterpart to [`declares_pure`]: it is how an author says that a
/// call changes state even though it returns a value, which is the one
/// case a return type cannot express.
pub fn declares_impure(info: &DocblockInfo) -> bool {
    info.tags.iter().any(|t| {
        matches!(
            t.name.as_ref(),
            "impure" | "phpstan-impure" | "psalm-impure"
        )
    })
}

/// Extract the PHP version from a `@removed` PHPDoc tag.
///
/// Handles the format `@removed X.Y` where `X.Y` is a PHP version
/// (e.g. `7.0`, `8.0`).
///
/// Returns `None` when no `@removed` tag is present or the version
/// cannot be parsed.
pub fn extract_removed_version(docblock: &str) -> Option<PhpVersion> {
    let info = parse_docblock_for_tags(docblock)?;
    // `@removed` is not a tag the parser models, so it arrives as
    // `TagKind::Other`.  We match by name instead.
    let tag = info.tags.iter().find(|t| t.name == "removed")?;
    let desc = tag.description.trim();
    if desc.is_empty() {
        return None;
    }
    PhpVersion::from_composer_constraint(desc)
}

/// Extract all `@see` references from a PHPDoc block.
///
/// Returns the raw text after each `@see` tag, which may be:
///   - A symbol reference: `ClassName`, `ClassName::method()`,
///     `ClassName::$property`, `functionName()`
///   - A URL: `https://example.com/docs`
///   - A doc reference: `doc://getting-started/index`
///
/// The full text after `@see` (including any trailing description) is
/// returned as-is, so `@see MyClass::foo() Use this instead` yields
/// `"MyClass::foo() Use this instead"`.
///
/// This is used alongside [`extract_deprecation_message`] to enrich
/// deprecated diagnostics with pointers to replacement APIs.
pub fn extract_see_references(docblock: &str) -> Vec<String> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };

    extract_see_references_from_info(&info)
}

/// Like [`extract_see_references`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_see_references_from_info(info: &DocblockInfo) -> Vec<String> {
    info.tags_by_kind(TagKind::See)
        .map(|tag| tag.description.trim().to_owned())
        .filter(|desc| !desc.is_empty())
        .collect()
}

/// Extract the deprecation message from a `@deprecated` PHPDoc tag,
/// enriched with any `@see` references from the same docblock.
///
/// Behaves like [`extract_deprecation_message`] but appends `@see`
/// references (if present) to the returned message.  This gives
/// diagnostic consumers a single string that includes both the
/// deprecation reason and pointers to replacement APIs.
///
/// Format examples:
///   - `@deprecated` alone → `Some("")`
///   - `@deprecated` + `@see NewClass` → `Some("See: NewClass")`
///   - `@deprecated Use new API` + `@see NewClass::method()` →
///     `Some("Use new API (see: NewClass::method())")`
///   - `@deprecated Use new API` + two `@see` tags →
///     `Some("Use new API (see: NewClass::method(), OtherFunc())")`
pub fn extract_deprecation_with_see(docblock: &str) -> Option<String> {
    let info = parse_docblock_for_tags(docblock)?;
    extract_deprecation_with_see_from_info(&info)
}

/// Like [`extract_deprecation_with_see`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_deprecation_with_see_from_info(info: &DocblockInfo) -> Option<String> {
    let base_msg = extract_deprecation_message_from_info(info)?;
    let see_refs = extract_see_references_from_info(info);

    if see_refs.is_empty() {
        return Some(base_msg);
    }

    let see_list = see_refs.join(", ");

    if base_msg.is_empty() {
        Some(format!("See: {}", see_list))
    } else {
        Some(format!("{} (see: {})", base_msg, see_list))
    }
}

/// Extract all `@mixin` tags from a class-level docblock.
///
/// PHPDoc `@mixin` tags declare that the annotated class exposes public
/// members from another class via magic methods (`__call`, `__get`, etc.).
/// The format is:
///
///   - `@mixin ClassName`
///   - `@mixin \Fully\Qualified\ClassName`
///   - `@mixin ClassName<TypeArg1, TypeArg2>`
///
/// Returns a list of `(base_class_name, generic_args)` tuples.  The base
/// class name has its leading `\` and generic parameters stripped.  The
/// `generic_args` vector is empty when the tag has no `<…>` suffix.
pub fn extract_mixin_tags(docblock: &str) -> Vec<(String, Vec<PhpType>)> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };

    extract_mixin_tags_from_info(&info)
}

/// Like [`extract_mixin_tags`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_mixin_tags_from_info(info: &DocblockInfo) -> Vec<(String, Vec<PhpType>)> {
    let mut results = Vec::new();

    for tag in info.tags_by_kind(TagKind::Mixin) {
        let Some(type_text) = tag.type_text() else {
            continue;
        };

        // Parse the type into a structured PhpType and extract the base
        // class name and optional generic arguments.
        let parsed = PhpType::parse(&type_text);

        // Collect individual type members. A union like `Foo|Bar` yields
        // multiple mixin entries, one per member.
        let members: Vec<&PhpType> = match parsed.kind() {
            TypeKind::Union(parts) => parts.iter().collect(),
            _ => vec![&parsed],
        };

        for member in members {
            let (base, generic_args) = match member.kind() {
                TypeKind::Generic(g) => {
                    let cleaned_args: Vec<PhpType> =
                        g.args.iter().map(strip_fqn_prefix_typed).collect();
                    (g.name.to_string(), cleaned_args)
                }
                TypeKind::Named(name) => (name.to_string(), vec![]),
                TypeKind::Nullable(inner) => match inner.kind() {
                    TypeKind::Named(name) => (name.to_string(), vec![]),
                    TypeKind::Generic(g) => {
                        let cleaned_args: Vec<PhpType> =
                            g.args.iter().map(strip_fqn_prefix_typed).collect();
                        (g.name.to_string(), cleaned_args)
                    }
                    _ => continue,
                },
                _ => continue,
            };

            if !base.is_empty() {
                results.push((base, generic_args));
            }
        }
    }

    results
}

/// Extract the required base class from a `@phpstan-require-extends`
/// (or `@psalm-require-extends` / bare `@require-extends`) tag.
///
/// This tag appears on traits to declare that any using class must extend
/// the named base class. Returns the class name as written (minus any
/// generic arguments), which is resolved to a fully-qualified name later
/// during post-processing. Returns `None` when no such tag is present.
pub fn extract_require_extends(docblock: &str) -> Option<String> {
    let info = parse_docblock_for_tags(docblock)?;
    extract_require_extends_from_info(&info)
}

/// Like [`extract_require_extends`], but operates on a pre-parsed
/// [`DocblockInfo`].
pub fn extract_require_extends_from_info(info: &DocblockInfo) -> Option<String> {
    for tag in info.tags_by_kind(TagKind::RequireExtends) {
        let Some(type_text) = tag.type_text() else {
            continue;
        };
        let base = match PhpType::parse(&type_text).kind() {
            TypeKind::Generic(g) => g.name.to_string(),
            TypeKind::Named(name) => name.to_string(),
            _ => continue,
        };
        if !base.is_empty() {
            return Some(base);
        }
    }
    None
}

/// Extract required interfaces from `@phpstan-require-implements`
/// (or `@psalm-require-implements` / bare `@require-implements`) tags.
///
/// This tag appears on traits to declare that any using class must implement
/// the named interface. Returns interface names as written (minus any generic
/// arguments), which are resolved to fully-qualified names later during
/// post-processing. Returns an empty vector when no such tag is present.
pub fn extract_require_implements(docblock: &str) -> Vec<String> {
    match parse_docblock_for_tags(docblock) {
        Some(info) => extract_require_implements_from_info(&info),
        None => Vec::new(),
    }
}

/// Like [`extract_require_implements`], but operates on a pre-parsed
/// [`DocblockInfo`].
pub fn extract_require_implements_from_info(info: &DocblockInfo) -> Vec<String> {
    let mut out = Vec::new();
    for tag in info.tags_by_kind(TagKind::RequireImplements) {
        let Some(type_text) = tag.type_text() else {
            continue;
        };
        let interface = match PhpType::parse(&type_text).kind() {
            TypeKind::Generic(g) => g.name.to_string(),
            TypeKind::Named(name) => name.to_string(),
            _ => continue,
        };
        if !interface.is_empty() {
            out.push(interface);
        }
    }
    out
}

/// Strip a leading `\` from a `PhpType` without a `to_string()` →
/// `PhpType::parse()` round-trip.  Uses `resolve_names` which already
/// walks the entire type structure recursively.
fn strip_fqn_prefix_typed(ty: &PhpType) -> PhpType {
    ty.resolve_names(&|name| name.strip_prefix('\\').unwrap_or(name).to_string())
}

/// Extract all `@throws` tags from a method-level docblock.
///
/// PHPDoc `@throws` tags declare which exceptions a method may throw.
/// The format is:
///
///   - `@throws ExceptionType`
///   - `@throws \Fully\Qualified\ExceptionType`
///   - `@throws ExceptionType Some description text`
///
/// Returns a list of parsed [`PhpType`] values as written.
pub fn extract_throws_tags(docblock: &str) -> Vec<PhpType> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };

    extract_throws_tags_from_info(&info)
}

/// Like [`extract_throws_tags`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_throws_tags_from_info(info: &DocblockInfo) -> Vec<PhpType> {
    let mut results = Vec::new();

    for tag in info.tags_by_kind(TagKind::Throws) {
        let Some(type_text) = tag.type_text() else {
            continue;
        };

        // The leading `\` stays: it is what tells the declaring file's
        // name resolution that `\LogicException` is the global class and
        // not one in the current namespace.
        if !type_text.is_empty() {
            results.push(PhpType::parse(&type_text));
        }
    }

    results
}

/// Extract `@phpstan-assert` / `@psalm-assert` type assertion annotations.
///
/// Supports all three variants:
///   - `@phpstan-assert Type $param`          → unconditional assertion
///   - `@phpstan-assert-if-true Type $param`  → assertion when return is true
///   - `@phpstan-assert-if-false Type $param` → assertion when return is false
///
/// Also supports the `@psalm-assert` equivalents and negated types
/// (`!Type`).
///
/// Returns a list of parsed assertions.  An empty list means no
/// assertion tags were found.
pub fn extract_type_assertions(docblock: &str) -> Vec<TypeAssertion> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };

    extract_type_assertions_from_info(&info)
}

/// Like [`extract_type_assertions`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_type_assertions_from_info(info: &DocblockInfo) -> Vec<TypeAssertion> {
    /// Map a `TagKind` to the corresponding `AssertionKind`.
    const fn assertion_kind_for(kind: TagKind) -> AssertionKind {
        match kind {
            TagKind::AssertIfTrue => AssertionKind::IfTrue,
            TagKind::AssertIfFalse => AssertionKind::IfFalse,
            // `TagKind::Assert` and anything else
            _ => AssertionKind::Always,
        }
    }

    const ASSERT_KINDS: &[TagKind] = &[
        TagKind::AssertIfTrue,
        TagKind::AssertIfFalse,
        TagKind::Assert,
    ];

    let mut results = Vec::new();

    for tag in info.tags_by_kinds(ASSERT_KINDS) {
        let Some((negated, is_equality, type_str, subject)) = assert_parts(tag) else {
            continue;
        };

        results.push(TypeAssertion {
            kind: assertion_kind_for(tag.kind),
            param_name: subject.into_owned(),
            asserted_type: PhpType::parse(&type_str),
            negated,
            is_equality,
        });
    }

    results
}

/// Split an assertion tag into `(negated, is_equality, asserted_type, subject)`.
///
/// The grammar reports the negation flag, the equality flag, the pattern and
/// the subject separately.  When it could not parse the tag (as with the
/// modifier stacking in `@phpstan-assert =!Foo $x`, which it declines to
/// model), the modifiers are peeled off by hand instead: `!` negates and `=`
/// marks an equality assertion.  The `=` form narrows the branch it belongs to
/// exactly as the subtype form does, but it may not be inverted into the
/// opposite branch, so the flag is carried rather than dropped.
fn assert_parts(tag: &TagInfo) -> Option<(bool, bool, Cow<'_, str>, Cow<'_, str>)> {
    if let TagValueInfo::Assert(value) = &tag.value {
        let type_text = value.type_text.as_deref()?;
        return Some((
            value.negated,
            value.is_equality,
            Cow::Borrowed(type_text),
            Cow::Borrowed(value.subject.as_str()),
        ));
    }

    let mut rest = tag.description.trim();
    let mut negated = false;
    let mut is_equality = false;
    loop {
        if let Some(r) = rest.strip_prefix('!') {
            negated = !negated;
            rest = r.trim_start();
        } else if let Some(r) = rest.strip_prefix('=') {
            is_equality = true;
            rest = r.trim_start();
        } else {
            break;
        }
    }

    let (type_str, remainder) = split_type_token(rest);
    let type_str = type_str.trim_end_matches(['.', ',']);
    if type_str.is_empty() {
        return None;
    }
    let subject = remainder
        .split_whitespace()
        .next()
        .filter(|token| token.starts_with('$'))?;

    Some((
        negated,
        is_equality,
        Cow::Borrowed(type_str),
        Cow::Borrowed(subject),
    ))
}

/// Extract the type from a `@var` PHPDoc tag.
///
/// Used for property type annotations like:
///   - `/** @var Session */`
///   - `/** @var \App\Models\User */`
pub fn extract_var_type(docblock: &str) -> Option<PhpType> {
    extract_type_via_mago(docblock, TagKind::Var)
}

/// Like [`extract_var_type`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_var_type_from_info(info: &DocblockInfo) -> Option<PhpType> {
    extract_type_via_mago_from_info(info, TagKind::Var)
}

/// Extract the type and optional variable name from a `@var` PHPDoc tag.
///
/// Handles both inline annotation formats:
///   - `/** @var TheType */`         → `Some(("TheType", None))`
///   - `/** @var TheType $var */`    → `Some(("TheType", Some("$var")))`
///
/// The variable name (if present) is returned **with** the `$` prefix so
/// callers can compare directly against AST variable names.
pub fn extract_var_type_with_name(docblock: &str) -> Option<(PhpType, Option<String>)> {
    extract_var_type_with_name_from_info(&parse_docblock_for_tags(docblock)?)
}

/// Like [`extract_var_type_with_name`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_var_type_with_name_from_info(
    info: &DocblockInfo,
) -> Option<(PhpType, Option<String>)> {
    for tag in info.tags_by_kind_vendor_first(TagKind::Var) {
        let Some(type_text) = tag.type_text() else {
            continue;
        };

        let parsed = sanitise_and_parse_docblock_type(&type_text)?;
        return Some((parsed, tag.variable().map(Cow::into_owned)));
    }
    None
}

/// Extract the raw (uncleaned) type from a `@param` tag for a specific
/// parameter in a docblock string.
///
/// Given a docblock and a parameter name (with `$` prefix), returns the
/// raw type string including generic parameters.
///
/// Example:
///   docblock containing `@param list<User> $users` with var_name `"$users"`
///   → `Some("list<User>")`
pub fn extract_param_raw_type(docblock: &str, var_name: &str) -> Option<PhpType> {
    extract_param_raw_type_from_info(&parse_docblock_for_tags(docblock)?, var_name)
}

/// Like [`extract_param_raw_type`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_param_raw_type_from_info(info: &DocblockInfo, var_name: &str) -> Option<PhpType> {
    // Check tags in priority order: @phpstan-param > @psalm-param > @param.
    // When both `@param` and `@psalm-param` document the same parameter,
    // the more specific variant must win, so iterate in vendor
    // precedence order rather than document order.
    for tag in info.tags_by_kind_vendor_first(TagKind::Param) {
        if tag.variable().as_deref() == Some(var_name)
            && let Some(type_text) = tag.type_text()
        {
            return sanitise_and_parse_docblock_type(&type_text);
        }
    }

    None
}

/// Merge a function or method's `@param` docblock tags into its parsed
/// native parameters: richer docblock types, per-parameter descriptions,
/// `@param-closure-this` binding, `@param-out` post-call types, when a
/// callable argument is invoked, and extra `@param` tags naming a
/// parameter `func_get_args()` reads that the native signature has none
/// for.
///
/// A `@param` tag that omits its variable name (common in
/// phpstorm-stubs, e.g. `@param callable(TValue, TKey): bool`) is matched
/// by position instead, once the name-based pass leaves a parameter
/// unenriched.
pub(crate) fn merge_param_docblock_into_parameters(
    info: &DocblockInfo,
    parameters: &mut Vec<ParameterInfo>,
    template_bounds: &crate::atom::AtomMap<PhpType>,
) {
    for param in parameters.iter_mut() {
        let param_doc_type = extract_param_raw_type_from_info(info, &param.name);
        if let Some(ref doc_type) = param_doc_type {
            let effective = resolve_effective_type_with_template_bounds(
                param.type_hint.as_ref(),
                Some(doc_type),
                template_bounds,
            );
            if effective.is_some() {
                param.type_hint = effective;
            }
        }
        param.description = extract_param_description_from_info(info, &param.name);
    }

    // Positional fallback for `@param` tags that omit the parameter name.
    // When the name-based merge above didn't enrich a parameter's type
    // hint, try matching unnamed `@param` tags by position.
    let positional_tags = extract_param_types_positional_from_info(info);
    for (idx, param) in parameters.iter_mut().enumerate() {
        let already_enriched = extract_param_raw_type_from_info(info, &param.name).is_some();
        if already_enriched {
            continue;
        }
        if let Some((None, doc_type)) = positional_tags.get(idx) {
            let effective = resolve_effective_type_with_template_bounds(
                param.type_hint.as_ref(),
                Some(doc_type),
                template_bounds,
            );
            if effective.is_some() {
                param.type_hint = effective;
            }
        }
    }

    // Populate `closure_this_type` from `@param-closure-this` tags so
    // that `$this` inside a closure argument resolves to the declared
    // type instead of the lexical class.
    for (this_type, param_name) in extract_param_closure_this_from_info(info) {
        if let Some(param) = parameters.iter_mut().find(|p| p.name == param_name) {
            param.closure_this_type = Some(this_type);
        }
    }

    // Populate `param_out_type` from `@param-out` tags so a by-reference
    // parameter's post-call type is read from the docblock's own
    // annotation rather than guessed from its declared input type.
    for (out_type, param_name) in extract_param_out_from_info(info) {
        if let Some(param) = parameters.iter_mut().find(|p| p.name == param_name) {
            param.param_out_type = Some(out_type);
        }
    }

    // Populate `callable_invocation` so a call site can tell whether a
    // callable it passes runs before the call returns.  The later-invoked
    // tags go last: a parameter tagged both ways is later-invoked, as
    // PHPStan reads it.
    for (kind, invocation) in [
        (
            TagKind::ParamImmediatelyInvokedCallable,
            CallableInvocation::Immediate,
        ),
        (
            TagKind::ParamLaterInvokedCallable,
            CallableInvocation::Later,
        ),
    ] {
        for tag in info.tags_by_kind(kind) {
            if let Some(name) = tag.variable()
                && let Some(param) = parameters.iter_mut().find(|p| p.name.as_str() == name)
            {
                param.callable_invocation = Some(invocation);
            }
        }
    }

    // Append extra `@param` tags that don't match any native parameter.
    // These document parameters accessed via `func_get_args()` or
    // similar mechanisms and should appear in hover/signature.
    for (tag_name, tag_type) in extract_all_param_tags_from_info(info) {
        if !parameters.iter().any(|p| p.name == tag_name) {
            let description = extract_param_description_from_info(info, &tag_name);
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
                param_out_type: None,
                callable_invocation: None,
            });
        }
    }
}

/// Extract all `@param` tags from a docblock as `(name, type)` pairs.
///
/// Returns a list where each entry is `(param_name, type_string)`.
/// The `param_name` includes the `$` prefix.  Variadic `...$name`
/// parameters are returned with the `$name` only (the `...` is stripped).
///
/// This is used to discover extra `@param` tags that document parameters
/// not present in the native function signature (e.g. parameters accessed
/// via `func_get_args()`).
pub fn extract_all_param_tags(docblock: &str) -> Vec<(String, PhpType)> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };

    extract_all_param_tags_from_info(&info)
}

/// Like [`extract_all_param_tags`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_all_param_tags_from_info(info: &DocblockInfo) -> Vec<(String, PhpType)> {
    let mut results = Vec::new();
    let mut seen_params = std::collections::HashSet::new();

    // Iterate in vendor precedence order so that when both `@param` and
    // `@psalm-param` document the same parameter, the more specific
    // variant wins.
    for tag in info.tags_by_kind_vendor_first(TagKind::Param) {
        if let Some(name) = tag.variable()
            && let Some(type_text) = tag.type_text()
            && seen_params.insert(name.to_string())
            && let Some(parsed) = sanitise_and_parse_docblock_type(&type_text)
        {
            results.push((name.into_owned(), parsed));
        }
    }

    results
}

/// Extract `@param` type strings in order of appearance, including
/// tags that omit the parameter name.
///
/// Returns a list of `(Option<param_name>, type_string)` pairs in
/// docblock order.  When a `@param` tag has no `$name` token (common
/// in phpstorm-stubs, e.g. `@param callable(TValue, TKey): bool`),
/// the first element is `None`.  Callers can match these entries to
/// native parameters by position.
///
/// This is used as a positional fallback when name-based matching
/// via [`extract_param_raw_type_from_info`] fails to find a docblock
/// type for a parameter.
pub fn extract_param_types_positional_from_info(
    info: &DocblockInfo,
) -> Vec<(Option<String>, PhpType)> {
    let mut results = Vec::new();

    for tag in info.tags_by_kind(TagKind::Param) {
        let Some(type_text) = tag.type_text() else {
            continue;
        };

        if let Some(parsed) = sanitise_and_parse_docblock_type(&type_text) {
            results.push((tag.variable().map(Cow::into_owned), parsed));
        }
    }

    results
}

/// Extract all `@param-closure-this` declarations from a docblock.
///
/// The tag format is `@param-closure-this TypeName $paramName`, declaring
/// that `$this` inside a closure passed as `$paramName` resolves to
/// `TypeName`.  This is the static-analysis equivalent of runtime
/// `Closure::bindTo()` and is used heavily in Laravel (routing, macros,
/// testing).
///
/// Returns a list of `(type, param_name)` pairs.  The `param_name`
/// includes the `$` prefix.  The type is parsed into a [`PhpType`].
pub fn extract_param_closure_this(docblock: &str) -> Vec<(PhpType, String)> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };

    extract_param_closure_this_from_info(&info)
}

/// Like [`extract_param_closure_this`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_param_closure_this_from_info(info: &DocblockInfo) -> Vec<(PhpType, String)> {
    let mut results = Vec::new();

    for tag in info.tags_by_kind(TagKind::ParamClosureThis) {
        if let Some(type_text) = tag.type_text()
            && let Some(name) = tag.variable()
        {
            results.push((PhpType::parse(&type_text), name.into_owned()));
        }
    }

    results
}

/// Extract all `@param-out` declarations from a docblock.
///
/// The tag format is `@param-out Type $paramName`, declaring what a
/// by-reference parameter holds after the call returns — often a
/// PHPStan conditional type keyed on the parameter's own pre-call value
/// (`@param-out ($arg is null ? A&I : A) $arg`).
///
/// Returns a list of `(type, param_name)` pairs.  The `param_name`
/// includes the `$` prefix.
pub fn extract_param_out_from_info(info: &DocblockInfo) -> Vec<(PhpType, String)> {
    let mut results = Vec::new();

    for tag in info.tags_by_kind(TagKind::ParamOut) {
        if let Some(type_text) = tag.type_text()
            && let Some(name) = tag.variable()
            && let Some(ty) = sanitise_and_parse_docblock_type(&type_text)
        {
            results.push((ty, name.into_owned()));
        }
    }

    results
}

/// Extract the human-readable description from a `@param` tag for a
/// specific parameter.
///
/// Given a docblock and a parameter name (with `$` prefix), returns the
/// description text that follows the type and `$name` on the `@param` line,
/// including any multi-line continuation (lines that don't start with `@`).
///
/// HTML tags like `<p>`, `</p>`, `<i>`, `</i>` are stripped.
///
/// Example:
///   `@param callable|null $callback Callback function to run for each element.`
///   with var_name `"$callback"` → `Some("Callback function to run for each element.")`
pub fn extract_param_description(docblock: &str, var_name: &str) -> Option<String> {
    extract_param_description_from_info(&parse_docblock_for_tags(docblock)?, var_name)
}

/// Like [`extract_param_description`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_param_description_from_info(info: &DocblockInfo, var_name: &str) -> Option<String> {
    for tag in info.tags_by_kind_vendor_first(TagKind::Param) {
        if tag.variable().as_deref() != Some(var_name) {
            continue;
        }

        // Multi-line tag values keep their newlines.  The description is
        // prose, so continuation lines join with a space.
        let normalised = collapse_newlines(tag.type_description().unwrap_or_default().as_ref());
        let cleaned = strip_html_tags(&normalised);
        let desc = cleaned.trim().to_string();
        if desc.is_empty() {
            return None;
        }
        return Some(desc);
    }

    None
}

/// Extract the human-readable description from the `@return` tag in a
/// docblock.
///
/// Returns the text that follows the type on the `@return` line,
/// including any multi-line continuation (lines that don't start with `@`).
///
/// HTML tags like `<p>`, `</p>`, `<i>`, `</i>` are stripped.
///
/// Example:
///   `@return array an array containing all the elements`
///   → `Some("an array containing all the elements")`
pub fn extract_return_description(docblock: &str) -> Option<String> {
    extract_return_description_from_info(&parse_docblock_for_tags(docblock)?)
}

/// Like [`extract_return_description`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_return_description_from_info(info: &DocblockInfo) -> Option<String> {
    for tag in info.tags_by_kind_vendor_first(TagKind::Return) {
        let desc = tag.description.trim();
        if desc.is_empty() {
            continue;
        }

        // Skip PHPStan conditional return types.
        if desc.starts_with('(') {
            return None;
        }

        // Multi-line tag values keep their newlines.  The description is
        // prose, so continuation lines join with a space.
        let normalised = collapse_newlines(tag.type_description().unwrap_or_default().as_ref());
        let cleaned = strip_html_tags(&normalised);
        let result = cleaned.trim().to_string();
        if result.is_empty() {
            return None;
        }
        return Some(result);
    }

    None
}

/// Extract the URL from a `@link` tag in a docblock.
///
/// Example:
///   `@link https://php.net/manual/en/function.array-map.php`
///   → `Some("https://php.net/manual/en/function.array-map.php")`
pub fn extract_link_urls(docblock: &str) -> Vec<String> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };

    extract_link_urls_from_info(&info)
}

/// Like [`extract_link_urls`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_link_urls_from_info(info: &DocblockInfo) -> Vec<String> {
    let mut urls = Vec::new();

    for tag in info.tags_by_kind(TagKind::Link) {
        let desc = tag.description.trim();
        // Take the first whitespace-delimited token as the URL.
        if let Some(url) = desc.split_whitespace().next()
            && !url.is_empty()
        {
            urls.push(url.to_string());
        }
    }

    urls
}

/// Convert common HTML tags in a docblock description to plain text.
///
/// Inline formatting tags (`<b>`, `<i>`, `<code>`, `<em>`, `<strong>`,
/// `<span>`) are unwrapped, `<br>` and `<p>` become line breaks, and list
/// and definition-list tags (`<ul>`/`<ol>`/`<li>`, `<dl>`/`<dt>`/`<dd>`)
/// become bullet-prefixed or indented lines. Tag matching is
/// case-insensitive. Unrecognised tags are left untouched.
fn strip_html_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '<' {
            if let Some(end) = s[i..].find('>') {
                let tag = &s[i..i + end + 1];
                let tag_lower = tag.to_ascii_lowercase();
                let replacement = match tag_lower.as_str() {
                    "<li>" => Some("- "),
                    "</li>" => Some("\n"),
                    "<ul>" | "<ol>" => Some("\n"),
                    "<dl>" | "</dl>" => Some("\n"),
                    "<dt>" => Some("\n"),
                    "<dd>" => Some("\n  "),
                    "<br>" | "<br/>" | "<br />" => Some("\n"),
                    "<p>" => Some("\n\n"),
                    "</ul>" | "</ol>" => Some("\n"),
                    "</p>" | "</dt>" | "</dd>" | "<i>" | "</i>" | "<b>" | "</b>" | "<code>"
                    | "</code>" | "<em>" | "</em>" | "<strong>" | "</strong>" | "<span>"
                    | "</span>" => Some(""),
                    _ => None,
                };
                if let Some(rep) = replacement {
                    result.push_str(rep);
                    for _ in 0..end {
                        chars.next();
                    }
                    continue;
                }
            }
            result.push(c);
        } else {
            result.push(c);
        }
    }
    result
}

// ─── Docblock Text Extraction ───────────────────────────────────────────────

/// Look up the docblock comment (if any) for a class-like member and return
/// its raw text.
///
/// This uses the program's trivia list to find the `/** ... */` comment that
/// immediately precedes the given AST node.  The `content` parameter is the
/// full source text and is used to verify there is no code between the
/// docblock and the node.
pub fn get_docblock_text_for_node<'a>(
    trivia: &'a [Trivia<'a>],
    content: &str,
    node: &impl HasSpan,
) -> Option<&'a str> {
    get_docblock_text_with_offset(trivia, content, node).map(|(text, _)| text)
}

/// Locate the docblock for an AST node and return it as a parsed
/// [`DocblockInfo`].
///
/// This combines [`get_docblock_text_for_node`] and
/// [`parse_docblock_for_tags`] into a single call, eliminating
/// redundant re-parsing when multiple tags need to be extracted from
/// the same docblock.
pub fn get_docblock_info_for_node(
    trivia: &[Trivia<'_>],
    content: &str,
    node: &impl HasSpan,
) -> Option<DocblockInfo> {
    let text = get_docblock_text_for_node(trivia, content, node)?;
    parse_docblock_for_tags(text)
}

// ─── Docblock Type Parsing ──────────────────────────────────────────────────

/// Parse a raw docblock type string into a [`PhpType`], applying
/// unclosed-bracket recovery when necessary.
///
/// Raw docblock strings from `extract_return_type`, `extract_var_type`,
/// `extract_param_raw_type`, etc. may contain malformed type expressions
/// (e.g. `"static<"`, `"Collection<int"`) when multi-line annotations
/// couldn't be fully joined.  This helper recovers the base type in such
/// cases, matching the sanitisation logic in
/// [`super::resolve_effective_type_typed`].
///
/// Returns `None` when the string is completely unrecoverable (e.g.
/// `"<garbage"` with no base type) or is not a valid type at all (e.g.
/// `"[$x]"`).  Like PHPStan, an invalid type is discarded rather than
/// kept as raw text, so the declaration falls back to its native type.
pub fn sanitise_and_parse_docblock_type(raw: &str) -> Option<PhpType> {
    if crate::docblock::type_strings::has_unclosed_delimiters(raw) {
        let base = recover_base_type(raw);
        if base.is_empty() {
            None
        } else {
            PhpType::try_parse(base)
        }
    } else {
        PhpType::try_parse(raw)
    }
}

// ─── Internals ──────────────────────────────────────────────────────────────

/// Generic tag extraction: find `@tag TypeName` and return the cleaned type.
///
/// Searches the parsed docblock for the first usable tag of `kind`,
/// preferring vendor-prefixed variants (`@phpstan-return` over `@return`).
///
/// **Skips** PHPStan conditional return types (those starting with `(`).
/// Use [`super::extract_conditional_return_type`] for those.
fn extract_type_via_mago(docblock: &str, kind: TagKind) -> Option<PhpType> {
    extract_type_via_mago_from_info(&parse_docblock_for_tags(docblock)?, kind)
}

/// Like [`extract_type_via_mago`], but operates on a pre-parsed [`DocblockInfo`].
fn extract_type_via_mago_from_info(info: &DocblockInfo, kind: TagKind) -> Option<PhpType> {
    // Vendor-prefixed tags outrank the plain form; return on the first
    // usable match.
    for tag in info.tags_by_kind_vendor_first(kind) {
        let Some(type_text) = tag.type_text() else {
            continue;
        };

        let parsed = sanitise_and_parse_docblock_type(&type_text);

        // A leading `(` may open either a PHPStan conditional return type
        // (`($p is T ? A : B)`) or a parenthesized type group such as a
        // DNF `(A&B)|null`.  Conditionals are handled separately by
        // `extract_conditional_return_type`, so bail here only when the
        // type genuinely parses as a conditional; a parenthesized
        // union/intersection group is a normal type and must be returned.
        if matches!(parsed.as_deref(), Some(TypeKind::Conditional { .. })) {
            return None;
        }
        return parsed;
    }

    None
}

/// Attempt to recover a usable base type from a type string with unclosed
/// brackets.  Truncates at the first unclosed `<` or `{` and returns the
/// base portion (e.g. `static<…broken` → `static`,
/// `Collection<int, User` → `Collection`).  Returns an empty string if
/// nothing useful can be recovered.
fn recover_base_type(s: &str) -> &str {
    // Walk forward and find the position where the first `<` or `{`
    // opens without a corresponding close.
    let mut angle: i32 = 0;
    let mut brace: i32 = 0;
    let mut first_unclosed = None;
    for (i, c) in s.char_indices() {
        match c {
            '<' => {
                if angle == 0 && brace == 0 && first_unclosed.is_none() {
                    first_unclosed = Some(i);
                }
                angle += 1;
            }
            '>' if angle > 0 => {
                angle -= 1;
                if angle == 0 && brace == 0 {
                    first_unclosed = None;
                }
            }
            '{' => {
                if brace == 0 && angle == 0 && first_unclosed.is_none() {
                    first_unclosed = Some(i);
                }
                brace += 1;
            }
            '}' if brace > 0 => {
                brace -= 1;
                if brace == 0 && angle == 0 {
                    first_unclosed = None;
                }
            }
            _ => {}
        }
    }
    match first_unclosed {
        Some(pos) => {
            let base = s[..pos].trim();
            if base.is_empty() { "" } else { base }
        }
        None => s,
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_deprecation_message ─────────────────────────────────

    #[test]
    fn bare_deprecated_tag() {
        let doc = "/** @deprecated */";
        assert_eq!(extract_deprecation_message(doc), Some(String::new()));
    }

    #[test]
    fn deprecated_tag_with_message() {
        let doc = "/** @deprecated Use collect() instead. */";
        assert_eq!(
            extract_deprecation_message(doc),
            Some("Use collect() instead.".to_string())
        );
    }

    #[test]
    fn deprecated_tag_with_version() {
        let doc = "/**\n * @deprecated since 2.0\n */";
        assert_eq!(
            extract_deprecation_message(doc),
            Some("since 2.0".to_string())
        );
    }

    #[test]
    fn deprecated_tag_with_tab_separator() {
        let doc = "/** @deprecated\tUse foo() */";
        assert_eq!(
            extract_deprecation_message(doc),
            Some("Use foo()".to_string())
        );
    }

    #[test]
    fn no_deprecated_tag() {
        let doc = "/** @return string */";
        assert_eq!(extract_deprecation_message(doc), None);
    }

    #[test]
    fn deprecated_bare_on_own_line() {
        let doc = "/**\n * @deprecated\n */";
        assert_eq!(extract_deprecation_message(doc), Some(String::new()));
    }

    #[test]
    fn deprecated_with_message_multiline_docblock() {
        let doc = "/**\n * Some description.\n * @deprecated Use newMethod() instead.\n * @return void\n */";
        assert_eq!(
            extract_deprecation_message(doc),
            Some("Use newMethod() instead.".to_string())
        );
    }

    #[test]
    fn has_deprecated_tag_returns_true() {
        let doc = "/** @deprecated Use foo() */";
        assert!(has_deprecated_tag(doc));
    }

    #[test]
    fn has_deprecated_tag_returns_false() {
        let doc = "/** @return string */";
        assert!(!has_deprecated_tag(doc));
    }

    // ── extract_see_references ──────────────────────────────────────

    #[test]
    fn see_references_empty_when_no_see_tag() {
        let doc = "/** @deprecated Use foo() */";
        assert!(extract_see_references(doc).is_empty());
    }

    #[test]
    fn see_references_single_class() {
        let doc = "/**\n * @deprecated\n * @see NewClass\n */";
        assert_eq!(extract_see_references(doc), vec!["NewClass"]);
    }

    #[test]
    fn see_references_method() {
        let doc = "/**\n * @deprecated\n * @see MyClass::newMethod()\n */";
        assert_eq!(extract_see_references(doc), vec!["MyClass::newMethod()"]);
    }

    #[test]
    fn see_references_property() {
        let doc = "/**\n * @deprecated\n * @see MyClass::$items\n */";
        assert_eq!(extract_see_references(doc), vec!["MyClass::$items"]);
    }

    #[test]
    fn see_references_function() {
        let doc = "/**\n * @deprecated\n * @see number_of()\n */";
        assert_eq!(extract_see_references(doc), vec!["number_of()"]);
    }

    #[test]
    fn see_references_url() {
        let doc = "/**\n * @see https://example.com/docs\n */";
        assert_eq!(
            extract_see_references(doc),
            vec!["https://example.com/docs"]
        );
    }

    #[test]
    fn see_references_with_description() {
        let doc = "/**\n * @see MyClass::setItems() To set the items.\n */";
        assert_eq!(
            extract_see_references(doc),
            vec!["MyClass::setItems() To set the items."]
        );
    }

    #[test]
    fn see_references_multiple() {
        let doc = "/**\n * @deprecated\n * @see number_of() Alias.\n * @see MyClass::$items For the property.\n * @see MyClass::setItems() To set items.\n */";
        let refs = extract_see_references(doc);
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0], "number_of() Alias.");
        assert_eq!(refs[1], "MyClass::$items For the property.");
        assert_eq!(refs[2], "MyClass::setItems() To set items.");
    }

    #[test]
    fn see_references_with_tab_separator() {
        let doc = "/**\n * @see\tMyClass\n */";
        assert_eq!(extract_see_references(doc), vec!["MyClass"]);
    }

    #[test]
    fn see_references_bare_see_tag_ignored() {
        // A bare @see with no reference text should not produce an entry.
        let doc = "/**\n * @see\n */";
        assert!(extract_see_references(doc).is_empty());
    }

    // ── extract_deprecation_with_see ────────────────────────────────

    #[test]
    fn deprecation_with_see_no_deprecated_tag() {
        let doc = "/**\n * @see NewClass\n * @return string\n */";
        assert_eq!(extract_deprecation_with_see(doc), None);
    }

    #[test]
    fn deprecation_with_see_no_see_tags() {
        let doc = "/** @deprecated Use foo() instead */";
        assert_eq!(
            extract_deprecation_with_see(doc),
            Some("Use foo() instead".to_string())
        );
    }

    #[test]
    fn deprecation_with_see_bare_deprecated_plus_see() {
        let doc = "/**\n * @deprecated\n * @see NewClass\n */";
        assert_eq!(
            extract_deprecation_with_see(doc),
            Some("See: NewClass".to_string())
        );
    }

    #[test]
    fn deprecation_with_see_message_plus_see() {
        let doc = "/**\n * @deprecated Use the new API.\n * @see NewClass::newMethod()\n */";
        assert_eq!(
            extract_deprecation_with_see(doc),
            Some("Use the new API. (see: NewClass::newMethod())".to_string())
        );
    }

    #[test]
    fn deprecation_with_see_message_plus_multiple_see() {
        let doc =
            "/**\n * @deprecated Old approach.\n * @see NewClass::foo()\n * @see OtherFunc()\n */";
        assert_eq!(
            extract_deprecation_with_see(doc),
            Some("Old approach. (see: NewClass::foo(), OtherFunc())".to_string())
        );
    }

    #[test]
    fn deprecation_with_see_bare_deprecated_plus_multiple_see() {
        let doc =
            "/**\n * @deprecated\n * @see NewClass\n * @see https://example.com/migration\n */";
        assert_eq!(
            extract_deprecation_with_see(doc),
            Some("See: NewClass, https://example.com/migration".to_string())
        );
    }

    #[test]
    fn deprecation_with_see_url_reference() {
        let doc =
            "/**\n * @deprecated\n * @see https://example.com/my/bar Documentation of Foo.\n */";
        assert_eq!(
            extract_deprecation_with_see(doc),
            Some("See: https://example.com/my/bar Documentation of Foo.".to_string())
        );
    }

    #[test]
    fn deprecation_with_see_doc_protocol_reference() {
        let doc = "/**\n * @deprecated\n * @see doc://getting-started/index Getting started.\n */";
        assert_eq!(
            extract_deprecation_with_see(doc),
            Some("See: doc://getting-started/index Getting started.".to_string())
        );
    }

    #[test]
    fn deprecation_with_see_realistic_phpdoc() {
        let doc = r#"/**
 * Count the items.
 *
 * @see number_of()                 Alias.
 * @see MyClass::$items             For the property whose items are counted.
 * @see MyClass::setItems()         To set the items for this collection.
 * @see https://example.com/my/bar  Documentation of Foo.
 *
 * @deprecated Use number_of() instead.
 * @return int Indicates the number of items.
 */"#;
        let result = extract_deprecation_with_see(doc).unwrap();
        assert!(result.starts_with("Use number_of() instead."));
        assert!(result.contains("number_of()"));
        assert!(result.contains("MyClass::$items"));
        assert!(result.contains("MyClass::setItems()"));
        assert!(result.contains("https://example.com/my/bar"));
    }

    // ── extract_removed_version ─────────────────────────────────────

    #[test]
    fn removed_tag_seven_zero() {
        let doc = "/** @removed 7.0 */";
        let version = extract_removed_version(doc).unwrap();
        assert_eq!(version.major, 7);
        assert_eq!(version.minor, 0);
    }

    #[test]
    fn removed_tag_eight_zero() {
        let doc = "/**\n * @removed 8.0\n */";
        let version = extract_removed_version(doc).unwrap();
        assert_eq!(version.major, 8);
        assert_eq!(version.minor, 0);
    }

    #[test]
    fn no_removed_tag() {
        let doc = "/** @return string */";
        assert_eq!(extract_removed_version(doc), None);
    }

    #[test]
    fn other_tags_but_no_removed() {
        let doc = "/**\n * @deprecated Use foo() instead.\n * @see NewClass\n * @return int\n */";
        assert_eq!(extract_removed_version(doc), None);
    }

    // ── extract_type_assertions (generic types) ─────────────────────

    #[test]
    fn assert_generic_type_with_spaces() {
        let doc = "/** @phpstan-assert Collection<int, User> $param */";
        let result = extract_type_assertions(doc);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].asserted_type.to_string(), "Collection<int, User>");
        assert_eq!(result[0].param_name, "$param");
        assert!(!result[0].negated);
    }

    #[test]
    fn assert_negated_generic_type() {
        let doc = "/** @phpstan-assert !Collection<int, User> $param */";
        let result = extract_type_assertions(doc);
        assert_eq!(result.len(), 1);
        assert!(result[0].negated);
        assert_eq!(result[0].asserted_type.to_string(), "Collection<int, User>");
    }

    #[test]
    fn assert_exact_type_prefix_is_stripped() {
        // PHPUnit's `assertInstanceOf` ships `@phpstan-assert =ExpectedType`.
        let doc = "/** @phpstan-assert =ExpectedType $actual */";
        let result = extract_type_assertions(doc);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].asserted_type.to_string(), "ExpectedType");
        assert_eq!(result[0].param_name, "$actual");
        assert!(!result[0].negated);
        assert!(result[0].is_equality);
    }

    #[test]
    fn assert_negated_exact_type_prefix() {
        // Both modifiers together, in either order, are stripped and the
        // negation is preserved.
        for doc in [
            "/** @phpstan-assert !=Foobar $actual */",
            "/** @phpstan-assert =!Foobar $actual */",
        ] {
            let result = extract_type_assertions(doc);
            assert_eq!(result.len(), 1, "doc: {doc}");
            assert_eq!(result[0].asserted_type.to_string(), "Foobar", "doc: {doc}");
            assert!(result[0].negated, "doc: {doc}");
            assert!(result[0].is_equality, "doc: {doc}");
        }
    }

    #[test]
    fn assert_without_equals_is_not_an_equality_assertion() {
        // The subtype form is the one that stays invertible into the
        // branch the tag does not name, so the two must not be conflated.
        for doc in [
            "/** @phpstan-assert Foobar $actual */",
            "/** @phpstan-assert !Foobar $actual */",
        ] {
            let result = extract_type_assertions(doc);
            assert_eq!(result.len(), 1, "doc: {doc}");
            assert!(!result[0].is_equality, "doc: {doc}");
        }
    }

    #[test]
    fn assert_equality_survives_a_union_asserted_type() {
        // Laravel's `filled()` / `blank()` pair, which is where the
        // equality form shows up in real code.
        let doc = "/** @phpstan-assert-if-true !=null|'' $value */";
        let result = extract_type_assertions(doc);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, AssertionKind::IfTrue);
        assert!(result[0].negated);
        assert!(result[0].is_equality);
    }

    // ── strip_html_tags ──────────────────────────────────────────────

    #[test]
    fn strip_html_tags_removes_inline_tags() {
        assert_eq!(
            strip_html_tags("<b>bold</b> <i>italic</i> <code>code</code>"),
            "bold italic code"
        );
    }

    #[test]
    fn strip_html_tags_strong_em_span() {
        assert_eq!(
            strip_html_tags("<strong>a</strong> <em>b</em> <span>c</span>"),
            "a b c"
        );
    }

    #[test]
    fn strip_html_tags_br_becomes_newline() {
        assert_eq!(strip_html_tags("a<br>b<br/>c<br />d"), "a\nb\nc\nd");
    }

    #[test]
    fn strip_html_tags_paragraph() {
        assert_eq!(strip_html_tags("first<p>second</p>"), "first\n\nsecond");
    }

    #[test]
    fn strip_html_tags_list_items_become_bullets() {
        assert_eq!(
            strip_html_tags("<ul><li>one</li><li>two</li></ul>"),
            "\n- one\n- two\n\n"
        );
    }

    #[test]
    fn strip_html_tags_ordered_list() {
        assert_eq!(
            strip_html_tags("<ol><li>first</li><li>second</li></ol>"),
            "\n- first\n- second\n\n"
        );
    }

    #[test]
    fn strip_html_tags_definition_list() {
        assert_eq!(
            strip_html_tags("<dl><dt>Term</dt><dd>Definition</dd></dl>"),
            "\n\nTerm\n  Definition\n"
        );
    }

    #[test]
    fn strip_html_tags_preserves_non_html_angle_brackets() {
        assert_eq!(strip_html_tags("a < b and c > d"), "a < b and c > d");
    }

    #[test]
    fn strip_html_tags_no_html() {
        let plain = "No HTML here.";
        assert_eq!(strip_html_tags(plain), plain);
    }
}
