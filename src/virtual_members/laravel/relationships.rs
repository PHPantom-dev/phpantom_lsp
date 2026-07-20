//! Eloquent relationship classification, property type synthesis, and
//! body-text inference.
//!
//! This module handles the mapping from Eloquent relationship method
//! return types (e.g. `HasMany<Post, $this>`) to virtual property types
//! (e.g. `Illuminate\Database\Eloquent\Collection<Post>`), as well as
//! inferring relationship types from method body text when no `@return`
//! annotation is present.

use std::sync::Arc;

use crate::php_type::PhpType;
use crate::types::{ClassInfo, ELOQUENT_COLLECTION_FQN};
use crate::util::{short_name, strip_fqn_prefix};

use super::helpers::{camel_to_snake, snake_to_camel};

/// Methods on `Builder` / `QueriesRelationships` that accept a relation
/// name string as the first argument and a closure typed as
/// `Closure(Builder<TRelatedModel>): mixed` as the second argument
/// (or at the listed position).
///
/// When one of these methods is detected, the closure parameter
/// inference overrides `TModel` with the related model resolved from
/// the relation name string.
pub(crate) const RELATION_QUERY_METHODS: &[&str] = &[
    "has",
    "orHas",
    "doesntHave",
    "orDoesntHave",
    "whereHas",
    "orWhereHas",
    "withWhereHas",
    "whereDoesntHave",
    "orWhereDoesntHave",
    "whereRelation",
];

/// Fully-qualified relationship class names used by
/// [`infer_relationship_from_body`].
const RELATIONSHIP_METHOD_FQN_MAP: &[(&str, &str)] = &[
    (
        "hasOne",
        "Illuminate\\Database\\Eloquent\\Relations\\HasOne",
    ),
    (
        "hasMany",
        "Illuminate\\Database\\Eloquent\\Relations\\HasMany",
    ),
    (
        "belongsTo",
        "Illuminate\\Database\\Eloquent\\Relations\\BelongsTo",
    ),
    (
        "belongsToMany",
        "Illuminate\\Database\\Eloquent\\Relations\\BelongsToMany",
    ),
    (
        "morphOne",
        "Illuminate\\Database\\Eloquent\\Relations\\MorphOne",
    ),
    (
        "morphMany",
        "Illuminate\\Database\\Eloquent\\Relations\\MorphMany",
    ),
    (
        "morphTo",
        "Illuminate\\Database\\Eloquent\\Relations\\MorphTo",
    ),
    (
        "morphToMany",
        "Illuminate\\Database\\Eloquent\\Relations\\MorphToMany",
    ),
    (
        "morphedByMany",
        "Illuminate\\Database\\Eloquent\\Relations\\MorphToMany",
    ),
    (
        "hasManyThrough",
        "Illuminate\\Database\\Eloquent\\Relations\\HasManyThrough",
    ),
    (
        "hasOneThrough",
        "Illuminate\\Database\\Eloquent\\Relations\\HasOneThrough",
    ),
];

/// Known Eloquent relationship class short names that yield a single
/// (nullable) related model instance when accessed as a property.
const SINGULAR_RELATIONSHIPS: &[&str] = &["HasOne", "MorphOne", "BelongsTo", "HasOneThrough"];

/// Known Eloquent relationship class short names that yield a
/// `Collection<TRelated>` when accessed as a property.
const COLLECTION_RELATIONSHIPS: &[&str] = &[
    "HasMany",
    "MorphMany",
    "BelongsToMany",
    "HasManyThrough",
    "MorphToMany",
];

/// The `MorphTo` relationship resolves to the generic `Model` base class
/// because the concrete related type is determined at runtime.
const MORPH_TO: &str = "MorphTo";

/// The FQN namespace prefix for Eloquent relationship classes.
///
/// When a return type is fully-qualified, we verify it lives under this
/// namespace before classifying it as a relationship.  This prevents
/// false positives from user classes that happen to share short names
/// with Eloquent relationships (e.g. `App\Relations\HasMany`).
const ELOQUENT_RELATIONS_NS: &str = "Illuminate\\Database\\Eloquent\\Relations\\";

