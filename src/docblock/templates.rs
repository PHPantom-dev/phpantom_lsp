//! Template, generics, and type alias tag extraction.
//!
//! This submodule handles `@template` (including `-covariant` /
//! `-contravariant` variants), `@extends` / `@implements` / `@use`
//! generic binding tags, `@phpstan-type` / `@psalm-type` local type
//! aliases, `@phpstan-import-type` / `@psalm-import-type` imported
//! aliases, and `class-string<T>` conditional return type synthesis.

use std::collections::HashMap;

use mago_docblock::document::TagKind;

use super::parser::{DocblockInfo, collapse_newlines, parse_docblock_for_tags};
use super::types::split_type_token;
use crate::php_type::PhpType;
use crate::types::{TemplateVariance, TypeAliasDef};
use crate::util::strip_fqn_prefix;

// ─── Template Parameters ────────────────────────────────────────────────────

/// Extract template parameter names from `@template` tags in a docblock.
///
/// Handles the common PHPStan / Psalm variants:
///   - `@template T`
///   - `@template TKey of array-key`
///   - `@template-covariant TValue`
///   - `@template-contravariant TValue`
///   - `@phpstan-template T`
///   - `@phpstan-template-covariant TValue`
///
/// Returns a list of template parameter names (e.g. `["T", "TKey"]`).
pub fn extract_template_params(docblock: &str) -> Vec<String> {
    extract_template_params_full(docblock)
        .into_iter()
        .map(|(name, _, _, _)| name)
        .collect()
}

/// Like [`extract_template_params`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_template_params_from_info(info: &DocblockInfo) -> Vec<String> {
    extract_template_params_full_from_info(info)
        .into_iter()
        .map(|(name, _, _, _)| name)
        .collect()
}

/// Extract template parameter names **and** their optional upper bounds
/// from `@template` tags in a docblock.
///
/// The bound is the type after the `of` keyword, e.g.:
///   - `@template T` → `("T", None)`
///   - `@template TNode of PDependNode` → `("TNode", Some("PDependNode"))`
///   - `@template-covariant TValue of Stringable` → `("TValue", Some("Stringable"))`
///
/// Returns a list of `(name, optional_bound)` pairs.
pub fn extract_template_params_with_bounds(docblock: &str) -> Vec<(String, Option<PhpType>)> {
    extract_template_params_full(docblock)
        .into_iter()
        .map(|(name, bound, _, _)| (name, bound))
        .collect()
}

/// Like [`extract_template_params_with_bounds`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_template_params_with_bounds_from_info(
    info: &DocblockInfo,
) -> Vec<(String, Option<PhpType>)> {
    extract_template_params_full_from_info(info)
        .into_iter()
        .map(|(name, bound, _, _)| (name, bound))
        .collect()
}

/// Extract template parameter names, optional upper bounds, **and** variance
/// from `@template` tags in a docblock.
///
/// Returns a list of `(name, optional_bound, variance)` tuples:
///   - `@template T` → `("T", None, Invariant)`
///   - `@template TNode of PDependNode` → `("TNode", Some("PDependNode"), Invariant)`
///   - `@template-covariant TValue` → `("TValue", None, Covariant)`
///   - `@template-contravariant TInput of Foo` → `("TInput", Some("Foo"), Contravariant)`
pub fn extract_template_params_full(
    docblock: &str,
) -> Vec<(String, Option<PhpType>, TemplateVariance, Option<PhpType>)> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };
    extract_template_params_full_from_info(&info)
}

/// Map a `TagKind` to the corresponding `TemplateVariance`.
pub(crate) const fn variance_for(kind: TagKind) -> TemplateVariance {
    match kind {
        TagKind::TemplateCovariant
        | TagKind::PhpstanTemplateCovariant
        | TagKind::PsalmTemplateCovariant => TemplateVariance::Covariant,
        TagKind::TemplateContravariant
        | TagKind::PhpstanTemplateContravariant
        | TagKind::PsalmTemplateContravariant => TemplateVariance::Contravariant,
        _ => TemplateVariance::Invariant,
    }
}

