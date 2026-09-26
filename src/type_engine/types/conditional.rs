/// PHPStan conditional return type resolution.
///
/// This module contains the free functions that resolve PHPStan conditional
/// return type annotations to concrete type strings.  These annotations
/// allow a function's return type to depend on the type or value of a
/// parameter at the call site.
///
/// Two resolution paths are supported:
///
/// - **Text-based** ([`resolve_conditional_with_text_args`]): the main path,
///   taking the raw argument text between the call's parentheses. Callers
///   that hold a parsed `ArgumentList` extract each argument's source text
///   from it, so an assignment (`$var = func(…)`) and an inline call
///   (`func(A::class)->method()`) decide their conditions identically.
/// - **No-args** ([`resolve_conditional_without_args`]): used when no
///   arguments were provided (or none were preserved); walks the
///   conditional tree deciding each condition against the parameter's
///   declared default.
///
/// Both decide an omitted argument by that declared default, since that is
/// the value the parameter takes at runtime.
use crate::atom::atom;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use mago_syntax::cst::*;

use crate::php_type::{LiteralValue, PhpType, TypeKind};
use crate::types::{ClassInfo, ParameterInfo};

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
/// class name cannot serve both roles.
#[derive(Clone, Copy, Default)]
pub struct ConditionalClassContext<'a> {
    pub calling: Option<&'a str>,
    pub declaring: Option<&'a str>,
}

/// Resolve a conditional return type using **textual** arguments extracted
/// from the source code (e.g. `"SessionManager::class"`).
///
/// This is used when the call is made inline (not assigned to a variable)
/// and we therefore don't have an AST `ArgumentList` — only the raw text
/// between the parentheses.
pub(crate) fn resolve_conditional_with_text_args(
    conditional: &PhpType,
    params: &[ParameterInfo],
    text_args: &str,
    var_resolver: VarClassStringResolver<'_>,
    calling_class_name: Option<&str>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    tpl: &TemplateContext<'_>,
) -> Option<PhpType> {
    resolve_conditional_with_text_args_and_defaults(
        conditional,
        params,
        text_args,
        var_resolver,
        ConditionalClassContext {
            calling: calling_class_name,
            declaring: None,
        },
        class_loader,
        tpl,
    )
}

