/// Template substitution for method-level `@template` parameters: builds
/// a substitution map from call-site argument texts and resolves closure
/// return types against bound template parameters.
use std::collections::HashMap;

use crate::Backend;
use crate::atom::{Atom, AtomMap, atom};
use crate::class_lookup::is_self_or_static;
use crate::php_type::{PhpType, TypeKind};
use crate::types::*;

use crate::type_engine::resolver::{Loaders, ResolutionCtx, with_isolated_chain_cache};
use crate::type_engine::variable::forward_walk::{
    ForwardWalkCtx, ScopeState, suspend_diagnostic_scope, suspend_return_edges, walk_body_forward,
};
use mago_syntax::cst::{Expression, Statement};

use super::return_types::{
    literal_arg_type, resolve_call_return_hint, resolve_cast_type, resolve_chain_declared_return,
    resolve_expression_to_type, resolve_literal_type, resolve_operator_type,
    resolve_static_access_type,
};

impl Backend {
    /// Build a template substitution map for a method-level `@template` call.
    ///
    /// Finds the method on the class (or inherited), checks for template
    /// params and bindings, resolves argument types from the pre-split
    /// `arg_texts` slice using the call resolution context, and returns a
    /// `HashMap` mapping template parameter names to their resolved
    /// concrete types.
    ///
    /// Callers with an AST `ArgumentList` should extract per-argument text
    /// via [`extract_arg_texts_from_ast`] and convert to `&[&str]`.
    /// Callers with only raw text should use [`split_text_args`] first.
    ///
    /// Returns an empty map if the method has no template params, no
    /// bindings, or if argument types cannot be resolved.
    pub(crate) fn build_method_template_subs(
        class_info: &ClassInfo,
        method_name: &str,
        arg_texts: &[&str],
        ctx: &ResolutionCtx<'_>,
    ) -> HashMap<String, PhpType> {
        // Find the method — first on the class directly, then via inheritance.
        // An override without a docblock of its own inherits the ancestor's
        // `@template` tags in the merge, so a direct method that declares
        // none is looked up again there.
        let own = class_info.get_method(method_name);
        let may_inherit_templates = class_info.parent_class.is_some()
            || !class_info.interfaces.is_empty()
            || !class_info.used_traits.is_empty();
        let method = match own {
            Some(m) if !m.template_params.is_empty() || !may_inherit_templates => Some(m.clone()),
            _ => {
                let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                    class_info,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                );
                merged
                    .get_method(method_name)
                    .cloned()
                    .or_else(|| own.cloned())
            }
        };

        let method = match method {
            Some(m) if !m.template_params.is_empty() => m,
            _ => return HashMap::new(),
        };

        let callee = super::TemplateCallee {
            parameters: &method.parameters,
            template_bindings: &method.template_bindings,
            template_param_bounds: &method.template_param_bounds,
        };
        let mut subs = super::bind_template_args(&callee, arg_texts, None, ctx);

        finish_template_subs(
            &mut subs,
            &method.template_params,
            &method.template_param_bounds,
            &method.template_param_defaults,
            method.return_type.as_ref(),
            ctx,
        );