/// `TagKind` values that represent `@template` declarations (all variance variants).
pub(crate) const TEMPLATE_KINDS: &[TagKind] = &[
    TagKind::Template,
    TagKind::TemplateCovariant,
    TagKind::TemplateContravariant,
    TagKind::PhpstanTemplate,
    TagKind::PhpstanTemplateCovariant,
    TagKind::PhpstanTemplateContravariant,
    TagKind::PsalmTemplate,
    TagKind::PsalmTemplateCovariant,
    TagKind::PsalmTemplateContravariant,
];

/// Like [`extract_template_params_full`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_template_params_full_from_info(
    info: &DocblockInfo,
) -> Vec<(String, Option<PhpType>, TemplateVariance, Option<PhpType>)> {
    let mut results = Vec::new();

    for tag in info.tags_by_kinds(TEMPLATE_KINDS) {
        let desc = tag.description.trim();
        if desc.is_empty() {
            continue;
        }

        let variance = variance_for(tag.kind);

        // The template parameter name is the first whitespace-delimited token.
        let mut tokens = desc.split_whitespace();
        if let Some(name) = tokens.next() {
            // Sanity: template names are identifiers (start with a letter or _).
            if name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            {
                // Everything after the parameter name, used for
                // `split_type_token`-based parsing that respects `<>` nesting.
                let rest = desc[name.len()..].trim_start();

                // Check for a bound keyword: `@template T of SomeClass` or `@template T as SomeClass`
                let (bound, rest_after_bound) = if let Some(after_of) = rest
                    .strip_prefix("of")
                    .and_then(|s| s.strip_prefix(|c: char| c.is_whitespace()))
                    .or_else(|| {
                        rest.strip_prefix("as")
                            .and_then(|s| s.strip_prefix(|c: char| c.is_whitespace()))
                    }) {
                    let after_of = after_of.trim_start();
                    if after_of.is_empty() {
                        (None, "")
                    } else {
                        let (type_tok, remainder) = split_type_token(after_of);
                        if type_tok.is_empty() {
                            (None, remainder)
                        } else {
                            (Some(PhpType::parse(type_tok)), remainder)
                        }
                    }
                } else {
                    (None, rest)
                };

                // Check for a `= default` value: `@template T of bool = false`
                let rest_trimmed = rest_after_bound.trim_start();
                let default = if let Some(after_eq) = rest_trimmed.strip_prefix('=') {
                    let after_eq = after_eq.trim_start();
                    if after_eq.is_empty() {
                        None
                    } else {
                        let (default_tok, _) = split_type_token(after_eq);
                        if default_tok.is_empty() {
                            None
                        } else {
                            Some(PhpType::parse(default_tok))
                        }
                    }
                } else {
                    None
                };

                results.push((name.to_string(), bound, variance, default));
            }
        }
    }

    results
}

// ─── Template Parameter Bindings ────────────────────────────────────────────

/// Extract `@param` tags that bind a template parameter to a function
/// parameter.
///
/// Given a list of known `template_params` (e.g. `["T"]`), scans the
/// docblock for `@param T $varName` (or `@param ?T $varName`,
/// `@param T|null $varName`) and returns `(template_name, "$varName")`
/// pairs.
pub fn extract_template_param_bindings(
    docblock: &str,
    template_params: &[String],
) -> Vec<(String, String)> {
    if template_params.is_empty() {
        return Vec::new();
    }

    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };

    extract_template_param_bindings_from_info(&info, template_params)
}

