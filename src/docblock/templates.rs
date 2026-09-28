//! Template, generics, and type alias tag extraction.
//!
//! This submodule handles `@template` (including `-covariant` /
//! `-contravariant` variants), `@extends` / `@implements` / `@use`
//! generic binding tags, `@phpstan-type` / `@psalm-type` local type
//! aliases, `@phpstan-import-type` / `@psalm-import-type` imported
//! aliases, and `class-string<T>` conditional return type synthesis.

use std::collections::HashMap;

use super::tag_kind::TagKind;

use super::parser::{DocblockInfo, TagInfo, TagValueInfo, parse_docblock_for_tags};
use crate::php_type::{PhpType, TypeKind};
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
        TagKind::TemplateCovariant => TemplateVariance::Covariant,
        TagKind::TemplateContravariant => TemplateVariance::Contravariant,
        _ => TemplateVariance::Invariant,
    }
}

/// `TagKind` values that represent `@template` declarations (all variance variants).
pub(crate) const TEMPLATE_KINDS: &[TagKind] = &[
    TagKind::Template,
    TagKind::TemplateCovariant,
    TagKind::TemplateContravariant,
];

/// Like [`extract_template_params_full`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_template_params_full_from_info(
    info: &DocblockInfo,
) -> Vec<(String, Option<PhpType>, TemplateVariance, Option<PhpType>)> {
    let mut results = Vec::new();

    for tag in info.tags_by_kinds(TEMPLATE_KINDS) {
        let Some(template) = tag.value.as_template() else {
            continue;
        };

        results.push((
            template.name.clone(),
            template.bound.as_deref().map(PhpType::parse),
            variance_for(tag.kind),
            template.default.as_deref().map(PhpType::parse),
        ));
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

    for tag in info.tags_by_kind(TagKind::Param) {
        let (Some(type_text), Some(param_name)) = (tag.type_text(), tag.variable()) else {
            continue;
        };

        // Parse the type into a PhpType tree and walk it to find all
        // template parameter references, correctly handling nested
        // generics like `Wrapper<Collection<T>, V>`.
        let parsed = PhpType::parse(&type_text);
        collect_template_bindings(&parsed, template_params, &param_name, &mut results);
    }

    if results.is_empty() {
        return results;
    }

    // `@template T as (Closure(TValue): TMappedValue)` with `@param T $cb`
    // binds `TMappedValue` from `$cb` as well, through `T`'s bound.
    let bound_bindings: Vec<(String, String)> = extract_template_params_full_from_info(info)
        .into_iter()
        .filter_map(|(name, bound, ..)| Some((name, bound?)))
        .flat_map(|(name, bound)| {
            let mut through_bound = Vec::new();
            for (template, param) in results.iter().filter(|(t, _)| *t == name) {
                collect_template_bindings(&bound, template_params, param, &mut through_bound);
                through_bound.retain(|(t, _)| t != template);
            }
            through_bound
        })
        .collect();
    for binding in bound_bindings {
        if !results.contains(&binding) {
            results.push(binding);
        }
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
    match ty.kind() {
        TypeKind::Named(name) => {
            if let Some(t) = template_params.iter().find(|t| t.as_str() == name) {
                results.push((t.to_string(), param_name.to_string()));
            }
        }
        TypeKind::Nullable(inner) => {
            collect_template_bindings(inner, template_params, param_name, results);
        }
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            for member in members {
                collect_template_bindings(member, template_params, param_name, results);
            }
        }
        TypeKind::Array(inner) => {
            collect_template_bindings(inner, template_params, param_name, results);
        }
        TypeKind::Generic(g) => {
            for arg in &g.args {
                collect_template_bindings(arg, template_params, param_name, results);
            }
        }
        TypeKind::ClassString(Some(inner))
        | TypeKind::InterfaceString(Some(inner))
        | TypeKind::KeyOf(inner)
        | TypeKind::ValueOf(inner) => {
            collect_template_bindings(inner, template_params, param_name, results);
        }
        TypeKind::Callable(c) => {
            for p in &c.params {
                collect_template_bindings(&p.type_hint, template_params, param_name, results);
            }
            if let Some(rt) = &c.return_type {
                collect_template_bindings(rt, template_params, param_name, results);
            }
        }
        TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => {
            for entry in entries {
                collect_template_bindings(&entry.value_type, template_params, param_name, results);
            }
        }
        TypeKind::IndexAccess(target, index) => {
            collect_template_bindings(target, template_params, param_name, results);
            collect_template_bindings(index, template_params, param_name, results);
        }
        TypeKind::Conditional(c) => {
            collect_template_bindings(&c.condition, template_params, param_name, results);
            collect_template_bindings(&c.then_type, template_params, param_name, results);
            collect_template_bindings(&c.else_type, template_params, param_name, results);
        }
        _ => {}
    }
}

/// Like [`extract_generics_tag`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_generics_tag_from_info(
    info: &DocblockInfo,
    tag: &str,
) -> Vec<(String, Vec<PhpType>)> {
    // Map the tag string to the corresponding `TagKind`.  Vendor prefixes
    // and the `@template-` spelling are already folded together by the
    // parser, so `@extends`, `@phpstan-extends` and `@template-extends`
    // all arrive as `TagKind::Extends`.
    let bare_tag = tag.strip_prefix('@').unwrap_or(tag);
    let kind = match bare_tag {
        "extends" => Some(TagKind::Extends),
        "implements" => Some(TagKind::Implements),
        "use" => Some(TagKind::Use),
        _ => None,
    };

    let matches_tag = |tag: &TagInfo| match kind {
        Some(kind) => tag.kind == kind,
        // Any other tag name is not modelled structurally; match it by name.
        None => tag.name == bare_tag,
    };

    info.tags
        .iter()
        .filter(|tag| matches_tag(tag))
        .filter_map(|tag| parse_generics_type(&tag.type_text()?))
        .collect()
}

