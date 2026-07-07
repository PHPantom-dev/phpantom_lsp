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
use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::ast::class_like::member::ClassLikeMember;
use mago_syntax::ast::*;

use crate::Backend;
use crate::atom::{Atom, AtomMap, bytes_to_str};
use crate::docblock;
use crate::parser::{extract_hint_type, with_parsed_program};
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ResolvedType};

use super::resolution::build_var_resolver_from_ctx;
use crate::completion::call_resolution::MethodReturnCtx;
use crate::completion::conditional_resolution::resolve_conditional_with_args;
use crate::completion::resolver::{Loaders, VarResolutionCtx};
use crate::completion::type_resolution;
use crate::util::strip_fqn_prefix;

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
            let cls = crate::util::find_class_by_name(all_classes, base)
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
            let pt =
                super::raw_type_inference::infer_array_literal_raw_type(arr.elements.iter(), ctx)
                    .unwrap_or_else(PhpType::array);
            vec![ResolvedType::from_type_string(pt)]
        }
        Expression::LegacyArray(arr) => {
            let pt =
                super::raw_type_inference::infer_array_literal_raw_type(arr.elements.iter(), ctx)
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
            // access (e.g. `$a["test"]` narrowed through null checks).
            if let Some(resolver) = ctx.scope_var_resolver
                && let Some(key) = crate::completion::types::narrowing::expr_to_subject_key(expr)
                && key.contains("[\"")
            {
                let from_scope = resolver(&key);
                if !from_scope.is_empty() {
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
            // When no scope_var_resolver is available (e.g. type error
            // diagnostic), apply property narrowing from enclosing
            // if-conditions (instanceof checks).  This ensures that
            // `$this->prop` inside `if ($this->prop instanceof X)`
            // resolves to X instead of the declared property type.
            if ctx.scope_var_resolver.is_none()
                && !result.is_empty()
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
            ResolvedType::extend_unique(&mut combined, resolve_rhs_expression(then_expr, ctx));
            ResolvedType::extend_unique(
                &mut combined,
                resolve_rhs_expression(cond_expr.r#else, ctx),
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
            // First-class callable syntax (`strlen(...)`),
            // closure literals (`function() { … }`), and
            // arrow functions (`fn() => …`) all produce a
            // `Closure` instance at runtime.
            // Use the fully-qualified name so that resolution
            // succeeds even inside a namespace block (unqualified
            // class names are prefixed with the current namespace
            // and do NOT fall back to the global scope in PHP).
            let closure_ty = PhpType::closure();
            ResolvedType::from_classes_with_hint(
                crate::completion::type_resolution::type_hint_to_classes_typed(
                    &closure_ty,
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                ),
                closure_ty,
            )
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
fn infer_type_from_constant_value(value: &str) -> Option<PhpType> {
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
            if let Some(fl) = ctx.function_loader()
                && let Some(func_info) = fl(&func_name)
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

/// Resolve `new ClassName(…)` to the instantiated class.
fn resolve_rhs_instantiation(
    inst: &Instantiation<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let class_name = match inst.class {
        Expression::Self_(_) => Some("self".to_string()),
        Expression::Static(_) => Some("static".to_string()),
        Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
        _ => None,
    };
    if let Some(ref name) = class_name {
        let fqn = match name.as_str() {
            "self" | "static" => ctx.current_class.name.to_string(),
            other => crate::util::resolve_name_via_loader(other, ctx.class_loader),
        };
        let parsed_name = PhpType::Named(fqn);
        let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
            &parsed_name,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        );

        // ── Constructor template inference ──────────────────────
        // When the class has `@template` params and the constructor
        // has `@param` bindings for them, infer concrete types from
        // the constructor arguments and apply the substitution to
        // the class so that methods returning `T` resolve correctly.
        if classes.len() == 1 && !classes[0].template_params.is_empty() {
            let cls = &classes[0];
            // Look for the constructor on the raw class first; if not
            // found (child class without its own constructor), walk up
            // the parent chain to find the original declaring class and
            // use its unsubstituted constructor.  This preserves the
            // original template param names in `template_bindings` so
            // that `classify_template_binding` can match them against
            // the parameter type hints (e.g. `array<T>` with binding
            // `("T", "$arr")`).
            let ancestor_cls_arc;
            let ctor_owner: &ClassInfo;
            let ctor_inherited;
            let ctor_ref = if let Some(c) = cls.get_method("__construct") {
                ctor_inherited = false;
                ctor_owner = cls;
                Some(c)
            } else {
                // Walk parent chain to find the raw ancestor that declares __construct.
                let mut found: Option<std::sync::Arc<ClassInfo>> = None;
                let mut cur = cls.parent_class.as_ref().map(|p| p.to_string());
                for _ in 0..15 {
                    let parent_name = match cur {
                        Some(ref n) => n.clone(),
                        None => break,
                    };
                    if let Some(parent) = (ctx.class_loader)(&parent_name) {
                        if parent.get_method("__construct").is_some() {
                            found = Some(parent);
                            break;
                        }
                        cur = parent.parent_class.as_ref().map(|p| p.to_string());
                    } else {
                        break;
                    }
                }
                match found {
                    Some(arc) => {
                        ancestor_cls_arc = arc;
                        ctor_inherited = true;
                        ctor_owner = &ancestor_cls_arc;
                        ancestor_cls_arc.get_method("__construct")
                    }
                    None => {
                        ctor_inherited = false;
                        ctor_owner = cls;
                        None
                    }
                }
            };
            if let Some(ctor) = ctor_ref
                && !ctor.template_bindings.is_empty()
                && let Some(ref arg_list) = inst.argument_list
            {
                let arg_texts =
                    super::raw_type_inference::extract_arg_texts_from_ast(arg_list, ctx.content);
                if !arg_texts.is_empty() {
                    let rctx = ctx.as_resolution_ctx();
                    let raw_subs =
                        build_constructor_template_subs(ctor_owner, ctor, &arg_texts, &rctx, ctx);
                    // When the constructor is inherited, its template_bindings
                    // reference the ancestor's template param names.  Remap
                    // them to the child's template params via the @extends chain.
                    let subs = if ctor_inherited && !raw_subs.is_empty() {
                        remap_inherited_ctor_subs(cls, &raw_subs, ctx.class_loader)
                    } else {
                        raw_subs
                    };
                    if !subs.is_empty() {
                        // ── Infer unbound template params from bound constraints ──
                        // When a template param has a bound like
                        // `TIterator as Iterator<TKey, TValue>` and TIterator
                        // has been resolved to a concrete type (e.g.
                        // `Generator<int, string>`), match the concrete type's
                        // generic args against the bound's args to infer the
                        // nested template params (TKey=int, TValue=string).
                        let mut subs = subs;
                        for (bound_param, bound_type) in cls.template_param_bounds.iter() {
                            let bound_param_str: &str = bound_param.as_ref();
                            if let Some(concrete) = subs.get(bound_param_str).cloned()
                                && let PhpType::Generic(_, bound_args) = bound_type
                            {
                                let concrete_args = match &concrete {
                                    PhpType::Generic(_, args) => Some(args.as_slice()),
                                    _ => None,
                                };
                                if let Some(concrete_args) = concrete_args {
                                    for (i, bound_arg) in bound_args.iter().enumerate() {
                                        if let PhpType::Named(tpl_name) = bound_arg
                                            && cls
                                                .template_params
                                                .iter()
                                                .any(|t| t.as_str() == tpl_name.as_str())
                                            && !subs.contains_key(tpl_name.as_str())
                                            && let Some(concrete_arg) = concrete_args.get(i)
                                        {
                                            subs.insert(tpl_name.clone(), concrete_arg.clone());
                                        }
                                    }
                                }
                            }
                        }
                        let type_args: Vec<PhpType> = cls
                            .template_params
                            .iter()
                            .map(|p| {
                                let p_str: &str = p.as_ref();
                                subs.get(p_str).cloned().unwrap_or_else(|| {
                                    // Use the declared upper bound or `mixed`
                                    // instead of the raw template name so that
                                    // downstream consumers never see
                                    // `PhpType::Named("TValue")`.
                                    cls.template_param_bounds
                                        .get(p)
                                        .cloned()
                                        .unwrap_or_else(PhpType::mixed)
                                })
                            })
                            .collect();
                        let substituted_arc =
                            crate::virtual_members::resolve_class_fully_with_type_args(
                                cls,
                                ctx.class_loader,
                                ctx.resolved_class_cache,
                                &type_args,
                            );
                        let mut substituted = Arc::unwrap_or_clone(substituted_arc);

                        // ── Template-param mixin resolution ────────────────
                        // When a class declares `@mixin TParam` where `TParam`
                        // is a template parameter, the mixin cannot be resolved
                        // during `resolve_class_fully` because the concrete type
                        // is not yet known.  Now that generic args are concrete,
                        // resolve those mixins and merge their members.
                        if cls
                            .mixins
                            .iter()
                            .any(|m| cls.template_params.iter().any(|t| t == m.as_str()))
                        {
                            let generic_subs =
                                crate::inheritance::build_generic_subs(cls, &type_args);
                            if !generic_subs.is_empty() {
                                let mixin_members =
                                    crate::virtual_members::phpdoc::resolve_template_param_mixins(
                                        cls,
                                        &generic_subs,
                                        ctx.class_loader,
                                    );
                                if !mixin_members.is_empty() {
                                    crate::virtual_members::merge_virtual_members(
                                        &mut substituted,
                                        mixin_members,
                                    );
                                }
                            }
                        }

                        let generic_type =
                            PhpType::Generic(substituted.name.to_string(), type_args.clone());
                        return vec![ResolvedType::from_both(generic_type, substituted)];
                    }
                }
            }

            // ── Fallback: resolve unbound template params to bounds ─
            // When no constructor argument bound any template param
            // (e.g. `new Collection()` with no args, or the
            // constructor has no template bindings), substitute all
            // template params with their declared upper bound or
            // `mixed`.  This follows PHPStan's `resolveToBounds()`
            // semantics and prevents raw template names from leaking
            // into method parameter/return types.
            let type_args = crate::inheritance::default_type_args(cls);
            let substituted = crate::virtual_members::resolve_class_fully_with_type_args(
                cls,
                ctx.class_loader,
                ctx.resolved_class_cache,
                &type_args,
            );
            let generic_type = PhpType::Generic(substituted.name.to_string(), type_args.clone());
            return vec![ResolvedType::from_both_arc(generic_type, substituted)];
        }

        return ResolvedType::from_classes_with_hint(classes, parsed_name);
    }

    // ── `new $var` where `$var` holds a class-string ────────────
    // When the class expression is a variable, resolve it to check
    // if it holds a class-string value (e.g. `$f = Foo::class;
    // new $f`).  Extract the class name from the class-string and
    // use it to resolve the instantiated type.
    if let Expression::Variable(Variable::Direct(dv)) = inst.class {
        let var_name = bytes_to_str(dv.name).to_string();
        let resolved =
            crate::completion::variable::class_string_resolution::resolve_class_string_targets(
                &var_name,
                ctx.current_class,
                ctx.all_classes,
                ctx.content,
                ctx.cursor_offset,
                ctx.class_loader,
            );
        if !resolved.is_empty() {
            return ResolvedType::from_classes(resolved.into_iter().map(Arc::new).collect());
        }

        // Fallback: resolve the variable's type and extract the inner
        // type from `class-string<T>`.  This handles parameters typed
        // as `@param class-string<Foo> $var` where there is no
        // `$var = Foo::class` assignment.
        let var_types = resolve_var_types(&var_name, ctx, ctx.cursor_offset);
        let class_name = extract_class_string_inner(&var_types);
        if let Some(name) = class_name
            && let Some(cls) = (ctx.class_loader)(&name)
        {
            return ResolvedType::from_classes(vec![cls]);
        }
    }

    vec![]
}

/// Extract the inner class name from a `class-string<T>` type in a list
/// of resolved types.  Handles `class-string<T>`, `?class-string<T>`,
/// and unions containing `class-string<T>`.
fn extract_class_string_inner(resolved: &[ResolvedType]) -> Option<String> {
    resolved.iter().find_map(|rt| match &rt.type_string {
        PhpType::ClassString(Some(inner)) => inner.base_name().map(|s| s.to_string()),
        PhpType::Nullable(inner) => match inner.as_ref() {
            PhpType::ClassString(Some(cs_inner)) => cs_inner.base_name().map(|s| s.to_string()),
            _ => None,
        },
        PhpType::Union(members) => members.iter().find_map(|m| match m {
            PhpType::ClassString(Some(inner)) => inner.base_name().map(|s| s.to_string()),
            PhpType::Nullable(inner) => match inner.as_ref() {
                PhpType::ClassString(Some(cs_inner)) => cs_inner.base_name().map(|s| s.to_string()),
                _ => None,
            },
            _ => None,
        }),
        _ => None,
    })
}

/// Extract a generic type argument from a class's ancestor chain.
///
/// Given an argument type (e.g. `FooContainer`) and a target wrapper class
/// (e.g. `Container`), walks the `@extends` chain to find where the argument
/// type (or one of its ancestors) extends the wrapper class, then extracts the
/// generic argument at `tpl_position`.
///
/// For example, if `FooContainer` has `@extends Container<Foo>`, calling
/// `extract_generic_arg_from_ancestor(FooContainer, "Container", 0, ...)` returns `Foo`.
fn extract_generic_arg_from_ancestor(
    arg_type: &PhpType,
    wrapper_name: &str,
    tpl_position: usize,
    rctx: &crate::completion::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    // Get the class name from the argument type.
    let class_name = match arg_type {
        PhpType::Named(n) => n.as_str(),
        PhpType::Generic(n, _) => n.as_str(),
        _ => return None,
    };

    // If the arg type itself is already generic with the wrapper name,
    // extract directly.  E.g. argument type is `Container<Foo>`.
    if let PhpType::Generic(n, args) = arg_type {
        let n_short = crate::util::short_name(n);
        let wrapper_short = crate::util::short_name(wrapper_name);
        if n_short.eq_ignore_ascii_case(wrapper_short) {
            return args.get(tpl_position).cloned();
        }
    }

    let class_loader = rctx.class_loader;
    let cls = class_loader(class_name)?;

    // Check the class's own @extends generics for the wrapper.
    let wrapper_short = crate::util::short_name(wrapper_name);
    if let Some(arg) = find_extends_generic_arg(&cls, wrapper_short, tpl_position) {
        return Some(arg);
    }

    // Walk parent chain.
    let mut current = cls;
    for _ in 0..15 {
        let parent_name = current.parent_class.as_ref()?;
        let parent = class_loader(parent_name)?;

        // Check if the parent's @extends generics reference the wrapper.
        // But first, build a substitution map from current → parent so
        // template params in the parent's @extends are resolved.
        if let Some(arg) = find_extends_generic_arg(&parent, wrapper_short, tpl_position) {
            // The arg might reference the parent's template params — substitute
            // through the chain to get concrete types.
            let subs = build_extends_sub_map(&current, &parent);
            let resolved = if subs.is_empty() {
                arg
            } else {
                arg.substitute(&subs)
            };
            return Some(resolved);
        }

        current = parent;
    }

    None
}

/// Find a generic arg at `position` from a class's `@extends` generics
/// matching a target short name.
fn find_extends_generic_arg(
    cls: &ClassInfo,
    target_short: &str,
    position: usize,
) -> Option<PhpType> {
    for (name, args) in cls
        .extends_generics
        .iter()
        .chain(cls.implements_generics.iter())
    {
        if crate::util::short_name(name) == target_short {
            return args.get(position).cloned();
        }
    }
    None
}

/// Build a simple substitution map from a child class to its parent based
/// on `@extends` generics.
fn build_extends_sub_map(child: &ClassInfo, parent: &ClassInfo) -> HashMap<String, PhpType> {
    if parent.template_params.is_empty() {
        return HashMap::new();
    }
    let parent_short = crate::util::short_name(&parent.name);
    let type_args = child
        .extends_generics
        .iter()
        .chain(child.implements_generics.iter())
        .find(|(name, _)| crate::util::short_name(name) == parent_short)
        .map(|(_, args)| args);
    let mut map = HashMap::new();
    if let Some(args) = type_args {
        for (i, param) in parent.template_params.iter().enumerate() {
            if let Some(arg) = args.get(i) {
                map.insert(param.to_string(), arg.clone());
            }
        }
    }
    map
}

/// Remap constructor template substitutions from ancestor param names to child
/// param names when a constructor is inherited.
///
/// When `CollectionChild<T, V>` extends `Collection<V>` and `Collection` has
/// `@template T` with constructor `@param array<T> $arr`, the inherited
/// constructor's `template_bindings` map `("T", "$arr")` where `T` is
/// `Collection`'s template param.  After inference, `raw_subs` contains
/// `{"T" => Dog}`.  We need to translate this to `{"V" => Dog}` because
/// `Collection.T` maps to `CollectionChild.V` via `@extends Collection<V>`.
pub(crate) fn remap_inherited_ctor_subs(
    child: &ClassInfo,
    raw_subs: &HashMap<String, PhpType>,
    class_loader: &dyn Fn(&str) -> Option<std::sync::Arc<ClassInfo>>,
) -> HashMap<String, PhpType> {
    // Walk up the extends chain to find the class that originally declares
    // the constructor, building a cumulative mapping from ancestor template
    // params to child template params.
    //
    // Start with an identity map for the child's own template params.
    let mut ancestor_to_child: HashMap<String, PhpType> = child
        .template_params
        .iter()
        .map(|p| (p.to_string(), PhpType::Named(p.to_string())))
        .collect();

    // Track the current node's extends info as owned data so we don't
    // need a reference across loop iterations.
    let mut cur_parent_class = child.parent_class;
    let mut cur_extends_generics = child.extends_generics.clone();

    for _ in 0..15 {
        let parent_name = match cur_parent_class {
            Some(ref p) => *p,
            None => break,
        };
        let parent = match class_loader(&parent_name) {
            Some(p) => p,
            None => break,
        };

        // Find @extends generics for this parent (e.g. @extends Collection<V>).
        let parent_short = crate::util::short_name(&parent.name);
        if let Some((_, type_args)) = cur_extends_generics
            .iter()
            .find(|(name, _)| crate::util::short_name(name) == parent_short)
        {
            // Build a mapping: parent.template_params[i] → type_args[i],
            // then resolve type_args through ancestor_to_child to get
            // parent param → child param.
            let mut new_mapping = HashMap::new();
            for (i, parent_param) in parent.template_params.iter().enumerate() {
                if let Some(arg) = type_args.get(i) {
                    let resolved = arg.substitute(&ancestor_to_child);
                    new_mapping.insert(parent_param.to_string(), resolved);
                }
            }
            ancestor_to_child = new_mapping;
        } else {
            // No @extends generics — can't map further.
            break;
        }

        // If the parent has the constructor, we've found our ancestor.
        if parent.get_method("__construct").is_some() {
            break;
        }

        cur_parent_class = parent.parent_class;
        cur_extends_generics = parent.extends_generics.clone();
    }

    // Now remap: for each entry in raw_subs (keyed by ancestor param name),
    // find which child param it maps to via ancestor_to_child.
    let mut result = HashMap::new();
    for (ancestor_param, inferred_type) in raw_subs {
        if let Some(child_type) = ancestor_to_child.get(ancestor_param) {
            // child_type is typically PhpType::Named("V") — extract the name.
            match child_type {
                PhpType::Named(child_param) => {
                    result.insert(child_param.clone(), inferred_type.clone());
                }
                _ => {
                    // Complex mapping (e.g. mapped to a concrete type, not a
                    // param name) — keep the original key as fallback.
                    result.insert(ancestor_param.clone(), inferred_type.clone());
                }
            }
        } else {
            // No mapping found — keep the original key.
            result.insert(ancestor_param.clone(), inferred_type.clone());
        }
    }
    result
}

/// Build a template substitution map from constructor arguments.
///
/// Uses the constructor's `template_bindings` (from `@param T $name`
/// annotations) to match template parameters to their concrete types
/// inferred from the call-site arguments.  Handles:
///   - Direct type: `@param T $bar` + `new Foo(new Baz())` → `T = Baz`
///   - Array type: `@param T[] $items` + `new Foo([new X()])` → `T = X`
///   - Generic wrapper: `@param Wrapper<T> $w` + `new Foo(new Wrapper(new X()))` → `T = X`
///     (by resolving the wrapper's constructor template params recursively)
fn build_constructor_template_subs(
    _class: &ClassInfo,
    ctor: &crate::types::MethodInfo,
    arg_texts: &[String],
    rctx: &crate::completion::resolver::ResolutionCtx<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let mut subs = HashMap::new();

    for (tpl_name, param_name) in &ctor.template_bindings {
        // Find the parameter index for this binding.
        let param_idx = match ctor
            .parameters
            .iter()
            .position(|p| p.name == param_name.as_str())
        {
            Some(idx) => idx,
            None => continue,
        };

        // Get the corresponding argument text.
        let provided_arg = arg_texts.get(param_idx).map(|t| t.trim());

        // Determine the binding mode by inspecting the parameter's
        // docblock type hint.  The type hint tells us how the template
        // param is embedded in the `@param` annotation.
        let param_hint = ctor
            .parameters
            .get(param_idx)
            .and_then(|p| p.type_hint.as_ref());
        let binding_mode = classify_template_binding(tpl_name, param_hint);

        // Fall back to the parameter's default value only for binding
        // modes where the default is meaningful.
        let default_value = ctor
            .parameters
            .get(param_idx)
            .and_then(|p| p.default_value.as_deref());
        let arg_text: &str = match provided_arg {
            Some(text) => text,
            None => match &binding_mode {
                TemplateBindingMode::ClassStringInner => match default_value {
                    Some(d) => d,
                    None => continue,
                },
                TemplateBindingMode::Direct => match default_value {
                    Some(d) if d.ends_with("::class") => d,
                    _ => continue,
                },
                _ => continue,
            },
        };

        match binding_mode {
            TemplateBindingMode::Direct => {
                // `@param T $bar` — the argument resolves directly to T.
                if let Some(resolved_type) = Backend::resolve_arg_text_to_type(arg_text, rctx) {
                    subs.insert(tpl_name.to_string(), resolved_type);
                }
            }
            TemplateBindingMode::CallableReturnType => {
                // `@param callable(...): T $cb` — extract the closure's
                // return type annotation from the argument text.
                if let Some(ret_type) =
                    crate::completion::source::helpers::extract_closure_return_type_from_text(
                        arg_text,
                    )
                {
                    subs.insert(tpl_name.to_string(), ret_type);
                }
            }
            TemplateBindingMode::CallableParamType(position) => {
                // `@param Closure(T): void $cb` — extract the closure's
                // parameter type annotation at the given position.
                if let Some(param_type) =
                    crate::completion::source::helpers::extract_closure_param_type_from_text(
                        arg_text, position,
                    )
                {
                    subs.insert(tpl_name.to_string(), param_type);
                }
            }
            TemplateBindingMode::ArrayElement => {
                // `@param T[] $items` — resolve individual array elements.
                if arg_text.starts_with('[') && arg_text.ends_with(']') {
                    let inner = arg_text[1..arg_text.len() - 1].trim();
                    if inner.is_empty() {
                        // Empty array `[]` → element type is `never`
                        // (an empty collection has no elements).
                        subs.insert(tpl_name.to_string(), PhpType::never());
                    } else {
                        let first_elem =
                            crate::completion::conditional_resolution::split_text_args(inner);
                        if let Some(elem) = first_elem.first()
                            && let Some(resolved_type) =
                                Backend::resolve_arg_text_to_type(elem.trim(), rctx)
                        {
                            subs.insert(tpl_name.to_string(), resolved_type);
                        }
                    }
                } else if let Some(resolved_type) =
                    Backend::resolve_arg_text_to_type(arg_text, rctx)
                {
                    // Extract the element type from array-like types
                    // so we bind T to the element, not the whole array.
                    if let Some(elem_type) = resolved_type.extract_value_type(false) {
                        insert_or_union(&mut subs, tpl_name.to_string(), elem_type.clone());
                    } else {
                        insert_or_union(&mut subs, tpl_name.to_string(), resolved_type);
                    }
                }
            }
            TemplateBindingMode::ClassStringInner => {
                if let Some(resolved_type) = Backend::resolve_arg_text_to_type(arg_text, rctx) {
                    // Unwrap `class-string<X>` → `X` so that the
                    // substitution doesn't double-wrap.
                    let unwrapped = match resolved_type {
                        PhpType::ClassString(Some(inner)) => *inner,
                        _ => resolved_type,
                    };
                    insert_or_union(&mut subs, tpl_name.to_string(), unwrapped);
                }
            }
            TemplateBindingMode::GenericWrapper(wrapper_name, tpl_position) => {
                // `@param array<TKey, T> $items` with `[]` → `never`.
                // An empty array literal has no keys or values, so all
                // generic type args of array-like wrappers are `never`.
                let is_array_like = matches!(
                    wrapper_name.as_str(),
                    "array" | "list" | "non-empty-array" | "non-empty-list"
                );
                if is_array_like
                    && arg_text.starts_with('[')
                    && arg_text.ends_with(']')
                    && arg_text[1..arg_text.len() - 1].trim().is_empty()
                {
                    subs.insert(tpl_name.to_string(), PhpType::never());
                } else if let Some(concrete) = resolve_generic_wrapper_template(
                    &wrapper_name,
                    tpl_position,
                    arg_text,
                    rctx,
                    ctx,
                ) {
                    subs.insert(tpl_name.to_string(), concrete);
                }
            }
        }
    }

    subs
}

/// How a template parameter is referenced in a `@param` type annotation.
#[derive(Debug)]
pub(crate) enum TemplateBindingMode {
    /// `@param T $bar` — the whole type is the template param.
    Direct,
    /// `@param T[] $items` — the template param is the array element type.
    ArrayElement,
    /// `@param Wrapper<..., T, ...> $a` — the template param is a generic
    /// argument of the wrapper class at the given position.
    GenericWrapper(String, usize),
    /// `@param callable(...): T $cb` — the template param appears in the
    /// callable's return type.  The binding is resolved by extracting the
    /// return type annotation from the closure/arrow-function argument.
    CallableReturnType,
    /// `@param Closure(T): void $cb` — the template param appears in the
    /// callable's parameter list at the given position (0-based).  The
    /// binding is resolved by extracting the closure's parameter type
    /// annotation at that index from the argument text.
    CallableParamType(usize),
    /// `@param class-string<T> $class` — the template param appears inside
    /// `class-string<>`.  The binding is resolved by unwrapping the
    /// `class-string<>` layer from the resolved argument type.
    ClassStringInner,
}

/// Classify how a template parameter name appears in a `@param` type hint.
///
/// Handles union types like `Arrayable<TKey, TValue>|iterable<TKey, TValue>|null`
/// by recursively inspecting the [`PhpType`] structure.
pub(crate) fn classify_template_binding(
    tpl_name: &str,
    param_hint: Option<&PhpType>,
) -> TemplateBindingMode {
    let hint = match param_hint {
        Some(h) => h,
        None => return TemplateBindingMode::Direct,
    };

    classify_from_php_type(tpl_name, hint)
}

/// Recursively classify how a template parameter name appears in a parsed
/// [`PhpType`].
fn classify_from_php_type(tpl_name: &str, ty: &PhpType) -> TemplateBindingMode {
    match ty {
        PhpType::Nullable(inner) => classify_from_php_type(tpl_name, inner),
        PhpType::Union(members) => {
            let mut fallback: Option<TemplateBindingMode> = None;
            for member in members {
                if member.is_null() {
                    continue;
                }
                // If the template name appears directly as a union
                // member, prefer Direct immediately.  Direct always
                // works regardless of what the argument is, while
                // CallableReturnType only works when the argument is
                // a closure.  This handles the common Laravel
                // `(Closure($this): T)|T|null` pattern in `when()`.
                if member.is_named(tpl_name) {
                    return TemplateBindingMode::Direct;
                }
                let result = classify_from_php_type(tpl_name, member);
                if !matches!(result, TemplateBindingMode::Direct) && fallback.is_none() {
                    fallback = Some(result);
                }
            }
            fallback.unwrap_or(TemplateBindingMode::Direct)
        }
        PhpType::Array(inner) => {
            if inner.as_ref().is_named(tpl_name) {
                return TemplateBindingMode::ArrayElement;
            }
            TemplateBindingMode::Direct
        }
        PhpType::Named(n) if n == tpl_name => TemplateBindingMode::Direct,
        PhpType::Generic(wrapper_name, args) => {
            // `array<T>` (single arg) should be treated as ArrayElement,
            // not GenericWrapper — "array" is not a real class that can
            // be resolved for constructor inference.  Multi-arg forms
            // like `array<TKey, TValue>` stay as GenericWrapper so that
            // function-level template inference can extract each arg
            // from a concrete generic type (e.g. `array<int, Foo>`).
            let is_array_like = matches!(
                wrapper_name.to_ascii_lowercase().as_str(),
                "array" | "list" | "non-empty-array" | "non-empty-list"
            );
            if is_array_like && args.len() == 1 && args[0].is_named(tpl_name) {
                return TemplateBindingMode::ArrayElement;
            }
            for (i, arg) in args.iter().enumerate() {
                if arg.is_named(tpl_name) {
                    return TemplateBindingMode::GenericWrapper(wrapper_name.clone(), i);
                }
            }
            TemplateBindingMode::Direct
        }
        PhpType::Callable {
            params,
            return_type,
            ..
        } => {
            if let Some(rt) = return_type
                && type_contains_name(rt, tpl_name)
            {
                return TemplateBindingMode::CallableReturnType;
            }
            for (i, p) in params.iter().enumerate() {
                if type_contains_name(&p.type_hint, tpl_name) {
                    return TemplateBindingMode::CallableParamType(i);
                }
            }
            TemplateBindingMode::Direct
        }
        PhpType::ClassString(Some(inner)) => {
            if inner.as_ref().is_named(tpl_name) {
                return TemplateBindingMode::ClassStringInner;
            }
            TemplateBindingMode::Direct
        }
        _ => TemplateBindingMode::Direct,
    }
}

/// Check whether a [`PhpType`] tree contains a [`PhpType::Named`] with the
/// given name anywhere in its structure.
fn type_contains_name(ty: &PhpType, name: &str) -> bool {
    match ty {
        PhpType::Named(n) => n == name,
        PhpType::Nullable(inner) | PhpType::Array(inner) => type_contains_name(inner, name),
        PhpType::Union(members) | PhpType::Intersection(members) => {
            members.iter().any(|m| type_contains_name(m, name))
        }
        PhpType::Generic(_, args) => args.iter().any(|a| type_contains_name(a, name)),
        PhpType::Callable {
            params,
            return_type,
            ..
        } => {
            params
                .iter()
                .any(|p| type_contains_name(&p.type_hint, name))
                || return_type
                    .as_ref()
                    .is_some_and(|rt| type_contains_name(rt, name))
        }
        PhpType::ClassString(Some(inner))
        | PhpType::InterfaceString(Some(inner))
        | PhpType::KeyOf(inner)
        | PhpType::ValueOf(inner) => type_contains_name(inner, name),
        _ => false,
    }
}

/// Resolve a template param that appears inside a generic wrapper type.
///
/// For `@param Wrapper<T> $a` with argument `new Wrapper(new X())`,
/// recursively resolve the wrapper's constructor template params to
/// find the concrete type for the template param at `tpl_position`.
fn resolve_generic_wrapper_template(
    wrapper_name: &str,
    tpl_position: usize,
    arg_text: &str,
    rctx: &crate::completion::resolver::ResolutionCtx<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    // ── Built-in array-like types ───────────────────────────────
    // `array`, `list`, `non-empty-array`, `non-empty-list` are not
    // real classes — infer key/value types directly from the array
    // literal argument.
    if matches!(
        wrapper_name,
        "array" | "list" | "non-empty-array" | "non-empty-list"
    ) {
        // Try to infer from array literal first.
        if let Some(result) = resolve_array_literal_generic(tpl_position, arg_text, rctx) {
            return Some(result);
        }
        // If the argument is not a literal (e.g. a variable), resolve its
        // type and extract the generic arg at the given position.
        if let Some(resolved) = Backend::resolve_arg_text_to_type(arg_text, rctx) {
            return extract_generic_arg_at_position(&resolved, tpl_position);
        }
        return None;
    }

    // Load the wrapper class.
    let wrapper_cls = (ctx.class_loader)(wrapper_name)
        .map(Arc::unwrap_or_clone)
        .or_else(|| {
            ctx.all_classes
                .iter()
                .find(|c| crate::util::short_name(&c.name) == crate::util::short_name(wrapper_name))
                .map(|c| ClassInfo::clone(c))
        })?;

    // Find the wrapper's constructor and its template bindings.
    let wrapper_ctor = wrapper_cls.get_method("__construct")?;
    if wrapper_ctor.template_bindings.is_empty() {
        return None;
    }

    // Extract the constructor arguments from the argument text.
    // e.g. from `new Foobar(new X())` extract `new X()`.
    let paren_start = arg_text.find('(')?;
    let paren_end = arg_text.rfind(')')?;
    let inner_args = arg_text[paren_start + 1..paren_end].trim();

    let wrapper_arg_texts = crate::completion::conditional_resolution::split_text_args(inner_args)
        .into_iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let wrapper_subs =
        build_constructor_template_subs(&wrapper_cls, wrapper_ctor, &wrapper_arg_texts, rctx, ctx);

    // Find the wrapper's template param at the given position and
    // look it up in the substitution map.
    let wrapper_tpl = wrapper_cls.template_params.get(tpl_position)?;
    wrapper_subs.get(wrapper_tpl.as_str()).cloned()
}

/// Extract a generic type argument from an array literal.
///
/// For `@param array<TKey, TValue> $kv` with argument `["a" => 1]`:
/// - `tpl_position == 0` → key type (`string`)
/// - `tpl_position == 1` → value type (`int`)
///
/// For single-param wrappers like `list<T>`, position 0 is the element type.
fn resolve_array_literal_generic(
    tpl_position: usize,
    arg_text: &str,
    rctx: &crate::completion::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    let trimmed = arg_text.trim();

    // Must be an array literal.
    let inner = if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed[1..trimmed.len() - 1].trim()
    } else if let Some(s) = trimmed.strip_prefix("array(") {
        s.strip_suffix(')')?.trim()
    } else {
        // Not an array literal — cannot infer.
        return None;
    };

    if inner.is_empty() {
        return Some(PhpType::never());
    }

    let elements = crate::completion::conditional_resolution::split_text_args(inner);

    // Determine whether elements are key=>value pairs.
    // Check the first element for `=>`.
    let first = elements.first()?.trim();
    let has_keys = first.contains("=>");

    if has_keys {
        // Collect key types (position 0) or value types (position 1)
        // from the first element (sufficient for inference).
        let arrow_pos = first.find("=>")?;
        match tpl_position {
            0 => {
                let key_text = first[..arrow_pos].trim();
                Backend::resolve_arg_text_to_type(key_text, rctx)
            }
            1 => {
                let val_text = first[arrow_pos + 2..].trim();
                Backend::resolve_arg_text_to_type(val_text, rctx)
            }
            _ => None,
        }
    } else {
        // No keys — this is a list-style array.
        // Position 0 in `array<T>` or `list<T>` is the element type.
        // Position 0 in `array<TKey, TValue>` would be `int` (implicit key).
        // Position 1 in `array<TKey, TValue>` is the element type.
        match tpl_position {
            0 => {
                // Implicit integer keys.
                Some(PhpType::Named("int".to_string()))
            }
            1 => {
                // Element type from first element.
                Backend::resolve_arg_text_to_type(first, rctx)
            }
            _ => None,
        }
    }
}

/// Extract the generic type argument at a given position from a resolved type.
///
/// For `array<int, string>` with position 0 → `int`, position 1 → `string`.
/// For `list<User>` with position 0 → `User`.
/// Also handles `PhpType::Array(inner)` as a single-arg generic.
fn extract_generic_arg_at_position(ty: &PhpType, position: usize) -> Option<PhpType> {
    match ty {
        PhpType::Generic(name, args) => {
            // `list<T>` has a single arg (the value type).  When the
            // binding expects position 1 (value position of `array<K, V>`),
            // map it to position 0 of the list.  Position 0 of a list
            // is implicitly `int` (sequential keys).
            let is_list_like = matches!(
                name.to_ascii_lowercase().as_str(),
                "list" | "non-empty-list"
            );
            if is_list_like && args.len() == 1 {
                return match position {
                    0 => Some(PhpType::int()),
                    1 => args.first().cloned(),
                    _ => None,
                };
            }
            args.get(position).cloned()
        }
        PhpType::Array(inner) if position == 0 => Some(inner.as_ref().clone()),
        _ => None,
    }
}

/// Resolve `$arr[0]` / `$arr[$key]` by extracting the generic element
/// type from the base array's annotation or assignment.
fn resolve_rhs_array_access<'b>(
    array_access: &ArrayAccess<'b>,
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // Collect bracket segments and find the innermost base variable by
    // walking through nested ArrayAccess nodes.  This handles both
    // single access (`$result['data']`) and chained access
    // (`$result['items'][0]`).
    let mut segments: Vec<ArrayBracketSegment> = Vec::new();
    let mut current_expr: &Expression<'_> = array_access.array;

    // Classify the outermost (current) index first.
    segments.push(classify_array_index(array_access.index));

    // Walk inward through nested ArrayAccess nodes.
    while let Expression::ArrayAccess(inner) = current_expr {
        segments.push(classify_array_index(inner.index));
        current_expr = inner.array;
    }

    // Segments were collected innermost-last; reverse to left-to-right order.
    segments.reverse();

    let access_offset = expr.span().start.offset as usize;

    // Resolve the base expression's raw type string.
    // For bare variables (`$var['key']`), use docblock or assignment scanning.
    // For property chains (`$obj->prop['key']`), resolve the property type.
    let raw_type: Option<PhpType> = if let Expression::Variable(Variable::Direct(base_dv)) =
        current_expr
    {
        let base_var = bytes_to_str(base_dv.name).to_string();
        // When a scope_var_resolver is available (forward walk),
        // prefer it over the docblock scan.  The forward walk
        // already incorporates @var annotations AND applies
        // condition-based narrowing (e.g. null stripping on array
        // shape keys through guard clauses).  Falling back to the
        // raw docblock would discard that narrowing.
        let scope_result = if ctx.scope_var_resolver.is_some() {
            let resolved = resolve_var_types(&base_var, ctx, access_offset as u32);
            if resolved.is_empty() {
                None
            } else {
                Some(ResolvedType::types_joined(&resolved))
            }
        } else {
            None
        };
        scope_result
            .or_else(|| {
                docblock::find_iterable_raw_type_in_source(ctx.content, access_offset, &base_var)
                    .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader))
            })
            .or_else(|| {
                let resolved = resolve_var_types(&base_var, ctx, access_offset as u32);
                if resolved.is_empty() {
                    None
                } else {
                    Some(ResolvedType::types_joined(&resolved))
                }
            })
    } else {
        // Non-variable base (e.g. property access `$obj->prop['key']`,
        // method call `$obj->getItems()['key']`, etc.).
        // Resolve the base expression to get its type.
        let base_resolved = resolve_rhs_expression(current_expr, ctx);
        if base_resolved.is_empty() {
            None
        } else {
            Some(ResolvedType::types_joined(&base_resolved))
        }
    };

    let Some(mut current) = raw_type else {
        // The base expression's type is unknown (e.g. an untyped parameter
        // or an unresolvable call). Accessing an offset on an unknown value
        // yields `mixed`, matching PHPStan's treatment of `mixed[$k]`. This
        // is the honest answer rather than an empty (untyped) result, and it
        // lets the `??` handler union it without a special case.
        return vec![ResolvedType::from_type_string(PhpType::mixed())];
    };

    // Expand type aliases so that shape/generic extraction can see the
    // underlying type (e.g. a `@phpstan-type` alias).
    if let Some(expanded) = crate::completion::type_resolution::resolve_type_alias_typed(
        &current,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    ) {
        current = expanded;
    }

    // Walk each bracket segment, narrowing the type at each step.
    for seg in &segments {
        // Try pure-type extraction first (array shapes, generics).
        let extracted = match seg {
            ArrayBracketSegment::StringKey(key) => current
                .shape_value_type(key)
                .cloned()
                .or_else(|| current.extract_value_type(true).cloned()),
            ArrayBracketSegment::ElementAccess => current.extract_value_type(true).cloned(),
        };

        if let Some(element) = extracted {
            current = element;
        } else {
            // Fallback: when the current type is a plain class name (e.g.
            // `OpeningHours`), resolve the class and check its iterable
            // generics (`@extends`, `@implements`) for the element type.
            // This handles `$obj->prop['key']` where `prop` is a collection
            // class like `OpeningHours extends DataCollection<string, Day>`.
            let class_element = crate::completion::type_resolution::type_hint_to_classes_typed(
                &current,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            )
            .into_iter()
            .find_map(|cls| {
                let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                    &cls,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                );
                super::foreach_resolution::extract_iterable_element_type_from_class(
                    &merged,
                    ctx.class_loader,
                )
            });

            if let Some(element) = class_element {
                current = element;
            } else if current.is_bare_array() || current.is_mixed() {
                // Bare `array` and `mixed` have unknown element types;
                // accessing any key yields `mixed`.
                current = PhpType::mixed();
            } else {
                return vec![];
            }
        }

        // After each segment, the resulting type might itself be an
        // alias (e.g. a shape value defined as another alias).
        if let Some(expanded) = crate::completion::type_resolution::resolve_type_alias_typed(
            &current,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        ) {
            current = expanded;
        }
    }

    let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
        &current,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    if classes.is_empty() {
        // No class matched (e.g. `list<Rule>`, `int`, `string`).
        // Return a type-string-only entry so the type information
        // is preserved for downstream consumers like foreach
        // element extraction.
        vec![ResolvedType::from_type_string(current)]
    } else {
        ResolvedType::from_classes_with_hint(classes, current)
    }
}

