//! Drupal forms: the keys a form or render array holds its callbacks
//! under, and the classes a form callback's `'::method'` can name a method
//! of.
//!
//! Drupal rewrites `'::method'` as `[$form_object, 'method']` before it
//! calls it, so the string names a method of the form being built, which
//! is the class it is written in only when that class is a form.  Written
//! in a trait, it names a method of whichever form uses the trait, which
//! the trait stands for.  Anywhere else it names a method of a form the
//! code does not know.

use std::sync::Arc;

use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::types::{ClassInfo, ClassLikeKind};

/// The keys of a form or render array that hold a list of callbacks.
pub(crate) const CALLBACK_LIST_KEYS: [&str; 8] = [
    "#after_build",
    "#element_validate",
    "#entity_builders",
    "#post_render",
    "#pre_render",
    "#process",
    "#submit",
    "#validate",
];

/// The keys that hold one callback.
pub(crate) const CALLBACK_KEYS: [&str; 2] = ["#access_callback", VALUE_CALLBACK];

/// The key whose callback Drupal calls only when it is a native callable.
pub(crate) const VALUE_CALLBACK: &str = "#value_callback";

/// The key whose list holds its callback first, then the arguments.
pub(crate) const LAZY_BUILDER: &str = "#lazy_builder";

/// The key whose array holds its callback under [`AJAX_CALLBACK`].
pub(crate) const AJAX: &str = "#ajax";
pub(crate) const AJAX_CALLBACK: &str = "callback";

/// The callback keys Drupal reads without rewriting `'::method'` for the
/// form, so `'::method'` names nothing under them.
pub(crate) const RENDER_CALLBACK_KEYS: [&str; 4] = [
    "#access_callback",
    LAZY_BUILDER,
    "#post_render",
    "#pre_render",
];

/// The interface every Drupal form implements.
const FORM_INTERFACE: &str = "Drupal\\Core\\Form\\FormInterface";

/// Whether `'::method'` written in `class` names a method of it.
fn names_own_method(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    class.kind == ClassLikeKind::Trait || is_subtype_of(class, FORM_INTERFACE, class_loader)
}

/// The classes among `classes`, those `'::method'` is written in, that it
/// names a method of.
pub(crate) fn form_callback_classes(
    mut classes: Vec<Arc<ClassInfo>>,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    classes.retain(|class| names_own_method(class, class_loader));
    classes
}

/// [`form_callback_classes`] for classes known by name.
pub(crate) fn form_callback_class_names(
    mut fqns: Vec<String>,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<String> {
    fqns.retain(|fqn| {
        class_loader(Fqn::new(fqn)).is_some_and(|class| names_own_method(&class, class_loader))
    });
    fqns
}
