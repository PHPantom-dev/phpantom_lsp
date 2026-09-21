//! Virtual member provider abstraction.
//!
//! Virtual members are methods and properties that do not exist as real
//! PHP declarations but are surfaced by magic methods (`__call`, `__get`,
//! `__set`, etc.) or framework conventions.  Three providers produce
//! virtual members today:
//!
//! 1. **Laravel model provider** — synthesizes members from
//!    framework-specific patterns (relationship properties, scope methods,
//!    Builder-as-static forwarding, convention-based `factory()` method).
//! 2. **Laravel factory provider** — synthesizes `create()` and `make()`
//!    methods on factory classes that return the corresponding model type,
//!    using the naming convention when no `@extends Factory<Model>`
//!    annotation is present.
//! 3. **PHPDoc provider** (`@method`, `@property`, `@property-read`,
//!    `@property-write`, `@mixin`) — documents magic members on a class.
//!    Within this provider, explicit `@method` / `@property` tags take
//!    precedence over members inherited from `@mixin` classes.
//!
//! All are unified behind the [`VirtualMemberProvider`] trait.
//! Providers are queried in priority order after base resolution
//! (own members + traits + parent chain) is complete.  A member
//! contributed by a higher-priority provider is never overwritten by a
//! lower-priority one, and all virtual members lose to real declared
//! members.
//!
//! # Caching
//!
//! [`resolve_class_fully`] is called from many code paths (completion,
//! hover, go-to-definition, call resolution, etc.) and often for the
//! same class within a single request.  The full resolution (inheritance
//! walk + virtual member providers + interface merging) is expensive, so
//! [`resolve_class_fully_cached`] accepts a [`ResolvedClassCache`] that
//! stores results keyed by fully-qualified class name plus concrete
//! generic arguments.  The cache is stored on `Backend`; when a file is
//! re-parsed, the classes it defines and their transitive dependents
//! are evicted ([`evict_fqn`]), so stale entries never survive an edit.
//!
//! # Precedence model
//!
//! ```text
//! 1. Real declared members (in PHP source code)
//! 2. Trait members (real implementations)
//! 3. Parent chain members (real implementations)
//! 4. Virtual member providers (in priority order):
//!    a. Laravel model provider  — richest type info
//!    b. Laravel factory provider — convention-based factory methods
//!    c. PHPDoc provider          — @method, @property, @mixin
//! ```

pub mod cache;
pub mod laravel;
pub mod phpdoc;
pub mod resolve;

pub use cache::{
    ResolvedClassCache, ResolvedClassCacheKey, active_resolved_class_cache, evict_fqn,
    new_resolved_class_cache, with_active_resolved_class_cache,
};
pub(crate) use cache::{
    TransformFingerprint, intern_transformed_method, intern_transformed_property,
};
pub(crate) use resolve::resolve_class_base_cached;
pub use resolve::{
    populate_from_sorted, property_write_is_magic, resolve_class_fully, resolve_class_fully_cached,
    resolve_class_fully_maybe_cached, resolve_class_fully_with_generics,
    resolve_class_fully_with_type_args,
};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ConstantInfo, MethodInfo, PropertyInfo, PropertySource};

/// Members synthesized by a provider.
///
/// Merged below real declared members, traits, and the parent chain.
/// Each provider returns a `VirtualMembers` value from its
/// [`provide`](VirtualMemberProvider::provide) method.
pub struct VirtualMembers {
    /// Virtual methods to add to the class.
    ///
    /// `Arc`-wrapped so a provider can share one allocation across every
    /// class it contributes the same method to (e.g. mixin members and
    /// Builder-forwarded methods interned per transform), instead of the
    /// merge cloning a fresh copy per class.
    pub methods: Vec<Arc<MethodInfo>>,
    /// Virtual properties to add to the class.
    pub properties: Vec<PropertyInfo>,
    /// Virtual constants to add to the class.
    pub constants: Vec<ConstantInfo>,
}

impl VirtualMembers {
    /// Whether this value contains no methods, properties, or constants.
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty() && self.properties.is_empty() && self.constants.is_empty()
    }
}

