//! Type aliases: `@phpstan-type` / `@psalm-type` definitions and
//! `@phpstan-import-type` imports, resolved to the types they stand for
//! ([`resolve_type_alias_typed`], [`expand_nested_type_aliases`]) and
//! expanded into the member signatures that name them.
//!
//! An alias is scoped to the class that declares or imports it, but a member
//! signature is read by every file that uses the class.  Once the signature
//! has left its declaring file nothing records which class a bare alias name
//! belongs to, so the name is replaced by its definition while that scope is
//! still known:
//!
//! - **Local aliases** are expanded when the declaring file is parsed
//!   ([`expand_local_type_aliases`]), where every class of the file is in
//!   scope.
//! - **Imported aliases** name a class in another file, which may not be
//!   loadable yet and can change independently of the importer.  They are
//!   linked when the class is loaded ([`link_imported_type_aliases`]), and
//!   the linked copy is cached until the class or a class it read an import
//!   from changes.

use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::{Atom, atom};
use crate::class_lookup::{Fqn, ResolvedClassLoader, find_class_by_fqn};
use crate::php_type::{PhpType, TypeKind, is_builtin_non_class_type};
use crate::types::*;

/// Replace every alias of `classes` named in their member signatures with
/// its definition.
///
/// Called on the classes of one file (one namespace block) after their names
/// have been qualified.  A class's own alias wins over another class's of the
/// same name, and a class-level `@template` or an import of the same name
/// hides it.  An alias body that names another alias is expanded first; a
/// cycle is left unexpanded where it recurs rather than unrolled.
pub(crate) fn expand_local_type_aliases(classes: &mut [ClassInfo]) {
    let mut names: Vec<String> = Vec::new();
    for class in classes.iter() {
        for name in class.type_aliases.keys() {
            if !names.iter().any(|n| n == name.as_str()) {
                names.push(name.to_string());
            }
        }
    }
    if names.is_empty() {
        return;
    }

    let per_class: Vec<HashMap<String, PhpType>> = {
        let mut expander = LocalAliasExpander {
            classes,
            names: &names,
            memo: HashMap::new(),
            visiting: Vec::new(),
            cycle_cut: false,
        };
        (0..classes.len())
            .map(|i| {
                let mut subs = HashMap::new();
                for name in &names {
                    if classes[i]
                        .template_params
                        .iter()
                        .any(|t| t == name.as_str())
                    {
                        continue;
                    }
                    if let Some(expanded) = expander.lookup(i, name) {
                        subs.insert(name.clone(), expanded);
                    }
                }
                subs
            })
            .collect()
    };

    for (class, subs) in classes.iter_mut().zip(per_class) {
        if !subs.is_empty() {
            substitute_member_aliases(class, &subs);
        }
    }
}

struct LocalAliasExpander<'a> {
    classes: &'a [ClassInfo],
    names: &'a [String],
    /// Expansions that did not run into a cycle, keyed by the declaring
    /// class's index.  One that did depends on where the walk entered the
    /// cycle, so it is recomputed each time.
    memo: HashMap<(usize, Atom), PhpType>,
    visiting: Vec<(usize, Atom)>,
    /// Set when the expansion under way stopped at a cycle.
    cycle_cut: bool,
}