/// Classification of an array access index expression.
enum ArrayBracketSegment {
    /// A string-key access, e.g. `['items']`.
    StringKey(String),
    /// A numeric or variable index access, e.g. `[0]` or `[$i]`.
    ElementAccess,
}

/// Classify an array index expression as either a string key or generic
/// element access.
fn classify_array_index(index: &Expression<'_>) -> ArrayBracketSegment {
    if let Expression::Literal(Literal::String(s)) = index {
        let key = s
            .value
            .map(|v| bytes_to_str(v).to_string())
            .unwrap_or_else(|| {
                let raw_str = bytes_to_str(s.raw);
                crate::util::unquote_php_string(raw_str)
                    .unwrap_or(raw_str)
                    .to_string()
            });
        ArrayBracketSegment::StringKey(key)
    } else {
        ArrayBracketSegment::ElementAccess
    }
}

/// Insert a template substitution, unioning with any existing entry.
/// When two arguments bind to the same `@template T`, the resolved type
/// is the union of all inferred argument types (e.g. `T` from `$a: int`
/// and `$b: float` becomes `int|float`).
pub(crate) fn insert_or_union(subs: &mut HashMap<String, PhpType>, key: String, value: PhpType) {
    use std::collections::hash_map::Entry;
    match subs.entry(key) {
        Entry::Vacant(e) => {
            e.insert(value);
        }
        Entry::Occupied(mut e) => {
            let existing = e.get().clone();
            if existing == value {
                return;
            }
            let mut parts = match existing {
                PhpType::Union(parts) => parts,
                other => vec![other],
            };
            match value {
                PhpType::Union(new_parts) => {
                    for p in new_parts {
                        if !parts.contains(&p) {
                            parts.push(p);
                        }
                    }
                }
                other => {
                    if !parts.contains(&other) {
                        parts.push(other);
                    }
                }
            }
            e.insert(if parts.len() == 1 {
                parts.into_iter().next().unwrap()
            } else {
                PhpType::Union(parts)
            });
        }
    }
}