/// Split a generics tag type (e.g. `"Collection<int, Language>"`) into a
/// `(base_name, generic_args)` tuple.  Types without arguments are not
/// generic bindings and yield `None`.
fn parse_generics_type(type_text: &str) -> Option<(String, Vec<PhpType>)> {
    match PhpType::parse(type_text).kind() {
        TypeKind::Generic(g) if !g.args.is_empty() => {
            let base_name = strip_fqn_prefix(&g.name).to_string();
            (!base_name.is_empty()).then(|| (base_name, g.args.clone()))
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

    for tag in &info.tags {
        match &tag.value {
            // Local alias: `@phpstan-type AliasName = Definition`.
            TagValueInfo::TypeAlias(alias) => {
                aliases.insert(
                    alias.alias.clone(),
                    TypeAliasDef::Local(PhpType::parse(&alias.definition)),
                );
            }
            // Imported alias: `@phpstan-import-type Name from Class as Local`.
            TagValueInfo::TypeAliasImport(import) => {
                let local = import
                    .local
                    .clone()
                    .unwrap_or_else(|| import.imported.clone());
                aliases.insert(
                    local,
                    TypeAliasDef::Import {
                        source_class: import.from.clone(),
                        original_name: import.imported.clone(),
                    },
                );
            }
            _ => {}
        }
    }

    aliases
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
/// [`TypeKind::Conditional`] so that the resolver can substitute the
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
    let stripped_name = match ret.kind() {
        TypeKind::Nullable(inner) => {
            if let TypeKind::Named(n) = inner.kind() {
                n.as_str()
            } else {
                return None;
            }
        }
        TypeKind::Named(n) => n.as_str(),
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

    Some(PhpType::conditional(
        format!("${param_name}"),
        false,
        PhpType::class_string(None),
        PhpType::mixed(),
        PhpType::mixed(),
    ))
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
    for tag in info.tags_by_kind(TagKind::Param) {
        let (Some(type_text), Some(var_name)) = (tag.type_text(), tag.variable()) else {
            continue;
        };
        if !contains_class_string_of(&PhpType::parse(&type_text), template_name) {
            continue;
        }
        if let Some(name) = var_name.strip_prefix('$') {
            names.push(name.to_string());
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
    match ty.kind() {
        TypeKind::ClassString(Some(inner)) => {
            // Check if the inner type is exactly the template name.
            matches!(inner.kind(), TypeKind::Named(name) if name == template_name)
        }
        TypeKind::Nullable(inner) => contains_class_string_of(inner, template_name),
        TypeKind::Union(members) => members
            .iter()
            .any(|m| contains_class_string_of(m, template_name)),
        TypeKind::Intersection(members) => members
            .iter()
            .any(|m| contains_class_string_of(m, template_name)),
        _ => false,
    }
}