impl LocalAliasExpander<'_> {
    /// The expansion of `name` as seen from the class at `from`, or `None`
    /// when it is not a local alias in that scope (or recurs in a cycle).
    fn lookup(&mut self, from: usize, name: &str) -> Option<PhpType> {
        let name = crate::atom::atom(name);
        let owner = if self.classes[from].type_aliases.contains_key(&name) {
            from
        } else {
            self.classes
                .iter()
                .position(|c| c.type_aliases.contains_key(&name))?
        };
        // An import is linked during class resolution instead, and hides
        // any other class's local alias of the same name.
        let Some(TypeAliasDef::Local(body)) = self.classes[owner].type_aliases.get(&name) else {
            return None;
        };
        let key = (owner, name);
        if let Some(done) = self.memo.get(&key) {
            return Some(done.clone());
        }
        if self.visiting.contains(&key) {
            self.cycle_cut = true;
            return None;
        }
        self.visiting.push(key);
        let outer_cut = std::mem::replace(&mut self.cycle_cut, false);
        let mut subs = HashMap::new();
        for alias in self.names {
            if body.references_any_template_param(std::slice::from_ref(alias))
                && let Some(expanded) = self.lookup(owner, alias)
            {
                subs.insert(alias.clone(), expanded);
            }
        }
        let expanded = body.substitute(&subs);
        let cyclic = self.cycle_cut;
        self.cycle_cut = outer_cut || cyclic;
        self.visiting.pop();
        if !cyclic {
            self.memo.insert(key, expanded.clone());
        }
        Some(expanded)
    }
}