/// Like [`extract_template_param_bindings`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_template_param_bindings_from_info(
    info: &DocblockInfo,
    template_params: &[String],
) -> Vec<(String, String)> {
    if template_params.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();

    for tag in info.tags_by_kinds(&[TagKind::PhpstanParam, TagKind::PsalmParam, TagKind::Param]) {
        let desc = tag.description.trim();
        if desc.is_empty() {
            continue;
        }

        // Extract the full type token (respects `<…>` nesting).
        let (type_token, remainder) = split_type_token(desc);

        // The next token should be the parameter name (e.g. `$model`).
        // It may have a variadic prefix: `...$items`.
        let param_name = match remainder.split_whitespace().next() {
            Some(name) if name.starts_with('$') => name,
            Some(name) if name.starts_with("...$") => &name[3..],
            _ => continue,
        };

        // Parse the type token into a PhpType tree and walk it to find
        // all template parameter references, correctly handling nested
        // generics like `Wrapper<Collection<T>, V>`.
        let parsed = PhpType::parse(type_token);
        collect_template_bindings(&parsed, template_params, param_name, &mut results);
    }

    results
}

// ─── Generics Tags (@extends, @implements, @use) ────────────────────────────

/// Extract generic type arguments from `@extends`, `@implements`, or `@use`
/// tags (and their `@phpstan-` prefixed variants) in a docblock.
///
/// The `tag` parameter should be one of `"@extends"`, `"@implements"`, or
/// `"@use"`.
///
/// For example, given `@extends Collection<int, Language>`, returns
/// `[("Collection", ["int", "Language"])]`.
///
/// Handles:
///   - `@extends Collection<int, Language>`
///   - `@phpstan-extends Collection<int, Language>`
///   - `@implements ArrayAccess<string, User>`
///   - Nested generics: `@extends Base<array<int, string>, User>`
pub fn extract_generics_tag(docblock: &str, tag: &str) -> Vec<(String, Vec<PhpType>)> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };

    extract_generics_tag_from_info(&info, tag)
}

/// Recursively walk a [`PhpType`] tree and collect `(template_name, param_name)` pairs
/// for every template parameter reference found anywhere in the type.
pub(crate) fn collect_template_bindings(
    ty: &PhpType,
    template_params: &[String],
    param_name: &str,
    results: &mut Vec<(String, String)>,
) {
    match ty {
        PhpType::Named(name) => {
            if let Some(t) = template_params.iter().find(|t| t.as_str() == name) {
                results.push((t.to_string(), param_name.to_string()));
            }
        }
        PhpType::Nullable(inner) => {
            collect_template_bindings(inner, template_params, param_name, results);
        }
        PhpType::Union(members) | PhpType::Intersection(members) => {
            for member in members {
                collect_template_bindings(member, template_params, param_name, results);
            }
        }
        PhpType::Array(inner) => {
            collect_template_bindings(inner, template_params, param_name, results);
        }
        PhpType::Generic(_, args) => {
            for arg in args {
                collect_template_bindings(arg, template_params, param_name, results);
            }
        }
        PhpType::ClassString(Some(inner))
        | PhpType::InterfaceString(Some(inner))
        | PhpType::KeyOf(inner)
        | PhpType::ValueOf(inner) => {
            collect_template_bindings(inner, template_params, param_name, results);
        }
        PhpType::Callable {
            params,
            return_type,
            ..
        } => {
            for p in params {
                collect_template_bindings(&p.type_hint, template_params, param_name, results);
            }
            if let Some(rt) = return_type {
                collect_template_bindings(rt, template_params, param_name, results);
            }
        }
        PhpType::ArrayShape(entries) | PhpType::ObjectShape(entries) => {
            for entry in entries {
                collect_template_bindings(&entry.value_type, template_params, param_name, results);
            }
        }
        PhpType::IndexAccess(target, index) => {
            collect_template_bindings(target, template_params, param_name, results);
            collect_template_bindings(index, template_params, param_name, results);
        }
        PhpType::Conditional {
            condition,
            then_type,
            else_type,
            ..
        } => {
            collect_template_bindings(condition, template_params, param_name, results);
            collect_template_bindings(then_type, template_params, param_name, results);
            collect_template_bindings(else_type, template_params, param_name, results);
        }
        _ => {}
    }
}

