//! PHPStan conditional return type resolution.
//!
//! This module contains the free functions that resolve PHPStan conditional
//! return type annotations to concrete type strings.  These annotations
//! allow a function's return type to depend on the type or value of a
//! parameter at the call site.
//!
//! Two resolution paths are supported:
//!
//! - **Text-based** ([`text_args`],
//!   [`resolve_conditional_with_text_args_and_defaults`]): the main path,
//!   taking the raw argument text between the call's parentheses. Callers
//!   that hold a parsed `ArgumentList` extract each argument's source text
//!   from it, so an assignment (`$var = func(…)`) and an inline call
//!   (`func(A::class)->method()`) decide their conditions identically.
//! - **No-args** ([`without_args`], [`resolve_conditional_without_args`]):
//!   used when no arguments were provided (or none were preserved); walks
//!   the conditional tree deciding each condition against the parameter's
//!   declared default.
//!
//! This module holds what both share: the call's template and class
//! context, and deciding whether a resolved type satisfies a condition.
//!
//! Both decide an omitted argument by that declared default, since that is
//! the value the parameter takes at runtime.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use mago_syntax::cst::*;

use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ParameterInfo};

mod arg_form;
mod text_args;
mod without_args;

use arg_form::{
    combine_union_results, condition_category, literal_condition_result, type_category,
};

pub use text_args::resolve_conditional_with_text_args_and_defaults;
pub use without_args::{
    ThisContext, resolve_conditional_from_values, resolve_conditional_without_args_and_defaults,
};
pub(crate) use without_args::{receiver_type_for_condition, resolve_conditional_without_args};

/// Groups template-related context for conditional return type resolution.
///
/// This bundles the class-level template defaults and the method/function-level
/// template parameter names into a single value, keeping function signatures
/// under clippy's 7-argument limit.
pub struct TemplateContext<'a> {
    /// Class-level template parameter defaults (e.g. from `@template TAsync = false`).
    pub defaults: Option<&'a HashMap<String, PhpType>>,
    /// Method/function-level `@template` parameter names.
    /// Used to distinguish template parameters (e.g. `T`) from concrete class
    /// names (e.g. `FormFlowTypeInterface`) in `class-string<Bound>` conditions.
    pub params: &'a [crate::atom::Atom],
    /// Method/function-level `@template` parameter bindings, as
    /// `(template name, parameter name)` pairs from `@param B $invalidBehavior`.
    ///
    /// A condition keyed on a template parameter (`B is 0|1`) is really keyed
    /// on the argument that binds it, so this is what routes the condition to
    /// the parameter it decides against.
    pub bindings: &'a [(crate::atom::Atom, crate::atom::Atom)],
    /// Optional resolver mapping an argument's source text to its resolved
    /// [`PhpType`].
    ///
    /// Used to evaluate `is <Type>` conditions (e.g. `$subject is string`)
    /// when the argument is not a syntactic literal but an expression whose
    /// type can still be resolved (a method-call chain like `$obj->toHtml()`,
    /// a property access, a variable, etc.). Without it, such arguments fall
    /// through to the else branch even when their real type would satisfy the
    /// condition.
    pub arg_type_resolver: ArgTypeResolver<'a>,
    /// The receiver's own resolved type, with its class-level `@template`
    /// parameters already substituted (e.g. `Builder<true, false>`).
    ///
    /// Feeds a condition keyed on `$this` (`@return ($this is self<true,
    /// true> ? void : never)` after a `@phpstan-self-out`): the receiver
    /// is not an argument, so it has no place in `params`/`text_args`,
    /// and this is where the condition's subject comes from instead.
    /// `None` leaves such a condition undecided.
    pub this_type: Option<&'a PhpType>,
}

/// Callback that resolves an argument's source text (e.g. `"$obj->toHtml()"`)
/// to its [`PhpType`], or `None` when the type cannot be determined.
pub type ArgTypeResolver<'a> = Option<&'a dyn Fn(&str) -> Option<PhpType>>;

impl<'a> TemplateContext<'a> {
    pub fn with_params(params: &'a [crate::atom::Atom]) -> Self {
        Self {
            defaults: None,
            params,
            bindings: &[],
            arg_type_resolver: None,
            this_type: None,
        }
    }
}

