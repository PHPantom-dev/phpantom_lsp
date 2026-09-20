/// Array literal inference and array function helpers.
///
/// These are utility helpers that support the forward-walking variable
/// resolver in [`super::forward_walk`] and the foreach/destructuring
/// resolution module.
use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::array_func_rules::{ArrayFuncArgs, array_func_element_type, array_func_raw_type};

use crate::atom::{atom, bytes_to_str, literal_bytes_to_str};
use crate::docblock;
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;

use crate::type_engine::resolver::VarResolutionCtx;
use crate::types::ResolvedType;

/// Infer the raw PHPStan-style type for an array literal (`[…]` or
/// `array(…)`) from its keys and value expressions.
pub(in crate::type_engine) fn infer_array_literal_raw_type<'b>(
    elements: impl Iterator<Item = &'b ArrayElement<'b>>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    // Maximum number of positional entries to record as a tuple-style
    // shape. Beyond this the array is almost certainly a homogeneous
    // collection rather than a fixed-arity tuple, so it is widened to
    // `list<T>` to avoid unbounded shape growth.
    const MAX_POSITIONAL_SHAPE_LEN: usize = 32;

    // Maximum number of distinct alternatives to keep in the `list<T>`
    // element union before falling back to the base scalar types. A
    // literal array that names more distinct values than this is a data
    // table rather than a set of alternatives worth reasoning about, and
    // the union's pairwise absorption is quadratic in its member count.
    const MAX_ELEMENT_ALTERNATIVES: usize = 32;

    let mut types: Vec<PhpType> = Vec::new();
    let mut key_types: Vec<PhpType> = Vec::new();
    let mut has_string_keys = false;
    let mut non_constant_key = false;
    let mut saw_spread = false;
    let mut saw_element = false;
    let mut shape_entries: Vec<crate::php_type::ShapeEntry> = Vec::new();

    for elem in elements {
        saw_element = true;
        match elem {
            ArrayElement::KeyValue(kv) => {
                has_string_keys = true;
                let value_type = infer_element_type(kv.value, ctx).unwrap_or_else(PhpType::mixed);
                match extract_array_key_text(kv.key) {
                    Some(key_text) => {
                        push_unique(&mut key_types, constant_key_type(kv.key));
                        shape_entries.push(crate::php_type::ShapeEntry {
                            key: Some(key_text),
                            value_type: value_type.clone(),
                            optional: false,
                        });
                    }
                    // A key that is not a literal has no name to record, and
                    // naming the entry after the key's *type* would invent a
                    // shape field nobody wrote. The whole literal falls back
                    // to `array<K, V>` instead.
                    None => {
                        non_constant_key = true;
                        push_unique(&mut key_types, dynamic_key_type(kv.key, ctx));
                    }
                }
                push_unique(&mut types, value_type);
            }
            ArrayElement::Value(v) => {
                let resolved = infer_element_type(v.value, ctx);
                // A positional shape must keep one entry per element to
                // preserve arity, so an unresolvable element becomes
                // `mixed`. The `list<T>` fallback keeps its original
                // behaviour of ignoring unresolvable elements. Recorded
                // in `shape_entries` (key: None) at the position it was
                // written so PHP's sequential auto-index numbering,
                // which `shape_keys` and `shape_value_type` both assume,
                // stays intact even when later entries have string keys.
                shape_entries.push(crate::php_type::ShapeEntry {
                    key: None,
                    value_type: resolved.clone().unwrap_or_else(PhpType::mixed),
                    optional: false,
                });
                push_unique(&mut key_types, PhpType::int());
                if let Some(t) = resolved
                    && !types.contains(&t)
                {
                    types.push(t);
                }
            }
            ArrayElement::Variadic(v) => {
                // Spread: `...$other` — try to resolve iterable element type.
                // A spread copies values the source already knows, so its
                // element type carries over as written, the same as a value
                // element beside it.
                saw_spread = true;
                let raw = super::foreach_resolution::resolve_expression_type(v.value, ctx);
                push_unique(
                    &mut key_types,
                    raw.as_ref()
                        .and_then(PhpType::iterable_key_type)
                        .unwrap_or_else(array_key_type),
                );
                if let Some(raw) = raw
                    && let Some(elem) = raw.iterable_element_type()
                    && !types.contains(&elem)
                {
                    types.push(elem);
                }
            }
            ArrayElement::Missing(_) => {}
        }
    }

    // `[]` is exactly the empty array, which a bare `array` (an array of
    // unknown contents) does not say. Recording it as `array{}` lets a
    // later write's result absorb it when branches rejoin, instead of
    // leaving `array|array<int, Foo>` behind for every array built up
    // conditionally from an empty start.
    if !saw_element {
        return Some(PhpType::array_shape(Vec::new()));
    }

    // At least one key is only known at runtime, so the literal has no
    // fixed set of fields: describe it by its key and value types instead.
    if non_constant_key {
        let key_type = join_key_types(key_types);
        let value_type =
            join_alternatives(types, MAX_ELEMENT_ALTERNATIVES).unwrap_or_else(PhpType::mixed);
        return Some(PhpType::generic_array(key_type, value_type));
    }

    if has_string_keys && !shape_entries.is_empty() {
        return Some(PhpType::array_shape(shape_entries));
    }

    if types.is_empty() {
        return None;
    }

    // A value-only literal with a fixed set of elements is recorded as a
    // positional (tuple-style) array shape so that integer-literal indexing
    // (`$pair[1]`) and list destructuring select the element at that
    // position, and out-of-bounds indices are known to be absent. A spread
    // element or an over-long literal makes the arity indeterminate, so
    // those widen to `list<T>` instead.
    if !saw_spread && !shape_entries.is_empty() && shape_entries.len() <= MAX_POSITIONAL_SHAPE_LEN {
        return Some(PhpType::array_shape(shape_entries));
    }

    // Preserved literals need absorbing against their siblings, so that a
    // list written as `[$stringVar, 'yes', 'no']` is `list<string>` rather
    // than `list<string|'yes'|'no'>`. A list that names more distinct
    // values than the cap is a data table, not a set of alternatives worth
    // carrying, and the join's pairwise absorption is quadratic in the
    // member count, so those widen to their base types first.
    //
    // Element sets with no literal in them keep the plain union: the join
    // also rewrites `?T` into `T|null`, and that spelling change alone
    // moves types that were never imprecise to begin with.
    let elem_type =
        join_alternatives(types, MAX_ELEMENT_ALTERNATIVES).unwrap_or_else(PhpType::mixed);
    Some(PhpType::list(elem_type))
}

