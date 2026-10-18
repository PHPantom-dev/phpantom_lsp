//! Deciding a conditional return type against the source text of the
//! call's arguments: binding each argument to its parameter, and judging
//! the condition by the argument's literal form, the class it names, or its
//! resolved type.

use std::borrow::Cow;
use std::sync::Arc;

use crate::atom::atom;
use crate::php_type::{ConditionalType, PhpType, TypeKind};
use crate::text_scan::split_top_level_commas;
use crate::types::{ClassInfo, ParameterInfo};

use super::arg_form::{condition_result_from_text, literal_condition_result};
use super::without_args::try_resolve_with_template_default;
use super::{
    ConditionalClassContext, TemplateContext, VarClassStringResolver, resolve_self_keyword,
    this_condition_holds, type_condition_result, union_branch_types,
};

/// Resolve a conditional return type using **textual** arguments extracted
/// from the source code (e.g. `"SessionManager::class"`).
///
/// `tpl` carries the template parameter defaults from the owning class.
///
/// When the conditional's subject (e.g. `TAsync`) is not a method parameter
/// but a class-level template parameter with a default value, the default
/// is used to evaluate the condition.
pub fn resolve_conditional_with_text_args_and_defaults(
    conditional: &PhpType,
    params: &[ParameterInfo],
    text_args: &str,
    var_resolver: VarClassStringResolver<'_>,
    class_ctx: ConditionalClassContext<'_>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    tpl: &TemplateContext<'_>,
) -> Option<PhpType> {
    TextArgs {
        params,
        text_args,
        var_resolver,
        class_ctx,
        class_loader,
        tpl,
    }
    .resolve(conditional)
}

/// A call whose conditional return type is decided against the source text
/// of its arguments, with everything needed to read that text.
#[derive(Clone, Copy)]
struct TextArgs<'a> {
    params: &'a [ParameterInfo],
    /// The raw text between the call's parentheses.
    text_args: &'a str,
    var_resolver: VarClassStringResolver<'a>,
    class_ctx: ConditionalClassContext<'a>,
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    tpl: &'a TemplateContext<'a>,
}