        subs
    }

    /// When a `GenericWrapper` extraction fails and the argument is a
    /// closure, try to infer the template param from the closure's
    /// return type (explicit annotation or yield inference).
    ///
    /// This handles union param types like
    /// `iterable<TKey, TValue>|(Closure(): Generator<TKey, TValue, mixed, void>)`
    /// where the classifier picked `GenericWrapper("iterable", pos)` but
    /// the arg is actually a closure.  We look for a `Callable` variant
    /// in the param hint union whose return type contains the template
    /// param, infer the closure's return type (via annotation or yields),
    /// and extract the generic arg at `tpl_position`.
    pub(crate) fn try_closure_return_type_for_template(
        arg_text: &str,
        tpl_name: &str,
        tpl_position: usize,
        param_hint: Option<&PhpType>,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        // Check that the param hint union contains a Callable variant
        // whose return type is a Generic containing the template param.
        let callable_return_type =
            Self::find_callable_return_generic_in_hint(param_hint?, tpl_name)?;

        let trimmed = arg_text.trim();

        // Infer the closure's effective return type.
        let closure_ret = if let Some(ret) = Self::infer_closure_return_type(arg_text, ctx) {
            ret
        } else {
            // Variable/chain argument like `$closure`: resolve the argument
            // type and, when it is a typed Closure(), unwrap its return type.
            let resolved = Self::resolve_arg_text_to_type(trimmed, ctx)?;
            match resolved.callable_return_type() {
                Some(ret) if resolved.is_closure() => ret.clone(),
                _ => return None,
            }
        };

        // Match the inferred return type against the expected generic
        // shape.  E.g., if callable returns `Generator<TKey, TValue, ...>`
        // and we inferred `Generator<int, string, mixed, mixed>`, extract
        // the arg at tpl_position.
        if let (TypeKind::Generic(expected), TypeKind::Generic(inferred)) =
            (callable_return_type.kind(), closure_ret.kind())
        {
            let exp_short = crate::util::short_name(&expected.name);
            let inf_short = crate::util::short_name(&inferred.name);
            if exp_short.eq_ignore_ascii_case(inf_short) {
                return inferred.args.get(tpl_position).cloned();
            }
        }

        // If the return type itself IS the template param (Closure(): T),
        // return the whole inferred type.
        if callable_return_type.is_named(tpl_name) {
            return Some(closure_ret);
        }

        None
    }

    /// Search a (possibly union) param type for a `Callable` variant whose
    /// return type is a Generic containing the given template param name.
    /// Returns that Generic return type if found.
    fn find_callable_return_generic_in_hint(hint: &PhpType, tpl_name: &str) -> Option<PhpType> {
        match hint.kind() {
            TypeKind::Union(members) => {
                for m in members {
                    if let Some(found) = Self::find_callable_return_generic_in_hint(m, tpl_name) {
                        return Some(found);
                    }
                }
                None
            }
            TypeKind::Nullable(inner) => {
                Self::find_callable_return_generic_in_hint(inner, tpl_name)
            }
            TypeKind::Callable(c) => {
                if let Some(rt) = &c.return_type
                    && crate::type_engine::variable::rhs_resolution::type_contains_name(
                        rt, tpl_name,
                    )
                {
                    return Some(rt.clone());
                }
                None
            }
            _ => None,
        }
    }

    /// Resolve an argument text string to a type name.
    ///
    /// Handles common patterns:
    /// - `ClassName::class` → `ClassName`
    /// - `new ClassName(…)` → `ClassName`
    /// - `$this` / `self` / `static` → current class name
    /// - `$this->prop` → property type
    /// - `$var` → variable type via assignment scanning
    /// - `"hello"` / `'world'` → `string`
    /// - `42` / `-1` → `int`
    /// - `3.14` → `float`
    /// - `true` / `false` → `bool`
    /// - `null` → `null`
    /// - `[…]` → `array`
    /// - `EnumClass::Case` → `EnumClass`
    /// - `ClassName::CONSTANT` → constant's declared type
    pub(crate) fn resolve_arg_text_to_type(
        arg_text: &str,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        let trimmed = arg_text.trim();

        // ── Literal values ──────────────────────────────────────
        if let Some(ty) = resolve_literal_type(trimmed) {
            return Some(ty);
        }

        // ── Casts ───────────────────────────────────────────────
        // `(string) $customer->id` is a `string` whatever the property
        // resolves to, so the cast answers before the operand is read.
        if let Some(ty) = resolve_cast_type(trimmed) {
            return Some(ty);
        }

        // ClassName::class → class-string<ClassName>
        //
        // The magic `::class` constant yields the fully-qualified class
        // name as a `class-string<T>`, mirroring the general expression
        // resolver (`resolve_rhs_property_access`).  Keeping the wrapper
        // here means a template param bound directly from a `::class`
        // argument (`@param T $x`) infers `class-string<T>` rather than
        // the bare class, matching the argument's actual type.  The
        // `class-string<T>` unwrapping paths (ClassStringInner and the
        // class-string generic wrapper) strip the wrapper back off when
        // they need the bare class.
        if let Some(name) = trimmed.strip_suffix("::class")
            && !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
        {
            // self::class / static::class / parent::class resolve relative
            // to the class at the call site.
            let class_named = if is_self_or_static(name) {
                ctx.current_class.map(|c| PhpType::named(c.fqn()))
            } else if name.eq_ignore_ascii_case("parent") {
                ctx.current_class
                    .and_then(|c| c.parent_class.as_ref())
                    .map(|p| PhpType::named(atom(p.as_ref())))
            } else {
                let resolved_name = if let Some(cls) = (ctx.class_loader)(name) {
                    cls.fqn().to_string()
                } else {
                    name.to_string()
                };
                Some(PhpType::named(atom(&resolved_name)))
            };
            return class_named.map(|n| PhpType::class_string(Some(n)));
        }

        // Global constant access: `PHP_VERSION`, `PHP_EOL`, etc.
        //
        // A bare identifier that isn't a keyword, a `::class`/enum/const
        // access (handled above and below), or any other special form is a
        // global constant reference.  Ask the attached `Backend` (the same
        // source `VarResolutionCtx`'s constant loader draws from) and infer
        // the type from its value, mirroring the
        // `Expression::ConstantAccess` branch the AST-based RHS resolver
        // already has for a plain `$x = PHP_EOL;` assignment.  This path
        // takes expression *text*, with no offset to resolve a namespaced
        // name against, so only the name as written is tried.
        if !trimmed.is_empty()
            && !trimmed.starts_with('$')
            && !trimmed.contains("::")
            && !trimmed.contains("->")
            && !trimmed.contains('(')
            && !trimmed.contains('[')
            && trimmed
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
            && !is_self_or_static(trimmed)
            && !trimmed.eq_ignore_ascii_case("parent")
            && let Some(backend) = ctx.backend
            && let Some(ty) = match backend.lookup_global_constant(trimmed) {
                None => None,
                Some(Some(value)) => {
                    crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(
                        &value,
                    )
                    .or_else(|| super::folded_global_constant_type(trimmed, &value, ctx))
                }
                Some(None) => {
                    crate::hover::constants::unversioned_php_version_constant_type(trimmed)
                }
            }
        {
            return Some(ty);
        }

        // When the expression contains a `->` chain (e.g.
        // `Country::DK->value`, `new Decimal($x)->toFixed(2)`),
        // skip the static-access and new-expression shortcuts —
        // they would match the prefix and ignore the chain.
        // Let `resolve_expression_to_type` handle the full chain.
        let has_arrow_chain = trimmed.contains("->");

        // ClassName::Member — enum cases and class constants.
        // Enum cases resolve to the enum type; class constants
        // resolve to the constant's declared type hint.  Of the `->`
        // chains, it answers only a case's `->name` / `->value`.
        if let Some(ty) = resolve_static_access_type(trimmed, ctx) {
            return Some(ty);
        }

        // new ClassName(…) → ClassName
        if !has_arrow_chain
            && let Some(class_name) =
                crate::completion::source::helpers::extract_new_expression_class(trimmed)
        {
            let resolved_name = if let Some(cls) = (ctx.class_loader)(&class_name) {
                cls.fqn().to_string()
            } else {
                class_name
            };
            return Some(PhpType::named(atom(&resolved_name)));
        }

        // $this / self / static → current class (or preserve the keyword when asked)
        if is_self_or_static(trimmed) {
            return ctx.current_class.map(|c| {
                if ctx.preserve_static {
                    match trimmed {
                        "static" => PhpType::static_type(c.fqn()),
                        "$this" => PhpType::this_type(c.fqn()),
                        _ => PhpType::named(c.fqn()),
                    }
                } else {
                    PhpType::named(atom(c.name.as_ref()))
                }
            });
        }

        // When preserve_static is set, try resolving method chains by
        // looking up the last method's declared return type directly.
        // This preserves $this/static and generics that the general
        // expression resolver would flatten to a bare class name.
        if ctx.preserve_static
            && trimmed.contains("->")
            && let Some(ty) = resolve_chain_declared_return(trimmed, ctx)
        {
            return Some(ty);
        }

        // Operators whose result is decided from their operands rather
        // than from source-text shape alone (`$a . $b`, `$body ?: ''`).
        // Checked before the general fallback because `SubjectExpr::parse`
        // has no notion of these operators and would otherwise misread the
        // whole expression as a single bare variable or class name.
        if let Some(ty) = resolve_operator_type(trimmed, ctx) {
            return Some(ty);
        }

        // General expression fallback: parse the argument text as a
        // SubjectExpr and try to resolve it to a type.  This handles
        // $var, $var->prop, $this->prop, $var->method(), method
        // chains, and any other expression pattern.
        if let Some(ty) = resolve_expression_to_type(trimmed, ctx) {
            return Some(ty);
        }

        // A call whose return type names no class (`$review->getRating()`
        // returning `int`) leaves the general resolver empty, because it
        // reports classes.  Read the call's return type from the same
        // resolution path so a template can still bind from it.
        if let Some(ty) = resolve_call_return_hint(trimmed, ctx) {
            return Some(ty);
        }

        // The general resolver only reports class-backed results, so a
        // property or variable holding a non-class type (`array<string,
        // Leaf>`) comes back empty.  Read the declared type directly so
        // template params can still bind from it.
        crate::type_engine::variable::rhs_resolution::resolve_arg_variable_raw_type(trimmed, ctx)
    }

    /// Infer a closure/arrow-function argument's effective return type.
    ///
    /// See [`infer_closure_return_type_seeded`](Self::infer_closure_return_type_seeded);
    /// this is the variant for a call site that knows nothing about what
    /// the closure's parameters receive.
    ///
    /// Returns `None` when the text is not a closure literal or nothing can
    /// be inferred.
    pub(crate) fn infer_closure_return_type(
        arg_text: &str,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        Self::infer_closure_return_type_seeded(arg_text, &[], ctx)
    }

    /// Infer a closure/arrow-function argument's effective return type,
    /// with its parameters seeded from `param_seeds` (one per position;
    /// see [`resolve_closure_body_type`](Self::resolve_closure_body_type)).
    ///
    /// The body expression (an arrow `fn() => EXPR`, or the first `return
    /// EXPR;` of a full closure body) is resolved through the shared type
    /// resolver.  A closure really returns what its body produces narrowed
    /// by what it declares, so an explicit `: ReturnType` annotation only
    /// wins when the body resolves to something that is not a subtype of
    /// it (this includes scalar refinements like `class-string` under a
    /// declared `string`).  An unannotated closure tries generator `yield`
    /// inference before its body.  The body fallback lets template params
    /// bind from unannotated closures like `Cache::remember($k, $ttl,
    /// fn() => new Order())`.
    pub(crate) fn infer_closure_return_type_seeded(
        arg_text: &str,
        param_seeds: &[Option<PhpType>],
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        let body_type = || {
            let body =
                crate::completion::source::helpers::extract_closure_body_expr_text(arg_text)?;
            // A body that resolves to `mixed` says nothing about the
            // template, and binding it hides the template's own bound
            // (`@template TNewKey of array-key`), which is strictly more
            // informative.  Leave the template unbound instead.
            Self::resolve_closure_body_type(arg_text, body, param_seeds, ctx)
                .filter(|ty| !ty.is_mixed())
        };
        let Some(declared) =
            crate::completion::source::helpers::extract_closure_return_type_from_text(arg_text)
        else {
            return crate::completion::source::helpers::infer_generator_type_from_closure_yields(
                arg_text,
            )
            .or_else(body_type);
        };
        // A `: ReturnType` annotation is raw source text, so its class
        // names are still spelled as the file writes them (`Support\Pen`
        // behind a `use App\Support;`).  A template bound from it is
        // compared against types that arrived fully qualified, so the
        // spelling has to be canonicalised before it is bound.
        let declared = crate::util::resolve_php_type_names(&declared, ctx.class_loader);
        let narrowed = body_type().filter(|body| {
            crate::class_lookup::is_subtype_of_typed(body, &declared, ctx.class_loader)
        });
        Some(narrowed.unwrap_or(declared))
    }

    /// Infer a closure/arrow-function argument's return type from its
    /// body, with its first parameter seeded to `param_type`.
    ///
    /// The call site sometimes knows what the callback's first
    /// parameter receives even though the callback leaves it untyped:
    /// `array_map($cb, $users)` hands `$cb` a `User`.  Unlike
    /// [`infer_closure_return_type`](Self::infer_closure_return_type)
    /// this skips the `: ReturnType` annotation, which the caller has
    /// already consulted.
    pub(crate) fn infer_closure_return_type_from_body(
        arg_text: &str,
        param_type: &PhpType,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        let body = crate::completion::source::helpers::extract_closure_body_expr_text(arg_text)?;
        Self::resolve_closure_body_type(arg_text, body, &[Some(param_type.clone())], ctx)
    }

    /// Resolve a closure's body expression to a type, seeding the
    /// closure's own parameters into variable resolution.
    ///
    /// A body expression rooted at a closure parameter (e.g.
    /// `fn(Decimal $carry, $op) => $carry->add(...)`) cannot resolve
    /// through outer-scope assignment scanning because the parameter is
    /// declared in the closure's own signature.  This injects a
    /// `scope_var_resolver` that answers parameter lookups and delegates
    /// everything else to the resolution the body would otherwise get
    /// (the outer scope resolver when present, assignment scanning
    /// otherwise).
    ///
    /// `param_seeds` holds, per position, what the call site hands that
    /// parameter when it knows (`array_map($cb, $users)` hands `$cb` a
    /// `User`).  An untyped parameter takes its seed; a typed one takes it
    /// only when the seed is a subtype of the declared hint, as a
    /// `Timeline<Percentage>` is of `Timeline`, and otherwise keeps the
    /// hint.
    fn resolve_closure_body_type(
        closure_text: &str,
        body: &str,
        param_seeds: &[Option<PhpType>],
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        let typed_params: Vec<(String, PhpType)> =
            crate::completion::source::helpers::extract_closure_params_from_text(closure_text)
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .filter_map(|(index, (name, declared))| {
                    let seed = param_seeds.get(index).and_then(Option::as_ref);
                    // The parameter hint is raw source text, so it carries
                    // the file's own spelling of the class name; canonicalise
                    // it so the seeded type matches one resolved any other
                    // way.
                    let declared =
                        declared.map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader));
                    let ty = match (declared, seed) {
                        (Some(declared), Some(seed))
                            if crate::class_lookup::is_subtype_of_typed(
                                seed,
                                &declared,
                                ctx.class_loader,
                            ) =>
                        {
                            seed.clone()
                        }
                        (Some(declared), _) => declared,
                        (None, seed) => seed?.clone(),
                    };
                    Some((name, ty))
                })
                .collect();
        if typed_params.is_empty() {
            return Self::resolve_arg_text_to_type(body, ctx);
        }

        // Pre-resolve each typed parameter to its classes so the
        // injected resolver is a cheap map lookup.
        let owning_class_name = ctx.current_class.map(|c| c.name.as_str()).unwrap_or("");
        let seed_one = |ty: PhpType| -> Vec<ResolvedType> {
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &ty,
                owning_class_name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if classes.is_empty() {
                vec![ResolvedType::from_type_string(ty)]
            } else {
                ResolvedType::from_classes_with_hint(classes, ty)
            }
        };
        let param_types: HashMap<String, Vec<ResolvedType>> = typed_params
            .into_iter()
            .map(|(name, ty)| {
                // Each alternative of a union is seeded on its own so it
                // keeps its own generic arguments. Two instantiations of the
                // same class (`Builder<A>|Builder<B>`) resolve to one class,
                // and a single entry could only carry the whole union as its
                // type string — leaving a `@return T` on the class with no
                // instantiation to substitute from.
                let resolved = match ty.kind() {
                    TypeKind::Union(members) => {
                        members.iter().cloned().flat_map(&seed_one).collect()
                    }
                    _ => seed_one(ty),
                };
                (name, resolved)
            })
            .collect();

        let walked_locals = walk_closure_body_locals(closure_text, &param_types, ctx);

        let outer_resolver = ctx.scope_var_resolver;
        let param_aware_resolver = move |name: &str| -> Vec<ResolvedType> {
            // A full closure body may reassign a parameter before
            // returning it (`$result['a'] = (string) $result['a']; return
            // $result;`), so the walked locals — the forward walker's own
            // scope after running the body — answer first when they have
            // something to say.  They fall back to the raw seed for a
            // parameter the body never touches, and for an arrow function
            // (nothing to walk).
            if let Some(locals) = &walked_locals
                && let Some(types) = locals.get(&atom(name))
                && !types.is_empty()
            {
                return types.clone();
            }
            if let Some(types) = param_types.get(name) {
                return types.clone();
            }
            match outer_resolver {
                Some(outer) => outer(name),
                // No outer scope resolver: replicate the assignment-scan
                // fallback the body resolution would otherwise take for
                // this variable (see `resolve_variable_fallback`).
                None => {
                    let dummy_class;
                    let effective_class = match ctx.current_class {
                        Some(cc) => cc,
                        None => {
                            dummy_class = crate::class_lookup::class_context_placeholder(
                                ctx.content,
                                ctx.cursor_offset,
                            );
                            &dummy_class
                        }
                    };
                    crate::type_engine::variable::resolution::resolve_variable_types(
                        name,
                        effective_class,
                        ctx.all_classes,
                        ctx.content,
                        ctx.cursor_offset,
                        ctx.class_loader,
                        ctx.backend,
                        Loaders::with_function(ctx.function_loader),
                    )
                }
            }
        };

        let param_ctx = ResolutionCtx {
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            backend: ctx.backend,
            laravel_macro_this_resolver: ctx.laravel_macro_this_resolver,
            resolved_class_cache: ctx.resolved_class_cache,
            function_loader: ctx.function_loader,
            scope_var_resolver: Some(&param_aware_resolver),
            is_in_static_method: ctx.is_in_static_method,
            preserve_static: ctx.preserve_static,
        };
        Self::resolve_arg_text_to_type(body, &param_ctx)
    }
}

