//! Type-hint resolution to concrete `ClassInfo` values.
//!
//! This module contains the logic for mapping parsed [`PhpType`] values
//! (as they appear in return types, property annotations, and PHPDoc
//! tags) to resolved `ClassInfo` values that the completion, hover, and
//! definition engines can work with.
//!
//! Split from [`crate::type_engine::resolver`] for navigability.  The entry points are:
//!
//! - [`type_hint_to_classes_typed`]: maps a parsed [`PhpType`] to all
//!   matching `ClassInfo` values (handles unions, intersections, generics,
//!   `self`/`static`/`$this`, nullable types, object shapes, and type
//!   alias expansion).
//! - [`resolve_property_types`]: resolves a property's type hint
//!   on a class to all candidate `ClassInfo` values.
//!
//! Type aliases themselves are expanded by [`super::aliases`].

use std::sync::Arc;

use super::aliases::resolve_type_alias_typed;
use crate::atom::atom;
use crate::class_lookup::{ResolvedClassLoader, find_or_load_resolved};
use crate::php_type::{PhpType, TypeKind, is_builtin_non_class_type};
use crate::types::*;
use crate::util::short_name;
use crate::virtual_members::{self, laravel};

/// Look up a property's type hint and resolve all candidate classes.
///
/// When the type hint is a union (e.g. `A|B`), every resolvable part
/// is returned.
pub(crate) fn resolve_property_types(
    prop_name: &str,
    class_info: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    // Resolve inheritance so that inherited (and generic-substituted)
    // properties are visible.  For example, if `ConfigWrapper extends
    // Wrapper<Config>` and `Wrapper` has `/** @var T */ public $value`,
    // the merged class will have `$value` with type `Config`.
    let type_hint =
        match crate::inheritance::resolve_property_type_hint(class_info, prop_name, class_loader) {
            Some(h) => h,
            None => return vec![],
        };
    let owner_fqn = class_info.fqn();
    type_hint_to_classes_typed(&type_hint, &owner_fqn, all_classes, class_loader)
}

