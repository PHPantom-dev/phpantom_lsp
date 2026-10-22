//! `@mixin` members: the public members of the classes a class, its
//! traits and its ancestors declare as mixins, including mixins named by a
//! template parameter and the decorated-forward rule of Laravel's
//! `ForwardsCalls`.

use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::{Atom, AtomSet};
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::inheritance;
use crate::inheritance::bind_template_args;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{
    ClassInfo, ConstantInfo, MAX_MIXIN_DEPTH, MethodInfo, PropertyInfo, Visibility,
};
use crate::util::short_name;

use super::{build_trait_substitution_map, doc_property};

/// Laravel's call-forwarding trait.  Classes that use it (Eloquent
/// `Builder`, `Relation`, …) implement the decorated-forward rule:
/// when a call is forwarded to another object and that object returns
/// itself, the forwarder returns `$this` instead.
const FORWARDS_CALLS_FQN: &str = "Illuminate\\Support\\Traits\\ForwardsCalls";

const FORWARDS_CALLS_SHORT: &str = "ForwardsCalls";

/// Accumulates mixin members during collection, grouping the output
/// vectors and dedup sets into a single value to keep the argument
/// count of [`collect_mixin_members`] within clippy's limit.
pub(super) struct MixinCollector {
    pub(super) methods: Vec<Arc<MethodInfo>>,
    pub(super) properties: Vec<PropertyInfo>,
    pub(super) constants: Vec<ConstantInfo>,
    pub(super) dedup: MixinDedup,
}

/// Tracks member names already seen during mixin collection.
///
/// Passed through [`collect_mixin_members`] (including recursive calls)
/// so that every addition is checked in O(1) instead of scanning the
/// accumulated vectors and base class members.
pub(super) struct MixinDedup {
    /// Method names from the base class + accumulated virtual methods.
    pub(super) methods: AtomSet,
    /// Property names from the base class + accumulated virtual properties.
    pub(super) properties: AtomSet,
    /// Constant names from the base class + accumulated virtual constants.
    pub(super) constants: AtomSet,
}

/// The substitution environment for a single [`collect_mixin_members`] level.
///
/// Groups the maps used to resolve a `@mixin` name that is a template
/// parameter into a concrete class, plus the ForwardsCalls flag, keeping
/// the argument count of [`collect_mixin_members`] within clippy's limit.
pub(super) struct MixinSubs<'a> {
    /// Concrete type per template param (from generic arguments provided by
    /// a subclass via `@extends`/`@mixin` generics).  Checked first.
    pub(super) subs: &'a HashMap<String, PhpType>,
    /// Upper bound per template param (from `@template T of Bound`).  Used
    /// as a fallback when no concrete type is bound, so a `@mixin T` still
    /// resolves members through the constraint.
    pub(super) bounds: &'a crate::atom::AtomMap<PhpType>,
    /// Whether the outermost consumer uses `ForwardsCalls` (decorated
    /// forward: target-self returns become the forwarder's `$this`).
    pub(super) decorated_forward: bool,
}

/// Whether `class` uses a trait (directly or through a trait it uses)
/// that declares `@mixin`.
pub(super) fn uses_trait_with_mixins(
    class: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    let mut queue: Vec<Atom> = class.used_traits.to_vec();
    let mut visited = AtomSet::default();
    while let Some(trait_name) = queue.pop() {
        if !visited.insert(trait_name) {
            continue;
        }
        let Some(trait_info) = class_loader(Fqn::new(&trait_name)) else {
            continue;
        };
        if !trait_info.mixins.is_empty() {
            return true;
        }
        queue.extend(trait_info.used_traits.iter().copied());
    }
    false
}