/// Build a template substitution map for a function-level `@template` call.
///
/// Uses the function's `template_bindings` to match template parameters to
/// their concrete types inferred from the call-site arguments.  Handles:
///   - Direct type: `@param T $bar` + `func(new Baz())` → `T = Baz`
///   - Array type: `@param T[] $items` + `func([new X()])` → `T = X`
///   - Generic wrapper: `@param array<TKey, TValue> $v` + `func($users)` →
///     positional resolution through the wrapper's generic arguments.
pub(crate) fn build_function_template_subs(
    func_info: &crate::types::FunctionInfo,
    arg_texts: &[String],
    rctx: &crate::completion::resolver::ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let mut subs = HashMap::new();

    for (tpl_name, param_name) in &func_info.template_bindings {
        let param_idx = match func_info
            .parameters
            .iter()
            .position(|p| p.name == param_name.as_str())
        {
            Some(idx) => idx,
            None => continue,
        };

        let provided_arg = arg_texts.get(param_idx).map(|t| t.trim());

        // Determine the binding mode by inspecting the parameter's
        // docblock type hint.  The type hint tells us how the template
        // param is embedded in the `@param` annotation.
        let param_hint = func_info
            .parameters
            .get(param_idx)
            .and_then(|p| p.type_hint.as_ref());
        let binding_mode = classify_template_binding(tpl_name, param_hint);

        // Fall back to the parameter's default value only for binding
        // modes where the default is meaningful (class-string<T> with
        // a `Foo::class` default, or direct bindings with `::class`).
        let default_value = func_info
            .parameters
            .get(param_idx)
            .and_then(|p| p.default_value.as_deref());
        let arg_text: &str = match provided_arg {
            Some(text) => text,
            None => match &binding_mode {
                TemplateBindingMode::ClassStringInner => match default_value {
                    Some(d) => d,
                    None => continue,
                },
                TemplateBindingMode::Direct => match default_value {
                    Some(d) if d.ends_with("::class") => d,
                    _ => continue,
                },
                _ => continue,
            },
        };

        match binding_mode {
            TemplateBindingMode::Direct => {
                if let Some(resolved_type) = Backend::resolve_arg_text_to_type(arg_text, rctx) {
                    insert_or_union(&mut subs, tpl_name.to_string(), resolved_type);
                }
            }
            TemplateBindingMode::CallableReturnType => {
                // `@param callable(...): T $cb` — extract the closure's
                // return type annotation from the argument text.
                if let Some(ret_type) =
                    crate::completion::source::helpers::extract_closure_return_type_from_text(
                        arg_text,
                    )
                {
                    insert_or_union(&mut subs, tpl_name.to_string(), ret_type);
                }
            }
            TemplateBindingMode::CallableParamType(position) => {
                // `@param Closure(T): void $cb` — extract the closure's
                // parameter type annotation at the given position.
                if let Some(param_type) =
                    crate::completion::source::helpers::extract_closure_param_type_from_text(
                        arg_text, position,
                    )
                {
                    insert_or_union(&mut subs, tpl_name.to_string(), param_type);
                }
            }
            TemplateBindingMode::ArrayElement => {
                // `@param T[] $items` — resolve individual array elements.
                // Empty array `[]` → element type is `never`.
                if arg_text.starts_with('[') && arg_text.ends_with(']') {
                    let inner = arg_text[1..arg_text.len() - 1].trim();
                    if inner.is_empty() {
                        // Empty array `[]` → element type is `never`.
                        subs.insert(tpl_name.to_string(), PhpType::never());
                    } else {
                        let first_elem =
                            crate::completion::conditional_resolution::split_text_args(inner);
                        if let Some(elem) = first_elem.first()
                            && let Some(resolved_type) =
                                Backend::resolve_arg_text_to_type(elem.trim(), rctx)
                        {
                            subs.insert(tpl_name.to_string(), resolved_type);
                        }
                    }
                } else if let Some(resolved_type) =
                    Backend::resolve_arg_text_to_type(arg_text, rctx)
                {
                    // Extract the element type from array-like types
                    // so we bind T to the element, not the whole array.
                    if let Some(elem_type) = resolved_type.extract_value_type(false) {
                        insert_or_union(&mut subs, tpl_name.to_string(), elem_type.clone());
                    } else {
                        insert_or_union(&mut subs, tpl_name.to_string(), resolved_type);
                    }
                }
            }
            TemplateBindingMode::ClassStringInner => {
                if let Some(resolved_type) = Backend::resolve_arg_text_to_type(arg_text, rctx) {
                    // Unwrap `class-string<X>` → `X` so that the
                    // substitution doesn't double-wrap.
                    let unwrapped = match resolved_type {
                        PhpType::ClassString(Some(inner)) => *inner,
                        _ => resolved_type,
                    };
                    insert_or_union(&mut subs, tpl_name.to_string(), unwrapped);
                }
            }
            TemplateBindingMode::GenericWrapper(ref wrapper_name, tpl_position) => {
                // For `@param array<TKey, TValue> $value` with a variable
                // argument like `$users`, resolve the variable's raw type
                // string (e.g. `User[]`, `array<int, User>`) and extract
                // the positional generic argument.
                if is_array_like_wrapper(wrapper_name)
                    && arg_text.starts_with('$')
                    && let Some(resolved) = resolve_arg_variable_raw_type(arg_text, rctx)
                    && let Some(concrete) = extract_array_type_at_position(&resolved, tpl_position)
                {
                    subs.insert(tpl_name.to_string(), concrete);
                    continue;
                }
                // Array literal argument for array-like wrappers:
                // `[1, 2, 3]` for `@param array<T>` → infer T from elements.
                if is_array_like_wrapper(wrapper_name)
                    && arg_text.starts_with('[')
                    && arg_text.ends_with(']')
                {
                    let inner = arg_text[1..arg_text.len() - 1].trim();
                    if inner.is_empty() {
                        // Empty array `[]` → element type is `never`.
                        subs.insert(tpl_name.to_string(), PhpType::never());
                        continue;
                    } else {
                        let elems =
                            crate::completion::conditional_resolution::split_text_args(inner);
                        // For `array<T>` (position 0 with 1 generic arg) or
                        // `array<K, V>` (position 1 = value), infer from
                        // element values.  For position 0 in a 2-arg generic
                        // (the key), infer from keys if available.
                        if let Some(elem) = elems.first()
                            && let Some(resolved_type) =
                                Backend::resolve_arg_text_to_type(elem.trim(), rctx)
                        {
                            subs.insert(tpl_name.to_string(), resolved_type);
                            continue;
                        }
                    }
                }
                // Special case: unwrap class-string<class-string<T>> to class-string<T>
                if wrapper_name == "class-string"
                    && tpl_position == 0
                    && let Some(resolved_type) = Backend::resolve_arg_text_to_type(arg_text, rctx)
                {
                    if let Some(inner) = resolved_type.unwrap_class_string_inner() {
                        subs.insert(tpl_name.to_string(), inner.clone());
                    } else {
                        subs.insert(tpl_name.to_string(), resolved_type);
                    }
                }
                // ── Class generic wrapper resolution ────────────────
                // For `@param Container<TItem> $c` where the argument
                // is a subclass like `FooContainer extends Container<Foo>`,
                // resolve the argument type and walk its @extends chain
                // to find the wrapper class's generic arg at the right
                // position.
                if !is_array_like_wrapper(wrapper_name)
                    && wrapper_name != "class-string"
                    && let Some(resolved_type) = Backend::resolve_arg_text_to_type(arg_text, rctx)
                    && let Some(concrete) = extract_generic_arg_from_ancestor(
                        &resolved_type,
                        wrapper_name,
                        tpl_position,
                        rctx,
                    )
                {
                    subs.insert(tpl_name.to_string(), concrete);
                    continue;
                }
                // When array-type extraction fails (e.g. bare `array`
                // property without generic annotation), do NOT fall back
                // to a Direct resolve — that would bind the template
                // param to the whole argument type instead of its
                // positional generic arg.  Leave it unbound so the
                // "fill in unbound" code below maps it to its declared
                // upper bound or `mixed`.
            }
        }
    }

    // ── Fill in unbound function-level template params ──────
    // Any template parameter that was not bound from call-site
    // arguments is replaced with its declared upper bound
    // (`@template T of Foo` → `Foo`) or `mixed`.  This follows
    // PHPStan's `resolveToBounds()` semantics and prevents raw
    // template names like `TReduceReturnType` from leaking into
    // parameter and return types.
    for tpl_name in &func_info.template_params {
        let tpl_key = tpl_name.to_string();
        subs.entry(tpl_key).or_insert_with(|| {
            func_info
                .template_param_bounds
                .get(tpl_name)
                .cloned()
                .unwrap_or_else(PhpType::mixed)
        });
    }

    subs
}

