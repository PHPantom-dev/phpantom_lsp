//! The base inheritance merge: a class's own members, then its traits,
//! then each ancestor's up the `extends` chain, with `@extends` generics
//! substituted into what is inherited and interface docblocks enriching
//! the methods the class overrides.

use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::{Atom, AtomSet};
use crate::class_lookup::ResolvedClassLoader;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH, Visibility};
use crate::virtual_members::laravel::{
    ELOQUENT_MODEL_FQN, factory_model_type, has_inheritable_model_metadata, inherit_model_metadata,
    is_factory_class,
};
use crate::virtual_members::{
    TransformFingerprint, intern_transformed_method, intern_transformed_property,
};

use super::{
    ClassRef, apply_substitution_to_method, apply_substitution_to_property,
    bind_inherited_class_keywords, bind_inherited_class_keywords_in_property,
    build_substitution_map, enrich_method_arc_from_ancestor, enrich_property_arc_from_ancestor,
    fill_template_bounds, method_has_inherited_class_keyword, method_references_params,
    property_has_inherited_class_keyword, property_references_params, traits,
};

/// Bundles the trait-level configuration passed through
/// [`merge_traits_into`] so the function stays within clippy's
/// argument-count limit.
pub(crate) struct TraitContext<'a> {
    /// Generic type arguments for `@use Trait<Type>` declarations.
    pub use_generics: &'a [(Atom, Vec<PhpType>)],
    /// `insteadof` precedence declarations.
    pub precedences: &'a [crate::types::TraitPrecedence],
    /// `as` alias declarations.
    pub aliases: &'a [crate::types::TraitAlias],
}

/// Tracks member names already present during inheritance merging.
///
/// Passed through `resolve_class_with_inheritance` and `merge_traits_into`
/// (including recursive calls) so that every addition is checked in O(1)
/// instead of scanning the full member vectors.
pub(crate) struct MergeDedup {
    /// Method names already merged, lowercased (PHP method names are
    /// case-insensitive, so a child `getvalue()` overrides a parent
    /// `getValue()`).
    pub methods: AtomSet,
    /// Property names already merged.
    pub properties: AtomSet,
    /// Constant names already merged.
    pub constants: AtomSet,
}

impl MergeDedup {
    /// Build from the members already present on a `ClassInfo`.
    fn from_class(class: &ClassInfo) -> Self {
        Self {
            methods: class
                .methods
                .iter()
                .map(|m| crate::atom::ascii_lowercase_atom(&m.name))
                .collect(),
            properties: class.properties.iter().map(|p| p.name).collect(),
            constants: class.constants.iter().map(|c| c.name).collect(),
        }
    }

    /// Copy every member of `source` a subclass inherits into `merged`,
    /// sharing each one's `Arc` rather than cloning it.
    ///
    /// A private member is not inherited, and a name already merged wins
    /// over the one further up the chain, which is PHP's own precedence.
    /// This is the plain form, without the generic substitution and
    /// docblock enrichment the `extends` walk in
    /// [`resolve_class_with_inheritance`] layers on top.
    pub(crate) fn merge_visible_members(&mut self, source: &ClassInfo, merged: &mut ClassInfo) {
        for method in &source.methods {
            if method.visibility != Visibility::Private
                && self
                    .methods
                    .insert(crate::atom::ascii_lowercase_atom(&method.name))
            {
                merged.methods.push(Arc::clone(method));
            }
        }
        for property in &source.properties {
            if property.visibility != Visibility::Private && self.properties.insert(property.name) {
                merged.properties.push(Arc::clone(property));
            }
        }
        for constant in &source.constants {
            if constant.visibility != Visibility::Private && self.constants.insert(constant.name) {
                merged.constants.push(Arc::clone(constant));
            }
        }
    }
}

