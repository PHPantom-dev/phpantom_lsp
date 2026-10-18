//! Closure return types: inferring what a closure argument returns from
//! its declared type, its `yield`s or a walk of its body, for binding the
//! templates of the callable parameter it is passed to.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::{Expression, Statement};

use crate::Backend;
use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::{Loaders, ResolutionCtx, with_isolated_chain_cache};
use crate::type_engine::variable::forward_walk::{
    ForwardWalkCtx, ScopeState, suspend_diagnostic_scope, suspend_return_edges, walk_body_forward,
};
use crate::types::*;

impl Backend {
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
    /// The body (an arrow `fn() => EXPR`, or every `return EXPR;` of a full
    /// closure body, joined) is resolved through the shared type resolver.  A closure really returns what its body produces narrowed
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
            // A body that resolves to `mixed` says nothing about the
            // template, and binding it hides the template's own bound
            // (`@template TNewKey of array-key`), which is strictly more
            // informative.  Leave the template unbound instead.
            Self::resolve_closure_body_type(arg_text, param_seeds, ctx).filter(|ty| !ty.is_mixed())
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
        let declared = ctx.written_names().resolve_hint(&declared);
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
        Self::resolve_closure_body_type(arg_text, &[Some(param_type.clone())], ctx)
    }

    /// Resolve what a closure's body returns, seeding the closure's own
    /// parameters into variable resolution.
    ///
    /// An arrow function returns its single body expression.  A full
    /// closure returns the union of every `return` it reaches, each
    /// resolved after its body has been walked so locals assigned along
    /// the way are known; one bare or unresolvable `return` leaves the
    /// whole body unknown, since joining the rest would claim a narrower
    /// type than the closure really produces.
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
        param_seeds: &[Option<PhpType>],
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        // Also rejects text that is not a closure literal, or a full
        // closure with no `return EXPR;`, before anything is parsed.
        let first_body =
            crate::completion::source::helpers::extract_closure_body_expr_text(closure_text)?;
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
                    let declared = declared.map(|t| ctx.written_names().resolve_hint(&t));
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
        let is_arrow = crate::completion::source::helpers::closure_literal_parts(closure_text)
            .is_some_and(|(is_arrow, _)| is_arrow);
        if is_arrow && typed_params.is_empty() {
            return Self::resolve_arg_text_to_type(first_body, ctx);
        }

        // Pre-resolve each typed parameter to its classes so the
        // injected resolver is a cheap map lookup.
        let owning_class_fqn = ctx.current_class.map(ClassInfo::fqn);
        let owning_class_name = owning_class_fqn.as_deref().unwrap_or("");
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

        let (walked_locals, returns) = if is_arrow {
            (None, vec![Some(first_body.to_owned())])
        } else {
            let (locals, returns) = walk_closure_body_locals(closure_text, &param_types, ctx)?;
            if returns.is_empty() {
                return None;
            }
            (Some(locals), returns)
        };

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
                None => crate::type_engine::variable::resolution::resolve_variable_types(
                    ctx.var_query(name),
                ),
            }
        };

        let param_ctx = ResolutionCtx {
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            source_class_loader: ctx.source_class_loader,
            source_imports: ctx.source_imports,
            backend: ctx.backend,
            laravel_macro_this_resolver: ctx.laravel_macro_this_resolver,
            resolved_class_cache: ctx.resolved_class_cache,
            function_loader: ctx.function_loader,
            scope_var_resolver: Some(&param_aware_resolver),
            is_in_static_method: ctx.is_in_static_method,
            preserve_static: ctx.preserve_static,
        };
        let mut members = Vec::with_capacity(returns.len());
        for ret in &returns {
            members.push(Self::resolve_arg_text_to_type(ret.as_deref()?, &param_ctx)?);
        }
        Some(PhpType::union(members))
    }
}

/// Walk a full closure literal's own body with the shared forward walker,
/// seeded with the parameter types the call site hands it, and return the
/// scope those statements leave behind together with the text of every
/// `return` the body reaches (`None` for a bare `return;`).
///
/// Resolving a return expression's text on its own knows nothing about
/// the body's locals, so `$p = make(); return $p;` could not see `$p`, and
/// a body that reassigns a parameter before returning it (`$result['a'] =
/// (string) $result['a']; return $result;`) would still report the
/// parameter's original shape. Re-parsing the closure in isolation and
/// running its body through [`walk_body_forward`] answers with what the
/// body actually leaves behind, the same as any other consumer of the
/// forward walker.
///
/// Returns `None` when `closure_text` is not a full (`function`) closure
/// literal — an arrow function's body is a single expression with nothing
/// to walk, so the caller's raw-seed resolver already answers correctly.
fn walk_closure_body_locals(
    closure_text: &str,
    param_types: &HashMap<String, Vec<ResolvedType>>,
    ctx: &ResolutionCtx<'_>,
) -> Option<(
    crate::type_engine::variable::forward_walk::Locals,
    Vec<Option<String>>,
)> {
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
                source_class_loader: ctx.source_class_loader,
                source_imports: ctx.source_imports,
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

            let mut found = Vec::new();
            crate::return_collection::collect_returns(closure.body.statements.iter(), &mut found);
            let returns = found
                .into_iter()
                .map(|(expr, ..)| {
                    expr.map(|e| {
                        let span = e.span();
                        content[span.start.offset as usize..span.end.offset as usize].to_owned()
                    })
                })
                .collect();

            Some((scope.locals, returns))
        },
    )
}