/// Like [`resolve_conditional_with_text_args`], but also accepts optional
/// template parameter defaults from the owning class.
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
    match conditional.kind() {
        TypeKind::Conditional(cond) => {
            let (param, negated, condition, then_type, else_type) = (
                &cond.param,
                &cond.negated,
                &cond.condition,
                &cond.then_type,
                &cond.else_type,
            );
            // A conditional that asks for proof answers an undecided
            // condition with its else branch instead of the union of
            // both — see [`ConditionalType::else_when_undecided`].
            let undecided: Option<&PhpType> = cond.else_when_undecided.then_some(else_type);
            let resolve_branch = |branch: &PhpType| {
                resolve_conditional_with_text_args_and_defaults(
                    branch,
                    params,
                    text_args,
                    var_resolver,
                    class_ctx,
                    class_loader,
                    tpl,
                )
            };
            // What the conditional answers when a resolver was consulted
            // and could not pin the argument down: the call really may take
            // either branch, so the answer is their union unless the
            // conditional asked for proof and named its else branch as the
            // undecided reading.
            let undecided_answer = || match undecided {
                Some(branch) => resolve_branch(branch),
                None => union_branch_types(resolve_branch(then_type), resolve_branch(else_type)),
            };
            let target = param.as_str();

            // `$this` names the receiver, not a parameter: decide the
            // condition against the resolved receiver type the caller
            // supplied (its class-level `@template` values already
            // substituted, e.g. by a preceding `@phpstan-self-out`) rather
            // than looking it up among `params`/`text_args`.
            if target.eq_ignore_ascii_case("$this") {
                let Some(this_ty) = tpl.this_type else {
                    return undecided_answer();
                };
                let condition = match class_ctx.declaring {
                    Some(declaring) => Cow::Owned(condition.replace_bare_self(declaring)),
                    None => Cow::Borrowed(condition),
                };
                let decided = if condition.is_null() {
                    Some(this_ty.is_null())
                } else {
                    type_condition_result(this_ty, &condition, class_loader)
                };
                return match decided {
                    Some(satisfied) if satisfied ^ *negated => resolve_branch(then_type),
                    Some(_) => resolve_branch(else_type),
                    None => undecided_answer(),
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
                    tpl.defaults,
                )
            {
                return Some(resolved);
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

            let param_idx = params.iter().position(|p| p.name == target).unwrap_or(0);
            let is_variadic = params
                .get(param_idx)
                .map(|p| p.is_variadic)
                .unwrap_or(false);

            // Split the textual arguments by comma (at depth 0), then bind
            // them to parameters by PHP's rules so a named argument resolves
            // to the parameter it targets rather than its ordinal slot.
            let args = split_text_args(text_args);
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

            if matches!(condition.kind(), TypeKind::ClassString(_)) {
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
                        let resolved = crate::util::resolve_name_via_loader(name, class_loader);
                        if class_loader(&resolved).is_some() {
                            resolved
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
                        Some(ref bound) => crate::class_lookup::is_subtype_of_names(
                            resolved_name,
                            bound,
                            class_loader,
                        ),
                    }
                };

                // Helper: choose the correct branch based on whether the
                // bound is satisfied and the `negated` flag.
                let choose_branch = |bound_satisfied: bool| -> Option<PhpType> {
                    let take_then = bound_satisfied ^ *negated;
                    resolve_conditional_with_text_args_and_defaults(
                        if take_then { then_type } else { else_type },
                        params,
                        text_args,
                        var_resolver,
                        class_ctx,
                        class_loader,
                        tpl,
                    )
                };

                // Helper: when the class-string bound is itself a template
                // parameter (not a concrete class to subtype-check), the
                // condition is definitionally satisfied and `then_type`
                // must have the template substituted with the resolved
                // class(es) rather than being discarded wholesale — this
                // preserves surrounding structure like `T&MockInterface`
                // instead of collapsing it to bare `T`.
                let substitute_bound = |resolved_ty: PhpType| -> PhpType {
                    match class_string_bound_name {
                        Some(bound_name) if bound_is_template => {
                            let subs = std::collections::HashMap::from([(
                                bound_name.to_string(),
                                resolved_ty,
                            )]);
                            then_type.substitute(&subs)
                        }
                        _ => resolved_ty,
                    }
                };

                // For variadic class-string parameters, collect class
                // names from ALL arguments at and after param_idx and
                // form a union type (e.g. `A|B` from `A::class, B::class`).
                if is_variadic {
                    let mut class_names: Vec<String> = Vec::new();
                    for arg in args.iter().skip(param_idx) {
                        let trimmed = arg.trim();
                        if let Some(class_name) = extract_class_name_from_text(trimmed) {
                            let class_name = resolve_self_keyword(&class_name, class_ctx.calling)
                                .unwrap_or(class_name);
                            if !class_names.contains(&class_name) {
                                class_names.push(class_name);
                            }
                        } else if trimmed.starts_with('$')
                            && let Some(resolver) = var_resolver
                        {
                            for name in resolver(trimmed) {
                                if !class_names.contains(&name) {
                                    class_names.push(name);
                                }
                            }
                        }
                    }
                    if !class_names.is_empty() {
                        let class_names: Vec<String> = class_names
                            .into_iter()
                            .map(|n| crate::util::resolve_name_via_loader(&n, class_loader))
                            .collect();

                        // When a bound exists, check all collected classes.
                        // If any fails the bound check, fall through to the
                        // else branch rather than returning a wrong type.
                        let all_satisfy = class_names.iter().all(|n| satisfies_bound(n));
                        if !all_satisfy {
                            return choose_branch(false);
                        }

                        return Some(substitute_bound(named_or_union(class_names)));
                    }
                    return resolve_conditional_with_text_args_and_defaults(
                        else_type,
                        params,
                        text_args,
                        var_resolver,
                        class_ctx,
                        class_loader,
                        tpl,
                    );
                }

                // Check if the argument text matches `X::class`.  An omitted
                // argument already reads back as its `Foo::class` default, so
                // `app()` resolves the same as `app(Foo::class)`.
                let class_name = arg_text.and_then(extract_class_name_from_text).or_else(|| {
                    arg_text.and_then(|text| class_named_by_quoted_string(text, class_loader))
                });
                if let Some(class_name) = class_name {
                    let class_name =
                        resolve_self_keyword(&class_name, class_ctx.calling).unwrap_or(class_name);
                    let resolved = crate::util::resolve_name_via_loader(&class_name, class_loader);

                    // When a bound exists, verify the class is a subtype
                    // before taking the then-branch.  E.g. for
                    // `($type is class-string<FormFlowTypeInterface> ? FormFlowInterface : FormInterface)`
                    // with `ImageUploadFormType::class`: if `ImageUploadFormType`
                    // does NOT implement `FormFlowTypeInterface`, return
                    // `FormInterface` (else branch), not the class name.
                    if concrete_class_string_bound.is_some() {
                        return choose_branch(satisfies_bound(&resolved));
                    }

                    return Some(substitute_bound(PhpType::named(atom(&resolved))));
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
                            .map(|n| crate::util::resolve_name_via_loader(&n, class_loader))
                            .collect();

                        // When a bound exists, check all resolved names.
                        if concrete_class_string_bound.is_some() {
                            let all_satisfy = names.iter().all(|n| satisfies_bound(n));
                            return choose_branch(all_satisfy);
                        }

                        return Some(substitute_bound(named_or_union(names)));
                    }
                }
                // Argument isn't a ::class literal or resolvable variable → try else branch
                resolve_conditional_with_text_args_and_defaults(
                    else_type,
                    params,
                    text_args,
                    var_resolver,
                    class_ctx,
                    class_loader,
                    tpl,
                )
            } else if condition.is_null() {
                // The null (`then`) branch is taken when the parameter really
                // holds null: no argument at all (so it falls back to a null
                // default), an explicit `null`, or an argument whose resolved
                // type is null. Any other type takes the else branch, and a
                // type that may or may not be null (`?string`) leaves both
                // branches open rather than committing to one the call may
                // not take.
                let decided = match arg_text {
                    // A variadic parameter has no single value to judge: it
                    // holds every trailing argument, so the only question the
                    // condition can be asking is whether any were passed. Its
                    // first argument's own type says nothing about that, and
                    // for the by-reference out-parameters this shape is used
                    // for (`sscanf($s, $fmt, $a, $b)`) that type is usually
                    // unresolvable anyway.
                    _ if is_variadic => Some(arg_text_owned.is_none()),
                    None => Some(true),
                    Some(text) if text.trim().is_empty() => Some(true),
                    Some(text) => condition_result_from_text(condition, text).or_else(|| {
                        tpl.arg_type_resolver
                            .and_then(|resolve| resolve(text))
                            .and_then(|arg_ty| {
                                type_condition_result(&arg_ty, condition, class_loader)
                            })
                            .or_else(|| {
                                // With no resolver to consult, an argument that
                                // is not the literal `null` reads as non-null,
                                // which is what it usually is.
                                tpl.arg_type_resolver.is_none().then_some(false)
                            })
                    }),
                };
                let branch = match decided {
                    Some(is_null) => {
                        if is_null ^ *negated {
                            then_type
                        } else {
                            else_type
                        }
                    }
                    None => return undecided_answer(),
                };
                resolve_branch(branch)
            } else if matches!(condition.kind(), TypeKind::Literal(_)) {
                // Value condition (`$format is 0`, `$flags is 15`). A literal
                // argument settles it outright; anything else is settled by
                // the argument's resolved type when that turns out to be a
                // literal of its own, which is how a named constant
                // (`pathinfo($p, PATHINFO_FILENAME)`) or a local holding one
                // decides the branch it really takes.
                let decided = arg_text.and_then(|arg| {
                    condition_result_from_text(condition, arg).or_else(|| {
                        tpl.arg_type_resolver
                            .and_then(|resolve| resolve(arg))
                            .and_then(|arg_ty| literal_condition_result(condition, &arg_ty))
                    })
                });
                let branch = match decided {
                    Some(matched) => {
                        if matched ^ *negated {
                            then_type
                        } else {
                            else_type
                        }
                    }
                    // Neither the text nor the resolved type pins the value
                    // down, so the call really may take either branch. Union
                    // them rather than committing to the else, which would
                    // report a type the call can't promise.
                    None if arg_text.is_some() && tpl.arg_type_resolver.is_some() => {
                        return undecided_answer();
                    }
                    None => else_type,
                };
                resolve_branch(branch)
            } else if let Some((cond_class, cond_const)) = class_const_condition_parts(condition) {
                // Class-constant condition (e.g. `$mode is PDO::FETCH_ASSOC`).
                // Take the then-branch when the bound argument is the same
                // class constant.
                let matched = arg_text
                    .and_then(|arg| arg.trim().rsplit_once("::"))
                    .is_some_and(|(arg_class, arg_const)| {
                        class_const_matches(
                            cond_class,
                            cond_const,
                            arg_class.trim(),
                            arg_const.trim(),
                            class_ctx.calling,
                        )
                    });
                let take_then = matched ^ *negated;
                resolve_conditional_with_text_args_and_defaults(
                    if take_then { then_type } else { else_type },
                    params,
                    text_args,
                    var_resolver,
                    class_ctx,
                    class_loader,
                    tpl,
                )
            } else {
                // IsType equivalent (`$x is string`, `$x is array|string`,
                // …). Decide the branch in three ways, in order of
                // confidence: (1) from the argument's syntactic form (a
                // literal), (2) from the argument's resolved type when a
                // resolver is available (e.g. a method-call chain that
                // returns `string`). When neither is conclusive, fall
                // through to the else branch as before.
                let decided = arg_text.and_then(|arg| {
                    condition_result_from_text(condition, arg).or_else(|| {
                        tpl.arg_type_resolver
                            .and_then(|resolve| resolve(arg))
                            .and_then(|arg_ty| {
                                type_condition_result(&arg_ty, condition, class_loader)
                            })
                    })
                });
                let branch = match decided {
                    Some(satisfied) => {
                        if satisfied ^ *negated {
                            then_type
                        } else {
                            else_type
                        }
                    }
                    None => {
                        // The condition is genuinely undecidable: the argument
                        // is an expression whose type we could not pin down.
                        // When a resolver was available (i.e. a real
                        // resolution context, not a bare completion lookup),
                        // the true result is one of the two branches, so
                        // return their union rather than committing to the
                        // else branch — otherwise `Str::replace(…, $x->y())`
                        // would resolve to `string[]` and falsely flag a
                        // `string` argument.
                        if arg_text.is_some() && tpl.arg_type_resolver.is_some() {
                            return undecided_answer();
                        }
                        else_type
                    }
                };
                resolve_branch(branch)
            }
        }
        _ => {
            if conditional.is_uninformative_return() {
                return None;
            }
            Some(conditional.clone())
        }
    }
}

