//! PHPDoc virtual member provider.
//!
//! Extracts `@method`, `@property` / `@property-read` / `@property-write`,
//! and `@mixin` tags from the class-level docblock and presents them as
//! virtual members.  This is the second-highest-priority virtual member
//! provider: framework providers (e.g. Laravel) take precedence, but
//! PHPDoc-sourced members beat all other virtual member sources.
//!
//! Within this provider, `@method` and `@property` tags take precedence
//! over `@mixin` members: if a class declares both `@property int $id`
//! and `@mixin SomeClass` where `SomeClass` also has an `$id` property,
//! the `@property` tag wins.
//!
//! Previously `@method` / `@property` and `@mixin` were handled by two
//! separate providers (`PHPDocProvider` and `MixinProvider`).  Since both
//! are driven by PHPDoc tags, they are now unified into a single provider
//! with internal precedence rules.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::atom::{Atom, atom};
use crate::docblock;
use crate::inheritance;
use crate::inheritance::ClassRef;
use crate::php_type::PhpType;
use crate::types::{
    ClassInfo, ConstantInfo, MAX_INHERITANCE_DEPTH, MAX_MIXIN_DEPTH, MethodInfo, PropertyInfo,
    Visibility,
};
use crate::util::short_name;

/// Global generation counter, incremented every time a file is re-parsed.
/// Thread-local caches compare against this to detect staleness.
static MIXIN_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Bump the mixin-cache generation so that all threads discard stale entries
/// on their next access.  Called from [`Backend::update_ast`] whenever a file
/// changes.
pub fn bump_mixin_generation() {
    MIXIN_GENERATION.fetch_add(1, Ordering::Relaxed);
}

thread_local! {
    /// Thread-local cache of base-resolved mixin classes.
    ///
    /// Keyed by fully-qualified mixin name, stores the result of
    /// [`resolve_class_with_inheritance`](crate::inheritance::resolve_class_with_inheritance)
    /// so that expensive inheritance walks (e.g. for
    /// `\Illuminate\Database\Eloquent\Builder`) are performed at most
    /// once per thread.
    ///
    /// Automatically invalidated when the global generation counter
    /// advances (i.e. when any file is re-parsed).
    static MIXIN_CACHE: RefCell<(u64, HashMap<String, Arc<ClassInfo>>)> =
        RefCell::new((0, HashMap::new()));
}

/// Clear the thread-local mixin resolution cache.
///
/// In production the cache lives for the lifetime of the thread and is
/// safe because the same FQN always maps to the same class.  In tests,
/// however, each test may define classes with identical short names but
/// different members.  Call this function when creating a new test
/// backend so that stale entries from a previous test do not leak.
pub fn clear_mixin_cache() {
    MIXIN_CACHE.with(|cache| {
        let mut inner = cache.borrow_mut();
        inner.0 = MIXIN_GENERATION.load(Ordering::Relaxed);
        inner.1.clear();
    });
}

/// Ensure the thread-local cache is current with the global generation.
/// Clears the cache if stale.
fn ensure_mixin_cache_fresh() {
    MIXIN_CACHE.with(|cache| {
        let current_gen = MIXIN_GENERATION.load(Ordering::Relaxed);
        let mut inner = cache.borrow_mut();
        if inner.0 != current_gen {
            inner.0 = current_gen;
            inner.1.clear();
        }
    });
}

/// Tracks member names already seen during mixin collection.
///
/// Accumulates mixin members during collection, grouping the output
/// vectors and dedup sets into a single value to keep the argument
/// count of [`collect_mixin_members`] within clippy's limit.
struct MixinCollector {
    methods: Vec<MethodInfo>,
    properties: Vec<PropertyInfo>,
    constants: Vec<ConstantInfo>,
    dedup: MixinDedup,
}

/// Passed through [`collect_mixin_members`] (including recursive calls)
/// so that every addition is checked in O(1) instead of scanning the
/// accumulated vectors and base class members.
struct MixinDedup {
    /// Method names from the base class + accumulated virtual methods.
    methods: HashSet<String>,
    /// Property names from the base class + accumulated virtual properties.
    properties: HashSet<String>,
    /// Constant names from the base class + accumulated virtual constants.
    constants: HashSet<String>,
}

/// The substitution environment for a single [`collect_mixin_members`] level.
///
/// Groups the two maps used to resolve a `@mixin` name that is a template
/// parameter into a concrete class, keeping the argument count of
/// [`collect_mixin_members`] within clippy's limit.
struct MixinSubs<'a> {
    /// Concrete type per template param (from generic arguments provided by
    /// a subclass via `@extends`/`@mixin` generics).  Checked first.
    subs: &'a HashMap<String, PhpType>,
    /// Upper bound per template param (from `@template T of Bound`).  Used
    /// as a fallback when no concrete type is bound, so a `@mixin T` still
    /// resolves members through the constraint.
    bounds: &'a crate::atom::AtomMap<PhpType>,
}

