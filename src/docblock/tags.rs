//! Core PHPDoc tag extraction.
//!
//! This submodule handles extracting type information from PHPDoc comments
//! (`/** ... */`), specifically `@return`, `@var`, `@param`, `@mixin`,
//! `@deprecated`, and `@phpstan-assert` / `@psalm-assert` tags.
//!
//! It also provides:
//!   - [`should_override_type_typed`]: compatibility check so that a docblock type
//!     only overrides a native type hint when the native hint is broad enough
//!     to be refined.
//!   - [`resolve_effective_type_typed`]: pick the best type between docblock and
//!     native hints.
//!   - [`get_docblock_text_for_node`]: extract raw docblock text from an AST
//!     node's preceding trivia.
//!
//! Template/generics/type-alias tags live in [`super::templates`].
//! Virtual member tags (`@property`, `@method`) live in
//! [`super::virtual_members`].

use std::borrow::Cow;

use super::tag_kind::TagKind;
use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::atom;
use crate::symbol_map::docblock::get_docblock_text_with_offset;
use crate::types::{AssertionKind, ParameterInfo, PhpVersion, TypeAssertion};

use super::parser::{
    DocblockInfo, TagInfo, TagValueInfo, collapse_newlines, parse_docblock_for_tags,
};
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
/// Returns a list of parsed [`PhpType`] values (leading `\` stripped).
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

        let cleaned = type_text.trim_start_matches('\\');
        if !cleaned.is_empty() {
            results.push(PhpType::parse(cleaned));
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

/// Search backward in `content` from `stmt_start` for an inline `/** @var … */`
/// docblock comment and extract the type (and optional variable name).
///
/// Only considers a docblock that is separated from the statement by
/// whitespace alone — no intervening code.
///
/// Returns `(cleaned_type, optional_var_name)` or `None`.
pub fn find_inline_var_docblock(
    content: &str,
    stmt_start: usize,
) -> Option<(PhpType, Option<String>)> {
    let before = content.get(..stmt_start)?;

    // Walk backward past whitespace / newlines.
    let trimmed = before.trim_end();
    if !trimmed.ends_with("*/") {
        return None;
    }

    // Find the matching `/**`.
    let block_end = trimmed.len();
    let open_pos = trimmed.rfind("/**")?;

    // Ensure nothing but whitespace between the start of the line and `/**`.
    let line_start = {
        let s = trimmed.get(..open_pos)?;
        s.rfind('\n').map_or(0, |p| p + 1)
    };
    let prefix = trimmed.get(line_start..open_pos)?;
    if !prefix.chars().all(|c| c.is_ascii_whitespace()) {
        return None;
    }

    let docblock = &trimmed[open_pos..block_end];
    extract_var_type_with_name(docblock)
}

/// Strip the docblock delimiters from a single trimmed source line so the
/// tag text it carries can be matched.
///
/// Each delimiter is optional and removed independently: `/**`, a trailing
/// `*/`, and the leading `*` of a continuation line.  Stripping them
/// independently is what makes a tag written on the same line as the
/// opening `/**` of a *multi-line* docblock readable — chaining
/// `strip_suffix("*/").unwrap_or(trimmed)` onto the prefix strip would
/// restore the `/**` whenever the line does not also close the block.
fn strip_docblock_line_delimiters(trimmed: &str) -> &str {
    let inner = trimmed.strip_prefix("/**").unwrap_or(trimmed);
    let inner = inner.strip_suffix("*/").unwrap_or(inner);
    inner.trim().trim_start_matches('*').trim()
}

/// Pull the inner text out of a `/** ... */` docblock that shares its
/// line with code on either side, e.g.
/// `$x = /** @param T $y */ static function (T $y) {`.
///
/// [`strip_docblock_line_delimiters`] only strips `/**`/`*/` when they
/// sit at the very start/end of the trimmed line, so a docblock preceded
/// or followed by other code on the same line is left untouched and its
/// tags never match. Returns `None` when the line has no `/** ... */`
/// span, or when the span *is* the whole line (that case is already
/// handled by `strip_docblock_line_delimiters`).
fn split_inline_docblock(trimmed: &str) -> Option<&str> {
    let open = trimmed.find("/**")?;
    let after_open = &trimmed[open + 3..];
    let close_rel = after_open.find("*/")?;
    let before = trimmed[..open].trim();
    let after = after_open[close_rel + 2..].trim();
    if before.is_empty() && after.is_empty() {
        return None;
    }
    Some(after_open[..close_rel].trim())
}

/// Search backward through `content` (up to `before_offset`) for any
/// `/** @var RawType $var_name */` annotation and return the **raw**
/// (uncleaned) type string — including generic parameters like `<User>`.
///
/// This is used by foreach element-type resolution: when iterating over
/// a variable annotated as `list<User>`, we need the raw `list<User>`
/// string so that the generic value type (`User`) can be extracted.
///
/// Only matches annotations that explicitly name the variable
/// (e.g. `/** @var list<User> $users */`).
pub fn find_var_raw_type_in_source(
    content: &str,
    before_offset: usize,
    var_name: &str,
) -> Option<PhpType> {
    let search_area = content.get(..before_offset)?;

    // Track brace depth so that annotations inside other function/method
    // bodies are not visible from the current scope.  When scanning
    // backward:
    //   `}` → entering a block above us → depth increases
    //   `{` → leaving that block        → depth decreases
    // Annotations found while `brace_depth > 0` belong to an inner
    // scope and must be skipped.  Once `min_depth` goes negative we
    // have exited our containing scope; if we then re-enter a block at
    // depth >= 0 we are inside a sibling scope (e.g. a different method
    // in the same class) and all further annotations are foreign.
    let mut brace_depth = 0i32;
    let mut min_depth = 0i32;

    for line in search_area.lines().rev() {
        let trimmed = line.trim();

        // Count braces on non-docblock lines to track scope depth.
        // Docblock lines are skipped because they may contain `{` / `}`
        // in array shape type annotations (e.g. `array{key: string}`).
        let is_comment_line =
            trimmed.starts_with('*') || trimmed.starts_with("/*") || trimmed.starts_with("//");

        if !is_comment_line {
            let (opens, closes) = count_braces_on_line(trimmed);
            // Going backward: `}` means entering a block, `{` means leaving.
            brace_depth += closes;
            brace_depth -= opens;
        }

        min_depth = min_depth.min(brace_depth);

        // Once we have exited our containing scope (min_depth < 0) and
        // re-entered a block close to that level, we are inside a
        // sibling scope (e.g. a different method in the same class) and
        // every remaining line is foreign, so stop rather than
        // brace-counting the rest of the file for an answer we would
        // discard.  The threshold is `min_depth` rather than `>= 0`
        // because the cursor may be inside a nested block (foreach,
        // if, etc.) whose extra depth prevents brace_depth from ever
        // reaching 0 when traversing sibling classes.
        if min_depth < 0 && brace_depth > min_depth {
            break;
        }

        // Skip annotations that belong to a deeper (inner) scope.
        if brace_depth > 0 {
            continue;
        }

        // Quick reject: must mention both `@var` and the variable.
        if !trimmed.contains("@var") || !trimmed.contains(var_name) {
            continue;
        }

        let inner = strip_docblock_line_delimiters(trimmed);

        if let Some(rest) = inner.strip_prefix("@var") {
            let rest = rest.trim_start();
            if rest.is_empty() {
                continue;
            }

            // Extract the full type token (respects `<…>` nesting).
            let (type_token, remainder) = split_type_token(rest);

            // The next token must be our variable name.
            if let Some(name) = remainder.split_whitespace().next()
                && name == var_name
            {
                return Some(PhpType::parse(type_token));
            }
        }
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
/// `@param-closure-this` binding, and extra `@param` tags naming a
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
) {
    for param in parameters.iter_mut() {
        let param_doc_type = extract_param_raw_type_from_info(info, &param.name);
        if let Some(ref doc_type) = param_doc_type {
            let effective = resolve_effective_type_typed(param.type_hint.as_ref(), Some(doc_type));
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
            let effective = resolve_effective_type_typed(param.type_hint.as_ref(), Some(doc_type));
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

/// Search backward through `content` (up to `before_offset`) for any
/// `@var` or `@param` annotation that assigns a raw (uncleaned) type to
/// `$var_name`.
///
/// This combines the logic of [`find_var_raw_type_in_source`] (which looks
/// for `@var Type $var`) and a backward scan for `@param Type $var` in
/// method/function docblocks.
///
/// Returns the first matching raw type string (including generic parameters
/// like `list<User>`), or `None` if no annotation is found.
pub fn find_iterable_raw_type_in_source(
    content: &str,
    before_offset: usize,
    var_name: &str,
) -> Option<PhpType> {
    let search_area = content.get(..before_offset)?;

    // Track brace depth so that annotations inside class/function bodies
    // are not visible from an outer scope.  When scanning backward:
    //   `}` → entering a block above us → depth increases
    //   `{` → leaving that block        → depth decreases
    // Annotations found while `brace_depth > 0` belong to an inner
    // scope and must be skipped.
    let mut brace_depth = 0i32;
    let mut min_depth = 0i32;
    let mut max_depth = 0i32;

    // Track the previous non-empty line we saw while scanning backward.
    // This lets us match `/** @var Type */` (no variable name) when the
    // *next* line is an assignment to our variable.
    let mut prev_non_empty_line: Option<&str> = None;

    for line in search_area.lines().rev() {
        let trimmed = line.trim();

        // Count braces on non-docblock lines to track scope depth.
        // Docblock lines are skipped because they may contain `{` / `}`
        // in array shape type annotations (e.g. `array{key: string}`).
        let is_comment_line =
            trimmed.starts_with('*') || trimmed.starts_with("/*") || trimmed.starts_with("//");

        let prev_min_depth = min_depth;

        if !is_comment_line {
            let (opens, closes) = count_braces_on_line(trimmed);
            // Going backward: `}` means entering a block, `{` means leaving.
            brace_depth += closes;
            brace_depth -= opens;
        }

        min_depth = min_depth.min(brace_depth);
        max_depth = max_depth.max(brace_depth);

        // Once we have exited our containing scope (min_depth < 0) and
        // re-entered a block close to that level, we are inside a
        // sibling scope (e.g. a different method in the same class) and
        // every remaining line is foreign, so stop rather than
        // brace-counting the rest of the file for an answer we would
        // discard.
        //
        // The threshold is `min_depth` rather than `>= 0` because
        // the cursor may be inside a nested block (foreach, if, etc.)
        // that adds extra depth.  When starting inside a foreach in a
        // class method, min_depth reaches -3 (foreach { + method { +
        // class {), so a sibling method body at depth -1 would never
        // reach 0.  Comparing against `min_depth` catches the first rise
        // back toward our exit point.
        if min_depth < 0 && brace_depth > min_depth {
            break;
        }

        // Detect sibling function/method boundaries at the same class
        // nesting level.  When scanning backward we may fully traverse
        // a sibling method body (entering at `}`, exiting at `{`) and
        // return to `brace_depth == 0` — the same level as our own
        // method signature.  The `min_depth < 0` check above only
        // fires when we cross the *enclosing* scope boundary (class
        // `{`), so it misses sibling methods entirely.
        //
        // Fix: once we have entered and fully exited a block at our
        // level (i.e. we saw `brace_depth > 0` and it returned to 0),
        // any `function` keyword at depth 0 is a sibling method
        // signature.  Its docblock belongs to that method, not ours.
        //
        // The `max_depth > 0` guard ensures we only trigger after
        // traversing at least one complete block.  Without it, a
        // scan starting from the parameter list `(` would flag the
        // function's own signature as a sibling boundary, hiding
        // the docblock directly above it.
        //
        // Neither of these two checks fires when a sibling's entire
        // body sits on one line (`function f() { ...; }` or a
        // bodyless `function f();`): its opens and closes cancel out
        // (or are both zero) within a single `count_braces_on_line`
        // call, so `brace_depth` never moves away from `min_depth` and
        // never rises above it either.  Detect that shape separately:
        // a non-comment line sitting at the already-established floor
        // (`brace_depth == min_depth`, unchanged from before this line
        // was processed) is a sibling, since the line that first
        // *carves out* that floor is our own enclosing signature, not
        // a sibling — that one is excluded by requiring `min_depth`
        // itself to be unchanged by this line.
        let at_established_floor =
            min_depth < 0 && brace_depth == min_depth && min_depth == prev_min_depth;

        if !is_comment_line && ((brace_depth == 0 && max_depth > 0) || at_established_floor) {
            // Check for a function/method keyword.  This covers:
            //   `public function foo(...)`, `private static function bar(...)`,
            //   `function baz(...)`, `public static function qux(): array`
            // We look for the `function` keyword as a word boundary.
            let lower = trimmed.to_ascii_lowercase();
            if lower.contains("function ")
                || lower.contains("function(")
                || lower.ends_with("function")
            {
                // We've hit a sibling function signature.  Any docblock
                // above this point belongs to that function, so stop.
                break;
            }
        }

        // Skip annotations that belong to a deeper (inner) scope.
        if brace_depth > 0 {
            if !trimmed.is_empty() {
                prev_non_empty_line = Some(trimmed);
            }
            continue;
        }

        // ── Named annotation: line mentions the variable name ───────
        // A docblock sharing its line with code (`$x = /** @param T $y
        // */ function ($y) {`) is matched against its own inner text so
        // the annotation is not swallowed by the surrounding code.
        let inline_annotation = split_inline_docblock(trimmed);
        let annotation_source = inline_annotation.unwrap_or(trimmed);
        if annotation_source.contains(var_name) {
            let inner = if inline_annotation.is_some() {
                annotation_source
            } else {
                strip_docblock_line_delimiters(annotation_source)
            };

            // Try @var first, then @param.
            let rest = if let Some(r) = inner.strip_prefix("@var") {
                Some(r)
            } else {
                inner.strip_prefix("@param")
            };

            if let Some(rest) = rest {
                let rest = rest.trim_start();
                if !rest.is_empty() {
                    // Extract the full type token (respects `<…>` nesting).
                    let (type_token, remainder) = split_type_token(rest);

                    // The next token must be our variable name.
                    if let Some(name) = remainder.split_whitespace().next()
                        && name == var_name
                    {
                        return Some(PhpType::parse(type_token));
                    }
                }
            }
        }

        // ── No-variable-name annotation: `/** @var Type */` ─────────
        // When the annotation has no variable name, check whether the
        // line immediately following it assigns to our target variable.
        // This handles the common pattern:
        //   /** @var array<int, Customer> */
        //   $thing = [];
        //   $thing[0]->
        if is_comment_line
            && trimmed.contains("@var")
            && let Some(next_line) = prev_non_empty_line
            && next_line.contains(var_name)
        {
            // Verify the next line is an assignment to the variable
            // (e.g. `$thing = …;` or `$thing;`).
            let next_trimmed = next_line.trim();
            if next_trimmed.starts_with(var_name)
                && next_trimmed[var_name.len()..].trim_start().starts_with('=')
            {
                let inner = strip_docblock_line_delimiters(trimmed);

                if let Some(rest) = inner.strip_prefix("@var") {
                    let rest = rest.trim_start();
                    if !rest.is_empty() {
                        let (type_token, remainder) = split_type_token(rest);

                        // Only match when there is no variable name in
                        // the annotation (otherwise the named check above
                        // would have matched already).
                        let has_var_name = remainder
                            .split_whitespace()
                            .next()
                            .is_some_and(|t| t.starts_with('$'));
                        if !has_var_name {
                            return Some(PhpType::parse(type_token));
                        }
                    }
                }
            }
        }

        if !trimmed.is_empty() {
            prev_non_empty_line = Some(trimmed);
        }
    }

    None
}

/// Find the `@return` type annotation of the enclosing function or method.
///
/// Scans backward from `cursor_offset` through `content`, crossing the
/// opening `{` of the enclosing function body, to locate the docblock
/// that immediately precedes the function/method declaration.  If a
/// `@return` tag is found, its type string is returned.
///
/// This is used inside generator bodies to reverse-infer variable types
/// from the declared `@return Generator<TKey, TValue, TSend, TReturn>`.
///
/// Returns `None` when no enclosing function docblock or `@return` tag
/// can be found.
pub fn find_enclosing_return_type(content: &str, cursor_offset: usize) -> Option<PhpType> {
    let search_area = content.get(..cursor_offset)?;

    // Walk backward, tracking brace depth.  We start inside a function
    // body (depth 0).  When we cross the opening `{` (depth goes to -1),
    // we have exited the function body and are in the function signature
    // region.  From there, look for the docblock above.
    let mut brace_depth = 0i32;

    // Find the byte offset of the opening `{` of the enclosing function.
    let mut func_open_brace: Option<usize> = None;
    for (i, ch) in search_area.char_indices().rev() {
        match ch {
            '}' => brace_depth += 1,
            '{' => {
                brace_depth -= 1;
                if brace_depth < 0 {
                    func_open_brace = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let brace_pos = func_open_brace?;

    // The region before the `{` should contain the function signature
    // and (optionally) the docblock above it.
    let before_brace = content.get(..brace_pos)?;

    // Find the `*/` that ends the docblock.  It must appear in the
    // region before the opening brace.  We search for the last `*/`
    // before the `function` keyword.
    //
    // First, locate the `function` keyword so we know where the
    // signature starts.
    let mut sig_start = before_brace.len().saturating_sub(2000);
    // Adjust to a valid UTF-8 char boundary so we don't panic on
    // multi-byte characters (e.g. `─` in comment banners).
    while sig_start > 0 && !before_brace.is_char_boundary(sig_start) {
        sig_start -= 1;
    }
    let sig_region = before_brace.get(sig_start..)?;
    let func_kw_rel = sig_region.rfind("function")?;
    let func_kw_pos = sig_start + func_kw_rel;

    // Everything before `function` (after trimming whitespace and
    // modifiers) should end with the docblock.
    let before_func = content.get(..func_kw_pos)?;

    // Scan backward over modifier keywords and whitespace.
    let trimmed = before_func.trim_end();
    let after_mods = crate::util::strip_trailing_modifiers(trimmed);

    if !after_mods.ends_with("*/") {
        return None;
    }

    let open_pos = after_mods.rfind("/**")?;
    let docblock = after_mods.get(open_pos..)?;

    extract_return_type(docblock)
}

// ─── Type Override Logic ────────────────────────────────────────────────────

/// Decide whether a docblock type should override a native type hint.
///
/// Returns `true` when the docblock type is likely to carry more
/// information than the native hint (e.g. `Collection<int, User>` vs
/// bare `object`), and `false` when overriding would lose precision
/// (e.g. both are scalars).
pub fn should_override_type_typed(docblock_type: &PhpType, native_type: &PhpType) -> bool {
    // If the docblock type is semantically equivalent to the native type
    // (handles `?X` ↔ `X|null`, reordered unions, FQN vs short names),
    // there is no value in overriding — the docblock doesn't carry any
    // extra information.
    if docblock_type.equivalent(native_type) {
        return false;
    }

    // Unwrap nullable wrappers for further analysis.  `?Foo` → `Foo`,
    // `Foo|null` → `Foo`.  For non-nullable types, use as-is.
    //
    // `non_null_type()` strips nullability from BOTH representations — the
    // `?Foo` (`Nullable`) form and the `Foo|null` (`Union` with a `null`
    // member) form.  Plain `unwrap_nullable()` only handled the former, so a
    // nullable-union native such as `object|null` reached the union branch
    // below with its `null` member still attached.  Since `object` and `null`
    // are both "scalar names", that branch then judged the whole type
    // unrefinable and discarded a generic docblock return like
    // `@psalm-return ?T`, leaving the bare native (`object|null`).
    let doc_owned = docblock_type.non_null_type();
    let doc_inner = doc_owned.as_ref().unwrap_or(docblock_type);
    let native_owned = native_type.non_null_type();
    let native_inner = native_owned.as_ref().unwrap_or(native_type);

    // If the docblock type is a bare, unparameterised primitive scalar
    // (`int`, `string`, `bool`, etc.), there's no value in overriding.
    // We intentionally exclude:
    //  - PHPDoc pseudo-types (`non-empty-string`, `class-string`,
    //    `positive-int`) — these are valid refinements.
    //  - Parameterised types (`array<int>`, `int<0, max>`) — these
    //    carry type information the native hint doesn't have.
    //  - Shapes, callables, slices — these also carry extra info.
    if doc_inner.is_bare_primitive_scalar() {
        return false;
    }

    // A type operator the parser could not evaluate — `key-of<TABLE>`,
    // `value-of<TABLE>`, `TABLE[K]` — reads one value out of a table or a
    // template, which no native hint can express: the declaration can only
    // spell the widest type the operator might produce (`int|string` for a
    // table holding both). It refines that hint however it evaluates, so the
    // docblock wins.
    if doc_inner.contains_unevaluated_operator() {
        return true;
    }

    // Produce a lowercased base name for the native type's inner part
    // `array`, `iterable`, `callable`, and `Closure` are broad types
    // that docblocks commonly refine (e.g. `array` → `list<User>`,
    // `iterable` → `Collection<int, Order>`,
    // `callable` → `callable(Task): void`).
    // `is_callable()` covers both `callable` and `Closure` (including `\Closure`).
    if native_inner.is_bare_array() || native_inner.is_iterable() || native_inner.is_callable() {
        return true;
    }

    // If the native type is a union or intersection, check each component.
    // A member is refinable when it is non-scalar (a class the docblock can
    // parameterise) or a broad type (`array`, `iterable`, `callable`,
    // `object`) that a docblock commonly refines. `is_scalar()` counts bare
    // `array` and `object` as scalar, so without the explicit container and
    // object checks a native union like `array|false` would wrongly reject a
    // `array<int, User>|false` docblock (dropping the element type), and
    // `object|string` would reject a `class-string<T>|T` docblock (as with
    // `new ReflectionClass($classString)`, dropping the template binding).
    match native_inner.kind() {
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            if members.iter().any(|m| {
                !m.is_scalar()
                    || m.is_bare_array()
                    || m.is_iterable()
                    || m.is_callable()
                    || m.is_object()
            }) {
                return true;
            }
            // An all-scalar native union can still be refined member by
            // member: `bool|string` → `false|string` is the same `bool` →
            // `false` refinement a lone native `bool` already accepts
            // below, and without it the literal `false` the `!== false`
            // idiom narrows away never reaches the type at all.
            return refines_native_union(doc_inner, members);
        }
        _ => {}
    }

    // If the native type is a narrow scalar (not a broad container
    // handled above), only allow override when the docblock type is a
    // *compatible refinement*.  For example `string` → `class-string<Foo>`
    // is valid, but `string` → `array<int>` is not.
    if native_inner.is_scalar() {
        return is_compatible_refinement_typed(doc_inner, native_inner);
    }

    // If the docblock type carries generic parameters or shape braces,
    // it is refining the class with extra type info — allow it.
    if has_parameterisation(doc_inner) {
        return true;
    }

    // PHPDoc pseudo-types like `class-string`, `non-empty-string`,
    // `positive-int`, `literal-string`, etc. refine their native
    // scalar counterparts.  These contain hyphens which never appear
    // in native PHP types.
    if extract_base_name_lower(doc_inner).contains('-') {
        return true;
    }

    // Native type is a non-scalar class — docblock can always refine.
    true
}

/// Whether a docblock type refines the members of the native union it is
/// written on.
///
/// Every member the docblock names has to line up with a native member it
/// narrows or restates, which is what separates `bool|string` →
/// `false|string` (each member accounted for) from `bool|string` →
/// `array<int>` (a docblock describing something else entirely, where the
/// native hint is the more trustworthy of the two).
fn refines_native_union(doc_type: &PhpType, native_members: &[PhpType]) -> bool {
    let doc_members: &[PhpType] = match doc_type.kind() {
        TypeKind::Union(members) | TypeKind::Intersection(members) => members,
        _ => std::slice::from_ref(doc_type),
    };
    doc_members.iter().all(|doc| {
        native_members
            .iter()
            .any(|native| doc.equivalent(native) || is_compatible_refinement_typed(doc, native))
    })
}

/// Check whether a `PhpType` has generic parameters or shape braces.
fn has_parameterisation(ty: &PhpType) -> bool {
    matches!(
        ty.kind(),
        TypeKind::Generic(_) | TypeKind::ArrayShape(_) | TypeKind::ObjectShape(_)
    )
}

/// Check whether a docblock type is a compatible refinement of a native
/// type.  Both parameters should be stripped of nullable wrappers before
/// calling.
///
/// A refinement is compatible when the docblock's base type narrows the
/// native type without changing its fundamental kind.  For example:
/// - `string` → `class-string<Foo>` (compatible: refines string)
/// - `string` → `non-empty-string` (compatible: refines string)
/// - `int` → `positive-int` (compatible: refines int)
/// - `array` → `list<User>` (compatible: refines array)
/// - `object` → `callable-object` (compatible: refines object)
/// - `string` → `array<int>` (incompatible: completely different type)
/// - `int` → `Collection<User>` (incompatible: completely different type)
///
/// This is the single source of truth for refinement compatibility and
/// is used by both `should_override_type` and the update-docblock
/// contradiction checker.
///
/// Accepts a pre-parsed [`PhpType`] to avoid a parse-stringify-reparse
/// round-trip.  Extracts the outermost type name from the docblock type,
/// stripping generic parameters, shape braces, and callable signatures.
/// Unlike `base_name()` this includes scalar names (`array`, `int`, ...)
/// which are needed for the refinement checks.
pub(crate) fn is_compatible_refinement_typed(doc_type: &PhpType, native_type: &PhpType) -> bool {
    if native_type.is_string_type() {
        return doc_type.is_string_subtype();
    }
    if native_type.is_int() {
        return doc_type.is_int_subtype();
    }
    if native_type.is_float() {
        return doc_type.is_float_subtype();
    }
    if native_type.is_bool() {
        return doc_type.is_bool() || doc_type.is_true() || doc_type.is_false();
    }
    if native_type.is_bare_array() {
        return doc_type.is_array_like() || doc_type.is_iterable();
    }
    if native_type.is_mixed() {
        return true;
    }
    if native_type.is_object() {
        return !doc_type.is_scalar() || matches!(doc_type.kind(), TypeKind::ObjectShape(_));
    }
    if native_type.is_self_like() {
        // `static` and `$this` are valid refinements of `self` — they
        // carry late-static-binding semantics the native `self` lacks.
        if doc_type.is_self_like() {
            return true;
        }
        return !doc_type.is_scalar();
    }
    if native_type.is_void()
        || native_type.is_never()
        || native_type.is_null()
        || native_type.is_true()
        || native_type.is_false()
    {
        return false;
    }
    if native_type.is_callable() {
        return true;
    }

    if native_type.is_iterable() {
        return true;
    }
    if native_type.is_closure() {
        return true;
    }
    if native_type.is_resource() {
        return doc_type.is_resource();
    }
    false
}

/// Extract the outermost type name from a `PhpType` as a lowercased string.
///
/// Strips generic parameters, shape braces, callable signatures, and
/// nullable wrappers.  Returns the base identifier lowercased (e.g.
/// `Generic("Collection", _)` → `"collection"`, `Named("int")` → `"int"`).
///
/// For complex types without a simple base name (e.g. unions, callables,
/// shapes), returns an empty string.
///
/// Used by `should_override_type_typed` for its hyphen-based pseudo-type
/// heuristic.
fn extract_base_name_lower(ty: &PhpType) -> String {
    ty.base_name()
        .map(|n| n.to_ascii_lowercase())
        .unwrap_or_default()
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

// ─── Effective Type Resolution ──────────────────────────────────────────────

/// Parse a raw docblock type string into a [`PhpType`], applying
/// unclosed-bracket recovery when necessary.
///
/// Raw docblock strings from `extract_return_type`, `extract_var_type`,
/// `extract_param_raw_type`, etc. may contain malformed type expressions
/// (e.g. `"static<"`, `"Collection<int"`) when multi-line annotations
/// couldn't be fully joined.  This helper recovers the base type in such
/// cases, matching the sanitisation logic in [`resolve_effective_type_typed`].
///
/// Returns `None` when the string is completely unrecoverable (e.g.
/// `"<garbage"` with no base type).
pub fn sanitise_and_parse_docblock_type(raw: &str) -> Option<PhpType> {
    if crate::docblock::type_strings::has_unclosed_delimiters(raw) {
        let base = recover_base_type(raw);
        if base.is_empty() {
            None
        } else {
            Some(PhpType::parse(base))
        }
    } else {
        Some(PhpType::parse(raw))
    }
}

/// Pick the best available type between a native type hint and a docblock
/// annotation, returning a parsed [`PhpType`].
///
/// When both are present, the docblock type is used only if
/// [`should_override_type_typed`] approves (i.e. the native hint is broad
/// enough to refine).
///
/// This function does not perform unclosed-bracket recovery; callers with
/// raw docblock strings should use [`sanitise_and_parse_docblock_type`]
/// first.
pub fn resolve_effective_type_typed(
    native_type: Option<&PhpType>,
    docblock_type: Option<&PhpType>,
) -> Option<PhpType> {
    match (native_type, docblock_type) {
        // Docblock provided, no native hint → use docblock.
        (None, Some(doc)) => Some(doc.clone()),
        // Both present → override only if compatible.
        (Some(native), Some(doc)) => {
            if should_override_type_typed(doc, native) {
                // Preserve nullability from the native hint. A `?array`
                // native with a non-nullable `@param Foo[]` docblock still
                // accepts null at runtime, so the effective type is
                // `Foo[]|null`. This mirrors PHPStan's `decideType`.
                //
                // `mixed` is excluded: it accepts null but carries no
                // explicit null member, so narrowing it through a docblock
                // is intentional and must not re-add null.
                if native.accepts_null() && !native.is_mixed() {
                    Some(doc.clone().or_null())
                } else {
                    Some(doc.clone())
                }
            } else {
                Some(native.clone())
            }
        }
        // Native only → keep it.
        (Some(native), None) => Some(native.clone()),
        // Neither → nothing.
        (None, None) => None,
    }
}

// ─── Internals ──────────────────────────────────────────────────────────────

/// Count `{` and `}` characters on a line, skipping those inside string
/// literals.  Returns `(open_count, close_count)`.
fn count_braces_on_line(line: &str) -> (i32, i32) {
    let mut opens = 0i32;
    let mut closes = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut prev = '\0';

    for ch in line.chars() {
        if in_single_quote {
            if ch == '\'' && prev != '\\' {
                in_single_quote = false;
            }
            prev = ch;
            continue;
        }
        if in_double_quote {
            if ch == '"' && prev != '\\' {
                in_double_quote = false;
            }
            prev = ch;
            continue;
        }
        match ch {
            '\'' => in_single_quote = true,
            '"' => in_double_quote = true,
            '{' => opens += 1,
            '}' => closes += 1,
            _ => {}
        }
        prev = ch;
    }

    (opens, closes)
}

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

    // ── find_var_raw_type_in_source — scope isolation ───────────────

    #[test]
    fn var_docblock_does_not_leak_across_sibling_methods() {
        // A `@var` in one class method must not be visible in another.
        let src = concat!(
            "<?php\n",
            "class A {\n",
            "    public function first(): void {\n",
            "        /** @var object{title: string} $item */\n",
            "        $item = foo();\n",
            "    }\n",
            "}\n",
            "class B {\n",
            "    public function second(): void {\n",
            "        $item->\n", // cursor here
            "    }\n",
            "}\n",
        );
        let cursor = src.find("$item->").unwrap();
        let result = find_var_raw_type_in_source(src, cursor, "$item");
        assert_eq!(
            result, None,
            "@var from A::first() must not leak into B::second()"
        );
    }

    #[test]
    fn var_docblock_does_not_leak_across_sibling_methods_same_class() {
        let src = concat!(
            "<?php\n",
            "class Demo {\n",
            "    public function first(): void {\n",
            "        /** @var Pen $item */\n",
            "        $item = foo();\n",
            "    }\n",
            "    public function second(): void {\n",
            "        $item->\n",
            "    }\n",
            "}\n",
        );
        let cursor = src.find("$item->").unwrap();
        let result = find_var_raw_type_in_source(src, cursor, "$item");
        assert_eq!(
            result, None,
            "@var from first() must not leak into second()"
        );
    }

    #[test]
    fn var_docblock_does_not_leak_when_cursor_inside_nested_block() {
        // The original bug: when the cursor is inside a foreach (or if,
        // while, etc.), the extra nesting depth prevented the sibling
        // scope detection from firing, allowing @var from a method in a
        // completely different class to leak through.
        let src = concat!(
            "<?php\n",
            "class ObjectShapeDemo {\n",
            "    public function demo(): void {\n",
            "        /** @var object{title: string, score: float} $item */\n",
            "        $item = getUnknownValue();\n",
            "    }\n",
            "}\n",
            "class Other {\n",
            "    public function demo(): void {\n",
            "        foreach ($things as $item) {\n",
            "            $item->\n",
            "        }\n",
            "    }\n",
            "}\n",
        );
        let cursor = src.find("$item->").unwrap();
        let result = find_var_raw_type_in_source(src, cursor, "$item");
        assert_eq!(
            result, None,
            "@var from ObjectShapeDemo must not leak into Other when cursor is inside foreach"
        );
    }

    #[test]
    fn var_docblock_found_in_own_scope() {
        let src = concat!(
            "<?php\n",
            "class Demo {\n",
            "    public function demo(): void {\n",
            "        /** @var Pen $item */\n",
            "        $item = foo();\n",
            "        $item->\n",
            "    }\n",
            "}\n",
        );
        let cursor = src.find("$item->").unwrap();
        let result = find_var_raw_type_in_source(src, cursor, "$item");
        assert_eq!(
            result.as_ref().map(|t| t.to_string()),
            Some("Pen".to_string())
        );
    }

    #[test]
    fn var_docblock_found_inside_nested_block_in_own_scope() {
        let src = concat!(
            "<?php\n",
            "class Demo {\n",
            "    public function demo(): void {\n",
            "        /** @var Pen $item */\n",
            "        $item = foo();\n",
            "        if (true) {\n",
            "            $item->\n",
            "        }\n",
            "    }\n",
            "}\n",
        );
        let cursor = src.find("$item->").unwrap();
        let result = find_var_raw_type_in_source(src, cursor, "$item");
        assert_eq!(
            result.as_ref().map(|t| t.to_string()),
            Some("Pen".to_string())
        );
    }

    // ── find_iterable_raw_type_in_source — scope isolation ──────────

    #[test]
    fn iterable_docblock_does_not_leak_across_sibling_classes_nested() {
        // Same bug scenario for find_iterable_raw_type_in_source:
        // @var in a sibling class method leaks when cursor is nested.
        let src = concat!(
            "<?php\n",
            "class A {\n",
            "    public function demo(): void {\n",
            "        /** @var object{title: string} $item */\n",
            "        $item = foo();\n",
            "    }\n",
            "}\n",
            "class B {\n",
            "    public function demo(): void {\n",
            "        foreach ($things as $x) {\n",
            "            $item->\n",
            "        }\n",
            "    }\n",
            "}\n",
        );
        let cursor = src.find("$item->").unwrap();
        let result = find_iterable_raw_type_in_source(src, cursor, "$item");
        assert_eq!(
            result, None,
            "@var from A::demo() must not leak into B::demo() foreach body"
        );
    }

    #[test]
    fn iterable_param_found_in_own_method_from_nested_block() {
        // @param in the enclosing method's docblock must still be found
        // even when the cursor is inside a nested block.
        let src = concat!(
            "<?php\n",
            "class Demo {\n",
            "    /**\n",
            "     * @param list<Pen> $items\n",
            "     */\n",
            "    public function demo(array $items): void {\n",
            "        foreach ($items as $x) {\n",
            "            // cursor\n",
            "        }\n",
            "    }\n",
            "}\n",
        );
        let cursor = src.find("// cursor").unwrap();
        let result = find_iterable_raw_type_in_source(src, cursor, "$items");
        assert_eq!(
            result.as_ref().map(|t| t.to_string()),
            Some("list<Pen>".to_string())
        );
    }

    #[test]
    fn iterable_param_found_on_docblock_opening_line() {
        // The first tag of a multi-line docblock may share the opening
        // `/**` line.  It must be read just like a continuation line.
        let src = concat!(
            "<?php\n",
            "class Demo {\n",
            "    /** @param list<Pen> $items\n",
            "     *  @return void */\n",
            "    public function demo(array $items): void {\n",
            "        foreach ($items as $x) {\n",
            "            // cursor\n",
            "        }\n",
            "    }\n",
            "}\n",
        );
        let cursor = src.find("// cursor").unwrap();
        let result = find_iterable_raw_type_in_source(src, cursor, "$items");
        assert_eq!(
            result.as_ref().map(|t| t.to_string()),
            Some("list<Pen>".to_string())
        );
    }

    #[test]
    fn iterable_var_found_on_docblock_opening_line() {
        let src = concat!(
            "<?php\n",
            "function demo(): void {\n",
            "    /** @var list<Pen> $items\n",
            "     *  a trailing description */\n",
            "    $items = foo();\n",
            "    foreach ($items as $x) {\n",
            "        // cursor\n",
            "    }\n",
            "}\n",
        );
        let cursor = src.find("// cursor").unwrap();
        assert_eq!(
            find_iterable_raw_type_in_source(src, cursor, "$items")
                .as_ref()
                .map(|t| t.to_string()),
            Some("list<Pen>".to_string())
        );
        assert_eq!(
            find_var_raw_type_in_source(src, cursor, "$items")
                .as_ref()
                .map(|t| t.to_string()),
            Some("list<Pen>".to_string())
        );
    }

    #[test]
    fn docblock_line_delimiters_stripped_independently() {
        assert_eq!(
            strip_docblock_line_delimiters("/** @param string $a */"),
            "@param string $a"
        );
        assert_eq!(
            strip_docblock_line_delimiters("/** @param string $a"),
            "@param string $a"
        );
        assert_eq!(
            strip_docblock_line_delimiters("* @param string $a */"),
            "@param string $a"
        );
        assert_eq!(
            strip_docblock_line_delimiters("*  @param string $a"),
            "@param string $a"
        );
        assert_eq!(strip_docblock_line_delimiters("*/"), "");
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

    #[test]
    fn iterable_var_found_in_own_scope_nested() {
        let src = concat!(
            "<?php\n",
            "class Demo {\n",
            "    public function demo(): void {\n",
            "        /** @var list<Pen> $items */\n",
            "        $items = foo();\n",
            "        foreach ($items as $x) {\n",
            "            // cursor\n",
            "        }\n",
            "    }\n",
            "}\n",
        );
        let cursor = src.find("// cursor").unwrap();
        let result = find_iterable_raw_type_in_source(src, cursor, "$items");
        assert_eq!(
            result.as_ref().map(|t| t.to_string()),
            Some("list<Pen>".to_string())
        );
    }

    #[test]
    fn iterable_docblock_does_not_leak_across_one_line_sibling_function() {
        // A sibling function whose entire body fits on one line opens and
        // closes its braces within the same `count_braces_on_line` call,
        // so brace_depth never moves.  Its own `@param` must not leak into
        // the next function just because it shares a parameter name.
        let src = concat!(
            "<?php\n",
            "/** @param array<Status> $s */\n",
            "function g3(array $s): void { foreach ($s as $x) { doThing($x); } }\n",
            "\n",
            "function g4(Status $s): void { doThing($s); }\n",
        );
        let cursor = src.find("doThing($s)").unwrap();
        let result = find_iterable_raw_type_in_source(src, cursor, "$s");
        assert_eq!(
            result, None,
            "@param from one-line g3() must not leak into g4()"
        );
    }

    #[test]
    fn iterable_docblock_does_not_leak_across_one_line_sibling_method() {
        let src = concat!(
            "<?php\n",
            "class Demo {\n",
            "    /** @param array<Status> $s */\n",
            "    public function g3(array $s): void { foreach ($s as $x) { doThing($x); } }\n",
            "\n",
            "    public function g4(Status $s): void { doThing($s); }\n",
            "}\n",
        );
        let cursor = src.find("doThing($s)").unwrap();
        let result = find_iterable_raw_type_in_source(src, cursor, "$s");
        assert_eq!(
            result, None,
            "@param from one-line g3() must not leak into g4()"
        );
    }
}