/// Walk a full closure literal's own body with the shared forward walker,
/// seeded with the parameter types the call site hands it, and return the
/// scope those statements leave behind.
///
/// [`Backend::resolve_closure_body_type`] otherwise resolves only the
/// return expression's text against the raw parameter seed, so a body that
/// reassigns a parameter before returning it (`$result['a'] = (string)
/// $result['a']; return $result;`) still reports the parameter's original
/// shape. Re-parsing the closure in isolation and running its body through
/// [`walk_body_forward`] answers with what the body actually leaves in
/// `$result`, the same as any other consumer of the forward walker.
///
/// Returns `None` when `closure_text` is not a full (`function`) closure
/// literal — an arrow function's body is a single expression with nothing
/// to walk, so the caller's raw-seed resolver already answers correctly.
fn walk_closure_body_locals(
    closure_text: &str,
    param_types: &HashMap<String, Vec<ResolvedType>>,
    ctx: &ResolutionCtx<'_>,
) -> Option<crate::type_engine::variable::forward_walk::Locals> {
    let trimmed = closure_text.trim().trim_end_matches(';');
    let wrapped = format!("<?php $__closure = {trimmed};");

    // The closure body is parsed and walked in complete isolation from
    // the file the call site sits in: its own offsets are unrelated to
    // (and may numerically collide with) the real file's, so neither the
    // diagnostic scope cache nor the chain-resolution cache may read
    // from, or record into, the active ones while this walk runs. See
    // `out_param::read_out_type`, which walks another file's body for the
    // same reason.
    let _isolated = (suspend_diagnostic_scope(), with_isolated_chain_cache());
    let _barrier = suspend_return_edges();

    crate::parser::with_parsed_program(
        &wrapped,
        "closure_body_return_narrowing",
        |program, content| {
            let closure = program.statements.iter().find_map(|stmt| {
                let Statement::Expression(expr_stmt) = stmt else {
                    return None;
                };
                let Expression::Assignment(assignment) = expr_stmt.expression else {
                    return None;
                };
                let Expression::Closure(closure) = assignment.rhs else {
                    return None;
                };
                Some(closure)
            })?;

            let dummy_class;
            let current_class = match ctx.current_class {
                Some(cc) => cc,
                None => {
                    dummy_class = crate::class_lookup::class_context_placeholder(content, 0);
                    &dummy_class
                }
            };

            let fw_ctx = ForwardWalkCtx {
                current_class,
                all_classes: ctx.all_classes,
                content,
                cursor_offset: u32::MAX,
                class_loader: ctx.class_loader,
                backend: ctx.backend,
                loaders: Loaders::with_function(ctx.function_loader),
                resolved_class_cache: ctx.resolved_class_cache,
                enclosing_return_type: None,
                top_level_scope: None,
                in_loop: false,
                template_markers: None,
                cursor_scope: None,
            };

            let mut scope = ScopeState::new();
            for (name, types) in param_types {
                scope.seed(name, types.clone());
            }

            walk_body_forward(closure.body.statements.iter(), &mut scope, &fw_ctx);

            Some(scope.locals)
        },
    )
}