impl TextArgs<'_> {
    /// Resolve `conditional` against this call.
    fn resolve(&self, conditional: &PhpType) -> Option<PhpType> {
        let Self {
            params,
            text_args,
            class_ctx,
            class_loader,
            tpl,
            ..
        } = *self;
        let TypeKind::Conditional(cond) = conditional.kind() else {
            // `void` carries no information beyond the method's declared
            // return type, so the caller falls back to that. `never` is
            // different: it means the call provably does not return, which
            // is informative and must propagate rather than being dropped.
            if conditional.is_void() {
                return None;
            }
            return Some(conditional.clone());
        };
        let target = cond.param.as_str();

        // `$this` names the receiver, not a parameter: decide the
        // condition against the resolved receiver type the caller
        // supplied (its class-level `@template` values already
        // substituted, e.g. by a preceding `@phpstan-self-out`) rather
        // than looking it up among `params`/`text_args`.
        if target.eq_ignore_ascii_case("$this") {
            let takes_then = tpl.this_type.and_then(|this_ty| {
                this_condition_holds(
                    this_ty,
                    &cond.condition,
                    cond.negated,
                    class_ctx.declaring,
                    class_loader,
                )
            });
            return self.branch(cond, takes_then);
        }

        // Check if the conditional subject is a template parameter
        // with a default value (not a method $parameter).
        if !target.starts_with('$')
            && let Some(resolved) = try_resolve_with_template_default(
                target,
                cond.negated,
                &cond.condition,
                &cond.then_type,
                &cond.else_type,
                tpl.defaults,
            )
        {
            return Some(resolved);
        }

        // `T[0] is string` asks about an offset of what the argument
        // binding `T` holds, which is read off that argument's type.
        if !target.starts_with('$')
            && target.contains('[')
            && let Some(decided) = decide_template_offset_condition(
                target,
                &cond.condition,
                params,
                text_args,
                tpl,
                class_loader,
            )
        {
            return self.branch(cond, Some(decided ^ cond.negated));
        }

        // A condition keyed on a `@template` parameter (`B is 0|1`) is
        // decided by the argument that binds it (`@param B $behavior`).
        let target = if target.starts_with('$') {
            target
        } else {
            tpl.bindings
                .iter()
                .find(|(template, _)| template.as_str() == target)
                .map_or(target, |(_, param)| param.as_str())
        };

        // A `$name` the signature does not declare has no argument to
        // decide it, so nothing is known and both branches remain.
        let Some(param_idx) = params
            .iter()
            .position(|p| p.name == target)
            .or_else(|| (!target.starts_with('$')).then_some(0))
        else {
            return self.undecided(cond);
        };
        let is_variadic = params
            .get(param_idx)
            .map(|p| p.is_variadic)
            .unwrap_or(false);

        // Split the textual arguments by comma (at depth 0), then bind
        // them to parameters by PHP's rules so a named argument resolves
        // to the parameter it targets rather than its ordinal slot.
        let args = split_top_level_commas(text_args);
        let bound_text = crate::call_args::bind_text_args_to_params(params, &args);
        let arg_text_owned = bound_text.get(param_idx).cloned().flatten();
        // An omitted argument takes the parameter's declared default at
        // runtime, so the condition is decided against that default
        // rather than being left undecidable. The default's own
        // `self`/`static`/`parent` (e.g. `= self::SOME_CONST`) is
        // written in the method's declaring class, not the call site,
        // so it is resolved against `class_ctx.declaring` here, before
        // the text reaches any of the branches below.
        let default_text_resolved: Option<Cow<'_, str>> = arg_text_owned
            .is_none()
            .then(|| param_default_text(params.get(param_idx)))
            .flatten()
            .map(|text| resolve_default_self_keyword(text, class_ctx.declaring));
        let arg_text = arg_text_owned
            .as_deref()
            .or(default_text_resolved.as_deref());

        let constant_values = condition_constants_as_values(&cond.condition, class_loader, tpl);
        let condition = constant_values.as_ref().unwrap_or(&cond.condition);

        if matches!(condition.kind(), TypeKind::ClassString(_)) {
            let variadic_args = is_variadic.then(|| args.get(param_idx..).unwrap_or_default());
            self.class_string_condition(cond, condition, variadic_args, arg_text)
        } else if condition.is_null() {
            let variadic_passed = is_variadic.then_some(arg_text_owned.is_some());
            self.null_condition(cond, condition, arg_text, variadic_passed)
        } else if matches!(condition.kind(), TypeKind::Literal(_)) {
            // Value condition (`$format is 0`, `$flags is 15`). A literal
            // argument settles it outright; anything else is settled by
            // the argument's resolved type when that turns out to be a
            // literal of its own, which is how a named constant
            // (`pathinfo($p, PATHINFO_FILENAME)`) or a local holding one
            // decides the branch it really takes.
            self.judged_condition(cond, condition, arg_text, |arg_ty| {
                literal_condition_result(condition, arg_ty)
            })
        } else if let Some(parts) = class_const_condition_parts(condition) {
            self.class_constant_condition(cond, parts, arg_text)
        } else {
            // IsType equivalent (`$x is string`, `$x is array|string`,
            // …), decided by the argument's resolved type when its text
            // is not a literal (e.g. a method-call chain that returns
            // `string`).
            self.judged_condition(cond, condition, arg_text, |arg_ty| {
                type_condition_result(arg_ty, condition, class_loader)
            })
        }
    }

    /// The conditional's then branch when `takes_then` is `Some(true)`, its
    /// else branch when `Some(false)`, and the undecided answer when the
    /// condition could not be decided.
    fn branch(&self, cond: &ConditionalType, takes_then: Option<bool>) -> Option<PhpType> {
        match takes_then {
            Some(true) => self.resolve(&cond.then_type),
            Some(false) => self.resolve(&cond.else_type),
            None => self.undecided(cond),
        }
    }

    /// What the conditional answers when a resolver was consulted and could
    /// not pin the argument down: the call really may take either branch,
    /// so the answer is their union unless the conditional asked for proof
    /// and named its else branch as the undecided reading (see
    /// [`ConditionalType::else_when_undecided`]).
    fn undecided(&self, cond: &ConditionalType) -> Option<PhpType> {
        if cond.else_when_undecided {
            self.resolve(&cond.else_type)
        } else {
            union_branch_types(self.resolve(&cond.then_type), self.resolve(&cond.else_type))
        }
    }

    /// Decide an `is null` condition.
    ///
    /// The null (`then`) branch is taken when the parameter really holds
    /// null: no argument at all (so it falls back to a null default), an
    /// explicit `null`, or an argument whose resolved type is null. Any
    /// other type takes the else branch, and a type that may or may not be
    /// null (`?string`) leaves both branches open rather than committing to
    /// one the call may not take.
    ///
    /// `variadic_passed` is set for a variadic parameter, which has no
    /// single value to judge: it holds every trailing argument, so the only
    /// question the condition can be asking is whether any were passed. Its
    /// first argument's own type says nothing about that, and for the
    /// by-reference out-parameters this shape is used for (`sscanf($s,
    /// $fmt, $a, $b)`) that type is usually unresolvable anyway.
    fn null_condition(
        &self,
        cond: &ConditionalType,
        condition: &PhpType,
        arg_text: Option<&str>,
        variadic_passed: Option<bool>,
    ) -> Option<PhpType> {
        let resolver = self.tpl.arg_type_resolver;
        let is_null = match (variadic_passed, arg_text) {
            (Some(passed), _) => Some(!passed),
            (None, None) => Some(true),
            (None, Some(text)) if text.trim().is_empty() => Some(true),
            (None, Some(text)) => condition_result_from_text(condition, text).or_else(|| {
                resolver
                    .and_then(|resolve| resolve(text))
                    .and_then(|arg_ty| type_condition_result(&arg_ty, condition, self.class_loader))
                    .or_else(|| {
                        // With no resolver to consult, an argument that is
                        // not the literal `null` reads as non-null, which is
                        // what it usually is.
                        resolver.is_none().then_some(false)
                    })
            }),
        };
        self.branch(cond, is_null.map(|is_null| is_null ^ cond.negated))
    }

    /// Decide a condition by the argument's syntactic form (a literal),
    /// else by `judge` on its resolved type when a resolver is available.
    fn judged_condition(
        &self,
        cond: &ConditionalType,
        condition: &PhpType,
        arg_text: Option<&str>,
        judge: impl Fn(&PhpType) -> Option<bool>,
    ) -> Option<PhpType> {
        let resolver = self.tpl.arg_type_resolver;
        let decided = arg_text.and_then(|arg| {
            condition_result_from_text(condition, arg).or_else(|| {
                resolver
                    .and_then(|resolve| resolve(arg))
                    .and_then(|arg_ty| judge(&arg_ty))
            })
        });
        match decided {
            Some(holds) => self.branch(cond, Some(holds ^ cond.negated)),
            // The condition is genuinely undecidable: the argument is an
            // expression whose type we could not pin down. When a resolver
            // was available (i.e. a real resolution context, not a bare
            // completion lookup), the true result is one of the two
            // branches, so return their union rather than committing to the
            // else branch — otherwise `Str::replace(…, $x->y())` would
            // resolve to `string[]` and falsely flag a `string` argument.
            None if arg_text.is_some() && resolver.is_some() => self.undecided(cond),
            None => self.resolve(&cond.else_type),
        }
    }

    /// Decide a class-constant condition (e.g. `$mode is
    /// PDO::FETCH_ASSOC`): the then-branch is taken when the bound argument
    /// is the same class constant.
    fn class_constant_condition(
        &self,
        cond: &ConditionalType,
        (cond_class, cond_const): (&str, &str),
        arg_text: Option<&str>,
    ) -> Option<PhpType> {
        let matched = arg_text
            .and_then(|arg| arg.trim().rsplit_once("::"))
            .is_some_and(|(arg_class, arg_const)| {
                class_const_matches(
                    cond_class,
                    cond_const,
                    arg_class.trim(),
                    arg_const.trim(),
                    self.class_ctx.calling,
                )
            });
        self.branch(cond, Some(matched ^ cond.negated))
    }

    /// Decide a `class-string` condition (`$type is class-string<Bound>`)
    /// against the class or classes the argument names.
    ///
    /// `variadic_args` holds every argument a variadic parameter collects,
    /// and is `None` for any other parameter, which `arg_text` describes.
    fn class_string_condition(
        &self,
        cond: &ConditionalType,
        condition: &PhpType,
        variadic_args: Option<&[&str]>,
        arg_text: Option<&str>,
    ) -> Option<PhpType> {
        let Self {
            var_resolver,
            class_ctx,
            class_loader,
            tpl,
            ..
        } = *self;
        // Extract the bound type from `class-string<Bound>`, if any.
        // When a bound is present AND resolves to a real class (not
        // a template parameter like `T`), the conditional checks
        // whether the argument class is a subtype of the bound
        // (e.g. `$type is class-string<FormFlowTypeInterface>`).
        //
        // When the bound is a template parameter (e.g. `T` from
        // `@template T of object`), any `::class` literal satisfies
        // the condition and the template param is substituted with
        // the concrete class name — this is the existing behavior.
        //
        // We distinguish the two by checking whether the bound name
        // is one of the method/function's own `@template` parameter
        // names (`tpl.params`), not by resolving it as a class:
        // `class_loader` resolution below determines whether a
        // *concrete* bound exists in the project, but a bare
        // template name like `T` would never appear there anyway,
        // so membership in `tpl.params` is the only reliable signal
        // for which case we're in.
        let class_string_bound_name: Option<&str> = match condition.kind() {
            TypeKind::ClassString(Some(inner)) => match inner.kind() {
                TypeKind::Named(name) => Some(name.as_str()),
                _ => None,
            },
            _ => None,
        };

        let bound_is_template = class_string_bound_name
            .is_some_and(|name| tpl.params.iter().any(|tp| tp.as_str() == name));

        // For concrete bounds, try to resolve the bound class.
        // `None` = no bound or template param (permissive)
        // `Some(resolved)` = concrete class (strict subtype check)
        // `Some(sentinel)` = unresolvable concrete name (always
        //   fails the subtype check, forcing the else branch)
        let concrete_class_string_bound: Option<String> = if bound_is_template {
            None
        } else {
            class_string_bound_name.map(|name| {
                if let Some(cls) = crate::class_lookup::load_resolved_name(name, class_loader) {
                    cls.fqn().to_string()
                } else {
                    // Concrete class that can't be resolved
                    // (cross-file name resolution failure).
                    // Use a sentinel that will never match,
                    // forcing the else branch.  This is the
                    // safe default: we can't verify the
                    // subtype relationship, so we fall back
                    // to the broader return type.
                    "!!unresolvable_bound!!".to_string()
                }
            })
        };

        // Helper: check whether a resolved class name satisfies the
        // concrete class-string bound.  Returns `true` when there is
        // no concrete bound (bare `class-string` or template param
        // bound — any class satisfies it) or the class is a subtype
        // of the bound.
        let satisfies_bound = |resolved_name: &str| -> bool {
            match concrete_class_string_bound {
                None => true,
                Some(ref bound) => {
                    crate::class_lookup::is_subtype_of_names(resolved_name, bound, class_loader)
                }
            }
        };

        // Helper: choose the correct branch based on whether the
        // bound is satisfied and the `negated` flag.
        let choose_branch =
            |bound_satisfied: bool| self.branch(cond, Some(bound_satisfied ^ cond.negated));

        // Helper: when the class-string bound is itself a template
        // parameter (not a concrete class to subtype-check), the
        // condition is definitionally satisfied and `then_type`
        // must have the template substituted with the resolved
        // class(es) rather than being discarded wholesale — this
        // preserves surrounding structure like `T&MockInterface`
        // instead of collapsing it to bare `T`. The substituted
        // branch can itself be a conditional (`(B is 0|1 ? C :
        // C|null)`), so it is resolved in turn.
        let substitute_bound = |resolved_ty: PhpType| -> Option<PhpType> {
            match class_string_bound_name {
                Some(bound_name) if bound_is_template => {
                    let subs =
                        std::collections::HashMap::from([(bound_name.to_string(), resolved_ty)]);
                    self.resolve(&cond.then_type.substitute(&subs))
                }
                _ => Some(resolved_ty),
            }
        };

        // For variadic class-string parameters, collect class
        // names from ALL arguments at and after param_idx and
        // form a union type (e.g. `A|B` from `A::class, B::class`).
        if let Some(variadic_args) = variadic_args {
            let mut class_names: Vec<String> = Vec::new();
            for arg in variadic_args {
                let trimmed = arg.trim();
                if let Some(class_name) = extract_class_name_from_text(trimmed) {
                    let class_name = class_ctx.resolve_written(&class_name, class_loader);
                    if !class_names.contains(&class_name) {
                        class_names.push(class_name);
                    }
                } else if trimmed.starts_with('$')
                    && let Some(resolver) = var_resolver
                {
                    for name in resolver(trimmed) {
                        let name = resolved_class_fqn(&name, class_loader);
                        if !class_names.contains(&name) {
                            class_names.push(name);
                        }
                    }
                }
            }
            if !class_names.is_empty() {
                // When a bound exists, check all collected classes.
                // If any fails the bound check, fall through to the
                // else branch rather than returning a wrong type.
                let all_satisfy = class_names.iter().all(|n| satisfies_bound(n));
                if !all_satisfy {
                    return choose_branch(false);
                }

                return substitute_bound(named_or_union(class_names));
            }
            return self.resolve(&cond.else_type);
        }

        // Check if the argument text matches `X::class`.  An omitted
        // argument already reads back as its `Foo::class` default, so
        // `app()` resolves the same as `app(Foo::class)`.
        let resolved = arg_text
            .and_then(extract_class_name_from_text)
            .map(|class_name| class_ctx.resolve_written(&class_name, class_loader))
            .or_else(|| arg_text.and_then(|text| class_named_by_quoted_string(text, class_loader)));
        if let Some(resolved) = resolved {
            // When a bound exists, verify the class is a subtype
            // before taking the then-branch.  E.g. for
            // `($type is class-string<FormFlowTypeInterface> ? FormFlowInterface : FormInterface)`
            // with `ImageUploadFormType::class`: if `ImageUploadFormType`
            // does NOT implement `FormFlowTypeInterface`, return
            // `FormInterface` (else branch), not the class name.
            if concrete_class_string_bound.is_some() {
                return choose_branch(satisfies_bound(&resolved));
            }

            return substitute_bound(PhpType::named(atom(&resolved)));
        }
        // Check if the argument is a variable holding class-string
        // value(s) (e.g. from a match expression).
        if let Some(arg) = arg_text
            && let trimmed = arg.trim()
            && trimmed.starts_with('$')
            && let Some(resolver) = var_resolver
        {
            let names = resolver(trimmed);
            if !names.is_empty() {
                let names: Vec<String> = names
                    .into_iter()
                    .map(|n| resolved_class_fqn(&n, class_loader))
                    .collect();

                // When a bound exists, check all resolved names.
                if concrete_class_string_bound.is_some() {
                    let all_satisfy = names.iter().all(|n| satisfies_bound(n));
                    return choose_branch(all_satisfy);
                }

                return substitute_bound(named_or_union(names));
            }
        }
        // Any other argument (a property, a method's return, a class
        // constant) is judged by its resolved type: a
        // `class-string<Foo>` names `Foo` just as `Foo::class` does.
        if let Some(arg) = arg_text
            && let Some(resolve) = tpl.arg_type_resolver
            && let Some(arg_ty) = resolve(arg)
            && let Some(inner) = class_string_inner_classes(&arg_ty)
        {
            if concrete_class_string_bound.is_some() {
                let all_satisfy = inner.iter().all(|ty| match ty.kind() {
                    TypeKind::Named(name) => satisfies_bound(name.as_str()),
                    _ => false,
                });
                return choose_branch(all_satisfy);
            }
            let bound = if inner.len() == 1 {
                inner.into_iter().next().unwrap()
            } else {
                PhpType::union(inner)
            };
            return substitute_bound(bound);
        }
        // Argument isn't a ::class literal or resolvable variable → try else branch
        self.resolve(&cond.else_type)
    }
}