/// Collapse a set of alternatives into one type, or `None` when empty.
fn join_alternatives(mut types: Vec<PhpType>, max_alternatives: usize) -> Option<PhpType> {
    if types.is_empty() {
        return None;
    }
    if types.iter().any(|t| t.as_literal().is_some()) {
        if types.len() > max_alternatives {
            types = types.iter().map(PhpType::widen_scalar_literals).collect();
        }
        return Some(PhpType::join_runtime_value_types(types));
    }
    if types.len() == 1 {
        return types.into_iter().next();
    }
    Some(PhpType::union(types))
}

/// Append `ty` unless an equal member is already recorded.
fn push_unique(types: &mut Vec<PhpType>, ty: PhpType) {
    if !types.contains(&ty) {
        types.push(ty);
    }
}

/// The `array-key` pseudo-type, used where a key is neither known to be
/// `int` nor `string`.
fn array_key_type() -> PhpType {
    PhpType::named(atom("array-key"))
}

/// Collapse the key types an array literal's entries contribute.
///
/// `array-key` covers every legal key, so one unplaceable key makes the
/// whole union `array-key` rather than leaving the redundant
/// `array-key|string` a plain union would produce.
fn join_key_types(key_types: Vec<PhpType>) -> PhpType {
    if key_types.iter().any(PhpType::is_array_key) {
        return array_key_type();
    }
    // Unlike a value union, the alternatives here are worth keeping as
    // written: a `Foo::class` key is a `class-string<Foo>`, and widening it
    // to `string` costs a `array<class-string, …>` parameter its match.
    match key_types.len() {
        0 => array_key_type(),
        1 => key_types.into_iter().next().unwrap(),
        _ => PhpType::union(key_types),
    }
}

