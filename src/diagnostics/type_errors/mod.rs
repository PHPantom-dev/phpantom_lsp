//! Argument type mismatch diagnostics.
//!
//! Walk the precomputed [`CallSite`] entries in the symbol map and flag
//! every call where an argument's resolved type is incompatible with
//! the declared parameter type.
//!
//! This is Phase 1 of the type error diagnostic suite. Only clearly
//! incompatible types are flagged — when in doubt (unresolved types,
//! `mixed`, complex generics), the diagnostic is suppressed to avoid
//! false positives.

pub(super) mod compatibility;

pub(super) use compatibility::is_type_compatible;
use compatibility::{missing_required_shape_keys, shape_breaks_list_order};

use std::collections::{HashMap, HashSet};

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::statement::Statement;
use mago_syntax::cst::{PartialArgument, Program};
use mago_syntax::walker::Walker;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::php_type::{PhpType, TypeKind, is_array_like_name};
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};
use crate::type_engine::variable::foreach_resolution::resolve_expression_type;
use crate::types::ResolvedCallableTarget;

use super::helpers::{
    find_innermost_enclosing_class, is_position_independent_call_expression, make_diagnostic,
};

/// Diagnostic code used for argument type mismatch diagnostics.
pub(crate) const TYPE_MISMATCH_ARGUMENT_CODE: &str = "type_mismatch_argument";

// ── Resolved argument info ──────────────────────────────────────────────────

/// A single argument's resolved type plus the byte range of the
/// expression in source.  Collected inside `with_parsed_program` so
/// we don't need to keep AST references alive.
struct ResolvedArg {
    /// The resolved type of the argument expression.
    ty: PhpType,
    /// Byte offset of the argument expression start (inclusive).
    start: usize,
    /// Byte offset of the argument expression end (exclusive).
    end: usize,
    /// String literal values extracted from array argument elements,
    /// with their byte offset spans.  Used for `model-property<Model>`
    /// validation when the param type is an array with `model-property`
    /// in a generic position.
    array_string_literals: Vec<(String, usize, usize)>,
    /// Whether the argument is an array written out at the call site
    /// with every key spelled as a literal, so its resolved shape lists
    /// all the keys the value has rather than the ones we happened to
    /// see.  See [`enumerates_all_keys`].
    enumerates_all_keys: bool,
}

/// All resolved argument types for a single call site.
struct ResolvedCallArgs {
    args: Vec<ResolvedArg>,
}