/// The source text of a parameter's declared default value.
///
/// An omitted argument takes this value at runtime, so a conditional keyed on
/// that parameter is decided against the default rather than being treated as
/// undecidable.
fn param_default_text(param: Option<&ParameterInfo>) -> Option<&str> {
    param?.default_value.as_deref()
}

/// `condition` with each global constant it names (`$flag is
/// PREG_SPLIT_NO_EMPTY`) replaced by the constant's value, which is what an
/// argument is compared against. `None` when it names none.
///
/// A name that loads as a class is a class, and a name the resolver cannot
/// read as a literal value is left as written.
fn condition_constants_as_values(
    condition: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    tpl: &TemplateContext<'_>,
) -> Option<PhpType> {
    let resolve = tpl.arg_type_resolver?;
    let members = condition.union_members();
    let mut changed = false;
    let values: Vec<PhpType> = members
        .iter()
        .map(|&member| {
            let value = match member.kind() {
                TypeKind::Named(name)
                    if !member.is_keyword()
                        && !tpl.params.contains(name)
                        && class_loader(name).is_none() =>
                {
                    resolve(name).filter(|ty| {
                        ty.as_literal().is_some() || ty.is_true() || ty.is_false() || ty.is_null()
                    })
                }
                _ => None,
            };
            changed |= value.is_some();
            value.unwrap_or_else(|| member.clone())
        })
        .collect();
    if !changed {
        return None;
    }
    Some(match <[PhpType; 1]>::try_from(values) {
        Ok([single]) => single,
        Err(values) => PhpType::union(values),
    })
}

