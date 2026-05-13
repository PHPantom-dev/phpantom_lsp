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
//! stores results keyed by fully-qualified class name.  The cache is
//! stored on `Backend` and cleared whenever a file is re-parsed
//! (`update_ast` / `parse_and_cache_content`), so stale entries never
//! survive an edit.
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

pub mod laravel;
pub mod phpdoc;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::atom::{Atom, atom};
use crate::inheritance::{
    ClassRef, apply_substitution_to_method, apply_substitution_to_property,
    enrich_method_from_ancestor, enrich_property_from_ancestor, resolve_class_with_inheritance,
};
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ConstantInfo, MethodInfo, PropertyInfo};
use crate::util::short_name;
use crate::virtual_members::laravel::patches::apply_laravel_patches;

/// Cache key for [`ResolvedClassCache`]: fully-qualified class name
/// paired with the concrete generic type arguments used at this
/// instantiation site.
///
/// For non-generic classes the argument list is empty, keeping the
/// common case cheap.  For generic classes like
/// `Illuminate\Database\Eloquent\Builder<App\Models\User>`, the key
/// would be `("Illuminate\\Database\\Eloquent\\Builder", vec!["App\\Models\\User"])`.
///
/// Generic args are stored normalized (fully qualified, sorted when
/// order-independent) to avoid near-miss cache entries.
pub type ResolvedClassCacheKey = (Atom, Vec<String>);

/// Thread-safe cache of fully-resolved classes, keyed by FQN + generic args.
///
/// Stored on [`Backend`](crate::Backend) and selectively invalidated
/// when a file is re-parsed (`update_ast` / `parse_and_cache_content`).
/// Within a single request cycle (completion, hover, etc.) the cache
/// eliminates redundant calls to [`resolve_class_fully`] for the same
/// class at the same generic instantiation.
pub type ResolvedClassCache = Arc<Mutex<HashMap<ResolvedClassCacheKey, Arc<ClassInfo>>>>;

/// Create a new, empty [`ResolvedClassCache`].
pub fn new_resolved_class_cache() -> ResolvedClassCache {
    Arc::new(Mutex::new(HashMap::new()))
}

// ─── Thread-local resolved-class cache access ───────────────────────────────
//
// Many code paths (e.g. `type_hint_to_classes_typed`) call `resolve_class_fully`
// without access to the `Backend`'s `resolved_class_cache`.  Rather than
// threading the cache through dozens of function signatures, we use the
// same thread-local guard pattern as the parse cache: the caller activates
// the cache at a high level (e.g. the diagnostic loop), and inner functions
// pick it up via `active_resolved_class_cache()`.

thread_local! {
    /// Raw pointer to the currently-active [`ResolvedClassCache`], or null.
    ///
    /// Set by [`with_active_resolved_class_cache`], read by
    /// [`active_resolved_class_cache`].  The pointer is valid for the
    /// lifetime of the [`ResolvedCacheGuard`] that set it.
    static ACTIVE_RESOLVED_CACHE: Cell<*const ResolvedClassCache> = const { Cell::new(std::ptr::null()) };
}

/// RAII guard that clears the thread-local cache pointer on drop.
pub struct ResolvedCacheGuard {
    prev: *const ResolvedClassCache,
}

impl Drop for ResolvedCacheGuard {
    fn drop(&mut self) {
        ACTIVE_RESOLVED_CACHE.with(|c| c.set(self.prev));
    }
}

/// Activate a [`ResolvedClassCache`] for the current thread.
///
/// While the returned guard is alive, [`active_resolved_class_cache`]
/// returns `Some(&cache)`.  Supports nesting (restores the previous
/// cache on drop).
///
/// # Example
///
/// ```ignore
/// let _guard = with_active_resolved_class_cache(&backend.resolved_class_cache);
/// // … deep call stacks can now use active_resolved_class_cache()
/// ```
pub fn with_active_resolved_class_cache(cache: &ResolvedClassCache) -> ResolvedCacheGuard {
    let prev = ACTIVE_RESOLVED_CACHE.with(|c| c.get());
    ACTIVE_RESOLVED_CACHE.with(|c| c.set(cache as *const _));
    ResolvedCacheGuard { prev }
}

/// Return the currently-active [`ResolvedClassCache`], if any.
///
/// Returns `None` when no [`with_active_resolved_class_cache`] guard
/// is alive on this thread.
///
/// # Safety
///
/// The returned reference borrows the cache through a raw pointer set
/// by [`with_active_resolved_class_cache`].  This is safe because the
/// pointer is only non-null while the [`ResolvedCacheGuard`] (which
/// holds a borrow of the original `&ResolvedClassCache`) is alive.
pub fn active_resolved_class_cache() -> Option<&'static ResolvedClassCache> {
    let ptr = ACTIVE_RESOLVED_CACHE.with(|c| c.get());
    if ptr.is_null() {
        None
    } else {
        // SAFETY: ptr was set from a valid &ResolvedClassCache reference
        // and the ResolvedCacheGuard that owns the borrow is still alive
        // (it clears the pointer on drop).
        Some(unsafe { &*ptr })
    }
}

