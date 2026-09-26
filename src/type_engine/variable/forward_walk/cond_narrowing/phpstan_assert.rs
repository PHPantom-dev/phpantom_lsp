use super::*;

/// Apply `@phpstan-assert-if-true` / `@phpstan-assert-if-false` narrowing
/// from a function or static/instance method call used as a condition.
///
/// When `inverted` is false we are in the truthy branch (then-body or
/// while-body).  When `inverted` is true we are in the else branch or
/// applying guard-clause inverse narrowing.
pub(crate) fn apply_phpstan_assert_condition_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inverted: bool,
) {
    use crate::types::AssertionKind;

    // `$name === 'static' && $scope->isInClass()` proves both operands in
    // the truthy branch, so each one's own tags apply.  Written as one
    // expression the chain is not a call at all, and the extractors below
    // would find nothing to read.  The inverse path needs no equivalent:
    // `apply_condition_narrowing_inverse` does the De Morgan
    // decomposition and hands this function a single operand.
    if !inverted {
        let operands = collect_and_chain_operands(condition);
        if operands.len() > 1 {
            for operand in &operands {
                apply_phpstan_assert_condition_narrowing(operand, scope, ctx, inverted);
            }
            return;
        }
    }

    // Unwrap parentheses and detect negation (`!func($var)`).
    let (func_call_expr, condition_negated) = narrowing::unwrap_condition_negation(condition);

    let call = match func_call_expr {
        Expression::Call(c) => c,
        _ => return,
    };

    // Determine whether the function returned true in this branch.
    let function_returned_true = !(inverted ^ condition_negated);

    let scope_resolver = scope.snapshot_resolver();

    // Try to extract assertion info from function calls and static method calls.
    match call {
        Call::Function(func_call) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let func_name_offset = func_call.function.span().start.offset;
            let func_info = match ctx.loaders.function_loader {
                Some(fl) => match fl(&func_name, func_name_offset) {
                    Some(fi) => fi,
                    None => return,
                },
                None => return,
            };
            if func_info.type_assertions.is_empty() {
                return;
            }
            for assertion in &func_info.type_assertions {
                let applies_positively = match assertion.kind {
                    AssertionKind::IfTrue => function_returned_true,
                    AssertionKind::IfFalse => !function_returned_true,
                    AssertionKind::Always => continue,
                };
                // The branch this tag does not name is reached by negating
                // what it promises, which only holds for the subtype form:
                // `-if-true Foo` failing means the value was not a `Foo`.
                // The equality form promises a comparison instead, and a
                // failed comparison rules nothing out, so it stays a one-way
                // implication.  Laravel's `filled()` carries
                // `@phpstan-assert-if-false !=numeric|bool`, and inverting
                // that made every filled value look like `numeric|bool`.
                if !applies_positively && assertion.is_equality {
                    continue;
                }
                let arg_vars = narrowing::find_assertion_arg_variables(
                    &func_call.argument_list,
                    &assertion.param_name,
                    &func_info.parameters,
                );
                if arg_vars.is_empty() {
                    continue;
                }
                let should_exclude = assertion.negated ^ !applies_positively;
                let asserted_type = narrowing::bind_call_templates(
                    &assertion.asserted_type,
                    &func_info.template_params,
                    &func_info.template_bindings,
                    &func_info.parameters,
                    &func_call.argument_list,
                    &build_var_ctx("", ctx, &scope_resolver),
                );
                for arg_var in &arg_vars {
                    apply_assertion_to_key(
                        arg_var,
                        &asserted_type,
                        should_exclude,
                        scope,
                        ctx,
                        &scope_resolver,
                    );
                }
            }
        }
        Call::StaticMethod(static_call) => {
            let method_name = match &static_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            // Resolve the receiver to a class, handling `self`, `static`,
            // `parent`, and subclass names.
            let receiver = match static_call.class {
                Expression::Identifier(ident) => {
                    let name = bytes_to_str(ident.value());
                    let fqn = crate::util::resolve_name_via_loader(name, ctx.class_loader);
                    (ctx.class_loader)(&fqn).or_else(|| (ctx.class_loader)(name))
                }
                Expression::Self_(_) | Expression::Static(_) => {
                    (ctx.class_loader)(&ctx.current_class.name)
                }
                Expression::Parent(_) => match ctx.current_class.parent_class.as_ref() {
                    Some(parent) => (ctx.class_loader)(parent),
                    None => return,
                },
                _ => return,
            };
            let class_info = match receiver {
                Some(ci) => ci,
                None => return,
            };
            // Search the trait/parent chain so assertions declared on an
            // ancestor (e.g. PHPUnit's `Assert`) are found.  Uses raw class
            // loads only, avoiding a full merge that would poison the shared
            // resolved-class cache mid-walk.
            let (method, declaring_fqn) = match narrowing::find_assertion_method_in_chain(
                &class_info,
                &method_name,
                ctx.class_loader,
                &mut Vec::new(),
                0,
            ) {
                Some(found) => found,
                None => return,
            };
            let declaring_namespace = namespace_of_fqn(&declaring_fqn);
            for assertion in &method.type_assertions {
                let applies_positively = match assertion.kind {
                    AssertionKind::IfTrue => function_returned_true,
                    AssertionKind::IfFalse => !function_returned_true,
                    AssertionKind::Always => continue,
                };
                if !applies_positively && assertion.is_equality {
                    continue;
                }
                let arg_vars = narrowing::find_assertion_arg_variables(
                    &static_call.argument_list,
                    &assertion.param_name,
                    &method.parameters,
                );
                if arg_vars.is_empty() {
                    continue;
                }
                let should_exclude = assertion.negated ^ !applies_positively;
                let asserted_type = narrowing::bind_call_templates(
                    &assertion.asserted_type,
                    &method.template_params,
                    &method.template_bindings,
                    &method.parameters,
                    &static_call.argument_list,
                    &build_var_ctx("", ctx, &scope_resolver),
                );
                // Resolve `self`/`static`/`$this` in the asserted type
                // against the declaring class, not the enclosing class.
                let resolved_assert_type = if asserted_type.contains_self_ref() {
                    asserted_type.replace_self(&class_info.fqn())
                } else {
                    qualify_assertion_type(&asserted_type, declaring_namespace.as_deref(), ctx)
                };
                for arg_var in &arg_vars {
                    apply_assertion_to_key(
                        arg_var,
                        &resolved_assert_type,
                        should_exclude,
                        scope,
                        ctx,
                        &scope_resolver,
                    );
                }
            }
        }
        Call::Method(method_call) => {
            // Instance method: `$var->method()` with `@phpstan-assert-if-true Type $this`.
            // The receiver is any subject the scope can key, not just a
            // bare local: `$this->scope->isInClass()` is the same promise
            // about the same value, and the guard is written that way
            // wherever the scope is held in a property.
            let Some(receiver_var) = narrowing::expr_to_subject_key(method_call.object) else {
                return;
            };
            let method_name = match &method_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            // A compound receiver is not a tracked local, so its type has
            // to be brought into the scope before it can be read.
            seed_synthetic_key_if_needed(&receiver_var, scope, ctx);
            // Resolve the receiver's type to find the method's assertions.
            let receiver_types = scope.get(&receiver_var).to_vec();
            if receiver_types.is_empty() {
                return;
            }
            // Collect assertions from all candidate classes.
            let mut to_apply: Vec<(crate::php_type::PhpType, bool, String)> = Vec::new();
            for rt in &receiver_types {
                // An intersection-typed receiver carries one entry per
                // member, so the class each entry names is looked up
                // rather than the joined `A&B` text, which is not a class.
                let receiver = match resolve_receiver_class(rt, ctx) {
                    Some(ci) => ci,
                    None => {
                        continue;
                    }
                };
                // Search the trait/parent chain for the method's assertions
                // using raw class loads only (a full merge would poison the
                // shared resolved-class cache mid-walk).
                let (method, declaring_fqn) = match narrowing::find_assertion_method_in_chain(
                    &receiver,
                    &method_name,
                    ctx.class_loader,
                    &mut Vec::new(),
                    0,
                ) {
                    Some(found) => found,
                    None => continue,
                };
                let declaring_namespace = namespace_of_fqn(&declaring_fqn);
                for assertion in &method.type_assertions {
                    let applies_positively = match assertion.kind {
                        AssertionKind::IfTrue => function_returned_true,
                        AssertionKind::IfFalse => !function_returned_true,
                        AssertionKind::Always => continue,
                    };
                    if !applies_positively && assertion.is_equality {
                        continue;
                    }
                    let should_exclude = assertion.negated ^ !applies_positively;
                    let asserted_type = narrowing::bind_call_templates(
                        &assertion.asserted_type,
                        &method.template_params,
                        &method.template_bindings,
                        &method.parameters,
                        &method_call.argument_list,
                        &build_var_ctx("", ctx, &scope_resolver),
                    );
                    // Resolve `self`/`static`/`$this` in the asserted type
                    // against the *declaring* class (e.g. `Decimal`), not the
                    // enclosing class (e.g. `Monetary`).  Without this,
                    // `@phpstan-assert-if-false self<true> $this` on
                    // `Decimal::isZero()` would narrow $denominator to
                    // `Monetary` instead of `Decimal`.
                    let resolved_type = if asserted_type.contains_self_ref() {
                        asserted_type.replace_self(&receiver.fqn())
                    } else {
                        qualify_assertion_type(&asserted_type, declaring_namespace.as_deref(), ctx)
                    };
                    if assertion.param_name == "$this" {
                        // Narrows the receiver variable itself.
                        to_apply.push((resolved_type, should_exclude, receiver_var.clone()));
                    } else if let Some(member) = assertion.param_name.strip_prefix("$this->") {
                        // `@phpstan-assert-if-true !null
                        // $this->getTraitReflection()` on `isInTrait()` is a
                        // promise about a member read off the *receiver*, so
                        // at the call site the subject is that same read
                        // through the variable the call was written on.
                        to_apply.push((
                            resolved_type,
                            should_exclude,
                            format!("{receiver_var}->{member}"),
                        ));
                    } else {
                        for arg_var in narrowing::find_assertion_arg_variables(
                            &method_call.argument_list,
                            &assertion.param_name,
                            &method.parameters,
                        ) {
                            to_apply.push((resolved_type.clone(), should_exclude, arg_var));
                        }
                    }
                }
            }
            for (asserted_type, should_exclude, target_var) in to_apply {
                apply_assertion_to_key(
                    &target_var,
                    &asserted_type,
                    should_exclude,
                    scope,
                    ctx,
                    &scope_resolver,
                );
            }
        }
        _ => {}
    }
}

/// The class a resolved receiver entry stands for, for looking up the
/// method whose docblock carries the assertion.
///
/// The entry's own `class_info` is the authority when it has one.  An
/// entry that only carries a type string is looked up by that text,
/// except that an intersection (`A&B`) is not a class name — each member
/// is tried in turn, since the assertion may be declared on any of them.
fn resolve_receiver_class(
    rt: &ResolvedType,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Arc<crate::types::ClassInfo>> {
    if let Some(ci) = &rt.class_info {
        return Some(Arc::clone(ci));
    }
    if let TypeKind::Intersection(members) = rt.type_string.kind() {
        return members
            .iter()
            .find_map(|member| (ctx.class_loader)(&member.to_string()));
    }
    (ctx.class_loader)(&rt.type_string.to_string())
}