/// Collect the members of every `@mixin` `class` answers to: its own and
/// its traits', then each ancestor's and that ancestor's traits'.
///
/// No substitution applies to the class's own mixins: the class's generic
/// arguments are applied later by `apply_generic_args`, and a mixin named
/// by one of its template parameters (`@mixin TWraps`) is resolved during
/// the ancestor walk of a child that binds it through `@extends`.  The
/// `ancestors` (see [`template_bound_ancestors`]) carry those bindings, so
/// that when `BelongsTo extends Relation<Product>` and `Relation` declares
/// `@mixin Builder<TRelatedModel>`, the mixin comes out as
/// `Builder<Product>`.
pub(super) fn collect_class_mixin_members(
    class: &ClassInfo,
    ancestors: &[(Arc<ClassInfo>, HashMap<String, PhpType>)],
    class_loader: &ResolvedClassLoader<'_>,
    collector: &mut MixinCollector,
    cache: Option<&super::super::ResolvedClassCache>,
) {
    let decorated_forward = uses_forwards_calls(class, class_loader);
    collect_mixin_members(
        &class.mixins,
        &class.mixin_generics,
        class_loader,
        collector,
        &MixinSubs {
            subs: &HashMap::new(),
            bounds: &class.template_param_bounds,
            decorated_forward,
        },
        0,
        cache,
    );
    collect_trait_mixin_members(
        class,
        &HashMap::new(),
        class_loader,
        collector,
        decorated_forward,
        cache,
    );

    for (parent, level_subs) in ancestors {
        if !parent.mixins.is_empty() {
            collect_mixin_members(
                &parent.mixins,
                &substituted_mixin_generics(parent, level_subs),
                class_loader,
                collector,
                &MixinSubs {
                    subs: level_subs,
                    bounds: &parent.template_param_bounds,
                    decorated_forward,
                },
                0,
                cache,
            );
        }
        collect_trait_mixin_members(
            parent,
            level_subs,
            class_loader,
            collector,
            decorated_forward,
            cache,
        );
    }
}