/// Look up the class the resolved `name` names and return its declaration
/// as written, with no generic substitution applied.
///
/// `name` is exactly the FQN it spells: a bare `Node` is the global class,
/// never a same-named class of the namespace the lookup happens in.
///
/// Callers that want the *type* `name` denotes (with unsupplied
/// `@template` parameters erased to their bounds) want
/// [`type_hint_to_classes_typed`] instead.  This entry point is for the
/// few callers that bind those parameters themselves and therefore need
/// the parameter names still standing in the member signatures.
pub(crate) fn lookup_class_declaration(
    name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Arc<ClassInfo>> {
    find_or_load_resolved(name, all_classes, class_loader)
}

/// Map a parsed [`PhpType`] to all matching `ClassInfo` values.
///
/// Handles:
///   - Nullable types: `?Foo` → strips `?`, resolves `Foo`
///   - Union types: `A|B|C` → resolves each part independently
///   - Intersection types: `A&B` → resolves each part independently
///   - Generic types: `Collection<int, User>` → resolves `Collection`,
///     then applies generic substitution (`TKey→int`, `TValue→User`)
///   - `self` / `static` / `$this` → owning class
///   - Scalar/built-in types (`int`, `string`, `bool`, `float`, `array`,
///     `void`, `null`, `mixed`, `never`, `object`, `callable`, `iterable`,
///     `false`, `true`) → skipped (not class types)
///
/// Each resolvable class-like part is returned as a separate entry.
///
/// Callers that start with a raw type string should parse it with
/// `PhpType::parse()` first.
pub(crate) fn type_hint_to_classes_typed(
    ty: &PhpType,
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    type_hint_to_classes_typed_depth(ty, owning_class_name, all_classes, class_loader, 0, false)
}

/// [`type_hint_to_classes_typed`] for a method's *return* type hint.
///
/// A returned collection is one Eloquent actually produced, so — unlike a
/// parameter, property, or `@var` hint resolved through the plain
/// entry point above — it is safe to swap the declared
/// `Illuminate\Database\Eloquent\Collection<…, TModel>` for the model's
/// custom collection class (see
/// [`laravel::try_swap_custom_collection`]). Resolving a value the
/// caller merely declared as the base collection to the narrower
/// subclass would be an over-claim: the caller may have built a plain
/// collection itself.
pub(crate) fn type_hint_to_classes_typed_returned(
    ty: &PhpType,
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    type_hint_to_classes_typed_depth(ty, owning_class_name, all_classes, class_loader, 0, true)
}

/// Type `hint` as the classes it names, or as a bare type string when it
/// names none.
///
/// The fallback is what keeps a non-class type alive: `list<Rule>`,
/// `int`, and a class name nothing can load all reach it, and dropping
/// them would leave the subject untyped rather than typed by something
/// that simply is not a loadable class.
pub(crate) fn resolved_types_for_hint(
    hint: crate::php_type::PhpType,
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<crate::types::ResolvedType> {
    let hint = if laravel::has_model_type_operator(&hint) && !owning_class_name.is_empty() {
        laravel::expand_model_type(
            &hint.resolve_self_refs_bounded(owning_class_name, None),
            class_loader,
        )
    } else {
        laravel::expand_model_type(&hint, class_loader)
    };
    let classes = type_hint_to_classes_typed(&hint, owning_class_name, all_classes, class_loader);
    if classes.is_empty() {
        vec![crate::types::ResolvedType::from_type_string(hint)]
    } else {
        crate::types::ResolvedType::from_classes_with_hint(classes, hint)
    }
}

/// Inner implementation with a recursion depth guard to prevent
/// infinite loops from circular type aliases.
///
/// `produced` is true only when `ty` is a method's return type hint (see
/// [`type_hint_to_classes_typed_returned`]); it is threaded unchanged
/// through every recursive call so a union/generic-arg member nested
/// inside a return type is still treated as returned.
fn type_hint_to_classes_typed_depth(
    ty: &PhpType,
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
    depth: u8,
    produced: bool,
) -> Vec<Arc<ClassInfo>> {
    if depth > MAX_ALIAS_DEPTH {
        return vec![];
    }

    match ty.kind() {
        // ── Leniency / list-shape / template / unsealed / exact-value markers
        // Unreachable via `kind()`, which sees through all six markers,
        // but the arms keep this match exhaustive over the type language.
        TypeKind::UnsealedShape(_) => vec![],
        TypeKind::Benevolent(inner)
        | TypeKind::ListShape(inner)
        | TypeKind::TemplateParam(_, inner)
        | TypeKind::ClassNameLiteral(inner)
        | TypeKind::EnumCase(inner, _) => type_hint_to_classes_typed_depth(
            inner,
            owning_class_name,
            all_classes,
            class_loader,
            depth,
            produced,
        ),
        // ── Nullable → unwrap inner ────────────────────────────────
        TypeKind::Nullable(inner) => type_hint_to_classes_typed_depth(
            inner,
            owning_class_name,
            all_classes,
            class_loader,
            depth,
            produced,
        ),

        // ── Union type ─────────────────────────────────────────────
        TypeKind::Union(members) => {
            let mut results: Vec<Arc<ClassInfo>> = Vec::new();
            let mut generic_members = Vec::new();
            for member in members {
                let resolved = type_hint_to_classes_typed_depth(
                    member,
                    owning_class_name,
                    all_classes,
                    class_loader,
                    depth,
                    produced,
                );
                if matches!(member.kind(), TypeKind::Generic(_)) {
                    // Different arguments can instantiate the same class with
                    // different members. FQN-only deduplication loses a branch.
                    if !generic_members.contains(&member) {
                        generic_members.push(member);
                        results.extend(resolved);
                    }
                } else {
                    ClassInfo::extend_unique_arc(&mut results, resolved);
                }
            }
            results
        }

        // ── Intersection type ──────────────────────────────────────
        TypeKind::Intersection(members) => {
            let mut results: Vec<Arc<ClassInfo>> = Vec::new();
            for member in members {
                let resolved = type_hint_to_classes_typed_depth(
                    member,
                    owning_class_name,
                    all_classes,
                    class_loader,
                    depth,
                    produced,
                );
                ClassInfo::extend_unique_arc(&mut results, resolved);
            }
            results
        }

        // ── Object shape ───────────────────────────────────────────
        TypeKind::ObjectShape(entries) => {
            // The synthetic class stands in for no class of its own, so a
            // `self` in a property type names the owning class, exactly as a
            // bare `self` would (see `resolve_named_type`).
            let self_fqn = if !owning_class_name.is_empty()
                && entries.iter().any(|e| e.value_type.contains_bare_self())
            {
                find_or_load_resolved(owning_class_name, all_classes, class_loader).map(|c| c.fqn())
            } else {
                None
            };
            let properties = SharedVec::from_vec(
                entries
                    .iter()
                    .map(|e| {
                        let type_hint = match &self_fqn {
                            Some(fqn) => e.value_type.replace_bare_self(fqn),
                            None => e.value_type.clone(),
                        };
                        Arc::new(PropertyInfo {
                            name: atom(&e.key.clone().unwrap_or_default()),
                            name_offset: 0,
                            type_hint: Some(type_hint),
                            native_type_hint: None,
                            description: None,
                            is_static: false,
                            visibility: Visibility::Public,
                            deprecation_message: None,
                            deprecated_replacement: None,
                            see_refs: Vec::new(),
                            is_readonly: false,
                            is_virtual: true,
                            source: None,
                        })
                    })
                    .collect(),
            );

            vec![Arc::new(ClassInfo {
                name: atom("__object_shape"),
                properties,
                ..ClassInfo::default()
            })]
        }

        TypeKind::StaticType(s) | TypeKind::ThisType(s) => resolve_named_type(
            s,
            &[],
            owning_class_name,
            all_classes,
            class_loader,
            depth,
            produced,
        ),

        // ── Named type (class name, keyword, or alias) ─────────────
        TypeKind::Named(name) => resolve_named_type(
            name,
            &[],
            owning_class_name,
            all_classes,
            class_loader,
            depth,
            produced,
        ),

        // ── Generic type ───────────────────────────────────────────
        TypeKind::Generic(g) => {
            if matches!(
                g.name.as_str(),
                "builder-of" | "collection-of" | "factory-of" | "relation-of"
            ) {
                let bound = if owning_class_name.is_empty() {
                    ty.clone()
                } else {
                    ty.resolve_self_refs_bounded(owning_class_name, None)
                };
                let expanded = laravel::expand_model_type(&bound, class_loader);
                if &expanded != ty {
                    return type_hint_to_classes_typed_depth(
                        &expanded,
                        owning_class_name,
                        all_classes,
                        class_loader,
                        depth + 1,
                        produced,
                    );
                }
            }
            resolve_named_type(
                &g.name,
                &g.args,
                owning_class_name,
                all_classes,
                class_loader,
                depth,
                produced,
            )
        }

        // ── Array slice (T[]) ──────────────────────────────────────
        // Not a class type itself; skip.
        TypeKind::Array(_)
        | TypeKind::ArrayShape(_)
        | TypeKind::Callable(_)
        | TypeKind::ClassString(_)
        | TypeKind::InterfaceString(_)
        | TypeKind::KeyOf(_)
        | TypeKind::ValueOf(_)
        | TypeKind::IntRange(..)
        | TypeKind::IndexAccess(..)
        | TypeKind::Literal(_)
        | TypeKind::Conditional { .. }
        | TypeKind::Raw(_) => vec![],
    }
}

/// Resolve a named type (with optional generic args) to `ClassInfo`.
///
/// Handles `self`/`static`/`$this`/`parent`, type alias expansion,
/// template parameter bound fallback, and class lookup with generic
/// substitution.
fn resolve_named_type(
    name: &str,
    generic_args: &[PhpType],
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
    depth: u8,
    produced: bool,
) -> Vec<Arc<ClassInfo>> {
    // ── Fast reject: built-in scalar/pseudo types can never resolve
    //    to a class and are never type aliases. ──────────────────────
    if is_builtin_non_class_type(name) {
        return vec![];
    }

    // ── Type alias resolution ──────────────────────────────────────
    if let Some(alias_type) = resolve_type_alias_typed(
        &PhpType::named(atom(name)),
        owning_class_name,
        all_classes,
        class_loader,
    ) {
        return type_hint_to_classes_typed_depth(
            &alias_type,
            owning_class_name,
            all_classes,
            class_loader,
            depth + 1,
            produced,
        );
    }

    // ── self / static / $this ──────────────────────────────────────
    // These names come from pre-parsed PhpType values; PHP treats them
    // case-insensitively (e.g. `Self::`, `STATIC::` are valid).
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "self" | "static" | "$this"
    ) {
        if !generic_args.is_empty() {
            // `self<RuleError>` → rewrite to `OwningClass<RuleError>`.
            let rewritten = PhpType::generic(owning_class_name, generic_args.to_vec());
            return type_hint_to_classes_typed_depth(
                &rewritten,
                owning_class_name,
                all_classes,
                class_loader,
                depth,
                produced,
            );
        }
        return find_or_load_resolved(owning_class_name, all_classes, class_loader)
            .into_iter()
            .collect();
    }

    // ── parent ─────────────────────────────────────────────────────
    // Case-insensitive to match PHP behaviour (`Parent::`, `PARENT::` are valid).
    if name.eq_ignore_ascii_case("parent") {
        let parent_name = find_or_load_resolved(owning_class_name, all_classes, class_loader)
            .and_then(|c| c.parent_class);
        if let Some(parent) = parent_name {
            return find_or_load_resolved(&parent, all_classes, class_loader)
                .into_iter()
                .collect();
        }
        return vec![];
    }

    // ── Resolve static/self/$this inside generic arguments ────────
    let resolved_generic_args: Vec<PhpType>;
    let generic_args: &[PhpType] = if generic_args.iter().any(|a| a.is_self_ref()) {
        resolved_generic_args = generic_args
            .iter()
            .map(|arg| {
                if arg.is_self_ref() {
                    PhpType::named(atom(owning_class_name))
                } else {
                    arg.clone()
                }
            })
            .collect();
        &resolved_generic_args
    } else {
        generic_args
    };

    let owner_cell: std::cell::OnceCell<Option<Arc<ClassInfo>>> = std::cell::OnceCell::new();
    let owner = || {
        owner_cell
            .get_or_init(|| find_or_load_resolved(owning_class_name, all_classes, class_loader))
            .as_deref()
    };

    // ── Template parameter of the owning class ─────────────────────
    // A template in scope shadows a class of the same name (`@template T`
    // beside a global `trait T`), so it is consulted before the lookup.
    // Its bound stands in for it; an unbounded one names no class.
    if generic_args.is_empty()
        && !name.contains('\\')
        && !owning_class_name.is_empty()
        && let Some(owner) = owner()
        && owner.template_params.iter().any(|p| p.as_str() == name)
    {
        return match owner.template_param_bounds.get(&atom(name)) {
            Some(bound) => type_hint_to_classes_typed_depth(
                bound,
                owning_class_name,
                all_classes,
                class_loader,
                depth + 1,
                produced,
            ),
            None => vec![],
        };
    }

    match lookup_class_declaration(name, all_classes, class_loader) {
        Some(cls) => {
            // ── Eloquent custom collection swapping ────────────────
            // Only for a returned type hint — see
            // `type_hint_to_classes_typed_returned`.
            let cls = if produced {
                laravel::try_swap_custom_collection(
                    cls,
                    name,
                    generic_args,
                    all_classes,
                    class_loader,
                )
            } else {
                cls
            };

            // A type hint that names a generic class without arguments
            // (`@var ItemCollection $items`, a bare parameter or return
            // type) still has to answer member lookups.  Erase every
            // unsupplied template parameter to its declared default, then
            // to the bound its `@template` declares (`@template TModel of
            // Item` → `Item`), or `mixed` when it declares neither. This
            // keeps a member typed `TModel` concrete instead of leaking the
            // parameter's own name, matching the treatment that
            // `new ItemCollection()` already gets.
            //
            // A hint naming the enclosing class is left alone: inside its
            // own body a class's `@template` parameters are in scope, and
            // a member typed `TModel` means the caller's binding, not the
            // bound.  `self`/`static` already return the class unerased
            // above, and a name that resolves back to the same class is
            // the same reference spelled out.
            let erased_args: Vec<PhpType>;
            let generic_args: &[PhpType] = if generic_args.is_empty()
                && !cls.template_params.is_empty()
                && !cls.fqn().eq_ignore_ascii_case(owning_class_name)
            {
                erased_args = crate::inheritance::default_type_args(&cls);
                &erased_args
            } else {
                generic_args
            };

            // Apply generic substitution if the type hint carried generic
            // arguments and the class has template parameters of its own
            // to substitute, or inherits them from a generic parent it
            // never bound (a `static<TNewKey, TValue>` rebind on a
            // concrete collection subclass, a custom Eloquent builder
            // specialised to its model).
            if !generic_args.is_empty()
                && (!cls.template_params.is_empty()
                    || crate::inheritance::is_extends_only_generic_rebindable(
                        &cls,
                        generic_args.len(),
                        class_loader,
                    ))
            {
                let generic_arg_strings: Vec<String> =
                    generic_args.iter().map(|a| a.to_string()).collect();
                vec![virtual_members::resolve_class_for_type_hint(
                    &cls,
                    class_loader,
                    &generic_arg_strings,
                    generic_args,
                )]
            } else {
                vec![cls]
            }
        }
        None => {
            // ── Template parameter bound fallback ──────────────────
            // For a template name that got namespace-qualified on the way.
            let short = short_name(name);
            if let Some(owner) = owner()
                && owner.template_params.iter().any(|p| p.as_str() == short)
                && let Some(bound) = owner.template_param_bounds.get(&atom(short))
            {
                return type_hint_to_classes_typed_depth(
                    bound,
                    owning_class_name,
                    all_classes,
                    class_loader,
                    depth + 1,
                    produced,
                );
            }

            // ── stdClass fallback ──────────────────────────────────
            if short == "stdClass" {
                return vec![Arc::new(ClassInfo {
                    name: atom("stdClass"),
                    ..ClassInfo::default()
                })];
            }

            vec![]
        }
    }
}
