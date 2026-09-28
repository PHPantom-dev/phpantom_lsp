//! Expanding `@phpstan-type` / `@psalm-type` aliases into the member
//! signatures that name them.
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

use crate::atom::Atom;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ConstantInfo, MethodInfo, PropertyInfo, TypeAliasDef};

use super::resolution::resolve_imported_type_alias;

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
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
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
    let keys = if method.template_params.is_empty() {
        keys
    } else {
        visible = keys
            .iter()
            .filter(|k| !method.template_params.iter().any(|t| t == k.as_str()))
            .cloned()
            .collect();
        &visible
    };
    if keys.is_empty() {
        return false;
    }
    names_any(method.return_type.as_ref(), keys)
        || names_any(method.conditional_return.as_ref(), keys)
        || names_any(method.if_this_is.as_ref(), keys)
        || names_any(method.self_out.as_ref(), keys)
        || method
            .type_assertions
            .iter()
            .any(|a| a.asserted_type.references_any_template_param(keys))
        || method
            .template_param_bounds
            .values()
            .any(|b| b.references_any_template_param(keys))
        || method
            .template_param_defaults
            .iter()
            .any(|(_, d)| d.references_any_template_param(keys))
        || method.parameters.iter().any(|p| {
            names_any(p.type_hint.as_ref(), keys) || names_any(p.param_out_type.as_ref(), keys)
        })
}

fn substitute_in_method(method: &mut MethodInfo, aliases: &HashMap<String, PhpType>) {
    let visible: HashMap<String, PhpType>;
    let aliases = if method
        .template_params
        .iter()
        .any(|t| aliases.contains_key(t.as_str()))
    {
        visible = aliases
            .iter()
            .filter(|(k, _)| !method.template_params.iter().any(|t| t == k.as_str()))
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
    sub(&mut method.conditional_return);
    sub(&mut method.if_this_is);
    sub(&mut method.self_out);
    for assertion in &mut method.type_assertions {
        assertion.asserted_type = assertion.asserted_type.substitute(aliases);
    }
    for bound in method.template_param_bounds.values_mut() {
        *bound = bound.substitute(aliases);
    }
    for (_, default) in method.template_param_defaults.iter_mut() {
        *default = default.substitute(aliases);
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