/// Decide a condition whose subject is an offset of a template parameter
/// (`T[0] is string`), or `None` when it cannot be decided.
///
/// Only a template bound directly by one argument (`@param T $bar`) is read:
/// that argument's type is what `T` stands for, and the offset is evaluated
/// on it.  An array literal argument contributes its own shape, which is
/// the one thing an offset can be read from.
fn decide_template_offset_condition(
    subject: &str,
    condition: &PhpType,
    params: &[ParameterInfo],
    text_args: &str,
    tpl: &TemplateContext<'_>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<bool> {
    let parsed = PhpType::try_parse(subject)?;
    let TypeKind::IndexAccess(base, index) = parsed.kind() else {
        return None;
    };
    let TypeKind::Named(tpl_name) = base.kind() else {
        return None;
    };
    let resolver = tpl.arg_type_resolver?;
    let param_name = tpl
        .bindings
        .iter()
        .find(|(template, _)| template == tpl_name)
        .map(|(_, param)| param)?;
    let param_idx = params.iter().position(|p| p.name == *param_name)?;
    let binds_directly = params[param_idx]
        .type_hint
        .as_ref()
        .is_some_and(|hint| hint.unwrap_nullable().is_named(tpl_name));
    if !binds_directly {
        return None;
    }
    let args = split_top_level_commas(text_args);
    let bound = crate::call_args::bind_text_args_to_params(params, &args);
    let arg_text = bound.get(param_idx)?.as_deref()?;
    let arg_ty =
        crate::type_engine::call_resolution::array_literal_shape_type_with(arg_text, resolver)
            .or_else(|| resolver(arg_text))?;
    let subject_ty = crate::php_type::evaluate_index_access(&arg_ty, index);
    if matches!(subject_ty.kind(), TypeKind::IndexAccess(..)) {
        return None;
    }
    type_condition_result(&subject_ty, condition, class_loader)
}