use super::{VirtualMemberProvider, VirtualMembers};

/// Virtual member provider for `@method`, `@property`, and `@mixin` docblock tags.
///
/// When a class declares `@method` or `@property` tags in its class-level
/// docblock, those tags describe magic members accessible via `__call`,
/// `__get`, and `__set`.  When a class declares `@mixin ClassName`, all
/// public members of `ClassName` (and its inheritance chain) become
/// available via magic methods.
///
/// Resolution order within this provider:
/// 1. `@method` and `@property` tags (highest precedence)
/// 2. `@mixin` class members (lower precedence, never overwrite tags)
///
/// Mixins are inherited: if `User extends Model` and `Model` has
/// `@mixin Builder`, then `User` also gains Builder's public members.
/// The provider walks the parent chain to collect mixin declarations
/// from ancestors.
///
/// Mixin classes can themselves declare `@mixin`, so the provider
/// recurses up to [`MAX_MIXIN_DEPTH`] levels.
pub struct PHPDocProvider;

impl VirtualMemberProvider for PHPDocProvider {
    /// Returns `true` if the class has a non-empty class-level docblock
    /// or declares `@mixin` tags (directly or via ancestors).
    ///
    /// This is a cheap pre-check. No parsing is performed.
    fn applies_to(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> bool {
        // Has a non-empty docblock with potential @method/@property tags.
        if class.class_docblock.as_ref().is_some_and(|d| !d.is_empty()) {
            return true;
        }

        // Has used traits that might have @method/@property tags.
        for trait_name in &class.used_traits {
            if let Some(trait_info) = class_loader(trait_name)
                && trait_info
                    .class_docblock
                    .as_ref()
                    .is_some_and(|d| !d.is_empty())
            {
                return true;
            }
        }

        // Has direct @mixin declarations.
        if !class.mixins.is_empty() {
            return true;
        }

        // Walk the parent chain to check for ancestor mixins or docblocks
        // with @method/@property tags.  Use a cheap Arc handle instead of
        // cloning the entire ClassInfo at each level.
        let mut current_parent = class.parent_class;
        let mut depth = 0u32;
        while let Some(ref parent_name) = current_parent {
            depth += 1;
            if depth > MAX_INHERITANCE_DEPTH {
                break;
            }
            let parent = if let Some(p) = class_loader(parent_name) {
                p
            } else {
                break;
            };
            if !parent.mixins.is_empty() {
                return true;
            }
            if parent
                .class_docblock
                .as_ref()
                .is_some_and(|d| !d.is_empty())
            {
                return true;
            }
            current_parent = parent.parent_class;
        }

        false
    }

    /// Parse `@method`, `@property`, and `@mixin` tags from the class.
    ///
    /// Uses the existing [`docblock::extract_method_tags`] and
    /// [`docblock::extract_property_tags`] functions for tag parsing.
    /// Then collects public members from `@mixin` classes.  Within the
    /// provider, `@method` / `@property` tags take precedence over
    /// `@mixin` members.
    fn provide(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        cache: Option<&super::ResolvedClassCache>,
    ) -> VirtualMembers {
        let mut methods = Vec::new();
        let mut properties = Vec::new();
        let constants = Vec::new();

        // Dedup sets for O(1) membership checks.  Seeded from the
        // base-resolved class members (real + inherited) and updated
        // as virtual members are collected.
        //
        // `seen_props` is NOT seeded from existing class properties.
        // Phase 1 (`@property` tags) always emits its properties so
        // that `merge_virtual_members` can compare type specificity
        // and keep the most specific type (e.g. `array<string>` from
        // `@property` beats bare `array` from `$casts`).  After
        // phase 1 emits, names are added to `seen_props` to prevent
        // lower-priority sources (trait tags, parent tags, `@mixin`
        // members) from overriding them.
        let mut seen_methods: HashSet<String> =
            class.methods.iter().map(|m| m.name.to_string()).collect();
        let mut seen_props: HashSet<String> = HashSet::new();
        let seen_consts: HashSet<String> =
            class.constants.iter().map(|c| c.name.to_string()).collect();

        // ── Phase 1: @method and @property tags (higher precedence) ─────

        if let Some(doc_text) = class.class_docblock.as_deref()
            && !doc_text.is_empty()
        {
            for m in docblock::extract_method_tags(doc_text) {
                seen_methods.insert(m.name.to_string());
                methods.push(m);
            }

            for (name, type_hint) in docblock::extract_property_tags(doc_text) {
                seen_props.insert(name.clone());
                properties.push(PropertyInfo {
                    name: atom(&name),
                    name_offset: 0,
                    type_hint,
                    native_type_hint: None,
                    description: None,
                    is_static: false,
                    visibility: Visibility::Public,
                    deprecation_message: None,
                    deprecated_replacement: None,
                    see_refs: Vec::new(),
                    is_virtual: true,
                });
            }
        }

        // ── Phase 1b: @method and @property tags from used traits ───────
        //
        // When a class uses a trait that declares `@method` or `@property`
        // tags in its docblock, those virtual members should propagate to
        // the consuming class.  Real trait methods are already merged by
        // `merge_traits_into`, but virtual members from docblock tags are
        // not — they only exist as text in the trait's `class_docblock`.
        for trait_name in &class.used_traits {
            let trait_info = if let Some(t) = class_loader(trait_name) {
                t
            } else {
                continue;
            };

            if let Some(doc_text) = trait_info.class_docblock.as_deref()
                && !doc_text.is_empty()
            {
                for m in docblock::extract_method_tags(doc_text) {
                    if seen_methods.insert(m.name.to_string()) {
                        methods.push(m);
                    }
                }

                for (name, type_hint) in docblock::extract_property_tags(doc_text) {
                    if seen_props.insert(name.clone()) {
                        properties.push(PropertyInfo {
                            name: atom(&name),
                            name_offset: 0,
                            type_hint,
                            native_type_hint: None,
                            description: None,
                            is_static: false,
                            visibility: Visibility::Public,
                            deprecation_message: None,
                            deprecated_replacement: None,
                            see_refs: Vec::new(),
                            is_virtual: true,
                        });
                    }
                }
            }
        }

        // ── Phase 1c: @method and @property tags from parent classes ────
        //
        // When a parent class declares `@method` or `@property` tags in
        // its docblock, those virtual members should be visible on child
        // classes.  Real inherited methods are already merged by
        // `resolve_class_with_inheritance`, but virtual members from
        // docblock tags are not — they only exist as text in the parent's
        // `class_docblock`.  Walk the parent chain and collect them.
        //
        // Template substitutions from `@extends` annotations are applied
        // so that `@method T get()` on a parent with `@template T` is
        // resolved to the concrete type when the child declares
        // `@extends Parent<ConcreteType>`.
        {
            let mut current: ClassRef<'_> = ClassRef::Borrowed(class);
            let mut active_subs: HashMap<String, PhpType> = HashMap::new();
            let mut depth = 0u32;

            while let Some(ref parent_name) = current.parent_class {
                depth += 1;
                if depth > MAX_INHERITANCE_DEPTH {
                    break;
                }
                let parent = if let Some(p) = class_loader(parent_name) {
                    p
                } else {
                    break;
                };

                // Build a substitution map for this parent level from
                // the child's `@extends` generics.
                let level_subs = build_mixin_substitution_map(
                    &current,
                    &parent,
                    &active_subs,
                    &class.template_param_bounds,
                );

                if let Some(doc_text) = parent.class_docblock.as_deref()
                    && !doc_text.is_empty()
                {
                    for mut m in docblock::extract_method_tags(doc_text) {
                        if seen_methods.insert(m.name.to_string()) {
                            if !level_subs.is_empty() {
                                inheritance::apply_substitution_to_method(&mut m, &level_subs);
                            }
                            methods.push(m);
                        }
                    }

                    for (name, type_hint) in docblock::extract_property_tags(doc_text) {
                        if seen_props.insert(name.clone()) {
                            let resolved_type = if !level_subs.is_empty() {
                                type_hint.map(|t| t.substitute(&level_subs))
                            } else {
                                type_hint
                            };
                            properties.push(PropertyInfo {
                                name: atom(&name),
                                name_offset: 0,
                                type_hint: resolved_type,
                                native_type_hint: None,
                                description: None,
                                is_static: false,
                                visibility: Visibility::Public,
                                deprecation_message: None,
                                deprecated_replacement: None,
                                see_refs: Vec::new(),
                                is_virtual: true,
                            });
                        }
                    }
                }

                active_subs = level_subs;
                current = ClassRef::Owned(parent);
            }
        }

        // ── Phase 1d: @method and @property tags from implemented interfaces ─
        //
        // When a class implements an interface that declares `@method` or
        // `@property` tags, those virtual members should be visible on the
        // implementing class.  Template substitutions from `@implements`
        // annotations are applied so that `@method E get()` on an interface
        // with `@template E` is resolved to the concrete type when the class
        // declares `@implements I<ConcreteType>`.
        //
        // We also walk each interface's parent interfaces (via `interfaces`
        // field, which stores `extends` for interfaces).
        {
            let mut iface_queue: Vec<(Atom, HashMap<String, PhpType>)> = Vec::new();

            // Seed with the class's own interfaces, building substitution
            // maps from `@implements` generics.
            for iface_name in &class.interfaces {
                if let Some(iface) = class_loader(iface_name) {
                    let subs = build_interface_substitution_map(class, &iface);
                    iface_queue.push((*iface_name, subs));
                }
            }

            let mut visited: HashSet<Atom> = HashSet::new();
            while let Some((iface_name, subs)) = iface_queue.pop() {
                if !visited.insert(iface_name) {
                    continue;
                }
                let iface = if let Some(i) = class_loader(&iface_name) {
                    i
                } else {
                    continue;
                };

                if let Some(doc_text) = iface.class_docblock.as_deref()
                    && !doc_text.is_empty()
                {
                    for mut m in docblock::extract_method_tags(doc_text) {
                        if seen_methods.insert(m.name.to_string()) {
                            if !subs.is_empty() {
                                inheritance::apply_substitution_to_method(&mut m, &subs);
                            }
                            methods.push(m);
                        }
                    }

                    for (name, type_hint) in docblock::extract_property_tags(doc_text) {
                        if seen_props.insert(name.clone()) {
                            let resolved_type = if !subs.is_empty() {
                                type_hint.map(|t| t.substitute(&subs))
                            } else {
                                type_hint
                            };
                            properties.push(PropertyInfo {
                                name: atom(&name),
                                name_offset: 0,
                                type_hint: resolved_type,
                                native_type_hint: None,
                                description: None,
                                is_static: false,
                                visibility: Visibility::Public,
                                deprecation_message: None,
                                deprecated_replacement: None,
                                see_refs: Vec::new(),
                                is_virtual: true,
                            });
                        }
                    }
                }

                // Walk parent interfaces (interface extends).
                for parent_iface_name in &iface.interfaces {
                    if let Some(parent_iface) = class_loader(parent_iface_name) {
                        let parent_subs =
                            build_interface_extends_substitution_map(&iface, &parent_iface, &subs);
                        iface_queue.push((*parent_iface_name, parent_subs));
                    }
                }
            }
        }

        // ── Phase 2: @mixin members (lower precedence) ─────────────────

        let mixin_dedup = MixinDedup {
            methods: seen_methods,
            properties: seen_props,
            constants: seen_consts,
        };

        let mut collector = MixinCollector {
            methods,
            properties,
            constants,
            dedup: mixin_dedup,
        };

        // Collect from the class's own mixins.
        //
        // No template substitutions are available at this stage because
        // the concrete generic arguments for the class itself are applied
        // later by `apply_generic_args`.  Template-param mixin names
        // (e.g. `@mixin TWraps`) on the own class are resolved during
        // the ancestor walk when a child class provides concrete types
        // via `@extends`.
        collect_mixin_members(
            &class.mixins,
            &class.mixin_generics,
            class_loader,
            &mut collector,
            &MixinSubs {
                subs: &HashMap::new(),
                bounds: &class.template_param_bounds,
            },
            0,
            cache,
        );

        // Collect from ancestor mixins.
        //
        // As we walk the parent chain we accumulate a substitution map
        // (template-param → concrete-type) so that mixin generic
        // arguments that reference a parent's template params are
        // resolved to concrete types.  For example, when
        // `BelongsTo extends Relation<Product>` and `Relation` has
        // `@mixin Builder<TRelatedModel>`, the walk builds
        // `{TRelatedModel → Product}` from the child's `@extends`
        // generics and applies it to the mixin's generic args, turning
        // `Builder<TRelatedModel>` into `Builder<Product>`.
        let mut current_ancestor: ClassRef<'_> = ClassRef::Borrowed(class);
        let mut active_subs: HashMap<String, PhpType> = HashMap::new();
        let mut depth = 0u32;
        while let Some(ref parent_name) = current_ancestor.parent_class {
            depth += 1;
            if depth > MAX_INHERITANCE_DEPTH {
                break;
            }
            let parent = if let Some(p) = class_loader(parent_name) {
                p
            } else {
                break;
            };

            // Build the substitution map for this parent level,
            // analogous to `build_substitution_map` in inheritance.rs.
            let level_subs = build_mixin_substitution_map(
                &current_ancestor,
                &parent,
                &active_subs,
                &class.template_param_bounds,
            );

            if !parent.mixins.is_empty() {
                // Apply the accumulated substitution map to the
                // parent's mixin generic arguments so that template
                // param names are replaced with concrete types.
                let resolved_mixin_generics: Vec<(Atom, Vec<PhpType>)> = if level_subs.is_empty() {
                    parent.mixin_generics.clone()
                } else {
                    parent
                        .mixin_generics
                        .iter()
                        .map(|(name, args)| {
                            let resolved_args: Vec<PhpType> =
                                args.iter().map(|arg| arg.substitute(&level_subs)).collect();
                            (*name, resolved_args)
                        })
                        .collect()
                };

                collect_mixin_members(
                    &parent.mixins,
                    &resolved_mixin_generics,
                    class_loader,
                    &mut collector,
                    &MixinSubs {
                        subs: &level_subs,
                        bounds: &parent.template_param_bounds,
                    },
                    0,
                    cache,
                );
            }
            active_subs = level_subs;
            current_ancestor = ClassRef::Owned(parent);
        }

        VirtualMembers {
            methods: collector.methods,
            properties: collector.properties,
            constants: collector.constants,
        }
    }
}

/// Recursively collect public members from mixin classes.
///
/// For each mixin name, loads the class via `class_loader`, resolves its
/// full inheritance chain (via [`crate::inheritance::resolve_class_with_inheritance`]),
/// and adds its public members to the output vectors.  Only members whose
/// names are not already present in `class` (the target class with base
/// resolution already applied) or in the output vectors are added.
/// This means `@method` / `@property` tags collected before this function
/// is called take precedence over mixin members.
///
/// Recurses into mixins declared on the mixin classes themselves, up to
/// [`MAX_MIXIN_DEPTH`] levels.
///
/// Uses a thread-local cache so that `resolve_class_with_inheritance` is
/// called at most once per unique mixin FQN across all `provide` calls
/// within the same thread.  Without this cache, a mixin like
/// `\Illuminate\Database\Eloquent\Builder` was fully re-resolved for
/// every Eloquent model class (very expensive: deep inheritance chain
/// with dozens of traits).
fn collect_mixin_members(
    mixin_names: &[Atom],
    mixin_generics: &[(Atom, Vec<PhpType>)],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    collector: &mut MixinCollector,
    subs: &MixinSubs<'_>,
    depth: u32,
    cache: Option<&super::ResolvedClassCache>,
) {
    if depth > MAX_MIXIN_DEPTH {
        return;
    }

    let template_subs = subs.subs;

    for mixin_name in mixin_names {
        // If the mixin name is a template parameter, substitute it
        // with the concrete type from the generic arguments.
        let resolved_mixin_name = if let Some(concrete) = template_subs.get(mixin_name.as_str()) {
            if let Some(base) = concrete.base_name() {
                base.to_string()
            } else {
                // The concrete type is a scalar, union, or other
                // non-class type — cannot be used as a mixin.
                continue;
            }
        } else if let Some(bound) = subs.bounds.get(mixin_name) {
            // The mixin name is a template parameter with no concrete
            // binding (e.g. `@mixin TNode` on a class declaring
            // `@template TNode of Engine`).  Resolve members through the
            // template's upper bound so the class itself still exposes the
            // bound's public API.  A concrete subclass that provides a
            // real binding via `@extends` overrides this through the
            // substitution path above.
            if let Some(base) = bound.base_name() {
                base.to_string()
            } else {
                // The bound is a scalar, union, or other non-class type.
                continue;
            }
        } else {
            mixin_name.to_string()
        };

        let mixin_class = if let Some(c) = class_loader(&resolved_mixin_name) {
            c
        } else {
            continue;
        };

        // Find generic args for this mixin from the @mixin tag.
        // Check both the original name (e.g. "TWraps") and the resolved
        // name in case the mixin_generics were stored under either form.
        let mixin_short = short_name(&resolved_mixin_name);
        let generic_args: Option<&[PhpType]> = mixin_generics
            .iter()
            .find(|(name, _)| {
                name == mixin_name
                    || short_name(name) == mixin_short
                    || name == &resolved_mixin_name
            })
            .map(|(_, args)| args.as_slice());

        // Resolve the mixin class *fully* so that its own virtual members
        // (Laravel relationship properties, scopes, casts, accessors, and
        // `@method` / `@property` tags) are exposed on the consuming class,
        // not just its real declared members.  A `@mixin` proxies the whole
        // public API via magic methods, so a model's synthesized members
        // must come through as well.
        //
        // The re-entrancy guard in `resolve_class_fully_inner` breaks any
        // cyclic `@mixin` chain by returning a base-only result on re-entry,
        // so this cannot recurse unboundedly.  Eager resolution populates the
        // shared cache in dependency order (mixin targets before their
        // dependents), so on the warm path this is a cache hit.
        //
        // Prefer the caller-supplied cache, falling back to the thread-local
        // active cache so consumers that reach this path through the uncached
        // `resolve_class_fully` still share resolved results.
        let resolved_mixin = if let Some(c) = cache {
            // The shared cache memoizes the full resolution, so resolve
            // directly and let it serve repeat lookups.
            super::resolve_class_fully_maybe_cached(&mixin_class, class_loader, Some(c))
        } else if let Some(active) = super::active_resolved_class_cache() {
            super::resolve_class_fully_maybe_cached(&mixin_class, class_loader, Some(active))
        } else {
            // No shared cache is active — memoize per thread so that a deep
            // mixin (e.g. the Eloquent query builder) is fully resolved at
            // most once per thread.  The full resolution runs virtual member
            // providers, which recurse back into this function for nested
            // mixins, so it must happen *outside* the cache borrow: get,
            // release, resolve, then insert.
            ensure_mixin_cache_fresh();
            let cached = MIXIN_CACHE
                .with(|thread_cache| thread_cache.borrow().1.get(&resolved_mixin_name).cloned());
            if let Some(cached) = cached {
                cached
            } else {
                let resolved =
                    super::resolve_class_fully_maybe_cached(&mixin_class, class_loader, None);
                MIXIN_CACHE.with(|thread_cache| {
                    thread_cache
                        .borrow_mut()
                        .1
                        .insert(resolved_mixin_name.clone(), Arc::clone(&resolved));
                });
                resolved
            }
        };

        // Build a substitution map from the mixin class's template params
        // to the concrete types provided in the @mixin tag's generic args.
        let subs: HashMap<String, PhpType> = if let Some(args) = generic_args {
            let mut map = HashMap::new();
            for (i, param_name) in mixin_class.template_params.iter().enumerate() {
                if let Some(arg) = args.get(i) {
                    map.insert(param_name.to_string(), arg.clone());
                }
            }
            map
        } else {
            HashMap::new()
        };

        // Known values for the mixin class's template parameters: explicit
        // `@mixin Foo<...>` generic args, falling back to each param's
        // declared default (`@template T of bool = false`).  A bare
        // `@mixin Foo` therefore behaves like `@mixin Foo<default>`, which
        // lets conditional return types keyed on those params (e.g.
        // `(TAsync is false ? Response : PromiseInterface)`) collapse to a
        // concrete branch instead of defaulting to the else type.
        let mut template_values = subs.clone();
        for (name, default) in mixin_class.template_param_defaults.iter() {
            template_values
                .entry(name.to_string())
                .or_insert_with(|| default.clone());
        }

        // Only merge public members — mixins proxy via magic methods
        // which only expose public API.
        for method in &resolved_mixin.methods {
            if method.visibility != Visibility::Public {
                continue;
            }
            // Skip if the base-resolved class already has this method,
            // or if a previous @method tag or mixin already contributed it.
            if !collector.dedup.methods.insert(method.name.to_string()) {
                continue;
            }
            let mut method = (**method).clone();
            if !subs.is_empty() {
                inheritance::apply_substitution_to_method(&mut method, &subs);
            }
            // Collapse a conditional return type keyed on one of the mixin
            // class's template params now that their values are known (from
            // generic args or defaults).  Without this, the conditional's
            // subject template is unresolvable at the call site — the mixin
            // origin is lost once the method is merged into the consumer —
            // and resolution falls back to the else branch.
            if !template_values.is_empty()
                && let Some(cond) = method.conditional_return.as_ref()
                && let Some(resolved) =
                    crate::completion::conditional_resolution::resolve_conditional_from_values(
                        cond,
                        &template_values,
                    )
            {
                method.return_type = Some(resolved);
                method.conditional_return = None;
            }
            // `@return $this` / `self` / `static` in mixin methods are
            // left as-is.  When the method is later called on the
            // consuming class, `$this` resolves to the consumer (not the
            // mixin), which is the correct semantic: fluent chains
            // continue with the consumer's full API (own methods + all
            // mixin methods).  In the builder-as-static forwarding path,
            // the substitution map rewrites `$this` to
            // `\Illuminate\Database\Eloquent\Builder<Model>`, so the
            // return type must still be the raw keyword at this stage.
            method.is_virtual = true;
            collector.methods.push(method);
        }

        for property in &resolved_mixin.properties {
            if property.visibility != Visibility::Public {
                continue;
            }
            if !collector.dedup.properties.insert(property.name.to_string()) {
                continue;
            }
            let mut property = property.clone();
            if !subs.is_empty() {
                inheritance::apply_substitution_to_property(&mut property, &subs);
            }
            property.is_virtual = true;
            collector.properties.push(property);
        }

        for constant in &resolved_mixin.constants {
            if constant.visibility != Visibility::Public {
                continue;
            }
            if !collector.dedup.constants.insert(constant.name.to_string()) {
                continue;
            }
            collector.constants.push(constant.clone());
        }

        // ── Phase: @method/@property tags from the mixin's own docblock ──
        // `resolve_class_with_inheritance` does NOT include virtual members
        // from @method/@property tags (to avoid circular provider calls).
        // Extract them manually so that e.g. `@mixin A` where A declares
        // `@method $this active()` propagates `active()` to the consumer.
        if let Some(doc_text) = mixin_class.class_docblock.as_deref()
            && !doc_text.is_empty()
        {
            for mut m in docblock::extract_method_tags(doc_text) {
                if !collector.dedup.methods.insert(m.name.to_string()) {
                    continue;
                }
                if !subs.is_empty() {
                    inheritance::apply_substitution_to_method(&mut m, &subs);
                }
                m.is_virtual = true;
                collector.methods.push(m);
            }

            for (name, type_hint) in docblock::extract_property_tags(doc_text) {
                if !collector.dedup.properties.insert(name.clone()) {
                    continue;
                }
                let resolved_type = if !subs.is_empty() {
                    type_hint.map(|t| t.substitute(&subs))
                } else {
                    type_hint
                };
                collector.properties.push(PropertyInfo {
                    name: atom(&name),
                    name_offset: 0,
                    type_hint: resolved_type,
                    native_type_hint: None,
                    description: None,
                    is_static: false,
                    visibility: Visibility::Public,
                    deprecation_message: None,
                    deprecated_replacement: None,
                    see_refs: Vec::new(),
                    is_virtual: true,
                });
            }
        }

        // Recurse into mixins declared by the mixin class itself.
        if !mixin_class.mixins.is_empty() {
            collect_mixin_members(
                &mixin_class.mixins,
                &mixin_class.mixin_generics,
                class_loader,
                collector,
                &MixinSubs {
                    subs: &HashMap::new(),
                    bounds: &mixin_class.template_param_bounds,
                },
                depth + 1,
                cache,
            );
        }
    }
}

/// Resolve `@mixin` tags that name a template parameter, using concrete
/// generic arguments provided at a call site.
///
/// During [`PHPDocProvider::provide`], mixin names that are template
/// parameters (e.g. `@mixin TWraps`) cannot be resolved because the
/// concrete type arguments are not yet known — they are applied later
/// by [`apply_generic_args`](crate::inheritance::apply_generic_args).
/// This function fills that gap: after generic substitution has been
/// performed, call it with the **original** (unsubstituted) class and
/// the substitution map to collect members from the now-concrete mixin
/// classes.
///
/// Only mixins whose names match a template parameter are processed;
/// non-template mixins were already resolved during `provide`.
///
/// The returned [`VirtualMembers`](super::VirtualMembers) should be
/// merged into the substituted class via
/// [`merge_virtual_members`](super::merge_virtual_members).
pub fn resolve_template_param_mixins(
    original_class: &ClassInfo,
    template_subs: &HashMap<String, PhpType>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> super::VirtualMembers {
    if template_subs.is_empty() || original_class.mixins.is_empty() {
        return super::VirtualMembers {
            methods: Vec::new(),
            properties: Vec::new(),
            constants: Vec::new(),
        };
    }

    // Only process mixins whose name is a template parameter — the
    // rest were already resolved during `PHPDocProvider::provide`.
    let template_mixins: Vec<Atom> = original_class
        .mixins
        .iter()
        .filter(|m| {
            original_class
                .template_params
                .iter()
                .any(|t| t.as_str() == m.as_str())
        })
        .copied()
        .collect();

    if template_mixins.is_empty() {
        return super::VirtualMembers {
            methods: Vec::new(),
            properties: Vec::new(),
            constants: Vec::new(),
        };
    }

    let dedup = MixinDedup {
        methods: HashSet::new(),
        properties: HashSet::new(),
        constants: HashSet::new(),
    };

    let mut collector = MixinCollector {
        methods: Vec::new(),
        properties: Vec::new(),
        constants: Vec::new(),
        dedup,
    };

    collect_mixin_members(
        &template_mixins,
        &original_class.mixin_generics,
        class_loader,
        &mut collector,
        &MixinSubs {
            subs: template_subs,
            bounds: &original_class.template_param_bounds,
        },
        0,
        None,
    );

    super::VirtualMembers {
        methods: collector.methods,
        properties: collector.properties,
        constants: collector.constants,
    }
}

/// Build a substitution map for mixin generic resolution by zipping the
/// parent class's `@template` parameters with the type arguments provided
/// Build a substitution map for a directly implemented interface.
///
/// Maps the interface's template parameters to the concrete types provided
/// in the class's `@implements` generics.
fn build_interface_substitution_map(
    class: &ClassInfo,
    iface: &ClassInfo,
) -> HashMap<String, PhpType> {
    if iface.template_params.is_empty() {
        return HashMap::new();
    }

    let iface_short = short_name(&iface.name);

    let type_args = class
        .implements_generics
        .iter()
        .find(|(name, _)| short_name(name) == iface_short)
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

/// Build a substitution map for an interface's parent interface (interface extends).
///
/// Maps the parent interface's template parameters to concrete types by
/// resolving through the child interface's `@extends` generics and applying
/// the already-accumulated substitutions.
fn build_interface_extends_substitution_map(
    child_iface: &ClassInfo,
    parent_iface: &ClassInfo,
    active_subs: &HashMap<String, PhpType>,
) -> HashMap<String, PhpType> {
    if parent_iface.template_params.is_empty() {
        return active_subs.clone();
    }

    let parent_short = short_name(&parent_iface.name);

    let type_args = child_iface
        .extends_generics
        .iter()
        .find(|(name, _)| short_name(name) == parent_short)
        .map(|(_, args)| args);

    let type_args = match type_args {
        Some(args) => args,
        None => return active_subs.clone(),
    };

    let mut map = HashMap::new();
    for (i, param_name) in parent_iface.template_params.iter().enumerate() {
        if let Some(arg) = type_args.get(i) {
            let resolved = if active_subs.is_empty() {
                arg.clone()
            } else {
                arg.substitute(active_subs)
            };
            map.insert(param_name.to_string(), resolved);
        }
    }
    map
}

/// This mirrors [`crate::inheritance::build_substitution_map`] but is
/// scoped to the virtual-member provider so it does not need to be public
/// on the inheritance module.
fn build_mixin_substitution_map(
    current: &ClassInfo,
    parent: &ClassInfo,
    active_subs: &HashMap<String, PhpType>,
    origin_bounds: &crate::atom::AtomMap<PhpType>,
) -> HashMap<String, PhpType> {
    if parent.template_params.is_empty() {
        return active_subs.clone();
    }

    let parent_short = short_name(&parent.name);

    // Find `@extends`/`@implements` generics matching this parent.
    let type_args = current
        .extends_generics
        .iter()
        .chain(current.implements_generics.iter())
        .find(|(name, _)| {
            let name_short = short_name(name);
            name_short == parent_short
        })
        .map(|(_, args)| args);

    let type_args = match type_args {
        Some(args) => args,
        None => return active_subs.clone(),
    };

    // Check whether the parent has any @mixin whose name is itself a
    // template parameter (e.g. `@mixin TNode` on a class with
    // `@template TNode`).  When this is the case and a substitution
    // still resolves to a raw template parameter name on the child
    // class, we fall back to the template bound.  This handles the
    // PHPMD pattern where `AbstractNode<TNode>` has `@mixin TNode`
    // and `ASTNode extends AbstractNode<TNode>` — without the
    // fallback, `TNode` stays as an unresolvable class name.
    //
    // We do NOT apply this fallback when the mixin is a concrete
    // class with template arguments (e.g. `@mixin Builder<TModel>`),
    // because the template param may be resolved later by a concrete
    // caller through the generic substitution chain.
    let parent_has_template_param_mixin = parent.mixins.iter().any(|m| {
        parent
            .template_params
            .iter()
            .any(|t| t.as_str() == m.as_str())
    });

    let mut map = HashMap::new();
    for (i, param_name) in parent.template_params.iter().enumerate() {
        if let Some(arg) = type_args.get(i) {
            let mut resolved = if active_subs.is_empty() {
                arg.clone()
            } else {
                arg.substitute(active_subs)
            };

            // Fall back to the template bound only when the parent
            // uses the template param directly as a mixin name.
            //
            // Prefer the bound declared on the walk-origin class (the
            // most-derived class whose members are being resolved) over
            // the intermediate level's bound.  In a straight-through chain
            // (`@extends Parent<TNode>` at every level) each class may
            // tighten the constraint, e.g. `AbstractNode<TNode of ASTNode>`
            // → `CallableNode<TNode of AbstractCallable>`.  The mixin lives
            // on the ancestor with the loosest bound, but the concrete
            // members available come from the origin's tighter bound, so
            // that is the one to resolve against.
            if parent_has_template_param_mixin
                && let Some(name) = resolved.base_name()
                && let Some(tp) = current.template_params.iter().find(|t| t.as_str() == name)
                && let Some(bound) = origin_bounds
                    .get(tp)
                    .or_else(|| current.template_param_bounds.get(tp))
            {
                resolved = bound.clone();
            }

            map.insert(param_name.to_string(), resolved);
        }
    }

    map
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "phpdoc_tests.rs"]
mod tests;