/// The type an array takes on for a key PHP evaluates at runtime.
///
/// PHP coerces every array key to `int` or `string`, so anything that does
/// not resolve to one of those (`mixed`, a union spanning both, a value the
/// walker could not place) is reported as `array-key`.
fn dynamic_key_type<'b>(key: &'b Expression<'b>, ctx: &VarResolutionCtx<'_>) -> PhpType {
    match infer_element_type(key, ctx) {
        Some(resolved) if resolved.is_int_subtype() || resolved.is_string_subtype() => resolved,
        _ => array_key_type(),
    }
}

/// The key type an array literal takes on from a constant key expression.
fn constant_key_type<'b>(key: &'b Expression<'b>) -> PhpType {
    match key {
        Expression::Literal(Literal::Integer(_) | Literal::True(_) | Literal::False(_)) => {
            PhpType::int()
        }
        _ => PhpType::string(),
    }
}

/// The name a constant array key contributes to an array shape, or `None`
/// when the key is only known at runtime.
///
/// PHP coerces a non-string, non-int key before using it, so the booleans
/// and `null` land on the `1`, `0` and `''` keys they index at runtime
/// rather than on a key named after the type they were written as.
fn extract_array_key_text<'b>(key: &'b Expression<'b>) -> Option<String> {
    match key {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the unquoted content; fall back to unquoting `raw`,
            // which is also where a value that is not UTF-8 (`"\x8b"`) lands.
            Some(
                s.value
                    .and_then(literal_bytes_to_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        crate::text_scan::unquote_php_string(bytes_to_str(s.raw))
                            .unwrap_or(bytes_to_str(s.raw))
                            .to_string()
                    }),
            )
        }
        Expression::Literal(Literal::Integer(i)) => Some(bytes_to_str(i.raw).to_string()),
        Expression::Literal(Literal::True(_)) => Some("1".to_string()),
        Expression::Literal(Literal::False(_)) => Some("0".to_string()),
        Expression::Literal(Literal::Null(_)) => Some(String::new()),
        _ => None,
    }
}

