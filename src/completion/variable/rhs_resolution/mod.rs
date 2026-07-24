/// Right-hand-side expression resolution for variable assignments.
///
/// This module resolves the type of the right-hand side of an assignment
/// (`$var = <expr>`) to zero or more [`ResolvedType`] values.  It handles:
///
///   - Scalar literals: `1` → `int`, `'hello'` → `string`, etc.
///   - Array literals: `[new Foo()]` → `list<Foo>`,
///     `['a' => 1]` → `array{a: int}`
///   - `new ClassName(…)` → the instantiated class
///   - Array access: `$arr[0]` → generic element type,
///     `$arr['key']` → array shape value type,
///     `$arr['key'][0]` → chained bracket access
///   - Function calls: `someFunc()` → return type
///   - Method calls: `$this->method()`, `$obj->method()` → return type
///   - Static calls: `ClassName::method()` → return type
///   - Property access: `$this->prop`, `$obj->prop` → property type
///   - Match expressions: union of all arm types
///   - Ternary / null-coalescing: union of both branches
///   - Clone: `clone $expr` → preserves the cloned expression's type
///
/// The entry point is [`resolve_rhs_expression`], which dispatches to
/// specialised helpers based on the AST node kind.
/// The only caller is
/// [`check_expression_for_assignment`](super::resolution::check_expression_for_assignment)
/// in `variable_resolution.rs`.
///
/// The dispatch logic lives here; specialised resolution is spread
/// across sibling files:
///
/// - [`instantiation`]: `new ClassName(…)` and constructor template
///   substitution.
/// - [`array_access`]: `$arr[0]` / `$arr['key']` generic element / shape
///   value resolution.
/// - [`calls`]: function, method, and static call return-type
///   resolution, plus function-level `@template` substitution.
/// - [`property_access`]: `$this->prop` / `$obj->prop` resolution and
///   the `find_*_this_property_assignment*` scanners.
use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{Atom, AtomMap, bytes_to_str};
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ResolvedType};

use crate::completion::resolver::{Loaders, VarResolutionCtx};
use crate::completion::type_resolution;
use crate::util::strip_fqn_prefix;

mod array_access;
mod calls;
mod instantiation;
mod property_access;

use array_access::resolve_rhs_array_access;
use calls::resolve_rhs_call;
use instantiation::resolve_rhs_instantiation;
use property_access::resolve_rhs_property_access;

pub(crate) use array_access::{class_string_inner_binding, insert_or_union};
pub(crate) use calls::{
    build_function_template_subs, infer_closure_literal_type, is_array_like_wrapper,
};
pub(crate) use instantiation::{
    TemplateBindingMode, classify_template_binding, remap_inherited_ctor_subs, type_contains_name,
};

/// Resolve a variable's type for use in RHS expression evaluation.
///
/// When `ctx.scope_var_resolver` is set (forward-walker RHS
/// resolution), the scope resolver is consulted first.  This reads
/// directly from the forward walker's in-progress `ScopeState`,
/// avoiding re-entry into the forward walk.  Otherwise falls back to
/// [`resolve_variable_types`] (which itself checks the diagnostic
/// scope cache and then delegates to the forward walker).
fn resolve_var_types(
    var_name: &str,
    ctx: &VarResolutionCtx<'_>,
    cursor_offset: u32,
) -> Vec<ResolvedType> {
    // ── Forward-walker fast path ────────────────────────────────
    // When a scope_var_resolver is available, read variable types
    // directly from the forward walker's ScopeState.  This avoids
    // the feedback loop where the backward scanner hits the
    // (incomplete) diagnostic scope cache during the forward walk.
    if let Some(resolver) = ctx.scope_var_resolver {
        let prefixed = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        let from_scope = resolver(&prefixed);
        if !from_scope.is_empty() {
            return from_scope;
        }
        // The forward walker is the authority for variable types.
        // If the variable isn't in its ScopeState, it hasn't been
        // assigned yet at this point in the walk.  Falling through
        // to `resolve_variable_types` would re-enter the forward
        // walker, causing O(N²) blowup
        // or stack overflow.  Return empty so the RHS resolver
        // treats the variable as unresolved.
        return vec![];
    }

    super::resolution::resolve_variable_types(
        var_name,
        ctx.current_class,
        ctx.all_classes,
        ctx.content,
        cursor_offset,
        ctx.class_loader,
        Loaders::with_function(ctx.function_loader()),
    )
}