/// Resolve a class together with all inherited members from its parent
/// chain.
///
/// Walks up the `extends` chain via `class_loader`, collecting public and
/// protected methods, properties, and constants from each ancestor.
/// If a child already defines a member with the same name as a parent
/// member, the child's version wins (even if the signatures differ).
///
/// Private members are never inherited.
///
/// When the child declares `@extends Parent<Type1, Type2>` and the parent
/// has `@template` parameters, the inherited members have their template
/// parameter types replaced with the concrete types from the `@extends`
/// annotation.  This substitution chains through the entire ancestry.
///
/// A depth limit of 20 prevents infinite loops from circular inheritance.
pub(crate) fn resolve_class_with_inheritance(
    class: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> ClassInfo {
    let mut merged = class.clone();

    // Build dedup sets from the class's own members.  These are passed
    // through trait merging and the parent chain walk so that every
    // addition is tracked in O(1) across all recursion levels.
    let mut dedup = MergeDedup::from_class(&merged);

    // A `@method` tag names something `__call()` handles, and PHP only
    // reaches `__call()` when no accessible method of that name exists.
    // A tag that collides with a real method from a trait or the parent
    // chain is therefore dead documentation, however precise it looks:
    // the walk below merges the real method, and the PHPDoc provider's
    // virtual member loses to it during the virtual-member merge.

    // 1. Merge traits used by this class.
    //    PHP precedence: class methods > trait methods > inherited methods.
    //    Since `merged` already contains the class's own members, we only
    //    add trait members that don't collide with existing ones.
    traits::merge_traits_into(
        &mut merged,
        &class.used_traits,
        &TraitContext {
            use_generics: &class.use_generics,
            precedences: &class.trait_precedences,
            aliases: &class.trait_aliases,
        },
        class_loader,
        0,
        &mut dedup,
        &class.fqn(),
    );

    // 2. Walk up the `extends` chain and merge parent members.
    merge_parent_chain(class, &mut merged, &mut dedup, class_loader);

    // 3. Enrich methods from implemented interfaces.
    enrich_from_interfaces(class, &mut merged, class_loader);

    retype_documented_properties(class, &mut merged);
    refine_enum_cases(&mut merged);
    merged
}

/// Walk up the `extends` chain of `class` and merge each parent's traits
/// and members into `merged`.
fn merge_parent_chain(
    class: &ClassInfo,
    merged: &mut ClassInfo,
    dedup: &mut MergeDedup,
    class_loader: &ResolvedClassLoader<'_>,
) {
    // `current` holds a reference to the class whose `parent_class`,
    // `extends_generics`, `used_traits`, etc. we read at each level.
    // For the first iteration this is the root `class` (a borrow —
    // zero allocation).  After that it becomes the `Arc<ClassInfo>`
    // returned by `class_loader` (a cheap Arc move).
    let mut current: ClassRef<'_> = ClassRef::Borrowed(class);
    let mut depth = 0;

    // The substitution map accumulates as we walk the chain.
    // It maps template parameter names → concrete types, and is
    // re-computed at each level based on the `@extends` generics
    // of the current class and the `@template` params of the parent.
    //
    // It starts empty: the root class's own `@extends` generics are
    // matched against the first parent's template_params in the loop.
    let mut active_subs: HashMap<String, PhpType> = HashMap::new();

    while let Some(ref parent_name) = current.parent_class {
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            break;
        }

        let Some(parent) =
            crate::class_lookup::load_ancestor(&current.fqn(), parent_name, class_loader)
        else {
            break;
        };

        inherit_class_metadata(merged, &parent);
        let level_subs = level_substitutions(
            class,
            &current,
            &parent,
            parent_name,
            &active_subs,
            class_loader,
        );
        merge_parent_members(merged, dedup, &parent, &level_subs, class_loader);

        // Carry the substitution map forward for the next level.
        // If `Collection` extends `AbstractCollection<TKey, TValue>`,
        // we need to apply the current substitutions to those type
        // arguments so that `TKey` → `int` flows through.
        active_subs = level_subs;
        current = ClassRef::Owned(parent);
    }
}