/// Resolve a variable argument to its raw type string.
///
/// For `$pens` with `/** @var Pen[] $pens */`, returns `Some("Pen[]")`.
/// For `$users` with `/** @var array<int, User> $users */`, returns
/// `Some("array<int, User>")`.
///
/// Tries docblock annotations first, then falls back to AST-based
/// raw type inference.
fn resolve_arg_variable_raw_type(
    arg_text: &str,
    rctx: &crate::completion::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    let var_name = arg_text.trim();
    if !var_name.starts_with('$') {
        return None;
    }

    // ── Property chain: `$this->items`, `$obj->prop` ────────────
    // When the argument is a property access chain, resolve the base
    // object's type and look up the property's type hint.  This is
    // needed for template substitution in calls like
    // `array_any($this->items, fn($item) => …)` where `$this->items`
    // is `array<int, PurchaseFileProduct>` after generic substitution.
    if let Some(arrow_pos) = var_name.find("->") {
        let base = &var_name[..arrow_pos];
        let prop = &var_name[arrow_pos + 2..];
        // Only handle simple single-level property access for now.
        if !prop.is_empty() && !prop.contains("->") && !prop.contains('(') {
            let base_classes = ResolvedType::into_arced_classes(
                crate::completion::resolver::resolve_target_classes(
                    base,
                    crate::types::AccessKind::Arrow,
                    rctx,
                ),
            );
            for cls in &base_classes {
                if let Some(hint) =
                    crate::inheritance::resolve_property_type_hint(cls, prop, rctx.class_loader)
                {
                    return Some(hint);
                }
            }
        }
    }

    // 1. Try docblock annotation (@var).
    if let Some(raw) = crate::docblock::find_iterable_raw_type_in_source(
        rctx.content,
        rctx.cursor_offset as usize,
        var_name,
    )
    .map(|t| crate::util::resolve_php_type_names(&t, rctx.class_loader))
    {
        return Some(raw);
    }

    // 2. When the diagnostic scope cache is active (and not still being
    //    built), read the variable's type from the pre-computed forward-
    //    walked scope snapshots.  This avoids hitting the backward
    //    scanner during diagnostic collection.
    if super::forward_walk::is_diagnostic_scope_active()
        && !super::forward_walk::is_building_scopes()
    {
        let prefixed = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        if let Some(types) =
            super::forward_walk::lookup_diagnostic_scope(&prefixed, rctx.cursor_offset)
        {
            return Some(ResolvedType::types_joined(&types));
        }
    }

    // 3. When a scope_var_resolver is available (forward walker is
    //    active on either diagnostic or completion path), read from
    //    the in-progress ScopeState.  If the variable isn't there,
    //    it hasn't been assigned yet — return None rather than
    //    falling through to resolve_variable_types which would
    //    re-enter the forward walker and cause stack overflow.
    if let Some(resolver) = rctx.scope_var_resolver {
        let prefixed = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        let from_scope = resolver(&prefixed);
        if from_scope.is_empty() {
            return None;
        }
        return Some(ResolvedType::types_joined(&from_scope));
    }

    // 4. During the build phase, the forward walker is the authority.
    //    If the variable isn't in the scope cache, don't fall through
    //    to the backward scanner — return None so the caller treats
    //    it as unresolved.
    if super::forward_walk::is_building_scopes() {
        return None;
    }

    // 5. Fall back to unified variable resolution pipeline (backward
    //    scanner).  This path is only reached for interactive features
    //    (hover, completion, goto-def) where no scope cache is active
    //    and no scope_var_resolver was provided.
    //
    // Guard: resolve_variable_types is designed for bare `$variable`
    // names.  Complex expressions (array access like `$arr['key']`,
    // comparisons like `$x === 'foo'`, boolean chains, null coalescing)
    // are not variable names and will never match a scope entry.
    // Skip them to avoid wasted backward scans and fallthrough noise.
    if var_name.contains("->")
        || var_name.contains("::")
        || var_name.contains('[')
        || var_name.contains("===")
        || var_name.contains("&&")
        || var_name.contains("??")
        || var_name.contains("||")
    {
        return None;
    }

    let default_class = crate::types::ClassInfo::default();
    let current_class = rctx.current_class.unwrap_or(&default_class);
    let resolved = super::resolution::resolve_variable_types(
        var_name,
        current_class,
        rctx.all_classes,
        rctx.content,
        rctx.cursor_offset,
        rctx.class_loader,
        Loaders::with_function(rctx.function_loader),
    );
    if resolved.is_empty() {
        None
    } else {
        Some(ResolvedType::types_joined(&resolved))
    }
}

/// Extract the concrete type at `position` from an array type string.
///
/// For array types with two generic parameters (key + value):
/// - `array<int, User>` at position 0 → `"int"`, position 1 → `"User"`
/// - `User[]` at position 0 → `"int"` (implicit key), position 1 → `"User"`
/// - `list<User>` at position 0 → `"int"`, position 1 → `"User"`
///
/// For single-param forms:
/// - `array<User>` at position 0 → `"User"`
fn extract_array_type_at_position(ty: &PhpType, position: usize) -> Option<PhpType> {
    match position {
        0 => ty.extract_key_type(false).cloned(),
        1 => ty.extract_value_type(false).cloned(),
        _ => None,
    }
}

/// Whether a wrapper type name should be treated as array-like for
/// positional generic argument extraction.
///
/// When `@param Wrapper<TKey, TValue> $value` binds a template param
/// via `GenericWrapper`, and the wrapper is an array-like type, we can
/// resolve the argument variable's raw type (e.g. `User[]`) and extract
/// the positional generic component (key at 0, value at 1).
///
/// This covers `array`, `iterable`, `list`, and common Laravel/PHPStan
/// collection interfaces whose generic args follow `<TKey, TValue>`.
pub(crate) fn is_array_like_wrapper(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "array" | "list" | "non-empty-array" | "non-empty-list" | "iterable"
    ) || crate::util::short_name(name).eq_ignore_ascii_case("arrayable")
}

/// Resolve function, method, and static method calls to their return
/// types.
fn resolve_rhs_call<'b>(
    call: &'b Call<'b>,
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    match call {
        Call::Function(func_call) => resolve_rhs_function_call(func_call, expr, ctx),
        Call::Method(method_call) => resolve_rhs_method_call_inner(
            method_call.object,
            &method_call.method,
            &method_call.argument_list,
            ctx,
        ),
        Call::NullSafeMethod(method_call) => resolve_rhs_method_call_inner(
            method_call.object,
            &method_call.method,
            &method_call.argument_list,
            ctx,
        ),
        Call::StaticMethod(static_call) => resolve_rhs_static_call(static_call, ctx),
    }
}

