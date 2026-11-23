//! Return types Drupal core's docblocks leave wider than the call.
//!
//! A query's `extend()` is declared to return `ExtendableInterface`, but it
//! builds the extender class it is given, so
//! `->extend(PagerSelectExtender::class)->limit(10)` forgets the pager's
//! methods. The method becomes generic over that class.
//!
//! Core declares `string|GeneratedUrl` on the URL methods that take
//! `$collect_bubbleable_metadata`, but the flag decides: they return a
//! plain string unless it is `TRUE`.
//!
//! Each correction is keyed on the exact FQN, and leaves alone a
//! declaration that no longer looks the way it is corrected for.

use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::ClassInfo;

use super::classes::{method_index, patch_method_conditional_return, update_method_at};

const EXTENDABLE_INTERFACE: &str = "Drupal\\Core\\Database\\Query\\ExtendableInterface";

/// The template `extend()` binds from the extender class it is given.
const EXTENDER: &str = "TExtender";

const GENERATED_URL: &str = "Drupal\\Core\\GeneratedUrl";
const URL_FLAG: &str = "$collect_bubbleable_metadata";

/// Correct the return types of Drupal core's classes; `false` for any
/// other class.
pub(super) fn patch(class: &mut ClassInfo) -> bool {
    match class.fqn().as_str() {
        EXTENDABLE_INTERFACE => patch_extend(class),
        "Drupal\\Core\\Url" => patch_url_method(class, "toString"),
        "Drupal\\Core\\Routing\\UrlGeneratorInterface" => {
            patch_url_method(class, "generateFromRoute")
        }
        "Drupal\\Core\\Utility\\UnroutedUrlAssemblerInterface" => {
            patch_url_method(class, "assemble")
        }
        _ => return false,
    }
    true
}

/// `extend(PagerSelectExtender::class)` returns a `PagerSelectExtender`.
///
/// Left alone when the method no longer takes `$extender_name` as a plain
/// `class-string`, already declares templates, or declares a return type
/// other than the interface.
fn patch_extend(class: &mut ClassInfo) {
    let Some(idx) = method_index(class, "extend") else {
        return;
    };
    let method = &class.methods[idx];
    let Some(first) = method.parameters.first() else {
        return;
    };
    let param = first.name;
    if param != "$extender_name"
        || first
            .type_hint
            .as_ref()
            .is_some_and(|ty| !matches!(ty.kind(), TypeKind::ClassString(None)))
        || !method.template_params().is_empty()
        || method
            .return_type
            .as_ref()
            .is_some_and(|ty| !ty.is_named(EXTENDABLE_INTERFACE))
    {
        return;
    }
    let template = atom(EXTENDER);
    update_method_at(class, idx, |method| {
        if let Some(first) = method.parameters.make_mut().first_mut() {
            first.type_hint = Some(PhpType::class_string(Some(PhpType::named(template))));
        }
        method.return_type = Some(PhpType::named(template));
        let extras = method.extras_mut();
        extras.template_params.push(template);
        extras
            .template_param_bounds
            .insert(template, PhpType::named(atom(EXTENDABLE_INTERFACE)));
        extras.template_bindings.push((template, param));
    });
}

/// `$method` returns a `GeneratedUrl` when the flag is `TRUE`, and a plain
/// string otherwise.
///
/// Left alone when the method no longer takes the flag, already declares a
/// conditional return, or declares a return type other than the union.
fn patch_url_method(class: &mut ClassInfo, method: &str) {
    let Some(idx) = method_index(class, method) else {
        return;
    };
    let declared = &class.methods[idx];
    if declared.conditional_return().is_some()
        || !declared
            .return_type
            .as_ref()
            .is_none_or(is_string_or_generated_url)
    {
        return;
    }
    patch_method_conditional_return(
        class,
        method,
        URL_FLAG,
        PhpType::conditional(
            URL_FLAG,
            false,
            PhpType::true_(),
            PhpType::named(atom(GENERATED_URL)),
            PhpType::string(),
        ),
    );
}

/// Whether `ty` is the `string|GeneratedUrl` core declares, its class name
/// stored resolved.
fn is_string_or_generated_url(ty: &PhpType) -> bool {
    let TypeKind::Union(members) = ty.kind() else {
        return false;
    };
    members.len() == 2
        && members.iter().any(|m| m.is_string_type())
        && members.iter().any(|m| m.is_named(GENERATED_URL))
}