/// What a class inherits from a parent beyond its members.
fn inherit_class_metadata(merged: &mut ClassInfo, parent: &ClassInfo) {
    // `readonly` covers the whole hierarchy: PHP rejects a
    // non-readonly class extending a readonly one, so a subclass is
    // readonly whether or not it repeats the keyword, and the
    // properties it inherits are readonly as well.
    merged.is_readonly |= parent.is_readonly;

    // Eloquent model configuration (`$fillable`, `$casts`,
    // `$primaryKey`, …) comes from the nearest class that declares it.
    // The framework's own `Model` is skipped: its declarations are the
    // defaults (`$guarded = ['*']`, `$primaryKey = 'id'`, …) that an
    // undeclared setting already stands for.
    if let Some(parent_meta) = parent.laravel()
        && has_inheritable_model_metadata(parent_meta)
        && !parent.fqn().eq_ignore_ascii_case(ELOQUENT_MODEL_FQN)
    {
        inherit_model_metadata(merged.laravel_mut(), parent_meta);
    }
}

/// The template substitutions for one parent level of `class`'s chain:
/// `current`'s `@extends` generics for `parent` (with `active_subs`, the
/// level below's, applied), the Laravel factory's model, then the bounds
/// of whatever template is still unbound.
fn level_substitutions(
    class: &ClassInfo,
    current: &ClassRef<'_>,
    parent: &ClassInfo,
    parent_name: &str,
    active_subs: &HashMap<String, PhpType>,
    class_loader: &ResolvedClassLoader<'_>,
) -> HashMap<String, PhpType> {
    // Look through current's `extends_generics` for an entry
    // whose class name matches this parent, and zip its type
    // arguments with the parent's `template_params`.
    let mut level_subs = build_substitution_map(current, parent, active_subs);

    // ── Laravel Factory model binding ────────────────────────
    // Set `TModel` from the source-aware factory hierarchy lookup:
    // a concrete generic binding, then `$model`, then the convention.
    if is_factory_class(parent_name)
        && !parent.template_params.is_empty()
        && let Some(model_type) = factory_model_type(class, class_loader)
    {
        // Re-apply the source-aware answer even when `level_subs` already
        // contains `Model`: an unbound template on an intermediate base is
        // filled from its bound during the preceding level, but that
        // fallback must not hide the concrete factory's `$model`. A real
        // explicit binding is returned by `factory_model_type` instead and
        // therefore remains authoritative here.
        for param in &parent.template_params {
            level_subs.insert(param.to_string(), model_type.clone());
        }
    }

    // ── Template bound fallback ─────────────────────────────
    // When a subclass extends a generic parent without providing
    // explicit `@extends` generics and no convention-based
    // substitution filled the map, fall back to the template
    // parameter bounds (e.g. `@template T of object` → `object`)
    // so that inherited methods don't leak raw template names.
    fill_template_bounds(parent, &mut level_subs);
    level_subs
}

/// How one parent's members are rewritten on their way into a subclass:
/// the level's template substitutions, and the classes a bare `self` /
/// `parent` in them binds to.
struct LevelTransform<'a> {
    subs: &'a HashMap<String, PhpType>,
    /// Template parameter names substituted at this level, used to
    /// decide per member whether substitution would change anything.
    /// Empty when the parent is non-generic, in which case every
    /// member keeps its shared `Arc` / avoids the substitution
    /// walk (copy-on-write).
    sub_keys: Vec<String>,
    parent_fqn: Atom,
    declaring_parent: Option<String>,
    /// Interning fingerprints for the three transform shapes a parent
    /// member can need (substitution, relative-keyword binding, or
    /// both), computed once per level.  The keyword binding is fully
    /// determined by the declaring parent (its own parent is what a bare
    /// `parent` binds to), so the transformed copies are identical for
    /// every subclass and can be shared through the interning store.
    fp_sub: TransformFingerprint,
    fp_self: TransformFingerprint,
    fp_both: TransformFingerprint,
}

