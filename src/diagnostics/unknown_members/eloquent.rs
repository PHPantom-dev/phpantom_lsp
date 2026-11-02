//! Eloquent's `__call` accepts a closed set of names.
//!
//! A method call on a class with `__call` is normally taken as valid, since
//! the handler may accept any name.  Eloquent's handlers do not:
//! `Model::__call` forwards to the model's query builder, the Eloquent
//! builder's `__call` answers its macros and the model's scopes before
//! forwarding to the base query builder, and that one answers its macros and
//! `where{Column}` calls and throws `BadMethodCallException` for anything
//! else.  Everything in that set apart from the `where` prefix is already a
//! member of the resolved model or builder (the builder's methods are
//! forwarded onto the model, the scopes and macros are injected), so a name
//! the member lookup did not find is one the chain rejects.
//!
//! Properties follow the same reasoning when the app turns on
//! `Model::preventAccessingMissingAttributes()`: `Model::__get` then throws
//! for an attribute the model does not have instead of returning `null`.
//!
//! The judgment stays silent wherever the set is open: a model or builder
//! that overrides `__call` itself, a builder whose model is not known (so
//! its scopes are not either), and registrations the macro scan saw but
//! could not read.

use std::sync::Arc;

use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::inheritance::ancestors;
use crate::php_type::{PhpType, TypeKind};
use crate::types::ClassInfo;
use crate::virtual_members::laravel::helpers::{extends_eloquent_builder, extends_eloquent_model};
use crate::virtual_members::laravel::{
    ELOQUENT_BUILDER_FQN, ELOQUENT_MODEL_FQN, custom_builder_fqn, model_schema_is_known,
};
use crate::virtual_members::resolve_class_fully_cached;

use super::UnknownMemberPass;
use super::access::MemberAccessSite;

const QUERY_BUILDER_FQN: &str = "Illuminate\\Database\\Query\\Builder";

/// The macros `SoftDeletingScope::extend()` registers on the builder of a
/// model that uses `SoftDeletes`.  They are local to the builder instance, so
/// no `Builder::macro()` call names them.
const SOFT_DELETE_BUILDER_MACROS: &[&str] = &[
    "restore",
    "restoreOrCreate",
    "createOrRestore",
    "withTrashed",
    "withoutTrashed",
    "onlyTrashed",
];