/// A single resolved class name, or a union of them when more than one
/// branch contributed a name.
///
/// `names` must be non-empty; a lone entry is returned bare rather than
/// wrapped in a one-member union.
fn named_or_union(names: Vec<String>) -> PhpType {
    let mut names = names.into_iter();
    let first = PhpType::named(atom(&names.next().expect("names is non-empty")));
    let rest: Vec<PhpType> = names.map(|n| PhpType::named(atom(n.as_ref()))).collect();
    if rest.is_empty() {
        first
    } else {
        let mut members = vec![first];
        members.extend(rest);
        PhpType::union(members)
    }
}

/// The classes a resolved argument type names as `class-string<X>`, or
/// `None` unless every member of it is such a class-string.
///
/// Stricter than the template-binding unwrap: a bare `class-string` names
/// no class to decide a bound against, and an object or plain `string`
/// argument is not a class-string at all.
fn class_string_inner_classes(ty: &PhpType) -> Option<Vec<PhpType>> {
    let mut inner: Vec<PhpType> = Vec::new();
    let mut push = |member: &PhpType| match member.kind() {
        TypeKind::ClassString(Some(class)) => {
            if !inner.contains(class) {
                inner.push(class.clone());
            }
            Some(())
        }
        _ => None,
    };
    match ty.kind() {
        TypeKind::Union(members) => {
            for member in members {
                push(member)?;
            }
        }
        _ => push(ty)?,
    }
    (!inner.is_empty()).then_some(inner)
}