/// Each parent of `class`, nearest first, with the bindings its template
/// parameters get from the `@extends` (and `@implements`) arguments of the
/// classes between it and `class`, accumulated level by level.
pub(super) fn template_bound_ancestors(
    class: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<(Arc<ClassInfo>, HashMap<String, PhpType>)> {
    let unbound = HashMap::new();
    let mut levels: Vec<(Arc<ClassInfo>, HashMap<String, PhpType>)> = Vec::new();
    for (_, parent) in inheritance::ancestors(class, class_loader) {
        let (child, child_subs) = match levels.last() {
            Some((child, child_subs)) => (&**child, child_subs),
            None => (class, &unbound),
        };
        let level_subs =
            build_mixin_substitution_map(child, &parent, child_subs, &class.template_param_bounds);
        levels.push((parent, level_subs));
    }
    levels
}

/// The generic arguments of `owner`'s `@mixin` declarations, with `subs`
/// replacing the template parameters they name.
fn substituted_mixin_generics(
    owner: &ClassInfo,
    subs: &HashMap<String, PhpType>,
) -> Vec<(Atom, Vec<PhpType>)> {
    if subs.is_empty() {
        return owner.mixin_generics.clone();
    }
    owner
        .mixin_generics
        .iter()
        .map(|(name, args)| (*name, args.iter().map(|arg| arg.substitute(subs)).collect()))
        .collect()
}

/// Collect the members of the `@mixin` classes declared on the traits
/// `owner` uses, and on the traits those traits use.
///
/// A trait's docblock describes the class that uses it, so its mixins
/// apply to that class like the class's own.  `owner_subs` binds
/// `owner`'s template parameters (non-empty when `owner` is an ancestor
/// reached through `@extends`); each trait's `@use Trait<…>` arguments
/// are layered on top of it.
pub(super) fn collect_trait_mixin_members(
    owner: &ClassInfo,
    owner_subs: &HashMap<String, PhpType>,
    class_loader: &ResolvedClassLoader<'_>,
    collector: &mut MixinCollector,
    decorated_forward: bool,
    cache: Option<&super::super::ResolvedClassCache>,
) {
    if owner.used_traits.is_empty() {
        return;
    }
    let mut queue: Vec<(Arc<ClassInfo>, HashMap<String, PhpType>)> = Vec::new();
    let mut visited = AtomSet::default();
    let mut enqueue =
        |consumer: &ClassInfo,
         subs: &HashMap<String, PhpType>,
         queue: &mut Vec<(Arc<ClassInfo>, HashMap<String, PhpType>)>| {
            for trait_name in &consumer.used_traits {
                if !visited.insert(*trait_name) {
                    continue;
                }
                if let Some(trait_info) = class_loader(Fqn::new(trait_name)) {
                    let trait_subs = build_trait_substitution_map(consumer, &trait_info, subs);
                    queue.push((trait_info, trait_subs));
                }
            }
        };
    enqueue(owner, owner_subs, &mut queue);
    // Walk in declaration order so an earlier `use` wins a name clash.
    queue.reverse();
    while let Some((trait_info, trait_subs)) = queue.pop() {
        if !trait_info.mixins.is_empty() {
            collect_mixin_members(
                &trait_info.mixins,
                &substituted_mixin_generics(&trait_info, &trait_subs),
                class_loader,
                collector,
                &MixinSubs {
                    subs: &trait_subs,
                    bounds: &trait_info.template_param_bounds,
                    decorated_forward,
                },
                0,
                cache,
            );
        }
        let mut nested = Vec::new();
        enqueue(&trait_info, &trait_subs, &mut nested);
        nested.reverse();
        queue.extend(nested);
    }
}

/// Recursively collect public members from mixin classes.
///
/// For each mixin name, loads the class via `class_loader`, fully resolves
/// it (via [`super::super::resolve_class_fully_maybe_cached`], so the mixin's own
/// virtual members come through too — see the inline comment below), and
/// adds its public members to the output vectors.  Only members whose
/// names are not already present in `class` (the target class with base
/// resolution already applied) or in the output vectors are added.
/// This means `@method` / `@property` tags collected before this function
/// is called take precedence over mixin members.
///
/// Recurses into mixins declared on the mixin classes themselves, up to
/// [`MAX_MIXIN_DEPTH`] levels.
///
/// Prefers the caller-supplied or thread-active [`super::super::ResolvedClassCache`]
/// so repeat lookups of the same mixin FQN are cache hits; when neither is
/// available, falls back to a thread-local cache so a mixin like
/// `\Illuminate\Database\Eloquent\Builder` is still resolved at most once
/// per thread instead of being fully re-resolved (very expensive: deep
/// inheritance chain with dozens of traits) for every Eloquent model class.
pub(super) fn collect_mixin_members(
    mixin_names: &[Atom],
    mixin_generics: &[(Atom, Vec<PhpType>)],
    class_loader: &ResolvedClassLoader<'_>,
    collector: &mut MixinCollector,
    subs: &MixinSubs<'_>,
    depth: u32,
    cache: Option<&super::super::ResolvedClassCache>,
) {
    if depth > MAX_MIXIN_DEPTH {
        return;
    }

    let template_subs = subs.subs;
    let decorated_forward = subs.decorated_forward;

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

        let mixin_class = if let Some(c) = class_loader(Fqn::new(&resolved_mixin_name)) {
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
            super::super::resolve_class_fully_maybe_cached(&mixin_class, class_loader, Some(c))
        } else if let Some(active) = super::super::active_resolved_class_cache() {
            super::super::resolve_class_fully_maybe_cached(&mixin_class, class_loader, Some(active))
        } else {
            // No shared cache is active: resolve against a throwaway one,
            // so the nested mixins this resolution reaches are each
            // resolved once within it.
            let scratch = super::super::cache::new_resolved_class_cache();
            super::super::resolve_class_fully_maybe_cached(
                &mixin_class,
                class_loader,
                Some(&scratch),
            )
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

        // The object a forwarded call actually runs on, which is what a
        // `static`/`self`/`$this` in one of the mixin method's parameters
        // describes (see `bind_param_self_refs`).
        let mixin_type = match generic_args {
            Some(args) if !args.is_empty() => PhpType::generic(&resolved_mixin_name, args.to_vec()),
            _ => PhpType::named(crate::atom::atom(&resolved_mixin_name)),
        };

        // Interning fingerprint for the transform applied below.  The
        // conditional-collapse inputs (`template_values`) are the subs
        // plus the mixin class's template defaults, and the
        // decorated-forward rewrite targets the mixin class — both are
        // fully determined by the origin method's class, so hashing the
        // subs and the decorated flag identifies the transform.
        let fp = super::super::cache::TransformFingerprint::new(
            Some(&subs),
            None,
            if decorated_forward {
                super::super::cache::transform_flags::MIXIN_VIRTUAL_DECORATED
            } else {
                super::super::cache::transform_flags::MIXIN_VIRTUAL
            },
        );

        // Only merge public members — mixins proxy via magic methods
        // which only expose public API.
        for method in &resolved_mixin.methods {
            if method.visibility != Visibility::Public {
                continue;
            }
            // Skip if the base-resolved class already has this method,
            // or if a previous @method tag or mixin already contributed it.
            if !collector.dedup.methods.insert(method.name) {
                continue;
            }
            let transformed = super::super::cache::intern_transformed_method(method, fp, || {
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
                    && let Some(cond) = method.conditional_return()
                    && let Some(resolved) =
                        crate::type_engine::conditional_resolution::resolve_conditional_from_values(
                            cond,
                            &template_values,
                        )
                {
                    method.return_type = Some(resolved);
                    method.extras_mut().conditional_return = None;
                }
                // A docblock's `self` names the class the docblock is written
                // in, so `@return self<T>` on the mixin returns the *mixin*
                // parameterised by `T`.  The consumer declares none of the
                // mixin's template parameters and so cannot carry those
                // arguments; leaving the keyword bare would read the return as
                // the consumer itself and silently drop them.  Only a `self`
                // that carries arguments is bound this way: a bare `self`,
                // `$this`, or `static` continues to bind late, to the consumer,
                // which is what keeps a fluent chain on the class it started on.
                if let Some(ret) = method.return_type.as_ref()
                    && returns_parameterised_self(ret)
                {
                    let bound = ret.replace_bare_self(&resolved_mixin_name);
                    method.return_type = Some(bound);
                }
                // Decorated forward (ForwardsCalls / forwardDecoratedCallTo):
                // when the target returns itself, the forwarder returns
                // `$this`.  Self-like returns are left as `$this`/`self`/
                // `static` so call-site resolution binds them to the
                // consumer (and preserves generics like `Builder<TModel>`).
                // Returns that name the mixin class FQN are rewritten to
                // `$this` for the same reason.  Non-self returns (int, bool,
                // Collection, …) pass through unchanged.
                //
                // Builder-as-static forwarding still needs the raw keyword
                // so its substitution map can rewrite `$this` →
                // `Builder<ConcreteModel>`.
                if decorated_forward {
                    apply_decorated_forward_return(&mut method, &resolved_mixin_name, &mixin_class);
                }
                bind_param_self_refs(&mut method, &mixin_type);
                method.is_virtual = true;
                method
            });
            collector.methods.push(transformed);
        }

        for property in &resolved_mixin.properties {
            if property.visibility != Visibility::Public {
                continue;
            }
            if !collector.dedup.properties.insert(property.name) {
                continue;
            }
            let mut property = (**property).clone();
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
            if !collector.dedup.constants.insert(constant.name) {
                continue;
            }
            collector.constants.push((**constant).clone());
        }

        // ── Phase: @method/@property tags from the mixin's own docblock ──
        // `resolve_class_with_inheritance` does NOT include virtual members
        // from @method/@property tags (to avoid circular provider calls).
        // Extract them manually so that e.g. `@mixin A` where A declares
        // `@method $this active()` propagates `active()` to the consumer.
        if let Some(doc) = mixin_class.doc_members.as_deref() {
            for m in &doc.methods {
                if !collector.dedup.methods.insert(m.name) {
                    continue;
                }
                let mut m = (**m).clone();
                if !subs.is_empty() {
                    inheritance::apply_substitution_to_method(&mut m, &subs);
                }
                if decorated_forward {
                    apply_decorated_forward_return(&mut m, &resolved_mixin_name, &mixin_class);
                }
                bind_param_self_refs(&mut m, &mixin_type);
                m.is_virtual = true;
                collector.methods.push(Arc::new(m));
            }

            for (name, type_hint) in &doc.properties {
                if !collector.dedup.properties.insert(*name) {
                    continue;
                }
                collector
                    .properties
                    .push(doc_property(*name, type_hint.as_ref(), &subs));
            }
        }

        // Recurse into mixins declared by the mixin class itself
        // (e.g. Relation → Builder → Query\Builder).  Keep the original
        // consumer's decorated-forward flag so nested target returns
        // still rewrite to the outermost forwarder.
        if !mixin_class.mixins.is_empty() {
            collect_mixin_members(
                &mixin_class.mixins,
                &mixin_class.mixin_generics,
                class_loader,
                collector,
                &MixinSubs {
                    subs: &HashMap::new(),
                    bounds: &mixin_class.template_param_bounds,
                    decorated_forward,
                },
                depth + 1,
                cache,
            );
        }
    }
}

/// Whether `class` (or an ancestor) uses Laravel's `ForwardsCalls` trait.
pub(in crate::virtual_members) fn uses_forwards_calls(
    class: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    let uses_it = |owner: &ClassInfo| {
        owner
            .used_traits
            .iter()
            .any(|t| is_forwards_calls_trait(t.as_str()))
    };
    uses_it(class)
        || inheritance::ancestors(class, class_loader).any(|(_, parent)| uses_it(&parent))
}

fn is_forwards_calls_trait(name: &str) -> bool {
    name == FORWARDS_CALLS_FQN
        || name == FORWARDS_CALLS_SHORT
        || short_name(name) == FORWARDS_CALLS_SHORT
}

/// Bind the `static`/`self`/`$this` in a mixed-in method's parameters to
/// the mixin class.
///
/// A mixin proxies the call to the object it names, so that object is what
/// the method runs on: `Relation::where(fn (Builder $q) => …)` reaches
/// `Builder::where(Closure(static) $column)` and the closure is handed the
/// builder, not the relation. The return type keeps its late binding, since
/// a forwarder that gets its target back returns itself.
fn bind_param_self_refs(method: &mut MethodInfo, mixin_type: &PhpType) {
    if !method
        .parameters
        .iter()
        .any(|p| p.type_hint.as_ref().is_some_and(PhpType::contains_self_ref))
    {
        return;
    }
    for param in method.parameters.make_mut() {
        if let Some(hint) = param.type_hint.as_ref()
            && hint.contains_self_ref()
        {
            param.type_hint = Some(hint.replace_self_with_type(mixin_type));
        }
    }
}

/// Whether a return type is a `self` that carries generic arguments
/// (`self<T>`), as opposed to a bare `self`, `$this`, or `static`.
fn returns_parameterised_self(return_type: &PhpType) -> bool {
    matches!(
        return_type.kind(),
        TypeKind::Generic(g) if !g.args.is_empty() && g.name.eq_ignore_ascii_case("self")
    )
}

/// Apply `forwardDecoratedCallTo` return semantics to a mixed-in method.
///
/// - Self-like (`$this` / `self` / `static`) → left as-is (call site binds
///   to the consumer, preserving generics).
/// - Return type whose base is the mixin class → rewritten to `$this`.
/// - Anything else (scalars, collections, models, …) → unchanged.
fn apply_decorated_forward_return(
    method: &mut MethodInfo,
    mixin_resolved_name: &str,
    mixin_class: &ClassInfo,
) {
    let Some(ret) = method.return_type.as_ref() else {
        return;
    };
    if ret.is_self_like() || ret.contains_self_ref() {
        return;
    }
    let mixin_fqn = mixin_class.fqn();
    let mixin_fqn = mixin_fqn.as_str();
    let mixin_short = mixin_class.name.as_str();
    let resolved_short = short_name(mixin_resolved_name);
    if return_type_is_mixin_self(
        ret,
        mixin_fqn,
        mixin_short,
        mixin_resolved_name,
        resolved_short,
    ) {
        method.return_type = Some(PhpType::this());
        if method.native_return_type.as_ref().is_some_and(|n| {
            return_type_is_mixin_self(
                n,
                mixin_fqn,
                mixin_short,
                mixin_resolved_name,
                resolved_short,
            )
        }) {
            method.native_return_type = Some(PhpType::this());
        }
    }
}

fn return_type_is_mixin_self(
    ty: &PhpType,
    mixin_fqn: &str,
    mixin_short: &str,
    mixin_resolved_name: &str,
    resolved_short: &str,
) -> bool {
    let check_name = |name: &str| {
        let stripped = name.strip_prefix('\\').unwrap_or(name);
        stripped == mixin_fqn
            || stripped == mixin_resolved_name
            || stripped == mixin_short
            || stripped == resolved_short
            || short_name(stripped) == mixin_short
    };
    match ty.kind() {
        TypeKind::Named(n) | TypeKind::StaticType(n) | TypeKind::ThisType(n) => check_name(n),
        TypeKind::Generic(g) => check_name(&g.name),
        TypeKind::Nullable(inner) => return_type_is_mixin_self(
            inner,
            mixin_fqn,
            mixin_short,
            mixin_resolved_name,
            resolved_short,
        ),
        TypeKind::Union(members) => {
            let non_null: Vec<_> = members.iter().filter(|m| !m.is_null()).collect();
            !non_null.is_empty()
                && non_null.iter().all(|m| {
                    return_type_is_mixin_self(
                        m,
                        mixin_fqn,
                        mixin_short,
                        mixin_resolved_name,
                        resolved_short,
                    )
                })
        }
        _ => false,
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
/// The returned [`VirtualMembers`](super::super::VirtualMembers) should be
/// merged into the substituted class via
/// [`merge_virtual_members`](super::merge_virtual_members).
pub fn resolve_template_param_mixins(
    original_class: &ClassInfo,
    template_subs: &HashMap<String, PhpType>,
    class_loader: &ResolvedClassLoader<'_>,
) -> super::super::VirtualMembers {
    if template_subs.is_empty() || original_class.mixins.is_empty() {
        return super::super::VirtualMembers {
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
        return super::super::VirtualMembers {
            methods: Vec::new(),
            properties: Vec::new(),
            constants: Vec::new(),
        };
    }

    let dedup = MixinDedup {
        methods: AtomSet::default(),
        properties: AtomSet::default(),
        constants: AtomSet::default(),
    };

    let mut collector = MixinCollector {
        methods: Vec::new(),
        properties: Vec::new(),
        constants: Vec::new(),
        dedup,
    };

    let decorated_forward = uses_forwards_calls(original_class, class_loader);
    collect_mixin_members(
        &template_mixins,
        &original_class.mixin_generics,
        class_loader,
        &mut collector,
        &MixinSubs {
            subs: template_subs,
            bounds: &original_class.template_param_bounds,
            decorated_forward,
        },
        0,
        None,
    );

    super::super::VirtualMembers {
        methods: collector.methods,
        properties: collector.properties,
        constants: collector.constants,
    }
}

/// Build a substitution map for mixin generic resolution by zipping the
/// parent class's `@template` parameters with the type arguments the
/// child provides via `@extends` / `@implements` generics.
///
/// This is [`crate::inheritance::build_substitution_map`] with an extra
/// fallback to template bounds when a `@mixin` names a template parameter
/// (see the comment on `parent_has_template_param_mixin` below).
fn build_mixin_substitution_map(
    current: &ClassInfo,
    parent: &ClassInfo,
    active_subs: &HashMap<String, PhpType>,
    origin_bounds: &crate::atom::AtomMap<PhpType>,
) -> HashMap<String, PhpType> {
    let Some(mut map) = bind_template_args(
        parent,
        current
            .extends_generics
            .iter()
            .chain(current.implements_generics.iter()),
        active_subs,
    ) else {
        return active_subs.clone();
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
    if !parent_has_template_param_mixin {
        return map;
    }

    // Prefer the bound declared on the walk-origin class (the
    // most-derived class whose members are being resolved) over the
    // intermediate level's bound.  In a straight-through chain
    // (`@extends Parent<TNode>` at every level) each class may tighten
    // the constraint, e.g. `AbstractNode<TNode of ASTNode>` →
    // `CallableNode<TNode of AbstractCallable>`.  The mixin lives on the
    // ancestor with the loosest bound, but the concrete members available
    // come from the origin's tighter bound, so that is the one to resolve
    // against.
    for resolved in map.values_mut() {
        if let Some(name) = resolved.base_name()
            && let Some(tp) = current.template_params.iter().find(|t| t.as_str() == name)
            && let Some(bound) = origin_bounds
                .get(tp)
                .or_else(|| current.template_param_bounds.get(tp))
        {
            *resolved = bound.clone();
        }
    }

    map
}