/// Resolve a plain function call: `someFunc()`, array functions, variable
/// invocations (`$fn()`), and conditional return types.
fn resolve_rhs_function_call<'b>(
    func_call: &'b FunctionCall<'b>,
    expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class_name: &str = &ctx.current_class.name;
    let all_classes = ctx.all_classes;
    let content = ctx.content;
    let class_loader = ctx.class_loader;
    let function_loader = ctx.function_loader();

    // ── First-class callable invocation: `Foo::method(...)()` ───
    // When the callee is a partial application (first-class callable),
    // invoking it with `()` returns the underlying method's return
    // type.  Delegate to the matching call-resolution path.
    if let Expression::PartialApplication(pa) = func_call.function {
        use mago_syntax::ast::ast::partial_application::PartialApplication;
        match pa {
            PartialApplication::StaticMethod(sma) => {
                // For first-class callable invocation through late-static-binding
                // targets (self::, static::, parent::), preserve `static` in the
                // return type rather than resolving to the concrete class name.
                let is_late_static = matches!(
                    sma.class,
                    Expression::Self_(_) | Expression::Static(_) | Expression::Parent(_)
                );
                if is_late_static {
                    // Look up the method's original return type to check if
                    // it contains static/self/$this before resolution replaces it.
                    let method_name = match sma.method {
                        ClassLikeMemberSelector::Identifier(ident) => {
                            bytes_to_str(ident.value).to_string()
                        }
                        _ => String::new(),
                    };
                    if !method_name.is_empty() {
                        // Check current class first, then walk parent chain.
                        let method_ret = ctx
                            .current_class
                            .get_method_ci(&method_name)
                            .and_then(|m| m.return_type.clone());
                        let method_ret = method_ret.or_else(|| {
                            // Walk parent chain to find the method.
                            let mut parent_name = ctx
                                .current_class
                                .parent_class
                                .as_ref()
                                .map(|a| a.to_string());
                            while let Some(ref p) = parent_name {
                                if let Some(cls) = (ctx.class_loader)(p) {
                                    if let Some(m) = cls.get_method_ci(&method_name) {
                                        return m.return_type.clone();
                                    }
                                    parent_name = cls.parent_class.as_ref().map(|a| a.to_string());
                                } else {
                                    break;
                                }
                            }
                            None
                        });
                        if let Some(ref ret) = method_ret
                            && ret.contains_self_ref()
                        {
                            return vec![ResolvedType::from_type_string(PhpType::static_())];
                        }
                    }
                }
                // Build a synthetic StaticMethodCall and resolve it.
                let synthetic = mago_syntax::ast::ast::call::StaticMethodCall {
                    class: sma.class,
                    double_colon: sma.double_colon,
                    method: sma.method.clone(),
                    argument_list: func_call.argument_list.clone(),
                };
                return resolve_rhs_static_call(&synthetic, ctx);
            }
            PartialApplication::Method(ma) => {
                let receiver_is_this = matches!(
                    ma.object,
                    Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this"
                );
                if receiver_is_this {
                    // Look up the method's original return type to check if
                    // it contains static/self/$this.
                    let method_name = match ma.method {
                        ClassLikeMemberSelector::Identifier(ident) => {
                            bytes_to_str(ident.value).to_string()
                        }
                        _ => String::new(),
                    };
                    if !method_name.is_empty() {
                        let method_ret = ctx
                            .current_class
                            .get_method_ci(&method_name)
                            .and_then(|m| m.return_type.clone());
                        let method_ret = method_ret.or_else(|| {
                            let mut parent_name = ctx
                                .current_class
                                .parent_class
                                .as_ref()
                                .map(|a| a.to_string());
                            while let Some(ref p) = parent_name {
                                if let Some(cls) = (ctx.class_loader)(p) {
                                    if let Some(m) = cls.get_method_ci(&method_name) {
                                        return m.return_type.clone();
                                    }
                                    parent_name = cls.parent_class.as_ref().map(|a| a.to_string());
                                } else {
                                    break;
                                }
                            }
                            None
                        });
                        if let Some(ref ret) = method_ret
                            && ret.contains_self_ref()
                        {
                            return vec![ResolvedType::from_type_string(PhpType::static_())];
                        }
                    }
                }
                return resolve_rhs_method_call_inner(
                    ma.object,
                    &ma.method,
                    &func_call.argument_list,
                    ctx,
                );
            }
            PartialApplication::Function(fa) => {
                // `strlen(...)()` — resolve the inner function name.
                if let Expression::Identifier(ident) = fa.function {
                    let name = bytes_to_str(ident.value()).to_string();
                    let function_loader = ctx.function_loader();
                    if let Some(fl) = function_loader
                        && let Some(func_info) = fl(&name)
                        && let Some(ref ret) = func_info.return_type
                    {
                        let resolved =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                ret,
                                &ctx.current_class.name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if !resolved.is_empty() {
                            return ResolvedType::from_classes_with_hint(resolved, ret.clone());
                        }
                        return vec![resolved_type_with_lookup(
                            ret.clone(),
                            &ctx.current_class.name,
                            ctx.all_classes,
                            ctx.class_loader,
                        )];
                    }
                }
            }
        }
    }

    let func_name = match func_call.function {
        Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
        _ => None,
    };

    // ── Known array functions ────────────────────────
    // For element-extracting functions (array_pop, etc.)
    // resolve to the element ClassInfo directly.
    if let Some(ref name) = func_name
        && let Some(element_type) = super::raw_type_inference::resolve_array_func_element_type(
            name,
            &func_call.argument_list,
            ctx,
        )
    {
        let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
            &element_type,
            current_class_name,
            all_classes,
            class_loader,
        );
        if !resolved.is_empty() {
            return ResolvedType::from_classes_with_hint(resolved, element_type);
        }
    }

    // For type-preserving functions (array_filter, array_values, etc.)
    // the output has the same iterable type as the input array.
    // Return the full type string (e.g. `list<User>`) so that
    // downstream consumers (foreach, array access, hover) see the
    // element type without needing the raw-type pipeline's fallback.
    if let Some(ref name) = func_name
        && let Some(raw_type) = super::raw_type_inference::resolve_array_func_raw_type(
            name,
            &func_call.argument_list,
            ctx,
        )
    {
        let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
            &raw_type,
            current_class_name,
            all_classes,
            class_loader,
        );
        if !resolved.is_empty() {
            return ResolvedType::from_classes_with_hint(resolved, raw_type);
        }
        // The type string is informative (e.g. `list<User>`) but
        // doesn't resolve to a class — return as type-string-only.
        return vec![resolved_type_with_lookup(
            raw_type,
            current_class_name,
            all_classes,
            class_loader,
        )];
    }

    if let Some(ref name) = func_name
        && let Some(fl) = function_loader
        && let Some(func_info) = fl(name)
    {
        // Try conditional return type first
        if let Some(ref cond) = func_info.conditional_return {
            let var_resolver = build_var_resolver_from_ctx(ctx);
            let tpl = crate::completion::types::conditional::TemplateContext::with_params(
                &func_info.template_params,
            );
            let resolved_type = resolve_conditional_with_args(
                cond,
                &func_info.parameters,
                &func_call.argument_list,
                Some(&var_resolver),
                Some(current_class_name),
                class_loader,
                &tpl,
            );
            if let Some(ref ty) = resolved_type {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    ty,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return ResolvedType::from_classes_with_hint(resolved, ty.clone());
                }
                // The conditional resolved to a non-class type (e.g.
                // `list<string>`, `int`).  Return it as a type-string-only
                // entry so downstream consumers see the resolved type.
                return vec![resolved_type_with_lookup(
                    ty.clone(),
                    current_class_name,
                    all_classes,
                    class_loader,
                )];
            }
        }

        // ── Function-level @template substitution ────────────
        // When the function has template params and bindings,
        // infer concrete types from the arguments and apply
        // substitution to the return type before resolving.
        if !func_info.template_params.is_empty() && func_info.return_type.is_some() {
            let arg_texts = super::raw_type_inference::extract_arg_texts_from_ast(
                &func_call.argument_list,
                content,
            );
            let rctx = ctx.as_resolution_ctx();
            let subs = build_function_template_subs(&func_info, &arg_texts, &rctx);
            if !subs.is_empty()
                && let Some(ref ret) = func_info.return_type
            {
                let substituted = ret.substitute(&subs);
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    &substituted,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return ResolvedType::from_classes_with_hint(resolved, substituted);
                }
                // The substituted type didn't resolve to any classes
                // (e.g. `mixed|null`, `int|null`, `array-key|null`).
                // Return it as a type-string-only entry so that
                // downstream consumers see the substituted type
                // instead of the raw template name.
                return vec![ResolvedType::from_type_string(substituted)];
            }
        }

        if let Some(ref ret) = func_info.return_type {
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                ret,
                current_class_name,
                all_classes,
                class_loader,
            );
            if !resolved.is_empty() {
                return ResolvedType::from_classes_with_hint(resolved, ret.clone());
            }
            // The function has a return type string but
            // `type_hint_to_classes_typed` found no matching class (e.g.
            // `list<Widget>`, `int`, `array{name: string}`).  Return a
            // type-string-only entry so that consumers reading
            // `.type_string` still get the information.
            //
            // When the return type is `void`, PHP yields `null` at
            // runtime — mirror that so the variable type is correct.
            if *ret == PhpType::void() {
                return vec![ResolvedType::from_type_string(PhpType::null())];
            }
            return vec![resolved_type_with_lookup(
                ret.clone(),
                current_class_name,
                all_classes,
                class_loader,
            )];
        }
    }

    // ── Source-scanning fallback for named function calls ────
    // When the function_loader is unavailable or could not resolve the
    // function (e.g. multi-namespace files where the file-level namespace
    // differs from the function's namespace), scan the source text for
    // the function's docblock @return annotation.
    let loader_found = func_name
        .as_ref()
        .and_then(|name| function_loader.and_then(|fl| fl(name)))
        .is_some();
    if let Some(ref name) = func_name
        && !loader_found
        && let Some(ret) =
            crate::completion::source::helpers::extract_function_return_from_source(name, content)
    {
        let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
            &ret,
            current_class_name,
            all_classes,
            class_loader,
        );
        if !resolved.is_empty() {
            return ResolvedType::from_classes_with_hint(resolved, ret);
        }
        if ret == PhpType::void() {
            return vec![ResolvedType::from_type_string(PhpType::null())];
        }
        return vec![resolved_type_with_lookup(
            ret,
            current_class_name,
            all_classes,
            class_loader,
        )];
    }

    // ── Variable invocation: $fn() ──────────────────
    // When the callee is a variable (not a named function),
    // resolve the variable's type annotation for a
    // callable/Closure return type, or look for a
    // closure/arrow-function literal in the assignment.
    if let Expression::Variable(Variable::Direct(dv)) = func_call.function {
        let var_name = bytes_to_str(dv.name).to_string();
        let offset = expr.span().start.offset as usize;

        // 1. Try docblock annotation:
        //    `@var Closure(): User $fn` or
        //    `@param callable(int): Response $fn`
        if let Some(raw_type) =
            crate::docblock::find_iterable_raw_type_in_source(content, offset, &var_name)
                .map(|t| crate::util::resolve_php_type_names(&t, class_loader))
            && let Some(ret_type) = raw_type.callable_return_type()
        {
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                ret_type,
                current_class_name,
                all_classes,
                class_loader,
            );
            if !resolved.is_empty() {
                return ResolvedType::from_classes_with_hint(resolved, ret_type.clone());
            }
        }

        // 2. Scan for closure literal assignment and
        //    extract native return type hint.
        if let Some(ret) =
            crate::completion::source::helpers::extract_closure_return_type_from_assignment(
                &var_name,
                content,
                ctx.cursor_offset,
            )
        {
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                &ret,
                current_class_name,
                all_classes,
                class_loader,
            );
            if !resolved.is_empty() {
                return ResolvedType::from_classes_with_hint(resolved, ret);
            }
        }

        // 3. Scan backward for first-class callable assignment:
        //    `$fn = strlen(...)`, `$fn = $obj->method(...)`, or
        //    `$fn = ClassName::staticMethod(...)`.
        //    Resolve the underlying function/method's return type.
        let rctx = ctx.as_resolution_ctx();
        if let Some(ret) =
            crate::completion::source::helpers::extract_first_class_callable_return_type(
                &var_name, &rctx,
            )
        {
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                &ret,
                current_class_name,
                all_classes,
                class_loader,
            );
            if !resolved.is_empty() {
                return ResolvedType::from_classes_with_hint(resolved, ret);
            }
        }

        // 4. Resolve the variable's type and check for __invoke().
        //    When $f holds an object with an __invoke() method,
        //    $f() should return __invoke()'s return type.
        let rctx = ctx.as_resolution_ctx();
        let var_classes =
            ResolvedType::into_arced_classes(crate::completion::resolver::resolve_target_classes(
                &var_name,
                crate::types::AccessKind::Arrow,
                &rctx,
            ));
        for owner in &var_classes {
            if let Some(invoke) = owner.get_method("__invoke")
                && let Some(ref ret) = invoke.return_type
            {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    ret,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return ResolvedType::from_classes_with_hint(resolved, ret.clone());
                }
                // When type_hint_to_classes_typed can't resolve the return
                // type (e.g. `Item[]` where the `[]` suffix prevents
                // class lookup), emit a type-string-only entry so that
                // callers like foreach resolution can still extract the
                // element type via `PhpType::extract_value_type`.
                if !ret.is_empty() {
                    return vec![resolved_type_with_lookup(
                        ret.clone(),
                        current_class_name,
                        all_classes,
                        class_loader,
                    )];
                }
            }
        }
    }

    // ── General expression invocation: ($expr)() ────
    // When the callee is an arbitrary expression (e.g.
    // `($this->foo)()`, `(getFactory())()`, etc.), resolve
    // the expression to classes and check for __invoke().
    let callee_expr = match func_call.function {
        Expression::Parenthesized(p) => p.expression,
        other => other,
    };
    // Skip if we already handled it as a variable above.
    if !matches!(callee_expr, Expression::Variable(Variable::Direct(_))) {
        // ── Directly invoked closure / arrow function ────
        // `(fn (): Foo => …)()` or `(function (): Foo { … })()`
        // Extract the return type from the literal instead of going
        // through `__invoke()` on the generic `Closure` stub.
        if let Some(parsed_ret_type) = extract_closure_or_arrow_return_type(callee_expr) {
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                &parsed_ret_type,
                current_class_name,
                all_classes,
                class_loader,
            );
            if !resolved.is_empty() {
                return ResolvedType::from_classes_with_hint(resolved, parsed_ret_type);
            }
        }

        let callee_results = resolve_rhs_expression(callee_expr, ctx);
        for rt in &callee_results {
            if let Some(ref owner_cls) = rt.class_info
                && let Some(invoke) = owner_cls.get_method("__invoke")
                && let Some(ref ret) = invoke.return_type
            {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    ret,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return ResolvedType::from_classes_with_hint(resolved, ret.clone());
                }
                if !ret.is_empty() {
                    return vec![resolved_type_with_lookup(
                        ret.clone(),
                        current_class_name,
                        all_classes,
                        class_loader,
                    )];
                }
            }
        }
    }

    vec![]
}