/// Like [`extract_generics_tag`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_generics_tag_from_info(
    info: &DocblockInfo,
    tag: &str,
) -> Vec<(String, Vec<PhpType>)> {
    // Map the tag string to the corresponding TagKinds.
    // For `@extends` we also accept `@phpstan-extends` and `@template-extends`.
    // Note: `@phpstan-extends`, `@phpstan-implements`, and `@phpstan-use`
    // are classified as `TagKind::Other` by mago-docblock, so we also
    // need to match them by tag name.
    let bare_tag = tag.strip_prefix('@').unwrap_or(tag);
    let (kinds, name_fallbacks): (Vec<TagKind>, Vec<&str>) = match bare_tag {
        "extends" => (
            vec![TagKind::Extends, TagKind::TemplateExtends],
            vec!["phpstan-extends"],
        ),
        "implements" => (
            vec![TagKind::Implements, TagKind::TemplateImplements],
            vec!["phpstan-implements"],
        ),
        "use" => (
            vec![TagKind::Use, TagKind::TemplateUse],
            vec!["phpstan-use"],
        ),
        _ => (vec![], vec![bare_tag]),
    };

    let mut results = Vec::new();

    // Match by TagKind first.
    for tag in info.tags_by_kinds(&kinds) {
        if let Some(result) = parse_generics_from_description(&tag.description) {
            results.push(result);
        }
    }

    // Also match by tag name for variants that mago-docblock classifies
    // as `TagKind::Other` (e.g. `@phpstan-extends`).
    for tag in &info.tags {
        if name_fallbacks.contains(&tag.name.as_str())
            && tag.kind == TagKind::Other
            && let Some(result) = parse_generics_from_description(&tag.description)
        {
            results.push(result);
        }
    }

    results
}

/// Parse a generics tag description (e.g. `"Collection<int, Language>"`) into
/// a `(base_name, generic_args)` tuple.
fn parse_generics_from_description(desc: &str) -> Option<(String, Vec<PhpType>)> {
    let desc = desc.trim();
    if desc.is_empty() {
        return None;
    }

    // mago-docblock joins multi-line descriptions with \n; normalise.
    let normalised = collapse_newlines(desc);

    // Extract the full type token (e.g. `Collection<int, Language>`),
    // respecting `<…>` nesting.
    let (type_token, _remainder) = split_type_token(&normalised);

    // Parse the type token and extract base name + generic arguments.
    let parsed = PhpType::parse(type_token);
    match parsed {
        PhpType::Generic(name, args) if !args.is_empty() => {
            let base_name = strip_fqn_prefix(&name).to_string();
            if base_name.is_empty() {
                return None;
            }
            Some((base_name, args))
        }
        _ => None,
    }
}

// ─── Type Aliases ───────────────────────────────────────────────────────────

/// Extract all `@phpstan-type` / `@psalm-type` local type aliases and
/// `@phpstan-import-type` / `@psalm-import-type` imported aliases from a
/// docblock.
///
/// Returns a map from alias name to [`TypeAliasDef`].  Local aliases are
/// parsed into a `PhpType` at construction time; imported aliases store
/// the source class and original alias name for cross-file resolution.
pub fn extract_type_aliases(docblock: &str) -> HashMap<String, TypeAliasDef> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return HashMap::new();
    };

    extract_type_aliases_from_info(&info)
}