// ── Match-arm narrowing override ────────────────────────────────────
//
// When resolving the RHS of a `match(true)` arm like:
//
//   match (true) {
//       $model instanceof Customer => $model->country,
//       …
//   }
//
// the arm expression `$model->country` must resolve `$model` as
// `Customer`, not its declared parameter type `?Model`.  The normal
// variable resolution pipeline doesn't know about the match-arm
// condition, so we propagate narrowings via the `match_arm_narrowing`
// field on `VarResolutionCtx`.  When entering a `match(true)` arm
// body, a new context is created with the narrowed types; callers in
// `resolve_rhs_method_call_inner` and `resolve_rhs_property_access`
// consult `ctx.match_arm_narrowing` when the object is a bare variable.

/// Extract instanceof narrowings from a `match(true)` arm's conditions.
///
/// For each condition like `$var instanceof ClassName`, adds an entry
/// mapping `"$var"` → the resolved `ClassInfo` for `ClassName`.
/// Multiple conditions on the same arm are OR-merged (each condition
/// narrows a potentially different variable).
fn extract_match_arm_narrowings(
    expr_arm: &MatchExpressionArm<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> HashMap<String, Vec<ResolvedType>> {
    let mut overrides: HashMap<String, Vec<ResolvedType>> = HashMap::new();
    for condition in expr_arm.conditions.iter() {
        if let Some((var_name, mut class_type)) = extract_instanceof_pair(condition) {
            // Resolve the short class name to FQN so that downstream
            // comparisons and ResolvedType hints carry the fully-qualified name.
            if let PhpType::Named(ref name) = class_type
                && let Some(cls) = (ctx.class_loader)(name)
            {
                class_type = PhpType::Named(cls.fqn().to_string());
            }
            let resolved = type_resolution::type_hint_to_classes_typed(
                &class_type,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if !resolved.is_empty() {
                let results = ResolvedType::from_classes_with_hint(resolved, class_type);
                overrides
                    .entry(var_name)
                    .and_modify(|existing| ResolvedType::extend_unique(existing, results.clone()))
                    .or_insert(results);
            }
        }
    }
    overrides
}

/// Extract `($var_name, ClassName)` from `$var instanceof ClassName`.
fn extract_instanceof_pair(expr: &Expression<'_>) -> Option<(String, PhpType)> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if let Expression::Binary(bin) = expr
        && bin.operator.is_instanceof()
    {
        // LHS: the variable
        let var_name = match bin.lhs {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => return None,
        };
        // RHS: the class name
        let class_type = match bin.rhs {
            Expression::Identifier(ident) => {
                PhpType::Named(bytes_to_str(ident.value()).to_string())
            }
            Expression::Self_(_) => PhpType::Named("self".to_string()),
            Expression::Static(_) => PhpType::Named("static".to_string()),
            Expression::Parent(_) => PhpType::Named("parent".to_string()),
            _ => return None,
        };
        Some((var_name, class_type))
    } else {
        None
    }
}

/// Create a `ResolvedType` from a `PhpType`, looking up class info when the type names a class.
///
/// When the `PhpType` has a `base_name()` that resolves to a known class, returns
/// `ResolvedType::from_both(ty, class)`. Otherwise returns `ResolvedType::from_type_string(ty)`.
fn resolved_type_with_lookup(
    ty: PhpType,
    _current_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> ResolvedType {
    if let Some(base) = ty.base_name() {
        let base = base.strip_prefix('\\').unwrap_or(base);
        // Don't try to look up scalars/pseudo-types
        if !crate::php_type::is_keyword_type(base) {
            // Try in-file classes first
            let cls = crate::class_lookup::find_class_by_name(all_classes, base)
                .map(|arc| arc.as_ref().clone())
                .or_else(|| class_loader(base).map(Arc::unwrap_or_clone));
            if let Some(class) = cls {
                return ResolvedType::from_both(ty, class);
            }
        }
    }
    ResolvedType::from_type_string(ty)
}

/// Resolve a right-hand-side expression to zero or more
/// [`ResolvedType`] values.
///
/// This is the single place where an arbitrary PHP expression is
/// resolved to a type.  It handles scalars, array literals,
/// instantiations, calls, property access, match/ternary/null-coalesce,
/// clone, closures, generators, pipe, and bare variables.
///
/// Entries may have `class_info: None` (e.g. scalar literals, array
/// shapes).  Callers that need only class-backed results should
/// filter with [`ResolvedType::into_classes`].
///
/// Used by `check_expression_for_assignment` (for `$var = <expr>`),
/// `check_expression_for_raw_type` (for hover/diagnostics type strings),
/// and recursively by multi-branch constructs (match, ternary, `??`).
pub(in crate::completion) fn resolve_rhs_expression<'b>(
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    thread_local! {
        static RHS_EXPR_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    let depth = RHS_EXPR_DEPTH.with(|d| {
        let v = d.get() + 1;
        d.set(v);
        v
    });
    if depth > 100 {
        RHS_EXPR_DEPTH.with(|d| d.set(depth - 1));
        return vec![];
    }
    let result = resolve_rhs_expression_inner(expr, ctx);
    RHS_EXPR_DEPTH.with(|d| d.set(depth - 1));
    result
}

fn resolve_rhs_expression_inner<'b>(
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    match expr {
        // ── Scalar literals ─────────────────────────────────────────
        Expression::Literal(Literal::Integer(_)) => {
            vec![ResolvedType::from_type_string(PhpType::int())]
        }
        Expression::Literal(Literal::Float(_)) => {
            vec![ResolvedType::from_type_string(PhpType::float())]
        }
        Expression::Literal(Literal::String(_)) => {
            vec![ResolvedType::from_type_string(PhpType::string())]
        }
        Expression::Literal(Literal::True(_) | Literal::False(_)) => {
            vec![ResolvedType::from_type_string(PhpType::bool())]
        }
        Expression::Literal(Literal::Null(_)) => {
            vec![ResolvedType::from_type_string(PhpType::null())]
        }
        // ── Array literals ──────────────────────────────────────────
        Expression::Array(arr) => {
            let pt = super::raw_type_inference::infer_array_literal_raw_type(
                arr.elements.iter(),
                ctx,
                false,
            )
            .unwrap_or_else(PhpType::array);
            vec![ResolvedType::from_type_string(pt)]
        }
        Expression::LegacyArray(arr) => {
            let pt = super::raw_type_inference::infer_array_literal_raw_type(
                arr.elements.iter(),
                ctx,
                false,
            )
            .unwrap_or_else(PhpType::array);
            vec![ResolvedType::from_type_string(pt)]
        }
        Expression::Instantiation(inst) => resolve_rhs_instantiation(inst, ctx),
        // ── Anonymous class: `new class extends Foo { … }` ──────────
        // The parser stores these in `all_classes` with a synthetic
        // name `__anonymous@<offset>`.  Look it up by matching the
        // left-brace offset so the variable inherits the full
        // ClassInfo (parent class, traits, methods, etc.).
        Expression::AnonymousClass(anon) => {
            let start = anon.left_brace.start.offset;
            let name = format!("__anonymous@{}", start);
            if let Some(cls) = ctx.all_classes.iter().find(|c| c.name == name) {
                return ResolvedType::from_classes(vec![Arc::clone(cls)]);
            }
            vec![]
        }
        Expression::ArrayAccess(array_access) => {
            // Check if the scope has a narrowed type for this array
            // access (e.g. `$a["test"]` narrowed through null checks, or
            // `$config['class']` narrowed to `class-string<Foo>` by an
            // `is_a(..., true)` guard).  The completion/hover paths carry
            // a `scope_var_resolver`; the diagnostic path instead reads
            // the forward walker's snapshot cache, so both are consulted.
            if let Some(key) = crate::completion::types::narrowing::expr_to_subject_key(expr)
                && key.contains("[\"")
            {
                if let Some(resolver) = ctx.scope_var_resolver {
                    let from_scope = resolver(&key);
                    if !from_scope.is_empty() {
                        return from_scope;
                    }
                } else if super::forward_walk::is_diagnostic_scope_active()
                    && !super::forward_walk::is_building_scopes()
                    && let Some(from_scope) =
                        super::forward_walk::lookup_diagnostic_scope(&key, expr.span().start.offset)
                    && !from_scope.is_empty()
                {
                    return from_scope;
                }
            }
            resolve_rhs_array_access(array_access, expr, ctx)
        }
        Expression::Call(call) => resolve_rhs_call(call, expr, ctx),
        Expression::Access(access) => {
            // Check if the scope has a narrowed type for this property
            // access (e.g. `$a->foo` narrowed through if/elseif conditions).
            if let Some(resolver) = ctx.scope_var_resolver
                && let Some(key) = crate::completion::types::narrowing::expr_to_subject_key(expr)
                && key.contains("->")
            {
                let from_scope = resolver(&key);
                if !from_scope.is_empty() {
                    return from_scope;
                }
            }
            let result = resolve_rhs_property_access(access, ctx);
            // Apply property narrowing from enclosing if / ternary
            // conditions (instanceof checks) so that `$this->prop` inside
            // `if ($this->prop instanceof X)` or
            // `$this->prop instanceof X ? $this->prop->m() : …` resolves to
            // X instead of the declared property type.  The scope resolver
            // (when present) is tried first above; property paths are not
            // locals, so it returns nothing for them and we fall through to
            // this walk.
            if !result.is_empty()
                && let Some(key) = crate::completion::types::narrowing::expr_to_subject_key(expr)
                && key.contains("->")
            {
                let rctx = ctx.as_resolution_ctx();
                let mut classes: Vec<Arc<ClassInfo>> =
                    result.iter().filter_map(|r| r.class_info.clone()).collect();
                if !classes.is_empty() {
                    crate::completion::resolver::apply_property_narrowing(
                        &key,
                        ctx.current_class,
                        &rctx,
                        &mut classes,
                    );
                    // If narrowing changed the classes, return the narrowed result.
                    let original_names: Vec<&str> = result
                        .iter()
                        .filter_map(|r| r.class_info.as_ref().map(|c| c.name.as_str()))
                        .collect();
                    let narrowed_names: Vec<&str> =
                        classes.iter().map(|c| c.name.as_str()).collect();
                    if original_names != narrowed_names {
                        return ResolvedType::from_classes(classes);
                    }
                }
            }
            result
        }
        Expression::Parenthesized(p) => resolve_rhs_expression(p.expression, ctx),
        // ── Error-suppression prefix: `@expr` ───────────────────────
        // The `@` operator doesn't change the runtime type of the
        // expression, so resolve straight through to the operand.
        Expression::UnaryPrefix(unary) if unary.operator.is_error_control() => {
            resolve_rhs_expression(unary.operand, ctx)
        }
        Expression::Match(match_expr) => {
            let is_match_true = match_expr.expression.is_true();
            let mut combined = Vec::new();
            for arm in match_expr.arms.iter() {
                // For match(true) arms with instanceof conditions,
                // create a new context with narrowed variable types so
                // that property and method accesses in the arm expression
                // resolve against the narrowed class.
                let arm_ctx = if is_match_true {
                    if let MatchArm::Expression(expr_arm) = arm {
                        let overrides = extract_match_arm_narrowings(expr_arm, ctx);
                        if !overrides.is_empty() {
                            Some(ctx.with_match_arm_narrowing(overrides))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                let effective_ctx = arm_ctx.as_ref().unwrap_or(ctx);
                let arm_results = resolve_rhs_expression(arm.expression(), effective_ctx);
                ResolvedType::extend_unique(&mut combined, arm_results);
            }
            combined
        }
        Expression::Conditional(cond_expr) => {
            let mut combined = Vec::new();
            let then_expr = cond_expr.then.unwrap_or(cond_expr.condition);
            // Resolve each branch with the cursor positioned inside it so
            // that instanceof / guard narrowing from the ternary condition
            // applies to variable and property subjects within the branch.
            // Without this, `$x instanceof Foo ? $x->m() : null` would
            // resolve `$x->m()` against the un-narrowed type, the then
            // branch would fail, and the whole ternary would collapse to
            // the else branch instead of unioning both.
            let then_ctx = ctx.with_cursor_offset(then_expr.span().start.offset);
            ResolvedType::extend_unique(
                &mut combined,
                resolve_rhs_expression(then_expr, &then_ctx),
            );
            let else_ctx = ctx.with_cursor_offset(cond_expr.r#else.span().start.offset);
            ResolvedType::extend_unique(
                &mut combined,
                resolve_rhs_expression(cond_expr.r#else, &else_ctx),
            );
            combined
        }
        Expression::Binary(binary) if binary.operator.is_null_coalesce() => {
            // When the LHS is syntactically non-nullable (e.g. `new Foo()`,
            // a literal, `clone $x`), the RHS is dead code — return only
            // the LHS results.  Otherwise resolve both sides; if the LHS
            // type string is nullable, strip `null` before unioning.
            let lhs_non_nullable = matches!(
                binary.lhs,
                Expression::Instantiation(_)
                    | Expression::Literal(_)
                    | Expression::Array(_)
                    | Expression::LegacyArray(_)
                    | Expression::Clone(_)
            );
            let lhs_results = resolve_rhs_expression(binary.lhs, ctx);
            if !lhs_results.is_empty() && lhs_non_nullable {
                lhs_results
            } else if !lhs_results.is_empty() {
                // Strip `null` entries and nullable wrappers from the
                // LHS type strings before unioning with the RHS.
                // Example: `?Foo ?? Bar` → `Foo|Bar`.
                let mut combined: Vec<ResolvedType> = lhs_results
                    .into_iter()
                    .filter_map(|mut rt| {
                        let parsed = rt.type_string.clone();
                        match parsed.non_null_type() {
                            // Nullable/union contained null — use the stripped version.
                            Some(non_null) => {
                                rt.type_string = non_null;
                                Some(rt)
                            }
                            // Not nullable/union: bare `null` is filtered out,
                            // everything else (including `mixed`) passes through.
                            None if rt.type_string == PhpType::null() => None,
                            None => Some(rt),
                        }
                    })
                    .collect();
                // Always union with the RHS.  Even when the LHS type
                // string looks non-nullable, the user wrote `??`
                // defensively and both branches are valid candidates.
                ResolvedType::extend_unique(&mut combined, resolve_rhs_expression(binary.rhs, ctx));
                combined
            } else {
                // The LHS resolved to nothing typeable (a genuinely
                // unresolvable expression). At runtime it could be any
                // value, so represent the unknown LHS as `mixed` and union
                // it with the RHS, mirroring how a `mixed` LHS is handled
                // above.
                let mut combined = vec![ResolvedType::from_type_string(PhpType::mixed())];
                ResolvedType::extend_unique(&mut combined, resolve_rhs_expression(binary.rhs, ctx));
                combined
            }
        }
        Expression::Clone(clone_expr) => resolve_rhs_clone(clone_expr, ctx),
        // ── Pipe operator (PHP 8.5): `$expr |> callable(...)` ──
        // The result type is the return type of the callable.
        // The callable is typically a first-class callable reference
        // (PartialApplication) such as `trim(...)` or `createDate(...)`.
        Expression::Pipe(pipe) => resolve_rhs_pipe(pipe, ctx),
        Expression::PartialApplication(_)
        | Expression::Closure(_)
        | Expression::ArrowFunction(_) => {
            // Closures produce a `Closure` instance at runtime, but when we
            // can infer their body return type (explicit `: T`, generator
            // yields, or arrow-body expression), preserve it in the
            // `PhpType::Callable` so callers like template binding can use
            // it through `$closure` variables.
            let closure_ty = infer_closure_literal_type(expr, ctx);
            // Always resolve against the plain Closure class so that
            // methods like bindTo() are available for completion, even
            // when the inferred type is a typed Callable (Closure(): T).
            let lookup_ty = PhpType::closure();
            let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
                &lookup_ty,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if classes.is_empty() {
                vec![ResolvedType::from_type_string(closure_ty)]
            } else {
                ResolvedType::from_classes_with_hint(classes, closure_ty)
            }
        }
        // ── Generator yield-assignment: `$var = yield $expr` ──
        // The value of a yield expression is the TSend type from
        // the enclosing function's `@return Generator<K, V, TSend, R>`.
        Expression::Yield(_) => {
            if let Some(ref ret_type) = ctx.enclosing_return_type
                && let Some(send_php_type) = ret_type.generator_send_type(true)
            {
                return ResolvedType::from_classes_with_hint(
                    crate::completion::type_resolution::type_hint_to_classes_typed(
                        send_php_type,
                        &ctx.current_class.name,
                        ctx.all_classes,
                        ctx.class_loader,
                    ),
                    send_php_type.clone(),
                );
            }
            vec![]
        }
        // ── Bare variable: `$a = $b` ────────────────────────────────
        // Resolve the RHS variable's type by walking assignments before
        // this point.  The caller (`check_expression_for_assignment`)
        // already set `ctx.cursor_offset` to the assignment's start
        // offset, so the recursive resolution only considers
        // assignments *before* the current one, preventing cycles.
        Expression::Variable(Variable::Direct(dv)) => {
            let rhs_var = bytes_to_str(dv.name).to_string();
            // Guard: never recurse into the same variable (self-assignment).
            if rhs_var == ctx.var_name {
                return vec![];
            }
            resolve_var_types(&rhs_var, ctx, ctx.cursor_offset)
        }
        // ── Concatenation: `"prefix" . $var` → string ───────────────
        Expression::Binary(binary) if binary.operator.is_concatenation() => {
            vec![ResolvedType::from_type_string(PhpType::string())]
        }
        // ── Global constant access: `PHP_EOL`, `SORT_ASC`, etc. ────
        Expression::ConstantAccess(ca) => {
            let name = bytes_to_str(ca.name.value()).to_string();
            let name_clean = strip_fqn_prefix(&name);
            // `true`, `false`, `null` are parsed as ConstantAccess by
            // some AST variants — handle them the same as literals.
            match name_clean.to_lowercase().as_str() {
                "true" | "false" => {
                    return vec![ResolvedType::from_type_string(PhpType::bool())];
                }
                "null" => {
                    return vec![ResolvedType::from_type_string(PhpType::null())];
                }
                _ => {}
            }
            if let Some(loader) = ctx.constant_loader()
                && let Some(maybe_value) = loader(name_clean)
                && let Some(ref value) = maybe_value
                && let Some(ts) = infer_type_from_constant_value(value)
            {
                return vec![ResolvedType::from_type_string(ts)];
            }
            vec![]
        }
        // ── Arithmetic: `$a + $b`, `$a * $b` etc. → numeric ────────
        // We can't distinguish int vs float without deeper analysis,
        // so we don't emit a type here and let callers fall back.
        //
        // ── Catch-all: unrecognised expression types ────────────────
        // Return an empty vec — callers that need a type string for
        // expressions not handled above should use the raw-type
        // inference pipeline.
        _ => vec![],
    }
}

/// Infer a scalar type from a constant's initializer value string.
///
/// Recognises integer literals (`42`, `-1`, `0xFF`), float literals
/// (`3.14`, `1e10`), string literals (`'hello'`, `"world"`), boolean
/// keywords (`true`, `false`), `null`, and array literals (`[...]`,
/// `array(...)`).  Returns `None` for expressions that cannot be
/// trivially classified (e.g. concatenation, function calls).
pub(crate) fn infer_type_from_constant_value(value: &str) -> Option<PhpType> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }

    // String literals: single or double quoted.
    if (v.starts_with('\'') && v.ends_with('\'')) || (v.starts_with('"') && v.ends_with('"')) {
        return Some(PhpType::string());
    }

    // Array literals.
    if v.starts_with('[') || v.starts_with("array(") || v.starts_with("array (") {
        return Some(PhpType::array());
    }

    let lower = v.to_lowercase();

    // Boolean / null keywords.
    if lower == "true" || lower == "false" {
        return Some(PhpType::bool());
    }
    if lower == "null" {
        return Some(PhpType::null());
    }

    // Numeric literals — try integer first, then float.
    // Strip optional leading sign for parsing.
    let numeric = v
        .strip_prefix('-')
        .or_else(|| v.strip_prefix('+'))
        .unwrap_or(v);
    if numeric.starts_with("0x") || numeric.starts_with("0X") {
        // Hex integer.
        if numeric[2..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '_')
        {
            return Some(PhpType::int());
        }
    }
    if numeric.starts_with("0b") || numeric.starts_with("0B") {
        // Binary integer.
        if numeric[2..]
            .chars()
            .all(|c| c == '0' || c == '1' || c == '_')
        {
            return Some(PhpType::int());
        }
    }
    if numeric.starts_with("0o") || numeric.starts_with("0O") {
        // Octal integer (PHP 8.1+).
        if numeric[2..]
            .chars()
            .all(|c| ('0'..='7').contains(&c) || c == '_')
        {
            return Some(PhpType::int());
        }
    }
    // Decimal integer (may contain underscores: 1_000_000).
    if !numeric.is_empty()
        && numeric.chars().all(|c| c.is_ascii_digit() || c == '_')
        && numeric.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return Some(PhpType::int());
    }
    // Float: contains `.` or `e`/`E` among digits.
    if !numeric.is_empty() {
        let has_dot = numeric.contains('.');
        let has_exp = numeric.contains('e') || numeric.contains('E');
        if (has_dot || has_exp)
            && numeric.chars().all(|c| {
                c.is_ascii_digit()
                    || c == '.'
                    || c == 'e'
                    || c == 'E'
                    || c == '+'
                    || c == '-'
                    || c == '_'
            })
        {
            return Some(PhpType::float());
        }
    }

    None
}

/// Resolve a pipe expression `$input |> callable(...)` to the callable's
/// return type.
///
/// The pipe operator passes `$input` as the first argument to `callable`
/// and returns its result.  Chains like `$a |> f(...) |> g(...)` are
/// nested: the outer pipe's input is the inner pipe expression.
///
/// Currently handles function-level callables (e.g. `createDate(...)`).
/// Method and static method callables are not yet supported.
fn resolve_rhs_pipe(pipe: &Pipe<'_>, ctx: &VarResolutionCtx<'_>) -> Vec<ResolvedType> {
    // The callable determines the result type.
    // For `PartialApplication::Function`, extract the function name
    // and look up its return type.
    match pipe.callable {
        Expression::PartialApplication(PartialApplication::Function(fpa)) => {
            let func_name = match fpa.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return vec![],
            };
            let func_name_offset = fpa.function.span().start.offset;
            if let Some(fl) = ctx.function_loader()
                && let Some(func_info) = fl(&func_name, func_name_offset)
                && let Some(ref ret) = func_info.return_type
            {
                return ResolvedType::from_classes_with_hint(
                    crate::completion::type_resolution::type_hint_to_classes_typed(
                        ret,
                        &ctx.current_class.name,
                        ctx.all_classes,
                        ctx.class_loader,
                    ),
                    ret.clone(),
                );
            }
            vec![]
        }
        // Method callable: `$input |> $obj->method(...)`
        // Static callable: `$input |> Class::method(...)`
        // Not yet supported — fall back to empty.
        _ => vec![],
    }
}

/// Resolve `clone $expr` — preserves the cloned expression's type.
///
/// First tries resolving the inner expression structurally (handles
/// `clone new Foo()`, `clone $this->getConfig()`, ternary, etc.).
/// If that yields nothing, falls back to text-based resolution by
/// extracting the source text of the cloned expression and resolving
/// it as a subject string via `resolve_target_classes`.
fn resolve_rhs_clone(clone_expr: &Clone<'_>, ctx: &VarResolutionCtx<'_>) -> Vec<ResolvedType> {
    let structural = resolve_rhs_expression(clone_expr.object, ctx);
    if !structural.is_empty() {
        return structural;
    }
    // Fallback: extract source text of the cloned expression
    // and resolve it as a subject.  This handles cases like
    // `clone $original` where `$original`'s type was set by a
    // prior assignment or parameter type hint.
    let obj_span = clone_expr.object.span();
    let start = obj_span.start.offset as usize;
    let end = obj_span.end.offset as usize;
    if end <= ctx.content.len() {
        let obj_text = ctx.content[start..end].trim();
        if !obj_text.is_empty() {
            let rctx = ctx.as_resolution_ctx();
            return crate::completion::resolver::resolve_target_classes(
                obj_text,
                crate::types::AccessKind::Arrow,
                &rctx,
            );
        }
    }
    vec![]
}

/// Extract the return type hint from a closure or arrow function expression.
///
/// Returns the type-hint string when the expression is a `Closure` or
/// `ArrowFunction` with an explicit return type annotation, e.g.
/// `fn (): Foo => …` yields `"Foo"`.  Returns `None` otherwise.
fn extract_closure_or_arrow_return_type(expr: &Expression<'_>) -> Option<PhpType> {
    match expr {
        Expression::ArrowFunction(arrow) => arrow
            .return_type_hint
            .as_ref()
            .map(|rth| extract_hint_type(&rth.hint)),
        Expression::Closure(closure) => closure
            .return_type_hint
            .as_ref()
            .map(|rth| extract_hint_type(&rth.hint)),
        _ => None,
    }
}

/// Infer template parameter substitutions from a `@psalm-if-this-is` pattern
/// by matching it against the receiver's concrete type.
///
/// For example, given:
/// - `pattern`: `ArrayList<TOption|TEither>`
/// - `receiver`: `ArrayList<Either<Exception, int>|Option<int>>`
/// - Method templates: `A`, `B`, `TOption of Option<A>`, `TEither of Either<mixed, B>`
///
/// This matches `TOption → Option<int>`, `TEither → Either<Exception, int>`,
/// then extracts `A = int` from `Option<A>` vs `Option<int>`, and
/// `B = int` from `Either<mixed, B>` vs `Either<Exception, int>`.
fn infer_if_this_is_subs(
    pattern: &PhpType,
    receiver: &PhpType,
    template_params: &[Atom],
    template_bounds: &AtomMap<PhpType>,
) -> HashMap<String, PhpType> {
    let mut subs: HashMap<String, PhpType> = HashMap::new();

    // Step 1: Match the top-level structure (e.g. Generic vs Generic)
    // and collect direct template bindings.
    match_type_pattern(
        pattern,
        receiver,
        template_params,
        template_bounds,
        &mut subs,
    );

    // Step 2: For each matched template that has a bound with nested
    // templates, match the bound against the concrete value to extract
    // the nested template parameters.
    let direct_subs = subs.clone();
    for (tpl_name, concrete_type) in &direct_subs {
        let tpl_atom = crate::atom::atom(tpl_name);
        if let Some(bound) = template_bounds.get(&tpl_atom) {
            match_type_pattern(
                bound,
                concrete_type,
                template_params,
                template_bounds,
                &mut subs,
            );
        }
    }

    subs
}

/// Recursively match a type pattern against a concrete type, collecting
/// template parameter bindings into `subs`.
fn match_type_pattern(
    pattern: &PhpType,
    concrete: &PhpType,
    template_params: &[Atom],
    template_bounds: &AtomMap<PhpType>,
    subs: &mut HashMap<String, PhpType>,
) {
    match (pattern, concrete) {
        // A named type that is a template parameter — bind it.
        (PhpType::Named(name), _)
            if template_params.iter().any(|t| t.as_str() == name.as_str()) =>
        {
            subs.entry(name.clone()).or_insert_with(|| concrete.clone());
        }
        // Generic types with matching base names — recurse into args.
        (PhpType::Generic(p_base, p_args), PhpType::Generic(c_base, c_args))
            if p_base == c_base && p_args.len() == c_args.len() =>
        {
            for (p_arg, c_arg) in p_args.iter().zip(c_args.iter()) {
                match_type_pattern(p_arg, c_arg, template_params, template_bounds, subs);
            }
        }
        // Union types — match pattern members against concrete members
        // by trying to pair each template pattern member with a concrete
        // member whose base name matches the template's bound.
        (PhpType::Union(p_members), PhpType::Union(c_members)) => {
            for p_m in p_members {
                if let PhpType::Named(name) = p_m {
                    if template_params.iter().any(|t| t.as_str() == name.as_str()) {
                        // This pattern member is a template param in a union.
                        // Find the concrete union member whose base name
                        // matches this template's bound base name.
                        let tpl_atom = crate::atom::atom(name);
                        if let Some(bound) = template_bounds.get(&tpl_atom) {
                            let bound_base = bound.base_name().unwrap_or_default();
                            for c_m in c_members {
                                let c_base = c_m.base_name().unwrap_or_default();
                                if c_base == bound_base {
                                    subs.entry(name.clone()).or_insert_with(|| c_m.clone());
                                    break;
                                }
                            }
                        } else {
                            // No bound — take the first concrete member.
                            if let Some(c_m) = c_members.first() {
                                subs.entry(name.clone()).or_insert_with(|| c_m.clone());
                            }
                        }
                    }
                } else {
                    // Non-template pattern member — recurse.
                    for c_m in c_members {
                        if p_m.base_name() == c_m.base_name() {
                            match_type_pattern(p_m, c_m, template_params, template_bounds, subs);
                            break;
                        }
                    }
                }
            }
        }
        // Nullable patterns.
        (PhpType::Nullable(p_inner), PhpType::Nullable(c_inner)) => {
            match_type_pattern(p_inner, c_inner, template_params, template_bounds, subs);
        }
        _ => {}
    }
}