/// Callback that resolves a variable name (e.g. `"$requestType"`) to the
/// class names it holds as class-string values (e.g. from match expression
/// arms like `match (...) { 'a' => A::class, 'b' => B::class }`).
///
/// Returns an empty `Vec` when the variable cannot be resolved or does not
/// hold class-string values.
pub(crate) type VarClassStringResolver<'a> = Option<&'a dyn Fn(&str) -> Vec<String>>;

/// The two distinct classes involved in resolving `self`/`static`/`parent`
/// during conditional return type resolution.
///
/// `calling` is the class enclosing the call expression: what `self::class`
/// means when it appears in a call-site argument. `declaring` is the class
/// that declares the method being called: what `self` means in that
/// method's own source, including a parameter's default value (`=
/// self::SOME_CONST`) which is written there, not at the call site. These
/// differ whenever the call sits outside the declaring class, so a single
/// class name cannot serve both roles.  `declaring` is the class's FQN.
///
/// `written_class` resolves a class a call-site argument names as written
/// (`Foo::class`) by the call site's imports and namespace.  Every other
/// name the conditional reads (its own types, a class-string a variable
/// holds) is already resolved, and the class loader it is handed looks
/// names up as such.  Without it, a written name goes through that loader
/// too.
#[derive(Clone, Copy, Default)]
pub struct ConditionalClassContext<'a> {
    pub calling: Option<&'a str>,
    pub declaring: Option<&'a str>,
    pub written_class: Option<&'a dyn Fn(&str) -> String>,
}

impl ConditionalClassContext<'_> {
    /// The FQN of a class a call-site argument names as written, with
    /// `self`/`static`/`parent` meaning the calling class.
    fn resolve_written(
        &self,
        name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> String {
        if let Some(calling) = resolve_self_keyword(name, self.calling) {
            return crate::names::resolve_name_via_loader(&calling, class_loader);
        }
        match self.written_class {
            Some(resolve) => resolve(name),
            None => crate::names::resolve_name_via_loader(name, class_loader),
        }
    }
}