/// Build the full template substitution map for a method call: class-level
/// substitutions from the receiver's own generic arguments, method-level
/// substitutions bound from the call's arguments, and `@psalm-if-this-is`
/// substitutions inferred from the receiver's concrete type.
///
/// Shared by call-site return-type resolution
/// ([`crate::type_engine::variable::rhs_resolution::calls`]) and
/// `@psalm-this-out` receiver mutation (the forward walker) — both need
/// the same three-layer substitution map, just applied to different
/// target types (the method's return type vs. its self-out type).
pub(crate) fn build_call_template_subs(
    owner: &ClassInfo,
    method_name: &str,
    arg_texts: &[&str],
    receiver_type: Option<&PhpType>,
    ctx: &ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let class_level_subs: HashMap<String, PhpType> = receiver_type
        .map(|ty| {
            if ty.is_self_like()
                || matches!(ty.kind(), TypeKind::Generic(g) if g.args.iter().any(|a| a.is_self_like()))
            {
                return HashMap::new();
            }

            let mut values: HashMap<String, PhpType> = owner
                .template_param_defaults
                .iter()
                .map(|(name, default)| (name.to_string(), default.clone()))
                .collect();
            if let TypeKind::Generic(g) = ty.kind()
                && !g.args.is_empty()
                && !owner.template_params.is_empty()
            {
                values.extend(
                    owner
                        .template_params
                        .iter()
                        .zip(g.args.iter())
                        .map(|(name, ty)| (name.to_string(), ty.clone())),
                );
            }
            values
        })
        .unwrap_or_default();

    let method_template_subs =
        Backend::build_method_template_subs(owner, method_name, arg_texts, ctx);

    let if_this_is_subs: HashMap<String, PhpType> = owner
        .get_method_ci(method_name)
        .and_then(|m| m.if_this_is.as_ref())
        .and_then(|pattern| {
            let method = owner.get_method_ci(method_name)?;
            Some(
                crate::type_engine::variable::rhs_resolution::infer_if_this_is_subs(
                    pattern,
                    receiver_type?,
                    &method.template_params,
                    &method.template_param_bounds,
                ),
            )
        })
        .unwrap_or_default();

    let mut template_subs = class_level_subs;
    template_subs.extend(method_template_subs);
    template_subs.extend(if_this_is_subs);
    template_subs
}

/// Parameter names (`$`-prefixed) that are the *exclusive* binding site
/// for a `@template` parameter at this call site.
///
/// A template bound from exactly one argument has no independent
/// signal to check that argument against: the substituted type came from
/// resolving that same argument, so any diagnostic that compares the
/// argument to it again is comparing the argument against itself through
/// two potentially-diverging resolution paths. For example, PHPUnit's
/// `assertSame(ExpectedType $expected, mixed $actual)` binds
/// `ExpectedType` only from `$expected`, so checking `$expected` against
/// `ExpectedType` is circular and can never legitimately fail.
///
/// What counts is the binding sites the *caller filled*, not the ones the
/// signature declares. Laravel's `travelTo` names `TDate` in both `$date`
/// and the optional `$callback`'s callable signature; a call that passes
/// only a date still binds `TDate` from that one argument, so checking it
/// is just as circular as if `$callback` did not exist. Omitted
/// parameters are dropped here for that reason.
///
/// A template two arguments both bind is not covered: those disagree with
/// each other rather than with themselves, which is a real check.
///
/// The substitution is circular, but a template's `of` bound is not: it
/// is what the argument had to satisfy to bind the template at all. Each
/// parameter maps to its declared type with the templates only it binds
/// replaced by their bounds, or to `None` when none of those templates
/// declares one, since then there is nothing to check. Every other
/// template the declaration binds becomes `mixed`: another argument
/// decides it, and that argument's own check is where the two can
/// disagree.
pub(crate) fn self_bound_template_params(
    bindings: &[(Atom, Atom)],
    parameters: &[ParameterInfo],
    arg_texts: &[&str],
    bound_of: &dyn Fn(&Atom) -> Option<PhpType>,
) -> AtomMap<Option<PhpType>> {
    let self_bound = exclusively_bound_templates(bindings, parameters, arg_texts);
    let mut result = AtomMap::default();
    for (tpl_name, param_name) in &self_bound {
        let bounds = result.entry(*param_name).or_insert_with(HashMap::new);
        if let Some(bound) = bound_of(tpl_name) {
            bounds.insert(tpl_name.to_string(), bound);
        }
    }
    result
        .into_iter()
        .map(|(param_name, bounds): (Atom, HashMap<String, PhpType>)| {
            if bounds.is_empty() {
                return (param_name, None);
            }
            let declared = parameters
                .iter()
                .find(|p| p.name == param_name.as_str())
                .and_then(|p| p.type_hint.as_ref());
            let checked = declared.map(|hint| {
                let mut bound_subs: HashMap<String, PhpType> = bindings
                    .iter()
                    .map(|(tpl_name, _)| (tpl_name.to_string(), PhpType::mixed()))
                    .collect();
                bound_subs.extend(bounds);
                hint.substitute(&bound_subs)
            });
            (param_name, checked)
        })
        .collect()
}

/// The `(template, parameter)` bindings the call fills, keeping only the
/// templates exactly one filled parameter binds.
fn exclusively_bound_templates(
    bindings: &[(Atom, Atom)],
    parameters: &[ParameterInfo],
    arg_texts: &[&str],
) -> Vec<(Atom, Atom)> {
    let bound = crate::call_args::bind_text_args_to_params(parameters, arg_texts);
    let was_passed = |param_name: &Atom| {
        parameters
            .iter()
            .position(|p| p.name == param_name.as_str())
            .is_some_and(|idx| bound.get(idx).is_some_and(Option::is_some))
    };
    let filled: Vec<&(Atom, Atom)> = bindings
        .iter()
        .filter(|(_, param_name)| was_passed(param_name))
        .collect();

    filled
        .iter()
        .filter(|(tpl_name, _)| filled.iter().filter(|(t, _)| t == tpl_name).count() == 1)
        .map(|binding| **binding)
        .collect()
}

/// Resolve the elements of an array literal argument to the type a template
/// bound through them takes.
///
/// `resolve_arg_text_to_type("[1, 2, 3]")` collapses the whole literal to
/// a bare `array` with no element type, so callers that need the element
/// type itself (binding a template through an array-like wrapper) must
/// unwrap the literal and resolve its elements directly instead.  A scalar
/// literal element stays a literal, as a scalar argument bound directly
/// does (`[1, 2]` binds `1|2`), and an empty literal has no element at all,
/// so it binds `never`.
///
/// Returns `None` when `arg_text` is not a `[...]` literal, or an element
/// (a spread, or an expression we cannot resolve) says nothing definite.
pub(crate) fn array_literal_element_type(
    arg_text: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let trimmed = arg_text.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))?
        .trim();
    if inner.is_empty() {
        return Some(PhpType::never());
    }
    let mut members: Vec<PhpType> = Vec::new();
    for elem in crate::type_engine::types::conditional::split_text_args(inner) {
        let elem = elem.trim();
        if elem.is_empty() {
            continue;
        }
        if elem.starts_with("...") {
            return None;
        }
        let value_text = match elem.find("=>") {
            Some(arrow_pos) => elem[arrow_pos + 2..].trim(),
            None => elem,
        };
        let ty = literal_arg_type(value_text)
            .or_else(|| Backend::resolve_arg_text_to_type(value_text, ctx))?;
        for member in ty.union_members() {
            if !members.contains(member) {
                members.push(member.clone());
            }
        }
    }
    match members.len() {
        0 => None,
        1 => members.pop(),
        _ => Some(PhpType::union(members)),
    }
}

/// Build an `array{key: type, ...}` shape from an array literal argument's
/// own keys and values.
///
/// `resolve_arg_text_to_type("['debug' => false]")` collapses the whole
/// literal to the bare `array` keyword, which is enough for most callers
/// but erases the literal's own keys. A template bound directly to the
/// whole argument (no wrapping hint to unify against) needs those keys
/// preserved so `key-of<T>`/`value-of<T>` on the bound template can still
/// project them out.
///
/// Only keyed entries are recorded — a mixed literal's positional elements
/// are dropped, matching the AST-based array literal inference in
/// `raw_type_inference.rs`. Returns `None` when `arg_text` is not a
/// `[...]`/`array(...)` literal, or none of its entries have a literal
/// string/int key.
pub(crate) fn array_literal_shape_type(arg_text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    array_literal_shape_type_with(arg_text, &|text| {
        Backend::resolve_arg_text_to_type(text, ctx)
    })
}