/// Rewrite a leading `self::`/`static::`/`parent::` in a parameter default's
/// source text to `declaring_class_name`.
///
/// A default like `self::EXCEPTION_ON_INVALID_REFERENCE` is written inside
/// the method that declares the parameter, so `self` names that class, not
/// whichever class the omitted argument is read back against downstream.
///
/// The class is written fully qualified: the text is read back as source at
/// the call site, where an unprefixed `Vendor\Container` would be qualified
/// against the caller's namespace.
fn resolve_default_self_keyword<'a>(
    text: &'a str,
    declaring_class_name: Option<&str>,
) -> Cow<'a, str> {
    let Some((class_part, rest)) = text.trim().split_once("::") else {
        return Cow::Borrowed(text);
    };
    match resolve_self_keyword(class_part.trim(), declaring_class_name) {
        Some(resolved) => Cow::Owned(format!("\\{}::{rest}", resolved.trim_start_matches('\\'))),
        None => Cow::Borrowed(text),
    }
}

/// Extract a class name from textual `X::class` syntax.
///
/// Matches strings like `"SessionManager::class"`, `"\\App\\Foo::class"`,
/// returning the class name portion (`"SessionManager"`, `"\\App\\Foo"`).
fn extract_class_name_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let name = trimmed.strip_suffix("::class")?;
    if name.is_empty() {
        return None;
    }
    // Validate that it looks like a class name (identifiers and backslashes).
    if name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
    {
        Some(name.to_string())
    } else {
        None
    }
}