/// Checks whether the argument text is a quoted string literal.
fn arg_is_string_literal(arg: &str) -> bool {
    let t = arg.trim();
    (t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"'))
}

/// Checks whether the argument text is an integer literal, in any of the
/// notations PHP accepts (`10`, `-10`, `0x0a`, `0b1010`, `012`, `1_0`).
fn arg_is_int_literal(arg: &str) -> bool {
    crate::php_type::parse_php_int_literal(arg.trim()).is_some()
}

/// Checks whether the argument text is a float literal.
fn arg_is_float_literal(arg: &str) -> bool {
    let t = arg.trim();
    let t = t.strip_prefix('-').unwrap_or(t);
    t.contains('.') && t.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// The syntactic form of an argument's source text, as far as it can be
/// classified without resolving its type. Used to decide `is <Type>`
/// conditions against literal arguments.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgForm {
    StringLit,
    IntLit,
    FloatLit,
    /// The two boolean literals are kept apart so a condition naming one of
    /// them (`$as_number is true`) is decided rather than treated as a plain
    /// `is bool`.
    True,
    False,
    Null,
    ArrayLit,
    /// Any expression whose type cannot be read from its syntax alone
    /// (variables, property/method chains, function calls, closures, …).
    Unknown,
}

/// Classify the syntactic form of an argument's source text.
fn classify_arg_form(arg: &str) -> ArgForm {
    let t = arg.trim();
    if t.is_empty() {
        return ArgForm::Unknown;
    }
    if arg_is_string_literal(t) {
        return ArgForm::StringLit;
    }
    if arg_is_int_literal(t) {
        return ArgForm::IntLit;
    }
    if arg_is_float_literal(t) {
        return ArgForm::FloatLit;
    }
    if t.eq_ignore_ascii_case("true") {
        return ArgForm::True;
    }
    if t.eq_ignore_ascii_case("false") {
        return ArgForm::False;
    }
    if t.eq_ignore_ascii_case("null") {
        return ArgForm::Null;
    }
    if t.starts_with('[') || t.to_ascii_lowercase().starts_with("array(") {
        return ArgForm::ArrayLit;
    }
    ArgForm::Unknown
}

/// The literal value an argument's source text denotes, when the text is a
/// literal at all (`0`, `-1`, `1.5`, `'json'`).
fn literal_value_from_text(arg_text: &str) -> Option<LiteralValue> {
    let t = arg_text.trim();
    match classify_arg_form(t) {
        ArgForm::StringLit => Some(LiteralValue::string_raw(t)),
        ArgForm::IntLit => Some(LiteralValue::int(t)),
        ArgForm::FloatLit => Some(LiteralValue::float(t)),
        _ => None,
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

/// Decide a conditional's condition against an argument's source text.
///
/// A value condition (`$format is 0`) is settled by comparing literal values,
/// so `0x1` satisfies `is 1` and `"foo"` satisfies `is 'foo'`. A type condition
/// (`$x is string`, `$x is array|string`) is settled by the argument's
/// syntactic form. Returns `None` when the text settles neither, which happens
/// whenever the argument is an expression rather than a literal.
fn condition_result_from_text(condition: &PhpType, arg_text: &str) -> Option<bool> {
    match condition.kind() {
        TypeKind::Union(members) => combine_union_results(
            members
                .iter()
                .map(|member| condition_result_from_text(member, arg_text)),
        ),
        TypeKind::Literal(expected) => match literal_value_from_text(arg_text) {
            Some(actual) => Some(crate::php_type::literals_equal(expected, &actual)),
            // A value of another kind entirely cannot be the literal, whatever
            // it holds: `null` is never `'array'`.
            None => match (literal_category(expected), form_category(arg_text)) {
                (lit, Some(form)) if lit != form => Some(false),
                _ => None,
            },
        },
        _ => condition_result_from_form(condition, classify_arg_form(arg_text)),
    }
}

/// Decide a value condition (`$flags is 15`) against an argument's *resolved*
/// type, for the arguments whose source text is not a literal.
///
/// A resolved literal settles the comparison both ways: `PATHINFO_FILENAME`
/// resolves to `8`, which refutes `is 15` as conclusively as the text `8`
/// would have. Anything broader than a literal (`int`, a union, an unresolved
/// expression) leaves the condition undecided.
fn literal_condition_result(condition: &PhpType, arg_ty: &PhpType) -> Option<bool> {
    match condition.kind() {
        TypeKind::Union(members) => combine_union_results(
            members
                .iter()
                .map(|member| literal_condition_result(member, arg_ty)),
        ),
        TypeKind::Literal(expected) => match arg_ty.as_literal() {
            Some(actual) => Some(crate::php_type::literals_equal(expected, actual)),
            // Even without a literal value, a type from another category
            // cannot hold one: an `int` argument is never `'array'`.
            None => match type_category(arg_ty) {
                Some(category) if category != literal_category(expected) => Some(false),
                _ => None,
            },
        },
        _ => None,
    }
}

/// The runtime category a literal value belongs to, in the same vocabulary
/// [`type_category`] reports.
fn literal_category(value: &LiteralValue) -> &'static str {
    match value {
        LiteralValue::Int(_) => "int",
        LiteralValue::Float(_) => "float",
        LiteralValue::String(_) => "string",
    }
}

/// The runtime category an argument's source text belongs to, or `None` when
/// its syntax does not say.
fn form_category(arg_text: &str) -> Option<&'static str> {
    match classify_arg_form(arg_text) {
        ArgForm::StringLit => Some("string"),
        ArgForm::IntLit => Some("int"),
        ArgForm::FloatLit => Some("float"),
        ArgForm::True | ArgForm::False => Some("bool"),
        ArgForm::Null => Some("null"),
        ArgForm::ArrayLit => Some("array"),
        ArgForm::Unknown => None,
    }
}

/// Decide a type condition (`$x is string`, `$x is array|string`) from the
/// argument's syntactic form.
///
/// A literal argument has a fully known type, so the answer is conclusive;
/// [`ArgForm::Unknown`] yields `None` for the caller to handle. Union
/// conditions are satisfied when any member is.
fn condition_result_from_form(condition: &PhpType, form: ArgForm) -> Option<bool> {
    if form == ArgForm::Unknown {
        return None;
    }
    match condition.kind() {
        TypeKind::Union(members) => combine_union_results(
            members
                .iter()
                .map(|member| condition_result_from_form(member, form)),
        ),
        _ => Some(scalar_condition_matches_form(condition, form)),
    }
}

/// Combine the per-member results of a union condition: it is satisfied when
/// any member is, and refuted only when every member is refuted.
fn combine_union_results(results: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    let mut any_true = false;
    let mut all_false = true;
    for result in results {
        match result {
            Some(true) => {
                any_true = true;
                all_false = false;
            }
            Some(false) => {}
            None => all_false = false,
        }
    }
    if any_true {
        Some(true)
    } else if all_false {
        Some(false)
    } else {
        None
    }
}

/// Whether a single (non-union) type condition is satisfied by a literal
/// argument form. A literal always has a fully known type, so this is
/// conclusive (unlike [`ArgForm::Unknown`], which the caller handles).
fn scalar_condition_matches_form(condition: &PhpType, form: ArgForm) -> bool {
    match form {
        ArgForm::StringLit => condition.is_string_type(),
        ArgForm::IntLit => condition.is_int(),
        ArgForm::FloatLit => condition.is_float(),
        // A boolean literal satisfies `is bool` either way, and the matching
        // half of `is true` / `is false`.
        ArgForm::True => condition.is_bool() || condition.is_true(),
        ArgForm::False => condition.is_bool() || condition.is_false(),
        ArgForm::Null => condition.is_null(),
        ArgForm::ArrayLit => condition.is_array_like(),
        ArgForm::Unknown => false,
    }
}

/// The broad runtime category a resolved type belongs to, used to decide
/// mutually-exclusive `is <scalar>` conditions. Returns `None` when the type
/// cannot be placed in a single category (so the condition stays undecided).
fn type_category(t: &PhpType) -> Option<&'static str> {
    if t.is_string_subtype() {
        Some("string")
    } else if t.is_int_subtype() {
        Some("int")
    } else if t.is_float_subtype() {
        Some("float")
    } else if t.is_bool() || t.is_true() || t.is_false() {
        // `true` and `false` are boolean values, not class names — without
        // this they fall through to the class-instance case below.
        Some("bool")
    } else if t.is_null() {
        Some("null")
    } else if t.is_array_like() {
        Some("array")
    } else if matches!(t.kind(), TypeKind::Callable(_)) || t.base_name().is_some() {
        // A closure/callable or any class instance — not a scalar or array.
        Some("object")
    } else {
        None
    }
}