/// [`array_literal_shape_type`] with the values that are not scalar
/// literals resolved by `resolve_value`, for a caller that has an argument
/// resolver rather than a resolution context.
///
/// A value that is itself an array literal gets a shape of its own
/// (`[['a', 'b']]` is `array{array{'a', 'b'}}`).
pub(crate) fn array_literal_shape_type_with(
    arg_text: &str,
    resolve_value: &dyn Fn(&str) -> Option<PhpType>,
) -> Option<PhpType> {
    let trimmed = arg_text.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .or_else(|| {
            trimmed
                .strip_prefix("array(")
                .and_then(|s| s.strip_suffix(')'))
        })?
        .trim();
    if inner.is_empty() {
        return None;
    }

    let mut entries: Vec<crate::php_type::ShapeEntry> = Vec::new();
    // The key PHP gives the next element written without one: one past the
    // largest integer key so far.  Unknown once a key we cannot read (or a
    // spread) could have been an integer.
    let mut next_index: Option<i64> = Some(0);
    for elem in crate::type_engine::types::conditional::split_text_args(inner) {
        let elem = elem.trim();
        if elem.is_empty() {
            continue;
        }
        let (key, value_text, unkeyed) = match elem.find("=>") {
            Some(arrow_pos) => {
                let key = literal_array_key_text(elem[..arrow_pos].trim());
                match key.as_deref().map(str::parse::<i64>) {
                    Some(Ok(int_key)) => {
                        next_index = next_index.map(|next| next.max(int_key.saturating_add(1)));
                    }
                    Some(Err(_)) => {}
                    None => next_index = None,
                }
                (key, elem[arrow_pos + 2..].trim(), false)
            }
            None if elem.starts_with("...") => {
                next_index = None;
                (None, elem, false)
            }
            None => {
                let key = next_index.map(|index| index.to_string());
                next_index = next_index.map(|index| index.saturating_add(1));
                (key, elem, true)
            }
        };
        let Some(key) = key else {
            continue;
        };
        // Positional while it lands on the index a reader counting the
        // positional entries before it would expect, the way the AST's
        // array literal inference spells it.
        let positional_count = entries.iter().filter(|entry| entry.key.is_none()).count();
        let key = (!unkeyed || positional_count.to_string() != key).then_some(key);
        // `resolve_arg_text_to_type` widens a scalar literal to its base
        // type (`1` → `int`), which would leave `value-of<T>` over the
        // bound shape with the scalar rather than the literal the caller
        // wrote. Keep int/float/string literals precise; everything else
        // (`true`/`false`, `null`, variables, calls) resolves as before.
        let value_type =
            crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(
                value_text,
            )
            .filter(|ty| matches!(ty.kind(), TypeKind::Literal(_)))
            .or_else(|| array_literal_shape_type_with(value_text, resolve_value))
            .or_else(|| resolve_value(value_text))
            .unwrap_or_else(PhpType::mixed);
        entries.push(crate::php_type::ShapeEntry {
            key,
            value_type,
            optional: false,
        });
    }

    (!entries.is_empty()).then(|| PhpType::array_shape(entries))
}

/// The literal an argument binds a template to when the template's own bound
/// is a type operator — `@template K of key-of<TABLE>`, `@template V of
/// value-of<TABLE>`, `@template E of TABLE[K]`.
///
/// `resolve_arg_text_to_type` widens a scalar literal to its base type, which
/// is what nearly every binding wants and exactly wrong here: the operator can
/// only be evaluated against the *specific* key or value the caller wrote, so
/// `'immutable'` has to stay `'immutable'` rather than becoming `string`. The
/// quotes are kept because that is the spelling `evaluate_index_access`
/// matches shape keys against.
///
/// Returns `None` when the bound is not a type operator or the argument is not
/// a scalar literal, leaving the ordinary binding modes to resolve it.
pub(crate) fn type_operator_bound_literal(
    bound: Option<&PhpType>,
    arg_text: &str,
) -> Option<PhpType> {
    let bound = bound?;
    if !matches!(
        bound.kind(),
        TypeKind::KeyOf(_) | TypeKind::ValueOf(_) | TypeKind::IndexAccess(..)
    ) {
        return None;
    }
    crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(arg_text.trim())
        .filter(|ty| matches!(ty.kind(), TypeKind::Literal(_)))
}

/// The array shape a constant read through a type operator describes.
///
/// `key-of<ID_TABLE>` and `ID_TABLE[K]` name an operand as concrete as an
/// inline `array{…}`, but the docblock parser only ever sees the name — it
/// cannot read the constant behind it. This does, from the constant's own
/// initializer text, for a global constant (`ID_TABLE`) and for the
/// `Class::CONST` spelling alike.
///
/// Returns `None` when the name is not a constant we can reach, when its
/// value is not an array literal, or when none of its keys are literal — in
/// each case the operator stays unevaluated and widens to its bound, which
/// is the honest reading of an operand nobody can read.
pub(crate) fn constant_operand_shape(name: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let value = match name.rsplit_once("::") {
        Some((class_part, const_name)) => {
            // `class_part` is a source-level reference, so an unqualified
            // name must resolve against the declaring class's namespace
            // before falling back to the global scope — otherwise a file
            // with several braced `namespace` blocks that each declare the
            // same short class name always picks the first one, regardless
            // of which block actually declared the operand.
            let class_name =
                crate::class_lookup::resolve_class_keyword(class_part, ctx.current_class)
                    .unwrap_or_else(|| {
                        let ns = ctx.current_class.and_then(|c| c.file_namespace.as_deref());
                        crate::util::resolve_source_class_name(
                            class_part,
                            ns,
                            ctx.all_classes,
                            ctx.class_loader,
                        )
                    });
            let class = crate::class_lookup::find_class_by_fqn(ctx.all_classes, &class_name)
                .cloned()
                .or_else(|| (ctx.class_loader)(&class_name))?;
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &class,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            merged.get_constant(const_name)?.value.clone()?
        }
        // The name may arrive in either spelling: some paths hand over what
        // `resolve_names` already qualified against the file's namespace,
        // since almost every bare name in a type position is a class, while
        // the ones that resolve names through the class loader alone leave a
        // constant bare.
        None => resolve_operand_constant_value(name, ctx)?,
    };
    array_literal_shape_type(&value, ctx)
}

/// The initializer text of the constant a type operand names, or `None`
/// when no spelling of it is indexed.
///
/// A constant is indexed under its fully-qualified name, so the enclosing
/// namespace is tried first for a bare operand, then the name as written,
/// then the global constant of that short name the way PHP itself falls
/// back, and finally the file's `use const` imports.
///
/// Only the indexed lookup is used, never the one that ends by parsing
/// every autoload file: an operand is as often a template parameter as a
/// constant, and a miss must not charge the whole autoload set.
fn resolve_operand_constant_value(name: &str, ctx: &ResolutionCtx<'_>) -> Option<String> {
    let backend = ctx.backend?;
    // An absolute name names exactly one constant, with no namespace or
    // import in play.
    if let Some(absolute) = name.strip_prefix('\\') {
        return backend.lookup_indexed_global_constant(absolute);
    }
    let short = crate::util::short_name(name);
    if short == name
        && let Some(value) = ctx
            .current_class
            .and_then(|c| c.file_namespace.as_ref())
            .and_then(|ns| backend.lookup_indexed_global_constant(&format!("{ns}\\{name}")))
    {
        return Some(value);
    }
    if let Some(value) = backend.lookup_indexed_global_constant(name) {
        return Some(value);
    }
    if short != name
        && let Some(value) = backend.lookup_indexed_global_constant(short)
    {
        return Some(value);
    }
    // Last: a `use const` import, which is the one spelling neither the
    // enclosing namespace nor the global scope accounts for.  The file's
    // import table is read from the source here because a type operand
    // carries no offset to resolve against, and it is read only once
    // every cheaper candidate has missed — a docblock naming an imported
    // constant is rare, and the walk runs over the already-parsed AST.
    let use_map = backend.parse_use_statements(ctx.content);

    let imported = match name.split_once('\\') {
        Some((first, rest)) => use_map.get(first).map(|fqn| format!("{fqn}\\{rest}")),
        None => use_map.get(name).cloned(),
    }?;
    backend.lookup_indexed_global_constant(&imported)
}

/// The substitution map that resolves every constant operand a set of types
/// reads through an unevaluated type operator.
///
/// Names in `skip` are left out: a surviving operator's operand is either a
/// constant or a template parameter, and looking a template parameter up as
/// a constant only wastes work.
fn constant_operand_subs<'t>(
    types: impl Iterator<Item = &'t PhpType>,
    skip: &[Atom],
    ctx: &ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let mut operands = Vec::new();
    for ty in types {
        if ty.contains_unevaluated_operator() {
            ty.unevaluated_operator_operands(&mut operands);
        }
    }

    let mut subs: HashMap<String, PhpType> = HashMap::new();
    for operand in operands {
        if subs.contains_key(&operand) || skip.iter().any(|p| p.as_str() == operand) {
            continue;
        }
        if let Some(shape) = constant_operand_shape(&operand, ctx) {
            subs.insert(operand, shape);
        }
    }
    subs
}

