/// Type-hint resolution to concrete `ClassInfo` values.
///
/// This module contains the logic for mapping parsed [`PhpType`] values
/// (as they appear in return types, property annotations, and PHPDoc
/// tags) to resolved `ClassInfo` values that the completion, hover, and
/// definition engines can work with.
///
/// Split from [`super::resolver`] for navigability.  The entry points are:
///
/// - [`type_hint_to_classes_typed`]: maps a parsed [`PhpType`] to all
///   matching `ClassInfo` values (handles unions, intersections, generics,
///   `self`/`static`/`$this`, nullable types, object shapes, and type
///   alias expansion).
/// - [`resolve_type_alias_typed`]: fully expands a type alias defined
///   via `@phpstan-type` / `@psalm-type` / `@phpstan-import-type`.
/// - [`resolve_property_types`]: resolves a property's type hint
///   on a class to all candidate `ClassInfo` values.
/// - [`resolve_imported_type_alias`]: resolves a single imported
///   type alias reference.
use std::sync::Arc;

use crate::atom::atom;
use crate::class_lookup::find_class_by_name;
use crate::inheritance::{apply_generic_args, build_generic_subs};
use crate::php_type::PhpType;
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
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
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
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<Arc<ClassInfo>> {
    type_hint_to_classes_typed_depth(ty, owning_class_name, all_classes, class_loader, 0)
}