/// The category named by a (non-union) `is <Type>` condition, or `None` when
/// the condition is not a plain scalar/array type we can categorise.
fn condition_category(condition: &PhpType) -> Option<&'static str> {
    if condition.is_string_type() {
        Some("string")
    } else if condition.is_int() {
        Some("int")
    } else if condition.is_float() {
        Some("float")
    } else if condition.is_bool() {
        Some("bool")
    } else if condition.is_null() {
        Some("null")
    } else if condition.is_array_like() {
        Some("array")
    } else if condition.is_object() {
        // The one category [`type_category`] reports that has no keyword of
        // its own below: `is object` asks the same question it answers for
        // every class instance, so a scalar argument refutes it outright
        // rather than being handed to the class-hierarchy check, where the
        // bare keyword resolves to no class at all and settles nothing.
        Some("object")
    } else {
        None
    }
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
/// Whether a value of `arg_ty` satisfies `condition`, for callers outside
/// this module that need to walk a conditional's branches themselves.
pub(in crate::type_engine) fn condition_holds_for_type(
    arg_ty: &PhpType,
    condition: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<bool> {
    type_condition_result(arg_ty, condition, class_loader)
}

fn type_condition_result(
    arg_ty: &PhpType,
    condition: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<bool> {
    if arg_ty.is_mixed() || arg_ty.is_untyped() {
        return None;
    }
    // A condition naming a literal value (`$invalidBehavior is 0|1`) compares
    // values, not types, so it is settled by the argument's own value.
    if matches!(condition.kind(), TypeKind::Literal(_)) {
        return literal_condition_result(condition, arg_ty);
    }
    // `is true` / `is false` name one boolean value, so only an argument
    // narrowed to a value of its own settles them: a plain `bool` really may
    // be either.
    if condition.is_true() || condition.is_false() {
        if arg_ty.is_true() || arg_ty.is_false() {
            return Some(condition.is_true() == arg_ty.is_true());
        }
        return None;
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
        (Some(arg_cat), Some(cond_cat)) => Some(arg_cat == cond_cat),
        // A condition that names no scalar category names a class, which the
        // class hierarchy decides (`$id is Arrayable`).
        (Some(arg_cat), None) => class_condition_result(arg_ty, arg_cat, condition, class_loader),
        _ => None,
    }
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

/// Split a textual argument list by commas, respecting nested brackets
/// so that `"foo(a, b), c"` splits into `["foo(a, b)", "c"]`.
///
/// Braces count as brackets too, so a closure argument written out in full
/// (`function ($a) { return [$a, 1]; }, $rest`) stays one argument.
pub fn split_text_args(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0u32;
    let mut start = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut prev_was_backslash = false;

    for (i, ch) in text.char_indices() {
        if prev_was_backslash {
            prev_was_backslash = false;
            continue;
        }
        match ch {
            '\\'
                // Only treat as escape if inside a quote
                if in_single_quote || in_double_quote =>
            {
                prev_was_backslash = true;
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            '(' | '[' | '{' if !in_single_quote && !in_double_quote => {
                depth += 1;
            }
            ')' | ']' | '}' if !in_single_quote && !in_double_quote => {
                depth = depth.saturating_sub(1);
            }
            ',' if depth == 0 && !in_single_quote && !in_double_quote => {
                result.push(&text[start..i]);
                start = i + 1; // skip the comma
            }
            _ => {}
        }
    }
    // Push the last segment (or the only one if there were no commas).
    if start <= text.len() {
        let last = &text[start..];
        if !last.trim().is_empty() {
            result.push(last);
        }
    }
    result
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

/// Rewrite a leading `self::`/`static::`/`parent::` in a parameter default's
/// source text to `declaring_class_name`.
///
/// A default like `self::EXCEPTION_ON_INVALID_REFERENCE` is written inside
/// the method that declares the parameter, so `self` names that class, not
/// whichever class the omitted argument is read back against downstream.
fn resolve_default_self_keyword<'a>(
    text: &'a str,
    declaring_class_name: Option<&str>,
) -> Cow<'a, str> {
    let Some((class_part, rest)) = text.trim().split_once("::") else {
        return Cow::Borrowed(text);
    };
    match resolve_self_keyword(class_part.trim(), declaring_class_name) {
        Some(resolved) => Cow::Owned(format!("{resolved}::{rest}")),
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
fn class_const_condition_parts(condition: &PhpType) -> Option<(&str, &str)> {
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

            // `$this` names the receiver, not a parameter — see the
            // matching branch in
            // [`resolve_conditional_with_text_args_and_defaults`].
            if target.eq_ignore_ascii_case("$this") {
                let Some(this_ctx) = this_ctx else {
                    return union_branch_types(recurse(then_type), recurse(else_type));
                };
                let condition = condition.replace_bare_self(this_ctx.declaring_class_name);
                let decided = if condition.is_null() {
                    Some(this_ctx.this_type.is_null())
                } else {
                    type_condition_result(this_ctx.this_type, &condition, this_ctx.class_loader)
                };
                return match decided {
                    Some(satisfied) if satisfied ^ *negated => recurse(then_type),
                    Some(_) => recurse(else_type),
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
            if conditional.is_uninformative_return() {
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
fn try_resolve_with_template_default(
    template_name: &str,
    negated: bool,
    condition: &PhpType,
    then_type: &PhpType,
    else_type: &PhpType,
    template_defaults: Option<&HashMap<String, PhpType>>,
) -> Option<PhpType> {
    let defaults = template_defaults?;
    let default_value = defaults.get(template_name)?;

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
    if branch.is_uninformative_return() {
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

/// The class a plain string argument names, when it names one at all.
///
/// PHP accepts a string wherever a `class-string` is expected, so
/// `$container->make('App\Services\Clock')` is as much a class-string call as
/// `make(Clock::class)` is; in a Laravel project the loader also answers for
/// the container keys service providers bind (`make('sentry')`).  The class
/// has to actually resolve: an ordinary string argument must keep falling
/// through to the conditional's else branch rather than being read as the name
/// of a class that does not exist.
fn class_named_by_string(
    name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    class_loader(name).map(|cls| cls.fqn().to_string())
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

/// Extract the class name from an `X::class` expression.
///
/// Matches `Expression::Access(Access::ClassConstant(cca))` where the
/// constant selector is the identifier `class`.
pub(crate) fn extract_class_string_from_expr(expr: &Expression<'_>) -> Option<String> {
    if let Expression::Access(Access::ClassConstant(cca)) = expr
        && let ClassLikeConstantSelector::Identifier(ident) = &cca.constant
        && ident.value == b"class"
    {
        // Extract the class name from the LHS
        return match cca.class {
            Expression::Identifier(class_ident) => {
                Some(crate::atom::bytes_to_str(class_ident.value()).to_string())
            }
            Expression::Self_(_) => Some("self".to_string()),
            Expression::Static(_) => Some("static".to_string()),
            Expression::Parent(_) => Some("parent".to_string()),
            _ => None,
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ClassLikeKind;

    /// A single required parameter with the given name (including `$`).
    fn param(name: &str) -> ParameterInfo {
        ParameterInfo {
            name: crate::atom::atom(name),
            is_required: true,
            type_hint: None,
            native_type_hint: None,
            description: None,
            default_value: None,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
            param_out_type: None,
        }
    }

    /// An optional parameter with the given declared default value.
    fn param_with_default(name: &str, default: &str) -> ParameterInfo {
        ParameterInfo {
            is_required: false,
            default_value: Some(default.to_string()),
            ..param(name)
        }
    }

    /// Resolve a `str_word_count`-style conditional keyed on the value of a
    /// `$format` parameter that defaults to `0`.
    fn resolve_word_count(text_args: &str) -> Option<String> {
        let cond = PhpType::parse(
            "($format is 0 ? int : ($format is 1 ? list<string> : ($format is 2 ? array<int, string> : list<string>|int)))",
        );
        let params = [param("$string"), param_with_default("$format", "0")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let tpl = TemplateContext::with_params(&[]);
        resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            text_args,
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string())
    }

    /// An omitted argument takes the parameter's declared default, so the
    /// condition on `$format` is decided by that `0` rather than left open.
    #[test]
    fn omitted_argument_is_decided_by_its_declared_default() {
        assert_eq!(resolve_word_count("$text").as_deref(), Some("int"));
    }

    /// A value condition compares literal values, so an explicit int argument
    /// picks its own branch.
    #[test]
    fn int_literal_argument_matches_value_condition() {
        assert_eq!(
            resolve_word_count("$text, 1").as_deref(),
            Some("list<string>")
        );
        assert_eq!(
            resolve_word_count("$text, 2").as_deref(),
            Some("array<int, string>")
        );
        // Comparison is by value, not by spelling.
        assert_eq!(
            resolve_word_count("$text, 0x1").as_deref(),
            Some("list<string>")
        );
    }

    /// A named argument reaches the parameter it targets rather than the slot
    /// it sits in.
    #[test]
    fn named_argument_matches_value_condition() {
        assert_eq!(
            resolve_word_count("$text, format: 1").as_deref(),
            Some("list<string>")
        );
    }

    /// An argument that is not a literal decides nothing, and with no resolver
    /// to fall back on the conditional collapses to its final else branch —
    /// every value the call could return.
    #[test]
    fn non_literal_argument_leaves_value_condition_undecided() {
        assert_eq!(
            resolve_word_count("$text, $format").as_deref(),
            Some("list<string>|int")
        );
    }

    /// Resolve a `pathinfo`-style value conditional with a resolver that
    /// answers for named constants, returning the resolved type's display
    /// string.
    fn resolve_pathinfo(text_args: &str) -> Option<String> {
        let cond = PhpType::parse("($flags is 15 ? Shape : string)");
        let params = [param("$path"), param_with_default("$flags", "PATHINFO_ALL")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let resolver = |t: &str| match t.trim() {
            "PATHINFO_ALL" => Some(PhpType::literal_int("15")),
            "PATHINFO_FILENAME" => Some(PhpType::literal_int("8")),
            _ => None,
        };
        let tpl = TemplateContext {
            defaults: None,
            params: &[],
            bindings: &[],
            arg_type_resolver: Some(&resolver),
            this_type: None,
        };
        resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            text_args,
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string())
    }

    /// A named constant is not a syntactic literal, but its resolved type is
    /// one, so it settles a value condition both ways.
    #[test]
    fn value_condition_is_decided_by_a_resolved_literal() {
        assert_eq!(resolve_pathinfo("$path").as_deref(), Some("Shape"));
        assert_eq!(
            resolve_pathinfo("$path, PATHINFO_ALL").as_deref(),
            Some("Shape")
        );
        assert_eq!(
            resolve_pathinfo("$path, PATHINFO_FILENAME").as_deref(),
            Some("string")
        );
    }

    /// An argument whose value cannot be pinned down really may take either
    /// branch, so both are returned rather than committing to the else.
    #[test]
    fn undecidable_value_condition_unions_branches() {
        assert_eq!(
            resolve_pathinfo("$path, $flags").as_deref(),
            Some("Shape|string")
        );
    }

    /// A parameter default written `self::SOME_CONST` (mirroring
    /// `ContainerInterface::get()`'s `$invalidBehavior`) is written inside
    /// the method that declares it, so its `self` must resolve against that
    /// declaring class, not whichever unrelated class the call happens to
    /// sit in — otherwise the constant is never found and the condition is
    /// left undecided.
    #[test]
    fn omitted_default_self_const_resolves_against_declaring_class() {
        let cond = PhpType::parse("($mode is 1 ? Found : Found|null)");
        let params = [
            param("$id"),
            param_with_default("$mode", "self::EXCEPTION_ON_INVALID_REFERENCE"),
        ];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let resolver = |t: &str| match t.trim() {
            "ContainerInterface::EXCEPTION_ON_INVALID_REFERENCE" => Some(PhpType::literal_int("1")),
            _ => None,
        };
        let tpl = TemplateContext {
            defaults: None,
            params: &[],
            bindings: &[],
            arg_type_resolver: Some(&resolver),
            this_type: None,
        };
        // The call sits in an unrelated class; only the declaring class
        // resolves the default's `self` to the constant the resolver knows.
        let resolved = resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            "Service::class",
            None,
            ConditionalClassContext {
                calling: Some("App\\Controller"),
                declaring: Some("ContainerInterface"),
            },
            loader,
            &tpl,
        )
        .map(|t| t.to_string());
        assert_eq!(resolved.as_deref(), Some("Found"));
    }

    /// `is true` and `is false` name one boolean value each, so the two
    /// literals pick different branches instead of both reading as `is bool`.
    #[test]
    fn boolean_literal_arguments_decide_is_true_conditions() {
        let params = [param("$value"), param_with_default("$flag", "false")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let tpl = TemplateContext::with_params(&[]);
        let resolve = |cond: &str, text_args: &str| {
            resolve_conditional_with_text_args_and_defaults(
                &PhpType::parse(cond),
                &params,
                text_args,
                None,
                ConditionalClassContext::default(),
                loader,
                &tpl,
            )
            .map(|t| t.to_string())
        };

        let is_true = "($flag is true ? Then : Else)";
        assert_eq!(resolve(is_true, "$value, true").as_deref(), Some("Then"));
        assert_eq!(resolve(is_true, "$value, false").as_deref(), Some("Else"));
        // The declared default answers for the omitted argument.
        assert_eq!(resolve(is_true, "$value").as_deref(), Some("Else"));

        let is_false = "($flag is false ? Then : Else)";
        assert_eq!(resolve(is_false, "$value, false").as_deref(), Some("Then"));
        assert_eq!(resolve(is_false, "$value, true").as_deref(), Some("Else"));

        // A plain `is bool` is satisfied by either literal.
        let is_bool = "($flag is bool ? Then : Else)";
        assert_eq!(resolve(is_bool, "$value, true").as_deref(), Some("Then"));
        assert_eq!(resolve(is_bool, "$value, false").as_deref(), Some("Then"));
    }

    /// A resolved `bool` cannot settle `is true`: the argument really may be
    /// either value. A resolved `true`/`false` can.
    #[test]
    fn is_true_condition_needs_a_resolved_boolean_value() {
        let params = [param("$flag")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let resolve = |resolved: PhpType| {
            let resolver = |_: &str| Some(resolved.clone());
            let tpl = TemplateContext {
                defaults: None,
                params: &[],
                bindings: &[],
                arg_type_resolver: Some(&resolver),
                this_type: None,
            };
            resolve_conditional_with_text_args_and_defaults(
                &PhpType::parse("($flag is true ? Then : Else)"),
                &params,
                "$flag",
                None,
                ConditionalClassContext::default(),
                loader,
                &tpl,
            )
            .map(|t| t.to_string())
        };

        assert_eq!(resolve(PhpType::parse("true")).as_deref(), Some("Then"));
        assert_eq!(resolve(PhpType::parse("false")).as_deref(), Some("Else"));
        assert_eq!(
            resolve(PhpType::bool()).as_deref(),
            Some("Then|Else"),
            "a plain bool leaves both branches open"
        );
    }

    /// With no arguments at all, the no-args path decides the condition from
    /// the declared default the same way the text path does.
    #[test]
    fn no_args_path_is_decided_by_declared_default() {
        let cond = PhpType::parse("($format is 0 ? int : list<string>)");
        let params = [param_with_default("$format", "0")];
        assert_eq!(
            resolve_conditional_without_args(&cond, &params).map(|t| t.to_string()),
            Some("int".to_string())
        );

        // A non-null default no longer reads as null just because the
        // parameter is optional.
        let nullable = PhpType::parse("($format is null ? int : list<string>)");
        assert_eq!(
            resolve_conditional_without_args(&nullable, &params).map(|t| t.to_string()),
            Some("list<string>".to_string())
        );
    }

    /// Resolve a `PDOStatement::fetch`-style conditional keyed on the fetch
    /// mode class constant, returning the resolved type's display string.
    fn resolve_fetch(text_args: &str) -> Option<String> {
        // ($mode is PDO::FETCH_OBJ ? \stdClass|false
        //   : ($mode is PDO::FETCH_ASSOC ? array<string, mixed>|false : mixed))
        let cond = PhpType::parse(
            "($mode is PDO::FETCH_OBJ ? \\stdClass|false : ($mode is PDO::FETCH_ASSOC ? array<string, mixed>|false : mixed))",
        );
        let params = [param("$mode")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let tpl = TemplateContext::with_params(&[]);
        resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            text_args,
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string())
    }

    #[test]
    fn class_const_condition_selects_matching_branch() {
        assert!(
            resolve_fetch("\\PDO::FETCH_OBJ")
                .unwrap()
                .contains("stdClass")
        );
        assert!(
            resolve_fetch("\\PDO::FETCH_ASSOC")
                .unwrap()
                .contains("array")
        );
    }

    #[test]
    fn class_const_condition_ignores_leading_backslash() {
        // Argument without a leading backslash still matches the condition.
        assert!(
            resolve_fetch("PDO::FETCH_OBJ")
                .unwrap()
                .contains("stdClass")
        );
    }

    #[test]
    fn class_const_condition_unlisted_mode_falls_through() {
        // A mode with no dedicated branch reaches the `mixed` else branch.
        // `mixed` is informative (a value of unknown type), so it flows
        // through rather than yielding no resolved type.
        assert_eq!(
            resolve_fetch("\\PDO::FETCH_COLUMN").as_deref(),
            Some("mixed")
        );
    }

    #[test]
    fn class_const_condition_requires_matching_class() {
        // The same constant name on a different class must not match the
        // `PDO::FETCH_OBJ` branch — it falls through past every PDO branch to
        // the `mixed` else, which flows through as `mixed`.
        assert_eq!(resolve_fetch("Other::FETCH_OBJ").as_deref(), Some("mixed"));
        // A different class whose constant matches an inner branch also fails.
        assert_eq!(
            resolve_fetch("Other::FETCH_ASSOC").as_deref(),
            Some("mixed")
        );
    }

    #[test]
    fn class_const_condition_parts_rejects_non_constants() {
        assert!(class_const_condition_parts(&PhpType::named(atom("string"))).is_none());
        assert!(class_const_condition_parts(&PhpType::raw("Foo::*")).is_none());
        assert!(class_const_condition_parts(&PhpType::raw("Foo::class")).is_none());
        assert_eq!(
            class_const_condition_parts(&PhpType::raw("PDO::FETCH_OBJ")),
            Some(("PDO", "FETCH_OBJ"))
        );
    }

    /// A `$x is (array|string)` condition with a string-literal argument
    /// takes the then-branch, mirroring Laravel's `keyBy`/`groupBy` key type.
    #[test]
    fn union_scalar_condition_with_string_literal_takes_then() {
        let cond = PhpType::parse("($key is (array|string) ? array-key : ObjectKey)");
        let params = [param("$key")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let tpl = TemplateContext::with_params(&[]);
        let resolved = resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            "'field'",
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string());
        assert_eq!(resolved.as_deref(), Some("array-key"));
    }

    /// An `is string` condition whose argument is a non-literal expression
    /// (a method-call chain) is decided from the argument's resolved type
    /// via the resolver, taking the then-branch when it is a string.
    #[test]
    fn is_string_condition_uses_resolved_arg_type() {
        let cond = PhpType::parse("($subject is string ? string : list<string>)");
        let params = [param("$subject")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let resolver = |t: &str| {
            if t == "$obj->toHtml()" {
                Some(PhpType::named(atom("string")))
            } else {
                None
            }
        };
        let tpl = TemplateContext {
            defaults: None,
            params: &[],
            bindings: &[],
            arg_type_resolver: Some(&resolver),
            this_type: None,
        };
        let resolved = resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            "$obj->toHtml()",
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string());
        assert_eq!(resolved.as_deref(), Some("string"));
    }

    #[test]
    fn scalar_conditions_classify_resolved_int_and_float_literals() {
        let params = [param("$value")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;

        for (condition_type, literal, expected) in [
            ("int", PhpType::literal_int("1"), "IntBranch"),
            ("float", PhpType::literal_float("1.5"), "FloatBranch"),
        ] {
            let cond = PhpType::parse(&format!(
                "($value is {condition_type} ? {expected} : ElseBranch)"
            ));
            let resolver = |_: &str| Some(literal.clone());
            let tpl = TemplateContext {
                defaults: None,
                params: &[],
                bindings: &[],
                arg_type_resolver: Some(&resolver),
                this_type: None,
            };
            let resolved = resolve_conditional_with_text_args_and_defaults(
                &cond,
                &params,
                "$value",
                None,
                ConditionalClassContext::default(),
                loader,
                &tpl,
            );
            assert_eq!(
                resolved.as_ref().map(ToString::to_string).as_deref(),
                Some(expected)
            );
        }
    }

    /// When the argument's type cannot be resolved (e.g. a magic property
    /// chain that resolves to `mixed`) an `is string` condition is genuinely
    /// undecidable, so the result is the union of both branches rather than a
    /// commitment to the else branch.
    #[test]
    fn undecidable_is_string_condition_unions_branches() {
        let cond = PhpType::parse("($subject is string ? string : list<string>)");
        let params = [param("$subject")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        // Resolver that cannot resolve the argument (returns None).
        let resolver = |_: &str| None;
        let tpl = TemplateContext {
            defaults: None,
            params: &[],
            bindings: &[],
            arg_type_resolver: Some(&resolver),
            this_type: None,
        };
        let resolved = resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            "$obj->magic->toHtml()",
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string());
        assert_eq!(resolved.as_deref(), Some("string|list<string>"));
    }

    /// An argument whose resolved type is mutually exclusive with the
    /// condition (a closure against `array|string`) takes the else branch,
    /// not the union — this keeps `Collection::groupBy(fn …)` resolving to
    /// its non-array key type.
    #[test]
    fn closure_arg_refutes_array_or_string_condition() {
        let cond = PhpType::parse("($groupBy is (array|string) ? array-key : Value)");
        let params = [param("$groupBy")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let resolver = |_: &str| Some(PhpType::named(atom("Closure")));
        let tpl = TemplateContext {
            defaults: None,
            params: &[],
            bindings: &[],
            arg_type_resolver: Some(&resolver),
            this_type: None,
        };
        let resolved = resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            "fn($x) => $x->value",
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string());
        assert_eq!(resolved.as_deref(), Some("Value"));
    }

    /// Without a resolver an unclassifiable `is string` argument falls
    /// through to the else branch (unchanged conservative default).
    #[test]
    fn is_string_condition_without_resolver_falls_to_else() {
        let cond = PhpType::parse("($subject is string ? string : list<string>)");
        let params = [param("$subject")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let tpl = TemplateContext::with_params(&[]);
        let resolved = resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            "$obj->toHtml()",
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string());
        assert_eq!(resolved.as_deref(), Some("list<string>"));
    }

    /// A conditional nested inside a generic wrapper is collapsed against
    /// the call arguments rather than surviving raw.
    #[test]
    fn nested_conditional_in_generic_is_evaluated() {
        let ty =
            PhpType::parse("Collection<($key is (array|string) ? array-key : ObjectKey), Value>");
        assert!(ty.contains_conditional());
        let params = [param("$key")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let tpl = TemplateContext::with_params(&[]);
        let evaluated = evaluate_nested_conditionals_text(
            &ty,
            &params,
            "'field'",
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        );
        assert!(!evaluated.contains_conditional());
        assert_eq!(evaluated.to_string(), "Collection<array-key, Value>");
    }

    /// A multi-level conditional whose branches are resolved via a template
    /// default (as in Laravel's `groupBy`) collapses fully — the template
    /// default resolver returns a branch that is itself a conditional, and
    /// the evaluator must keep collapsing rather than leaving a raw residue.
    #[test]
    fn nested_template_default_conditional_collapses_fully() {
        // static<($g is array|string ? array-key
        //   : (T is \UnitEnum ? array-key : (T is \Stringable ? string : T))), V>
        let ty = PhpType::parse(
            "Collection<($g is (array|string) ? array-key : (T is \\UnitEnum ? array-key : (T is \\Stringable ? string : T))), V>",
        );
        let params = [param("$g")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        // The closure return type binds T to a nullable string key. As in the
        // real call path, template substitution runs before the evaluator, so
        // every `T` in a branch position is already concrete; only the `T is
        // …` condition subjects (carried in the conditional's `param` field)
        // remain and are evaluated via the template defaults.
        let mut defaults = HashMap::new();
        defaults.insert(
            "T".to_string(),
            PhpType::union(vec![
                PhpType::named(atom("string")),
                PhpType::named(atom("null")),
            ]),
        );
        let ty = ty.substitute(&defaults);
        assert!(ty.contains_conditional());
        let resolver = |_: &str| Some(PhpType::named(atom("Closure")));
        let tpl = TemplateContext {
            defaults: Some(&defaults),
            params: &[crate::atom::atom("T")],
            bindings: &[],
            arg_type_resolver: Some(&resolver),
            this_type: None,
        };
        let evaluated = evaluate_nested_conditionals_text(
            &ty,
            &params,
            "fn($x) => $x->value",
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        );
        assert!(
            !evaluated.contains_conditional(),
            "residual conditional left in {evaluated}"
        );
        assert_eq!(evaluated.to_string(), "Collection<string|null, V>");
    }

    /// A loader answering for an Eloquent-shaped hierarchy: `Collection`
    /// implements `Arrayable`, `Order` extends `Model`, and nothing else
    /// relates to anything.
    fn eloquent_loader(name: &str) -> Option<Arc<ClassInfo>> {
        let class = |name: &str, parent: Option<&str>, interfaces: &[&str], kind| ClassInfo {
            name: atom(name),
            parent_class: parent.map(atom),
            interfaces: interfaces.iter().map(|i| atom(i)).collect(),
            kind,
            ..ClassInfo::default()
        };
        let info = match name.trim_start_matches('\\') {
            "Collection" => class("Collection", None, &["Arrayable"], ClassLikeKind::Class),
            "Arrayable" => class("Arrayable", None, &[], ClassLikeKind::Interface),
            "Model" => class("Model", None, &[], ClassLikeKind::Class),
            "Order" => class("Order", Some("Model"), &[], ClassLikeKind::Class),
            _ => return None,
        };
        Some(Arc::new(info))
    }

    /// Resolve an Eloquent `find`-shaped conditional against an argument of
    /// the given resolved type.
    fn resolve_find(arg_type: Option<PhpType>) -> Option<String> {
        let cond = PhpType::parse("($id is array<mixed>|Arrayable ? Collection : Order|null)");
        let params = [param("$id")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &eloquent_loader;
        let resolver = |_: &str| arg_type.clone();
        let tpl = TemplateContext {
            defaults: None,
            params: &[],
            bindings: &[],
            arg_type_resolver: Some(&resolver),
            this_type: None,
        };
        resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            "$id",
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string())
    }

    /// A condition naming a class is decided by the class hierarchy: a scalar
    /// can never satisfy it, and a class that implements the named interface
    /// always does.
    #[test]
    fn class_condition_is_decided_by_the_resolved_argument_type() {
        assert_eq!(
            resolve_find(Some(PhpType::int())).as_deref(),
            Some("Order|null")
        );
        assert_eq!(
            resolve_find(Some(PhpType::named(atom("Collection")))).as_deref(),
            Some("Collection")
        );
        // A class that does not implement the interface still leaves both
        // branches open: a subclass of it may implement it.
        assert_eq!(
            resolve_find(Some(PhpType::named(atom("Order")))).as_deref(),
            Some("Collection|Order|null")
        );
        // An argument whose type says nothing leaves both branches open.
        assert_eq!(
            resolve_find(Some(PhpType::mixed())).as_deref(),
            Some("Collection|Order|null")
        );
    }

    /// Resolve a `tap`-shaped `is null` conditional against an argument of the
    /// given resolved type.
    fn resolve_is_null(text_args: &str, arg_type: Option<PhpType>) -> Option<String> {
        let cond = PhpType::parse("($callback is null ? Proxy : Value)");
        let params = [param_with_default("$callback", "null")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let resolver = |_: &str| arg_type.clone();
        let tpl = TemplateContext {
            defaults: None,
            params: &[],
            bindings: &[],
            arg_type_resolver: Some(&resolver),
            this_type: None,
        };
        resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            text_args,
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string())
    }

    /// An `is null` condition follows the argument's type rather than its mere
    /// presence, so an argument that may be null leaves both branches open.
    #[test]
    fn is_null_condition_follows_the_resolved_argument_type() {
        assert_eq!(resolve_is_null("", None).as_deref(), Some("Proxy"));
        assert_eq!(resolve_is_null("null", None).as_deref(), Some("Proxy"));
        assert_eq!(
            resolve_is_null("$callback", Some(PhpType::named(atom("Closure")))).as_deref(),
            Some("Value")
        );
        assert_eq!(
            resolve_is_null("$name", Some(PhpType::nullable(PhpType::string()))).as_deref(),
            Some("Proxy|Value")
        );
    }

    /// `is not null` takes the branch its negation names, rather than reading
    /// as the un-negated condition.
    #[test]
    fn negated_null_condition_swaps_its_branches() {
        let cond = PhpType::parse("($callback is not null ? Value : Proxy)");
        let params = [param_with_default("$callback", "null")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let tpl = TemplateContext::with_params(&[]);
        let resolve = |text_args: &str| {
            resolve_conditional_with_text_args_and_defaults(
                &cond,
                &params,
                text_args,
                None,
                ConditionalClassContext::default(),
                loader,
                &tpl,
            )
            .map(|t| t.to_string())
        };
        assert_eq!(resolve("null").as_deref(), Some("Proxy"));
        assert_eq!(resolve("$fn").as_deref(), Some("Value"));
    }

    /// A value condition is refuted by an argument of another kind entirely:
    /// `null` is not the string `'array'`, so spatie's `Data::collect()` picks
    /// its collection branch rather than unioning every branch it has.
    #[test]
    fn value_condition_is_refuted_by_another_kind_of_argument() {
        let cond = PhpType::parse("($into is 'array' ? array<Data> : Collection)");
        let params = [param("$items"), param_with_default("$into", "null")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let resolver = |_: &str| Some(PhpType::null());
        let tpl = TemplateContext {
            defaults: None,
            params: &[],
            bindings: &[],
            arg_type_resolver: Some(&resolver),
            this_type: None,
        };
        let resolve = |text_args: &str| {
            resolve_conditional_with_text_args_and_defaults(
                &cond,
                &params,
                text_args,
                None,
                ConditionalClassContext::default(),
                loader,
                &tpl,
            )
            .map(|t| t.to_string())
        };
        assert_eq!(resolve("$rows").as_deref(), Some("Collection"));
        assert_eq!(resolve("$rows, 'array'").as_deref(), Some("array<Data>"));
    }

    /// A condition keyed on a `@template` parameter is decided against the
    /// argument that binds it, not against the first parameter.
    #[test]
    fn template_subject_is_decided_through_its_parameter_binding() {
        let cond = PhpType::parse("(B is true ? Sync : Async)");
        let params = [param("$id"), param_with_default("$wait", "true")];
        let loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = &|_| None;
        let bindings = [(atom("B"), atom("$wait"))];
        let tpl = TemplateContext {
            defaults: None,
            params: &[atom("B")],
            bindings: &bindings,
            arg_type_resolver: None,
            this_type: None,
        };
        let resolve = |text_args: &str| {
            resolve_conditional_with_text_args_and_defaults(
                &cond,
                &params,
                text_args,
                None,
                ConditionalClassContext::default(),
                loader,
                &tpl,
            )
            .map(|t| t.to_string())
        };
        assert_eq!(resolve("$id").as_deref(), Some("Sync"));
        assert_eq!(resolve("$id, false").as_deref(), Some("Async"));
    }
}
