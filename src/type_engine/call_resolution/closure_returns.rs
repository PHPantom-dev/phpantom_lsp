//! Closure return types: inferring what a closure argument returns from
//! its declared type, its `yield`s or a walk of its body, for binding the
//! templates of the callable parameter it is passed to.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::{Expression, Program, Statement};

use crate::Backend;
use crate::atom::{Atom, atom};
use crate::parser::ParsedFile;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::{Loaders, ResolutionCtx, with_isolated_chain_cache};
use crate::type_engine::variable::forward_walk::{
    ForwardWalkCtx, ScopeState, WalkFlow, suspend_diagnostic_scope, walk_body_forward,
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
            crate::class_lookup::is_subtype_of_typed(body, &declared, ctx.file.class_loader)
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
    /// otherwise).  A full closure sees only what it captures of the scope
    /// around it.
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
                                ctx.file.class_loader,
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
        if !is_arrow {
            return resolve_full_closure_body_type(closure_text, typed_params, ctx);
        }

        let untyped_arrow = typed_params.is_empty();
        if untyped_arrow && let Some(ty) = Self::resolve_arg_text_to_type(first_body, ctx) {
            return Some(ty);
        }

        let param_types = seed_param_types(typed_params, ctx);
        let param_aware_resolver = |name: &str| -> Vec<ResolvedType> {
            if let Some(types) = param_types.get(name) {
                return types.clone();
            }
            outer_types(name, ctx.scope_var_resolver, ctx)
        };
        // The untyped arrow function's body was just tried with nothing
        // else to seed it, so asking again would get the same answer.
        if !untyped_arrow
            && let Some(ty) =
                Self::resolve_arg_text_to_type(first_body, &scoped_ctx(&param_aware_resolver, ctx))
        {
            return Some(PhpType::union(vec![ty]));
        }
        // The text resolver only knows the expression shapes it has a rule
        // for, so a body like `$carry && $item > 0` leaves it empty-handed.
        // The shared expression pipeline answers for the rest.
        with_walked_closure(closure_text, &param_types, ctx, |scope, returns, fw_ctx| {
            let resolver = |name: &str| match scope.locals.get(&atom(name)) {
                Some(types) if !types.is_empty() => types.clone(),
                _ => param_aware_resolver(name),
            };
            resolve_returns_in_ast(returns, &resolver, &scope, fw_ctx)
        })
        .flatten()
    }
}

/// What the scope around a closure holds for `name`: the outer scope
/// resolver's answer when there is one, assignment scanning otherwise
/// (see `resolve_variable_fallback`).
fn outer_types(
    name: &str,
    outer: crate::type_engine::resolver::ScopeVarResolverFn<'_>,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    match outer {
        Some(outer) => outer(name),
        None => {
            crate::type_engine::variable::resolution::resolve_variable_types(ctx.var_query(name))
        }
    }
}

/// `ctx` with `resolver` answering its variables.
fn scoped_ctx<'b>(
    resolver: &'b dyn Fn(&str) -> Vec<ResolvedType>,
    ctx: &ResolutionCtx<'b>,
) -> ResolutionCtx<'b> {
    ResolutionCtx {
        laravel_macro_this_resolver: ctx.laravel_macro_this_resolver,
        resolved_class_cache: ctx.resolved_class_cache,
        function_loader: ctx.function_loader,
        scope_var_resolver: Some(resolver),
        is_in_static_method: ctx.is_in_static_method,
        preserve_static: ctx.preserve_static,
        ..ResolutionCtx::new(ctx.file, ctx.current_class, ctx.cursor_offset)
    }
}