impl<'a> LevelTransform<'a> {
    fn new(parent: &ClassInfo, subs: &'a HashMap<String, PhpType>) -> Self {
        let parent_fqn = parent.fqn();
        Self {
            subs,
            sub_keys: subs.keys().cloned().collect(),
            parent_fqn,
            declaring_parent: parent.parent_class.map(|a| a.to_string()),
            fp_sub: TransformFingerprint::new(Some(subs), None, 0),
            fp_self: TransformFingerprint::new(None, Some(parent_fqn.as_str()), 0),
            fp_both: TransformFingerprint::new(Some(subs), Some(parent_fqn.as_str()), 0),
        }
    }

    /// The fingerprint of the transform a member needs.
    fn fingerprint(&self, needs_sub: bool, needs_keywords: bool) -> TransformFingerprint {
        match (needs_sub, needs_keywords) {
            (true, false) => self.fp_sub,
            (false, true) => self.fp_self,
            _ => self.fp_both,
        }
    }
}

/// Merge one parent's traits, methods, properties and constants into
/// `merged`, with the level's template substitutions applied.
fn merge_parent_members(
    merged: &mut ClassInfo,
    dedup: &mut MergeDedup,
    parent: &ClassInfo,
    level_subs: &HashMap<String, PhpType>,
    class_loader: &ResolvedClassLoader<'_>,
) {
    // Merge traits used by the parent class as well, so that
    // grandparent-level trait members are visible.
    // Apply the current level's template substitutions to the
    // parent's `@use` generics.  Without this, a chain like:
    //
    //   /** @extends DataCollection<int, DeliveryOption> */
    //   class DeliveryOptionCollection extends DataCollection
    //
    // where DataCollection has:
    //   /** @use EnumerableMethods<TKey, TValue> */
    //
    // would pass the raw `TKey`/`TValue` template params to the
    // trait instead of the concrete `int`/`DeliveryOption` types.
    let substituted_use_generics: Vec<(Atom, Vec<PhpType>)> = if level_subs.is_empty() {
        parent.use_generics.clone()
    } else {
        parent
            .use_generics
            .iter()
            .map(|(name, args)| {
                let substituted_args: Vec<PhpType> =
                    args.iter().map(|arg| arg.substitute(level_subs)).collect();
                (*name, substituted_args)
            })
            .collect()
    };

    traits::merge_traits_into(
        merged,
        &parent.used_traits,
        &TraitContext {
            use_generics: &substituted_use_generics,
            precedences: &parent.trait_precedences,
            aliases: &parent.trait_aliases,
        },
        class_loader,
        0,
        dedup,
        &parent.fqn(),
    );

    let transform = LevelTransform::new(parent, level_subs);
    merge_parent_methods(merged, dedup, parent, &transform, class_loader);
    merge_parent_properties(merged, dedup, parent, &transform);

    // Merge parent constants.  Constants are never transformed on
    // inheritance, so an inherited constant is always a plain
    // `Arc::clone` of the declaring class's.
    for constant in &parent.constants {
        if constant.visibility == Visibility::Private {
            continue;
        }
        if !dedup.constants.insert(constant.name) {
            continue;
        }
        merged.constants.push(Arc::clone(constant));
    }
}

