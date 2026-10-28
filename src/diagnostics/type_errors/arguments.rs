//! The argument type check: resolve every call site's argument types in one
//! walk of the AST, resolve the callable each site invokes, and compare each
//! argument with the parameter it fills.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use mago_span::HasSpan;
use mago_syntax::cst::PartialArgument;
use mago_syntax::cst::argument::Argument;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::literal::Literal;
use mago_syntax::walker::Walker;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::{
    Fqn, ResolvedClassLoader, first_rejected_callable_param, missing_required_shape_keys,
    shape_breaks_list_order, undeclared_shape_keys,
};
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::php_type::{PhpType, TypeKind, is_array_like_name};
use crate::resolution::FileLoaders;
use crate::symbol_map::CallSite;
use crate::type_engine::variable::iterable_elements::resolve_expression_type;
use crate::types::{FileContext, ParameterInfo, ResolvedCallableTarget};

use super::super::helpers::{
    CallTargetCache, FileDiagnosticContext, is_position_independent_call_expression,
    make_diagnostic,
};
use super::{TYPE_MISMATCH_ARGUMENT_CODE, has_strict_types, is_type_compatible};

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
    /// The bounds of the `@template` parameters in scope at the call site.
    /// They have been substituted into each argument type, and a parameter
    /// type bound from those arguments names the same templates, so it
    /// gets the same substitution before the two are compared.
    template_bounds: HashMap<String, PhpType>,
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

/// Argument expressions with their byte ranges, keyed by the call site's
/// `args_start`.
type ArgExpressions<'a> = HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>;

/// Try to register the argument expressions of an [`ArgumentList`] if its
/// `args_start` offset matches one of the call sites we are interested in.
fn try_collect_argument_list<'a>(
    arg_list: &'a mago_syntax::cst::argument::ArgumentList<'a>,
    call_site_starts: &HashSet<u32>,
    result: &mut ArgExpressions<'a>,
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
    result: &mut ArgExpressions<'a>,
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

/// Walker that records the argument expressions of every call site whose
/// `args_start` offset is one we care about (`starts`). The generated
/// traversal visits every argument list in the tree; the two overrides
/// register the ones in `starts`, and default recursion descends into
/// argument values to find nested calls.
struct CallArgWalker<'s> {
    starts: &'s HashSet<u32>,
}

impl<'a, 's> Walker<'a, 'a, ArgExpressions<'a>> for CallArgWalker<'s> {
    fn walk_in_argument_list(
        &self,
        node: &'a mago_syntax::cst::argument::ArgumentList<'a>,
        context: &mut ArgExpressions<'a>,
    ) {
        try_collect_argument_list(node, self.starts, context);
    }

    fn walk_in_partial_argument_list(
        &self,
        node: &'a mago_syntax::cst::argument::PartialArgumentList<'a>,
        context: &mut ArgExpressions<'a>,
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
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_argument_type_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// [`Self::collect_argument_type_diagnostics`] over an already-gathered
    /// [`FileDiagnosticContext`].
    pub(crate) fn collect_argument_type_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let symbol_map = &ctx.symbol_map;
        let file_ctx = &ctx.file;

        // Activate the thread-local parse cache so that every call to
        // `with_parsed_program(content, …)` in the resolution pipeline
        // reuses the same parsed AST instead of re-parsing the file.
        let _parse_guard = with_parse_cache(content);

        // Build the set of call site args_start offsets for the AST walk.
        // Only include sites without argument unpacking.
        let call_site_starts: HashSet<u32> = symbol_map
            .call_sites
            .iter()
            .filter(|cs| is_checked_site(cs))
            .map(|cs| cs.args_start)
            .collect();

        if call_site_starts.is_empty() {
            return;
        }

        let loaders = self.file_loaders(file_ctx);
        let (resolved_map, strict_types) =
            self.resolve_call_arguments(content, &call_site_starts, file_ctx, &loaders);
        let check = ArgumentCheck {
            backend: self,
            uri,
            content,
            class_loader: loaders.resolved_class_loader(),
            strict_types,
            // Read once for the whole file: `config()` clones the config
            // behind a lock, and the flag cannot change mid-pass.
            downgrade_nullable_mismatch: self
                .config()
                .diagnostics
                .downgrade_nullable_argument_mismatch_enabled(),
        };

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
        // One per `namespace` block: the same text can name a different
        // target under another block's imports.
        let call_targets = CallTargetCache::new(file_ctx);

        // ── Walk every call site ────────────────────────────────────
        for call_site in &symbol_map.call_sites {
            if !is_checked_site(&call_site) {
                continue;
            }
            let Some(args) = resolved_map.get(&call_site.args_start) else {
                continue;
            };
            let Some(target) =
                self.resolve_call_site_target(&call_site, content, file_ctx, &call_targets)
            else {
                continue;
            };
            let call = CheckedCall::new(&target, args, check.class_loader);
            check.check_call_site(&call_site, &call, out);
        }
    }