/// The category of a relationship return type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelationshipKind {
    /// HasOne, MorphOne, BelongsTo — singular nullable model.
    Singular,
    /// HasMany, MorphMany, BelongsToMany, HasManyThrough, MorphToMany — Collection.
    Collection,
    /// MorphTo — generic Model.
    MorphTo,
}

/// Classify a relationship return type into its [`RelationshipKind`].
///
/// Accepts both short names (`HasMany`) and fully-qualified names
/// (`\Illuminate\Database\Eloquent\Relations\HasMany`).  Generic
/// parameters are stripped before matching.
///
/// When the base type is namespace-qualified (contains `\`), the
/// function verifies that it lives under
/// `Illuminate\Database\Eloquent\Relations\` before classifying.
/// This prevents false positives from user classes whose short name
/// collides with an Eloquent relationship class (e.g. a custom
/// `App\Relations\HasMany` that does not extend Eloquent's).
///
/// Unqualified names (no `\`) are matched by short name only, which
/// is the common case for body-inferred types and docblock annotations
/// that use `use` imports.
pub(crate) fn classify_relationship_typed(return_type: &PhpType) -> Option<RelationshipKind> {
    let base = return_type.base_name()?;
    let sname = short_name(base);

    if base.contains('\\') && !base.starts_with(ELOQUENT_RELATIONS_NS) {
        return None;
    }

    if SINGULAR_RELATIONSHIPS.contains(&sname) {
        return Some(RelationshipKind::Singular);
    }
    if COLLECTION_RELATIONSHIPS.contains(&sname) {
        return Some(RelationshipKind::Collection);
    }
    if sname == MORPH_TO {
        return Some(RelationshipKind::MorphTo);
    }

    None
}

/// Extract the `TRelated` type from a relationship return type's
/// generic parameters.
///
/// Given `HasMany<Post, $this>`, returns `Some(&PhpType::Named("Post"))`.
/// Given `HasOne<\App\Models\Profile, $this>`, returns
/// `Some(&PhpType::Named("\App\Models\Profile"))`.
///
/// Returns `None` if no generic parameters are present.
pub(super) fn extract_related_type_typed(return_type: &PhpType) -> Option<&PhpType> {
    if let PhpType::Generic(_, args) = return_type {
        let first = args.first()?;
        if first.is_empty() {
            return None;
        }
        return Some(first);
    }
    None
}

/// Pre-built `Illuminate\Database\Eloquent\Model` type for fallback related types.
fn eloquent_model_type() -> PhpType {
    PhpType::Named("Illuminate\\Database\\Eloquent\\Model".to_owned())
}

/// Build the property type string for a relationship.
///
/// - Singular relationships → the related type as-is (nullable).
/// - Collection relationships → the custom collection class (if set) or
///   `Illuminate\Database\Eloquent\Collection`, parameterised with `<TRelated>`.
/// - MorphTo → `Illuminate\Database\Eloquent\Model`.
pub(super) fn build_property_type(
    kind: RelationshipKind,
    related_type: Option<&PhpType>,
    custom_collection: Option<&str>,
) -> Option<PhpType> {
    match kind {
        RelationshipKind::Singular => related_type.cloned(),
        RelationshipKind::Collection => {
            let inner = related_type.cloned().unwrap_or_else(eloquent_model_type);
            let collection_class = custom_collection.unwrap_or(ELOQUENT_COLLECTION_FQN);
            Some(PhpType::Generic(collection_class.to_string(), vec![inner]))
        }
        RelationshipKind::MorphTo => Some(eloquent_model_type()),
    }
}

/// Whether `name` (case-insensitively) resolves to a relationship method
/// on `class`.
///
/// Eloquent relation property access (`$model->orderProducts`) flows
/// through `Model::__get()` → `getRelationValue()` → `isRelation()` →
/// `method_exists()`, all of which are case-insensitive.  So accessing a
/// relation as `$model->orderproducts` resolves the same relationship at
/// runtime.  Property matching is otherwise case-sensitive, so callers
/// use this to grant relation-backed virtual properties the same
/// case-insensitivity PHP method calls already have.
pub(crate) fn class_has_relation_method_ci(class: &ClassInfo, name: &str) -> bool {
    class
        .get_method(name)
        .and_then(|m| m.return_type.as_ref())
        .is_some_and(|rt| classify_relationship_typed(rt).is_some())
}

