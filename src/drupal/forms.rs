//! Drupal forms: the classes a form callback's `'::method'` can name a
//! method of.
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