/// Resolve an instance method call: `$this->method()`, `$var->method()`,
/// chained calls, and other object expressions via AST-based resolution.
/// Resolve a method call (regular or null-safe) from its constituent parts.
///
/// Both `$obj->method()` and `$obj?->method()` share the same resolution
/// logic — the null-safe operator only affects whether `null` propagates
/// at runtime, not which class the method belongs to.
fn resolve_rhs_method_call_inner<'b>(
    object: &'b Expression<'b>,
    method: &'b ClassLikeMemberSelector<'b>,
    argument_list: &'b ArgumentList<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let method_name = match method {
        ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
        // Variable method name (`$obj->$method()`) — can't resolve statically.
        _ => return vec![],
    };
    // Resolve the object expression to candidate owner classes.
    // Keep the full `ResolvedType` for non-$this variables and chain
    // expressions so that the receiver's generic type string (e.g.
    // `Builder<Article>`) is available when the method returns
    // `static`/`self`/`$this`.
    let (owner_classes, receiver_resolved): (Vec<Arc<ClassInfo>>, Vec<ResolvedType>) =
        if let Expression::Variable(Variable::Direct(dv)) = object
            && dv.name == b"$this"
        {
            let classes: Vec<Arc<ClassInfo>> = ctx
                .all_classes
                .iter()
                .find(|c| c.name == ctx.current_class.name)
                .map(Arc::clone)
                .into_iter()
                .collect();
            (classes, vec![])
        } else if let Expression::Variable(Variable::Direct(dv)) = object {
            let var = bytes_to_str(dv.name).to_string();
            // Check match-arm narrowing override first — when inside
            // a match(true) arm, the variable may be narrowed to a
            // specific class by the arm's instanceof condition.
            let resolved = match ctx.match_arm_narrowing.get(&var).cloned() {
                Some(overridden) => overridden,
                None => resolve_var_types(&var, ctx, object.span().end.offset),
            };
            if !resolved.is_empty() {
                let classes = ResolvedType::into_arced_classes(resolved.clone());
                (classes, resolved)
            } else {
                // Fall back to resolve_target_classes when the
                // variable resolution pipeline returns nothing (e.g.
                // for parameters that are resolved through the
                // completion pipeline's subject resolution).
                let classes: Vec<Arc<ClassInfo>> = ResolvedType::into_arced_classes(
                    crate::completion::resolver::resolve_target_classes(
                        &var,
                        crate::types::AccessKind::Arrow,
                        &ctx.as_resolution_ctx(),
                    ),
                );
                (classes, vec![])
            }
        } else {
            // Handle non-variable object expressions like
            // `(new Factory())->create()`, `getService()->method()`,
            // or chained calls by recursively resolving the expression.
            let resolved = resolve_rhs_expression(object, ctx);
            let classes = ResolvedType::into_arced_classes(resolved.clone());
            (classes, resolved)
        };

    let arg_texts =
        super::raw_type_inference::extract_arg_texts_from_ast(argument_list, ctx.content);
    let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();
    let text_args = arg_texts.join(", ");
    let rctx = ctx.as_resolution_ctx();
    let var_resolver = build_var_resolver_from_ctx(ctx);

    // ── Expand union generic receivers ──────────────────────────
    // When the receiver is a union type like `C<A>|C<B>`, the variable
    // resolution pipeline returns a single ResolvedType with a Union
    // type_string and one class_info.  To resolve the method on each
    // branch separately (so `->get()` yields `A|B` not just `A`),
    // expand the union into separate owner entries with per-branch
    // generic substitutions applied.
    let (owner_classes, receiver_resolved) =
        expand_union_generic_owners(owner_classes, receiver_resolved, ctx);

    let is_union = owner_classes.len() > 1;
    let mut union_results: Vec<ResolvedType> = Vec::new();

    for (idx, owner) in owner_classes.iter().enumerate() {
        // Build class-level template substitutions from the receiver's
        // generic type string (e.g. `Collection<int, User>` maps
        // `TKey => int, TValue => User`).  This ensures method return
        // types like `TValue` are concretised when the receiver was
        // annotated with generic arguments via `@var`.
        let class_level_subs: HashMap<String, PhpType> = receiver_resolved
            .get(idx)
            .or_else(|| receiver_resolved.first())
            .and_then(|rt| match &rt.type_string {
                PhpType::Generic(_, args)
                    if !args.is_empty()
                        && !owner.template_params.is_empty()
                        && !args.iter().any(|a| a.is_self_like()) =>
                {
                    Some(
                        owner
                            .template_params
                            .iter()
                            .zip(args.iter())
                            .map(|(name, ty)| (name.to_string(), ty.clone()))
                            .collect(),
                    )
                }
                _ => None,
            })
            .unwrap_or_default();

        let method_template_subs =
            Backend::build_method_template_subs(owner, &method_name, &arg_refs, &rctx);

        // ── @psalm-if-this-is template inference ────────────────
        // When a method has a `@psalm-if-this-is` annotation and
        // method-level template parameters remain unresolved (no
        // arguments to infer from), match the receiver's concrete
        // type against the pattern to compute substitutions.
        let if_this_is_subs: HashMap<String, PhpType> = owner
            .get_method_ci(&method_name)
            .and_then(|m| m.if_this_is.as_ref())
            .and_then(|pattern| {
                let receiver_type = receiver_resolved
                    .get(idx)
                    .or_else(|| receiver_resolved.first())
                    .map(|rt| &rt.type_string)?;
                let method = owner.get_method_ci(&method_name)?;
                Some(infer_if_this_is_subs(
                    pattern,
                    receiver_type,
                    &method.template_params,
                    &method.template_param_bounds,
                ))
            })
            .unwrap_or_default();

        // Merge class-level, method-level, and if-this-is subs.
        // if-this-is overrides method-level defaults (which may be
        // `mixed` for unresolvable templates). Method-level takes
        // precedence over class-level.
        let mut template_subs = class_level_subs;
        template_subs.extend(method_template_subs);
        template_subs.extend(if_this_is_subs);
        let mr_ctx = MethodReturnCtx {
            all_classes: ctx.all_classes,
            class_loader: ctx.class_loader,
            template_subs: &template_subs,
            var_resolver: Some(&var_resolver),
            cache: ctx.resolved_class_cache,
            calling_class_name: Some(&ctx.current_class.name),
            is_static: false,
        };
        // Recover the effective return type string from the method.
        // Look up the method on the (possibly merged) owner and apply
        // the same template substitution that
        // `resolve_method_return_types_with_args` used internally,
        // then replace `static`/`self`/`$this` with the owner class
        // name (or the receiver's full generic type when available)
        // so that e.g. `static[]` becomes `Country[]` and a bare
        // `static` on `Builder<Article>` becomes `Builder<Article>`.
        // Try the owner directly first — it may already be fully resolved
        // with generic substitutions applied.  The cache is keyed by bare
        // FQN and returns the un-substituted base class, so prefer the
        // owner's own method to preserve template substitutions.
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            owner,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        let owner_method = owner.get_method_ci(&method_name);
        let merged_method = merged.get_method_ci(&method_name);
        // Prefer the merged method's return type when the owner's method
        // has no docblock override (return_type == native_return_type).
        // The merged method carries inherited types from interfaces/parents
        // with template substitutions already applied (e.g. `V|null` →
        // `User|null` from `@implements Collection<string, User>`).
        let method_ref = match (owner_method, merged_method) {
            (Some(om), Some(mm))
                if om.return_type == om.native_return_type
                    && mm.return_type != mm.native_return_type =>
            {
                Some(mm)
            }
            (Some(om), _) => Some(om),
            (None, Some(mm)) => Some(mm),
            (None, None) => {
                // Method not found — fall back to __call's return type.
                merged.get_method_ci("__call")
            }
        };
        let ret_type_string = method_ref.and_then(|m| m.return_type.as_ref()).map(|ret| {
            let substituted = if !template_subs.is_empty() {
                ret.substitute(&template_subs).simplified()
            } else {
                ret.clone()
            };
            // Resolve `parent` to the concrete parent class name before
            // any self/static replacement so that downstream consumers
            // see a real FQN instead of the keyword.
            let substituted = if substituted.is_parent_ref() {
                owner
                    .parent_class
                    .as_ref()
                    .map(|p| PhpType::Named(p.to_string()))
                    .unwrap_or(substituted)
            } else {
                substituted
            };
            // When the return type contains `static`/`self`/`$this`
            // and the receiver was resolved with generic parameters,
            // use the receiver's full type (e.g. `Builder<Article>`)
            // for substitution so the generics are preserved.
            let receiver_type = if substituted.contains_self_ref() {
                receiver_type_for_owner(&receiver_resolved, &owner.name)
            } else {
                None
            };
            match receiver_type {
                Some(rt) => substituted.replace_self_with_type(&rt),
                None => substituted.replace_self(&owner.fqn()),
            }
        });

        let results = Backend::resolve_method_return_types_with_args(
            owner,
            &method_name,
            &text_args,
            &mr_ctx,
        );
        if !results.is_empty() {
            let classes: Vec<Arc<ClassInfo>> = results;
            // When the method has a conditional return type, the
            // resolved classes came from evaluating the conditional
            // (e.g. `$type is class-string<T> ? T : mixed` resolved
            // to the concrete class).  In that case, using the
            // method's declared return type (typically `mixed`) as
            // the type hint would be misleading.  Skip it so that
            // `from_classes` uses the resolved class names instead.
            let has_conditional = merged
                .get_method_ci(&method_name)
                .is_some_and(|m| m.conditional_return.is_some());
            let effective_hint = if has_conditional {
                None
            } else {
                ret_type_string
            };
            let owner_results = match effective_hint {
                Some(hint) => ResolvedType::from_classes_with_hint(classes, hint),
                None => ResolvedType::from_classes(classes),
            };
            if !is_union {
                return owner_results;
            }
            ResolvedType::extend_unique(&mut union_results, owner_results);
            continue;
        }

        // The method has a return type string but `type_hint_to_classes_typed`
        // found no matching class (e.g. `list<Widget>`, `int`,
        // `array{name: string}`).  Return a type-string-only entry so
        // that consumers reading `.type_string` (hover, foreach
        // resolution, null-coalesce stripping) still get the information.
        //
        // Return the type string even for non-informative types like
        // `array` or `mixed` — a correct-but-vague type is better
        // than keeping the previous (wrong) type after reassignment.
        // Skip only `void` (void methods don't produce a value).
        // Also expand type aliases before returning so that
        // `@phpstan-type UserList array<int, User>` with
        // `@return UserList` is expanded to its concrete type.
        if let Some(ref hint) = ret_type_string {
            let expanded = crate::completion::type_resolution::resolve_type_alias_typed(
                hint,
                &owner.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            let parsed_effective = match expanded {
                Some(e) => e,
                None => hint.clone(),
            };
            if parsed_effective == PhpType::void() {
                let owner_results = vec![ResolvedType::from_type_string(PhpType::null())];
                if !is_union {
                    return owner_results;
                }
                ResolvedType::extend_unique(&mut union_results, owner_results);
                continue;
            }
            let owner_results = vec![resolved_type_with_lookup(
                parsed_effective,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            )];
            if !is_union {
                return owner_results;
            }
            ResolvedType::extend_unique(&mut union_results, owner_results);
            continue;
        }

        // Body return type inference fallback: when the method has no
        // declared return type and no @return docblock, try to infer
        // the return type from the method body.  This handles non-class
        // types (list<Foo>, int, array shapes) that
        // resolve_method_return_types_with_args cannot represent.
        if method_ref
            .is_some_and(|m| m.return_type.is_none() && m.name_offset != 0 && !m.is_virtual)
            && let Some(inferred) = crate::completion::call_resolution::try_infer_body_return_type(
                &owner.fqn(),
                method_ref.unwrap(),
            )
            && !inferred.is_void()
            && !inferred.is_mixed()
        {
            let owner_results = vec![resolved_type_with_lookup(
                inferred,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            )];
            if !is_union {
                return owner_results;
            }
            ResolvedType::extend_unique(&mut union_results, owner_results);
            continue;
        }
    }

    // For intersection types, filter out `mixed` when concrete types exist.
    // When a receiver is an intersection like `IChild&IParent<C>`, each member
    // resolves the method independently: the unparameterized interface may
    // return `mixed` while the parameterized one returns `C`.  In an
    // intersection the most specific type wins, so discard `mixed` entries
    // when at least one non-mixed result is present.
    if union_results.len() > 1 {
        let has_non_mixed = union_results.iter().any(|rt| !rt.type_string.is_mixed());
        if has_non_mixed {
            union_results.retain(|rt| !rt.type_string.is_mixed());
        }
    }

    union_results
}

/// Expand union generic receiver types into separate owner entries.
///
/// When a variable has type `C<A>|C<B>`, the resolution pipeline produces
/// a single `ResolvedType` with `type_string = Union(Generic("C",[A]), Generic("C",[B]))`
/// and one `class_info` (the base class `C`).  Calling a method on such
/// a union should resolve each branch independently: `->get()` on
/// `C<A>|C<B>` where `get()` returns `T` should yield `A|B`.
///
/// This function detects such union-of-generics patterns and expands them
/// into separate owner classes, each with the appropriate template
/// substitutions applied.
fn expand_union_generic_owners(
    owner_classes: Vec<Arc<ClassInfo>>,
    receiver_resolved: Vec<ResolvedType>,
    ctx: &VarResolutionCtx<'_>,
) -> (Vec<Arc<ClassInfo>>, Vec<ResolvedType>) {
    // Only expand when we have exactly one owner and the type_string
    // is a union with generic branches referencing the same base class.
    if owner_classes.len() != 1 || receiver_resolved.len() != 1 {
        return (owner_classes, receiver_resolved);
    }
    let rt = &receiver_resolved[0];
    let union_members = match &rt.type_string {
        PhpType::Union(members) => members,
        _ => return (owner_classes, receiver_resolved),
    };

    // Check that at least two branches are generic types of the same
    // base class, and the class has template parameters.
    let base_cls = &owner_classes[0];
    if base_cls.template_params.is_empty() {
        return (owner_classes, receiver_resolved);
    }

    let base_fqn = base_cls.fqn();
    let base_short = base_cls.name.as_str();
    let is_same_base = |name: &str| -> bool {
        name == base_short
            || name == base_fqn.as_str()
            || crate::util::short_name(name) == base_short
    };
    let generic_branches: Vec<&PhpType> = union_members
        .iter()
        .filter(|m| matches!(m, PhpType::Generic(name, _) if is_same_base(name)))
        .collect();
    if generic_branches.len() < 2 {
        return (owner_classes, receiver_resolved);
    }

    // Expand: for each generic branch, apply the type args to produce
    // a substituted ClassInfo.
    let mut expanded_owners: Vec<Arc<ClassInfo>> = Vec::new();
    let mut expanded_resolved: Vec<ResolvedType> = Vec::new();

    for member in union_members {
        match member {
            PhpType::Generic(name, args) if is_same_base(name) => {
                let arc = crate::virtual_members::resolve_class_fully_with_type_args(
                    base_cls,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                    args,
                );
                expanded_resolved.push(ResolvedType::from_both_arc(
                    member.clone(),
                    Arc::clone(&arc),
                ));
                expanded_owners.push(arc);
            }
            // Non-generic union members (e.g. scalars in `C<A>|int`)
            // are kept as type-string-only entries in receiver_resolved
            // but don't contribute an owner class.
            other => {
                expanded_resolved.push(ResolvedType::from_type_string(other.clone()));
            }
        }
    }

    (expanded_owners, expanded_resolved)
}

/// Find the receiver's type string that matches the given owner class name.
///
/// Scans `receiver_resolved` for a `ResolvedType` whose `class_info`
/// name matches `owner_name` and whose `type_string` is a `Generic`
/// (i.e. carries generic parameters like `Builder<Article>`).  Returns
/// the matching `PhpType` so that `replace_self_with_type` can preserve
/// those generic parameters when the method returns `static`/`self`/`$this`.
fn receiver_type_for_owner(
    receiver_resolved: &[ResolvedType],
    owner_name: &str,
) -> Option<PhpType> {
    for rt in receiver_resolved {
        let matches = rt
            .class_info
            .as_ref()
            .is_some_and(|ci| ci.name == owner_name)
            && matches!(rt.type_string, PhpType::Generic(_, _));
        if matches {
            return Some(rt.type_string.clone());
        }
    }
    None
}