/// Map a `*_count` virtual property name back to the relationship method
/// name that produced it.
///
/// Returns `Some(method_name)` when `property_name` ends with `_count`
/// and the stripped/camelCased remainder is a relationship method on
/// `class`.  Go-to-definition uses this so that clicking on
/// `posts_count` jumps to the `posts()` method, and
/// `master_recipe_count` jumps to `masterRecipe()`.
pub(crate) fn count_property_to_relationship_method(
    class: &ClassInfo,
    property_name: &str,
) -> Option<String> {
    let base = property_name.strip_suffix("_count")?;
    if base.is_empty() {
        return None;
    }
    let method_name = snake_to_camel(base);
    let method = class.get_method(&method_name)?;
    let return_type = method.return_type.as_ref()?;
    if classify_relationship_typed(return_type).is_some() {
        Some(method_name)
    } else {
        None
    }
}

/// Infer a relationship return type from a method's body text.
///
/// When a relationship method has no `@return` annotation, this function
/// scans the body for patterns like `$this->hasMany(Post::class)` and
/// synthesizes a fully-qualified return type string (e.g.
/// `Illuminate\Database\Eloquent\Relations\HasMany<Post>`).
///
/// Supports all standard Eloquent relationship builder methods:
/// `hasOne`, `hasMany`, `belongsTo`, `belongsToMany`, `morphOne`,
/// `morphMany`, `morphTo`, `morphToMany`, `morphedByMany`,
/// `hasManyThrough`, and `hasOneThrough`.
///
/// Returns `None` if no recognisable pattern is found.
pub fn infer_relationship_from_body(body_text: &str) -> Option<PhpType> {
    for &(method_name, fqn) in RELATIONSHIP_METHOD_FQN_MAP {
        // Look for `$this->methodName(` in the body text.
        let needle = format!("$this->{method_name}(");
        let Some(call_pos) = body_text.find(&needle) else {
            continue;
        };

        // `morphTo` never carries a related-model generic parameter;
        // the concrete type is determined at runtime.
        //
        // The FQN is prefixed with `\` so that `resolve_type_string`
        // in `ast_update.rs` recognises it as already-qualified and
        // does not prepend the declaring file's namespace.
        // `resolve_name` will strip the leading `\` back to canonical
        // form during the resolution pass.
        if method_name == "morphTo" {
            return Some(PhpType::Named(format!("\\{fqn}")));
        }

        // Extract the first argument from the call.  We look for
        // `SomeName::class` as the first positional argument.
        let args_start = call_pos + needle.len();
        let after_paren = &body_text[args_start..];

        if let Some(class_arg) = extract_class_argument(after_paren) {
            return Some(PhpType::Generic(
                format!("\\{fqn}"),
                vec![PhpType::Named(class_arg)],
            ));
        }

        // No `::class` argument found — return the bare relationship
        // name without generics.  The provider will handle it the same
        // way it handles annotated relationships without generics.
        return Some(PhpType::Named(format!("\\{fqn}")));
    }

    None
}

/// Extract a class name from the first `X::class` argument in a
/// parenthesised argument list.
///
/// Given the text after the opening `(`, e.g. `Post::class, 'user_id')`,
/// returns `Some("Post")`.  Also handles fully-qualified names like
/// `\App\Models\Post::class` and `self::class` / `static::class`.
///
/// Returns `None` if no `::class` token is found before the closing `)`.
fn extract_class_argument(after_paren: &str) -> Option<String> {
    // Find the closing paren to bound our search.
    let end = after_paren.find(')')?;
    let args_region = &after_paren[..end];

    // Isolate the first argument (before the first comma) and look for
    // `X::class` within it.
    let first_arg = args_region.split(',').next().unwrap_or(args_region);
    let class_pos = first_arg.find("::class")?;
    let before = first_arg[..class_pos].trim();

    if before.is_empty() {
        return None;
    }

    // Strip leading backslash for FQNs and extract the short name.
    let name = strip_fqn_prefix(before);
    let short_name = short_name(name);

    if short_name.is_empty() {
        return None;
    }

    Some(short_name.to_string())
}