    /// Walk the AST once, collect the argument expressions of the call
    /// sites starting at `call_site_starts`, and resolve their types — all
    /// inside the `with_parsed_program` closure so AST references never
    /// escape the arena lifetime.  Also answers whether the file declares
    /// `strict_types=1`.
    fn resolve_call_arguments(
        &self,
        content: &str,
        call_site_starts: &HashSet<u32>,
        file_ctx: &FileContext,
        loaders: &FileLoaders,
    ) -> (HashMap<u32, ResolvedCallArgs>, bool) {
        let resolved_class_loader = loaders.resolved_class_loader();
        with_parsed_program(content, "type_error_diagnostics", |program, _content| {
            let strict_types = has_strict_types(program);

            // Phase 1: walk the AST and collect raw argument expressions
            // keyed by args_start offset.
            let mut expr_map: ArgExpressions<'_> = HashMap::new();
            let walker = CallArgWalker {
                starts: call_site_starts,
            };
            for stmt in program.statements.iter() {
                walker.walk_statement(stmt, &mut expr_map);
            }

            // Phase 2: resolve types for each collected expression.
            let mut result: HashMap<u32, ResolvedCallArgs> = HashMap::new();
            for (args_start, exprs) in &expr_map {
                let var_ctx = loaders.var_ctx_at(content, *args_start);
                let current_class_info = var_ctx.current_class;

                let template_bounds = super::super::helpers::template_bounds_in_scope(
                    current_class_info,
                    super::super::helpers::method_declared_before(current_class_info, *args_start),
                    resolved_class_loader,
                    &self.resolved_class_cache,
                );

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
                    // A template in scope is known only by its bound, and
                    // must not be looked up as a class of the same name.
                    let ty = if template_bounds.is_empty() {
                        ty
                    } else {
                        ty.substitute(&template_bounds)
                    };
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
                        if let Some(cls) = resolved_class_loader(Fqn::new(name)) {
                            cls.fqn().to_string()
                        } else {
                            name.to_string()
                        }
                    });
                    // Expand @phpstan-type / @psalm-type aliases so
                    // that e.g. `Payload` becomes `array{name: string,
                    // phone: string}` before the compatibility check.
                    let ty = crate::type_engine::type_ops::aliases::resolve_type_alias_typed(
                        &ty,
                        &current_class_info.fqn(),
                        &file_ctx.classes,
                        resolved_class_loader,
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
                        template_bounds,
                    },
                );
            }
            (result, strict_types)
        })
    }

    /// Resolve the callable a call site invokes.
    ///
    /// Class-level template substitution happens inside
    /// `resolve_callable_target` (driven by the variable's generic type
    /// string); method-level template substitution uses the site's
    /// argument text.  So a site is resolved on its own when its target
    /// depends on where it is (a receiver variable's assigned type, or the
    /// enclosing class behind `self::`/`static::`/`parent::`) or when it
    /// passes arguments, and only a position-independent call without
    /// arguments goes through the per-file cache.
    fn resolve_call_site_target(
        &self,
        call_site: &CallSite,
        content: &str,
        file_ctx: &FileContext,
        call_targets: &CallTargetCache,
    ) -> Option<ResolvedCallableTarget> {
        let expr = call_site.call_expression;
        // The raw argument text, from which method-level @template
        // parameters are resolved.
        let call_args_text = content
            .get(call_site.args_start as usize..call_site.args_end as usize)
            .map(str::trim)
            .filter(|text| !text.is_empty());

        if !is_position_independent_call_expression(expr) || call_args_text.is_some() {
            self.resolve_callable_target_with_args_at_offset(
                expr,
                content,
                call_site.args_start,
                file_ctx,
                call_args_text,
            )
        } else {
            call_targets.target(self, expr, content, call_site.args_start, file_ctx)
        }
    }
}

/// Whether the argument check looks at a call site.  Calls with argument
/// unpacking are skipped: the types of the individual arguments are
/// unknown.
fn is_checked_site(call_site: &CallSite) -> bool {
    !call_site.has_unpacking && super::super::incremental::reports_site(call_site.args_start)
}