/// Evict all cache entries whose FQN matches the given name, then
/// transitively evict any cached class that depends on the evicted
/// FQN through `parent_class`, `used_traits`, `interfaces`, or
/// `mixins`.
///
/// Because the cache is keyed by `(FQN, generic_args)`, a single FQN
/// may have multiple entries (one per distinct generic instantiation).
/// This helper removes all of them, which is used during targeted
/// cache invalidation when a class definition changes.
///
/// Transitive eviction is necessary because a cached child class
/// (e.g. `ChildJob extends ScheduledJob`) embeds fully-merged members
/// from its parent.  When the parent's `@property` docblock changes,
/// the child's cache entry still holds the old inherited property and
/// must be discarded.
pub fn evict_fqn(
    cache: &mut HashMap<ResolvedClassCacheKey, Arc<ClassInfo>>,
    fqn: &str,
) -> Vec<String> {
    // Fast path: nothing to evict from an empty cache.
    if cache.is_empty() {
        return vec![];
    }

    // Remove direct matches for this FQN (any generic-arg variant).
    let before = cache.len();
    cache.retain(|(k, _), _| k != fqn);
    let removed_direct = cache.len() < before;

    // If the FQN wasn't in the cache, do a single pass to check
    // whether any remaining entry depends on it.  When nothing
    // depends on it either (the overwhelmingly common case during
    // bulk loading), skip the expensive transitive loop entirely.
    if !removed_direct {
        let seed = [fqn.to_string()];
        let has_dependent = cache.values().any(|cls| depends_on_any(cls, &seed));
        if !has_dependent {
            return vec![];
        }
    }

    // Transitive eviction: repeatedly scan for classes whose
    // inheritance chain references any already-evicted FQN.
    let mut evicted: Vec<String> = vec![fqn.to_string()];

    loop {
        let mut newly_evicted: Vec<String> = Vec::new();

        for ((cached_fqn, _), cls) in cache.iter() {
            let cached_fqn_str = cached_fqn.to_string();
            if depends_on_any(cls, &evicted) && !evicted.contains(&cached_fqn_str) {
                newly_evicted.push(cached_fqn_str);
            }
        }

        if newly_evicted.is_empty() {
            break;
        }

        for dep_fqn in &newly_evicted {
            cache.retain(|(k, _), _| k != dep_fqn);
            evicted.push(dep_fqn.clone());
        }
    }

    evicted
}

/// Check whether `cls` directly depends on any FQN in `fqns` through
/// its `parent_class`, `used_traits`, `interfaces`, `mixins`, or
/// `casts_definitions`.
///
/// Comparisons are done against both the raw field value and the short
/// name of the evicted FQN, because the cached `ClassInfo` may store
/// parent/trait/interface names as short names (same-file references)
/// or as FQNs (cross-file, post-resolution).
///
/// The `casts_definitions` check ensures that when a cast class is
/// edited (e.g. its `@implements CastsAttributes<T>` annotation
/// changes), models referencing that cast class via `$casts` are
/// transitively evicted from the resolved-class cache.
fn depends_on_any(cls: &ClassInfo, fqns: &[String]) -> bool {
    for fqn in fqns {
        let short = crate::util::short_name(fqn);

        // parent_class
        if let Some(ref parent) = cls.parent_class
            && (parent == fqn || parent == short)
        {
            return true;
        }

        // used_traits
        if cls.used_traits.iter().any(|t| t == fqn || t == short) {
            return true;
        }

        // interfaces
        if cls.interfaces.iter().any(|i| i == fqn || i == short) {
            return true;
        }

        // mixins
        if cls.mixins.iter().any(|m| m == fqn || m == short) {
            return true;
        }

        // casts_definitions — cast type values may reference class FQNs
        // (e.g. `"App\\Casts\\DecimalCast"` or `"DecimalCast:8:2"`).
        // Strip the `:argument` suffix before comparing.
        if let Some(laravel) = cls.laravel()
            && laravel.casts_definitions.iter().any(|(_, cast_type)| {
                let class_part = cast_type.split(':').next().unwrap_or(cast_type);
                class_part == fqn || class_part == short
            })
        {
            return true;
        }
    }
    false
}

/// Members synthesized by a provider.
///
/// Merged below real declared members, traits, and the parent chain.
/// Each provider returns a `VirtualMembers` value from its
/// [`provide`](VirtualMemberProvider::provide) method.
pub struct VirtualMembers {
    /// Virtual methods to add to the class.
    pub methods: Vec<MethodInfo>,
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
    /// `cache` is the shared resolved-class cache.  Providers that need
    /// to fully resolve helper classes (e.g. the Laravel model provider
    /// resolving the Eloquent Builder) should use
    /// [`resolve_class_fully_cached`] via this cache to avoid redundant
    /// work across requests.
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
/// **Exception:** when the existing method has `has_scope_attribute: true`,
/// the virtual method **replaces** it.  `#[Scope]`-attributed methods
/// share their name with the synthesized scope method, but the original
/// is a `protected` implementation detail that should not appear in
/// completion results.  The virtual replacement is `public` with the
/// first `$query` parameter stripped, which is what callers actually see.
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
/// `mixed` from `$fillable`.
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
    let mut method_index: HashMap<(String, bool), usize> = class
        .methods
        .iter()
        .enumerate()
        .map(|(i, m)| ((m.name.to_string(), m.is_static), i))
        .collect();