/// Infer the type of a single array element value expression.
///
/// A scalar literal keeps its exact value here. The array's contents are
/// fully known at the point the literal is written, so `[1, 1.5, '123']`
/// records `1|1.5|'123'` and a read off it can still be proven `numeric`.
/// Precision is given up where the array is *mutated* instead: a later
/// push or keyed write widens through [`merge_push_type`] and friends,
/// because a value arriving after construction says the array is being
/// built up rather than written out.
///
/// [`merge_push_type`]: super::resolution::merge_push_type
fn infer_element_type<'b>(
    value: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    match value {
        // ── Nested array literals ──
        Expression::Array(arr) => infer_array_literal_raw_type(arr.elements.iter(), ctx)
            .or_else(|| Some(PhpType::array())),
        Expression::LegacyArray(arr) => infer_array_literal_raw_type(arr.elements.iter(), ctx)
            .or_else(|| Some(PhpType::array())),
        // ── Object instantiation ──
        Expression::Instantiation(inst) => match inst.class {
            Expression::Identifier(ident) => {
                let name = bytes_to_str(ident.value()).to_string();
                let fqn = crate::util::resolve_source_class_name(
                    &name,
                    ctx.current_class.file_namespace.as_deref(),
                    ctx.class_loader,
                );
                Some(PhpType::named(atom(&fqn)))
            }
            Expression::Self_(_) => Some(PhpType::named(atom(ctx.current_class.name.as_ref()))),
            Expression::Static(_) => Some(PhpType::named(atom(ctx.current_class.name.as_ref()))),
            _ => None,
        },
        Expression::Call(_) => {
            // Resolve call return type via the unified pipeline.
            super::foreach_resolution::resolve_expression_type(value, ctx)
        }
        Expression::Variable(Variable::Direct(dv)) => {
            let var_text = bytes_to_str(dv.name).to_string();
            let offset = value.span().start.offset as usize;
            // A `[$x]` written inside a ternary branch or `match` arm whose
            // condition proved something about `$x` builds an array of what
            // was proven, not of the type `$x` had before the test.  The
            // scope entry below still describes the statement as a whole.
            if let Some(narrowed) = ctx.arm_narrowed(&var_text) {
                return Some(crate::types::ResolvedType::types_joined(narrowed));
            }
            // When a scope variable resolver is available (i.e. we are
            // inside the forward walker), read the variable's type
            // directly from the in-progress ScopeState instead of
            // calling the full resolution pipeline which would trigger
            // a recursive method-body walk.
            //
            // The scope comes before the docblock because it is the only
            // one of the two that knows where the literal is written: a
            // `[$x]` inside `if (!is_array($x))` builds an array of the
            // narrowed `$x`, while the `@param array<T>|T $x` the
            // docblock states describes `$x` at the top of the function
            // and would put the ruled-out array arm back in the element.
            let scope_type = ctx.scope_var_resolver.and_then(|resolver| {
                let prefixed = if var_text.starts_with('$') {
                    var_text.clone()
                } else {
                    format!("${}", var_text)
                };
                let from_scope = resolver(&prefixed);
                (!from_scope.is_empty())
                    .then(|| crate::types::ResolvedType::types_joined(&from_scope))
            });
            if let Some(t) = scope_type {
                return Some(t);
            }
            // A `@var`/`@param` annotation read straight out of the source
            // (e.g. `@var list<User> $items`), for a variable nothing above
            // could type.
            let annotated = || {
                docblock::find_iterable_raw_type_in_source(ctx.content, offset, &var_text)
                    .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader))
            };
            // Inside the forward walker the scope above was the only
            // narrowing-aware answer on offer: running the full pipeline
            // here would walk the enclosing body all over again.  The
            // annotation is what is left.
            if ctx.scope_var_resolver.is_some() {
                return annotated();
            }
            // Outside the walker the full pipeline (parameter type hints,
            // `@param`/`@var` docblocks, assignments, foreach bindings)
            // answers this, and it goes first because it reads the same
            // annotations *and* the narrowing that holds where the literal
            // is written.  A `[$n]` under `if (is_int($n))` builds
            // `array{int}`, not the `int|float` the annotation states for
            // the assignment above it.
            let current_class = ctx
                .all_classes
                .iter()
                .find(|c| c.name == ctx.current_class.name)
                .map(|c| c.as_ref());
            crate::type_engine::variable::resolution::resolve_variable_php_type(
                &var_text,
                ctx.content,
                offset as u32,
                current_class,
                ctx.all_classes,
                ctx.class_loader,
                ctx.backend,
                ctx.loaders,
            )
            .or_else(annotated)
        }
        // ── Parenthesized ──
        Expression::Parenthesized(p) => infer_element_type(p.expression, ctx),
        // ── Property access, method calls on objects, etc. ──
        // Delegate to the unified pipeline which resolves property
        // type hints and method return types through the class
        // hierarchy.
        _ => super::foreach_resolution::resolve_expression_type(value, ctx),
    }
}

/// [`ArrayFuncArgs`] over a parsed argument list.
struct AstArrayFuncArgs<'a, 'ast, 'ctx> {
    args: &'a ArgumentList<'ast>,
    ctx: &'a VarResolutionCtx<'ctx>,
}

