/// Standalone function and `define()` constant extraction.
///
/// This module handles extracting standalone (non-method) function
/// definitions and `define('NAME', value)` constant declarations from
/// the PHP AST.
use mago_span::HasSpan;
use mago_syntax::ast::*;

use crate::Backend;
use crate::atom::{atom, atom_bytes, bytes_to_str};
use crate::docblock;
use crate::types::*;

use super::{
    DocblockCtx, extract_hint_type, extract_parameters, is_available_for_version,
    is_removed_for_version, merge_deprecation_info,
};

/// Try to extract the guarded function name from a
/// `if (! function_exists('name'))` condition.
///
/// Recognises both `! function_exists('name')` and
/// `! function_exists("name")` (with optional parenthesised wrapping).
/// Returns `Some("name")` when the pattern matches, `None` otherwise.
fn try_extract_function_exists_guard<'a>(condition: &'a Expression<'a>) -> Option<&'a str> {
    // Peel parentheses and a single `!` prefix.
    let inner = match condition {
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => prefix.operand,
        Expression::Parenthesized(p) => match p.expression {
            Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => prefix.operand,
            _ => return None,
        },
        _ => return None,
    };

    // Peel one more layer of parentheses (e.g. `!(function_exists(…))`)
    let inner = match inner {
        Expression::Parenthesized(p) => p.expression,
        other => other,
    };

    // Must be a function call to `function_exists`.
    let func_call = match inner {
        Expression::Call(Call::Function(fc)) => fc,
        _ => return None,
    };
    let func_name = match func_call.function {
        Expression::Identifier(ident) => ident.value(),
        _ => return None,
    };
    if func_name != b"function_exists".as_slice() {
        return None;
    }

    // First argument must be a string literal.
    let first_arg = func_call.argument_list.arguments.iter().next()?;
    let first_expr = match first_arg {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };
    if let Expression::Literal(Literal::String(lit_str)) = first_expr {
        // `value` is the unquoted content; fall back to stripping quotes
        // from `raw`.
        let name = lit_str
            .value
            .map(bytes_to_str)
            .or_else(|| crate::util::unquote_php_string(bytes_to_str(lit_str.raw)))?;
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

impl Backend {
    /// Extract standalone function definitions from a sequence of statements.
    ///
    /// Recurses into `Statement::Namespace` blocks, passing the namespace
    /// name down so that each `FunctionInfo` records which namespace it
    /// belongs to (if any).
    pub(crate) fn extract_functions_from_statements<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        functions: &mut Vec<FunctionInfo>,
        current_namespace: &Option<String>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        for statement in statements {
            match statement {
                Statement::Function(func) => {
                    // Skip functions whose #[PhpStormStubsElementAvailable]
                    // range excludes the target PHP version.
                    if let Some(ctx) = doc_ctx
                        && let Some(ver) = ctx.php_version
                        && !is_available_for_version(&func.attribute_lists, ctx, ver)
                    {
                        continue;
                    }

                    // Skip functions whose docblock has `@removed X.Y`
                    // where X.Y <= the target PHP version.
                    if let Some(ctx) = doc_ctx
                        && let Some(ver) = ctx.php_version
                        && is_removed_for_version(func, ctx, ver)
                    {
                        continue;
                    }

                    let name = atom_bytes(func.name.value);
                    let name_offset = func.name.span.start.offset;
                    let php_version = doc_ctx.and_then(|ctx| ctx.php_version);
                    let mut parameters = extract_parameters(
                        &func.parameter_list,
                        doc_ctx.map(|ctx| ctx.content),
                        php_version,
                        doc_ctx,
                    );
                    let raw_native_return_type = func
                        .return_type_hint
                        .as_ref()
                        .map(|rth| extract_hint_type(&rth.hint));

                    // Check for a #[LanguageLevelTypeAware] override on the
                    // function's return type.  When present, it replaces the
                    // native type hint with the version-appropriate string.
                    let lang_level_return = if let Some(ctx) = doc_ctx
                        && let Some(ver) = ctx.php_version
                    {
                        super::extract_language_level_type(&func.attribute_lists, ctx, ver)
                    } else {
                        None
                    };
                    // The `#[LanguageLevelTypeAware]` attribute is JetBrains'
                    // authoritative, version-resolved return type.  When it is
                    // present the accompanying `@return` docblock is legacy
                    // text (e.g. `resource|false` left over from before the
                    // handle→object migration) and must not be allowed to
                    // widen the attribute type back to the old value.
                    let return_from_lang_level = lang_level_return.is_some();
                    let native_return_type = lang_level_return.or(raw_native_return_type);

                    // Apply PHPDoc `@return` override for the function.
                    // Also extract PHPStan conditional return types,
                    // type assertion annotations, and `@deprecated` if present.

                    // Parse the docblock once — used by both the
                    // return-type / metadata extraction and the @param
                    // merge loop below.
                    let docblock_text = doc_ctx.and_then(|ctx| {
                        docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, func)
                    });
                    let info = docblock_text.and_then(docblock::parse_docblock_for_tags);

                    let (
                        return_type,
                        conditional_return,
                        type_assertions,
                        deprecation_message,
                        deprecated_replacement,
                        description,
                        return_description,
                        link_urls,
                        see_refs,
                        func_template_params,
                        func_template_param_bounds,
                        func_template_bindings,
                        throws,
                    ) = if let Some(ctx) = doc_ctx {
                        let parsed_doc_return = info
                            .as_ref()
                            .and_then(docblock::extract_return_type_from_info);

                        let effective = if return_from_lang_level {
                            native_return_type.clone()
                        } else {
                            docblock::resolve_effective_type_typed(
                                native_return_type.as_ref(),
                                parsed_doc_return.as_ref(),
                            )
                        };

                        // If the effective return type is bare `array` (or
                        // nullable/union containing array) and the function
                        // has a `#[ArrayShape]` attribute, replace it with
                        // the more specific shape type.
                        let effective = effective.map(|ty| {
                            super::apply_array_shape_override(ty, &func.attribute_lists, ctx)
                        });

                        let conditional = info
                            .as_ref()
                            .and_then(docblock::extract_conditional_return_type_from_info);

                        // Extract function-level @template params and their
                        // @param bindings for generic type substitution at
                        // call sites.
                        let params_full = info
                            .as_ref()
                            .map(docblock::extract_template_params_full_from_info)
                            .unwrap_or_default();
                        let tpl_params: Vec<String> =
                            params_full.iter().map(|(n, _, _, _)| n.clone()).collect();
                        let tpl_param_bounds: crate::atom::AtomMap<crate::php_type::PhpType> =
                            params_full
                                .iter()
                                .filter_map(|(name, bound, _, _)| {
                                    bound.as_ref().map(|b| (atom(name), b.clone()))
                                })
                                .collect();
                        let tpl_bindings: Vec<(crate::atom::Atom, crate::atom::Atom)> =
                            if !tpl_params.is_empty() {
                                info.as_ref()
                                    .map(|i| {
                                        docblock::extract_template_param_bindings_from_info(
                                            i,
                                            &tpl_params,
                                        )
                                        .into_iter()
                                        .map(|(a, b)| (atom(&a), atom(&b)))
                                        .collect()
                                    })
                                    .unwrap_or_default()
                            } else {
                                Vec::new()
                            };

                        // If no explicit conditional return type was found,
                        // try to synthesize one from function-level @template
                        // annotations.  For example:
                        //   @template T
                        //   @param class-string<T> $class
                        //   @return T
                        // becomes a conditional that resolves T from the
                        // call-site argument (e.g. resolve(User::class) → User).
                        let conditional = conditional.or_else(|| {
                            docblock::synthesize_template_conditional_from_info(
                                info.as_ref()?,
                                &tpl_params,
                                effective.as_ref(),
                                false,
                            )
                        });

                        let assertions = info
                            .as_ref()
                            .map(docblock::extract_type_assertions_from_info)
                            .unwrap_or_default();

                        let depr_info = merge_deprecation_info(
                            info.as_ref()
                                .and_then(docblock::extract_deprecation_message_from_info),
                            &func.attribute_lists,
                            Some(ctx),
                        );
                        let deprecation_message = depr_info.message;
                        let deprecated_replacement = depr_info.replacement;

                        let desc = info
                            .as_ref()
                            .and_then(crate::hover::extract_description_from_info);

                        let ret_desc = info
                            .as_ref()
                            .and_then(docblock::extract_return_description_from_info);

                        let link_urls = info
                            .as_ref()
                            .map(docblock::extract_link_urls_from_info)
                            .unwrap_or_default();

                        let see_refs = info
                            .as_ref()
                            .map(docblock::extract_see_references_from_info)
                            .unwrap_or_default();

                        let throws = info
                            .as_ref()
                            .map(docblock::extract_throws_tags_from_info)
                            .unwrap_or_default();

                        (
                            effective,
                            conditional,
                            assertions,
                            deprecation_message,
                            deprecated_replacement,
                            desc,
                            ret_desc,
                            link_urls,
                            see_refs,
                            tpl_params,
                            tpl_param_bounds,
                            tpl_bindings,
                            throws,
                        )
                    } else {
                        // No docblock context available — attribute argument
                        // strings cannot be read without source text, so we
                        // skip #[Deprecated] extraction here.  In practice
                        // `doc_ctx` is always `Some` for real file parsing.
                        (
                            native_return_type.clone(),
                            None,
                            Vec::new(),
                            None,
                            None,
                            None,
                            None,
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            crate::atom::AtomMap::<crate::php_type::PhpType>::default(),
                            Vec::new(),
                            Vec::new(),
                        )
                    };

                    // Merge `@param` docblock types into parameter type
                    // hints and populate per-parameter descriptions.
                    if let Some(ref info) = info {
                        for param in &mut parameters {
                            let param_doc_type =
                                docblock::extract_param_raw_type_from_info(info, &param.name);
                            if let Some(ref doc_type) = param_doc_type {
                                let effective = docblock::resolve_effective_type_typed(
                                    param.type_hint.as_ref(),
                                    Some(doc_type),
                                );
                                if effective.is_some() {
                                    param.type_hint = effective;
                                }
                            }
                            param.description =
                                docblock::extract_param_description_from_info(info, &param.name);
                        }

                        // Positional fallback for `@param` tags that omit
                        // the parameter name (common in phpstorm-stubs,
                        // e.g. `@param callable(TValue, TKey): bool`
                        // without `$callback`).  When the name-based merge
                        // above didn't enrich a parameter's type hint, try
                        // matching unnamed `@param` tags by position.
                        let positional_tags =
                            docblock::extract_param_types_positional_from_info(info);
                        for (idx, param) in parameters.iter_mut().enumerate() {
                            // Skip parameters that already got a richer
                            // docblock type from the name-based merge.
                            let already_enriched =
                                docblock::extract_param_raw_type_from_info(info, &param.name)
                                    .is_some();
                            if already_enriched {
                                continue;
                            }
                            // Find the positional @param tag at this index.
                            if let Some((None, doc_type)) = positional_tags.get(idx) {
                                let effective = docblock::resolve_effective_type_typed(
                                    param.type_hint.as_ref(),
                                    Some(doc_type),
                                );
                                if effective.is_some() {
                                    param.type_hint = effective;
                                }
                            }
                        }

                        // Populate `closure_this_type` from
                        // `@param-closure-this` tags so that `$this`
                        // inside a closure argument resolves to the
                        // declared type instead of the lexical class.
                        for (this_type, param_name) in
                            docblock::extract_param_closure_this_from_info(info)
                        {
                            if let Some(param) =
                                parameters.iter_mut().find(|p| p.name == param_name)
                            {
                                param.closure_this_type = Some(this_type);
                            }
                        }

                        // Append extra `@param` tags that don't match any
                        // native parameter.  These document parameters
                        // accessed via `func_get_args()` or similar
                        // mechanisms and should appear in hover/signature.
                        for (tag_name, tag_type) in docblock::extract_all_param_tags_from_info(info)
                        {
                            if !parameters.iter().any(|p| p.name == tag_name) {
                                let description =
                                    docblock::extract_param_description_from_info(info, &tag_name);
                                parameters.push(ParameterInfo {
                                    name: atom(&tag_name),
                                    is_required: false,
                                    type_hint: Some(tag_type),
                                    native_type_hint: None,
                                    description,
                                    default_value: None,
                                    is_variadic: false,
                                    is_reference: false,
                                    closure_this_type: None,
                                });
                            }
                        }
                    }

                    let func_tpl_atoms: Vec<crate::atom::Atom> =
                        func_template_params.iter().map(|s| atom(s)).collect();
                    // Merge overloaded function declarations: when a
                    // function with the same name already exists (common
                    // in phpstorm-stubs for functions like `strtr`,
                    // `implode`, `array_keys`), add the new parameter
                    // list as an overload instead of pushing a duplicate.
                    if let Some(existing) = functions.iter_mut().find(|f| {
                        f.name.eq_ignore_ascii_case(&name)
                                && f.namespace == *current_namespace
                                // Only merge when parameter counts differ —
                                // true overloads (strtr, implode) always
                                // differ in arity.  Same-name functions
                                // with the same param count are version
                                // variants, not overloads.
                                && f.parameters.len() != parameters.len()
                    }) {
                        existing.overloads.push(parameters);
                    } else {
                        functions.push(FunctionInfo {
                            name,
                            name_offset,
                            parameters,
                            native_return_type,
                            return_type,
                            description,
                            return_description,
                            links: link_urls,
                            see_refs,
                            namespace: current_namespace.clone(),
                            conditional_return,
                            type_assertions,
                            deprecation_message,
                            deprecated_replacement,
                            template_params: func_tpl_atoms,
                            template_param_bounds: func_template_param_bounds,
                            template_bindings: func_template_bindings,
                            throws,
                            is_polyfill: false,
                            overloads: Vec::new(),
                        });
                    }
                }
                Statement::Namespace(namespace) => {
                    let ns_name = namespace
                        .name
                        .as_ref()
                        .map(|ident| bytes_to_str(ident.value()).to_string())
                        .filter(|s| !s.is_empty());

                    // Merge: if we already have a namespace and the inner
                    // one is set, use the inner one; otherwise keep current.
                    let effective_ns = ns_name.or_else(|| current_namespace.clone());

                    Self::extract_functions_from_statements(
                        namespace.statements().iter(),
                        functions,
                        &effective_ns,
                        doc_ctx,
                    );
                }
                // Recurse into block statements `{ ... }` to find nested
                // function declarations.
                Statement::Block(block) => {
                    Self::extract_functions_from_statements(
                        block.statements.iter(),
                        functions,
                        current_namespace,
                        doc_ctx,
                    );
                }
                // Recurse into `if` bodies — this is critical for the very
                // common PHP pattern:
                //   if (! function_exists('session')) {
                //       function session(...) { ... }
                //   }
                // When the condition matches the
                // `! function_exists('name')` pattern, all functions
                // extracted from the body are marked as polyfills so
                // that callers can prefer native stubs when available.
                Statement::If(if_stmt) => {
                    let guard_name = try_extract_function_exists_guard(if_stmt.condition);
                    let before = functions.len();
                    Self::extract_functions_from_if_body(
                        &if_stmt.body,
                        functions,
                        current_namespace,
                        doc_ctx,
                    );
                    // Mark newly extracted functions as polyfills when
                    // inside a function_exists guard.
                    if guard_name.is_some() {
                        for func in &mut functions[before..] {
                            func.is_polyfill = true;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Helper: recurse into an `if` statement body to extract function
    /// declarations.  Handles both brace-delimited and colon-delimited
    /// `if` bodies, including `elseif` and `else` branches.
    fn extract_functions_from_if_body<'a>(
        body: &'a IfBody<'a>,
        functions: &mut Vec<FunctionInfo>,
        current_namespace: &Option<String>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        match body {
            IfBody::Statement(body) => {
                Self::extract_functions_from_statements(
                    std::iter::once(body.statement),
                    functions,
                    current_namespace,
                    doc_ctx,
                );
                for else_if in body.else_if_clauses.iter() {
                    Self::extract_functions_from_statements(
                        std::iter::once(else_if.statement),
                        functions,
                        current_namespace,
                        doc_ctx,
                    );
                }
                if let Some(else_clause) = &body.else_clause {
                    Self::extract_functions_from_statements(
                        std::iter::once(else_clause.statement),
                        functions,
                        current_namespace,
                        doc_ctx,
                    );
                }
            }
            IfBody::ColonDelimited(body) => {
                Self::extract_functions_from_statements(
                    body.statements.iter(),
                    functions,
                    current_namespace,
                    doc_ctx,
                );
                for else_if in body.else_if_clauses.iter() {
                    Self::extract_functions_from_statements(
                        else_if.statements.iter(),
                        functions,
                        current_namespace,
                        doc_ctx,
                    );
                }
                if let Some(else_clause) = &body.else_clause {
                    Self::extract_functions_from_statements(
                        else_clause.statements.iter(),
                        functions,
                        current_namespace,
                        doc_ctx,
                    );
                }
            }
        }
    }

    // ─── define() constant extraction ───────────────────────────────

    /// Walk statements and extract constant names from `define()` calls
    /// and top-level `const` statements.
    ///
    /// Handles top-level `define('NAME', value)` calls, as well as those
    /// nested inside namespace blocks, block statements, and `if` guards
    /// (the common `if (!defined('X')) { define('X', …); }` pattern).
    /// Also handles `const FOO = 'bar';` statements at the top level or
    /// inside namespace blocks.
    ///
    /// The `content` parameter is the full source text of the file, used
    /// to extract the initializer value as a string slice.
    ///
    /// Uses the parsed AST rather than regex, so it piggybacks on the
    /// parse pass that `update_ast` already performs.
    pub(crate) fn extract_defines_from_statements<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        defines: &mut Vec<(String, u32, Option<String>)>,
        content: &str,
    ) {
        for statement in statements {
            match statement {
                Statement::Expression(expr_stmt) => {
                    if let Some(entry) =
                        Self::try_extract_define_info(expr_stmt.expression, content)
                    {
                        defines.push(entry);
                    }
                }
                // Handle namespace-level const declarations
                Statement::Constant(const_decl) => {
                    for item in const_decl.items.iter() {
                        let start = item.value.span().start.offset as usize;
                        let end = item.value.span().end.offset as usize;
                        let value = content.get(start..end).map(|s| s.to_string());
                        defines.push((
                            bytes_to_str(item.name.value).to_string(),
                            item.name.span.start.offset,
                            value,
                        ));
                    }
                }
                Statement::Namespace(namespace) => {
                    Self::extract_defines_from_statements(
                        namespace.statements().iter(),
                        defines,
                        content,
                    );
                }
                Statement::Block(block) => {
                    Self::extract_defines_from_statements(
                        block.statements.iter(),
                        defines,
                        content,
                    );
                }
                Statement::If(if_stmt) => {
                    Self::extract_defines_from_if_body(&if_stmt.body, defines, content);
                }
                Statement::Class(class) => {
                    for member in class.members.iter() {
                        if let ClassLikeMember::Method(method) = member
                            && let MethodBody::Concrete(body) = &method.body
                        {
                            Self::extract_defines_from_statements(
                                body.statements.iter(),
                                defines,
                                content,
                            );
                        }
                    }
                }
                Statement::Trait(trait_def) => {
                    for member in trait_def.members.iter() {
                        if let ClassLikeMember::Method(method) = member
                            && let MethodBody::Concrete(body) = &method.body
                        {
                            Self::extract_defines_from_statements(
                                body.statements.iter(),
                                defines,
                                content,
                            );
                        }
                    }
                }
                Statement::Enum(enum_def) => {
                    for member in enum_def.members.iter() {
                        if let ClassLikeMember::Method(method) = member
                            && let MethodBody::Concrete(body) = &method.body
                        {
                            Self::extract_defines_from_statements(
                                body.statements.iter(),
                                defines,
                                content,
                            );
                        }
                    }
                }
                Statement::Function(func) => {
                    Self::extract_defines_from_statements(
                        func.body.statements.iter(),
                        defines,
                        content,
                    );
                }
                _ => {}
            }
        }
    }

    /// Helper: recurse into an `if` statement body to extract `define()`
    /// calls.  Mirrors `extract_functions_from_if_body`.
    fn extract_defines_from_if_body<'a>(
        body: &'a IfBody<'a>,
        defines: &mut Vec<(String, u32, Option<String>)>,
        content: &str,
    ) {
        match body {
            IfBody::Statement(body) => {
                Self::extract_defines_from_statements(
                    std::iter::once(body.statement),
                    defines,
                    content,
                );
                for else_if in body.else_if_clauses.iter() {
                    Self::extract_defines_from_statements(
                        std::iter::once(else_if.statement),
                        defines,
                        content,
                    );
                }
                if let Some(else_clause) = &body.else_clause {
                    Self::extract_defines_from_statements(
                        std::iter::once(else_clause.statement),
                        defines,
                        content,
                    );
                }
            }
            IfBody::ColonDelimited(body) => {
                Self::extract_defines_from_statements(body.statements.iter(), defines, content);
                for else_if in body.else_if_clauses.iter() {
                    Self::extract_defines_from_statements(
                        else_if.statements.iter(),
                        defines,
                        content,
                    );
                }
                if let Some(else_clause) = &body.else_clause {
                    Self::extract_defines_from_statements(
                        else_clause.statements.iter(),
                        defines,
                        content,
                    );
                }
            }
        }
    }

    /// Try to extract the constant name, byte offset, and value from a
    /// `define('NAME', value)` call expression.  Returns
    /// `Some((name, define_keyword_offset, value_text))` if the expression
    /// is a function call to `define` whose first argument is a string literal.
    fn try_extract_define_info(
        expr: &Expression<'_>,
        content: &str,
    ) -> Option<(String, u32, Option<String>)> {
        if let Expression::Call(Call::Function(func_call)) = expr {
            let ident = match func_call.function {
                Expression::Identifier(ident) => ident,
                _ => return None,
            };
            if !ident.value().eq_ignore_ascii_case(b"define") {
                return None;
            }
            let args: Vec<_> = func_call.argument_list.arguments.iter().collect();
            if args.is_empty() {
                return None;
            }
            let first_expr = match &args[0] {
                Argument::Positional(pos) => pos.value,
                Argument::Named(named) => named.value,
            };
            if let Expression::Literal(Literal::String(lit_str)) = first_expr
                && let Some(name) = lit_str.value
                && !name.is_empty()
            {
                let offset = ident.span().start.offset;
                // Extract the value from the second argument if present.
                let value_text = args.get(1).and_then(|arg| {
                    let val_expr = match arg {
                        Argument::Positional(pos) => pos.value,
                        Argument::Named(named) => named.value,
                    };
                    let start = val_expr.span().start.offset as usize;
                    let end = val_expr.span().end.offset as usize;
                    content.get(start..end).map(|s| s.to_string())
                });
                return Some((bytes_to_str(name).to_string(), offset, value_text));
            }
        }
        None
    }
}