/// The parameter argument `arg_idx` fills: by name for a named argument,
/// else the next positional one, counted by `positional_idx`.
fn param_for_argument<'p>(
    call_site: &CallSite,
    params: &'p [ParameterInfo],
    arg_idx: usize,
    positional_idx: &mut usize,
) -> Option<&'p ParameterInfo> {
    let index = arg_idx as u32;
    if call_site.named_arg_indices.contains(&index) {
        let name_pos = call_site
            .named_arg_indices
            .iter()
            .position(|&i| i == index)?;
        let param_name = call_site.named_arg_names.get(name_pos);
        return params
            .iter()
            .find(|p| Some(p.name.trim_start_matches('$')) == param_name);
    }
    let param = params.get(*positional_idx);
    *positional_idx += 1;
    param
}

/// What every argument check in one file shares.
struct ArgumentCheck<'a> {
    backend: &'a Backend,
    uri: &'a str,
    content: &'a str,
    class_loader: &'a ResolvedClassLoader<'a>,
    strict_types: bool,
    downgrade_nullable_mismatch: bool,
}

/// One call site's callee and resolved arguments.
struct CheckedCall<'a> {
    target: &'a ResolvedCallableTarget,
    args: &'a ResolvedCallArgs,
    /// Class that `self` / `static` / `parent` in the signature refer to.
    /// This is the class the callable was resolved on, never the
    /// syntactic prefix of the call expression: in
    /// `$this->state->canChangeTo(…)` the enclosing class is not the one
    /// declaring `canChangeTo`.  `self` on an inherited method is already
    /// bound to its declaring class by the inheritance merge, so anything
    /// still bare here was declared on the owner itself.
    context_class: Option<String>,
    context_parent: Option<String>,
}

impl<'a> CheckedCall<'a> {
    fn new(
        target: &'a ResolvedCallableTarget,
        args: &'a ResolvedCallArgs,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Self {
        let context_class = target.owner_class.map(|fqn| fqn.to_string());
        let context_parent = context_class.as_ref().and_then(|fqn| {
            class_loader(Fqn::new(fqn))
                .and_then(|cls| cls.parent_class.as_ref().map(|p| p.to_string()))
        });
        Self {
            target,
            args,
            context_class,
            context_parent,
        }
    }

    /// `ty` with `self` / `static` / `parent` bound to the class the
    /// callable was resolved on.
    fn bind_relative_class_refs<'t>(&self, ty: &'t PhpType) -> Cow<'t, PhpType> {
        match &self.context_class {
            Some(fqn) if ty.contains_relative_class_ref() => {
                Cow::Owned(ty.resolve_self_refs_bounded(fqn, self.context_parent.as_deref()))
            }
            _ => Cow::Borrowed(ty),
        }
    }
}