/// Like [`extract_type_aliases`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_type_aliases_from_info(info: &DocblockInfo) -> HashMap<String, TypeAliasDef> {
    let mut aliases = HashMap::new();

    // ── Local type alias: @phpstan-type / @psalm-type ──
    for tag in info.tags_by_kinds(&[TagKind::PhpstanType, TagKind::PsalmType, TagKind::Type]) {
        let desc = tag.description.trim();
        if desc.is_empty() {
            continue;
        }

        // mago-docblock joins multi-line descriptions with \n; normalise.
        let normalised = collapse_newlines(desc);

        // Split into alias name and definition.
        // Format: `AliasName = Definition` or `AliasName Definition`
        if let Some((name, def)) = parse_local_type_alias(&normalised)
            && !name.is_empty()
            && !def.is_empty()
        {
            aliases.insert(name.to_string(), TypeAliasDef::Local(PhpType::parse(def)));
        }
    }

    // ── Imported type alias: @phpstan-import-type / @psalm-import-type ──
    for tag in info.tags_by_kinds(&[
        TagKind::PhpstanImportType,
        TagKind::PsalmImportType,
        TagKind::ImportType,
    ]) {
        let desc = tag.description.trim();
        if desc.is_empty() {
            continue;
        }

        // Format: `TypeName from ClassName` or `TypeName from ClassName as LocalAlias`
        if let Some((alias_name, definition)) = parse_import_type_alias(desc) {
            aliases.insert(alias_name, definition);
        }
    }

    aliases
}

/// Parse a local `@phpstan-type` alias definition.
///
/// Accepts both `AliasName = Definition` and `AliasName Definition` forms.
/// The definition may contain complex types with `{…}`, `<…>`, `(…)` nesting.
///
/// Returns `(alias_name, definition)` or `None` if parsing fails.
fn parse_local_type_alias(rest: &str) -> Option<(&str, &str)> {
    // The alias name is the first word (identifier characters).
    let name_end = rest
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty() {
        return None;
    }

    let after_name = rest[name_end..].trim_start();

    // Optional `=` separator
    let definition = after_name
        .strip_prefix('=')
        .unwrap_or(after_name)
        .trim_start();

    if definition.is_empty() {
        return None;
    }

    // The definition runs to the end of the line (docblock lines are
    // already split).  Trim trailing whitespace.
    let definition = definition.trim_end();

    Some((name, definition))
}

/// Parse an `@phpstan-import-type` alias.
///
/// Format: `TypeName from ClassName` or `TypeName from ClassName as LocalAlias`
///
/// Returns `(local_alias_name, TypeAliasDef::Import { … })` so the
/// resolver can look up the alias in the source class.
fn parse_import_type_alias(rest: &str) -> Option<(String, TypeAliasDef)> {
    // Split: TypeName from ClassName [as LocalAlias]
    let parts: Vec<&str> = rest.split_whitespace().collect();

    // Minimum: TypeName from ClassName  (3 parts)
    if parts.len() < 3 || parts[1] != "from" {
        return None;
    }

    let original_name = parts[0];
    let source_class = parts[2];

    // Check for `as LocalAlias`
    let alias_name = if parts.len() >= 5 && parts[3] == "as" {
        parts[4].to_string()
    } else {
        original_name.to_string()
    };

    let definition = TypeAliasDef::Import {
        source_class: source_class.to_string(),
        original_name: original_name.to_string(),
    };

    Some((alias_name, definition))
}

// ─── Conditional Return Type Synthesis ──────────────────────────────────────

/// Synthesize a conditional return type from `@template` + `@param class-string<T>`
/// patterns.
///
/// When a method declares a template parameter (e.g. `@template TClass`)
/// whose return type is that template parameter, and a `@param` annotation
/// binds it via `class-string<TClass>`, the method effectively returns
/// an instance of whatever class name is passed as that argument.
///
/// This function detects that pattern and produces a
/// [`PhpType::Conditional`] so that the resolver can substitute the
/// concrete class at call sites.
///
/// Returns `None` if the pattern is not detected, or if
/// `has_existing_conditional` is true (an explicit conditional return type
/// in the docblock takes precedence).
pub fn synthesize_template_conditional(
    docblock: &str,
    template_params: &[String],
    return_type: Option<&PhpType>,
    has_existing_conditional: bool,
) -> Option<PhpType> {
    let info = parse_docblock_for_tags(docblock)?;
    synthesize_template_conditional_from_info(
        &info,
        template_params,
        return_type,
        has_existing_conditional,
    )
}