    for method in virtual_members.methods {
        let key = (method.name.to_string(), method.is_static);
        if let Some(&idx) = method_index.get(&key) {
            if class.methods[idx].has_scope_attribute
                || matches!(method.name.as_str(), "query" | "newQuery" | "newModelQuery")
            {
                // Replace the original with the synthesized virtual method.
                // For scope attributes, the original is an implementation detail.
                // For query methods, we want to return the custom builder.
                class.methods.make_mut()[idx] = Arc::new(method);
            }
            // Otherwise: real declared member — keep the original.
        } else {
            let new_idx = class.methods.len();
            class.methods.push(Arc::new(method));
            method_index.insert(key, new_idx);
        }
    }

    // Build a property name → position index for O(1) dedup.
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
            if property_type_specificity(&property)
                > property_type_specificity(&class.properties[idx])
            {
                class.properties.make_mut()[idx] = property;
            }
        } else {
            let new_idx = class.properties.len();
            prop_index.insert(property.name.to_string(), new_idx);
            class.properties.push(property);
        }
    }
    let mut const_names: HashSet<String> =
        class.constants.iter().map(|c| c.name.to_string()).collect();
    for constant in virtual_members.constants {
        if const_names.insert(constant.name.to_string()) {
            class.constants.push(constant);
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
    match hint {
        None => 0,
        Some(t) if t.is_mixed() => 0,
        Some(PhpType::Raw(s)) if s.trim().is_empty() => 0,
        Some(t) if t.has_type_structure() => 2,
        Some(_) => 1,
    }
}

/// Score a property's type by how specific it is, considering both
/// native and effective type hints.
///
/// The function first checks the effective type hint (docblock override),
/// then falls back to the native type hint if the effective type is
/// absent or non-specific.
///
/// This ensures that properties with actual PHP type declarations
/// (e.g., `public string $name`) are ranked higher than those without
/// any type information, even when docblocks are absent.
fn property_type_specificity(property: &PropertyInfo) -> u8 {
    // First check the effective type hint (may include docblock override)
    let effective_score = type_specificity(&property.type_hint);

    // If effective type is specific enough, use it
    if effective_score > 0 {
        return effective_score;
    }

    // Otherwise, fall back to native type hint
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
pub fn default_providers() -> Vec<Box<dyn VirtualMemberProvider>> {
    vec![
        // Laravel model provider — relationship properties, scopes, Builder
        // forwarding, convention-based factory() method.
        Box::new(laravel::LaravelModelProvider),
        // Laravel factory provider — convention-based create()/make() methods
        // for factory classes extending Illuminate\Database\Eloquent\Factories\Factory.
        Box::new(laravel::LaravelFactoryProvider),
        // PHPDoc provider — @method / @property / @mixin tags.
        Box::new(phpdoc::PHPDocProvider),
    ]
}

// ─── Recursion guard ────────────────────────────────────────────────────────
//
// Thread-local depth counter and set of class FQNs currently being
// resolved by `resolve_class_fully_inner` on this thread.  When a class
// is already in the set, re-entrant calls return a partial result (base
// inheritance only, no virtual members or interface merging) instead of
// recursing.  The depth counter provides an additional safety net: when
// nesting exceeds `MAX_RESOLVE_DEPTH`, resolution bails out regardless
// of the per-class guard.

/// Maximum nesting depth for `resolve_class_fully_inner` before
/// returning a partial (base-only) result.  This is a safety net in
/// addition to the per-class `RESOLVING` guard.
const MAX_RESOLVE_DEPTH: u32 = 30;

thread_local! {
    static RESOLVING: std::cell::RefCell<HashSet<String>> =
        std::cell::RefCell::new(HashSet::new());
    static RESOLVE_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Mark a class as being resolved.  Returns `true` if it was freshly
/// inserted (caller should proceed), `false` if it was already present
/// (caller should bail to avoid infinite recursion).
fn mark_resolving(fqn: &str) -> bool {
    RESOLVING.with(|set| set.borrow_mut().insert(fqn.to_string()))
}

/// Remove a class from the resolving set when resolution is complete.
fn unmark_resolving(fqn: &str) {
    RESOLVING.with(|set| set.borrow_mut().remove(fqn));
}

// ─── Eager class population ─────────────────────────────────────────────────

/// Pre-populate the [`ResolvedClassCache`] from a pre-computed
/// topologically sorted list of class FQNs.
///
/// Because the topological sort guarantees that all dependencies of a
/// class are processed before the class itself, each call to
/// [`resolve_class_fully_inner`] finds its parents, traits, interfaces,
/// and mixins already in the cache.  Provider callbacks (which call
/// `resolve_class_fully` for related classes) hit the cache immediately
/// instead of recursing, eliminating the unbounded mutual recursion that
/// previously caused stack overflow on large codebases.
///
/// This function should be called once after initial indexing is complete
/// (all files parsed into `uri_classes_index`) and again incrementally when files
/// change (see ER4 in `docs/todo/eager-resolution.md`).
///
/// # Arguments
///
/// * `sorted_fqns` — class FQNs in topological (dependency-first) order,
///   e.g. from [`crate::toposort::toposort_from_uri_classes_index`].
/// * `cache` — the [`ResolvedClassCache`] to populate (typically
///   `Backend.resolved_class_cache`).
/// * `class_loader` — a closure that resolves a class FQN to its raw
///   (un-merged) `ClassInfo`.  Typically `Backend::class_loader` or a
///   closure over `fqn_index` + `find_or_load_class`.
pub fn populate_from_sorted(
    sorted_fqns: &[String],
    cache: &ResolvedClassCache,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) {
    for fqn in sorted_fqns {
        // Skip classes already in the cache (e.g. from a previous
        // incremental population run).
        let cache_key: ResolvedClassCacheKey = (atom(fqn), Vec::new());
        {
            let map = cache.lock();
            if map.contains_key(&cache_key) {
                continue;
            }
        }

        // Load the raw (un-merged) class.  If the class_loader cannot
        // find it (e.g. it was removed between toposort and now), skip.
        let raw_class = match class_loader(fqn) {
            Some(c) => c,
            None => continue,
        };

        // resolve_class_fully_inner will:
        // 1. Check cache (miss for this class, but hits for deps)
        // 2. Run resolve_class_with_inheritance (parent chain walk —
        //    parents are raw, but the walk is bounded by MAX_INHERITANCE_DEPTH)
        // 3. Run apply_virtual_members — providers call resolve_class_fully
        //    for related classes, which hit the cache (already populated)
        // 4. Merge interfaces — also hit cache for resolved interfaces
        // 5. Store result in cache
        resolve_class_fully_inner(&raw_class, class_loader, Some(cache), &[]);
    }
}

// ─── Full class resolution ──────────────────────────────────────────────────

/// Resolve a class with full inheritance and virtual member providers.
///
/// This is the primary entry point for completion, go-to-definition,
/// and any other feature that needs the complete set of members
/// visible on a class instance or static access.
///
/// The resolution proceeds in two phases:
///
/// 1. **Base resolution** via
///    [`resolve_class_with_inheritance`]: merges own members, trait
///    members, and parent chain members, applying generic type
///    substitution along the way.
///
/// 2. **Virtual member providers**: queries each registered provider
///    in priority order and merges their contributions.  Virtual
///    members never overwrite real declared members or contributions
///    from higher-priority providers.
///
/// Code that needs only the base resolution (e.g. providers
/// themselves, to avoid circular loading) should call
/// [`resolve_class_with_inheritance`] directly.
pub fn resolve_class_fully(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Arc<ClassInfo> {
    resolve_class_fully_inner(class, class_loader, None, &[])
}

/// Cached variant of [`resolve_class_fully`].
///
/// Identical semantics, but stores and retrieves results from `cache`
/// so that repeated resolutions of the same class within a single
/// request cycle (or across requests between edits) are free.
///
/// The cache is keyed by the class's fully-qualified name
/// (`namespace\ClassName` or just `ClassName` for the global namespace).
/// Callers that apply post-resolution transforms (e.g.
/// [`apply_generic_args`](crate::inheritance::apply_generic_args)) should
/// still call this function for the base resolution and apply the
/// transform to the returned value.
pub fn resolve_class_fully_cached(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: &ResolvedClassCache,
) -> Arc<ClassInfo> {
    resolve_class_fully_inner(class, class_loader, Some(cache), &[])
}

/// Resolve a class fully, using the cache when available.
///
/// This is the preferred entry point for code paths that may or may
/// not have access to a [`ResolvedClassCache`] (e.g. context structs
/// where the cache field is `Option<&ResolvedClassCache>`).  When
/// `cache` is `Some`, behaves like [`resolve_class_fully_cached`];
/// when `None`, behaves like [`resolve_class_fully`].
pub fn resolve_class_fully_maybe_cached(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
) -> Arc<ClassInfo> {
    let started = std::time::Instant::now();
    let result = resolve_class_fully_inner(class, class_loader, cache, &[]);
    let elapsed = started.elapsed();
    if elapsed >= std::time::Duration::from_millis(50) {
        tracing::info!(
            "PHPantom: resolve_class_fully_cached({}) took {:?}",
            class.fqn(),
            elapsed
        );
    }
    result
}

/// Resolve a class fully and apply generic type argument substitution,
/// caching the combined result under `(FQN, generic_arg_strings)`.
///
/// For generic classes like `Builder<Product>`, calling
/// [`resolve_class_fully_maybe_cached`] followed by
/// [`apply_generic_args`](crate::inheritance::apply_generic_args) is
/// expensive because `apply_generic_args` clones the entire class and
/// substitutes template parameters in every method and property.  On
/// an Eloquent model with hundreds of virtual members this takes
/// milliseconds per call, and a large service file can trigger it
/// hundreds of times for the same `(FQN, generic_args)` pair.
///
/// This function fuses the two steps and caches the result so the
/// substitution is performed at most once per `(FQN, generic_args)`.
/// When `generic_args` is empty or the class has no template
/// parameters, this behaves identically to
/// [`resolve_class_fully_maybe_cached`].
pub fn resolve_class_fully_with_generics(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
    generic_arg_strings: &[String],
    generic_args: &[crate::php_type::PhpType],
) -> Arc<ClassInfo> {
    // Fast path: no generics — just do the base resolution.
    if generic_args.is_empty() {
        return resolve_class_fully_inner(class, class_loader, cache, &[]);
    }

    // Check the cache for (FQN, generic_args).
    let fqn = class.fqn();
    let cache_key: ResolvedClassCacheKey = (fqn, generic_arg_strings.to_vec());

    if let Some(c) = cache {
        let map = c.lock();
        if let Some(cached) = map.get(&cache_key) {
            return Arc::clone(cached);
        }
    }

    let started = std::time::Instant::now();
    // Resolve the base class (cached at (FQN, [])).
    let base = resolve_class_fully_inner(class, class_loader, cache, &[]);

    // Apply generic substitution.
    let result = if !base.template_params.is_empty() {
        Arc::new(crate::inheritance::apply_generic_args(&base, generic_args))
    } else {
        base
    };

    // Store the substituted result.
    if let Some(c) = cache {
        c.lock().insert(cache_key, Arc::clone(&result));
    }

    let elapsed = started.elapsed();
    if elapsed >= std::time::Duration::from_millis(50) {
        tracing::info!(
            "PHPantom: resolve_class_fully_with_generics({}, {:?}) took {:?}",
            class.fqn(),
            generic_arg_strings,
            elapsed
        );
    }

    result
}

/// Resolve a class fully and apply generic type arguments, deriving the
/// cache key strings from the structured [`PhpType`] arguments.
///
/// Use this on hot paths that already have concrete type arguments and would
/// otherwise call `resolve_class_fully_maybe_cached` followed by
/// `apply_generic_args`. The substituted result is cached under
/// `(FQN, generic_args)` while the base resolved class is still shared under
/// `(FQN, [])`.
pub fn resolve_class_fully_with_type_args(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
    generic_args: &[crate::php_type::PhpType],
) -> Arc<ClassInfo> {
    if generic_args.is_empty() {
        return resolve_class_fully_inner(class, class_loader, cache, &[]);
    }

    let generic_arg_strings: Vec<String> = generic_args.iter().map(|arg| arg.to_string()).collect();
    resolve_class_fully_with_generics(
        class,
        class_loader,
        cache,
        &generic_arg_strings,
        generic_args,
    )
}

/// Shared implementation behind [`resolve_class_fully`] and
/// [`resolve_class_fully_cached`].
fn resolve_class_fully_inner(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: Option<&ResolvedClassCache>,
    generic_args: &[String],
) -> Arc<ClassInfo> {
    let fqn = class.fqn();
    let cache_key: ResolvedClassCacheKey = (fqn, generic_args.to_vec());

    // ── Reload raw class from class_loader ──────────────────────────
    // Callers sometimes pass a ClassInfo that has already been through
    // `apply_generic_args` (e.g. `MockBuilder<Rule>` where template
    // parameters are substituted with concrete types).  If we resolve
    // from that substituted class and cache the result under the bare
    // FQN key `(MockBuilder, [])`, every subsequent lookup gets the
    // contaminated version — causing non-deterministic diagnostics
    // depending on which thread/file was processed first (B28).
    //
    // To prevent this, always try to reload the raw (un-substituted)
    // class from the class_loader.  The class_loader returns the
    // parsed ClassInfo from uri_classes_index/stubs, which has unsubstituted
    // template parameters.  Fall back to the passed-in `class` only
    // when the loader cannot find it (e.g. anonymous classes).
    let raw_class_arc = class_loader(fqn.as_str());
    let effective_class: &ClassInfo = match &raw_class_arc {
        Some(raw) => raw,
        None => class,
    };

    // ── Cache lookup ────────────────────────────────────────────────
    if let Some(cache) = cache {
        let map = cache.lock();
        if let Some(cached) = map.get(&cache_key) {
            return Arc::clone(cached);
        }
    }

    // ── Recursion guard ─────────────────────────────────────────────
    // If this class is already being resolved on this thread (re-entrant
    // call from a virtual member provider or interface merge), or we have
    // exceeded the maximum nesting depth, return a partial result with
    // base inheritance only.  This breaks the mutual recursion that
    // causes stack overflow.
    let depth = RESOLVE_DEPTH.with(|d| d.get());
    if depth >= MAX_RESOLVE_DEPTH || !mark_resolving(&fqn) {
        // Already resolving or too deep — return base-only resolution.
        return Arc::new(resolve_class_with_inheritance(
            effective_class,
            class_loader,
        ));
    }
    RESOLVE_DEPTH.with(|d| d.set(depth + 1));

    // ── Uncached resolution ─────────────────────────────────────────
    let mut merged = resolve_class_with_inheritance(effective_class, class_loader);

    // ── Pre-provider patches ────────────────────────────────────────
    // Inject missing `@mixin` annotations before virtual member
    // providers run, so that `collect_mixin_members` picks them up.
    if fqn.as_str() == "Illuminate\\Redis\\Connections\\Connection" {
        let mixin = atom("Redis");
        if !merged.mixins.contains(&mixin) {
            merged.mixins.push(mixin);
        }
    }
    if fqn.as_str() == "Illuminate\\Database\\Eloquent\\Builder" {
        let mixin = atom("Illuminate\\Database\\Query\\Builder");
        if !merged.mixins.contains(&mixin) {
            merged.mixins.push(mixin);
        }
    }

    let providers = default_providers();
    if !providers.is_empty() {
        apply_virtual_members(&mut merged, class_loader, &providers, cache);
    }

    // 3. Merge members from implemented interfaces.
    //    Interfaces can declare `@method` / `@property` / `@property-read`
    //    tags that should be visible on implementing classes.  We collect
    //    interfaces from the class itself and from every parent in the
    //    extends chain, then fully resolve each interface (which applies
    //    its own virtual member providers) and merge any members that
    //    don't already exist.
    //
    //    When a class declares `@implements SomeInterface<ConcreteType>`,
    //    the interface's template parameters are substituted with the
    //    concrete types before merging.  This mirrors how `@extends`
    //    generics are handled in the parent chain walk.  Substitutions
    //    from `@implements` on parent classes are also collected, with
    //    the `@extends` chain substitutions applied so that template
    //    parameters from intermediate classes resolve correctly.
    let mut all_iface_names: Vec<String> = effective_class
        .interfaces
        .iter()
        .map(|a| a.to_string())
        .collect();

    // Collect all `@implements` generics from the class and its parent
    // chain.  As we walk up the `extends` chain we apply the active
    // substitution map so that template parameter references in parent
    // `@implements` annotations resolve to concrete types.
    //
    // For example, given:
    //   class Test1<TKey> implements MyIterator<TKey, string>
    //   class Test2 extends Test1<int>
    //
    // Walking from Test2: active_subs starts empty, then after loading
    // Test1 we get {TKey => int}.  Test1's `@implements MyIterator<TKey, string>`
    // becomes `@implements MyIterator<int, string>` after substitution.
    let mut all_implements_generics: Vec<(String, Vec<PhpType>)> = effective_class
        .implements_generics
        .iter()
        .map(|(a, v)| (a.to_string(), v.clone()))
        .collect();
    {
        let mut current: ClassRef<'_> = ClassRef::Borrowed(effective_class);
        let mut depth = 0u32;
        let mut active_subs: HashMap<String, PhpType> = HashMap::new();

        // Seed initial subs from the root class's @extends generics
        // so that if the root class itself has template params referenced
        // in its @implements, they can be resolved.

        while let Some(ref parent_name) = current.parent_class {
            depth += 1;
            if depth > 20 {
                break;
            }
            if let Some(parent) = class_loader(parent_name) {
                // Build the substitution map for this parent level,
                // mirroring the logic in resolve_class_with_inheritance.
                let parent_short = short_name(&parent.name);
                let type_args = current
                    .extends_generics
                    .iter()
                    .chain(current.implements_generics.iter())
                    .find(|(name, _)| short_name(name) == parent_short)
                    .map(|(_, args)| args);

                let mut level_subs = if let Some(args) = type_args {
                    let mut map = HashMap::new();
                    for (i, param_name) in parent.template_params.iter().enumerate() {
                        if let Some(arg) = args.get(i) {
                            let resolved = if active_subs.is_empty() {
                                arg.clone()
                            } else {
                                arg.substitute(&active_subs)
                            };
                            map.insert(param_name.to_string(), resolved);
                        }
                    }
                    map
                } else {
                    active_subs.clone()
                };

                // If no explicit @extends generics matched but there are
                // active subs, carry them forward.
                if level_subs.is_empty() && !active_subs.is_empty() {
                    level_subs = active_subs.clone();
                }

                for iface in &parent.interfaces {
                    let iface_str = iface.to_string();
                    if !all_iface_names.contains(&iface_str) {
                        all_iface_names.push(iface_str);
                    }
                }

                // Collect parent's @implements generics with substitutions
                // applied so that template params resolve to concrete types.
                for (iface_name, args) in &parent.implements_generics {
                    let resolved_args: Vec<PhpType> = if level_subs.is_empty() {
                        args.clone()
                    } else {
                        args.iter().map(|a| a.substitute(&level_subs)).collect()
                    };
                    all_implements_generics.push((iface_name.to_string(), resolved_args));
                }

                active_subs = level_subs;
                current = ClassRef::Owned(parent);
            } else {
                break;
            }
        }
    }

    // Use an index-based loop so that we can grow `all_iface_names`
    // while iterating — each resolved interface may itself extend
    // additional interfaces that need to be collected transitively.
    let mut iface_idx = 0;
    while iface_idx < all_iface_names.len() {
        let iface_name = all_iface_names[iface_idx].clone();
        iface_idx += 1;

        if let Some(iface) = class_loader(&iface_name) {
            // Collect interfaces that this interface itself extends.
            // For example, CarbonInterface extends DateTimeInterface,
            // JsonSerializable, UnitValue — all of those need to be
            // resolved and their members merged transitively.
            for child_iface in &iface.interfaces {
                let child_iface_str = child_iface.to_string();
                if !all_iface_names.contains(&child_iface_str) {
                    all_iface_names.push(child_iface_str);
                }
            }

            // Build a substitution map from `@implements` generics for
            // this interface.  If the class (or a parent) declared
            // `@implements ThisInterface<Type1, Type2>`, map the
            // interface's template params to those concrete types.
            let mut iface_subs =
                build_implements_substitution_map(&iface_name, &iface, &all_implements_generics);

            // When no @implements generics were provided but the interface
            // declares template parameters, substitute each template param
            // with its declared upper bound (or `mixed`).  Without this,
            // raw template names like `TKey` / `TValue` leak through
            // inherited method signatures into downstream consumers.
            if iface_subs.is_empty() && !iface.template_params.is_empty() {
                for param in &iface.template_params {
                    let bound = iface
                        .template_param_bounds
                        .get(param)
                        .cloned()
                        .unwrap_or_else(PhpType::mixed);
                    iface_subs.insert(param.to_string(), bound);
                }
            }

            // Collect @extends / @implements generics from the
            // interface so that template substitutions flow through
            // transitive interface chains.  Apply the substitution map
            // we just built so that template params like `T` in
            // `@extends IteratorAggregate<T>` are resolved to concrete
            // types (e.g. `ReflectionArgument`) before propagation.
            for (name, args) in &iface.extends_generics {
                if !all_implements_generics.iter().any(|(n, _)| n == name) {
                    let resolved_args: Vec<PhpType> = if iface_subs.is_empty() {
                        args.clone()
                    } else {
                        args.iter().map(|a| a.substitute(&iface_subs)).collect()
                    };
                    all_implements_generics.push((name.to_string(), resolved_args));
                }
            }
            for (name, args) in &iface.implements_generics {
                if !all_implements_generics
                    .iter()
                    .any(|(n, _)| n.as_str() == name.as_str())
                {
                    let resolved_args: Vec<PhpType> = if iface_subs.is_empty() {
                        args.clone()
                    } else {
                        args.iter().map(|a| a.substitute(&iface_subs)).collect()
                    };
                    all_implements_generics.push((name.to_string(), resolved_args));
                }
            }

            // When we have substitutions to apply, we cannot use a
            // cached bare-interface resolution because the cached version
            // has unsubstituted template parameters.  Only use the cache
            // for interfaces without generic substitutions.
            if iface_subs.is_empty() {
                let iface_key: ResolvedClassCacheKey = (iface.fqn(), Vec::new());
                if let Some(c) = cache {
                    let map = c.lock();
                    if let Some(cached) = map.get(&iface_key) {
                        let resolved_iface = ClassInfo::clone(cached);
                        drop(map);
                        merge_interface_members_into(&mut merged, resolved_iface, &iface_subs);
                        continue;
                    }
                }
            }

            let mut resolved_iface = resolve_class_with_inheritance(&iface, class_loader);
            if !providers.is_empty() {
                apply_virtual_members(&mut resolved_iface, class_loader, &providers, cache);
            }

            merge_interface_members_into(&mut merged, resolved_iface, &iface_subs);
        }
    }

    // Store the accumulated `@implements` generics (with `@extends`
    // chain substitutions applied) on the merged class so that
    // downstream consumers like foreach resolution can see generics
    // from parent classes too.  For example, when `Test2 extends
    // Test1<int>` and `Test1` has `@implements MyIterator<TKey, string>`,
    // the merged Test2 class gets `implements_generics` containing
    // `("MyIterator", ["int", "string"])`.
    for (name, args) in &all_implements_generics {
        if !merged
            .implements_generics
            .iter()
            .any(|(n, _)| n.as_str() == name.as_str())
        {
            merged.implements_generics.push((atom(name), args.clone()));
        }
    }

    // ── Laravel class patches ──────────────────────────────────────
    // Apply centralized post-resolution patches for Laravel classes.
    // These modify existing members' type information (e.g. fixing
    // return types) rather than adding new members.  See
    // `laravel/patches.rs` for the full patch inventory.
    apply_laravel_patches(&mut merged, &fqn);

    // ── Cache store ─────────────────────────────────────────────────
    // ── Remove from recursion guard ─────────────────────────────────
    unmark_resolving(&fqn);
    RESOLVE_DEPTH.with(|d| d.set(depth));

    merged.rebuild_method_index();
    let result = Arc::new(merged);
    if let Some(cache) = cache {
        cache.lock().insert(cache_key, Arc::clone(&result));
    }

    result
}

/// Merge resolved interface members into a class, applying `@implements`
/// generic substitutions.
///
/// For methods and properties that already exist on the class, this fills
/// in missing type information from the interface declaration.  When a
/// class declares `boo()` with no return type but the interface has
/// `@return Y`, the substituted interface return type is applied to the
/// class method.  Similarly, parameter docblock types from the interface
/// are applied when the class parameter lacks a type hint or has a
/// less-specific native hint (e.g. `object`) while the interface provides
/// a concrete docblock type.
///
/// Members that don't already exist on the class are added directly.
fn merge_interface_members_into(
    merged: &mut ClassInfo,
    mut resolved_iface: ClassInfo,
    iface_subs: &HashMap<String, PhpType>,
) {
    // Apply @implements generic substitutions to the resolved
    // interface members before merging.
    if !iface_subs.is_empty() {
        for method in resolved_iface.methods.make_mut().iter_mut() {
            apply_substitution_to_method(Arc::make_mut(method), iface_subs);
        }
        for property in resolved_iface.properties.make_mut().iter_mut() {
            apply_substitution_to_property(property, iface_subs);
        }
    }

    // For small method counts, linear scan is cheaper than building a
    // HashMap (avoids N String allocations for the keys).  The threshold
    // is chosen so that the HashMap overhead is amortised by the number
    // of interface methods that need lookup.
    const HASHMAP_THRESHOLD: usize = 32;

    let method_index: Option<HashMap<String, usize>> = if merged.methods.len() >= HASHMAP_THRESHOLD
    {
        Some(
            merged
                .methods
                .iter()
                .enumerate()
                .map(|(i, m)| (m.name.to_string(), i))
                .collect(),
        )
    } else {
        None
    };

    for iface_method in resolved_iface.methods.into_vec() {
        // Find the existing method index — O(1) via HashMap or O(N) linear scan.
        let existing_idx = if let Some(ref index) = method_index {
            index.get(&iface_method.name.to_string()).copied()
        } else {
            merged
                .methods
                .iter()
                .position(|m| m.name == iface_method.name)
        };

        if let Some(idx) = existing_idx {
            let existing = &mut merged.methods.make_mut()[idx];
            // Delegate to the shared enrichment helper which handles
            // return types, parameters, descriptions, template params,
            // conditional returns, and type assertions uniformly.
            enrich_method_from_ancestor(Arc::make_mut(existing), &iface_method);
        } else {
            merged.methods.push(iface_method);
        }
    }
    // Merge interface properties — enrich existing ones, add new ones.
    for property in resolved_iface.properties.into_vec() {
        if let Some(existing) = merged
            .properties
            .make_mut()
            .iter_mut()
            .find(|p| p.name == property.name)
        {
            enrich_property_from_ancestor(existing, &property);
        } else {
            merged.properties.push(property);
        }
    }
    let existing_consts: HashSet<String> = merged
        .constants
        .iter()
        .map(|c| c.name.to_string())
        .collect();
    for constant in resolved_iface.constants.into_vec() {
        if !existing_consts.contains(&constant.name.to_string()) {
            merged.constants.push(constant);
        }
    }
}

/// Build a substitution map for an interface based on collected
/// `@implements` generics.
///
/// Searches `all_implements_generics` for an entry whose class name
/// matches `iface_name` (by FQN comparison), then zips the
/// type arguments with the interface's `template_params`.
///
/// Returns an empty map if no matching `@implements` annotation exists
/// or if the interface has no template parameters.
fn build_implements_substitution_map(
    iface_name: &str,
    iface: &ClassInfo,
    all_implements_generics: &[(String, Vec<PhpType>)],
) -> HashMap<String, PhpType> {
    if iface.template_params.is_empty() || all_implements_generics.is_empty() {
        return HashMap::new();
    }

    let type_args = all_implements_generics
        .iter()
        .find(|(name, _)| name == iface_name)
        .map(|(_, args)| args);

    let type_args = match type_args {
        Some(args) => args,
        None => return HashMap::new(),
    };

    let mut map = HashMap::new();
    for (i, param_name) in iface.template_params.iter().enumerate() {
        if let Some(arg) = type_args.get(i) {
            map.insert(param_name.to_string(), arg.clone());
        }
    }
    map
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
