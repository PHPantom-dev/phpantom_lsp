//! The model class of a `yiisoft/active-record` query.
//!
//! An `ActiveQuery` is built for one record class, and returns instances of
//! it, but the package spells every query `ActiveQueryInterface` and every
//! result `ActiveRecordInterface|array|null`, so `Post::query()->one()`
//! forgets it is a `Post`. The query becomes generic over its model:
//! `query()` and `createQuery()` build one for the class they are called
//! on, `hasOne()` and `hasMany()` one for the class they are given, and
//! the query's results are that class. The `array` alternative stays, for
//! a query switched to `asArray()`.

use crate::atom::{Atom, atom};
use crate::php_type::PhpType;
use crate::types::ClassInfo;

use super::classes::{add_templates, method_index, update_method_at};

const QUERY_INTERFACE: &str = "Yiisoft\\ActiveRecord\\ActiveQueryInterface";
const QUERY: &str = "Yiisoft\\ActiveRecord\\ActiveQuery";
const RECORD_INTERFACE: &str = "Yiisoft\\ActiveRecord\\ActiveRecordInterface";

/// The query's template: the record class it returns.
const MODEL: &str = "TModel";

/// Make the query and record classes of `yiisoft/active-record` say which
/// record a query is for; `false` for any other class.
pub(super) fn patch(class: &mut ClassInfo) -> bool {
    match class.fqn().as_str() {
        QUERY_INTERFACE => patch_query(class),
        QUERY => {
            patch_query(class);
            if !class
                .implements_generics
                .iter()
                .any(|(name, _)| name.as_str() == QUERY_INTERFACE)
            {
                class
                    .implements_generics
                    .push((atom(QUERY_INTERFACE), vec![model()]));
            }
            bind_constructor_model(class);
        }
        RECORD_INTERFACE | "Yiisoft\\ActiveRecord\\AbstractActiveRecord" => patch_record(class),
        "Yiisoft\\ActiveRecord\\Trait\\EventsTrait" => patch_static_query(class),
        _ => return false,
    }
    true
}

fn model() -> PhpType {
    PhpType::named(atom(MODEL))
}

fn query_of(model: PhpType) -> PhpType {
    PhpType::generic(QUERY_INTERFACE, vec![model])
}

/// `array<string, mixed>`, a row of a query switched to `asArray()`.
fn row() -> PhpType {
    PhpType::generic_array(PhpType::string(), PhpType::mixed())
}

/// Give the query its model template and type its results with it.
fn patch_query(class: &mut ClassInfo) {
    if !class.template_params.is_empty() {
        return;
    }
    add_templates(class, &[(MODEL, Some(RECORD_INTERFACE))]);
    let one = PhpType::union(vec![model(), row(), PhpType::null()]);
    set_return(class, "one", one.clone());
    set_return(class, "findByPk", one);
    set_return(
        class,
        "all",
        PhpType::generic_array_val(PhpType::union(vec![model(), row()])),
    );
    set_return(class, "getModel", model());
}

/// Bind the query's model from the record class or instance its
/// constructor is given.
fn bind_constructor_model(class: &mut ClassInfo) {
    let Some(idx) = method_index(class, "__construct") else {
        return;
    };
    let templates = class.template_params.clone();
    update_method_at(class, idx, |ctor| {
        let Some(param) = ctor.parameters.first().map(|p| p.name) else {
            return;
        };
        let extras = ctor.extras_mut();
        if extras.template_params.is_empty() {
            extras.template_params = templates;
        }
        extras.template_bindings.push((atom(MODEL), param));
        if let Some(param) = ctor.parameters.make_mut().first_mut() {
            param.type_hint = Some(model_or_class_string(atom(MODEL)));
        }
    });
}

/// `T|class-string<T>`, what a parameter that names a record class takes.
fn model_or_class_string(template: Atom) -> PhpType {
    PhpType::union(vec![
        PhpType::named(template),
        PhpType::class_string(Some(PhpType::named(template))),
    ])
}

/// Type the relation builders, and the query factories, of a record.
fn patch_record(class: &mut ClassInfo) {
    patch_static_query(class);
    for name in ["hasOne", "hasMany"] {
        patch_relation_builder(class, name);
    }
    if let Some(idx) = method_index(class, "createQuery") {
        update_method_at(class, idx, |method| {
            method.return_type = Some(query_for_argument(
                "$modelClass",
                PhpType::named(atom("static")),
            ));
        });
    }
}

/// `query()` builds a query for the class it is called on, unless it is
/// given another.
fn patch_static_query(class: &mut ClassInfo) {
    if let Some(idx) = method_index(class, "query") {
        update_method_at(class, idx, |method| {
            method.return_type = Some(query_for_argument(
                "$modelClass",
                PhpType::named(atom("static")),
            ));
        });
    }
}

/// A query for `own` when `param` is left `null`, and for some record
/// otherwise.
fn query_for_argument(param: &str, own: PhpType) -> PhpType {
    PhpType::conditional(
        param,
        false,
        PhpType::null(),
        query_of(own),
        query_of(PhpType::named(atom(RECORD_INTERFACE))),
    )
}

/// `hasOne(Profile::class, …)` builds a query for `Profile`.
fn patch_relation_builder(class: &mut ClassInfo, name: &str) {
    let Some(idx) = method_index(class, name) else {
        return;
    };
    let template = atom("TRelated");
    update_method_at(class, idx, |method| {
        let Some(param) = method.parameters.first().map(|p| p.name) else {
            return;
        };
        if let Some(first) = method.parameters.make_mut().first_mut() {
            first.type_hint = Some(model_or_class_string(template));
        }
        method.return_type = Some(query_of(PhpType::named(template)));
        let extras = method.extras_mut();
        if extras.template_params.is_empty() {
            extras.template_params.push(template);
            extras
                .template_param_bounds
                .insert(template, PhpType::named(atom(RECORD_INTERFACE)));
            extras.template_bindings.push((template, param));
        }
    });
}

fn set_return(class: &mut ClassInfo, method: &str, ty: PhpType) {
    if let Some(idx) = method_index(class, method) {
        update_method_at(class, idx, |method| method.return_type = Some(ty));
    }
}