/// Merge parent methods — skip private.
/// When the child already has a method with the same name,
/// enrich it with the parent's richer docblock types instead
/// of silently discarding the parent's type information.
fn merge_parent_methods(
    merged: &mut ClassInfo,
    dedup: &mut MergeDedup,
    parent: &ClassInfo,
    transform: &LevelTransform,
    class_loader: &ResolvedClassLoader<'_>,
) {
    for method in &parent.methods {
        if method.visibility == Visibility::Private {
            continue;
        }
        let needs_sub = method_references_params(method, &transform.sub_keys);
        if !dedup
            .methods
            .insert(crate::atom::ascii_lowercase_atom(&method.name))
        {
            // Child already has this method — enrich it from parent.
            // Only clone + substitute when the parent's signature
            // actually references a substituted template param;
            // otherwise enrich directly from the shared method.
            if let Some(existing) = merged
                .methods
                .make_mut()
                .iter_mut()
                .find(|m| m.name.eq_ignore_ascii_case(&method.name))
            {
                if needs_sub {
                    let ancestor_method =
                        intern_transformed_method(method, transform.fp_sub, || {
                            let mut m = (**method).clone();
                            apply_substitution_to_method(&mut m, transform.subs);
                            m
                        });
                    enrich_method_arc_from_ancestor(existing, &ancestor_method, class_loader);
                } else {
                    enrich_method_arc_from_ancestor(existing, method, class_loader);
                }
            }
            continue;
        }
        // Bind bare `self` / `parent` in the return type and parameter
        // hints to the declaring (parent) class and its own parent, so
        // they resolve to the classes that the declaration meant rather
        // than to the inheriting child and its parent.
        let needs_keywords =
            method_has_inherited_class_keyword(method, transform.declaring_parent.as_deref());
        if !needs_sub && !needs_keywords {
            // Substitution is a no-op and there is no relative keyword:
            // keep the shared `Arc` instead of deep-cloning.
            merged.methods.push(Arc::clone(method));
            continue;
        }
        let fp = transform.fingerprint(needs_sub, needs_keywords);
        let transformed = intern_transformed_method(method, fp, || {
            let mut ancestor_method = (**method).clone();
            if needs_sub {
                apply_substitution_to_method(&mut ancestor_method, transform.subs);
            }
            if needs_keywords {
                bind_inherited_class_keywords(
                    &mut ancestor_method,
                    &transform.parent_fqn,
                    transform.declaring_parent.as_deref(),
                );
            }
            ancestor_method
        });
        merged.methods.push(transformed);
    }
}

/// Merge parent properties — same enrichment logic as methods.
/// The substitution transform reuses the methods' fingerprint (a property
/// and a method with the same substitution map fingerprint to the same
/// value, but they intern into separate maps keyed by origin type, so
/// there is no collision).
fn merge_parent_properties(
    merged: &mut ClassInfo,
    dedup: &mut MergeDedup,
    parent: &ClassInfo,
    transform: &LevelTransform,
) {
    for property in &parent.properties {
        if property.visibility == Visibility::Private {
            continue;
        }
        let needs_sub = property_references_params(property, &transform.sub_keys);
        // A bare `self` / `parent` in the type names the declaring
        // class and its parent, not the class the property is read
        // through, exactly as for an inherited method.
        let needs_keywords =
            property_has_inherited_class_keyword(property, transform.declaring_parent.as_deref());
        let transformed = if !needs_sub && !needs_keywords {
            // Neither transform applies: keep the shared `Arc`.
            Arc::clone(property)
        } else {
            let fp = transform.fingerprint(needs_sub, needs_keywords);
            intern_transformed_property(property, fp, || {
                let mut p = (**property).clone();
                if needs_sub {
                    apply_substitution_to_property(&mut p, transform.subs);
                }
                if needs_keywords {
                    bind_inherited_class_keywords_in_property(
                        &mut p,
                        &transform.parent_fqn,
                        transform.declaring_parent.as_deref(),
                    );
                }
                p
            })
        };
        if !dedup.properties.insert(property.name) {
            // Child already has this property — enrich it from parent.
            if let Some(existing) = merged
                .properties
                .make_mut()
                .iter_mut()
                .find(|p| p.name == property.name)
            {
                enrich_property_arc_from_ancestor(existing, &transformed);
            }
            continue;
        }
        merged.properties.push(transformed);
    }
}