/// Pre-resolve each closure parameter's type to its classes, so a variable
/// lookup in the body is a cheap map lookup.
fn seed_param_types(
    typed_params: Vec<(String, PhpType)>,
    ctx: &ResolutionCtx<'_>,
) -> HashMap<String, Vec<ResolvedType>> {
    let owning_class_fqn = ctx.current_class.map(ClassInfo::fqn);
    let owning_class_name = owning_class_fqn.as_deref().unwrap_or("");
    let seed_one = |ty: PhpType| -> Vec<ResolvedType> {
        let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
            &ty,
            owning_class_name,
            ctx.file.all_classes,
            ctx.file.class_loader,
        );
        if classes.is_empty() {
            vec![ResolvedType::from_type_string(ty)]
        } else {
            ResolvedType::from_classes_with_hint(classes, ty)
        }
    };
    typed_params
        .into_iter()
        .map(|(name, ty)| {
            // Each alternative of a union is seeded on its own so it
            // keeps its own generic arguments. Two instantiations of the
            // same class (`Builder<A>|Builder<B>`) resolve to one class,
            // and a single entry could only carry the whole union as its
            // type string — leaving a `@return T` on the class with no
            // instantiation to substitute from.
            let resolved = match ty.kind() {
                TypeKind::Union(members) => members.iter().cloned().flat_map(&seed_one).collect(),
                _ => seed_one(ty),
            };
            (name, resolved)
        })
        .collect()
}

/// [`Backend::resolve_closure_body_type`] for a full (`function`) closure,
/// with its parameters typed as `typed_params`.
///
/// Read once per [`ClosureTextKey`] while [`CLOSURE_TEXT_MEMO`] is active.
fn resolve_full_closure_body_type(
    closure_text: &str,
    typed_params: Vec<(String, PhpType)>,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let captured = captured_variables(closure_text, ctx);
    let key = ClosureTextKey {
        text: closure_text.trim().into(),
        file: (ctx.file.content.as_ptr() as usize, ctx.file.content.len()),
        cursor_offset: ctx.cursor_offset,
        imports: ctx
            .file
            .source_imports
            .map_or(0, |imports| std::ptr::from_ref(imports) as usize),
        class: ctx.current_class.map(ClassInfo::fqn),
        inference_context: crate::type_engine::call_resolution::body_inference_context(),
        static_context: (
            ctx.is_in_static_method,
            ctx.preserve_static,
            ctx.laravel_macro_this_resolver.is_some(),
        ),
        params: typed_params.clone().into_boxed_slice(),
        captured: captured
            .iter()
            .map(|(_, types)| (!types.is_empty()).then(|| ResolvedType::types_joined(types)))
            .collect(),
    };
    if let Some(hit) = CLOSURE_TEXT_MEMO.with(|cell| {
        cell.borrow()
            .as_ref()
            .and_then(|memo| memo.returns.get(&key).cloned())
    }) {
        return hit;
    }

    let param_types = seed_param_types(typed_params, ctx);
    let inferred =
        with_walked_closure(closure_text, &param_types, ctx, |scope, returns, fw_ctx| {
            // The body was walked from the parameters alone; what a variable
            // it does not assign holds is the parameter's seed, or what the
            // closure captured.
            let resolver = |name: &str| -> Vec<ResolvedType> {
                if let Some(types) = scope.locals.get(&atom(name))
                    && !types.is_empty()
                {
                    return types.clone();
                }
                if let Some(types) = param_types.get(name) {
                    return types.clone();
                }
                captured
                    .iter()
                    .find(|(captured, _)| *captured == name)
                    .map(|(_, types)| types.clone())
                    .unwrap_or_default()
            };
            if returns.is_empty() {
                return None;
            }
            let content = fw_ctx.file.content;
            let param_ctx = scoped_ctx(&resolver, ctx);
            if let Some(members) = returns
                .iter()
                .map(|ret| {
                    let span = (*ret)?.span();
                    Backend::resolve_arg_text_to_type(
                        &content[span.start.offset as usize..span.end.offset as usize],
                        &param_ctx,
                    )
                })
                .collect::<Option<Vec<_>>>()
            {
                return Some(PhpType::union(members));
            }
            // The text resolver only knows the expression shapes it has a rule
            // for, so a body like `$carry && $item > 0` leaves it empty-handed.
            // The shared expression pipeline answers for the rest.
            resolve_returns_in_ast(returns, &resolver, &scope, fw_ctx)
        })
        .flatten();

    CLOSURE_TEXT_MEMO.with(|cell| {
        if let Some(memo) = cell.borrow_mut().as_mut() {
            memo.returns.insert(key, inferred.clone());
        }
    });
    inferred
}

