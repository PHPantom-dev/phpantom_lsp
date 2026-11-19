//! The properties Yii form fields and hydration maps name.
//!
//! ```php
//! Field::text($form, 'title');
//! $formHydrator->populateFromPost($form, $request, map: ['title' => 'post_title']);
//! $hydrator->hydrate($post, new ArrayData($data, ['title' => 'post_title']));
//! ```
//!
//! `yiisoft/form-model`'s field helpers take a form model and the name of
//! one of its properties, which the field reads by reflection and throws
//! for when the model has no such property.  The map a `FormHydrator` or an
//! `ArrayData` is given (`yiisoft/hydrator`) is keyed by the properties of
//! the object being filled, which the hydrator writes by reflection
//! whatever their visibility.
//!
//! Symbol-map extraction (`symbol_map/extraction/yii_forms.rs`) emits each
//! name as a [`YiiProperty`](crate::symbol_map::DocblockMemberRef::YiiProperty)
//! access on the object, so the shared member machinery navigates, renames,
//! and checks it.  A field helper and a form hydrator are called through a
//! class the extractor cannot see, so their names are kept only where the
//! object resolves to a form model (see [`form_model_classes`]).

use std::sync::Arc;

use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::types::ClassInfo;

/// The interface every form model implements.
const FORM_MODEL_INTERFACE: &str = "Yiisoft\\FormModel\\FormModelInterface";

/// The data a hydrator fills an object from, with the map of the object's
/// properties to the data's keys.
pub(crate) const ARRAY_DATA: &str = "Yiisoft\\Hydrator\\ArrayData";

/// A map of an object's properties to the data's keys.
pub(crate) const OBJECT_MAP: &str = "Yiisoft\\Hydrator\\ObjectMap";

/// The methods of `Field` and `FieldFactory` that take a form model and the
/// name of one of its properties, as their first two arguments.
const FIELD_METHODS: &[&str] = &[
    "checkbox",
    "checkboxList",
    "date",
    "dateTimeLocal",
    "email",
    "error",
    "file",
    "hidden",
    "hint",
    "label",
    "number",
    "password",
    "radioList",
    "range",
    "select",
    "telephone",
    "text",
    "textarea",
    "time",
    "url",
];

/// The methods of `FormHydrator` that take a form model first and a map of
/// its properties third.
const POPULATE_METHODS: &[&str] = &[
    "populate",
    "populateAndValidate",
    "populateFromGet",
    "populateFromGetAndValidate",
    "populateFromPost",
    "populateFromPostAndValidate",
];

/// Whether `method` is a field helper, which names a property of the form
/// model it is given by its `$property` argument.
pub(crate) fn is_field_method(method: &str) -> bool {
    FIELD_METHODS.iter().any(|m| m.eq_ignore_ascii_case(method))
}

/// Whether `method` is one of `FormHydrator`'s, which keys its `$map`
/// argument by the properties of the form model it is given.
pub(crate) fn is_populate_method(method: &str) -> bool {
    POPULATE_METHODS
        .iter()
        .any(|m| m.eq_ignore_ascii_case(method))
}

/// Whether `method` is `HydratorInterface::hydrate()`, which fills the
/// object it is given from its `$data` argument.
pub(crate) fn is_hydrate_method(method: &str) -> bool {
    method.eq_ignore_ascii_case("hydrate")
}

/// The property of the form model that the property expression a field
/// helper is given names, with its byte offset in the expression.
///
/// The expression is the property, after an optional tabular index
/// (`[0]content`) and before an optional array index (`dates[0]`) or a
/// path into its value (`address.city`).  An expression the form model
/// would read some other way, or would reject, names nothing.
pub(crate) fn field_property(expression: &str) -> Option<(usize, &str)> {
    let mut start = 0;
    while expression[start..].starts_with('[') {
        start += expression[start..].find(']')? + 1;
    }
    let rest = &expression[start..];
    let end = rest.find(['[', '.']).unwrap_or(rest.len());
    let name = &rest[..end];
    is_property_name(name).then_some((start, name))
}

/// Whether `name` is a property name PHP accepts unquoted.
pub(crate) fn is_property_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || !c.is_ascii())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii())
}

/// Whether `class` is a form model.
fn is_form_model(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    class.fqn().as_str() == FORM_MODEL_INTERFACE
        || is_subtype_of(class, FORM_MODEL_INTERFACE, class_loader)
}

/// The form models among `classes`, the classes the object a field helper
/// or a form hydrator is given resolved to.  Anything else takes no
/// property names there, and yields nothing.
pub(crate) fn form_model_classes(
    mut classes: Vec<Arc<ClassInfo>>,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    classes.retain(|class| is_form_model(class, class_loader));
    classes
}

/// [`form_model_classes`] for an object resolved to class names.
pub(crate) fn form_model_class_names(
    mut fqns: Vec<String>,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<String> {
    fqns.retain(|fqn| {
        class_loader(Fqn::new(fqn)).is_some_and(|class| is_form_model(&class, class_loader))
    });
    fqns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_property_reads_the_property_of_an_expression() {
        assert_eq!(field_property("title"), Some((0, "title")));
        assert_eq!(field_property("dates[0]"), Some((0, "dates")));
        assert_eq!(field_property("[0]content"), Some((3, "content")));
        assert_eq!(field_property("[0][1]dates[0]"), Some((6, "dates")));
        assert_eq!(field_property("address.city"), Some((0, "address")));
        assert_eq!(field_property(""), None);
        assert_eq!(field_property("[0"), None);
        assert_eq!(field_property("1st"), None);
        assert_eq!(field_property("a-b"), None);
    }
}