/// Like [`synthesize_template_conditional`], but operates on a pre-parsed [`DocblockInfo`].
pub fn synthesize_template_conditional_from_info(
    info: &DocblockInfo,
    template_params: &[String],
    return_type: Option<&PhpType>,
    has_existing_conditional: bool,
) -> Option<PhpType> {
    // Don't override an existing conditional return type.
    if has_existing_conditional {
        return None;
    }

    if template_params.is_empty() {
        return None;
    }

    let ret = return_type?;

    // Strip nullable wrapper so that `?T` matches template param `T`.
    let stripped_name = match ret {
        PhpType::Nullable(inner) => {
            if let PhpType::Named(n) = inner.as_ref() {
                n.as_str()
            } else {
                return None;
            }
        }
        PhpType::Named(n) => n.as_str(),
        _ => return None,
    };

    // Check if the (stripped) return type is one of the template params.
    if !template_params.iter().any(|t| t == stripped_name) {
        return None;
    }

    // Find a `@param class-string<T> $paramName` annotation for this
    // template param, and extract the parameter name (without `$`).
    //
    // When the same template param appears in multiple `@param class-string<T>`
    // annotations (e.g. `@param class-string<T> $a1, @param class-string<T> $a2`),
    // skip the conditional synthesis.  The synthesized conditional only references
    // one parameter, so it cannot produce a union of the concrete types from
    // multiple arguments.  The template substitution path (`build_function_template_subs`)
    // handles this correctly by calling `insert_or_union` for each binding.
    let param_names = find_all_class_string_param_names_from_info(info, stripped_name);
    if param_names.len() != 1 {
        return None;
    }
    let param_name = param_names.into_iter().next()?;

    Some(PhpType::Conditional {
        param: format!("${param_name}"),
        negated: false,
        condition: Box::new(PhpType::ClassString(None)),
        then_type: Box::new(PhpType::mixed()),
        else_type: Box::new(PhpType::mixed()),
    })
}

/// Search a parsed docblock for all `@param class-string<T> $paramName`
/// annotations where `T` matches the given `template_name`.
///
/// Returns parameter names **without** the `$` prefix.
/// When the result has more than one entry, the same template param
/// is bound to multiple parameters (e.g. `@param class-string<T> $a1`,
/// `@param class-string<T> $a2`).
fn find_all_class_string_param_names_from_info(
    info: &DocblockInfo,
    template_name: &str,
) -> Vec<String> {
    let mut names = Vec::new();
    for tag in info.tags_by_kinds(&[TagKind::PhpstanParam, TagKind::PsalmParam, TagKind::Param]) {
        let desc = tag.description.trim();
        if desc.is_empty() {
            continue;
        }
        let (type_token, remainder) = split_type_token(desc);
        let parsed = PhpType::parse(type_token);
        if !contains_class_string_of(&parsed, template_name) {
            continue;
        }
        let mut search = remainder;
        while let Some(rest) = search.strip_prefix('|') {
            let rest = rest.trim_start();
            let (_, after) = split_type_token(rest);
            search = after;
        }
        if let Some(var_name) = search.split_whitespace().next() {
            let var_name = var_name.strip_prefix("...").unwrap_or(var_name);
            if let Some(name) = var_name.strip_prefix('$') {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// Check whether a [`PhpType`] contains `class-string<T>` where the inner
/// type parameter matches `template_name`.
///
/// Recursively unwraps nullable and union types so that `?class-string<T>`,
/// `class-string<T>|null`, and `class-string<T>|string` are all matched.
fn contains_class_string_of(ty: &PhpType, template_name: &str) -> bool {
    match ty {
        PhpType::ClassString(Some(inner)) => {
            // Check if the inner type is exactly the template name.
            matches!(inner.as_ref(), PhpType::Named(name) if name == template_name)
        }
        PhpType::Nullable(inner) => contains_class_string_of(inner, template_name),
        PhpType::Union(members) => members
            .iter()
            .any(|m| contains_class_string_of(m, template_name)),
        PhpType::Intersection(members) => members
            .iter()
            .any(|m| contains_class_string_of(m, template_name)),
        _ => false,
    }
}