/// Evaluate the type operators a declared type reads through a constant.
///
/// `key-of<ID_TABLE>` on a parameter and `value-of<ID_TABLE>` on a return
/// describe a set of values as concrete as any written-out union, but the
/// docblock parser only saw a name it could not read and left the operator
/// standing. This reads the constant behind the name and finishes the
/// operator, so the type constrains its call sites like the union it is.
///
/// Every consumer of a declared parameter or return type goes through here,
/// not just the template path: a constant is no less readable from a
/// signature that declares no `@template`.
///
/// Returns `None` when the type has no unevaluated operator, when no operand
/// is a constant we can read, or when reading it changed nothing — in each
/// case the caller keeps the type it already has.
pub(crate) fn evaluate_constant_operands(ty: &PhpType, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    if !ty.contains_unevaluated_operator() {
        return None;
    }
    let subs = constant_operand_subs(std::iter::once(ty), &[], ctx);
    let evaluated = if subs.is_empty() {
        ty.clone()
    } else {
        ty.substitute(&subs)
    };
    let evaluated = if evaluated.contains_unevaluated_operator() {
        evaluate_enum_value_of(&evaluated, ctx)
    } else {
        evaluated
    };
    (evaluated != *ty).then_some(evaluated)
}

/// Finish every `value-of<…>` in `ty` whose operand is a backed enum or one
/// of its cases, re-evaluating the operators around it.
///
/// `value-of<Suit::Hearts>` is the case's backing value and `value-of<Suit>`
/// the union of every case's, which is what a template bound to an enum
/// case reads a shape through (`Data[value-of<T>]` with `T` bound to
/// `Target::DASHBOARD`).  An operand that is not a backed enum, or a case
/// with no backing value, leaves the operator standing.
fn evaluate_enum_value_of(ty: &PhpType, ctx: &ResolutionCtx<'_>) -> PhpType {
    if !ty.contains_unevaluated_operator() {
        return ty.clone();
    }
    let recurse = |inner: &PhpType| evaluate_enum_value_of(inner, ctx);
    match ty.kind() {
        TypeKind::ValueOf(operand) => {
            enum_backing_values(operand, ctx).unwrap_or_else(|| PhpType::value_of(recurse(operand)))
        }
        TypeKind::KeyOf(operand) => crate::php_type::evaluate_key_of(&recurse(operand)),
        TypeKind::IndexAccess(base, index) => {
            crate::php_type::evaluate_index_access(&recurse(base), &recurse(index))
        }
        _ => ty.map_children(&recurse),
    }
}

/// The backing value of the enum case `operand` names (`Suit::Hearts`), or
/// the union of every case's for a backed enum named by itself.
fn enum_backing_values(operand: &PhpType, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let name: &str = match operand.kind() {
        TypeKind::Named(name) => name,
        TypeKind::Raw(raw) => raw,
        _ => return None,
    };
    let (class_part, case) = match name.rsplit_once("::") {
        Some((class_part, case)) => (class_part, Some(case)),
        None => (name, None),
    };
    let class_name = crate::class_lookup::resolve_class_keyword(class_part, ctx.current_class)
        .unwrap_or_else(|| {
            let ns = ctx.current_class.and_then(|c| c.file_namespace.as_deref());
            crate::util::resolve_source_class_name(
                class_part,
                ns,
                ctx.all_classes,
                ctx.class_loader,
            )
        });
    let class = crate::class_lookup::find_class_by_fqn(ctx.all_classes, &class_name)
        .cloned()
        .or_else(|| (ctx.class_loader)(&class_name))?;
    if class.kind != crate::types::ClassLikeKind::Enum {
        return None;
    }
    let mut values: Vec<PhpType> = Vec::new();
    for constant in class.constants.iter().filter(|c| c.is_enum_case) {
        if case.is_some_and(|case| constant.name != case) {
            continue;
        }
        let value = crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(
            constant.enum_value.as_deref()?,
        )?;
        if !values.contains(&value) {
            values.push(value);
        }
    }
    match values.len() {
        0 => None,
        1 => values.pop(),
        _ => Some(PhpType::union(values)),
    }
}

/// Recover a template parameter no argument names directly, from the
/// *bound* of a template that an argument did bind.
///
/// `usort`'s stub declares `@template T` and `@template TArray of array<T>`
/// with `@param TArray $array` / `@param callable(T, T): int $callback`.
/// Nothing at the call site binds `T`: the array argument binds `TArray`,
/// and the callback is the very thing whose parameters `T` is meant to
/// type. Unifying `TArray`'s bound (`array<T>`) against what `TArray` was
/// bound to (`list<Error>`) recovers `T = Error`, which is what turns an
/// untyped `usort($errors, fn ($a, $b) => …)` callback into a typed one.
///
/// The bound may also name a *supertype* of what the other template was
/// bound to rather than its own shape. `CollectedDataNode::get()` declares
/// `@template TCollector of Collector<Node, TValue>` with
/// `@param class-string<TCollector>`, so `TValue` is whatever the collector
/// class's own `@implements Collector<…, …>` says it is — read off the
/// class's ancestry rather than off the argument's shape.
///
/// Runs before the fill-in below, so it only ever reads bindings that came
/// from real arguments, never a bound standing in for a missing one.
fn propagate_bound_template_bindings(
    subs: &mut HashMap<String, PhpType>,
    template_params: &[Atom],
    template_param_bounds: &crate::atom::AtomMap<PhpType>,
    ctx: &ResolutionCtx<'_>,
) {
    for tpl_name in template_params {
        if subs.contains_key(tpl_name.as_str()) {
            continue;
        }
        // Iterate the declared order rather than the bounds map so the
        // binding a template picks up does not depend on hash order.
        for other in template_params {
            if other == tpl_name {
                continue;
            }
            let Some(bound) = template_param_bounds.get(other) else {
                continue;
            };
            let Some(bound_to) = subs.get(other.as_str()) else {
                continue;
            };
            if let Some(recovered) = unify_template(bound, bound_to, tpl_name)
                .or_else(|| ancestor_bound_binding(bound, bound_to, tpl_name, ctx))
            {
                subs.insert(tpl_name.to_string(), recovered);
                break;
            }
        }
    }
}

/// Read `tpl_name` off the ancestry of what another template was bound to,
/// for a bound that names a generic supertype (`@template TCollector of
/// Collector<Node, TValue>`).
///
/// A `class-string<TCollector>` parameter binds the class itself, so the
/// generic arguments live on its `@extends`/`@implements` clauses, not on
/// the bound type it was matched against.
fn ancestor_bound_binding(
    bound: &PhpType,
    bound_to: &PhpType,
    tpl_name: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let TypeKind::Generic(g) = bound.kind() else {
        return None;
    };
    let position = g.args.iter().position(|a| a.is_named(tpl_name))?;
    let subject = bound_to.unwrap_class_string_inner().unwrap_or(bound_to);
    crate::type_engine::variable::rhs_resolution::extract_generic_arg_from_ancestor(
        subject, &g.name, position, ctx,
    )
}

/// Finish a template substitution map: bind the constants its types read
/// through a type operator, recover the templates only another template's
/// bound names, then fill in the template params no argument bound.
///
/// The halves exist so a raw name never leaks downstream. An unbound
/// template resolves to its declared default (`@template T of object =
/// \stdClass` → `stdClass`), else its upper bound (`@template T of Foo` →
/// `Foo`) or `mixed`, following PHPStan's `resolveToBounds()`. A constant
/// operand resolves to the array shape it names, which is what lets the
/// substitution every call site already runs finish the operator:
/// `key-of<TABLE>` becomes the table's own keys, and `TABLE[K]` picks out
/// the single value the argument bound `K` to.
///
/// The constant bindings are applied to the bounds as well, so a call that
/// leaves `K` unbound still reads `TABLE[key-of<TABLE>]` as the table's
/// value union rather than giving up on the operator.
pub(crate) fn finish_template_subs(
    subs: &mut HashMap<String, PhpType>,
    template_params: &[Atom],
    template_param_bounds: &crate::atom::AtomMap<PhpType>,
    template_param_defaults: &[(Atom, PhpType)],
    return_type: Option<&PhpType>,
    ctx: &ResolutionCtx<'_>,
) {
    drop_bindings_outside_bounds(subs, template_params, template_param_bounds, ctx);
    propagate_bound_template_bindings(subs, template_params, template_param_bounds, ctx);

    let constant_subs = constant_operand_subs(
        return_type
            .into_iter()
            .chain(template_param_bounds.values()),
        template_params,
        ctx,
    );

    for tpl_name in template_params {
        if subs.contains_key(tpl_name.as_str()) {
            continue;
        }
        let fallback = template_param_defaults
            .iter()
            .find(|(name, _)| name == tpl_name)
            .map(|(_, default)| default.substitute(subs))
            .or_else(|| {
                template_param_bounds
                    .get(tpl_name)
                    .map(|bound| bound.substitute(&constant_subs))
            })
            .unwrap_or_else(PhpType::mixed);
        subs.insert(tpl_name.to_string(), fallback);
    }

    for (name, shape) in constant_subs {
        subs.entry(name).or_insert(shape);
    }
}