/// The variables of the scope around a full closure its body can read,
/// with what that scope holds for each: what its `use` clause captures,
/// `$this` unless it is `static`, and the superglobals.  Only those the
/// body names are read.
fn captured_variables<'t>(
    closure_text: &'t str,
    ctx: &ResolutionCtx<'_>,
) -> Vec<(&'t str, Vec<ResolvedType>)> {
    let is_static = closure_text
        .trim_start()
        .strip_prefix("static")
        .is_some_and(|rest| rest.starts_with(char::is_whitespace));
    let implicit = (!is_static)
        .then_some("$this")
        .into_iter()
        .chain(crate::scope_collector::AUTO_GLOBALS)
        .filter(|name| closure_text.contains(name));
    crate::completion::source::helpers::extract_closure_use_vars_from_text(closure_text)
        .unwrap_or_default()
        .into_iter()
        .chain(implicit)
        .map(|name| (name, outer_types(name, ctx.scope_var_resolver, ctx)))
        .collect()
}

thread_local! {
    /// When `Some`, the closure literals parsed for
    /// [`Backend::resolve_closure_body_type`] and what each was found to
    /// return.  Activated with the rest of the request-scoped type-engine
    /// memos.
    ///
    /// Binding a template from a closure argument reads the closure's body,
    /// and reading that body binds the templates of the calls in it, so
    /// without this a closure nested `n` deep in call arguments would be
    /// read again for every enclosing closure read.
    static CLOSURE_TEXT_MEMO: RefCell<Option<ClosureTextMemo>> = const { RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    static TEST_CLOSURE_WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// See [`CLOSURE_TEXT_MEMO`].
#[derive(Default)]
pub(crate) struct ClosureTextMemo {
    /// Each closure literal's parse, by its text.  Kept for the whole pass
    /// so a closure nested in one is read from the same source at the
    /// same address every time, which is how [`ClosureTextKey::file`]
    /// tells one such source from another.
    programs: HashMap<Box<str>, Option<Arc<ParsedFile>>>,
    returns: HashMap<ClosureTextKey, Option<PhpType>>,
}

/// What identifies one reading of a full closure literal: its text, where
/// it is read (the source text's address, the cursor, the `use` imports
/// and the class, which decide how the names written in it resolve), the
/// body-inference context it is read under, what its parameters are typed
/// as, and what it captures.
#[derive(PartialEq, Eq, Hash)]
struct ClosureTextKey {
    text: Box<str>,
    file: (usize, usize),
    cursor_offset: u32,
    imports: usize,
    class: Option<Atom>,
    inference_context: u64,
    /// Whether the cursor is in a static method, whether `static` is kept,
    /// and whether a Laravel macro's `$this` can be resolved.
    static_context: (bool, bool, bool),
    params: Box<[(String, PhpType)]>,
    captured: Box<[Option<PhpType>]>,
}

/// The guard [`with_closure_text_memo`] hands back.
pub(crate) type ClosureTextMemoGuard = crate::type_engine::MemoGuard<ClosureTextMemo>;

/// Activate [`CLOSURE_TEXT_MEMO`] for the current thread.  Nested
/// activation is a no-op.
pub(crate) fn with_closure_text_memo(scope: crate::type_engine::MemoScope) -> ClosureTextMemoGuard {
    crate::type_engine::activate_memo(&CLOSURE_TEXT_MEMO, scope)
}

/// Resolve every `return` expression of a walked closure body through the
/// shared expression pipeline, with `resolver` answering its variables.
///
/// Returns `None` when a `return` is bare or resolves to nothing.
fn resolve_returns_in_ast(
    returns: &[Option<&Expression<'_>>],
    resolver: &dyn Fn(&str) -> Vec<ResolvedType>,
    scope: &ScopeState,
    fw_ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let mut members = Vec::with_capacity(returns.len());
    for ret in returns {
        let expr = (*ret)?;
        let var_ctx = fw_ctx.var_ctx_for_with_scope(
            "$__return",
            expr.span().start.offset,
            resolver,
            Some(scope.proofs()),
        );
        members.push(
            crate::type_engine::variable::iterable_elements::resolve_expression_type(
                expr, &var_ctx,
            )?,
        );
    }
    Some(PhpType::union(members))
}

/// Walk a closure or arrow function literal's body with the parameters
/// seeded from `param_types`, and hand `f` the resulting scope together
/// with every `return` expression the body reaches (`None` for a bare
/// `return;`). An arrow function has nothing to walk and returns its body
/// expression.
///
/// Resolving a return expression's text on its own knows nothing about
/// the body's locals, so `$p = make(); return $p;` could not see `$p`, and
/// a body that reassigns a parameter before returning it (`$result['a'] =
/// (string) $result['a']; return $result;`) would still report the
/// parameter's original shape. Running the body through
/// [`walk_body_forward`] answers with what the body actually leaves
/// behind, the same as any other consumer of the forward walker.
///
/// A literal written in the file being resolved is read from that file's
/// own AST, so the closures nested in it are read from the same source
/// too, and [`CLOSURE_TEXT_MEMO`] answers for each of them however many
/// closures around it are read.  Text found nowhere in the file is parsed
/// as a program of its own.
///
/// Returns `None` when `closure_text` is neither.
fn with_walked_closure<R>(
    closure_text: &str,
    param_types: &HashMap<String, Vec<ResolvedType>>,
    ctx: &ResolutionCtx<'_>,
    f: impl FnOnce(ScopeState, &[Option<&Expression<'_>>], &ForwardWalkCtx<'_>) -> R,
) -> Option<R> {
    let trimmed = closure_text.trim().trim_end_matches(';');

    // The body is walked with the call site's seeds, which need not be
    // what the walk of the file around it gives its parameters, and a
    // body parsed on its own has offsets unrelated to (and that may
    // numerically collide with) the real file's.  Either way neither the
    // diagnostic scope cache nor the chain-resolution cache may read
    // from, or record into, the active ones while this walk runs. See
    // `out_param::read_out_type`, which walks another file's body for the
    // same reason.
    let _isolated = (suspend_diagnostic_scope(), with_isolated_chain_cache());

    let mut f = Some(f);
    if let Some(start) = closure_offset_in_file(trimmed, ctx) {
        let end = start + trimmed.len() as u32;
        let walked = crate::parser::with_parsed_program(
            ctx.file.content,
            "closure_body_return_narrowing",
            |program, _| {
                let function = find_function_literal(program, start, end)?;
                walk_function_literal(function, ctx.file, start, param_types, ctx, |s, r, c| {
                    f.take().map(|f| f(s, r, c))
                })
            },
        );
        if walked.is_some() || f.is_none() {
            return walked.flatten();
        }
    }

    with_closure_program(trimmed, |program, content| {
        let function = program.statements.iter().find_map(|stmt| {
            let Statement::Expression(expr_stmt) = stmt else {
                return None;
            };
            let Expression::Assignment(assignment) = expr_stmt.expression else {
                return None;
            };
            match assignment.rhs {
                Expression::Closure(_) | Expression::ArrowFunction(_) => Some(assignment.rhs),
                _ => None,
            }
        })?;
        let file = ctx.file.with_source(content, ctx.file.all_classes);
        walk_function_literal(function, file, 0, param_types, ctx, f.take()?)
    })
}

/// Where `literal` is written in the file `ctx` resolves, as its start
/// offset: the occurrence nearest the cursor when the same text is written
/// more than once.  Every occurrence reads alike, since its body sees only
/// what the call site seeds and captures, but the nearest one sits in the
/// same namespace and class as the call.
fn closure_offset_in_file(literal: &str, ctx: &ResolutionCtx<'_>) -> Option<u32> {
    let cursor = ctx.cursor_offset as usize;
    ctx.file
        .content
        .match_indices(literal)
        .map(|(offset, _)| offset)
        .min_by_key(|&offset| offset.abs_diff(cursor))
        .and_then(|offset| u32::try_from(offset).ok())
}

/// The closure or arrow function in `program` spanning `start..end`.
fn find_function_literal<'ast, 'arena>(
    program: &'ast Program<'arena>,
    start: u32,
    end: u32,
) -> Option<&'ast Expression<'arena>> {
    use mago_syntax::walker::Walker;

    /// Descends only into the statements and expressions that enclose the
    /// span it looks for.
    struct Finder {
        start: u32,
        end: u32,
    }

    impl Finder {
        fn encloses(&self, node: &impl HasSpan) -> bool {
            let span = node.span();
            span.start.offset <= self.start && self.end <= span.end.offset
        }
    }

    impl<'ast, 'arena> Walker<'ast, 'arena, Option<&'ast Expression<'arena>>> for Finder {
        crate::ast_walk::iterative_binary_walk!(Option<&'ast Expression<'arena>>);

        fn walk_statement(
            &self,
            statement: &'ast Statement<'arena>,
            found: &mut Option<&'ast Expression<'arena>>,
        ) {
            if found.is_none() && self.encloses(statement) {
                mago_syntax::walker::walk_statement(self, statement, found);
            }
        }

        fn walk_expression(
            &self,
            expression: &'ast Expression<'arena>,
            found: &mut Option<&'ast Expression<'arena>>,
        ) {
            if found.is_some() || !self.encloses(expression) {
                return;
            }
            let span = expression.span();
            if span.start.offset == self.start
                && span.end.offset == self.end
                && matches!(
                    expression,
                    Expression::Closure(_) | Expression::ArrowFunction(_)
                )
            {
                *found = Some(expression);
                return;
            }
            mago_syntax::walker::walk_expression(self, expression, found);
        }
    }

    let mut found = None;
    Finder { start, end }.walk_program(program, &mut found);
    found
}

/// [`with_walked_closure`] for the literal `function`, written at `offset`
/// in `file`.
fn walk_function_literal<R>(
    function: &Expression<'_>,
    file: crate::type_engine::resolver::FileCtx<'_>,
    offset: u32,
    param_types: &HashMap<String, Vec<ResolvedType>>,
    ctx: &ResolutionCtx<'_>,
    f: impl FnOnce(ScopeState, &[Option<&Expression<'_>>], &ForwardWalkCtx<'_>) -> R,
) -> Option<R> {
    let dummy_class;
    let current_class = match ctx.current_class {
        Some(cc) => cc,
        None => {
            dummy_class = crate::class_lookup::class_context_placeholder(file.content, offset);
            &dummy_class
        }
    };

    let flow = WalkFlow::default();
    let fw_ctx = ForwardWalkCtx {
        loaders: Loaders::with_function(ctx.function_loader),
        resolved_class_cache: ctx.resolved_class_cache,
        ..ForwardWalkCtx::new(file, current_class, u32::MAX, &flow)
    };

    let mut scope = ScopeState::new();
    for (name, types) in param_types {
        scope.seed(name, types.clone());
    }

    let returns: Vec<Option<&Expression<'_>>> = match function {
        Expression::Closure(closure) => {
            #[cfg(test)]
            TEST_CLOSURE_WALKS.with(|count| count.set(count.get() + 1));
            walk_body_forward(closure.body.statements.iter(), &mut scope, &fw_ctx);
            let mut found = Vec::new();
            crate::return_collection::collect_returns(closure.body.statements.iter(), &mut found);
            found.into_iter().map(|(expr, ..)| expr).collect()
        }
        Expression::ArrowFunction(arrow) => vec![Some(arrow.expression)],
        _ => return None,
    };

    Some(f(scope, &returns, &fw_ctx))
}

/// Run `f` on `closure_text` parsed as the value assigned in a program of
/// its own, `<?php $__closure = …;`.
///
/// While [`CLOSURE_TEXT_MEMO`] is active each text is parsed once and kept
/// for the pass.  A parser panic, or one in `f`, is logged and answered
/// with `R::default()`.
fn with_closure_program<R: Default>(
    closure_text: &str,
    f: impl FnOnce(&Program<'_>, &str) -> R,
) -> R {
    const METHOD: &str = "closure_body_return_narrowing";
    let wrap = || format!("<?php $__closure = {closure_text};");
    let kept = CLOSURE_TEXT_MEMO.with(|cell| {
        let mut memo = cell.borrow_mut();
        let memo = memo.as_mut()?;
        if let Some(parsed) = memo.programs.get(closure_text) {
            return Some(parsed.clone());
        }
        let parsed = crate::parser::parse_to_owned(Arc::new(wrap()), METHOD).map(Arc::new);
        memo.programs.insert(closure_text.into(), parsed.clone());
        Some(parsed)
    });
    match kept {
        None => crate::parser::with_parsed_program(&wrap(), METHOD, f),
        Some(None) => R::default(),
        Some(Some(parsed)) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f(parsed.program(), parsed.content())
        }))
        .unwrap_or_else(|_| {
            tracing::error!("PHPantom: parser panicked in {}", METHOD);
            R::default()
        }),
    }
}

#[cfg(test)]
#[path = "closure_returns_tests.rs"]
mod tests;