/// Resolve a static method call: `ClassName::method()`, `self::method()`,
/// `static::method()`.
fn resolve_rhs_static_call(
    static_call: &StaticMethodCall<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class_name: &str = &ctx.current_class.name;

    let class_name = match static_call.class {
        Expression::Self_(_) => Some(current_class_name.to_string()),
        Expression::Static(_) => Some(current_class_name.to_string()),
        Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string()),
        Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
        // ── `$var::method()` where `$var` holds a class-string ──
        Expression::Variable(Variable::Direct(dv)) => {
            let var_name = bytes_to_str(dv.name).to_string();
            let targets =
                crate::completion::variable::class_string_resolution::resolve_class_string_targets(
                    &var_name,
                    ctx.current_class,
                    ctx.all_classes,
                    ctx.content,
                    ctx.cursor_offset,
                    ctx.class_loader,
                );
            // When there are multiple possible class targets (union class-string),
            // resolve the method return type through each and union the results.
            if targets.len() > 1 {
                if let ClassLikeMemberSelector::Identifier(ident) = &static_call.method {
                    let method_name_str = bytes_to_str(ident.value).to_string();
                    let mut union_types: Vec<PhpType> = Vec::new();
                    let mut union_classes: Vec<ResolvedType> = Vec::new();
                    for target in &targets {
                        let arg_texts = super::raw_type_inference::extract_arg_texts_from_ast(
                            &static_call.argument_list,
                            ctx.content,
                        );
                        let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();
                        let text_args = arg_texts.join(", ");
                        let rctx = ctx.as_resolution_ctx();
                        let template_subs = Backend::build_method_template_subs(
                            target,
                            &method_name_str,
                            &arg_refs,
                            &rctx,
                        );
                        let var_resolver = build_var_resolver_from_ctx(ctx);
                        let mr_ctx = MethodReturnCtx {
                            all_classes: ctx.all_classes,
                            class_loader: ctx.class_loader,
                            template_subs: &template_subs,
                            var_resolver: Some(&var_resolver),
                            cache: ctx.resolved_class_cache,
                            calling_class_name: Some(&ctx.current_class.name),
                            is_static: true,
                        };
                        // Get the method's return type string.
                        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                            target,
                            ctx.class_loader,
                            ctx.resolved_class_cache,
                        );
                        let method_ref = target
                            .get_method_ci(&method_name_str)
                            .or_else(|| merged.get_method_ci(&method_name_str));
                        if let Some(m) = method_ref {
                            if let Some(ref ret) = m.return_type {
                                let substituted = if !template_subs.is_empty() {
                                    ret.substitute(&template_subs)
                                } else {
                                    ret.clone()
                                };
                                let resolved = substituted.replace_self(&target.fqn());
                                union_types.push(resolved);
                            }
                        } else {
                            // Try to resolve through resolve_method_return_types_with_args
                            let results = Backend::resolve_method_return_types_with_args(
                                target,
                                &method_name_str,
                                &text_args,
                                &mr_ctx,
                            );
                            for r in results {
                                union_classes.push(ResolvedType::from_both_arc(
                                    PhpType::Named(r.name.to_string()),
                                    r,
                                ));
                            }
                        }
                    }
                    if !union_types.is_empty() || !union_classes.is_empty() {
                        // Build a unified type from all resolved return types.
                        let combined = if union_types.len() == 1 && union_classes.is_empty() {
                            union_types.remove(0)
                        } else if union_types.is_empty() && !union_classes.is_empty() {
                            return union_classes;
                        } else {
                            PhpType::Union(union_types)
                        };
                        let resolved_classes =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                &combined,
                                current_class_name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if !resolved_classes.is_empty() {
                            return ResolvedType::from_classes_with_hint(
                                resolved_classes,
                                combined,
                            );
                        }
                        return vec![ResolvedType::from_type_string(combined)];
                    }
                }
                // Fallback: use first target.
                return vec![];
            }
            if let Some(first) = targets.first() {
                Some(first.name.to_string())
            } else {
                // Fallback: resolve the variable's type and extract the
                // inner type from `class-string<T>`.  This handles
                // parameters typed as `@param class-string<Foo> $var`
                // where there is no `$var = Foo::class` assignment.
                let resolved = resolve_var_types(&var_name, ctx, ctx.cursor_offset);
                resolved
                    .iter()
                    .find_map(|rt| match &rt.type_string {
                        PhpType::ClassString(Some(inner)) => {
                            inner.base_name().map(|s| s.to_string())
                        }
                        PhpType::Nullable(inner) => match inner.as_ref() {
                            PhpType::ClassString(Some(cs_inner)) => {
                                cs_inner.base_name().map(|s| s.to_string())
                            }
                            _ => None,
                        },
                        PhpType::Union(members) => members.iter().find_map(|m| match m {
                            PhpType::ClassString(Some(inner)) => {
                                inner.base_name().map(|s| s.to_string())
                            }
                            PhpType::Nullable(inner) => match inner.as_ref() {
                                PhpType::ClassString(Some(cs_inner)) => {
                                    cs_inner.base_name().map(|s| s.to_string())
                                }
                                _ => None,
                            },
                            _ => None,
                        }),
                        _ => None,
                    })
                    .or_else(|| {
                        // Final fallback: `$var::method()` where `$var` is an
                        // object instance (not a class-string). In PHP you can
                        // call static methods on an instance reference.
                        resolved
                            .iter()
                            .find_map(|rt| rt.type_string.base_name().map(|s| s.to_string()))
                    })
            }
        }
        _ => None,
    };
    if let Some(cls_name) = class_name
        && let ClassLikeMemberSelector::Identifier(ident) = &static_call.method
    {
        let method_name = bytes_to_str(ident.value).to_string();
        let owner = (ctx.class_loader)(&cls_name)
            .map(Arc::unwrap_or_clone)
            .or_else(|| {
                ctx.all_classes
                    .iter()
                    .find(|c| c.name == cls_name)
                    .map(|c| ClassInfo::clone(c))
            });
        if let Some(ref owner) = owner {
            let arg_texts = super::raw_type_inference::extract_arg_texts_from_ast(
                &static_call.argument_list,
                ctx.content,
            );
            let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();
            let text_args = arg_texts.join(", ");
            let rctx = ctx.as_resolution_ctx();
            let template_subs =
                Backend::build_method_template_subs(owner, &method_name, &arg_refs, &rctx);
            let var_resolver = build_var_resolver_from_ctx(ctx);
            let mr_ctx = MethodReturnCtx {
                all_classes: ctx.all_classes,
                class_loader: ctx.class_loader,
                template_subs: &template_subs,
                var_resolver: Some(&var_resolver),
                cache: ctx.resolved_class_cache,
                calling_class_name: Some(&ctx.current_class.name),
                is_static: true,
            };
            // Recover the effective return type string from the method.
            // Look up the method on the (possibly merged) owner and apply
            // the same template substitution that
            // `resolve_method_return_types_with_args` used internally,
            // then replace `static`/`self`/`$this` with the owner class
            // name so that e.g. `static[]` becomes `Country[]`.
            // Try the owner directly first — it may already be fully resolved
            // with generic substitutions applied.  The cache is keyed by bare
            // FQN and returns the un-substituted base class, so prefer the
            // owner's own method to preserve template substitutions.
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                owner,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            let method_ref = owner
                .get_method_ci(&method_name)
                .or_else(|| merged.get_method_ci(&method_name));
            let ret_type_string = method_ref.and_then(|m| m.return_type.as_ref()).map(|ret| {
                let substituted = if !template_subs.is_empty() {
                    ret.substitute(&template_subs)
                } else {
                    ret.clone()
                };
                substituted.replace_self(&owner.fqn())
            });

            let results = Backend::resolve_method_return_types_with_args(
                owner,
                &method_name,
                &text_args,
                &mr_ctx,
            );
            if !results.is_empty() {
                let classes: Vec<Arc<ClassInfo>> = results;
                // When the method has a conditional return type, the
                // resolved classes came from evaluating the conditional.
                // Using the method's declared return type (typically
                // `mixed`) as the type hint would be misleading.
                let has_conditional = merged
                    .get_method_ci(&method_name)
                    .is_some_and(|m| m.conditional_return.is_some());
                let effective_hint = if has_conditional {
                    None
                } else {
                    ret_type_string
                };
                return match effective_hint {
                    Some(hint) => ResolvedType::from_classes_with_hint(classes, hint),
                    None => ResolvedType::from_classes(classes),
                };
            }

            // The method has a return type string but `type_hint_to_classes_typed`
            // found no matching class (e.g. `list<Widget>`, `int`,
            // `array{name: string}`).  Return a type-string-only entry so
            // that consumers reading `.type_string` (hover, raw-type
            // pipeline, null-coalesce stripping) still get the information.
            if let Some(ref hint) = ret_type_string {
                if *hint == PhpType::void() {
                    return vec![ResolvedType::from_type_string(PhpType::null())];
                }
                return vec![resolved_type_with_lookup(
                    hint.clone(),
                    current_class_name,
                    ctx.all_classes,
                    ctx.class_loader,
                )];
            }

            // Body return type inference fallback (static calls).
            if method_ref
                .is_some_and(|m| m.return_type.is_none() && m.name_offset != 0 && !m.is_virtual)
                && let Some(inferred) =
                    crate::completion::call_resolution::try_infer_body_return_type(
                        &owner.fqn(),
                        method_ref.unwrap(),
                    )
                && !inferred.is_void()
                && !inferred.is_mixed()
            {
                return vec![resolved_type_with_lookup(
                    inferred,
                    current_class_name,
                    ctx.all_classes,
                    ctx.class_loader,
                )];
            }
        }
    }
    vec![]
}