/// A copy of `class` with the aliases it imports with
/// `@phpstan-import-type` linked into the member signatures it declares
/// itself, or `None` when none of them names one.
///
/// The second value is whether every import resolved: a source class that
/// cannot be loaded yet may be indexed later, so a result missing one is
/// not worth keeping.
///
/// The members a class inherits are linked when their declaring class is
/// loaded, with that class's imports rather than the inheriting class's.
pub(crate) fn link_imported_type_aliases(
    class: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> (Option<ClassInfo>, bool) {
    let mut subs = HashMap::new();
    let mut complete = true;
    for (name, def) in class.type_aliases.iter() {
        let TypeAliasDef::Import {
            source_class,
            original_name,
        } = def
        else {
            continue;
        };
        if class.template_params.contains(name) {
            continue;
        }
        match resolve_imported_type_alias(source_class, original_name, &[], class_loader) {
            Some(resolved) => {
                subs.insert(name.to_string(), resolved);
            }
            None => complete = false,
        }
    }
    if subs.is_empty() {
        return (None, complete);
    }
    let mut linked = class.clone();
    let changed = substitute_member_aliases(&mut linked, &subs);
    (changed.then_some(linked), complete)
}

/// Whether `class` imports any type alias.
pub(crate) fn has_imported_type_aliases(class: &ClassInfo) -> bool {
    class
        .type_aliases
        .values()
        .any(|d| matches!(d, TypeAliasDef::Import { .. }))
}

/// Substitute `aliases` into every signature `class` declares: methods,
/// properties, constants, `@method` / `@property` tags, and the type
/// arguments of `@extends` / `@implements` / `@use` / `@mixin`.  A method's
/// own `@template` of the same name hides an alias inside that method.
///
/// Returns whether anything changed.  Members that name no alias keep
/// their shared `Arc`.
pub(crate) fn substitute_member_aliases(
    class: &mut ClassInfo,
    aliases: &HashMap<String, PhpType>,
) -> bool {
    let keys: Vec<String> = aliases.keys().cloned().collect();
    let mut changed = false;

    if class.methods.iter().any(|m| method_names_alias(m, &keys)) {
        for method in class.methods.make_mut() {
            if method_names_alias(method, &keys) {
                substitute_in_method(Arc::make_mut(method), aliases);
                changed = true;
            }
        }
    }

    if class
        .properties
        .iter()
        .any(|p| property_names_alias(p, &keys))
    {
        for prop in class.properties.make_mut() {
            if property_names_alias(prop, &keys) {
                let prop = Arc::make_mut(prop);
                prop.type_hint = prop.type_hint.as_ref().map(|t| t.substitute(aliases));
                changed = true;
            }
        }
    }

    if class
        .constants
        .iter()
        .any(|c| constant_names_alias(c, &keys))
    {
        for constant in class.constants.make_mut() {
            if constant_names_alias(constant, &keys) {
                let constant = Arc::make_mut(constant);
                constant.type_hint = constant.type_hint.as_ref().map(|t| t.substitute(aliases));
                changed = true;
            }
        }
    }

    if let Some(doc) = class.doc_members.as_deref() {
        let names_alias = doc.methods.iter().any(|m| method_names_alias(m, &keys))
            || doc
                .properties
                .iter()
                .any(|(_, t)| names_any(t.as_ref(), &keys));
        if names_alias {
            let mut doc = doc.clone();
            for method in &mut doc.methods {
                if method_names_alias(method, &keys) {
                    substitute_in_method(Arc::make_mut(method), aliases);
                }
            }
            for (_, ty) in &mut doc.properties {
                if names_any(ty.as_ref(), &keys) {
                    *ty = ty.as_ref().map(|t| t.substitute(aliases));
                }
            }
            class.doc_members = Some(Arc::new(doc));
            changed = true;
        }
    }

    for generics in [
        &mut class.extends_generics,
        &mut class.implements_generics,
        &mut class.use_generics,
        &mut class.mixin_generics,
    ] {
        for (_, args) in generics.iter_mut() {
            for arg in args.iter_mut() {
                if arg.references_any_template_param(&keys) {
                    *arg = arg.substitute(aliases);
                    changed = true;
                }
            }
        }
    }

    changed
}

fn names_any(ty: Option<&PhpType>, keys: &[String]) -> bool {
    ty.is_some_and(|t| t.references_any_template_param(keys))
}

fn property_names_alias(prop: &PropertyInfo, keys: &[String]) -> bool {
    names_any(prop.type_hint.as_ref(), keys)
}

fn constant_names_alias(constant: &ConstantInfo, keys: &[String]) -> bool {
    names_any(constant.type_hint.as_ref(), keys)
}

/// Whether [`substitute_in_method`] would rewrite anything in `method`.
fn method_names_alias(method: &MethodInfo, keys: &[String]) -> bool {
    let visible: Vec<String>;
    let keys = if method.template_params().is_empty() {
        keys
    } else {
        visible = keys
            .iter()
            .filter(|k| !method.template_params().iter().any(|t| t == k.as_str()))
            .cloned()
            .collect();
        &visible
    };
    if keys.is_empty() {
        return false;
    }
    names_any(method.return_type.as_ref(), keys)
        || names_any(method.conditional_return(), keys)
        || names_any(method.if_this_is(), keys)
        || names_any(method.self_out(), keys)
        || method
            .type_assertions()
            .iter()
            .any(|a| a.asserted_type.references_any_template_param(keys))
        || method
            .template_param_bounds()
            .values()
            .any(|b| b.references_any_template_param(keys))
        || method
            .template_param_defaults()
            .iter()
            .any(|(_, d)| d.references_any_template_param(keys))
        || method.parameters.iter().any(|p| {
            names_any(p.type_hint.as_ref(), keys) || names_any(p.param_out_type.as_ref(), keys)
        })
}

fn substitute_in_method(method: &mut MethodInfo, aliases: &HashMap<String, PhpType>) {
    let visible: HashMap<String, PhpType>;
    let aliases = if method
        .template_params()
        .iter()
        .any(|t| aliases.contains_key(t.as_str()))
    {
        visible = aliases
            .iter()
            .filter(|(k, _)| !method.template_params().iter().any(|t| t == k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        &visible
    } else {
        aliases
    };
    let sub = |ty: &mut Option<PhpType>| {
        if let Some(t) = ty.as_mut() {
            *t = t.substitute(aliases);
        }
    };
    sub(&mut method.return_type);
    if let Some(extras) = method.extras.as_deref_mut() {
        sub(&mut extras.conditional_return);
        sub(&mut extras.if_this_is);
        sub(&mut extras.self_out);
        for assertion in &mut extras.type_assertions {
            assertion.asserted_type = assertion.asserted_type.substitute(aliases);
        }
        for bound in extras.template_param_bounds.values_mut() {
            *bound = bound.substitute(aliases);
        }
        for (_, default) in extras.template_param_defaults.iter_mut() {
            *default = default.substitute(aliases);
        }
    }
    let keys: Vec<String> = aliases.keys().cloned().collect();
    if method.parameters.iter().any(|p| {
        names_any(p.type_hint.as_ref(), &keys) || names_any(p.param_out_type.as_ref(), &keys)
    }) {
        let is_virtual = method.is_virtual;
        for param in method.parameters.make_mut() {
            // A `@method` tag's parameter mirrors its docblock type into
            // the native slot, which has to follow it.
            if is_virtual && param.native_type_hint == param.type_hint {
                sub(&mut param.native_type_hint);
            }
            sub(&mut param.type_hint);
            sub(&mut param.param_out_type);
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
pub(crate) fn resolve_type_alias_typed(
    ty: &PhpType,
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    let mut current = ty.clone();
    let mut last_resolved: Option<PhpType> = None;

    for _ in 0..10 {
        // Only bare identifiers can be type aliases.  Skip anything that
        // looks like a complex type expression to avoid false matches.
        if !matches!(current.kind(), TypeKind::Named(_)) {
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

/// Expand every type alias named anywhere inside `ty` (`Shape|null`,
/// `list<Row>`), or `None` when it names none.
///
/// [`resolve_type_alias_typed`] only expands a type that *is* an alias; a
/// declared type that merely contains one needs this walk.  An alias body
/// that names another alias is expanded too, up to the same ten levels the
/// chain lookup allows, so a cycle stops rather than recursing forever.
pub(crate) fn expand_nested_type_aliases(
    ty: &PhpType,
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    // Aliases are declared on a class in the file, so a file that declares
    // none cannot name one.
    if all_classes.iter().all(|c| c.type_aliases.is_empty()) {
        return None;
    }
    fn walk(
        ty: &PhpType,
        depth: u8,
        owning_class_name: &str,
        all_classes: &[Arc<ClassInfo>],
        class_loader: &ResolvedClassLoader<'_>,
    ) -> PhpType {
        if depth >= 10 {
            return ty.clone();
        }
        if matches!(ty.kind(), TypeKind::Named(_)) {
            return match resolve_type_alias_typed(ty, owning_class_name, all_classes, class_loader)
            {
                Some(expanded) => walk(
                    &expanded,
                    depth + 1,
                    owning_class_name,
                    all_classes,
                    class_loader,
                ),
                None => ty.clone(),
            };
        }
        ty.map_children(&|child| walk(child, depth, owning_class_name, all_classes, class_loader))
    }
    let expanded = walk(ty, 0, owning_class_name, all_classes, class_loader);
    (expanded != *ty).then_some(expanded)
}

/// [`expand_nested_type_aliases`] for a type read off a member of `class`,
/// which may be declared in another file.
///
/// The file-scoped lookup only sees the classes of the file being
/// analysed, but a member signature names the aliases of the class that
/// declares it. An inherited or trait-imported member no longer records
/// that class, so a bare alias name the file cannot resolve is looked up
/// on `class`, the traits it uses and its ancestors, nearest first.
pub(crate) fn expand_member_type_aliases(
    ty: &PhpType,
    class: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    if let Some(expanded) = expand_nested_type_aliases(ty, &class.fqn(), all_classes, class_loader)
    {
        return Some(expanded);
    }
    let TypeKind::Named(name) = ty.kind() else {
        return None;
    };
    if is_builtin_non_class_type(name) {
        return None;
    }
    let name = atom(name);
    let mut queue: Vec<Arc<ClassInfo>> = Vec::new();
    let mut visited = crate::atom::AtomSet::default();
    let mut loaded_current: Option<Arc<ClassInfo>> = None;
    let mut index = 0;
    loop {
        let current: &ClassInfo = loaded_current.as_deref().unwrap_or(class);
        if let Some(def) = current.type_aliases.get(&name) {
            let expanded = expand_type_alias_def(def, all_classes, class_loader)?;
            return Some(
                expand_nested_type_aliases(&expanded, &current.fqn(), all_classes, class_loader)
                    .unwrap_or(expanded),
            );
        }
        for related in current
            .used_traits
            .iter()
            .chain(current.parent_class.iter())
        {
            if visited.insert(*related)
                && let Some(loaded) = class_loader(Fqn::new(related))
            {
                queue.push(loaded);
            }
        }
        loaded_current = Some(Arc::clone(queue.get(index)?));
        index += 1;
    }
}

/// Single-level alias lookup (no chaining).
fn resolve_type_alias_once(
    hint: &PhpType,
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    let name = match hint.kind() {
        TypeKind::Named(n) => n.as_str(),
        _ => return None,
    };

    let owning_class = find_class_by_fqn(all_classes, owning_class_name);

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
        if owning_class.is_some_and(|owner| Arc::ptr_eq(owner, cls)) {
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
    class_loader: &ResolvedClassLoader<'_>,
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
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    let visiting = std::cell::RefCell::new(Vec::new());
    resolve_imported_alias_in_source(
        source_class_name,
        original_name,
        all_classes,
        class_loader,
        &visiting,
    )
}

/// [`resolve_imported_type_alias`], with the imports already being
/// resolved on the way here.
///
/// An imported alias is written in its source class's scope, so any alias
/// its body names (`array{r: RDto}` under `@import-type RDto from R`) is
/// that class's, not the importer's, and is expanded there. An import of an
/// import is followed the same way. `visiting` holds the `(class, alias)`
/// pairs being resolved, so an alias that names itself through a cycle of
/// imports resolves to `mixed` where it recurs, as PHPStan gives up on a
/// circular alias. Leaving the name there instead would hand it back to the
/// caller's own alias walk, which would expand the cycle once more per
/// level it allows.
fn resolve_imported_alias_in_source(
    source_class_name: &str,
    original_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
    visiting: &std::cell::RefCell<Vec<(String, String)>>,
) -> Option<PhpType> {
    let lookup = source_class_name
        .rsplit('\\')
        .next()
        .unwrap_or(source_class_name);
    let source_class = all_classes
        .iter()
        .find(|c| c.name == lookup)
        .cloned()
        .or_else(|| class_loader(Fqn::new(source_class_name)))?;
    let def = source_class.type_aliases.get(&atom(original_name))?;

    let key = (source_class_name.to_string(), original_name.to_string());
    if visiting.borrow().contains(&key) {
        return Some(PhpType::mixed());
    }
    visiting.borrow_mut().push(key);
    let resolved = match def {
        TypeAliasDef::Local(php_type) => Some(expand_aliases_in_source(
            php_type,
            &source_class,
            class_loader,
            visiting,
        )),
        TypeAliasDef::Import {
            source_class,
            original_name,
        } => resolve_imported_alias_in_source(
            source_class,
            original_name,
            &[],
            class_loader,
            visiting,
        ),
    };
    visiting.borrow_mut().pop();
    resolved
}

/// Expand the aliases `source_class` declares or imports wherever they are
/// named inside `ty`.
fn expand_aliases_in_source(
    ty: &PhpType,
    source_class: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
    visiting: &std::cell::RefCell<Vec<(String, String)>>,
) -> PhpType {
    if source_class.type_aliases.is_empty() {
        return ty.clone();
    }
    if let TypeKind::Named(name) = ty.kind()
        && let Some(def) = source_class.type_aliases.get(name)
    {
        let expanded = match def {
            TypeAliasDef::Local(body) => {
                let key = (source_class.fqn().to_string(), name.to_string());
                if visiting.borrow().contains(&key) {
                    return PhpType::mixed();
                }
                visiting.borrow_mut().push(key);
                let expanded = expand_aliases_in_source(body, source_class, class_loader, visiting);
                visiting.borrow_mut().pop();
                Some(expanded)
            }
            TypeAliasDef::Import {
                source_class,
                original_name,
            } => resolve_imported_alias_in_source(
                source_class,
                original_name,
                &[],
                class_loader,
                visiting,
            ),
        };
        return expanded.unwrap_or_else(|| ty.clone());
    }
    ty.map_children(&|child| expand_aliases_in_source(child, source_class, class_loader, visiting))
}