/// Unbind every template an argument bound to a class its bound rules out.
///
/// `@template F of User` handed an `Article` cannot be `Article`: the call
/// is an error, and the template falls back to the bound it fails to
/// satisfy, which is what the call is declared to deal in (PHPStan does the
/// same).  Only a class judged against a class bound is decided here; a
/// class nobody can load, a bound that still names a template, or anything
/// but a class on either side keeps its binding.
fn drop_bindings_outside_bounds(
    subs: &mut HashMap<String, PhpType>,
    template_params: &[Atom],
    template_param_bounds: &AtomMap<PhpType>,
    ctx: &ResolutionCtx<'_>,
) {
    if template_param_bounds.is_empty() || subs.is_empty() {
        return;
    }
    let loaded_class_names = |ty: &PhpType| -> Option<Vec<Atom>> {
        let mut names = Vec::new();
        for member in ty.union_members() {
            let name = match member.kind() {
                TypeKind::Named(name) if !crate::php_type::is_keyword_type(name) => *name,
                TypeKind::Generic(g) if !crate::php_type::is_keyword_type(&g.name) => g.name,
                _ => return None,
            };
            if template_params.contains(&name) || (ctx.class_loader)(&name).is_none() {
                return None;
            }
            names.push(name);
        }
        Some(names)
    };
    for tpl_name in template_params {
        let Some(bound) = template_param_bounds.get(tpl_name) else {
            continue;
        };
        let Some(bound_to) = subs.get(tpl_name.as_str()) else {
            continue;
        };
        let (Some(bound_classes), Some(bound_to_classes)) =
            (loaded_class_names(bound), loaded_class_names(bound_to))
        else {
            continue;
        };
        let satisfies = bound_to_classes.iter().all(|sub| {
            bound_classes
                .iter()
                .any(|sup| crate::class_lookup::is_subtype_of_names(sub, sup, ctx.class_loader))
        });
        if !satisfies {
            subs.remove(tpl_name.as_str());
        }
    }
}

/// Extract the literal key text of an array literal's key expression
/// (`'debug'`, `"verbose"`, `42`), unquoting string keys. Returns `None`
/// for any key that is not a literal (e.g. a constant or variable), since
/// those cannot be projected into a shape's key set.
fn literal_array_key_text(key_text: &str) -> Option<String> {
    if let Some(unquoted) = crate::text_scan::unquote_php_string(key_text) {
        return Some(unquoted.to_string());
    }
    let numeric = key_text.strip_prefix('-').unwrap_or(key_text);
    (!numeric.is_empty() && numeric.bytes().all(|b| b.is_ascii_digit()))
        .then(|| key_text.to_string())
}

/// `bindings` in the order they should be bound: those read off a
/// callable-typed parameter come after all the rest.
///
/// A callable argument's return type is inferred with its parameters
/// seeded from the templates the other arguments bound, and a template it
/// names in a parameter position only binds when nothing else did, so both
/// need every other argument bound first, whatever order the `@param` tags
/// are written in.
pub(crate) fn callable_bindings_last<'a>(
    bindings: &'a [(Atom, Atom)],
    params: &'a [ParameterInfo],
) -> impl Iterator<Item = &'a (Atom, Atom)> {
    let is_callable = move |binding: &&(Atom, Atom)| {
        params
            .iter()
            .find(|p| p.name == binding.1.as_str())
            .and_then(|p| p.type_hint.as_ref())
            .is_some_and(|h| h.callable_param_types().is_some())
    };
    bindings
        .iter()
        .filter(move |b| !is_callable(b))
        .chain(bindings.iter().filter(is_callable))
}

/// Bind a template parameter that a `@param Closure(T): void` hint names in
/// the callback's *parameter* list, reading the type off the closure
/// argument's own annotation at `position`.
///
/// The annotation is raw source text, so it carries the file's spelling of
/// the class rather than its FQCN (`Support\Pen` behind a `use App\Support;`).
/// A template bound from it is compared against types that arrived fully
/// qualified, so the spelling is canonicalised here.
///
/// A parameter position is contravariant: the closure accepting a
/// `Timeline` says nothing about the `Timeline<Percentage>` the call hands
/// it.  Callers therefore only bind from here when no other argument bound
/// the template (see [`callable_bindings_last`]).
pub(crate) fn bind_callable_param_template(
    arg_text: &str,
    position: usize,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let param_type = crate::completion::source::helpers::extract_closure_param_type_from_text(
        arg_text, position,
    )?;
    Some(crate::util::resolve_php_type_names(
        &param_type,
        ctx.class_loader,
    ))
}

/// Bind the template a `@param callable(...): …` hint names in its return
/// type, from the closure argument written at the call site.
///
/// A closure's return type is what its body produces, narrowed by what it
/// declares (PHPStan's `intersectButNotNever`).  `fn (Timeline $t): Timeline
/// => $t` handed a `Timeline<Percentage>` returns `Timeline<Percentage>`,
/// and `fn ($c): array => arrStr($c)` returns the `array<int, string>` that
/// `arrStr()` declares.  The body is resolved with each closure parameter
/// seeded from the hint's own parameter types wherever the templates they
/// name are already in `bound`, so an untyped `fn ($t) => $t` receives what
/// the call hands it.  An annotation the body cannot narrow (a scalar, or a
/// body that resolves to something unrelated) stands as written, and an
/// unannotated closure falls back to its generator yields.
///
/// The template is rarely the whole return type: `array<TKey, TValue>`,
/// `list<TValue>`, and Laravel's `Collection<TKey, TValue>|array<TKey,
/// TValue>` each name it at a position inside a larger shape.  The inferred
/// type is matched against that shape so each template binds to its own
/// part rather than to the whole return type.
///
/// `bindings` is the callee's full template binding list, which is how an
/// unbound template in the hint's parameter types is told apart from a
/// class name.
pub(crate) fn bind_callable_return_template(
    arg_text: &str,
    param_hint: Option<&PhpType>,
    tpl_name: &str,
    bound: &HashMap<String, PhpType>,
    bindings: &[(Atom, Atom)],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let declared_ret = param_hint.and_then(|h| h.callable_return_type());
    let ret_type = if crate::completion::source::helpers::is_closure_like_text(arg_text.trim()) {
        let seeds = callable_param_seeds(param_hint, bound, bindings);
        Backend::infer_closure_return_type_seeded(arg_text, &seeds, ctx)?
    } else {
        // A callable held in a variable or returned by a call has no body
        // to read, but its own type (`callable(callable(): int): string`)
        // still says what it returns.
        Backend::resolve_arg_text_to_type(arg_text, ctx)?
            .callable_return_type()?
            .clone()
    };
    let bound = declared_ret.and_then(|declared| unify_template(declared, &ret_type, tpl_name));
    Some(bound.unwrap_or(ret_type))
}

/// The types a callable hint promises each of its parameters, with the
/// callee's already-bound templates substituted in.
///
/// A position whose hint still names an unbound template has nothing
/// concrete to promise and is left `None`.
fn callable_param_seeds(
    param_hint: Option<&PhpType>,
    bound: &HashMap<String, PhpType>,
    bindings: &[(Atom, Atom)],
) -> Vec<Option<PhpType>> {
    let Some(params) = param_hint.and_then(|h| h.callable_param_types()) else {
        return Vec::new();
    };
    params
        .iter()
        .map(|p| {
            let names_unbound = bindings.iter().any(|(t, _)| {
                !bound.contains_key(t.as_str())
                    && crate::type_engine::variable::rhs_resolution::type_contains_name(
                        &p.type_hint,
                        t,
                    )
            });
            (!names_unbound).then(|| p.type_hint.substitute(bound))
        })
        .collect()
}