/// Resolve property access: `$this->prop`, `$obj->prop`, `$obj?->prop`.
fn resolve_rhs_property_access(
    access: &Access<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class_name: &str = &ctx.current_class.name;
    let all_classes = ctx.all_classes;
    let class_loader = ctx.class_loader;

    /// Resolve a property's type to `Vec<ResolvedType>`, preserving the
    /// property's type hint string in each result.
    ///
    /// When the property type is a scalar (e.g. `string`, `int`) and
    /// `type_hint_to_classes_typed` returns no `ClassInfo`, a type-string-only
    /// `ResolvedType` is produced so that the type information is not lost.
    fn resolve_property_with_hint(
        prop_name: &str,
        owner: &ClassInfo,
        current_class_name: &str,
        all_classes: &[Arc<ClassInfo>],
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Vec<ResolvedType> {
        // Get the type hint before resolving to ClassInfo.
        let type_hint =
            crate::inheritance::resolve_property_type_hint(owner, prop_name, class_loader);
        let resolved = crate::completion::type_resolution::resolve_property_types(
            prop_name,
            owner,
            all_classes,
            class_loader,
        );
        if resolved.is_empty() {
            // The property has a type hint but `type_hint_to_classes_typed`
            // found no matching class (e.g. `list<Widget>`, `int`,
            // `array{name: string}`).  Return a type-string-only
            // entry when the type is informative (carries generics,
            // shapes, or names a non-scalar class).
            return match type_hint {
                Some(hint) => {
                    vec![resolved_type_with_lookup(
                        hint,
                        current_class_name,
                        all_classes,
                        class_loader,
                    )]
                }
                _ => vec![],
            };
        }
        match type_hint {
            Some(hint) => ResolvedType::from_classes_with_hint(resolved, hint),
            None => ResolvedType::from_classes(resolved),
        }
    }

    // ── Class constant / enum case access: `Foo::BAR` ──
    // When the RHS is a class constant access, resolve the class and
    // check whether the constant is an enum case (→ type is the enum
    // itself) or a typed constant (→ use its type_hint).
    if let Access::ClassConstant(cca) = access {
        let class_name = match cca.class {
            Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
            Expression::Self_(_) => Some(current_class_name.to_string()),
            Expression::Static(_) => Some(current_class_name.to_string()),
            _ => None,
        };
        if let Some(class_name) = class_name {
            let resolved_name = class_name.strip_prefix('\\').unwrap_or(&class_name);
            let resolved_typed = PhpType::Named(resolved_name.to_string());
            let target_classes = crate::completion::type_resolution::type_hint_to_classes_typed(
                &resolved_typed,
                current_class_name,
                all_classes,
                class_loader,
            );

            let const_name = match &cca.constant {
                ClassLikeConstantSelector::Identifier(ident) => {
                    Some(bytes_to_str(ident.value).to_string())
                }
                _ => None,
            };

            if let Some(const_name) = const_name {
                // Search local classes first.  If the constant is not
                // found, resolve via full inheritance merging so that
                // constants from parent classes are visible (e.g.
                // `self::PARENT_CONST` in a subclass).
                let merged_classes: Vec<Arc<ClassInfo>>;
                let all_candidates: &[Arc<ClassInfo>] = if target_classes
                    .iter()
                    .any(|cls| cls.constants.iter().any(|c| c.name == const_name))
                {
                    &target_classes
                } else {
                    merged_classes = target_classes
                        .iter()
                        .map(|cls| {
                            crate::virtual_members::resolve_class_fully_maybe_cached(
                                cls,
                                class_loader,
                                ctx.resolved_class_cache,
                            )
                        })
                        .collect();
                    &merged_classes
                };

                for cls in all_candidates {
                    // Check if the constant is an enum case — the
                    // result type is the enum class itself.
                    if let Some(c) = cls.constants.iter().find(|c| c.name == const_name) {
                        if c.is_enum_case {
                            return ResolvedType::from_classes(target_classes);
                        }
                        // Typed class constant — resolve via type_hint.
                        if let Some(ref th) = c.type_hint {
                            let resolved =
                                crate::completion::type_resolution::type_hint_to_classes_typed(
                                    th,
                                    current_class_name,
                                    all_classes,
                                    class_loader,
                                );
                            if !resolved.is_empty() {
                                return ResolvedType::from_classes_with_hint(resolved, th.clone());
                            }
                        }
                        // No type_hint — infer from the initializer value.
                        if let Some(ref val) = c.value
                            && let Some(ts) = infer_type_from_constant_value(val)
                        {
                            let resolved =
                                crate::completion::type_resolution::type_hint_to_classes_typed(
                                    &ts,
                                    current_class_name,
                                    all_classes,
                                    class_loader,
                                );
                            if !resolved.is_empty() {
                                return ResolvedType::from_classes_with_hint(resolved, ts);
                            }
                            return vec![ResolvedType::from_type_string(ts)];
                        }
                    }
                }
            }
        }
        return vec![];
    }

    // ── Static property access: `self::$prop`, `static::$prop`, `Foo::$prop` ──
    if let Access::StaticProperty(spa) = access {
        let class_name = match spa.class {
            Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
            Expression::Self_(_) => Some(current_class_name.to_string()),
            Expression::Static(_) => Some(current_class_name.to_string()),
            Expression::Parent(_) => {
                // Resolve parent class name from the current class.
                all_classes
                    .iter()
                    .find(|c| c.name == current_class_name)
                    .and_then(|c| c.parent_class.map(|a| a.to_string()))
            }
            _ => None,
        };
        let prop_name = match &spa.property {
            Variable::Direct(dv) => {
                let raw = bytes_to_str(dv.name).to_string();
                Some(raw.strip_prefix('$').unwrap_or(&raw).to_string())
            }
            _ => None,
        };
        if let Some(class_name) = class_name
            && let Some(prop_name) = prop_name
        {
            let resolved_name = class_name.strip_prefix('\\').unwrap_or(&class_name);
            let resolved_typed = PhpType::Named(resolved_name.to_string());
            let target_classes = crate::completion::type_resolution::type_hint_to_classes_typed(
                &resolved_typed,
                current_class_name,
                all_classes,
                class_loader,
            );
            for cls in &target_classes {
                let resolved = resolve_property_with_hint(
                    &prop_name,
                    cls,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return resolved;
                }
            }
        }
        return vec![];
    }

    let (object_expr, prop_selector) = match access {
        Access::Property(pa) => (Some(pa.object), Some(&pa.property)),
        Access::NullSafeProperty(pa) => (Some(pa.object), Some(&pa.property)),
        _ => (None, None),
    };
    if let Some(obj) = object_expr
        && let Some(sel) = prop_selector
    {
        let prop_name = match sel {
            ClassLikeMemberSelector::Identifier(ident) => {
                Some(bytes_to_str(ident.value).to_string())
            }
            _ => None,
        };
        if let Some(prop_name) = prop_name {
            // ── $this->prop assignment narrowing ────────────────
            // When the object is `$this`, check if there is an
            // assignment to `$this->propName` before the cursor in
            // the current method.  If so, use the assigned value's
            // type — but ONLY when it is narrower (a subtype of)
            // the declared property type.  This handles patterns
            // like:
            //   $this->mock = $this->createMock(Foo::class);
            //   new Bar($this->mock); // mock is MockObject&Foo
            //
            // We reject widening assignments (e.g. narrowed type is
            // `object` but declared type is `Foo`) to avoid losing
            // declared type information.
            if let Expression::Variable(Variable::Direct(dv)) = obj
                && dv.name == b"$this"
            {
                let narrowed = try_resolve_this_property_from_assignment(&prop_name, ctx);
                if !narrowed.is_empty() {
                    // Look up the declared property type so we can
                    // verify the narrowed type is actually narrower.
                    let current_class_arc =
                        all_classes.iter().find(|c| c.name == current_class_name);
                    let declared_type = current_class_arc.and_then(|cls| {
                        crate::inheritance::resolve_property_type_hint(
                            cls,
                            &prop_name,
                            class_loader,
                        )
                    });
                    if let Some(ref declared) = declared_type {
                        // Only use the narrowed type when every
                        // resolved type is a subtype of the declared
                        // type.  Use structural subtyping first, then
                        // fall back to nominal class hierarchy.
                        let all_narrow = narrowed.iter().all(|rt| {
                            let ts = &rt.type_string;
                            // Structural check covers scalars, unions,
                            // intersections, nullable, generic, etc.
                            if ts.is_subtype_of(declared) {
                                return true;
                            }
                            // Nominal check: if both are class-like,
                            // walk the class hierarchy.
                            if let Some(narrowed_base) = ts.base_name()
                                && let Some(cls) = (class_loader)(narrowed_base)
                                && let Some(declared_base) = declared.base_name()
                            {
                                return crate::util::is_subtype_of(
                                    &cls,
                                    declared_base,
                                    class_loader,
                                );
                            }
                            // Intersection types: each member must be
                            // a subtype.  If any member satisfies the
                            // declared type, the intersection does too.
                            if let crate::php_type::PhpType::Intersection(members) = ts {
                                return members.iter().any(|m| {
                                    if m.is_subtype_of(declared) {
                                        return true;
                                    }
                                    if let Some(base) = m.base_name()
                                        && let Some(cls) = (class_loader)(base)
                                        && let Some(declared_base) = declared.base_name()
                                    {
                                        return crate::util::is_subtype_of(
                                            &cls,
                                            declared_base,
                                            class_loader,
                                        );
                                    }
                                    false
                                });
                            }
                            false
                        });
                        if all_narrow {
                            return narrowed;
                        }
                        // Narrowed type is wider than declared — fall
                        // through to normal property type resolution.
                    } else {
                        // No declared type (untyped property) — the
                        // narrowed type is the best we have.
                        return narrowed;
                    }
                }
            }

            let owner_classes: Vec<Arc<ClassInfo>> =
                if let Expression::Variable(Variable::Direct(dv)) = obj
                    && dv.name == b"$this"
                {
                    all_classes
                        .iter()
                        .find(|c| c.name == current_class_name)
                        .map(Arc::clone)
                        .into_iter()
                        .collect()
                } else if let Expression::Variable(Variable::Direct(dv)) = obj {
                    let var = bytes_to_str(dv.name).to_string();
                    // Check match-arm narrowing override first.
                    if let Some(overridden) = ctx.match_arm_narrowing.get(&var).cloned() {
                        ResolvedType::into_arced_classes(overridden)
                    } else {
                        // When a scope_var_resolver is available (forward-walker
                        // RHS resolution), try it first so we read from the
                        // in-progress ScopeState instead of the diagnostic
                        // scope cache or backward scanner.
                        let from_scope = if let Some(resolver) = ctx.scope_var_resolver {
                            let prefixed = if var.starts_with('$') {
                                var.clone()
                            } else {
                                format!("${}", var)
                            };
                            resolver(&prefixed)
                        } else {
                            vec![]
                        };
                        let classes = ResolvedType::into_arced_classes(from_scope);
                        if !classes.is_empty() {
                            classes
                        } else {
                            ResolvedType::into_arced_classes(
                                crate::completion::resolver::resolve_target_classes(
                                    &var,
                                    crate::types::AccessKind::Arrow,
                                    &ctx.as_resolution_ctx(),
                                ),
                            )
                        }
                    }
                } else {
                    // Handle non-variable object expressions like
                    // `(new Canvas())->easel`, `getService()->prop`,
                    // or `SomeClass::make()->prop` by recursively
                    // resolving the expression type.
                    ResolvedType::into_arced_classes(resolve_rhs_expression(obj, ctx))
                };

            let mut all_resolved: Vec<ResolvedType> = Vec::new();
            for owner in &owner_classes {
                let resolved = resolve_property_with_hint(
                    &prop_name,
                    owner,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                for rt in resolved {
                    if !all_resolved
                        .iter()
                        .any(|existing| existing.type_string == rt.type_string)
                    {
                        all_resolved.push(rt);
                    }
                }
            }
            if !all_resolved.is_empty() {
                return all_resolved;
            }
        }
    }
    vec![]
}

/// Try to resolve `$this->propName` from a prior assignment in the
/// current method body.
///
/// Walks the parsed AST to find the enclosing method, then scans its
/// statements for the last unconditional `$this->propName = <expr>`
/// before the cursor.  If found, resolves `<expr>` and returns the
/// result.  Returns an empty vec when no assignment is found (caller
/// should fall back to the declared property type).
fn try_resolve_this_property_from_assignment(
    prop_name: &str,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    with_parsed_program(
        ctx.content,
        "try_resolve_this_property_from_assignment",
        |program, _content| {
            // Find the RHS of the last `$this->propName = <expr>` in the
            // enclosing method body, before the cursor.
            let rhs_expr = find_this_property_assignment_in_toplevel(
                program.statements.iter(),
                prop_name,
                ctx.cursor_offset,
            );
            let Some(rhs_expr) = rhs_expr else {
                return Vec::new();
            };

            // Resolve the RHS expression with cursor set to the
            // assignment position so recursive resolution only sees
            // prior assignments.
            let rhs_ctx = ctx.with_cursor_offset(rhs_expr.span().start.offset);
            resolve_rhs_expression(rhs_expr, &rhs_ctx)
        },
    )
}

/// Search class-like members for a concrete method body containing `cursor_offset`,
/// then scan that body for the last `$this->propName = <expr>` assignment.
fn find_property_assignment_in_members<'b>(
    members: impl Iterator<Item = &'b ClassLikeMember<'b>>,
    prop_name: &str,
    cursor_offset: u32,
) -> Option<&'b Expression<'b>> {
    let block = crate::util::find_enclosing_method_block_in_members(members, cursor_offset)?;
    find_last_this_property_assignment(block.statements.iter(), prop_name, cursor_offset)
}

/// Walk top-level statements to find the enclosing method, then scan
/// its body for the last `$this->propName = <expr>` before the cursor.
fn find_this_property_assignment_in_toplevel<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    prop_name: &str,
    cursor_offset: u32,
) -> Option<&'b Expression<'b>> {
    for stmt in statements {
        let stmt_span = stmt.span();
        if cursor_offset < stmt_span.start.offset || cursor_offset > stmt_span.end.offset {
            continue;
        }
        match stmt {
            Statement::Class(class) => {
                if let Some(found) = find_property_assignment_in_members(
                    class.members.iter(),
                    prop_name,
                    cursor_offset,
                ) {
                    return Some(found);
                }
            }
            Statement::Trait(trait_def) => {
                if let Some(found) = find_property_assignment_in_members(
                    trait_def.members.iter(),
                    prop_name,
                    cursor_offset,
                ) {
                    return Some(found);
                }
            }
            Statement::Enum(enum_def) => {
                if let Some(found) = find_property_assignment_in_members(
                    enum_def.members.iter(),
                    prop_name,
                    cursor_offset,
                ) {
                    return Some(found);
                }
            }
            Statement::Namespace(ns) => {
                if let Some(found) = find_this_property_assignment_in_toplevel(
                    ns.statements().iter(),
                    prop_name,
                    cursor_offset,
                ) {
                    return Some(found);
                }
            }
            Statement::If(if_stmt) => {
                // Walk the then-branch, every elseif branch, and the else
                // branch: an assignment to `$this->prop` in any of them
                // (before the cursor) is a valid narrowing source.
                let search = |inner: &'b Statement<'b>| {
                    find_this_property_assignment_in_toplevel(
                        std::iter::once(inner),
                        prop_name,
                        cursor_offset,
                    )
                };
                match &if_stmt.body {
                    IfBody::Statement(body) => {
                        if let Some(found) = search(body.statement) {
                            return Some(found);
                        }
                        for elseif in body.else_if_clauses.iter() {
                            if let Some(found) = search(elseif.statement) {
                                return Some(found);
                            }
                        }
                        if let Some(ref else_clause) = body.else_clause
                            && let Some(found) = search(else_clause.statement)
                        {
                            return Some(found);
                        }
                    }
                    IfBody::ColonDelimited(body) => {
                        if let Some(found) = find_this_property_assignment_in_toplevel(
                            body.statements.iter(),
                            prop_name,
                            cursor_offset,
                        ) {
                            return Some(found);
                        }
                        for elseif in body.else_if_clauses.iter() {
                            if let Some(found) = find_this_property_assignment_in_toplevel(
                                elseif.statements.iter(),
                                prop_name,
                                cursor_offset,
                            ) {
                                return Some(found);
                            }
                        }
                        if let Some(ref else_clause) = body.else_clause
                            && let Some(found) = find_this_property_assignment_in_toplevel(
                                else_clause.statements.iter(),
                                prop_name,
                                cursor_offset,
                            )
                        {
                            return Some(found);
                        }
                    }
                }
            }
            Statement::Block(block) => {
                if let Some(found) = find_this_property_assignment_in_toplevel(
                    block.statements.iter(),
                    prop_name,
                    cursor_offset,
                ) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// Scan `statements` for the last unconditional `$this->propName = <expr>`
/// whose offset is before `cursor_offset`.  Returns the RHS expression.
fn find_last_this_property_assignment<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    prop_name: &str,
    cursor_offset: u32,
) -> Option<&'b Expression<'b>> {
    let mut last_rhs: Option<&'b Expression<'b>> = None;

    for stmt in statements {
        if stmt.span().start.offset >= cursor_offset {
            break;
        }
        if let Statement::Expression(expr_stmt) = stmt
            && let Some(rhs) = extract_this_property_assignment_rhs(expr_stmt.expression, prop_name)
        {
            last_rhs = Some(rhs);
        }
    }

    last_rhs
}

/// If `expr` is `$this->propName = <rhs>`, return `Some(rhs)`.
fn extract_this_property_assignment_rhs<'b>(
    expr: &'b Expression<'b>,
    prop_name: &str,
) -> Option<&'b Expression<'b>> {
    let Expression::Assignment(assignment) = expr else {
        return None;
    };
    if !assignment.operator.is_assign() {
        return None;
    }
    let Expression::Access(Access::Property(pa)) = assignment.lhs else {
        return None;
    };
    let Expression::Variable(Variable::Direct(dv)) = pa.object else {
        return None;
    };
    if dv.name != b"$this" {
        return None;
    }
    let ClassLikeMemberSelector::Identifier(ident) = &pa.property else {
        return None;
    };
    if bytes_to_str(ident.value) != prop_name {
        return None;
    }
    Some(assignment.rhs)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_direct_param() {
        let ty = PhpType::parse("T");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::Direct));
    }

    #[test]
    fn classify_array_element() {
        let ty = PhpType::parse("T[]");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::ArrayElement));
    }

    #[test]
    fn classify_generic_wrapper() {
        let ty = PhpType::parse("Collection<T>");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::GenericWrapper(_, 0)));
    }

    #[test]
    fn classify_callable_return_type() {
        let ty =
            PhpType::parse("callable(TReduceInitial|TReduceReturnType, TValue): TReduceReturnType");
        let mode = classify_template_binding("TReduceReturnType", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableReturnType));
    }

    #[test]
    fn classify_closure_return_type() {
        let ty = PhpType::parse("Closure(int, string): T");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableReturnType));
    }

    #[test]
    fn classify_callable_param_type() {
        // Template appears only in params, not in return type — should be CallableParamType.
        let ty = PhpType::parse("callable(T): void");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableParamType(0)));
    }

    #[test]
    fn classify_callable_param_type_second_position() {
        let ty = PhpType::parse("Closure(int, T): void");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableParamType(1)));
    }

    #[test]
    fn classify_callable_return_type_preferred_over_param() {
        // When T appears in both params and return type, return type wins.
        let ty = PhpType::parse("callable(T): T");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableReturnType));
    }

    #[test]
    fn classify_nullable_union_callable() {
        // Template in callable return type within a union.
        let ty = PhpType::parse("callable(int): T|null");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableReturnType));
    }

    #[test]
    fn classify_none_hint() {
        let mode = classify_template_binding("T", None);
        assert!(matches!(mode, TemplateBindingMode::Direct));
    }

    #[test]
    fn type_contains_name_simple() {
        let ty = PhpType::Named("Foo".to_owned());
        assert!(type_contains_name(&ty, "Foo"));
        assert!(!type_contains_name(&ty, "Bar"));
    }

    #[test]
    fn type_contains_name_nested_callable() {
        let ty = PhpType::parse("callable(int): Decimal");
        assert!(type_contains_name(&ty, "Decimal"));
        assert!(type_contains_name(&ty, "int"));
        assert!(!type_contains_name(&ty, "string"));
    }

    #[test]
    fn type_contains_name_union() {
        let ty = PhpType::parse("Foo|Bar|null");
        assert!(type_contains_name(&ty, "Foo"));
        assert!(type_contains_name(&ty, "Bar"));
        assert!(type_contains_name(&ty, "null"));
        assert!(!type_contains_name(&ty, "Baz"));
    }
}