/// A provider that contributes virtual members to a class.
///
/// Receives the class with traits and parents already merged (via
/// [`resolve_class_with_inheritance`](crate::inheritance::resolve_class_with_inheritance)),
/// but **without** other providers' contributions.  This prevents
/// circular loading when one provider's output would trigger another
/// provider.
///
/// Implementations must be cheap to construct and stateless.  All
/// contextual information is passed through the `class` and
/// `class_loader` arguments.
pub trait VirtualMemberProvider {
    /// Whether this provider has anything to say about this class.
    ///
    /// This is a cheap pre-check so the resolver can skip providers
    /// early without calling [`provide`](Self::provide).  Returning
    /// `false` means [`provide`](Self::provide) will not be called.
    fn applies_to(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> bool;

    /// Produce virtual members for this class.
    ///
    /// Only called when [`applies_to`](Self::applies_to) returned `true`.
    /// The returned members are merged into the class below all real
    /// declared members (own, trait, and parent chain).
    ///
    /// `cache` is the shared resolved-class cache, as an `Option` since
    /// not every caller has one available.  Providers that need to fully
    /// resolve helper classes (e.g. the Laravel model provider resolving
    /// the Eloquent Builder) should use [`resolve_class_fully_maybe_cached`]
    /// with this cache to avoid redundant work across requests.
    fn provide(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        cache: Option<&ResolvedClassCache>,
    ) -> VirtualMembers;
}

/// Merge virtual members into a resolved `ClassInfo`.
///
/// For each method in `virtual.methods`, adds it to `class.methods` only
/// if no method with the same name and same staticness already exists.
/// This allows a provider to contribute both a static and an instance
/// variant of the same method (e.g. Laravel scope methods that are
/// accessible via both `User::active()` and `$user->active()`).
///
/// **Exceptions:** when the existing method has `has_scope_attribute: true`,
/// the virtual method **replaces** it.  `#[Scope]`-attributed methods
/// share their name with the synthesized scope method, but the original
/// is a `protected` implementation detail that should not appear in
/// completion results.  The virtual replacement is `public` with the
/// first `$query` parameter stripped, which is what callers actually see.
/// The `query` / `newQuery` / `newModelQuery` methods are also replaced,
/// so a model with a custom builder returns that builder rather than the
/// base one from the framework declaration. The Laravel provider also
/// refines the real `newQueryWithoutScopes` declaration in place.
///
/// Properties are deduplicated by name.  When a property with the same
/// name already exists, the **more specific** type wins regardless of
/// which provider contributed it.  Specificity is ranked as:
///
///   `array<int, string>` > `array` > `mixed` > (absent)
///
/// More precisely:
/// - absent / empty / `mixed` is the weakest (score 0)
/// - a bare type like `array`, `string`, `Collection` (score 1)
/// - a type with generic parameters like `array<int>` (score 2)
///
/// This allows PHPDoc `@property array<string> $tags` to override a
/// bare `array` from `$casts`, and a `$casts` `array` to override
/// `mixed` from `$fillable`.  One tie-break: an explicit `@property`
/// tag also overrides a type that was merely *inferred* (database
/// schema, attribute defaults) at equal specificity.
///
/// Constants are deduplicated by name only.
///
/// This ensures that real declared members (and contributions from
/// higher-priority providers that were merged earlier) are never
/// overwritten, unless the incoming property carries a more specific type.
pub fn merge_virtual_members(class: &mut ClassInfo, virtual_members: VirtualMembers) {
    // Build an index of (name, is_static) → position for O(1) dedup
    // instead of a linear scan per virtual method.  With hundreds of
    // methods on Eloquent models this turns O(N×M) memcmp into O(M).
    // Keys are lowercased: PHP method names are case-insensitive, so a
    // `@method getvalue` tag matches a declared `getValue()`.
    let mut method_index: HashMap<(String, bool), usize> = class
        .methods
        .iter()
        .enumerate()
        .map(|(i, m)| ((m.name.to_ascii_lowercase(), m.is_static), i))
        .collect();

    for method in virtual_members.methods {
        let key = (method.name.to_ascii_lowercase(), method.is_static);
        if let Some(&idx) = method_index.get(&key) {
            if class.methods[idx].has_scope_attribute
                || matches!(method.name.as_str(), "query" | "newQuery" | "newModelQuery")
                || (method.name == "newQueryWithoutScopes" && !method.is_virtual)
            {
                // Replace the original with the synthesized virtual method.
                // For scope attributes, the original is an implementation detail.
                // For query methods, we want to return the custom builder.
                class.methods.make_mut()[idx] = method;
            }
            // Otherwise: real declared member — keep the original.
        } else {
            let new_idx = class.methods.len();
            class.methods.push(method);
            method_index.insert(key, new_idx);
        }
    }

    let mut prop_index: HashMap<String, usize> = class
        .properties
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.to_string(), i))
        .collect();

    for property in virtual_members.properties {
        if let Some(&idx) = prop_index.get(&property.name.to_string()) {
            // The property already exists.  Replace it only when the
            // incoming property carries a strictly more specific type.
            // This lets PHPDoc `@property array<string> $tags` override
            // a bare `array` from `$casts`, and a `$casts` `array`
            // override `mixed` from `$fillable`.
            //
            // Exception: an explicit `@property` tag overrides a type
            // that was merely *inferred* from the database schema, even
            // at equal specificity.  The schema knows the storage type
            // (`published_at` is a `timestamp` → `string|null`), but the
            // tag declares what the attribute is at runtime (`Carbon`) —
            // user-written intent that the column type cannot see.
            // Types derived from real PHP code ($casts, accessors,
            // relationship methods) still win the tie.
            let existing = &class.properties[idx];
            // Only the two schema-derived sources count as inferred. A
            // property with no source recorded is not "inferred from the
            // schema", it is "provided by something that does not say where
            // its type came from", so it keeps the tie.
            let overrides_inferred = matches!(property.source, Some(PropertySource::DocblockTag))
                && existing.is_virtual
                && matches!(
                    existing.source,
                    Some(
                        PropertySource::DatabaseColumn { .. }
                            | PropertySource::AttributeDefault { .. }
                    )
                )
                && property_type_specificity(&property) > 0;
            if property_type_specificity(&property) > property_type_specificity(existing)
                || (overrides_inferred
                    && property_type_specificity(&property) >= property_type_specificity(existing))
            {
                class.properties.make_mut()[idx] = Arc::new(property);
            }
        } else {
            let new_idx = class.properties.len();
            prop_index.insert(property.name.to_string(), new_idx);
            class.properties.push(Arc::new(property));
        }
    }
    let mut const_names: HashSet<String> =
        class.constants.iter().map(|c| c.name.to_string()).collect();
    for constant in virtual_members.constants {
        if const_names.insert(constant.name.to_string()) {
            class.constants.push(Arc::new(constant));
        }
    }
}