/// Inner implementation with a recursion depth guard to prevent
/// infinite loops from circular type aliases.
fn type_hint_to_classes_typed_depth(
    ty: &PhpType,
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    depth: u8,
) -> Vec<Arc<ClassInfo>> {
    if depth > MAX_ALIAS_DEPTH {
        return vec![];
    }

    match ty {
        // ── Nullable → unwrap inner ────────────────────────────────
        PhpType::Nullable(inner) => type_hint_to_classes_typed_depth(
            inner,
            owning_class_name,
            all_classes,
            class_loader,
            depth,
        ),

        // ── Union type ─────────────────────────────────────────────
        PhpType::Union(members) => {
            let mut results: Vec<Arc<ClassInfo>> = Vec::new();
            for member in members {
                let resolved = type_hint_to_classes_typed_depth(
                    member,
                    owning_class_name,
                    all_classes,
                    class_loader,
                    depth,
                );
                for cls in resolved {
                    if !results.iter().any(|existing| existing.name == cls.name) {
                        results.push(cls);
                    }
                }
            }
            results
        }

        // ── Intersection type ──────────────────────────────────────
        PhpType::Intersection(members) => {
            let mut results: Vec<Arc<ClassInfo>> = Vec::new();
            for member in members {
                let resolved = type_hint_to_classes_typed_depth(
                    member,
                    owning_class_name,
                    all_classes,
                    class_loader,
                    depth,
                );
                for cls in resolved {
                    if !results.iter().any(|existing| existing.name == cls.name) {
                        results.push(cls);
                    }
                }
            }
            results
        }

        // ── Object shape ───────────────────────────────────────────
        PhpType::ObjectShape(entries) => {
            let properties = SharedVec::from_vec(
                entries
                    .iter()
                    .map(|e| PropertyInfo {
                        name: atom(&e.key.clone().unwrap_or_default()),
                        name_offset: 0,
                        type_hint: Some(e.value_type.clone()),
                        native_type_hint: None,
                        description: None,
                        is_static: false,
                        visibility: Visibility::Public,
                        deprecation_message: None,
                        deprecated_replacement: None,
                        see_refs: Vec::new(),
                        is_virtual: true,
                        source: None,
                    })
                    .collect(),
            );

            vec![Arc::new(ClassInfo {
                name: atom("__object_shape"),
                properties,
                ..ClassInfo::default()
            })]
        }

        // ── Named type (class name, keyword, or alias) ─────────────
        PhpType::Named(name) => resolve_named_type(
            name,
            &[],
            owning_class_name,
            all_classes,
            class_loader,
            depth,
        ),

        // ── Generic type ───────────────────────────────────────────
        PhpType::Generic(name, args) => resolve_named_type(
            name,
            args,
            owning_class_name,
            all_classes,
            class_loader,
            depth,
        ),

        // ── Array slice (T[]) ──────────────────────────────────────
        // Not a class type itself; skip.
        PhpType::Array(_)
        | PhpType::ArrayShape(_)
        | PhpType::Callable { .. }
        | PhpType::ClassString(_)
        | PhpType::InterfaceString(_)
        | PhpType::KeyOf(_)
        | PhpType::ValueOf(_)
        | PhpType::IntRange(..)
        | PhpType::IndexAccess(..)
        | PhpType::Literal(_)
        | PhpType::Conditional { .. }
        | PhpType::Raw(_) => vec![],
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
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    depth: u8,
) -> Vec<Arc<ClassInfo>> {
    // ── Fast reject: built-in scalar/pseudo types can never resolve
    //    to a class and are never type aliases. ──────────────────────
    if is_builtin_non_class_type(name) {
        return vec![];
    }

    // ── Type alias resolution ──────────────────────────────────────
    if let Some(alias_type) = resolve_type_alias_typed(
        &PhpType::Named(name.to_string()),
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
            let rewritten = PhpType::Generic(owning_class_name.to_string(), generic_args.to_vec());
            return type_hint_to_classes_typed_depth(
                &rewritten,
                owning_class_name,
                all_classes,
                class_loader,
                depth,
            );
        }
        return find_class_by_name(all_classes, owning_class_name)
            .map(Arc::clone)
            .or_else(|| class_loader(owning_class_name))
            .into_iter()
            .collect();
    }

    // ── parent ─────────────────────────────────────────────────────
    // Case-insensitive to match PHP behaviour (`Parent::`, `PARENT::` are valid).
    if name.eq_ignore_ascii_case("parent") {
        let parent_name = find_class_by_name(all_classes, owning_class_name)
            .and_then(|c| c.parent_class)
            .or_else(|| class_loader(owning_class_name).and_then(|c| c.parent_class));
        if let Some(parent) = parent_name {
            return find_class_by_name(all_classes, &parent)
                .map(Arc::clone)
                .or_else(|| class_loader(&parent))
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
                    PhpType::Named(owning_class_name.to_string())
                } else {
                    arg.clone()
                }
            })
            .collect();
        &resolved_generic_args
    } else {
        generic_args
    };

    // ── Class lookup ───────────────────────────────────────────────
    // When `name` is a short name (no backslash), prefer a class in the
    // same namespace as the owning class.  This prevents cross-namespace
    // contamination in multi-namespace files where multiple namespaces
    // define a class with the same short name.
    let found = if !name.contains('\\') && owning_class_name.contains('\\') {
        let owning_ns = owning_class_name.rsplit_once('\\').map(|(ns, _)| ns);
        // Try same-namespace first.
        let same_ns = all_classes
            .iter()
            .find(|c| c.name == name && c.file_namespace.as_deref() == owning_ns)
            .map(Arc::clone);
        same_ns
            .or_else(|| find_class_by_name(all_classes, name).map(Arc::clone))
            .or_else(|| class_loader(name))
    } else if !name.contains('\\') && owning_class_name.is_empty() {
        // For unqualified names in top-level code (no enclosing class),
        // prefer the namespace-aware class_loader over find_class_by_name.
        // The class_loader uses the FileContext's namespace to resolve
        // short names correctly, while find_class_by_name returns the
        // first match regardless of namespace in multi-namespace files.
        class_loader(name).or_else(|| find_class_by_name(all_classes, name).map(Arc::clone))
    } else {
        find_class_by_name(all_classes, name)
            .map(Arc::clone)
            .or_else(|| class_loader(name))
    };

    match found {
        Some(cls) => {
            // ── Eloquent custom collection swapping ────────────────
            let cls = laravel::try_swap_custom_collection(
                Arc::unwrap_or_clone(cls),
                name,
                generic_args,
                all_classes,
                class_loader,
            );

            // Apply generic substitution if the type hint carried
            // generic arguments and the class has template parameters.
            if !generic_args.is_empty() && !cls.template_params.is_empty() {
                let generic_arg_strings: Vec<String> =
                    generic_args.iter().map(|a| a.to_string()).collect();
                let resolved = if let Some(cache) = virtual_members::active_resolved_class_cache() {
                    virtual_members::resolve_class_fully_with_generics(
                        &cls,
                        class_loader,
                        Some(cache),
                        &generic_arg_strings,
                        generic_args,
                    )
                } else {
                    let base = virtual_members::resolve_class_fully(&cls, class_loader);
                    if !base.template_params.is_empty() {
                        std::sync::Arc::new(apply_generic_args(&base, generic_args))
                    } else {
                        base
                    }
                };
                let mut result = std::sync::Arc::unwrap_or_clone(resolved);

                // ── Template-param mixin resolution ────────────────
                // When a class declares `@mixin TParam` where `TParam`
                // is a template parameter, the mixin cannot be resolved
                // during `resolve_class_fully` because the concrete type
                // is not yet known.  Now that generic args are concrete,
                // resolve those mixins and merge their members.
                if cls
                    .mixins
                    .iter()
                    .any(|m| cls.template_params.iter().any(|t| t == m.as_str()))
                {
                    let subs = build_generic_subs(&cls, generic_args);
                    if !subs.is_empty() {
                        let mixin_members = virtual_members::phpdoc::resolve_template_param_mixins(
                            &cls,
                            &subs,
                            class_loader,
                        );
                        if !mixin_members.is_empty() {
                            virtual_members::merge_virtual_members(&mut result, mixin_members);
                        }
                    }
                }

                // ── Eloquent Builder scope injection ───────────────
                laravel::try_inject_builder_scopes(
                    &mut result,
                    &cls,
                    name,
                    generic_args,
                    class_loader,
                );

                // ── Inherited Builder mixin scope injection ────────
                // When a class inherits `@mixin Builder<TRelatedModel>`
                // from an ancestor (e.g. HasMany inherits it from
                // Relation), the mixin expansion adds Builder's own
                // methods but not model-specific scopes.  Now that
                // generic args are concrete, resolve the model type
                // and inject its scopes.
                laravel::try_inject_mixin_builder_scopes(
                    &mut result,
                    &cls,
                    generic_args,
                    class_loader,
                );

                vec![Arc::new(result)]
            } else {
                vec![Arc::new(cls)]
            }
        }
        None => {
            // ── Template parameter bound fallback ──────────────────
            let short = short_name(name);
            let loaded;
            let owning = match find_class_by_name(all_classes, owning_class_name) {
                Some(c) => Some(c.as_ref()),
                None => {
                    loaded = class_loader(owning_class_name);
                    loaded.as_deref()
                }
            };

            if let Some(owner) = owning
                && owner.template_params.iter().any(|p| p.as_str() == short)
                && let Some(bound) = owner.template_param_bounds.get(&atom(short))
            {
                return type_hint_to_classes_typed_depth(
                    bound,
                    owning_class_name,
                    all_classes,
                    class_loader,
                    depth + 1,
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

/// Look up a type alias by name and fully expand alias chains.
///
/// Returns the fully expanded type definition string if `hint` is a
/// known alias, or `None` if it is not. Follows up to 10 levels of
/// alias indirection to handle aliases that reference other aliases.
///
/// For imported aliases, the source class is loaded and the original
/// alias is resolved from its `type_aliases` map.
///
/// Pass an empty `owning_class_name` to search all classes without
/// priority (used by the array-key completion path).
use crate::php_type::is_builtin_non_class_type;

pub(crate) fn resolve_type_alias_typed(
    ty: &PhpType,
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    let mut current = ty.clone();
    let mut last_resolved: Option<PhpType> = None;

    for _ in 0..10 {
        // Only bare identifiers can be type aliases.  Skip anything that
        // looks like a complex type expression to avoid false matches.
        if !matches!(current, PhpType::Named(_)) {
            break;
        }

        let expanded =
            resolve_type_alias_once(&current, owning_class_name, all_classes, class_loader);

        match expanded {
            Some(php_type) => {
                current = php_type.clone();
                last_resolved = Some(php_type);
            }
            None => break,
        }
    }

    last_resolved
}

/// Single-level alias lookup (no chaining).
fn resolve_type_alias_once(
    hint: &PhpType,
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    let name = match hint {
        PhpType::Named(n) => n.as_str(),
        _ => return None,
    };

    // Find the owning class to check its type_aliases.
    let owning_class = all_classes.iter().find(|c| c.name == owning_class_name);

    if let Some(cls) = owning_class
        && let Some(def) = cls.type_aliases.get(&atom(name))
    {
        return expand_type_alias_def(def, all_classes, class_loader);
    }

    // Also check all classes in the file — the type alias might be
    // referenced from a method inside a different class that uses the
    // owning class's return type.  This is rare but handles the case
    // where the owning class name is empty (top-level code) or when
    // the type is used in a context where the owning class is not the
    // declaring class.
    for cls in all_classes {
        if cls.name == owning_class_name {
            continue; // Already checked above.
        }
        if let Some(def) = cls.type_aliases.get(&atom(name)) {
            return expand_type_alias_def(def, all_classes, class_loader);
        }
    }

    None
}

/// Expand a [`TypeAliasDef`] into a resolved [`PhpType`].
///
/// For local aliases, returns the `PhpType` directly.
/// For imports, loads the source class and returns the original alias
/// definition.
fn expand_type_alias_def(
    def: &TypeAliasDef,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    match def {
        TypeAliasDef::Local(php_type) => Some(php_type.clone()),
        TypeAliasDef::Import {
            source_class,
            original_name,
        } => resolve_imported_type_alias(source_class, original_name, all_classes, class_loader),
    }
}

/// Resolve an imported type alias reference.
///
/// Loads the source class by `source_class_name` and looks up
/// `original_name` in its `type_aliases` map.
pub(crate) fn resolve_imported_type_alias(
    source_class_name: &str,
    original_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    // Try to find the source class.
    let lookup = source_class_name
        .rsplit('\\')
        .next()
        .unwrap_or(source_class_name);
    let source_class = all_classes
        .iter()
        .find(|c| c.name == lookup)
        .map(|c| ClassInfo::clone(c))
        .or_else(|| class_loader(source_class_name).map(Arc::unwrap_or_clone));

    let source_class = source_class?;
    let def = source_class.type_aliases.get(&atom(original_name))?;

    // Don't follow nested imports — just return the local definition.
    match def {
        TypeAliasDef::Local(php_type) => Some(php_type.clone()),
        TypeAliasDef::Import { .. } => None,
    }
}