/// Whether a `($this is C ? …)` conditional takes its then branch for the
/// receiver type `this_ty`, `self` in the condition naming `declaring`.
/// `None` when the receiver type cannot decide it.
fn this_condition_holds(
    this_ty: &PhpType,
    condition: &PhpType,
    negated: bool,
    declaring: Option<&str>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<bool> {
    let condition = match declaring {
        Some(declaring) => Cow::Owned(condition.replace_bare_self(declaring)),
        None => Cow::Borrowed(condition),
    };
    let satisfied = if condition.is_null() {
        this_ty.is_null()
    } else {
        type_condition_result(this_ty, &condition, class_loader)?
    };
    Some(satisfied ^ negated)
}

/// Whether a value of `arg_ty` satisfies `condition`, for callers outside
/// this module that need to walk a conditional's branches themselves.
pub(in crate::type_engine) fn condition_holds_for_type(
    arg_ty: &PhpType,
    condition: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<bool> {
    type_condition_result(arg_ty, condition, class_loader)
}

/// Decide whether an argument of resolved type `arg_ty` satisfies a type
/// `condition`.
///
/// Returns `Some(true)` when the resolved type clearly satisfies the
/// condition, `Some(false)` when it clearly cannot (a mutually-exclusive
/// category, e.g. a `Closure` argument against `array|string`), and `None`
/// when it cannot be proven either way (`mixed`, an unresolved type, or a
/// mixed union). The caller uses `None` to fall back to a union of both
/// branches rather than committing to the wrong one.
fn type_condition_result(
    arg_ty: &PhpType,
    condition: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<bool> {
    if arg_ty.is_mixed() || arg_ty.is_untyped() {
        return None;
    }
    if let (TypeKind::Callable(arg), TypeKind::Callable(cond)) = (arg_ty.kind(), condition.kind()) {
        return callable_condition_result(arg, cond, class_loader);
    }
    // A condition naming a literal value (`$invalidBehavior is 0|1`) compares
    // values, not types, so it is settled by the argument's own value.
    if matches!(condition.kind(), TypeKind::Literal(_)) {
        return literal_condition_result(condition, arg_ty);
    }
    // `is true` / `is false` name one boolean value, so only an argument
    // narrowed to a value of its own settles them: a plain `bool` really may
    // be either. A value outside `bool` altogether is neither.
    if condition.is_true() || condition.is_false() {
        if arg_ty.is_true() || arg_ty.is_false() {
            return Some(condition.is_true() == arg_ty.is_true());
        }
        // A union or nullable argument is judged member by member below.
        if !matches!(arg_ty.kind(), TypeKind::Union(_) | TypeKind::Nullable(_)) {
            return match type_category(arg_ty) {
                Some("bool") | None => None,
                Some(_) => Some(false),
            };
        }
    }
    // Condition union (`array|string`): satisfied when any member matches,
    // refuted only when every member is refuted.
    if let TypeKind::Union(members) = condition.kind() {
        return combine_union_results(
            members
                .iter()
                .map(|member| type_condition_result(arg_ty, member, class_loader)),
        );
    }
    // A nullable argument is the union of its inner type and `null`, and has
    // to be judged as one: read whole, `?string` would answer an `is null`
    // condition as a plain string and commit to a branch the call may not
    // take.
    if let TypeKind::Nullable(inner) = arg_ty.kind() {
        let inner_result = type_condition_result(inner, condition, class_loader);
        let null_result = type_condition_result(&PhpType::null(), condition, class_loader);
        return match (inner_result, null_result) {
            (Some(inner), Some(null)) if inner == null => Some(inner),
            _ => None,
        };
    }
    // Argument union: satisfied only when every member matches, refuted only
    // when every member is refuted, otherwise indeterminate.
    if let TypeKind::Union(members) = arg_ty.kind() {
        let results: Vec<Option<bool>> = members
            .iter()
            .map(|m| type_condition_result(m, condition, class_loader))
            .collect();
        return if results.iter().all(|r| *r == Some(true)) {
            Some(true)
        } else if results.iter().all(|r| *r == Some(false)) {
            Some(false)
        } else {
            None
        };
    }
    match (type_category(arg_ty), condition_category(condition)) {
        // `is non-empty-array` asks about the entries, which every array
        // type shares the category of: only a type that promises an entry
        // holds it, and only `array{}` refutes it.
        (Some("array"), Some("array")) if condition.is_provably_non_empty() => {
            if arg_ty.is_provably_non_empty() {
                Some(true)
            } else if arg_ty.is_empty_array_shape() {
                Some(false)
            } else {
                None
            }
        }
        (Some(arg_cat), Some(cond_cat)) => Some(arg_cat == cond_cat),
        // A condition that names no scalar category names a class, which the
        // class hierarchy decides (`$id is Arrayable`).
        (Some(arg_cat), None) => class_condition_result(arg_ty, arg_cat, condition, class_loader),
        _ => None,
    }
}

/// `conditional` with the call's own template bindings substituted into
/// every condition that is a callable signature (`$callback is Closure():
/// T`), so the argument's signature is compared against what `T` was bound
/// to.  Subjects, branches, and every other condition are left as written:
/// a `class-string<T>` condition relies on `T` still naming the template.
pub(crate) fn bind_callable_condition_templates(
    conditional: &PhpType,
    subs: &HashMap<String, PhpType>,
) -> Option<PhpType> {
    let TypeKind::Conditional(c) = conditional.kind() else {
        return None;
    };
    let condition = matches!(c.condition.kind(), TypeKind::Callable(_))
        .then(|| c.condition.substitute(subs))
        .filter(|bound| *bound != c.condition);
    let then_type = bind_callable_condition_templates(&c.then_type, subs);
    let else_type = bind_callable_condition_templates(&c.else_type, subs);
    if condition.is_none() && then_type.is_none() && else_type.is_none() {
        return None;
    }
    Some(PhpType::conditional_type(
        crate::php_type::ConditionalType {
            param: c.param,
            negated: c.negated,
            condition: condition.unwrap_or_else(|| c.condition.clone()),
            then_type: then_type.unwrap_or_else(|| c.then_type.clone()),
            else_type: else_type.unwrap_or_else(|| c.else_type.clone()),
            else_when_undecided: c.else_when_undecided,
        },
    ))
}

/// Decide an `is Closure(…): R` condition against a callable argument's own
/// signature (`fn (): Item => …` against `Closure(): Item`).
///
/// Holds when the argument accepts every parameter the condition passes
/// and returns a subtype of what it promises. An argument that needs more
/// parameters than the condition passes refutes it; anything short of a
/// proven fit is left undecided, since the signatures may still overlap.
fn callable_condition_result(
    arg: &crate::php_type::CallableType,
    cond: &crate::php_type::CallableType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<bool> {
    let is_closure = |kind: &str| kind.eq_ignore_ascii_case("Closure");
    if is_closure(&cond.kind) && !is_closure(&arg.kind) {
        return None;
    }
    let required = arg
        .params
        .iter()
        .filter(|p| !p.optional && !p.variadic)
        .count();
    if required > cond.params.len() {
        return Some(false);
    }
    let params_fit = cond.params.iter().zip(arg.params.iter()).all(|(c, a)| {
        a.type_hint.is_mixed()
            || crate::class_lookup::is_subtype_of_typed(&c.type_hint, &a.type_hint, class_loader)
    });
    let returns_fit = match (&cond.return_type, &arg.return_type) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(c), Some(a)) => crate::class_lookup::is_subtype_of_typed(a, c, class_loader),
    };
    (params_fit && returns_fit).then_some(true)
}

/// Decide an `is <ClassName>` condition (`$id is Arrayable`, `$items is
/// Collection<int, T>`) against the argument's resolved type.
///
/// Returns `Some(true)` when the argument's type is a subtype of the class the
/// condition names, `Some(false)` when no value of that type can be an
/// instance of it, and `None` when the argument's *declared* type is broader
/// than what it may hold at runtime, so either branch is still reachable.
fn class_condition_result(
    arg_ty: &PhpType,
    arg_category: &str,
    condition: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<bool> {
    let cond_name = condition.base_name()?;
    // A scalar, an array or null is never an instance of a class.
    if arg_category != "object" {
        return Some(false);
    }
    if crate::class_lookup::is_subtype_of_typed(arg_ty, condition, class_loader) {
        return Some(true);
    }
    // Beyond that the hierarchy has to be readable for the failed check to
    // mean anything: an unindexed class refutes nothing.
    let arg_name = arg_ty.base_name()?;
    let (Some(arg_class), Some(cond_class)) = (class_loader(arg_name), class_loader(cond_name))
    else {
        return None;
    };
    // The same class with type arguments that provably don't line up in
    // either direction (`Builder<true, false>` against `Builder<true,
    // true>`): template arguments are invariant, so no value of the
    // argument's type is an instance of the condition's. Without this the
    // class check below would see a class that is trivially "a subtype of
    // itself" and leave the condition open.
    if let (TypeKind::Generic(arg_generic), TypeKind::Generic(cond_generic)) =
        (arg_ty.kind(), condition.kind())
        && arg_class.fqn() == cond_class.fqn()
        && arg_generic.args.len() == cond_generic.args.len()
        && arg_generic
            .args
            .iter()
            .zip(cond_generic.args.iter())
            .any(|(a, c)| {
                !crate::class_lookup::is_subtype_of_typed(a, c, class_loader)
                    && !crate::class_lookup::is_subtype_of_typed(c, a, class_loader)
            })
    {
        return Some(false);
    }
    // The condition names a subtype of the argument's declared type, so the
    // value handed over may well be one of them.
    if crate::class_lookup::is_subtype_of_names(cond_name, arg_name, class_loader) {
        return None;
    }
    // Two unrelated classes are mutually exclusive, since PHP gives a class a
    // single parent chain. An unrelated *interface* is not: a subclass of the
    // argument's class may implement it, which only a final class rules out.
    if cond_class.kind == crate::types::ClassLikeKind::Interface && !arg_class.is_final {
        return None;
    }
    Some(false)
}

/// Union two optional branch types produced by an undecidable conditional,
/// dropping duplicates and any uninformative branch.
fn union_branch_types(a: Option<PhpType>, b: Option<PhpType>) -> Option<PhpType> {
    let mut members: Vec<PhpType> = Vec::new();
    let mut push = |ty: PhpType| match ty.kind() {
        TypeKind::Union(inner) => {
            for m in inner {
                if !members.contains(m) {
                    members.push(m.clone());
                }
            }
        }
        _ => {
            if !members.contains(&ty) {
                members.push(ty);
            }
        }
    };
    if let Some(t) = a {
        push(t);
    }
    if let Some(t) = b {
        push(t);
    }
    match members.len() {
        0 => None,
        1 => members.into_iter().next(),
        _ => Some(PhpType::union(members).simplified()),
    }
}

/// Recursively evaluate any nested [`TypeKind::Conditional`] nodes inside a
/// type against textual call-site arguments, replacing each with the type of
/// its winning branch.
///
/// A method's return type can embed a conditional inside a generic wrapper,
/// e.g. Laravel's `Collection::groupBy` returns
/// `static<($groupBy is array|string ? array-key : …), static<…>>`. The
/// top-level conditional resolvers only handle a return type that is *itself*
/// a conditional; this walker reaches conditionals nested inside `Generic`,
/// `Union`, `Array`, and shape positions so they never survive raw into a
/// resolved variable type (where they would later be compared against a
/// call argument and printed unevaluated).
///
/// When a conditional cannot be resolved to an informative type it collapses
/// to `mixed` rather than remaining a raw conditional.
pub fn evaluate_nested_conditionals_text(
    ty: &PhpType,
    params: &[ParameterInfo],
    text_args: &str,
    var_resolver: VarClassStringResolver<'_>,
    class_ctx: ConditionalClassContext<'_>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    tpl: &TemplateContext<'_>,
) -> PhpType {
    let recurse = |inner: &PhpType| {
        evaluate_nested_conditionals_text(
            inner,
            params,
            text_args,
            var_resolver,
            class_ctx,
            class_loader,
            tpl,
        )
    };
    match ty.raw_kind() {
        TypeKind::Conditional(_) => {
            let resolved = resolve_conditional_with_text_args_and_defaults(
                ty,
                params,
                text_args,
                var_resolver,
                class_ctx,
                class_loader,
                tpl,
            )
            .unwrap_or_else(|| PhpType::named(atom("mixed")));
            // Template-default resolution hands back the winning branch
            // without recursing into it, so the branch may itself be a
            // conditional (e.g. `TGroupKey is \UnitEnum ? … : (TGroupKey is
            // \Stringable ? … : …)`). Keep collapsing until none remains.
            // The `resolved != *ty` guard prevents a non-terminating loop on
            // a conditional that cannot be reduced further; such a residual
            // conditional collapses to `mixed` rather than surviving raw.
            if !resolved.contains_conditional() {
                resolved
            } else if resolved != *ty {
                recurse(&resolved)
            } else {
                PhpType::named(atom("mixed"))
            }
        }
        _ => ty.map_children(&recurse),
    }
}

/// If `name` is `"self"`, `"static"`, or `"parent"`, substitute the
/// calling-site class name so that the resolved type is concrete rather
/// than relative to the method-owner class. Returns `None` for any other
/// name.
fn resolve_self_keyword(name: &str, calling_class_name: Option<&str>) -> Option<String> {
    match name {
        "self" | "static" | "parent" => calling_class_name.map(|n| n.to_string()),
        _ => None,
    }
}

/// Extract the class name from an `X::class` expression, as written
/// (`self`, `static` and `parent` included).
pub(crate) fn extract_class_string_from_expr(expr: &Expression<'_>) -> Option<String> {
    match crate::parser::class_fetch_target(expr)? {
        Expression::Identifier(class_ident) => {
            Some(crate::atom::bytes_to_str(class_ident.value()).to_string())
        }
        Expression::Self_(_) => Some("self".to_string()),
        Expression::Static(_) => Some("static".to_string()),
        Expression::Parent(_) => Some("parent".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