/// Bind a template parameter by walking a parameter hint and an argument
/// type together.
///
/// Returns the argument's subtree at whichever position `tpl_name` occupies
/// in `param_hint`.  For `@param array<string, array<string, T>> $in` and an
/// argument typed `array<string, array<string, Leaf>>`, that is `Leaf` —
/// where positional extraction, which unwraps a single level, would bind the
/// whole inner array.
///
/// A union hint offers several shapes and the argument picks one: for
/// `@param iterable<array-key, T>|T $value` (Laravel's `Collection::wrap()`)
/// an `array<string>` argument binds `T` to `string` through the iterable
/// alternative, while a `string` argument binds `T` to `string` through the
/// bare one.  The bare alternative matches anything, so it is only used when
/// no other alternative fits.
///
/// Returns `None` when the hint does not name the template, or when the two
/// shapes disagree, leaving the caller's positional extraction to run.
pub(super) fn unify_template(
    param_hint: &PhpType,
    arg_type: &PhpType,
    tpl_name: &str,
) -> Option<PhpType> {
    match param_hint.kind() {
        TypeKind::Named(name) if &**name == tpl_name => Some(arg_type.clone()),
        TypeKind::Union(members) => {
            let mut has_bare = false;
            for member in members {
                if member.is_null() {
                    continue;
                }
                if member.is_named(tpl_name) {
                    has_bare = true;
                    continue;
                }
                if let Some(unified) = unify_template(member, arg_type, tpl_name) {
                    return Some(unified);
                }
            }
            has_bare.then(|| bare_template_remainder(members, arg_type, tpl_name))
        }
        TypeKind::Generic(hint) => {
            if let TypeKind::Generic(arg) = arg_type.kind()
                && arg.args.len() == hint.args.len()
            {
                return hint
                    .args
                    .iter()
                    .zip(arg.args.iter())
                    .find_map(|(h, a)| unify_template(h, a, tpl_name));
            }
            // Two container types whose arguments don't line up positionally
            // (`iterable<TKey, TValue>` against `list<string>`) still line up
            // key-to-key and value-to-value.
            if !crate::type_engine::variable::rhs_resolution::is_array_like_wrapper(&hint.name) {
                return None;
            }
            // An empty array has no key or value for the template to be.
            if arg_type.is_empty_array_shape() && names_template_directly(param_hint, tpl_name) {
                return Some(PhpType::never());
            }
            let key_match = (hint.args.len() >= 2)
                .then(|| arg_type.extract_key_type(false))
                .flatten()
                .and_then(|k| unify_template(&hint.args[0], k, tpl_name));
            key_match.or_else(|| {
                let value_hint = hint.args.last()?;
                // An untyped `array` still says "the argument is a container",
                // its elements are just unknown — `mixed`.  Without this the
                // hint does not match at all and a bare `T` alternative in a
                // union hint binds the array itself as the element type.
                let mixed = PhpType::mixed();
                let value = match arg_type.extract_value_type(false) {
                    Some(v) => v,
                    None if arg_type.is_bare_array() => &mixed,
                    None => return None,
                };
                unify_template(value_hint, value, tpl_name)
            })
        }
        TypeKind::Array(inner) => match arg_type.kind() {
            TypeKind::Array(arg_inner) => unify_template(inner, arg_inner, tpl_name),
            // `T[]` against `array<K, V>` / `list<V>`: the value type lines up.
            _ => arg_type
                .extract_value_type(false)
                .and_then(|v| unify_template(inner, v, tpl_name)),
        },
        TypeKind::Nullable(inner) => unify_template(inner, arg_type.unwrap_nullable(), tpl_name),
        _ => None,
    }
}

/// What a bare `T` alternative of a union hint binds to: the part of the
/// argument the hint's other alternatives don't already account for.
///
/// For `@param Tvalue|string|null` and a `string|float|bool|null` argument
/// that is `float|bool`, so the return type's `Tvalue` does not bring the
/// `null` back.  When the other alternatives cover the whole argument, the
/// argument itself is the binding.
fn bare_template_remainder(
    hint_members: &[PhpType],
    arg_type: &PhpType,
    tpl_name: &str,
) -> PhpType {
    let others: Vec<&PhpType> = hint_members
        .iter()
        .filter(|m| !crate::type_engine::variable::rhs_resolution::type_contains_name(m, tpl_name))
        .collect();
    if others.is_empty() {
        return arg_type.clone();
    }
    let arg_members = arg_type.union_members();
    let remainder: Vec<PhpType> = arg_members
        .iter()
        .filter(|a| !others.iter().any(|o| a.is_subtype_of(o)))
        .map(|a| (*a).clone())
        .collect();
    if remainder.is_empty() || remainder.len() == arg_members.len() {
        arg_type.clone()
    } else {
        PhpType::union(remainder)
    }
}

/// How many arguments the `wrapper_name<…>` in a parameter hint takes, or 1
/// when the hint holds no such generic.
///
/// The wrapper can sit inside a union (`ArrayIterator`'s constructor takes
/// `array<TKey, TValue>|object`), and reading the arity off the union
/// itself would count one argument and bind the key template to the value
/// type.
pub(super) fn wrapper_arity(param_hint: Option<&PhpType>, wrapper_name: &str) -> usize {
    let wrapper_short = crate::util::short_name(wrapper_name);
    let find = |ty: &PhpType| -> Option<usize> {
        let members: &[PhpType] = match ty.kind() {
            TypeKind::Union(members) => members,
            _ => std::slice::from_ref(ty),
        };
        members
            .iter()
            .find_map(|m| match m.unwrap_nullable().kind() {
                TypeKind::Generic(g)
                    if crate::util::short_name(&g.name).eq_ignore_ascii_case(wrapper_short) =>
                {
                    Some(g.args.len())
                }
                _ => None,
            })
    };
    match param_hint {
        Some(hint) => find(hint).unwrap_or(match hint.kind() {
            TypeKind::Generic(g) => g.args.len(),
            _ => 1,
        }),
        None => 1,
    }
}

/// Whether a generic hint names `tpl_name` as one of its own arguments.
///
/// The flat case (`array<TKey, TValue>`) is the positional extractor's
/// business — it knows the key/value arity quirks — so structural
/// unification stays out of its way.
pub(super) fn names_template_directly(hint: &PhpType, tpl_name: &str) -> bool {
    matches!(hint.kind(), TypeKind::Generic(g)
        if g.args.iter().any(|a| matches!(a.kind(), TypeKind::Named(n) if &**n == tpl_name)))
}

/// Generalize a literal bound to a template of an object type.
///
/// An object outlives the call that shaped it, so `new Box(42)` is a
/// `Box<int>` that can hold any int later rather than a `Box<42>`, the way
/// PHPStan generalizes it; a `@phpstan-self-out self<T>` result is the same
/// kind of type. A bound that is itself a scalar (`@template T of 'a'|'b'`,
/// `of int`) says the literal is the point, except `array-key`, which only
/// says the value can index an array.
pub(crate) fn generalize_object_template_arg(ty: &PhpType, bound: Option<&PhpType>) -> PhpType {
    let keeps_literals = bound.is_some_and(|bound| {
        !bound.is_array_key()
            && bound.union_members().iter().all(|member| {
                member.is_string_subtype()
                    || member.is_int_subtype()
                    || member.is_float_subtype()
                    || member.is_bool()
                    || member.is_true()
                    || member.is_false()
                    || matches!(member.kind(), TypeKind::Named(n)
                        if n.eq_ignore_ascii_case("scalar") || n.eq_ignore_ascii_case("numeric"))
            })
    });
    if keeps_literals {
        ty.clone()
    } else {
        ty.widen_scalar_literals()
    }
}

#[cfg(test)]
mod class_template_sub_tests {
    use std::sync::Arc;

    use super::build_call_template_subs;
    use crate::atom::atom;
    use crate::php_type::PhpType;
    use crate::type_engine::resolver::ResolutionCtx;
    use crate::types::ClassInfo;

    #[test]
    fn unresolved_self_like_receivers_do_not_apply_class_defaults() {
        let owner = ClassInfo {
            name: atom("PendingRequest"),
            template_params: vec![atom("TAsync")],
            template_param_defaults: [(atom("TAsync"), PhpType::parse("false"))]
                .into_iter()
                .collect(),
            ..ClassInfo::default()
        };
        let classes = Vec::new();
        let class_loader = |_: &str| -> Option<Arc<ClassInfo>> { None };
        let ctx = ResolutionCtx {
            current_class: None,
            all_classes: &classes,
            content: "",
            cursor_offset: 0,
            class_loader: &class_loader,
            backend: None,
            laravel_macro_this_resolver: None,
            resolved_class_cache: None,
            function_loader: None,
            scope_var_resolver: None,
            is_in_static_method: false,
            preserve_static: false,
        };

        assert!(
            build_call_template_subs(&owner, "send", &[], Some(&PhpType::parse("static")), &ctx,)
                .is_empty()
        );
        assert!(
            build_call_template_subs(
                &owner,
                "send",
                &[],
                Some(&PhpType::parse("PendingRequest<static>")),
                &ctx,
            )
            .is_empty()
        );
    }
}
