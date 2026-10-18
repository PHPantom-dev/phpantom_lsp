//! Deciding a conditional return type without the call's arguments: every
//! parameter takes its declared default, and a template parameter its
//! default or known value.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ParameterInfo};

use super::arg_form::condition_result_from_text;
use super::{this_condition_holds, type_condition_result, union_branch_types};

/// Resolve a conditional return type **without** call-site arguments
/// (text-based path).  Walks the tree taking the "no argument / null
/// default" branch at each level.
pub(crate) fn resolve_conditional_without_args(
    conditional: &PhpType,
    params: &[ParameterInfo],
) -> Option<PhpType> {
    resolve_conditional_without_args_and_defaults(conditional, params, None, None)
}

/// The receiver's own type for deciding a `$this`-keyed condition:
/// `declaring_fqn` with its class-level `@template` parameters filled in
/// from `template_subs`, in declaration order (`Builder<true, false>`),
/// which is the order a `self<…>` condition writes its arguments in.
pub(crate) fn receiver_type_for_condition(
    declaring_fqn: &str,
    class_template_params: &[crate::atom::Atom],
    template_subs: &HashMap<String, PhpType>,
) -> PhpType {
    if class_template_params.is_empty() {
        return PhpType::named(atom(declaring_fqn));
    }
    let args = class_template_params
        .iter()
        .map(|p| {
            template_subs
                .get(p.as_str())
                .cloned()
                .unwrap_or_else(PhpType::mixed)
        })
        .collect();
    PhpType::generic(declaring_fqn, args)
}

/// The receiver a `$this`-keyed condition is decided against on the
/// no-arguments path. See [`TemplateContext::this_type`].
#[derive(Clone, Copy)]
pub struct ThisContext<'a> {
    /// The receiver's resolved type, class-level `@template` values
    /// already substituted.
    pub this_type: &'a PhpType,
    /// The class `self` names inside the condition.
    pub declaring_class_name: &'a str,
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
}

/// Like [`resolve_conditional_without_args`], but also accepts optional
/// template parameter defaults from the owning class, and the receiver a
/// condition keyed on `$this` is decided against.
///
/// When the conditional's subject (e.g. `TAsync`) is not a method parameter
/// but a class-level template parameter with a default value, the default
/// is used to evaluate the condition.  For example, given
/// `@template TAsync of bool = false` and a conditional
/// `(TAsync is false ? Response : PromiseInterface)`, this function
/// recognises `TAsync`'s default `false`, matches it against the `false`
/// condition, and returns `Response`.
pub fn resolve_conditional_without_args_and_defaults(
    conditional: &PhpType,
    params: &[ParameterInfo],
    template_defaults: Option<&HashMap<String, PhpType>>,
    this_ctx: Option<ThisContext<'_>>,
) -> Option<PhpType> {
    let recurse = |branch: &PhpType| {
        resolve_conditional_without_args_and_defaults(branch, params, template_defaults, this_ctx)
    };
    match conditional.kind() {
        TypeKind::Conditional(cond) => {
            let (param, negated, condition, then_type, else_type) = (
                &cond.param,
                &cond.negated,
                &cond.condition,
                &cond.then_type,
                &cond.else_type,
            );
            let target = param.as_str();

            // `$this` names the receiver, not a parameter; see the matching
            // branch in [`TextArgs::resolve`].
            if target.eq_ignore_ascii_case("$this") {
                let decided = this_ctx.and_then(|this_ctx| {
                    this_condition_holds(
                        this_ctx.this_type,
                        condition,
                        *negated,
                        Some(this_ctx.declaring_class_name),
                        this_ctx.class_loader,
                    )
                });
                return match decided {
                    Some(true) => recurse(then_type),
                    Some(false) => recurse(else_type),
                    None if cond.else_when_undecided => recurse(else_type),
                    None => union_branch_types(recurse(then_type), recurse(else_type)),
                };
            }

            // Check if the conditional subject is a template parameter
            // with a default value (not a method $parameter).
            if !target.starts_with('$')
                && let Some(resolved) = try_resolve_with_template_default(
                    target,
                    *negated,
                    condition,
                    then_type,
                    else_type,
                    template_defaults,
                )
            {
                return Some(resolved);
            }

            // Every parameter takes its declared default, so a default the
            // condition can be decided against settles the branch.
            let param_info = params.iter().find(|p| p.name == target);
            if param_info.is_none() && target.starts_with('$') {
                return if cond.else_when_undecided {
                    recurse(else_type)
                } else {
                    union_branch_types(recurse(then_type), recurse(else_type))
                };
            }
            if let Some(default_text) = param_info.and_then(|p| p.default_value.as_deref())
                && let Some(matched) = condition_result_from_text(condition, default_text)
            {
                let branch = if matched ^ *negated {
                    then_type
                } else {
                    else_type
                };
                return recurse(branch);
            }

            // Otherwise fall back to whether the parameter is optional at all:
            // an omitted optional argument is most often defaulted to null.
            let has_null_default = param_info.is_some_and(|p| !p.is_required);

            if condition.is_null() && has_null_default {
                recurse(then_type)
            } else {
                // Try else branch
                recurse(else_type)
            }
        }
        _ => {
            // See the matching early return in `TextArgs::resolve`: `never`
            // must propagate as informative, only `void` falls back.
            if conditional.is_void() {
                return None;
            }
            Some(conditional.clone())
        }
    }
}