/// Build a `{snake_name}_count` property name for a relationship method.
///
/// Used by the provider to synthesize `*_count` properties for each
/// relationship.
pub(super) fn count_property_name(method_name: &str) -> String {
    format!("{}_count", camel_to_snake(method_name))
}

/// Walk a dot-separated relation chain starting from `model` and return
/// the fully-qualified name of the final related model.
///
/// For example, given model `ArticleCategoryTranslation` and chain
/// `"category.articles"`:
///
/// 1. Look up `category()` on `ArticleCategoryTranslation` → returns
///    `BelongsTo<ArticleCategory>` → extract `ArticleCategory`.
/// 2. Look up `articles()` on `ArticleCategory` → returns
///    `HasMany<Article>` → extract `Article`.
/// 3. Return `"App\\Models\\Article"` (the FQN).
///
/// Returns `None` if any segment cannot be resolved (missing method,
/// no relationship return type, class not found).
pub(crate) fn resolve_relation_chain(
    model: &ClassInfo,
    chain: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&super::super::ResolvedClassCache>,
) -> Option<String> {
    let segments: Vec<&str> = chain.split('.').collect();
    if segments.is_empty() {
        return None;
    }

    let mut current_class = resolve_class_with_inheritance(model, class_loader, cache);
    for segment in &segments {
        let segment = segment.trim();
        if segment.is_empty() {
            return None;
        }

        // Find the relationship method on the current model.
        let method = current_class.get_method(segment)?;

        // Get the return type and extract the related model type.
        // Body-inferred relationship types are already stored in
        // `return_type` by the parser, so no fallback is needed.
        let return_type = method.return_type.as_ref()?;
        let related_type = extract_related_type_for_chain(return_type, &current_class)?;

        // Resolve the related type to a full class, trying the model's
        // namespace first (e.g. short name "Article" → "App\Models\Article").
        let resolved = resolve_related_fqn(&related_type, &current_class, class_loader)?;
        current_class = resolve_class_with_inheritance(&resolved, class_loader, cache);
    }

    Some(current_class.fqn().to_string())
}

/// Resolve a class fully (with inheritance and virtual members) so that
/// relationship methods from traits and parent classes are visible.
fn resolve_class_with_inheritance(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&super::super::ResolvedClassCache>,
) -> Arc<ClassInfo> {
    crate::virtual_members::resolve_class_fully_maybe_cached(class, class_loader, cache)
}

/// Extract the related type from a relationship return type string,
/// resolving `$this` / `static` to the declaring class.
fn extract_related_type_for_chain(
    return_type: &PhpType,
    declaring_class: &ClassInfo,
) -> Option<String> {
    classify_relationship_typed(return_type)?;

    // Check the first generic arg directly as a PhpType before
    // stringifying, so we can use the `is_self_ref()` predicate
    // instead of comparing raw strings.
    if let PhpType::Generic(_, args) = return_type {
        let first = args.first()?;
        if first.is_self_ref() {
            return Some(declaring_class.fqn().to_string());
        }
    }

    extract_related_type_typed(return_type).and_then(|t| t.base_name().map(|s| s.to_string()))
}

/// Resolve a short or FQN related type to a loadable FQN.
///
/// Tries the following strategies:
/// 1. Direct load (works for FQNs).
/// 2. Prepend the declaring class's namespace (works for short names
///    in the same namespace, e.g. `Article` → `App\Models\Article`).
fn resolve_related_fqn(
    related_type: &str,
    declaring_class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Arc<ClassInfo>> {
    let cleaned = related_type.trim_start_matches('\\');

    // Try direct load first (handles FQNs).
    if let Some(cls) = class_loader(cleaned) {
        return Some(cls);
    }

    // Try prepending the declaring class's namespace.
    if let Some(ref ns) = declaring_class.file_namespace {
        let fqn = format!("{}\\{}", ns, cleaned);
        if let Some(cls) = class_loader(&fqn) {
            return Some(cls);
        }
    }

    None
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "relationships_tests.rs"]
mod tests;
