//! Standalone function and `define()` constant extraction.
//!
//! This module handles extracting standalone (non-method) function
//! definitions and `define('NAME', value)` constant declarations from
//! the PHP AST.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::{AtomMap, atom_bytes, bytes_to_str, literal_bytes_to_str};
use crate::docblock;
use crate::types::*;

use super::signature::{Signature, extract_signature};
use super::{
    DocblockCtx, for_each_nested_statement_list, is_removed_for_version, merge_deprecation_info,
};

/// Whether a function or method body calls `func_get_args()`,
/// `func_get_arg()` or `func_num_args()`, which makes it accept more
/// arguments than it declares.
///
/// Nested closures, arrow functions, classes and named functions have their
/// own argument list, so their calls do not count.
pub(super) fn body_reads_func_args<'a>(
    statements: impl Iterator<Item = &'a Statement<'a>>,
) -> bool {
    struct Walker;
    impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, bool> for Walker {
        fn walk_in_function_call(&self, call: &'ast FunctionCall<'arena>, found: &mut bool) {
            if let Expression::Identifier(ident) = call.function {
                let name = bytes_to_str(ident.value());
                let name = name.strip_prefix('\\').unwrap_or(name);
                *found |= ["func_get_args", "func_get_arg", "func_num_args"]
                    .iter()
                    .any(|f| name.eq_ignore_ascii_case(f));
            }
        }
        fn walk_closure(&self, _: &'ast Closure<'arena>, _: &mut bool) {}
        fn walk_arrow_function(&self, _: &'ast ArrowFunction<'arena>, _: &mut bool) {}
        fn walk_anonymous_class(&self, _: &'ast AnonymousClass<'arena>, _: &mut bool) {}
        fn walk_function(&self, _: &'ast Function<'arena>, _: &mut bool) {}
        fn walk_class(&self, _: &'ast Class<'arena>, _: &mut bool) {}
    }
    let mut found = false;
    for stmt in statements {
        mago_syntax::walker::Walker::walk_statement(&Walker, stmt, &mut found);
    }
    found
}

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
        let name = match lit_str.value {
            Some(bytes) => literal_bytes_to_str(bytes)?,
            None => crate::text_scan::unquote_php_string(bytes_to_str(lit_str.raw))?,
        };
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// The indexes in a function list of the declarations of each name, keyed
/// by the lowercased name and the namespace.
type OverloadIndex = HashMap<(String, Option<String>), Vec<usize>>;

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
        let mut by_name = OverloadIndex::default();
        for (index, func) in functions.iter().enumerate() {
            by_name
                .entry((func.name.to_ascii_lowercase(), func.namespace.clone()))
                .or_default()
                .push(index);
        }
        Self::extract_functions_into(
            statements,
            functions,
            &mut by_name,
            current_namespace,
            doc_ctx,
        );
    }

    /// [`extract_functions_from_statements`](Self::extract_functions_from_statements)
    /// with `by_name` indexing what `functions` already holds, so finding
    /// the declaration an overload merges into does not scan every
    /// function a stub file of thousands declared before it.
    fn extract_functions_into<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        functions: &mut Vec<FunctionInfo>,
        by_name: &mut OverloadIndex,
        current_namespace: &Option<String>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        for statement in statements {
            match statement {
                Statement::Function(func) => {
                    let Some(Signature {
                        mut parameters,
                        native_return_type,
                        return_type,
                        conditional_return,
                        docblock: info,
                        template_params: func_template_params,
                        template_param_bounds: func_template_param_bounds,
                        template_param_defaults: func_template_param_defaults,
                        template_bindings: func_template_bindings,
                        ..
                    }) = extract_signature(
                        func,
                        &func.attribute_lists,
                        &func.parameter_list,
                        func.return_type_hint.as_ref(),
                        doc_ctx,
                        &[],
                        &AtomMap::default(),
                    )
                    else {
                        continue;
                    };

                    let name = atom_bytes(func.name.value);
                    let name_offset = func.name.span.start.offset;

                    let type_assertions = info
                        .as_ref()
                        .map(docblock::extract_type_assertions_from_info)
                        .unwrap_or_default();

                    let depr_info = merge_deprecation_info(
                        info.as_ref()
                            .and_then(docblock::extract_deprecation_message_from_info),
                        &func.attribute_lists,
                        doc_ctx,
                    );

                    let description = info
                        .as_ref()
                        .and_then(crate::hover::extract_description_from_info);

                    let return_description = info
                        .as_ref()
                        .and_then(docblock::extract_return_description_from_info);

                    let link_urls = info
                        .as_ref()
                        .map(docblock::extract_link_urls_from_info)
                        .unwrap_or_default();

                    let see_refs = info
                        .as_ref()
                        .map(docblock::extract_see_refs_from_info)
                        .unwrap_or_default();

                    let throws = info
                        .as_ref()
                        .map(docblock::extract_throws_tags_from_info)
                        .unwrap_or_default();

                    // Merge `@param` docblock types into parameter type
                    // hints and populate per-parameter descriptions.
                    if let Some(ref info) = info {
                        docblock::merge_param_docblock_into_parameters(
                            info,
                            &mut parameters,
                            &func_template_param_bounds,
                        );
                    }

                    // A docblock `@param` merge above may have overwritten
                    // `type_hint` with a non-nullable docblock type. Re-fold
                    // null for parameters whose default value is `null`.
                    for param in &mut parameters {
                        param.apply_null_default(|name| {
                            func_template_params.iter().any(|t| t == name)
                        });
                    }

                    // `@pure` promises the call changes nothing, which is what
                    // lets a check recorded about an argument survive it;
                    // `@impure` promises it changes something even though it
                    // returns a value.
                    let is_pure = info.as_ref().is_some_and(docblock::declares_pure);
                    let is_impure = info.as_ref().is_some_and(docblock::declares_impure);

                    let reads_func_args = body_reads_func_args(func.body.statements.iter());

                    // Merge overloaded function declarations: when a
                    // function with the same name already exists (common
                    // in phpstorm-stubs for functions like `strtr`,
                    // `implode`, `array_keys`), add the new parameter
                    // list as an overload instead of pushing a duplicate.
                    let same_name = by_name
                        .entry((name.to_ascii_lowercase(), current_namespace.clone()))
                        .or_default();
                    // Only merge when parameter counts differ — true
                    // overloads (strtr, implode) always differ in arity.
                    // Same-name functions with the same param count are
                    // version variants, not overloads.
                    if let Some(&existing) = same_name
                        .iter()
                        .find(|&&i| functions[i].parameters.len() != parameters.len())
                    {
                        functions[existing].overloads.push(parameters);
                    } else {
                        same_name.push(functions.len());
                        functions.push(FunctionInfo {
                            name,
                            name_offset,
                            parameters: parameters.into(),
                            native_return_type,
                            return_type,
                            description,
                            return_description,
                            links: link_urls,
                            see_refs,
                            namespace: current_namespace.clone(),
                            conditional_return,
                            type_assertions,
                            deprecation_message: depr_info.message,
                            deprecated_replacement: depr_info.replacement,
                            template_params: func_template_params,
                            template_param_bounds: func_template_param_bounds,
                            template_bindings: func_template_bindings,
                            template_param_defaults: func_template_param_defaults,
                            throws,
                            is_polyfill: false,
                            overloads: Vec::new(),
                            is_pure,
                            is_impure,
                            reads_func_args,
                            is_no_discard: super::has_no_discard_attribute(
                                &func.attribute_lists,
                                doc_ctx,
                            ),
                            unverified_doc_types: Box::default(),
                        });
                    }
                }
                Statement::Namespace(namespace) => {
                    // A `namespace { }` block with no name is the global
                    // namespace, whatever block came before it.
                    let ns_name = namespace
                        .name
                        .as_ref()
                        .map(|ident| bytes_to_str(ident.value()).to_string())
                        .filter(|s| !s.is_empty());

                    Self::extract_functions_into(
                        namespace.statements().iter(),
                        functions,
                        by_name,
                        &ns_name,
                        doc_ctx,
                    );
                }
                // Recurse into block statements `{ ... }` to find nested
                // function declarations.
                Statement::Block(block) => {
                    Self::extract_functions_into(
                        block.statements.iter(),
                        functions,
                        by_name,
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
                    for_each_nested_statement_list(statement, |nested| {
                        Self::extract_functions_into(
                            nested.iter(),
                            functions,
                            by_name,
                            current_namespace,
                            doc_ctx,
                        );
                    });
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
    /// `namespace` is the namespace the statements sit in, which the walk
    /// updates as it descends into `Statement::Namespace`.  A `const`
    /// declaration is recorded under its fully-qualified name, matching
    /// PHP: `const FOO` inside `namespace App` declares `App\FOO`, not a
    /// global `FOO`.  A `define()` call is not namespaced — the name it
    /// declares is exactly the string it was given, wherever the call
    /// sits.
    ///
    /// With a target PHP version in `doc_ctx`, a declaration whose
    /// docblock marks it `@removed` at or before that version is skipped.
    ///
    /// Uses the parsed AST rather than regex, so it piggybacks on the
    /// parse pass that `update_ast` already performs.
    pub(crate) fn extract_defines_from_statements<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        defines: &mut Vec<(String, u32, Option<String>)>,
        content: &str,
        namespace: Option<&str>,
        doc_ctx: Option<&DocblockCtx<'_>>,
    ) {
        for statement in statements {
            match statement {
                Statement::Expression(_) | Statement::Constant(_)
                    if doc_ctx.is_some_and(|ctx| {
                        ctx.php_version
                            .is_some_and(|ver| is_removed_for_version(statement, ctx, ver))
                    }) => {}
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
                        let name = bytes_to_str(item.name.value);
                        let fqn = match namespace {
                            Some(ns) => format!("{ns}\\{name}"),
                            None => name.to_string(),
                        };
                        defines.push((fqn, item.name.span.start.offset, value));
                    }
                }
                Statement::Namespace(ns_stmt) => {
                    let inner = ns_stmt
                        .name
                        .as_ref()
                        .map(|ident| bytes_to_str(ident.value()))
                        .filter(|s| !s.is_empty());
                    Self::extract_defines_from_statements(
                        ns_stmt.statements().iter(),
                        defines,
                        content,
                        inner.or(namespace),
                        doc_ctx,
                    );
                }
                Statement::Block(block) => {
                    Self::extract_defines_from_statements(
                        block.statements.iter(),
                        defines,
                        content,
                        namespace,
                        doc_ctx,
                    );
                }
                Statement::If(_) => {
                    for_each_nested_statement_list(statement, |nested| {
                        Self::extract_defines_from_statements(
                            nested.iter(),
                            defines,
                            content,
                            namespace,
                            doc_ctx,
                        );
                    });
                }
                _ if let Some(body) = crate::parser::class_like_body(statement) => {
                    crate::parser::for_each_concrete_method(body.members, |_, block| {
                        Self::extract_defines_from_statements(
                            block.statements.iter(),
                            defines,
                            content,
                            namespace,
                            doc_ctx,
                        );
                    });
                }
                Statement::Function(func) => {
                    Self::extract_defines_from_statements(
                        func.body.statements.iter(),
                        defines,
                        content,
                        namespace,
                        doc_ctx,
                    );
                }
                _ => {}
            }
        }
    }

    /// Try to extract the constant name, byte offset, and value from a
    /// `define('NAME', value)` call expression.  Returns
    /// `Some((name, name_offset, value_text))` if the expression is a
    /// function call to `define` whose first argument is a string literal.
    ///
    /// The offset points at the name inside the quotes, the way a `const`
    /// declaration's points at its identifier, so go-to-definition and the
    /// symbol outlines land on the name rather than on `define`.
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
                && let Some(name) = lit_str.value.and_then(literal_bytes_to_str)
                && !name.is_empty()
            {
                let offset = lit_str.span.start.offset + 1;
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
                return Some((name.to_string(), offset, value_text));
            }
        }
        None
    }
}