impl UnknownMemberPass<'_> {
    /// Whether the call `access` makes is rejected by Eloquent on the one
    /// class among `classes` whose magic handler would take it.
    ///
    /// `classes[i]` is the merged form of the class resolved from
    /// `types[i]`.  Returns `false` when no class has an Eloquent handler, or
    /// when any handler might accept the name.
    pub(super) fn eloquent_rejects_call(
        &self,
        access: &MemberAccessSite,
        classes: &[Arc<ClassInfo>],
        types: &[PhpType],
        has_magic_call: impl Fn(usize) -> bool,
    ) -> bool {
        let mut any = false;
        for (i, class) in classes.iter().enumerate() {
            if !has_magic_call(i) {
                continue;
            }
            if !self.eloquent_rejects_on(access, class, types.get(i)) {
                return false;
            }
            any = true;
        }
        any
    }

    fn eloquent_rejects_on(
        &self,
        access: &MemberAccessSite,
        class: &ClassInfo,
        ty: Option<&PhpType>,
    ) -> bool {
        let loader = self.class_loader();
        let name = access.member_name;
        // `Query\Builder::__call` sends every name with this prefix to
        // `dynamicWhere()`, which never throws.  The check is
        // `str_starts_with`, so it is case-sensitive.
        if name.starts_with("where") {
            return false;
        }
        // The helpers compare the class's own `name`, which may be short,
        // so the base classes themselves are matched by FQN here.
        let fqn = class.fqn();
        if fqn == ELOQUENT_MODEL_FQN {
            // A receiver typed as `Model` itself may be any model, with any
            // scopes.
            false
        } else if extends_eloquent_model(class, loader) {
            self.model_rejects(access, class)
        } else if !access.is_static
            && (fqn == ELOQUENT_BUILDER_FQN || extends_eloquent_builder(class, loader))
        {
            let Some(model) = ty.and_then(builder_model_name) else {
                return false;
            };
            let Some(model) = self.concrete_model(model) else {
                return false;
            };
            self.builder_rejects(name, &class.fqn(), &model)
        } else {
            false
        }
    }

    /// Whether `class` is a model whose `__get` throws for an attribute it
    /// does not have, and whose attributes are all known.
    ///
    /// That takes the app turning on
    /// `Model::preventAccessingMissingAttributes()`, the model leaving
    /// Eloquent's `__get` in place, and the migrations declaring its table:
    /// a column the scan does not know about is still a column.
    pub(super) fn eloquent_rejects_missing_attributes(&self, class: &ClassInfo) -> bool {
        if !self.strict_attributes {
            return false;
        }
        let loader = self.class_loader();
        class.fqn() != ELOQUENT_MODEL_FQN
            && extends_eloquent_model(class, loader)
            && !declares_below(class, &["__get"], ELOQUENT_MODEL_FQN, loader)
            && model_schema_is_known(class, &self.backend.resolved_class_cache)
    }

    /// `Model::__callStatic` makes a new instance and calls the method on it,
    /// so a static call ends up in `Model::__call` too.
    fn model_rejects(&self, access: &MemberAccessSite, model: &ClassInfo) -> bool {
        let loader = self.class_loader();
        let name = access.member_name;
        let model_fqn = model.fqn();
        let handlers: &[&str] = if access.is_static {
            &["__call", "__callStatic"]
        } else {
            &["__call"]
        };
        if declares_below(model, handlers, ELOQUENT_MODEL_FQN, loader) {
            return false;
        }
        // `through{Relation}()` builds a pending has-one-or-many-through
        // relation from the relation method it names.
        if let Some(relation) = name.strip_prefix("through")
            && !relation.is_empty()
            && model
                .methods
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case(relation))
        {
            return false;
        }
        let model_chain = std::iter::once(model_fqn.to_string())
            .chain(ancestors(model, loader).map(|(_, parent)| parent.fqn().to_string()));
        if self.macro_scan_may_answer(model_chain, name) {
            return false;
        }
        let builder_fqn =
            custom_builder_fqn(model, loader).unwrap_or_else(|| ELOQUENT_BUILDER_FQN.to_string());
        let Some(builder) = loader(Fqn::new(&builder_fqn)) else {
            return false;
        };
        self.builder_rejects(name, &builder.fqn(), model)
    }

    /// Whether the Eloquent builder `builder_fqn`, building `model`, rejects
    /// `name`, given that the merged builder does not declare it.
    fn builder_rejects(&self, name: &str, builder_fqn: &str, model: &ClassInfo) -> bool {
        let loader = self.class_loader();
        if builder_fqn != ELOQUENT_BUILDER_FQN {
            let Some(builder) = loader(Fqn::new(builder_fqn)) else {
                return false;
            };
            if declares_below(&builder, &["__call"], ELOQUENT_BUILDER_FQN, loader) {
                return false;
            }
        }
        let soft_deletes =
            resolve_class_fully_cached(model, loader, &self.backend.resolved_class_cache)
                .laravel()
                .is_some_and(|l| l.soft_deletes);
        if soft_deletes && SOFT_DELETE_BUILDER_MACROS.contains(&name) {
            return false;
        }
        let builders = [builder_fqn, ELOQUENT_BUILDER_FQN, QUERY_BUILDER_FQN];
        !self.macro_scan_may_answer(builders.iter().map(|s| s.to_string()), name)
    }

    /// Whether a registration the macro scan could not type may make `name`
    /// callable on one of `targets`.
    fn macro_scan_may_answer(&self, targets: impl Iterator<Item = String>, name: &str) -> bool {
        let index = self.backend.laravel.macros.read();
        targets
            .into_iter()
            .any(|target| index.may_answer_call(&target, name))
    }

    /// The model named `fqn`, when it is a concrete subclass of Eloquent's
    /// `Model` rather than `Model` itself or a template parameter.
    fn concrete_model(&self, fqn: &str) -> Option<Arc<ClassInfo>> {
        let loader = self.class_loader();
        let model = loader(Fqn::new(fqn))?;
        (model.fqn() != ELOQUENT_MODEL_FQN && extends_eloquent_model(&model, loader))
            .then_some(model)
    }
}

/// The model a builder type is parameterised with, `Builder<Order>` giving
/// `Order`.
fn builder_model_name(ty: &PhpType) -> Option<&str> {
    match ty.kind() {
        TypeKind::Generic(generic) => generic.args.first()?.base_name(),
        TypeKind::Nullable(inner) => builder_model_name(inner),
        _ => None,
    }
}

/// Whether `class`, a parent of it below `stop_fqn`, or a trait any of them
/// uses declares one of `methods`.
fn declares_below(
    class: &ClassInfo,
    methods: &[&str],
    stop_fqn: &str,
    loader: &ResolvedClassLoader<'_>,
) -> bool {
    let declares = |c: &ClassInfo| {
        c.methods
            .iter()
            .any(|m| methods.iter().any(|name| m.name.eq_ignore_ascii_case(name)))
    };
    // The class in hand may be the merged form, which carries the
    // framework's own handler, so its declared form is read instead.
    let own = loader(Fqn::new(&class.fqn()));
    let chain = own
        .into_iter()
        .chain(ancestors(class, loader).map(|(_, parent)| parent));
    let mut seen_traits = crate::atom::AtomSet::default();
    for c in chain {
        if c.fqn() == stop_fqn {
            return false;
        }
        if declares(&c) || uses_trait_declaring(&c, &declares, loader, &mut seen_traits) {
            return true;
        }
    }
    false
}

/// Whether a trait `class` uses, directly or through another trait,
/// satisfies `declares`.
fn uses_trait_declaring(
    class: &ClassInfo,
    declares: &dyn Fn(&ClassInfo) -> bool,
    loader: &ResolvedClassLoader<'_>,
    seen: &mut crate::atom::AtomSet,
) -> bool {
    for name in &class.used_traits {
        if !seen.insert(*name) {
            continue;
        }
        let Some(t) = loader(Fqn::new(name)) else {
            continue;
        };
        if declares(&t) || uses_trait_declaring(&t, declares, loader, seen) {
            return true;
        }
    }
    false
}
