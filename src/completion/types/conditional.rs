/// PHPStan conditional return type resolution.
///
/// This module contains the free functions that resolve PHPStan conditional
/// return type annotations to concrete type strings.  These annotations
/// allow a function's return type to depend on the type or value of a
/// parameter at the call site.
///
/// Two resolution paths are supported:
///
/// - **AST-based** ([`resolve_conditional_with_args`]): used when the call
///   is an assignment (`$var = func(…)`) and we have the parsed
///   `ArgumentList` available.
/// - **Text-based** ([`resolve_conditional_with_text_args`]): used when the
///   call appears inline (e.g. `func(A::class)->method()`) and only the
///   raw argument text between parentheses is available.
/// - **No-args** ([`resolve_conditional_without_args`]): used when no
///   arguments were provided (or none were preserved); walks the
///   conditional tree taking the "null default" branch at each level.
use std::collections::HashMap;
use std::sync::Arc;

use mago_syntax::ast::*;

use crate::php_type::PhpType;
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
}

impl<'a> TemplateContext<'a> {
    pub fn with_params(params: &'a [crate::atom::Atom]) -> Self {
        Self {
            defaults: None,
            params,
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

/// Split a call-expression subject into the call body and any textual
/// arguments.  Handles both `"app()"` → `("app", "")` and
/// `"app(A::class)"` → `("app", "A::class")`.
///
/// For method / static-method calls the arguments are currently not
/// preserved by the extractors, so they always arrive as `""`.
pub(crate) fn split_call_subject(subject: &str) -> Option<(&str, &str)> {
    // Subject must end with ')'.
    let inner = subject.strip_suffix(')')?;
    // Find the matching '(' for the stripped ')' by scanning backwards
    // and tracking balanced parentheses.  This correctly handles nested
    // calls inside the argument list (e.g. `Environment::get(self::country())`).
    let bytes = inner.as_bytes();
    let mut depth: u32 = 0;
    let mut open = None;
    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b')' => depth += 1,
            b'(' => {
                if depth == 0 {
                    open = Some(i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let open = open?;
    let call_body = &inner[..open];
    let args_text = inner[open + 1..].trim();
    if call_body.is_empty() {
        return None;
    }
    Some((call_body, args_text))
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
        calling_class_name,
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
    calling_class_name: Option<&str>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    tpl: &TemplateContext<'_>,
) -> Option<PhpType> {
    match conditional {
        PhpType::Conditional {
            param,
            negated,
            condition,
            then_type,
            else_type,
        } => {
            // Check if the conditional subject is a template parameter
            // with a default value (not a method $parameter).
            let target = param.as_str();
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

            // Find which parameter index corresponds to $param_name
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
            let arg_text = arg_text_owned.as_deref();

            if matches!(condition.as_ref(), PhpType::ClassString(_)) {
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
                // We distinguish template params from real classes by
                // attempting to resolve the bound via the class loader.
                // Template params like `T` won't resolve; real classes
                // like `FormFlowTypeInterface` will.
                //
                // We distinguish template params from concrete class
                // bounds by comparing the bound name to `then_type`.
                // When both match (e.g. condition=`class-string<T>`,
                // then=`T`), the bound is a template parameter and any
                // `::class` literal satisfies the condition — the
                // template is substituted with the concrete class name.
                // When they differ (e.g. condition=`class-string<FormFlowTypeInterface>`,
                // then=`FormFlowInterface`), the bound is a concrete
                // class and a subtype check is required.
                let class_string_bound_name: Option<&str> = match condition.as_ref() {
                    PhpType::ClassString(Some(inner)) => match inner.as_ref() {
                        PhpType::Named(name) => Some(name.as_str()),
                        _ => None,
                    },
                    _ => None,
                };

                // Check if the bound is a template parameter by comparing
                // it to then_type.  `class-string<T> ? T : mixed` is a
                // template pattern; `class-string<X> ? Y : Z` where X≠Y
                // is a concrete bound check.
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
                        Some(ref bound) => {
                            crate::util::is_subtype_of_names(resolved_name, bound, class_loader)
                        }
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
                        calling_class_name,
                        class_loader,
                        tpl,
                    )
                };

                // For variadic class-string parameters, collect class
                // names from ALL arguments at and after param_idx and
                // form a union type (e.g. `A|B` from `A::class, B::class`).
                if is_variadic {
                    let mut class_names: Vec<String> = Vec::new();
                    for arg in args.iter().skip(param_idx) {
                        let trimmed = arg.trim();
                        if let Some(class_name) = extract_class_name_from_text(trimmed) {
                            let class_name = resolve_self_keyword(&class_name, calling_class_name)
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

                        let ty = if class_names.len() == 1 {
                            PhpType::Named(class_names.into_iter().next().unwrap())
                        } else {
                            PhpType::Union(class_names.into_iter().map(PhpType::Named).collect())
                        };
                        return Some(ty);
                    }
                    return resolve_conditional_with_text_args_and_defaults(
                        else_type,
                        params,
                        text_args,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    );
                }

                // Check if the argument text matches `X::class`
                if let Some(arg) = arg_text
                    && let Some(class_name) = extract_class_name_from_text(arg)
                {
                    let class_name =
                        resolve_self_keyword(&class_name, calling_class_name).unwrap_or(class_name);
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

                    return Some(PhpType::Named(resolved));
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

                        let ty = if names.len() == 1 {
                            PhpType::Named(names.into_iter().next().unwrap())
                        } else {
                            PhpType::Union(names.into_iter().map(PhpType::Named).collect())
                        };
                        return Some(ty);
                    }
                }
                // Argument isn't a ::class literal or resolvable variable → try else branch
                resolve_conditional_with_text_args_and_defaults(
                    else_type,
                    params,
                    text_args,
                    var_resolver,
                    calling_class_name,
                    class_loader,
                    tpl,
                )
            } else if condition.as_ref().is_null() {
                // The null (`then`) branch is taken when the argument is
                // absent (the parameter falls back to its null default) or
                // when `null` is passed explicitly. This mirrors the AST
                // path's rule so the same call resolves identically through
                // the inline-text and AST resolution paths.
                let arg_is_null = arg_text.is_none_or(|t| {
                    let t = t.trim();
                    t.is_empty() || t.eq_ignore_ascii_case("null")
                });
                if arg_is_null {
                    // No argument provided or explicitly null → null branch
                    resolve_conditional_with_text_args_and_defaults(
                        then_type,
                        params,
                        text_args,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    )
                } else {
                    // Argument was provided → not null
                    resolve_conditional_with_text_args_and_defaults(
                        else_type,
                        params,
                        text_args,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    )
                }
            } else if let PhpType::Literal(s) = condition.as_ref() {
                // Strip quotes from the literal to get the expected value.
                let expected = crate::util::unquote_php_string(s).unwrap_or(s);

                // Check if the argument is a quoted string literal
                // matching the expected value (e.g. `'foo'` or `"foo"`).
                if let Some(arg) = arg_text {
                    let trimmed = arg.trim();
                    let arg_value = if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
                        || (trimmed.starts_with('"') && trimmed.ends_with('"'))
                    {
                        Some(&trimmed[1..trimmed.len() - 1])
                    } else {
                        None
                    };
                    if arg_value == Some(expected) {
                        return resolve_conditional_with_text_args_and_defaults(
                            then_type,
                            params,
                            text_args,
                            var_resolver,
                            calling_class_name,
                            class_loader,
                            tpl,
                        );
                    }
                }
                // Argument doesn't match the literal → else branch.
                resolve_conditional_with_text_args_and_defaults(
                    else_type,
                    params,
                    text_args,
                    var_resolver,
                    calling_class_name,
                    class_loader,
                    tpl,
                )
            } else {
                // IsType equivalent: can't statically determine most
                // conditions, but we handle scalar types and `array` specially.
                if condition_is_scalar_type(condition.as_ref(), "string")
                    && let Some(arg) = arg_text
                    && arg_is_string_literal(arg)
                {
                    let take_then = !*negated;
                    return resolve_conditional_with_text_args_and_defaults(
                        if take_then { then_type } else { else_type },
                        params,
                        text_args,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    );
                }
                if condition_is_scalar_type(condition.as_ref(), "int")
                    && let Some(arg) = arg_text
                    && arg_is_int_literal(arg)
                {
                    let take_then = !*negated;
                    return resolve_conditional_with_text_args_and_defaults(
                        if take_then { then_type } else { else_type },
                        params,
                        text_args,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    );
                }
                if condition_is_scalar_type(condition.as_ref(), "float")
                    && let Some(arg) = arg_text
                    && arg_is_float_literal(arg)
                {
                    let take_then = !*negated;
                    return resolve_conditional_with_text_args_and_defaults(
                        if take_then { then_type } else { else_type },
                        params,
                        text_args,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    );
                }
                // Check if the condition mentions `array` and the
                // argument is an array literal (starts with `[`).
                if condition_includes_array(condition.as_ref())
                    && let Some(arg) = arg_text
                    && arg.trim_start().starts_with('[')
                {
                    return resolve_conditional_with_text_args_and_defaults(
                        then_type,
                        params,
                        text_args,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    );
                }
                // Can't statically determine; fall through to else.
                resolve_conditional_with_text_args_and_defaults(
                    else_type,
                    params,
                    text_args,
                    var_resolver,
                    calling_class_name,
                    class_loader,
                    tpl,
                )
            }
        }
        // Non-Conditional PhpType variant (replaces Concrete)
        other => {
            if other.is_uninformative_return() {
                return None;
            }
            Some(other.clone())
        }
    }
}

/// Checks whether a condition is a specific scalar type name.
fn condition_is_scalar_type(condition: &PhpType, type_name: &str) -> bool {
    match condition {
        PhpType::Named(n) => n == type_name,
        _ => false,
    }
}

/// Checks whether the argument text is a quoted string literal.
fn arg_is_string_literal(arg: &str) -> bool {
    let t = arg.trim();
    (t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"'))
}

/// Checks whether the argument text is an integer literal.
fn arg_is_int_literal(arg: &str) -> bool {
    let t = arg.trim();
    let t = t.strip_prefix('-').unwrap_or(t);
    !t.is_empty() && t.chars().all(|c| c.is_ascii_digit())
}

/// Checks whether the argument text is a float literal.
fn arg_is_float_literal(arg: &str) -> bool {
    let t = arg.trim();
    let t = t.strip_prefix('-').unwrap_or(t);
    t.contains('.') && t.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Check whether a condition type includes `array` as one of its
/// union members.
fn condition_includes_array(condition: &PhpType) -> bool {
    if condition.is_array_like() {
        return true;
    }
    match condition {
        PhpType::Union(members) => members.iter().any(condition_includes_array),
        _ => false,
    }
}

/// Split a textual argument list by commas, respecting nested parentheses
/// so that `"foo(a, b), c"` splits into `["foo(a, b)", "c"]`.
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
            '(' | '[' if !in_single_quote && !in_double_quote => {
                depth += 1;
            }
            ')' | ']' if !in_single_quote && !in_double_quote => {
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

/// Extract a class name from textual `X::class` syntax.
///
/// Matches strings like `"SessionManager::class"`, `"\\App\\Foo::class"`,
/// returning the class name portion (`"SessionManager"`, `"\\App\\Foo"`).
/// If `name` is `"self"`, `"static"`, or `"parent"`, substitute the
/// calling-site class name so that the resolved type is concrete rather
/// than relative to the method-owner class.
fn resolve_self_keyword(name: &str, calling_class_name: Option<&str>) -> Option<String> {
    match name {
        "self" | "static" | "parent" => calling_class_name.map(|n| n.to_string()),
        _ => None,
    }
}

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

/// Resolve a PHPStan conditional return type given AST-level call-site
/// arguments.
///
/// Walks the conditional tree and matches argument expressions against
/// the conditions:
///   - `class-string<T>`: checks if the positional argument is `X::class`
///     and returns `"X"`.
///   - `is null`: satisfied when no argument is provided (parameter has
///     a null default).
///   - `is SomeType`: not statically resolvable from AST; falls through
///     to the else branch.
pub(crate) fn resolve_conditional_with_args<'b>(
    conditional: &PhpType,
    params: &[ParameterInfo],
    argument_list: &ArgumentList<'b>,
    var_resolver: VarClassStringResolver<'_>,
    calling_class_name: Option<&str>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    tpl: &TemplateContext<'_>,
) -> Option<PhpType> {
    resolve_conditional_with_args_and_defaults(
        conditional,
        params,
        argument_list,
        var_resolver,
        calling_class_name,
        class_loader,
        tpl,
    )
}

/// Like [`resolve_conditional_with_args`], but also accepts optional
/// template parameter defaults from the owning class.
pub fn resolve_conditional_with_args_and_defaults<'b>(
    conditional: &PhpType,
    params: &[ParameterInfo],
    argument_list: &ArgumentList<'b>,
    var_resolver: VarClassStringResolver<'_>,
    calling_class_name: Option<&str>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    tpl: &TemplateContext<'_>,
) -> Option<PhpType> {
    match conditional {
        PhpType::Conditional {
            param,
            negated,
            condition,
            then_type,
            else_type,
        } => {
            // Check if the conditional subject is a template parameter
            // with a default value (not a method $parameter).
            let target = param.as_str();
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

            // Find which parameter index corresponds to param
            let param_idx = params.iter().position(|p| p.name == target).unwrap_or(0);

            // Bind arguments to parameters following PHP's rules (positional
            // fill in order, named fill by name) so a named argument in an
            // earlier slot does not shadow the parameter the conditional
            // refers to.
            let arg_expr: Option<&Expression<'b>> =
                crate::call_args::bind_args_to_params(params, argument_list)
                    .get(param_idx)
                    .copied()
                    .flatten();

            if matches!(condition.as_ref(), PhpType::ClassString(_)) {
                // Extract the bound from `class-string<Bound>` and determine
                // whether it is a template parameter or a concrete class.
                // Mirrors the logic in resolve_conditional_with_text_args_and_defaults.
                let class_string_bound_name: Option<&str> = match condition.as_ref() {
                    PhpType::ClassString(Some(inner)) => match inner.as_ref() {
                        PhpType::Named(name) => Some(name.as_str()),
                        _ => None,
                    },
                    _ => None,
                };

                let bound_is_template = class_string_bound_name
                    .is_some_and(|name| tpl.params.iter().any(|tp| tp.as_str() == name));

                let concrete_class_string_bound: Option<String> = if bound_is_template {
                    None
                } else {
                    class_string_bound_name.map(|name| {
                        let resolved = crate::util::resolve_name_via_loader(name, class_loader);
                        if class_loader(&resolved).is_some() {
                            resolved
                        } else {
                            "!!unresolvable_bound!!".to_string()
                        }
                    })
                };

                let satisfies_bound = |resolved_name: &str| -> bool {
                    match concrete_class_string_bound {
                        None => true,
                        Some(ref bound) => {
                            crate::util::is_subtype_of_names(resolved_name, bound, class_loader)
                        }
                    }
                };

                let choose_branch = |bound_satisfied: bool| -> Option<PhpType> {
                    let take_then = bound_satisfied ^ *negated;
                    resolve_conditional_with_args_and_defaults(
                        if take_then { then_type } else { else_type },
                        params,
                        argument_list,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    )
                };

                // Check if the argument is `X::class`
                if let Some(class_name) = arg_expr.and_then(extract_class_string_from_expr) {
                    let class_name =
                        resolve_self_keyword(&class_name, calling_class_name).unwrap_or(class_name);
                    let resolved = crate::util::resolve_name_via_loader(&class_name, class_loader);

                    if concrete_class_string_bound.is_some() {
                        return choose_branch(satisfies_bound(&resolved));
                    }

                    return Some(PhpType::Named(resolved));
                }
                // Check if the argument is a variable holding class-string
                // value(s) (e.g. from a match expression).
                if let Some(Expression::Variable(Variable::Direct(dv))) = arg_expr
                    && let Some(resolver) = var_resolver
                {
                    let names = resolver(crate::atom::bytes_to_str(dv.name));
                    if !names.is_empty() {
                        let names: Vec<String> = names
                            .into_iter()
                            .map(|n| crate::util::resolve_name_via_loader(&n, class_loader))
                            .collect();

                        if concrete_class_string_bound.is_some() {
                            let all_satisfy = names.iter().all(|n| satisfies_bound(n));
                            return choose_branch(all_satisfy);
                        }

                        let ty = if names.len() == 1 {
                            PhpType::Named(names.into_iter().next().unwrap())
                        } else {
                            PhpType::Union(names.into_iter().map(PhpType::Named).collect())
                        };
                        return Some(ty);
                    }
                }
                // Argument isn't a ::class literal or resolvable variable → try else branch
                resolve_conditional_with_args_and_defaults(
                    else_type,
                    params,
                    argument_list,
                    var_resolver,
                    calling_class_name,
                    class_loader,
                    tpl,
                )
            } else if condition.as_ref().is_null() {
                // The null (`then`) branch is taken when the argument is
                // absent (the parameter falls back to its null default) or
                // when `null` is passed explicitly.
                let arg_is_null = match arg_expr {
                    None => true,
                    Some(expr) => matches!(expr, Expression::Literal(Literal::Null(_))),
                };
                if arg_is_null {
                    // No argument provided or explicitly null → null branch
                    resolve_conditional_with_args_and_defaults(
                        then_type,
                        params,
                        argument_list,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    )
                } else {
                    // Argument was provided and not null → else branch
                    resolve_conditional_with_args_and_defaults(
                        else_type,
                        params,
                        argument_list,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    )
                }
            } else if let PhpType::Literal(s) = condition.as_ref() {
                // Strip quotes from the literal to get the expected value.
                let expected = crate::util::unquote_php_string(s).unwrap_or(s);

                // Check if the argument is a string literal matching
                // the expected value.
                let matches = match arg_expr {
                    Some(Expression::Literal(Literal::String(lit_str))) => {
                        // `value` is the unquoted content; fall back
                        // to stripping quotes from `raw`.
                        let inner = lit_str
                            .value
                            .map(|v| crate::atom::bytes_to_str(v).to_string())
                            .unwrap_or_else(|| {
                                crate::util::unquote_php_string(crate::atom::bytes_to_str(
                                    lit_str.raw,
                                ))
                                .unwrap_or(crate::atom::bytes_to_str(lit_str.raw))
                                .to_string()
                            });
                        inner == *expected
                    }
                    _ => false,
                };
                if matches {
                    resolve_conditional_with_args_and_defaults(
                        then_type,
                        params,
                        argument_list,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    )
                } else {
                    resolve_conditional_with_args_and_defaults(
                        else_type,
                        params,
                        argument_list,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    )
                }
            } else {
                // IsType equivalent: check scalar types from AST literals.
                if condition_is_scalar_type(condition.as_ref(), "string")
                    && matches!(arg_expr, Some(Expression::Literal(Literal::String(_))))
                {
                    let take_then = !*negated;
                    return resolve_conditional_with_args_and_defaults(
                        if take_then { then_type } else { else_type },
                        params,
                        argument_list,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    );
                }
                if condition_is_scalar_type(condition.as_ref(), "int")
                    && matches!(arg_expr, Some(Expression::Literal(Literal::Integer(_))))
                {
                    let take_then = !*negated;
                    return resolve_conditional_with_args_and_defaults(
                        if take_then { then_type } else { else_type },
                        params,
                        argument_list,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    );
                }
                if condition_is_scalar_type(condition.as_ref(), "float")
                    && matches!(arg_expr, Some(Expression::Literal(Literal::Float(_))))
                {
                    let take_then = !*negated;
                    return resolve_conditional_with_args_and_defaults(
                        if take_then { then_type } else { else_type },
                        params,
                        argument_list,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    );
                }
                // Check if the condition mentions `array` and the
                // argument is an array literal (`[...]`).
                if condition_includes_array(condition.as_ref())
                    && let Some(Expression::Array(_)) = arg_expr
                {
                    return resolve_conditional_with_args_and_defaults(
                        then_type,
                        params,
                        argument_list,
                        var_resolver,
                        calling_class_name,
                        class_loader,
                        tpl,
                    );
                }
                // We can't statically determine the type of an
                // arbitrary expression; fall through to else.
                resolve_conditional_with_args_and_defaults(
                    else_type,
                    params,
                    argument_list,
                    var_resolver,
                    calling_class_name,
                    class_loader,
                    tpl,
                )
            }
        }
        // Non-Conditional PhpType variant (replaces Concrete)
        other => {
            if other.is_uninformative_return() {
                return None;
            }
            Some(other.clone())
        }
    }
}

/// Resolve a conditional return type **without** call-site arguments
/// (text-based path).  Walks the tree taking the "no argument / null
/// default" branch at each level.
pub(crate) fn resolve_conditional_without_args(
    conditional: &PhpType,
    params: &[ParameterInfo],
) -> Option<PhpType> {
    resolve_conditional_without_args_and_defaults(conditional, params, None)
}

/// Like [`resolve_conditional_without_args`], but also accepts optional
/// template parameter defaults from the owning class.
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
) -> Option<PhpType> {
    match conditional {
        PhpType::Conditional {
            param,
            negated,
            condition,
            then_type,
            else_type,
        } => {
            // Check if the conditional subject is a template parameter
            // with a default value (not a method $parameter).
            let target = param.as_str();
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

            // Without arguments we check whether the parameter has a
            // null default — if so, the `is null` branch is taken.
            let param_info = params.iter().find(|p| p.name == target);
            let has_null_default = param_info.is_some_and(|p| !p.is_required);

            if condition.as_ref().is_null() && has_null_default {
                resolve_conditional_without_args_and_defaults(then_type, params, template_defaults)
            } else {
                // Try else branch
                resolve_conditional_without_args_and_defaults(else_type, params, template_defaults)
            }
        }
        // Non-Conditional PhpType variant (replaces Concrete)
        other => {
            if other.is_uninformative_return() {
                return None;
            }
            Some(other.clone())
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
    } else if let PhpType::Literal(s) = condition {
        let expected = crate::util::unquote_php_string(s).unwrap_or(s);
        match default_value {
            PhpType::Literal(dv) => dv == expected,
            PhpType::Named(dv) => dv == expected,
            _ => false,
        }
    } else if let PhpType::Named(s) = condition {
        match default_value {
            PhpType::Named(dv) => dv == s,
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