/// Enrich the methods `class` overrides from the interfaces it implements.
/// When a class overrides an interface method without a return type,
/// propagate the interface method's return type (with template
/// substitution from `@implements` generics).
fn enrich_from_interfaces(
    class: &ClassInfo,
    merged: &mut ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) {
    let class_fqn = class.fqn();
    for iface_name in &class.interfaces {
        let Some(iface) = crate::class_lookup::load_ancestor(&class_fqn, iface_name, class_loader)
        else {
            continue;
        };

        // Build substitution map from @implements/@template-implements generics.
        //
        // Any of the interface's own @template params this class doesn't
        // bind explicitly (e.g. a plain `implements ArrayAccess` with no
        // `@implements ArrayAccess<TKey, TValue>` at all) falls back to its
        // declared bound (or `mixed`) here. Without this, a docblock return
        // type like `ArrayAccess`'s own `@return TValue` would leak through
        // `enrich_method_arc_from_ancestor` below as if "TValue" were a real,
        // resolvable class, clobbering the class's own concrete override
        // (e.g. `offsetGet(): Pen`). This fallback is local to interface
        // enrichment; `build_substitution_map` itself must keep leaving an
        // absent annotation unresolved, since other consumers (the `extends`
        // chain walk, Laravel factory model detection) rely on that absence
        // to fall through to their own convention-based resolution.
        let mut iface_subs =
            build_substitution_map(&ClassRef::Borrowed(class), &iface, &HashMap::new());
        fill_template_bounds(&iface, &mut iface_subs);
        let iface_sub_keys: Vec<String> = iface_subs.keys().cloned().collect();
        let fp_iface = TransformFingerprint::new(Some(&iface_subs), None, 0);

        for method in &iface.methods {
            // Only enrich methods that the class already has (i.e. overrides).
            if let Some(existing) = merged
                .methods
                .make_mut()
                .iter_mut()
                .find(|m| m.name.eq_ignore_ascii_case(&method.name))
            {
                if method_references_params(method, &iface_sub_keys) {
                    let ancestor_method = intern_transformed_method(method, fp_iface, || {
                        let mut m = (**method).clone();
                        apply_substitution_to_method(&mut m, &iface_subs);
                        m
                    });
                    enrich_method_arc_from_ancestor(existing, &ancestor_method, class_loader);
                } else {
                    enrich_method_arc_from_ancestor(existing, method, class_loader);
                }
            }
        }
    }
}

/// Retype an inherited non-public property that the class documents
/// with a `@property` tag of its own.
///
/// A tag documents a magic read, and PHP only reaches `__get()` when no
/// *accessible* property of that name exists.  An ancestor's
/// `protected` / `private` declaration is invisible from outside, so it
/// is not what the read yields and its type must not describe it — an
/// Eloquent model declaring `@property string $connection` means
/// `string`, not `Model::$connection`'s `\UnitEnum|string|null`. The
/// same shadowing is routine for `$table` and `$keyType`.
///
/// A property the class declares *itself* is a different matter: it is
/// in scope everywhere the tag is, so it keeps its own type (the tag is
/// then a contradiction, and the real declaration is the truth).
fn retype_documented_properties(class: &ClassInfo, merged: &mut ClassInfo) {
    let Some(doc) = class.doc_members.as_deref() else {
        return;
    };
    for (name, type_hint) in &doc.properties {
        let Some(hint) = type_hint else {
            continue;
        };
        if class.properties.iter().any(|p| p.name == *name) {
            continue;
        }
        // Look the index up immutably so a class with nothing to
        // retype keeps sharing its property vector.
        let Some(idx) = merged
            .properties
            .iter()
            .position(|p| p.name == *name && p.visibility != Visibility::Public)
        else {
            continue;
        };
        let prop = &mut merged.properties.make_mut()[idx];
        Arc::make_mut(prop).type_hint = Some(hint.clone());
    }
}

/// Refine the `cases()` method on enums.  The `UnitEnum` interface
/// declares `public static function cases(): array`, which loses the
/// element type: `Country::cases()[0]` would resolve to `mixed`.
/// Every concrete enum returns a list of its own instances, so
/// replace the bare `array` with `list<EnumName>` (using the FQN so
/// the element resolves regardless of the call site's namespace).
fn refine_enum_cases(merged: &mut ClassInfo) {
    if merged.kind != crate::types::ClassLikeKind::Enum {
        return;
    }
    let list_type = PhpType::list(PhpType::named(merged.fqn()));
    if let Some(cases) = merged
        .methods
        .make_mut()
        .iter_mut()
        .find(|m| m.name.eq_ignore_ascii_case("cases"))
    {
        Arc::make_mut(cases).return_type = Some(list_type);
    }
}