/// If `condition` is a class-constant reference such as `PDO::FETCH_ASSOC`,
/// return its `(class, constant)` parts.
///
/// PHPStan conditional return types use class constants as the compared
/// type, e.g. `@return ($mode is PDO::FETCH_ASSOC ? ... : ...)`. The type
/// parser represents such a member reference as a `Raw`/`Named` variant
/// whose payload contains `::`. Wildcard members (`Foo::*`, used by
/// `int-mask-of<Foo::*>`) and the `::class` pseudo-constant are not real
/// class constants and return `None`.
pub(super) fn class_const_condition_parts(condition: &PhpType) -> Option<(&str, &str)> {
    let raw = match condition.kind() {
        TypeKind::Raw(s) => s,
        TypeKind::Named(s) => s.as_str(),
        _ => return None,
    };
    let (class, member) = raw.rsplit_once("::")?;
    if class.is_empty() || member.is_empty() || member.contains('*') || member == "class" {
        return None;
    }
    Some((class, member))
}

/// Whether an argument referring to the class constant `(arg_class,
/// arg_const)` matches the condition's `(cond_class, cond_const)`.
///
/// Constant names must match exactly; class names are compared on their
/// short (namespace-stripped) form, case-insensitively, which mirrors
/// PHP's case-insensitive class-name resolution. `self`/`static`/`parent`
/// in the argument are resolved against `calling_class_name` first.
fn class_const_matches(
    cond_class: &str,
    cond_const: &str,
    arg_class: &str,
    arg_const: &str,
    calling_class_name: Option<&str>,
) -> bool {
    if arg_const != cond_const {
        return false;
    }
    let arg_class = resolve_self_keyword(arg_class, calling_class_name)
        .unwrap_or_else(|| arg_class.to_string());
    let cond_short = crate::util::short_name(cond_class.trim_start_matches('\\'));
    let arg_short = crate::util::short_name(arg_class.trim_start_matches('\\'));
    arg_short.eq_ignore_ascii_case(cond_short)
}

/// The class a plain string argument names, when it names one at all.
///
/// PHP accepts a string wherever a `class-string` is expected, so
/// `$container->make('App\Services\Clock')` is as much a class-string call as
/// `make(Clock::class)` is; in a Laravel project the loader also answers for
/// the container keys service providers bind (`make('sentry')`).  The class
/// has to actually resolve: an ordinary string argument must keep falling
/// through to the conditional's else branch rather than being read as the name
/// of a class that does not exist.
///
/// A string names a class by its FQN at runtime, whatever namespace the call
/// is written in, so it is looked up as a resolved name.
fn class_named_by_string(
    name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    crate::class_lookup::load_resolved_name(name, class_loader).map(|cls| cls.fqn().to_string())
}

/// [`class_named_by_string`] for the text-argument path, where the argument is
/// still the quoted source text.
fn class_named_by_quoted_string(
    arg_text: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    let name = crate::util::unescape_php_string_literal(arg_text.trim())?;
    class_named_by_string(&name, class_loader)
}

/// The FQN of the class a resolved `name` names, or `name` itself when no
/// class loads.
fn resolved_class_fqn(name: &str, class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>) -> String {
    crate::class_lookup::load_resolved_name(name, class_loader)
        .map(|cls| cls.fqn().to_string())
        .unwrap_or_else(|| name.to_string())
}