/// Try to resolve a conditional type using a template parameter's default value.
///
/// When a conditional references a template parameter (e.g. `TAsync`) rather
/// than a method parameter (e.g. `$param`), and the template parameter has a
/// default value, this function evaluates the condition against the default.
///
/// Handles conditions like:
///   - `TAsync is false` with default `false` → condition matches → then branch
///   - `TAsync is true`  with default `false` → condition doesn't match → else branch
///   - `TAsync is null`  with default `null`  → condition matches → then branch
///
/// Returns `None` when the template has no default or the condition cannot
/// be evaluated, allowing the caller to fall through to normal resolution.
pub(super) fn try_resolve_with_template_default(
    template_name: &str,
    negated: bool,
    condition: &PhpType,
    then_type: &PhpType,
    else_type: &PhpType,
    template_defaults: Option<&HashMap<String, PhpType>>,
) -> Option<PhpType> {
    let defaults = template_defaults?;
    let default_value = defaults.get(template_name)?;

    // A condition naming one value (`is false`, `is null`, `is 'array'`) is
    // only settled by a value narrowed to one; `bool`, `string`, a union, or
    // `mixed` may be it or not, so both branches stay possible.
    let names_one_value = condition.is_true()
        || condition.is_false()
        || condition.is_null()
        || matches!(condition.kind(), TypeKind::Literal(_));
    let holds_one_value = default_value.is_true()
        || default_value.is_false()
        || default_value.is_null()
        || default_value.as_literal().is_some();
    if names_one_value && !holds_one_value {
        let branch = match type_condition_result(default_value, condition, &|_| None) {
            Some(satisfied) if satisfied ^ negated => then_type,
            Some(_) => else_type,
            None => {
                return union_branch_types(Some(then_type.clone()), Some(else_type.clone()));
            }
        };
        return (!branch.is_void()).then(|| branch.clone());
    }

    // Determine whether the default value matches the condition.
    let condition_matches = if condition.is_false() {
        default_value.is_false()
    } else if condition.is_true() {
        default_value.is_true()
    } else if condition.is_null() {
        default_value.is_null()
    } else if condition.is_bool() {
        default_value.is_true() || default_value.is_false()
    } else if condition.is_string_type() {
        default_value.is_string_literal()
    } else if condition.is_int() {
        default_value.is_int_literal()
    } else if let TypeKind::Literal(lit) = condition.kind() {
        let expected = lit
            .string_content()
            .map(Cow::into_owned)
            .unwrap_or_else(|| lit.as_raw());
        match default_value.kind() {
            TypeKind::Literal(dv) => {
                dv.string_content()
                    .map(Cow::into_owned)
                    .unwrap_or_else(|| dv.as_raw())
                    == expected
            }
            TypeKind::Named(dv) => dv == &expected,
            _ => false,
        }
    } else if let TypeKind::Named(s) = condition.kind() {
        match default_value.kind() {
            TypeKind::Named(dv) => dv == s,
            _ => false,
        }
    } else {
        return None;
    };

    let effective_match = if negated {
        !condition_matches
    } else {
        condition_matches
    };

    let branch = if effective_match {
        then_type
    } else {
        else_type
    };
    if branch.is_void() {
        return None;
    }
    Some(branch.clone())
}

/// Collapse a conditional return type using known template-parameter values.
///
/// When the conditional's subject is a template parameter (not a method
/// `$parameter`) whose value is known from `values` — for example
/// `@template TAsync of bool = false` supplying `TAsync => false`, or an
/// explicit `@mixin Foo<false>` generic argument — the condition can be
/// evaluated and the conditional replaced by the winning branch.
///
/// Returns `None` when the conditional's subject is a runtime parameter,
/// is absent from `values`, or the winning branch is uninformative.
pub fn resolve_conditional_from_values(
    conditional: &PhpType,
    values: &HashMap<String, PhpType>,
) -> Option<PhpType> {
    if let TypeKind::Conditional(cond) = conditional.kind()
        && !cond.param.starts_with('$')
    {
        return try_resolve_with_template_default(
            &cond.param,
            cond.negated,
            &cond.condition,
            &cond.then_type,
            &cond.else_type,
            Some(values),
        );
    }
    None
}