impl ArgumentCheck<'_> {
    fn range(&self, start: usize, end: usize) -> Option<Range> {
        self.backend
            .offset_range_to_lsp_range(self.uri, self.content, start, end)
    }

    fn check_call_site(&self, call_site: &CallSite, call: &CheckedCall, out: &mut Vec<Diagnostic>) {
        // Track how many positional args we've seen so far for
        // mapping positional args to parameter indices.
        let mut positional_idx: usize = 0;
        for (arg_idx, arg) in call.args.args.iter().enumerate() {
            // Skip spread arguments.
            if call_site.spread_arg_indices.contains(&(arg_idx as u32)) {
                continue;
            }
            // An argument beyond the declared parameters has none to
            // be checked against.
            let Some(param) = param_for_argument(
                call_site,
                &call.target.parameters,
                arg_idx,
                &mut positional_idx,
            ) else {
                continue;
            };
            self.check_argument(call, arg, arg_idx, positional_idx, param, out);
        }
    }

    /// Check one argument against the parameter it fills.
    /// `positional_idx` counts the positional arguments up to and
    /// including this one.
    fn check_argument(
        &self,
        call: &CheckedCall,
        arg: &ResolvedArg,
        arg_idx: usize,
        positional_idx: usize,
        param: &ParameterInfo,
        out: &mut Vec<Diagnostic>,
    ) {
        // Skip a parameter whose substituted type came solely from
        // resolving this very argument (e.g. PHPUnit's
        // `assertSame(ExpectedType $expected, mixed $actual)`,
        // where `ExpectedType` is bound only by `$expected`).
        // Comparing the argument to its own substitution is
        // circular and can only produce false positives.
        // Its template's `of` bound is still a constraint, so that is
        // what the argument is checked against instead.
        let self_bound_type = match call.target.self_bound_params.get(&param.name) {
            Some(None) => return,
            Some(Some(bound)) => Some(bound),
            None => None,
        };

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
            return;
        }

        // Skip if parameter has no type hint.
        let param_type = match self_bound_type.or(param.type_hint.as_ref()) {
            Some(t) if !t.is_untyped() && !t.is_mixed() => t,
            _ => return,
        };

        // Skip variadic parameters — hard to match individual
        // arg types to the variadic param's inner type.
        if param.is_variadic {
            return;
        }

        let arg_type = &arg.ty;
        // Skip unresolved / empty / Raw("") sentinel types.
        if arg_type.is_untyped()
            || arg_type.is_empty()
            || matches!(arg_type.kind(), TypeKind::Raw(s) if s.is_empty())
        {
            return;
        }

        let effective_param_type = call.bind_relative_class_refs(param_type);
        let effective_param_type = if call.args.template_bounds.is_empty() {
            effective_param_type
        } else {
            Cow::Owned(effective_param_type.substitute(&call.args.template_bounds))
        };
        let effective_param_type = effective_param_type.as_ref();

        if self.check_view_string_argument(arg, effective_param_type, out) {
            return;
        }

        // The compatibility layer treats an array shape as an open
        // description — a key it does not mention may still be
        // there at runtime — which is right for a shape inferred
        // from a variable but not for one written out at the call
        // site. There the keys are all of them, so a required key
        // that is not among them is genuinely absent. A key that
        // *is* there with the wrong value type stays the
        // compatibility layer's call.
        let missing_keys = if arg.enumerates_all_keys {
            missing_required_shape_keys(arg_type, effective_param_type)
        } else {
            Vec::new()
        };

        // Same reasoning for the keys a `list` demands: only a
        // literal that spells every key out proves the value's keys
        // are in an order `array_is_list()` would reject.
        let breaks_list_order =
            arg.enumerates_all_keys && shape_breaks_list_order(arg_type, effective_param_type);

        if missing_keys.is_empty()
            && !breaks_list_order
            && is_type_compatible(
                arg_type,
                effective_param_type,
                self.class_loader,
                self.strict_types,
            )
        {
            self.check_model_property_literals(arg, param_type, out);
            return;
        }

        if self.compatible_with_overload(call, arg_type, positional_idx) {
            return;
        }

        let Some(range) = self.range(arg.start, arg.end) else {
            return;
        };
        let (message, severity) = self.mismatch_message(
            arg_idx,
            &param.name,
            arg_type,
            effective_param_type,
            &missing_keys,
            breaks_list_order,
        );
        out.push(make_diagnostic(
            range,
            severity,
            TYPE_MISMATCH_ARGUMENT_CODE,
            message,
        ));
    }

    /// A `view-string` parameter demands a name some Blade template
    /// answers to, which the compatibility layer cannot judge: the
    /// project's templates settle it, not the type. Report the name
    /// rather than the type, the same way a `view('…')` call naming
    /// nothing is reported, and let anything that is not a literal
    /// through — a variable may well hold a real view name.
    ///
    /// Returns whether the argument was a literal for a `view-string`
    /// parameter, which leaves nothing else to check.
    fn check_view_string_argument(
        &self,
        arg: &ResolvedArg,
        param_type: &PhpType,
        out: &mut Vec<Diagnostic>,
    ) -> bool {
        if !param_type.is_view_string() {
            return false;
        }
        let TypeKind::Literal(lit) = arg.ty.kind() else {
            return false;
        };
        let Some(view) = lit.string_content() else {
            return false;
        };
        if self.backend.view_string_literal_is_unknown(&view)
            && let Some(range) = self.range(arg.start, arg.end)
        {
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::Warning,
                "invalid_laravel_view",
                format!("Unknown view: '{view}'"),
            ));
        }
        true
    }

    /// Even when the array types are compatible, validate string literals
    /// against `model-property<Model>` when the param type is an array
    /// with `model-property` in a generic position.
    fn check_model_property_literals(
        &self,
        arg: &ResolvedArg,
        param_type: &PhpType,
        out: &mut Vec<Diagnostic>,
    ) {
        if arg.array_string_literals.is_empty() {
            return;
        }
        let Some(model_fqn) = extract_model_property_from_array_type(param_type) else {
            return;
        };
        let Some(cls) = (self.class_loader)(Fqn::new(&model_fqn)) else {
            return;
        };
        let resolved = crate::virtual_members::resolve_class_fully_cached(
            &cls,
            self.class_loader,
            &self.backend.resolved_class_cache,
        );
        for (lit, lit_start, lit_end) in &arg.array_string_literals {
            let found = resolved.properties.iter().any(|p| p.name == lit.as_str());
            if !found && let Some(range) = self.range(*lit_start, *lit_end) {
                out.push(make_diagnostic(
                    range,
                    DiagnosticSeverity::Error,
                    TYPE_MISMATCH_ARGUMENT_CODE,
                    format!(
                        "'{}' is not a known property of {}",
                        lit,
                        crate::util::short_name(&model_fqn),
                    ),
                ));
            }
        }
    }

    /// When the function has overloaded signatures, whether the argument
    /// fits the same positional parameter of one of them: the diagnostic
    /// is only emitted when every signature rejects it.
    fn compatible_with_overload(
        &self,
        call: &CheckedCall,
        arg_type: &PhpType,
        positional_idx: usize,
    ) -> bool {
        call.target.overloads.iter().any(|alt_params| {
            // An overload with fewer params has no parameter for this
            // argument, so it is not relevant for this check.
            let Some(alt_param) = alt_params.get(positional_idx.saturating_sub(1)) else {
                return false;
            };
            match &alt_param.type_hint {
                Some(alt_type) if !alt_type.is_untyped() && !alt_type.is_mixed() => {
                    is_type_compatible(
                        arg_type,
                        &call.bind_relative_class_refs(alt_type),
                        self.class_loader,
                        self.strict_types,
                    )
                }
                // No type hint on the alternative parameter accepts anything.
                _ => true,
            }
        })
    }

    /// The mismatch message for an argument, and its severity.
    fn mismatch_message(
        &self,
        arg_idx: usize,
        param_name: &str,
        arg_type: &PhpType,
        param_type: &PhpType,
        missing_keys: &[String],
        breaks_list_order: bool,
    ) -> (String, DiagnosticSeverity) {
        let compatible = |arg: &PhpType, param: &PhpType| {
            is_type_compatible(arg, param, self.class_loader, self.strict_types)
        };
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
            param_type.conditionals_as_branch_unions(),
            arg_type,
        );
        // Two signatures printed side by side leave the reader to
        // diff them by eye, so name the half that disagrees.  The return
        // type is judged as the compatibility check judges it, without the
        // `strict_types` coercions.
        if let (TypeKind::Callable(arg_sig), TypeKind::Callable(param_sig)) =
            (arg_type.kind(), param_type.kind())
        {
            if let (Some(arg_return), Some(param_return)) =
                (&arg_sig.return_type, &param_sig.return_type)
                && !is_type_compatible(arg_return, param_return, self.class_loader, true)
            {
                message.push_str(&format!(
                    " (return type {arg_return} does not satisfy {param_return})"
                ));
            } else if let Some((pos, passed, accepts)) = first_rejected_callable_param(
                arg_sig,
                param_sig,
                self.class_loader,
                self.strict_types,
            ) {
                message.push_str(&format!(
                    " (parameter {} accepts {accepts}, but is passed {passed})",
                    pos + 1
                ));
            }
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
        let undeclared_keys = undeclared_shape_keys(arg_type, param_type, self.class_loader);
        if !undeclared_keys.is_empty() {
            message.push_str(&format!(
                " (undeclared key{} {})",
                if undeclared_keys.len() == 1 { "" } else { "s" },
                undeclared_keys
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
                    .filter(|m| !compatible(m, param_type))
                    .collect();
                if !unsatisfied.is_empty() && unsatisfied.len() < members.len() {
                    message.push_str(&format!(
                        " ({} does not satisfy {})",
                        unsatisfied
                            .iter()
                            .map(|m| m.to_string())
                            .collect::<Vec<_>>()
                            .join("|"),
                        param_type.conditionals_as_branch_unions(),
                    ));
                    only_null_unsatisfied = unsatisfied.iter().all(|m| m.is_null());
                }
            }
            // `?T` is the same gap as `T|null`, spelled shorter, so
            // it has to answer the same way. It carries no union
            // members to name, so only the severity is at stake and
            // the check is worth nothing unless it can change it.
            TypeKind::Nullable(inner) if self.downgrade_nullable_mismatch => {
                only_null_unsatisfied = compatible(inner, param_type);
            }
            _ => {}
        }

        // `null` alone failing means every non-null possibility
        // already satisfies the parameter: a nullability gap rather
        // than a type gap. Projects can opt into treating that
        // narrower case as a warning.
        let severity = if only_null_unsatisfied && self.downgrade_nullable_mismatch {
            DiagnosticSeverity::Warning
        } else {
            DiagnosticSeverity::Error
        };
        (message, severity)
    }
}