impl ArrayFuncArgs for AstArrayFuncArgs<'_, '_, '_> {
    fn arg_raw_type(&self, index: usize) -> Option<PhpType> {
        let expr = super::resolution::nth_arg_expr(self.args, index)?;
        super::resolution::resolve_arg_raw_type(expr, self.ctx)
    }

    fn bool_literal(&self, index: usize) -> Option<bool> {
        match super::resolution::nth_arg_expr(self.args, index)? {
            Expression::Literal(Literal::True(_)) => Some(true),
            Expression::Literal(Literal::False(_)) => Some(false),
            _ => None,
        }
    }

    fn has_arg(&self, index: usize) -> bool {
        super::resolution::nth_arg_expr(self.args, index).is_some()
    }

    fn is_spread(&self, index: usize) -> bool {
        matches!(
            self.args.arguments.iter().nth(index),
            Some(Argument::Positional(pos)) if pos.ellipsis.is_some()
        )
    }

    fn callback_declared_return_type(&self, index: usize) -> Option<PhpType> {
        // A written `: ReturnType` carries the file's own spelling of the
        // class (`Support\Pen` behind a `use App\Support;`), and it is
        // compared against types that arrived fully qualified, so the
        // spelling is canonicalised on the way out.
        let qualify = |ty: PhpType| crate::util::resolve_php_type_names(&ty, self.ctx.class_loader);
        match super::resolution::nth_arg_expr(self.args, index)? {
            Expression::Closure(closure) => closure
                .return_type_hint
                .as_ref()
                .map(|rth| qualify(extract_hint_type(&rth.hint))),
            Expression::ArrowFunction(arrow) => arrow
                .return_type_hint
                .as_ref()
                .map(|rth| qualify(extract_hint_type(&rth.hint))),
            // `array_map('intval', $xs)` names its callback instead of
            // spelling it out; the named function's own return type is what
            // the call produces.
            Expression::Literal(Literal::String(s)) => {
                let name =
                    super::array_func_rules::callable_string_function_name(bytes_to_str(s.raw))?;
                (self.ctx.loaders.function_loader?)(name, 0)?.return_type
            }
            _ => None,
        }
    }

    fn callback_inferred_return_type(&self, index: usize, param_type: &PhpType) -> Option<PhpType> {
        let expr = super::resolution::nth_arg_expr(self.args, index)?;
        infer_callback_return_type(expr, param_type, self.ctx)
    }

    fn arg_atom_text(&self, index: usize) -> Option<String> {
        match super::resolution::nth_arg_expr(self.args, index)? {
            Expression::ConstantAccess(ca) => {
                Some(crate::util::strip_fqn_prefix(bytes_to_str(ca.name.value())).to_string())
            }
            Expression::Literal(Literal::Integer(i)) => Some(bytes_to_str(i.raw).to_string()),
            _ => None,
        }
    }

    fn callback_param_narrowing(
        &self,
        index: usize,
        param_index: usize,
        subject: &PhpType,
    ) -> Option<PhpType> {
        let expr = super::resolution::nth_arg_expr(self.args, index)?;
        super::callback_narrowing::narrow_callback_param(
            expr,
            param_index,
            subject,
            Some(&self.ctx.class_loader),
        )
    }

    fn narrows(&self, inferred: &PhpType, declared: &PhpType) -> bool {
        crate::class_lookup::is_subtype_of_typed(inferred, declared, self.ctx.class_loader)
    }
}

/// For known array-producing functions, resolve the **raw output type**
/// (e.g. `list<User>`) from the input arguments.
///
/// Used by foreach and destructuring resolution so that iterating over
/// `array_filter(...)` etc. preserves element types.  Element-extracting
/// functions are handled by [`resolve_array_func_element_type`], which the
/// caller consults first.
pub(in crate::type_engine) fn resolve_array_func_raw_type(
    func_name: &str,
    args: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    array_func_raw_type(func_name, &AstArrayFuncArgs { args, ctx })
}

/// For known array functions, resolve the **element type**
/// (e.g. `User`) of the output.
///
/// Used by `resolve_rhs_expression` so that `$item = array_pop($users)`
/// resolves `$item` to `User`.
pub(in crate::type_engine) fn resolve_array_func_element_type(
    func_name: &str,
    args: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    array_func_element_type(func_name, &AstArrayFuncArgs { args, ctx })
}

/// Constant-fold a string builtin whose arguments are all literals.
///
/// See [`super::string_func_rules`] for which functions fold and why the
/// literal answer matters.
pub(in crate::type_engine) fn resolve_string_func_literal_type(
    func_name: &str,
    args: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    if !super::string_func_rules::is_foldable_string_func(func_name) {
        return None;
    }
    super::string_func_rules::string_func_literal_type(func_name, &AstArrayFuncArgs { args, ctx })
}