/// Score a type hint by how specific it is.
///
/// The ranking (lowest to highest):
/// - **0** — absent, empty, or `mixed` (no useful type information)
/// - **1** — a bare type name without generic parameters
///   (e.g. `array`, `string`, `Collection`, `?Foo`)
/// - **2** — a type with generic parameters
///   (e.g. `array<int, string>`, `Collection<User>`)
///
/// When merging virtual properties, the property with the higher
/// specificity score wins.  Equal scores preserve the existing property
/// (first-writer-wins within the same specificity tier).
fn type_specificity(hint: &Option<PhpType>) -> u8 {
    let Some(hint) = hint else {
        return 0;
    };
    if hint.is_mixed() || matches!(hint.kind(), TypeKind::Raw(s) if s.trim().is_empty()) {
        return 0;
    }
    if hint.has_type_structure() { 2 } else { 1 }
}

/// Score a property's type by how specific it is, preferring the
/// effective type hint (which may carry a docblock override) and
/// falling back to the native type hint when the effective one scores
/// zero.
///
/// The fallback ensures that properties with actual PHP type
/// declarations (e.g. `public string $name`) are ranked higher than
/// those without any type information, even when docblocks are absent.
fn property_type_specificity(property: &PropertyInfo) -> u8 {
    let effective_score = type_specificity(&property.type_hint);
    if effective_score > 0 {
        return effective_score;
    }
    type_specificity(&property.native_type_hint)
}

/// Apply all registered providers to a base-resolved class.
///
/// Iterates over `providers` in order (highest priority first) and
/// merges each provider's virtual members into `class`.  Because
/// [`merge_virtual_members`] skips members that already exist,
/// higher-priority providers' contributions shadow lower-priority ones.
pub fn apply_virtual_members(
    class: &mut ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    providers: &[Box<dyn VirtualMemberProvider>],
    cache: Option<&ResolvedClassCache>,
) {
    for provider in providers {
        if provider.applies_to(class, class_loader) {
            let virtual_members = provider.provide(class, class_loader, cache);
            if !virtual_members.is_empty() {
                merge_virtual_members(class, virtual_members);
            }
        }
    }
}

/// Return the default set of virtual member providers in priority order.
///
/// Providers are queried in order; a member contributed by an earlier
/// provider is never overwritten by a later one.
///
/// 1. Laravel model provider (highest priority — richest type info)
/// 2. Laravel factory provider (convention-based create/make methods)
/// 3. PHPDoc provider (`@method` / `@property` / `@mixin` tags)
/// 4. Laravel facade provider (concrete class forwarded as static)
///
/// The facade provider deliberately comes last: a facade that carries
/// generated `@method static` tags should keep them, and only a facade
/// without them falls through to the concrete class's own signatures.
///
/// When `is_laravel` is `false`, the Laravel providers are omitted so
/// that a non-Laravel project does not pay for their parent-chain walks.
/// The framework-agnostic PHPDoc provider is always included.
pub fn default_providers(is_laravel: bool) -> Vec<Box<dyn VirtualMemberProvider>> {
    let mut providers: Vec<Box<dyn VirtualMemberProvider>> = Vec::new();
    if is_laravel {
        providers.push(Box::new(laravel::LaravelModelProvider));
        providers.push(Box::new(laravel::LaravelFactoryProvider));
    }
    providers.push(Box::new(phpdoc::PHPDocProvider));
    if is_laravel {
        providers.push(Box::new(laravel::LaravelFacadeProvider));
    }
    providers
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