/// Returns `true` when the file declares `strict_types=1`.
///
/// Scans the top-level statements of the parsed program for a
/// `declare(strict_types=1)` directive.  In PHP this must appear as
/// the very first statement (after `<?php`), but we check all
/// top-level statements for robustness.
pub(super) fn has_strict_types(program: &Program<'_>) -> bool {
    for stmt in program.statements.iter() {
        if let Statement::Declare(declare) = stmt {
            for item in declare.items.iter() {
                if bytes_to_str(item.name.value).eq_ignore_ascii_case("strict_types")
                    && let Expression::Literal(Literal::Integer(i)) = item.value
                    && bytes_to_str(i.raw) == "1"
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether the expression is an array literal whose keys are all known
/// statically, so the shape inferred from it is the whole array rather
/// than a lower bound on it.
///
/// This is what separates a missing key from an unproven one. A shape
/// that came from a variable was built by watching assignments, and a
/// key we never saw assigned may still be there at runtime — through a
/// branch we could not follow, a constant key we could not read, or a
/// merge somewhere upstream. An array written out at the call site has
/// no such history: every key is right there.
///
/// A single entry written without a key, or spread in with `...`, is
/// enough to disqualify the literal, because the shape inference drops
/// positional entries as soon as the array has string keys. `[]` itself
/// enumerates all (zero) of its keys, so it is not disqualified.
fn enumerates_all_keys(expr: &Expression<'_>) -> bool {
    use mago_syntax::cst::array::ArrayElement;

    let elements = match expr {
        Expression::Array(arr) => &arr.elements,
        Expression::LegacyArray(arr) => &arr.elements,
        _ => return false,
    };

    elements.iter().all(|elem| match elem {
        ArrayElement::KeyValue(kv) => matches!(
            kv.key,
            Expression::Literal(Literal::String(_) | Literal::Integer(_))
        ),
        _ => false,
    })
}

/// Extract string literal values from array expression elements.
///
/// Collects both keys and values so that `model-property<Model>`
/// validation works regardless of whether the model-property type
/// is in the key or value position of the param's array generic.
fn extract_array_string_literals(expr: &Expression<'_>) -> Vec<(String, usize, usize)> {
    use mago_syntax::cst::array::ArrayElement;

    let elements = match expr {
        Expression::Array(arr) => &arr.elements,
        Expression::LegacyArray(arr) => &arr.elements,
        _ => return Vec::new(),
    };

    let mut literals = Vec::new();
    let mut push_string = |s: &mago_syntax::cst::literal::LiteralString| {
        if let Some(content) = crate::text_scan::unquote_php_string(bytes_to_str(s.raw)) {
            let start = s.span.start.offset as usize;
            let end = s.span.end.offset as usize;
            literals.push((content.to_string(), start, end));
        }
    };
    for elem in elements.iter() {
        match elem {
            ArrayElement::KeyValue(kv) => {
                if let Expression::Literal(Literal::String(s)) = kv.key {
                    push_string(s);
                }
                if let Expression::Literal(Literal::String(s)) = kv.value {
                    push_string(s);
                }
            }
            ArrayElement::Value(v) => {
                if let Expression::Literal(Literal::String(s)) = v.value {
                    push_string(s);
                }
            }
            _ => {}
        }
    }
    literals
}

/// Extract the model name from a `model-property<Model>` type that
/// appears as a generic argument of an array/list type.
fn extract_model_property_from_array_type(ty: &PhpType) -> Option<String> {
    let TypeKind::Generic(g) = ty.kind() else {
        return None;
    };
    if !is_array_like_name(&g.name) && !g.name.eq_ignore_ascii_case("list") {
        return None;
    }
    for arg in &g.args {
        if let TypeKind::Generic(inner) = arg.kind()
            && inner.name.eq_ignore_ascii_case("model-property")
            && inner.args.len() == 1
        {
            return inner.args[0].base_name().map(|s| s.to_string());
        }
    }
    None
}

// ── AST walking: collect argument expressions keyed by args_start ───────────

/// Try to register the argument expressions of an [`ArgumentList`] if its
/// `args_start` offset matches one of the call sites we are interested in.
fn try_collect_argument_list<'a>(
    arg_list: &'a mago_syntax::cst::argument::ArgumentList<'a>,
    call_site_starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    let args_start = arg_list.left_parenthesis.end.offset;
    if !call_site_starts.contains(&args_start) {
        return;
    }
    let expressions: Vec<(&'a Expression<'a>, usize, usize)> = arg_list
        .arguments
        .iter()
        .map(|arg| {
            let value = match arg {
                Argument::Positional(pos) => pos.value,
                Argument::Named(named) => named.value,
            };
            let start = value.span().start.offset as usize;
            let end = value.span().end.offset as usize;
            (value, start, end)
        })
        .collect();
    result.insert(args_start, expressions);
}

/// Try to register the partial argument expressions of an [`PartialArgumentList`] if its
/// `args_start` offset matches one of the call sites we are interested in.
fn try_collect_partial_argument_list<'a>(
    arg_list: &'a mago_syntax::cst::argument::PartialArgumentList<'a>,
    call_site_starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    let args_start = arg_list.left_parenthesis.end.offset;
    if !call_site_starts.contains(&args_start) {
        return;
    }
    // Placeholders have no expression and therefore no type to validate.
    // Keep only supplied positional/named arguments.
    let expressions: Vec<(&'a Expression<'a>, usize, usize)> = arg_list
        .arguments
        .iter()
        .filter_map(|arg| {
            let value = match arg {
                PartialArgument::Positional(pos) => pos.value,
                PartialArgument::Named(named) => named.value,
                PartialArgument::NamedPlaceholder(_)
                | PartialArgument::Placeholder(_)
                | PartialArgument::VariadicPlaceholder(_) => return None,
            };
            let start = value.span().start.offset as usize;
            let end = value.span().end.offset as usize;
            Some((value, start, end))
        })
        .collect();
    result.insert(args_start, expressions);
}

// ── AST walk: collect call/instantiation argument lists ────────────────────

/// Walker that records the argument expressions of every call site whose
/// `args_start` offset is one we care about (`starts`). The generated
/// traversal visits every argument list in the tree; the two overrides
/// register the ones in `starts`, and default recursion descends into
/// argument values to find nested calls.
struct CallArgWalker<'s> {
    starts: &'s HashSet<u32>,
}

impl<'a, 's>
    mago_syntax::walker::Walker<'a, 'a, HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>>
    for CallArgWalker<'s>
{
    fn walk_in_argument_list(
        &self,
        node: &'a mago_syntax::cst::argument::ArgumentList<'a>,
        context: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
    ) {
        try_collect_argument_list(node, self.starts, context);
    }

    fn walk_in_partial_argument_list(
        &self,
        node: &'a mago_syntax::cst::argument::PartialArgumentList<'a>,
        context: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
    ) {
        try_collect_partial_argument_list(node, self.starts, context);
    }
}

// ── Main diagnostic collection ──────────────────────────────────────────────

impl Backend {
    /// Collect argument type mismatch diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_argument_type_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        // ── Gather context under locks ──────────────────────────────
        let symbol_map = {
            let maps = self.symbol_maps.read();
            match maps.get(uri) {
                Some(sm) => sm.clone(),
                None => return,
            }
        };

        let file_ctx = self.file_context(uri);

        // Activate the thread-local parse cache so that every call to
        // `with_parsed_program(content, …)` in the resolution pipeline
        // reuses the same parsed AST instead of re-parsing the file.
        let _parse_guard = with_parse_cache(content);

        // Build the set of call site args_start offsets for the AST walk.
        // Only include sites without argument unpacking.
        let call_site_starts: HashSet<u32> = symbol_map
            .call_sites
            .iter()
            .filter(|cs| !cs.has_unpacking)
            .map(|cs| cs.args_start)
            .collect();

        if call_site_starts.is_empty() {
            return;
        }

        let class_loader = self.class_loader(&file_ctx);
        let function_loader_cl = self.function_loader(&file_ctx);
        let constant_loader_cl = self.constant_loader(&file_ctx);
        // Read once for the whole file: `config()` clones the config
        // behind a lock, and the flag cannot change mid-pass.
        let downgrade_nullable_mismatch = self
            .config()
            .diagnostics
            .downgrade_nullable_argument_mismatch_enabled();

        // Walk the AST once, collect argument expressions, and resolve
        // their types — all inside the `with_parsed_program` closure so
        // AST references never escape the arena lifetime.
        let (resolved_map, strict_types): (HashMap<u32, ResolvedCallArgs>, bool) =
            with_parsed_program(content, "type_error_diagnostics", |program, _content| {
                let strict_types = has_strict_types(program);

                // Phase 1: walk the AST and collect raw argument expressions
                // keyed by args_start offset.
                let mut expr_map: HashMap<u32, Vec<(&Expression<'_>, usize, usize)>> =
                    HashMap::new();
                let walker = CallArgWalker {
                    starts: &call_site_starts,
                };
                for stmt in program.statements.iter() {
                    walker.walk_statement(stmt, &mut expr_map);
                }

                // Phase 2: resolve types for each collected expression.
                let mut result: HashMap<u32, ResolvedCallArgs> = HashMap::new();
                for (args_start, exprs) in &expr_map {
                    let placeholder;
                    let current_class_info =
                        match find_innermost_enclosing_class(&file_ctx.classes, *args_start) {
                            Some(cc) => cc,
                            None => {
                                placeholder = crate::class_lookup::class_context_placeholder(
                                    content,
                                    *args_start,
                                );
                                &placeholder
                            }
                        };

                    let config_resolver = |key: &str| self.resolve_config_type(key);
                    let trans_resolver = |key: &str| self.resolve_trans_type(key);
                    let loaders = Loaders {
                        function_loader: Some(&function_loader_cl),
                        constant_loader: Some(&constant_loader_cl),
                        config_resolver: Some(&config_resolver),
                        trans_resolver: Some(&trans_resolver),
                    };

                    let var_ctx = VarResolutionCtx {
                        var_name: "",
                        top_level_scope: None,
                        current_class: current_class_info,
                        all_classes: &file_ctx.classes,
                        content,
                        cursor_offset: *args_start,
                        class_loader: &class_loader,
                        backend: Some(self),
                        loaders,
                        resolved_class_cache: Some(&self.resolved_class_cache),
                        enclosing_return_type: None,
                        branch_aware: true,
                        match_arm_narrowing: HashMap::new(),
                        scope_var_resolver: None,
                        scope_proofs: None,
                    };

                    let mut resolved_args = Vec::with_capacity(exprs.len());
                    for &(arg_expr, start, end) in exprs {
                        // Use the argument expression's own start offset
                        // as the cursor position so that variable
                        // resolution only sees assignments *before* this
                        // expression.  Without this, `$type = Enum::from($type)`
                        // would resolve the RHS `$type` to `Enum` (the
                        // result of the assignment on the same line)
                        // instead of the prior `string` value from the
                        // foreach key.
                        let arg_ctx = var_ctx.with_cursor_offset(start as u32);
                        let ty = resolve_expression_type(arg_expr, &arg_ctx)
                            .unwrap_or_else(PhpType::untyped);
                        // Resolve any short class names in the arg type
                        // to FQN via the class loader.  Variable
                        // resolution may return raw docblock names
                        // (e.g. `SubscriptionProduct` instead of its
                        // FQN) — normalise them so that comparisons
                        // against parameter types (which are already
                        // FQN from resolve_parent_class_names) succeed.
                        let ty = ty.resolve_names(&|name: &str| {
                            // Don't FQN-ify anonymous class names — they
                            // use synthetic names (`__anonymous@<offset>`)
                            // that are only resolvable via the local-class
                            // shortcut in the class loader.  Prepending
                            // the namespace would break that lookup.
                            if name.contains("__anonymous@") {
                                return name.to_string();
                            }
                            if let Some(cls) = class_loader(name) {
                                cls.fqn().to_string()
                            } else {
                                name.to_string()
                            }
                        });
                        // Expand @phpstan-type / @psalm-type aliases so
                        // that e.g. `Payload` becomes `array{name: string,
                        // phone: string}` before the compatibility check.
                        let ty = crate::type_engine::types::resolution::resolve_type_alias_typed(
                            &ty,
                            &current_class_info.fqn(),
                            &file_ctx.classes,
                            &class_loader,
                        )
                        .unwrap_or(ty);
                        let array_string_literals = extract_array_string_literals(arg_expr);
                        resolved_args.push(ResolvedArg {
                            ty,
                            start,
                            end,
                            array_string_literals,
                            enumerates_all_keys: enumerates_all_keys(arg_expr),
                        });
                    }
                    result.insert(
                        *args_start,
                        ResolvedCallArgs {
                            args: resolved_args,
                        },
                    );
                }
                (result, strict_types)
            });

        // Call-expression resolution cache: avoids re-resolving the
        // same call expression (e.g. `ClassName::method`) at every
        // call site that uses it.
        //
        // Only expressions that are guaranteed to resolve to the same
        // target everywhere in the file are cached (see
        // `is_position_independent_call_expression`).  Variable-based
        // calls (`$listener->handle`, `$repo->save`) are NOT cached
        // because the same variable name can hold different types in
        // different methods or after reassignment, and neither are
        // `self::`/`static::`/`parent::` calls, whose target depends on
        // the enclosing class at the call site.  Calls through a
        // literal class name (`Foo::bar`) and plain function calls
        // (`array_map`) are safe to cache.
        let mut call_cache: HashMap<String, Option<ResolvedCallableTarget>> = HashMap::new();

        // ── Walk every call site ────────────────────────────────────
        for call_site in &symbol_map.call_sites {
            // Skip calls with argument unpacking — actual types of
            // individual arguments are unknown.
            if call_site.has_unpacking {
                continue;
            }

            let expr = &call_site.call_expression;

            // Look up or populate the call expression cache.
            // Variable-based and `self::`/`static::`/`parent::` calls
            // are resolved fresh every time because their target
            // depends on the call site's position (the receiver
            // variable's assigned type, or the enclosing class).
            let is_position_dependent_call = !is_position_independent_call_expression(expr);

            // Extract the raw argument text from the source so that
            // method-level @template parameters can be resolved from
            // the call-site argument types.
            let call_args_text: Option<&str> = {
                let start = call_site.args_start as usize;
                let end = call_site.args_end as usize;
                if let Some(slice) = content.get(start..end) {
                    let trimmed = slice.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                } else {
                    None
                }
            };

            // Resolve the callable target.  Class-level template
            // substitution happens inside resolve_callable_target
            // (driven by the variable's generic type string).
            // Method-level template substitution uses the per-site
            // argument text extracted above.
            //
            // Position-dependent calls are always resolved per-site
            // because their target may differ at different call sites.
            // Position-independent calls are also resolved per-site
            // when argument text is available (method-level template
            // subs depend on per-site args); only zero-arg calls can
            // be cached.
            let resolved = if is_position_dependent_call || call_args_text.is_some() {
                self.resolve_callable_target_with_args_at_offset(
                    expr,
                    content,
                    call_site.args_start,
                    &file_ctx,
                    call_args_text,
                )
            } else {
                call_cache
                    .entry(expr.clone())
                    .or_insert_with(|| {
                        self.resolve_callable_target_at_offset(
                            expr,
                            content,
                            call_site.args_start,
                            &file_ctx,
                        )
                    })
                    .clone()
            };

            let resolved = match resolved {
                Some(r) => r,
                None => continue,
            };

            let resolved_args = match resolved_map.get(&call_site.args_start) {
                Some(c) => c,
                None => continue,
            };

            let params = &resolved.parameters;

            // Track how many positional args we've seen so far for
            // mapping positional args to parameter indices.
            let mut positional_idx: usize = 0;

            // Class that `self` / `static` / `parent` in the signature
            // refer to.  This is the class the callable was resolved on,
            // never the syntactic prefix of the call expression: in
            // `$this->state->canChangeTo(…)` the enclosing class is not
            // the one declaring `canChangeTo`.  `self` on an inherited
            // method is already bound to its declaring class by the
            // inheritance merge, so anything still bare here was
            // declared on the owner itself.
            let call_context_class: Option<String> =
                resolved.owner_class.map(|fqn| fqn.to_string());
            let ctx_parent_fqn: Option<String> = call_context_class.as_ref().and_then(|fqn| {
                class_loader(fqn).and_then(|cls| cls.parent_class.as_ref().map(|p| p.to_string()))
            });

            for (arg_idx, resolved_arg) in resolved_args.args.iter().enumerate() {
                // Skip spread arguments.
                if call_site.spread_arg_indices.contains(&(arg_idx as u32)) {
                    continue;
                }

                // Find the corresponding parameter.
                let param = if call_site.named_arg_indices.contains(&(arg_idx as u32)) {
                    // Named argument: look up parameter by name.
                    let name_pos = call_site
                        .named_arg_indices
                        .iter()
                        .position(|&i| i == arg_idx as u32);
                    match name_pos {
                        Some(idx) => {
                            let param_name = &call_site.named_arg_names[idx];
                            params
                                .iter()
                                .find(|p| p.name.trim_start_matches('$') == param_name.as_str())
                        }
                        None => continue,
                    }
                } else {
                    // Positional argument.
                    let p = params.get(positional_idx);
                    positional_idx += 1;
                    p
                };

                let param = match param {
                    Some(p) => p,
                    None => continue, // Extra argument beyond declared params
                };

                // Skip a parameter whose substituted type came solely from
                // resolving this very argument (e.g. PHPUnit's
                // `assertSame(ExpectedType $expected, mixed $actual)`,
                // where `ExpectedType` is bound only by `$expected`).
                // Comparing the argument to its own substitution is
                // circular and can only produce false positives.
                if resolved.self_bound_params.contains(&param.name) {
                    continue;
                }

                // An out-parameter's declared type describes what the
                // callee *writes* through the reference, not what the
                // caller has to hand over. The value going in is whatever
                // the variable happened to hold, and the standard library
                // — where nearly every parameter of this shape comes from
                // — does not check it: `preg_match_all(…, $matches,
                // PREG_OFFSET_CAPTURE)` inside a loop is handed the
                // previous iteration's shape, and PHP accepts a `$matches`
                // holding a string just as readily as one that does not
                // exist yet. There is nothing here for this check to
                // compare against; what the *call* leaves behind is typed
                // by the by-reference write-back instead.
                if param.is_reference && param.defaults_to_null() {
                    continue;
                }

                // Skip if parameter has no type hint.
                let param_type = match &param.type_hint {
                    Some(t) if !t.is_untyped() && !t.is_mixed() => t,
                    _ => continue,
                };

                // Skip variadic parameters — hard to match individual
                // arg types to the variadic param's inner type.
                if param.is_variadic {
                    continue;
                }

                let arg_type = &resolved_arg.ty;
                // Skip unresolved / empty / Raw("") sentinel types.
                if arg_type.is_untyped()
                    || arg_type.is_empty()
                    || matches!(arg_type.kind(), TypeKind::Raw(s) if s.is_empty())
                {
                    continue;
                }

                let resolved_param;
                let effective_param_type = if param_type.contains_relative_class_ref() {
                    if let Some(ref fqn) = call_context_class {
                        resolved_param =
                            param_type.resolve_self_refs_bounded(fqn, ctx_parent_fqn.as_deref());
                        &resolved_param
                    } else {
                        param_type
                    }
                } else {
                    param_type
                };

                // The compatibility layer treats an array shape as an open
                // description — a key it does not mention may still be
                // there at runtime — which is right for a shape inferred
                // from a variable but not for one written out at the call
                // site. There the keys are all of them, so a required key
                // that is not among them is genuinely absent. A key that
                // *is* there with the wrong value type stays the
                // compatibility layer's call.
                let missing_keys = if resolved_arg.enumerates_all_keys {
                    missing_required_shape_keys(arg_type, effective_param_type)
                } else {
                    Vec::new()
                };

                // Same reasoning for the keys a `list` demands: only a
                // literal that spells every key out proves the value's keys
                // are in an order `array_is_list()` would reject.
                let breaks_list_order = resolved_arg.enumerates_all_keys
                    && shape_breaks_list_order(arg_type, effective_param_type);

                if missing_keys.is_empty()
                    && !breaks_list_order
                    && is_type_compatible(
                        arg_type,
                        effective_param_type,
                        &class_loader,
                        strict_types,
                    )
                {
                    // Even when the array types are compatible, validate
                    // string literals against model-property<Model> when
                    // the param type is an array with model-property in
                    // a generic position.
                    if !resolved_arg.array_string_literals.is_empty()
                        && let Some(model_fqn) = extract_model_property_from_array_type(param_type)
                        && let Some(cls) = class_loader(&model_fqn)
                    {
                        let resolved = crate::virtual_members::resolve_class_fully_cached(
                            &cls,
                            &class_loader,
                            &self.resolved_class_cache,
                        );
                        let columns: Vec<String> = resolved
                            .properties
                            .iter()
                            .map(|p| p.name.to_string())
                            .collect();
                        for (lit, lit_start, lit_end) in &resolved_arg.array_string_literals {
                            let found = columns.iter().any(|col| col == lit);
                            if !found
                                && let Some(range) = self
                                    .offset_range_to_lsp_range(uri, content, *lit_start, *lit_end)
                            {
                                out.push(make_diagnostic(
                                    range,
                                    DiagnosticSeverity::ERROR,
                                    TYPE_MISMATCH_ARGUMENT_CODE,
                                    format!(
                                        "'{}' is not a known property of {}",
                                        lit,
                                        model_fqn.rsplit('\\').next().unwrap_or(&model_fqn),
                                    ),
                                ));
                            }
                        }
                    }
                    continue;
                }

                // When the function has overloaded signatures, check
                // whether the argument is compatible with the same
                // positional parameter in any overload.  Only emit the
                // diagnostic when ALL signatures reject it.
                if !resolved.overloads.is_empty() {
                    let compatible_with_overload = resolved.overloads.iter().any(|alt_params| {
                        if let Some(alt_param) = alt_params.get(positional_idx.saturating_sub(1)) {
                            if let Some(ref alt_type) = alt_param.type_hint
                                && !alt_type.is_untyped()
                                && !alt_type.is_mixed()
                            {
                                let resolved_alt;
                                let effective_alt = if alt_type.contains_relative_class_ref() {
                                    if let Some(ref fqn) = call_context_class {
                                        resolved_alt = alt_type.resolve_self_refs_bounded(
                                            fqn,
                                            ctx_parent_fqn.as_deref(),
                                        );
                                        &resolved_alt
                                    } else {
                                        alt_type
                                    }
                                } else {
                                    alt_type
                                };
                                return is_type_compatible(
                                    arg_type,
                                    effective_alt,
                                    &class_loader,
                                    strict_types,
                                );
                            }
                            true // no type hint on alt param = compatible
                        } else {
                            // This overload has fewer params — the arg
                            // doesn't correspond to any parameter, so
                            // it's not relevant for this check.
                            false
                        }
                    });
                    if compatible_with_overload {
                        continue;
                    }
                }

                let range = match self.offset_range_to_lsp_range(
                    uri,
                    content,
                    resolved_arg.start,
                    resolved_arg.end,
                ) {
                    Some(r) => r,
                    None => continue,
                };

                let param_name = &param.name;
                // Always show full type names (FQN) so the developer
                // can actually find and fix the types.  Short names
                // strip the namespace which is the very information
                // needed to resolve the mismatch.  Report the same
                // branch-union collapse the compatibility check made, so
                // an undecided conditional is named by the types it can
                // actually be rather than as a type expression.
                let mut message = format!(
                    "Argument {} ({}) expects {}, got {}",
                    arg_idx + 1,
                    param_name,
                    effective_param_type.conditionals_as_branch_unions(),
                    arg_type,
                );
                // Two callables only ever disagree here on their return
                // type, and the parameter list printed for the argument is
                // empty whether or not the closure declares parameters —
                // say which halves were actually compared so the empty
                // parentheses don't read as the complaint.
                if let (TypeKind::Callable(arg_sig), TypeKind::Callable(param_sig)) =
                    (arg_type.kind(), effective_param_type.kind())
                    && let (Some(arg_return), Some(param_return)) =
                        (&arg_sig.return_type, &param_sig.return_type)
                {
                    message.push_str(&format!(
                        " (return type {arg_return} does not satisfy {param_return})"
                    ));
                }
                // "got void" reads as a type the value happens to have
                // rather than as the absence of one, so say what the call
                // site actually did.
                if arg_type.is_void() {
                    message.push_str(" (the expression returns no value)");
                }
                // Two shapes that differ by a key the developer forgot
                // read as two long type expressions to diff by eye. Name
                // the keys that are missing instead.
                if !missing_keys.is_empty() {
                    message.push_str(&format!(
                        " (missing required key{} {})",
                        if missing_keys.len() == 1 { "" } else { "s" },
                        missing_keys
                            .iter()
                            .map(|key| format!("'{key}'"))
                            .collect::<Vec<_>>()
                            .join(", "),
                    ));
                }
                // The two types printed above can look interchangeable when
                // the only thing separating them is the order of the keys,
                // so say that that is the complaint.
                if breaks_list_order {
                    message.push_str(" (keys are not in list order)");
                }
                // Name the specific member(s) that broke a partially
                // compatible union, rather than leaving the developer to
                // work out which one out of the full union is at fault.
                let mut only_null_unsatisfied = false;
                match arg_type.kind() {
                    TypeKind::Union(members) => {
                        let unsatisfied: Vec<&PhpType> = members
                            .iter()
                            .filter(|m| {
                                !is_type_compatible(
                                    m,
                                    effective_param_type,
                                    &class_loader,
                                    strict_types,
                                )
                            })
                            .collect();
                        if !unsatisfied.is_empty() && unsatisfied.len() < members.len() {
                            message.push_str(&format!(
                                " ({} does not satisfy {})",
                                unsatisfied
                                    .iter()
                                    .map(|m| m.to_string())
                                    .collect::<Vec<_>>()
                                    .join("|"),
                                effective_param_type.conditionals_as_branch_unions(),
                            ));
                            only_null_unsatisfied = unsatisfied.iter().all(|m| m.is_null());
                        }
                    }
                    // `?T` is the same gap as `T|null`, spelled shorter, so
                    // it has to answer the same way. It carries no union
                    // members to name, so only the severity is at stake and
                    // the check is worth nothing unless it can change it.
                    TypeKind::Nullable(inner) if downgrade_nullable_mismatch => {
                        only_null_unsatisfied = is_type_compatible(
                            inner,
                            effective_param_type,
                            &class_loader,
                            strict_types,
                        );
                    }
                    _ => {}
                }

                // `null` alone failing means every non-null possibility
                // already satisfies the parameter: a nullability gap rather
                // than a type gap. Projects can opt into treating that
                // narrower case as a warning.
                let severity = if only_null_unsatisfied && downgrade_nullable_mismatch {
                    DiagnosticSeverity::WARNING
                } else {
                    DiagnosticSeverity::ERROR
                };

                out.push(make_diagnostic(
                    range,
                    severity,
                    TYPE_MISMATCH_ARGUMENT_CODE,
                    message,
                ));
            }
        }
    }
}