/// Extract per-argument source text from a parsed `ArgumentList`.
///
/// Returns one `String` per argument by walking the AST nodes and
/// extracting their spans. This avoids serialising the argument list
/// to a flat string and then re-splitting with `split_text_args`.
pub(in crate::type_engine) fn extract_arg_texts_from_ast(
    argument_list: &mago_syntax::cst::ArgumentList<'_>,
    content: &str,
) -> Vec<String> {
    argument_list
        .arguments
        .iter()
        .map(|arg| {
            let value_span = match arg {
                mago_syntax::cst::argument::Argument::Positional(pos) => pos.value.span(),
                mago_syntax::cst::argument::Argument::Named(named) => named.value.span(),
            };
            let start = value_span.start.offset as usize;
            let end = value_span.end.offset as usize;
            let value = if end <= content.len() {
                &content[start..end]
            } else {
                ""
            };
            // Preserve the `name:` prefix for named arguments so that
            // downstream argument binding (`bind_text_args_to_params`) can
            // route them to the parameter they target rather than their
            // source-order slot. Without it, `f(b: 1, a: 2)` would bind `a`
            // to the value `1` and misresolve conditional return types and
            // template parameters that key on `a`.
            match arg {
                mago_syntax::cst::argument::Argument::Named(named) => {
                    let name = crate::atom::bytes_to_str(named.name.value);
                    format!("{name}: {value}")
                }
                mago_syntax::cst::argument::Argument::Positional(_) => value.to_string(),
            }
        })
        .collect()
}

/// Infer the return type of a callback (arrow function or closure) by
/// resolving its body expression with the first parameter seeded to
/// `param_type`.
///
/// For arrow functions: resolves `arrow.expression` directly.
/// For closures: finds the first `return` statement and resolves its
/// expression.
fn infer_callback_return_type(
    callback_expr: &Expression<'_>,
    param_type: &PhpType,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let (param_name, body_expr) = match callback_expr {
        Expression::ArrowFunction(arrow) => {
            let param = arrow.parameter_list.parameters.first()?;
            let name = bytes_to_str(param.variable.name).to_string();
            (name, arrow.expression)
        }
        Expression::Closure(closure) => {
            let param = closure.parameter_list.parameters.first()?;
            let name = bytes_to_str(param.variable.name).to_string();
            // Find the first return statement's expression.
            let ret_expr = closure.body.statements.iter().find_map(|stmt| {
                if let Statement::Return(ret) = stmt {
                    ret.value.as_ref()
                } else {
                    None
                }
            })?;
            (name, *ret_expr)
        }
        _ => return None,
    };

    // Build a scope resolver that maps the callback parameter to the
    // input element type.  Include ClassInfo when available so that
    // property access resolution can find the class members.
    //
    // A union element type seeds one entry per alternative rather than one
    // entry holding the whole union: two instantiations of the same class
    // (`Builder<A>|Builder<B>`) resolve to one class, and a single entry
    // could only carry the union as its type string, leaving a `@return T`
    // on that class with no instantiation to substitute from.
    let seed_member = |ty: &PhpType| -> ResolvedType {
        match ty.base_name().and_then(|name| (ctx.class_loader)(name)) {
            Some(cls) => ResolvedType::from_both(ty.clone(), (*cls).clone()),
            None => ResolvedType::from_type_string(ty.clone()),
        }
    };
    let resolved_param: Vec<ResolvedType> = match param_type.kind() {
        crate::php_type::TypeKind::Union(members) => members.iter().map(seed_member).collect(),
        _ => vec![seed_member(param_type)],
    };
    let scope_resolver = move |var: &str| -> Vec<ResolvedType> {
        if var == param_name {
            resolved_param.clone()
        } else {
            vec![]
        }
    };

    // Create a synthetic context with the scope resolver.
    let body_offset = body_expr.span().start.offset;
    let infer_ctx = VarResolutionCtx {
        var_name: "",
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: body_offset,
        class_loader: ctx.class_loader,
        backend: ctx.backend,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: None,
        top_level_scope: None,
        branch_aware: false,
        match_arm_narrowing: std::collections::HashMap::new(),
        scope_var_resolver: Some(&scope_resolver),
        scope_proofs: None,
    };

    super::foreach_resolution::resolve_expression_type(body_expr, &infer_ctx)
}
